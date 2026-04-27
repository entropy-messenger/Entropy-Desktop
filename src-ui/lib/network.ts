import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { get } from 'svelte/store';
import { userStore } from './stores/user';
import { addMessage, updateMessageStatusUI, updateSingleMessageStatusUI, handleTypingSignal } from './actions/chat';

export class NetworkLayer {
    private url: string = "";
    private isAuthenticated = false;
    private isConnected = false;
    private isManualDisconnect = false;
    private lastWarningTime: Map<string, number> = new Map();

    constructor() {
        // bridge native tauri events to svelte state
        listen('network-status', (event) => {
            const payload = event.payload as any;
            const status = typeof payload === 'string' ? payload : payload.status;

            if (status === 'reconnecting') {
                userStore.update(s => ({ ...s, connectionStatus: 'reconnecting', reconnectTimer: payload.seconds }));
            } else if (status === 'jailed') {
                this.onJailed();
            } else if (status === 'mining') {
                userStore.update(s => ({ ...s, connectionStatus: 'mining' }));
            } else if (status === 'connecting') {
                userStore.update(s => ({ ...s, connectionStatus: 'connecting' }));
            } else if (status === 'disconnected') {
                this.onDisconnect();
            } else if (status === 'authenticated') {
                this.onAuthenticated();
            } else if (status === 'auth_failed') {
                this.onAuthFailed();
            }
        });

        listen('network-warning', async (event) => {
            const { type } = event.payload as any;

            const now = Date.now();
            const last = this.lastWarningTime.get(type) || 0;
            if (now - last < 10000) return;
            this.lastWarningTime.set(type, now);

            const { addToast } = await import('./stores/ui');
            if (type === 'media_offline') {
                addToast("Recipient is offline. Media cannot be sent.", 'warning');
            } else if (type === 'storage_full' || type === 'Mailbox full') {
                addToast("Recipient's offline storage is full (500 limit).", 'error');
            } else if (type === 'sender_quota_exceeded' || type === 'Sender quota exceeded') {
                addToast("You've hit your limit for this user's mailbox (15/15).", 'error');
            } else if (type) {
                addToast(`Relay Error: ${type}`, 'error');
            }
        });

        listen('msg://added', (event) => {
            const m = event.payload as any;
            const state = get(userStore);

            const safeParse = (str: string) => {
                try { return str ? JSON.parse(str) : undefined; } 
                catch (e) { return undefined; }
            };

            const uiMsg: any = {
                ...m,
                isMine: m.senderHash === state.identityHash,
                attachment: safeParse(m.attachmentJson),
                replyTo: safeParse(m.replyToJson)
            };

            addMessage(m.chatAddress, uiMsg);
        });

        listen('msg://status', (event) => {
            const payload = event.payload as any;
            const chatAddress = payload.chatAddress || payload.chat_address;
            if (payload.id) {
                updateSingleMessageStatusUI(payload.id, payload.status, chatAddress);
            } else if (payload.ids) {
                updateMessageStatusUI(chatAddress, payload.ids, payload.status);
            }
        });

        listen('msg://typing', (event) => {
            const { sender, payload } = event.payload as any;
            handleTypingSignal(sender, payload);
        });

        listen('msg://invite', (event) => {
            const { groupId, name, members, lastMsg, lastTimestamp } = event.payload as any;
            const uniqueMembers = Array.from(new Set(((members || []) as string[]).map(m => m.toLowerCase())));
            userStore.update(s => ({
                ...s,
                chats: {
                    ...s.chats,
                    [groupId]: {
                        peerHash: groupId,
                        localNickname: name,
                        peerNickname: name,
                        messages: [],
                        unreadCount: 1,
                        isGroup: true,
                        members: uniqueMembers,
                        lastMsg: lastMsg || `Added to ${name}`,
                        lastTimestamp: lastTimestamp || Date.now()
                    }
                }
            }));
        });

        listen('msg://group_leave', (event) => {
            const { groupId, member } = event.payload as any;
            userStore.update(s => {
                const chat = s.chats[groupId];
                if (chat && chat.isGroup && chat.members) {
                    chat.members = chat.members.filter(m => m !== member);
                    s.chats[groupId] = { ...chat };
                }
                return { ...s, chats: { ...s.chats } };
            });
        });

        listen('msg://group_update', (event) => {
            const { groupId, name, members } = event.payload as any;
            userStore.update(s => {
                const chat = s.chats[groupId];
                if (chat) {
                    if (name) {
                        chat.localNickname = name;
                    }
                    if (members) {
                        chat.members = members;
                    }
                    s.chats[groupId] = { ...chat };
                }
                return { ...s, chats: { ...s.chats } };
            });
        });

        listen('contact-update', (event) => {
            const { hash, alias } = event.payload as any;
            if (!alias) return;

            userStore.update(s => {
                if (s.chats[hash]) {
                    s.chats[hash] = { ...s.chats[hash], globalNickname: alias };
                }
                if (!s.chats[hash]?.localNickname || s.chats[hash]?.isGroup) {
                    s.nicknames[hash] = alias;
                }
                return { ...s, chats: { ...s.chats }, nicknames: { ...s.nicknames } };
            });
        });

        listen('transfer://progress', (event) => {
            const { transferId, current, total, direction, msgId } = event.payload as any;
            import('./stores/transfers').then(m => {
                m.updateTransferProgress(transferId, current, total, direction, undefined, msgId);
                if (current >= total) {
                    setTimeout(() => m.removeTransfer(transferId), 3000);
                }
            });
        });

        listen('network-bin-progress', (event) => {
            const { transfer_id, current, total, sender } = event.payload as any;
            import('./stores/transfers').then(m => {
                m.updateTransferProgress(transfer_id, current, total, 'download', sender);
                if (current >= total) {
                    setTimeout(() => m.removeTransfer(transfer_id), 3000);
                }
            });
        });

        listen('network-bin-complete', (event) => {
            const { msg_id } = event.payload as any;
            if (!msg_id) return;
            
            // Mark the message as ready in the UI store
            import('./actions/chat').then(m => {
                m.refreshMessageUI(msg_id);
            });
        });
    }

    private connectingPromise: Promise<void> | null = null;

    async connect() {
        if (this.isConnected) return;
        if (this.connectingPromise) return this.connectingPromise;

        this.connectingPromise = (async () => {
            // resolve proxy configuration based on privacy settings
            try {
                let proxyUrl = undefined;
                const state = get(userStore) as any;

                if (state.privacySettings.routingMode !== 'direct') {
                    proxyUrl = state.privacySettings.proxyUrl;
                    if (state.privacySettings.routingMode === 'tor') {
                        proxyUrl = 'socks5://127.0.0.1:9050';
                    } else if (state.privacySettings.proxyUrl && !state.privacySettings.proxyUrl.includes('://')) {
                        proxyUrl = `socks5://${state.privacySettings.proxyUrl}`;
                    }
                }

                await invoke('connect_network', { proxyUrl });
                this.onConnect();
            } catch (e: any) {
                const errorStr = e.toString();
                if (errorStr.includes("Proxy connection failed")) {
                    const { addToast } = await import('./stores/ui');
                    addToast("Privacy routing failed. Is Tor/Proxy running?", 'error');
                }
            } finally {
                this.connectingPromise = null;
            }
        })();

        return this.connectingPromise;
    }

    async disconnect() {
        this.isManualDisconnect = true;
        try {
            await invoke('disconnect_network');
        } catch (e) {
            // Native disconnect failed
        }

        this.isConnected = false;
        this.isAuthenticated = false;

        userStore.update(s => ({
            ...s,
            isConnected: false,
            connectionStatus: 'disconnected'
        }));
    }

    async reconnect() {
        await this.disconnect();
        await new Promise(r => setTimeout(r, 600));
        return this.connect();
    }

    private onConnect() {
        userStore.update((s: any) => ({ ...s, isConnected: true }));
    }

    private async onAuthenticated() {
        this.isAuthenticated = true;
        userStore.update((s: any) => ({
            ...s,
            connectionStatus: 'connected',
            isSynced: true
        }));
    }

    private onAuthFailed() {
        this.isAuthenticated = false;
        userStore.update((s: any) => ({ ...s, connectionStatus: 'disconnected' }));
    }

    private async onJailed() {
        this.isAuthenticated = false;
        const { addToast } = await import('./stores/ui');
        addToast("Identity Jailed. Connection suspended for 5m.", 'error');

        userStore.update((s: any) => ({
            ...s,
            connectionStatus: 'jailed',
            jailTimeRemaining: 300
        }));
    }

    private onDisconnect() {
        if (this.isManualDisconnect) {
            this.isManualDisconnect = false;
            this.isConnected = false;
            this.isAuthenticated = false;
            userStore.update(s => ({ ...s, isConnected: false, connectionStatus: 'disconnected' }));
            return;
        }

        this.isConnected = false;
        this.isAuthenticated = false;

        userStore.update((s: any) => {
            return {
                ...s,
                isConnected: false,
                connectionStatus: 'disconnected'
            };
        });
    }
}
export const network = new NetworkLayer();
