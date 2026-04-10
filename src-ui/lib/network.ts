import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { get } from 'svelte/store';
import { userStore } from './stores/user';
import { addMessage, updateMessageStatusUI, updateSingleMessageStatusUI, handleTypingSignal } from './actions/chat';

/**
 * Handles communication with the underlying Rust network node via Tauri's bridge.
 * Manage WebSocket lifecycle, message multiplexing, and binary/JSON serialization.
 */
export class NetworkLayer {
    private url: string = "";
    private isAuthenticated = false;
    private isConnected = false;
    private isManualDisconnect = false;
    private lastWarningTime: Map<string, number> = new Map();



    constructor() {

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

            // 10-second debounce per warning type
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

        // Listen for authoritative message creation from RustW
        listen('msg://added', (event) => {
            const m = event.payload as any;
            const state = get(userStore);

            const uiMsg: any = {
                ...m,
                isMine: m.senderHash === state.identityHash,
                attachment: m.attachmentJson ? JSON.parse(m.attachmentJson) : undefined,
                replyTo: m.replyToJson ? JSON.parse(m.replyToJson) : undefined
            };

            addMessage(m.chatAddress, uiMsg);
        });

        // Listen for message status updates (Confirmed Delivery & Read Receipts)
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



        // Group Handlers
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

                // Update the nickname cache for group-wide resolution
                if (!s.chats[hash]?.localNickname || s.chats[hash]?.isGroup) {
                    s.nicknames[hash] = alias;
                }

                return { ...s, chats: { ...s.chats }, nicknames: { ...s.nicknames } };
            });
        });

    }

    private connectingPromise: Promise<void> | null = null;

    /**
     * Establishes a connection to the relay server.
     * Integrates routing mode preferences (Tor/Proxy) into the bridge command.
     */
    async connect() {
        if (this.isConnected) return;
        if (this.connectingPromise) return this.connectingPromise;

        this.connectingPromise = (async () => {
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

                await invoke('connect_network', {
                    proxyUrl
                });
                // Note: isConnected will be set by the 'network-status' event listener
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

    /**
     * Forcefully terminates the active connection and clears background tasks.
     */
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

    /**
     * Cycles the connection to apply new routing/proxy settings.
     */
    async reconnect() {
        await this.disconnect();
        // Shift context to allow native cleanup
        await new Promise(r => setTimeout(r, 600));
        return this.connect();
    }


    private onConnect() {


        userStore.update((s: any) => ({ ...s, isConnected: true }));

    }

    private async onAuthenticated() {
        this.isAuthenticated = true;

        // 1. Sync connection status
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


        const wasAuthenticated = this.isAuthenticated;
        this.isConnected = false;
        this.isAuthenticated = false;

        userStore.update((s: any) => {
            const newState = {
                ...s,
                isConnected: false,
                connectionStatus: 'disconnected'
            };
            if (!wasAuthenticated) {
                // Re-auth required on next connect
            }
            return newState;
        });

    }

}

export const network = new NetworkLayer();
