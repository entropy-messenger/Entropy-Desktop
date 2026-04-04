import { get, writable } from 'svelte/store';
import { userStore, messageStore } from '../stores/user';
import { network } from '../network';
import { invoke } from '@tauri-apps/api/core';
import type { Message, Chat } from '../types';

/**
 * CACHE & ATTACHMENT ORCHESTRATION
 * Low-level memory cache for media and indexing logic.
 */
const attachmentCache = new Map<string, Uint8Array>();

export const getAttachment = async (id: string): Promise<Uint8Array | null> => {
    if (attachmentCache.has(id)) return attachmentCache.get(id)!;
    try {
        const bytes = await invoke<number[]>('vault_load_media', { id });
        const uint8 = new Uint8Array(bytes);
        attachmentCache.set(id, uint8);
        return uint8;
    } catch (e) {
        return null;
    }
};

/**
 * OUTGOING MESSAGE PIPELINE
 * Orchestrates the flow: UI -> Rust (Encrypt & Send) -> Success
 */
export const setReplyingTo = (msg: Message | null) => userStore.update(s => ({ ...s, replyingTo: msg }));

export const sendMessage = async (destIdRaw: string, content: string) => {
    const destId = destIdRaw.toLowerCase();
    const state = get(userStore);
    if (!state.identityHash || state.blockedHashes.includes(destId)) return;
    const chat = state.chats[destId];

    try {
        await invoke('process_outgoing_text', {
            payload: {
                recipient: destId,
                content,
                replyTo: state.replyingTo ? {
                    id: state.replyingTo.id,
                    content: state.replyingTo.content.length > 200 ? state.replyingTo.content.substring(0, 197) + '...' : state.replyingTo.content,
                    senderHash: state.replyingTo.senderHash,
                    senderAlias: state.replyingTo.senderAlias,
                    type: state.replyingTo.type
                } : null,
                isGroup: !!chat?.isGroup,
                groupMembers: chat?.members || null
            }
        });
        setReplyingTo(null);
    } catch (e) {
        console.error("[Chat] Send failed:", e);
    }
};

export const sendFile = async (destIdRaw: string, file: File | { name: string, type: string, path: string }, type: 'file' | 'voice_note' = 'file', duration?: number) => {
    const destId = destIdRaw.toLowerCase();
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];

    let filePath: string | null = null;
    if ('path' in file) {
        filePath = file.path;
    } else {
        const buffer = await file.arrayBuffer();
        const uint8 = new Uint8Array(buffer);
        // Avoid sending huge JSON number arrays across IPC. 
        // Write to a temporary file via a dedicated command and send the path instead.
        filePath = await invoke<string>('write_temp_media', { 
            name: file.name || 'blob', 
            data: Array.from(uint8) 
        });
    }

    try {
        await invoke('process_outgoing_media', {
            payload: {
                recipient: destId,
                filePath: filePath,
                fileName: file.name,
                fileType: file.type,
                msgType: type,
                duration: duration,
                isGroup: !!chat?.isGroup,
                groupMembers: chat?.members || null,
                replyTo: state.replyingTo ? {
                    id: state.replyingTo.id,
                    content: state.replyingTo.content,
                    senderHash: state.replyingTo.senderHash,
                    senderAlias: state.replyingTo.senderAlias,
                    type: state.replyingTo.type
                } : null
            }
        });
        setReplyingTo(null);
    } catch (e: any) {
        console.error("[Chat] Media send failed:", e);
        const { addToast } = await import('../stores/ui');
        addToast(e.toString(), 'error');
    }
};

/**
 * INCOMING & DATA MANAGEMENT
 * Synchronizes store state with the SQLite database and native notification engine.
 */
export const addMessage = async (peerHash: string, msg: Message) => {
    // 1. Index attachments
    if (msg.attachment?.data && !(msg.isMine && msg.status === 'sending')) {
        attachmentCache.set(msg.id, msg.attachment.data);
        await invoke('vault_save_media', { id: msg.id, data: msg.attachment.data }).catch(() => { });
    }

    let updatedChatMetadata: Chat | null = null;

    // 2. Update Chat Metadata (Brain/Store)
    let isNewChat = false;
    userStore.update(s => {
        if (!s.chats[peerHash]) {
            isNewChat = true;
            s.chats[peerHash] = {
                peerHash,
                peerNickname: peerHash.slice(0, 8),
                unreadCount: 0,
                isGroup: !!msg.groupId,
                trustLevel: 1
            };
        }
        const chat = { ...s.chats[peerHash] };
        chat.lastMsg = msg.content;
        chat.lastTimestamp = msg.timestamp;
        chat.lastStatus = msg.status;
        chat.lastIsMine = msg.isMine;
        chat.lastSenderHash = msg.senderHash;
        chat.isTyping = false;

        if (!msg.isMine && s.activeChatHash !== peerHash) {
            chat.unreadCount = (chat.unreadCount || 0) + 1;
        }

        s.chats[peerHash] = chat;
        updatedChatMetadata = chat;
        return { ...s, chats: { ...s.chats } };
    });

    // 3. Update History (Transient Store) - Keep starred messages immune from slicing!
    messageStore.update(mStore => {
        const msgs = mStore[peerHash] || [];
        if (msgs.some(m => m.id === msg.id)) return mStore;
        
        // Merge and sort
        const combined = [...msgs, msg].sort((a, b) => a.timestamp - b.timestamp);
        
        // Slicing Strategy: Keep ALL starred messages + latest 100 non-starred
        const starredLimit = combined.filter(m => m.isStarred);
        const mostRecent = combined.filter(m => !m.isStarred).slice(-100);
        
        const final = [...starredLimit, ...mostRecent].sort((a, b) => a.timestamp - b.timestamp);
        return { ...mStore, [peerHash]: final };
    });

    // 4. Persistence & Notifications
    if (updatedChatMetadata) {
        syncChatToDb(updatedChatMetadata);
        const s = get(userStore);
        if (!msg.isMine && s.activeChatHash !== peerHash) triggerNativeNotification(updatedChatMetadata, msg);

        // 5. Automated Identity Guard (New & Recurring)
        const now = Date.now();
        const lastCheck = (updatedChatMetadata as any).lastIdentityCheck || 0;
        const REFRESH_INTERVAL = 60 * 60 * 1000; // 1 Hour

        if (!msg.groupId && (isNewChat || (now - lastCheck > REFRESH_INTERVAL))) {
            // Update the check timestamp locally first to prevent triple-firing
            userStore.update(st => {
                if (st.chats[peerHash]) st.chats[peerHash].lastIdentityCheck = now;
                return { ...st, chats: { ...st.chats } };
            });

            import('./contacts').then(({ resolveIdentity }) => {
                resolveIdentity(peerHash);
            });
        }
    }
};

export const syncChatToDb = async (chat: Chat) => {
    const state = get(userStore);
    try {
        await invoke('db_upsert_chat', {
            chat: {
                address: chat.peerHash,
                isGroup: !!chat.isGroup,
                alias: chat.peerNickname || null,
                lastMsg: chat.lastMsg || null,
                lastTimestamp: chat.lastTimestamp || null,
                lastSenderHash: chat.lastSenderHash || null,
                lastStatus: chat.lastStatus || null,
                unreadCount: chat.unreadCount || 0,
                isArchived: !!chat.isArchived,
                isPinned: !!chat.isPinned,
                isBlocked: !!chat.isBlocked,
                members: chat.members || null
            }
        });
    } catch (e) {
        console.error("[Chat] DB sync failed:", e);
    }
};

export const sendReceipt = async (peerHash: string, msgIds: string[], status: 'delivered' | 'read') => {
    const state = get(userStore);
    if (state.blockedHashes.includes(peerHash)) return;
    if (status === 'read' && !state.privacySettings.readReceipts) return;

    try {
        await invoke('send_receipt', { peerHash, msgIds, status });

        // Also update our own local state to reflect that we've read/received these
        messageStore.update(mStore => {
            if (!mStore[peerHash]) return mStore;
            mStore[peerHash] = mStore[peerHash].map(m => msgIds.includes(m.id) ? { ...m, status } : m);
            return { ...mStore };
        });

        if (status === 'read') {
            userStore.update(s => {
                const chat = s.chats[peerHash];
                if (chat) {
                    s.chats[peerHash] = { ...chat, unreadCount: 0 };
                }
                return { ...s, chats: { ...s.chats } };
            });
            // Persist the status to DB
            invoke('db_update_messages', { ids: msgIds, status: 'read' }).catch(console.error);
        }
    } catch (e) {
        console.error("[Chat] Send receipt failed:", e);
    }
};

const triggerNativeNotification = (chat: Chat, msg: Message) => {
    if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
        import('@tauri-apps/plugin-notification').then(({ sendNotification, isPermissionGranted }) => {
            isPermissionGranted().then((granted: boolean) => {
                if (granted) {
                    sendNotification({
                        title: `Message from ${chat.peerNickname || 'Peer'}`,
                        body: msg.content.length > 50 ? msg.content.substring(0, 47) + '...' : msg.content
                    });
                }
            });
        });
    }
};

export const loadChatMessages = async (peerHash: string, limit = 50, offset = 0) => {
    try {
        const state = get(userStore);
        const identityHash = state.identityHash;
        const rawMsgs = await invoke<any[]>('db_get_messages', { chatAddress: peerHash, limit, offset, includeAttachments: true });
        const messages: Message[] = rawMsgs.map(m => ({
            id: m.id,
            timestamp: m.timestamp,
            senderHash: m.senderHash,
            content: m.content,
            type: m.type as any,
            isMine: m.senderHash === identityHash,
            status: m.status as any,
            attachment: m.attachmentJson ? JSON.parse(m.attachmentJson) : undefined,
            isStarred: !!m.isStarred,
            replyTo: m.replyToJson ? JSON.parse(m.replyToJson) : undefined
        }));

        messageStore.update(mStore => {
            const existing = mStore[peerHash] || [];
            
            // Merge newly loaded messages with existing ones
            const combined = [...existing];
            messages.forEach(newMsg => {
                if (!combined.some(m => m.id === newMsg.id)) {
                    combined.push(newMsg);
                }
            });
            
            // Slicing Strategy: Keep ALL starred messages + latest 100 non-starred
            const sorted = combined.sort((a, b) => a.timestamp - b.timestamp);
            const starred = sorted.filter(m => m.isStarred);
            const latestNonStarred = sorted.filter(m => !m.isStarred).slice(-100);
            
            const final = [...starred, ...latestNonStarred].sort((a, b) => a.timestamp - b.timestamp);
            return {
                ...mStore,
                [peerHash]: final
            };
        });

        return messages.length;
    } catch (e) {
        console.error("[Chat] Load failed:", e);
        return 0;
    }
};

export const loadStarredMessages = async () => {
    try {
        const state = get(userStore);
        const identityHash = state.identityHash;
        const rawMsgs = await invoke<any[]>('db_get_starred_messages');
        
        messageStore.update(mStore => {
            const newStore = { ...mStore };
            rawMsgs.forEach(m => {
                const peerHash = m.chatAddress;
                const msg: Message = {
                    id: m.id,
                    timestamp: m.timestamp,
                    senderHash: m.senderHash,
                    content: m.content,
                    type: m.type as any,
                    isMine: m.senderHash === identityHash,
                    status: m.status as any,
                    attachment: m.attachmentJson ? JSON.parse(m.attachmentJson) : undefined,
                    isStarred: !!m.isStarred,
                    replyTo: m.replyToJson ? JSON.parse(m.replyToJson) : undefined
                };
                
                if (!newStore[peerHash]) newStore[peerHash] = [];
                if (!newStore[peerHash].some(existing => existing.id === msg.id)) {
                    newStore[peerHash].push(msg);
                }
            });
            // Resort all updated chats
            Object.keys(newStore).forEach(hash => {
                newStore[hash].sort((a, b) => a.timestamp - b.timestamp);
            });
            return newStore;
        });
        return rawMsgs.length;
    } catch (e) {
        console.error("[Chat] Load starred failed:", e);
        return 0;
    }
};

export const loadMoreMessages = async (peerHash: string) => {
    const msgs = get(messageStore)[peerHash] || [];
    // Calculate offset based on non-starred messages to maintain standard pagination windows
    const nonStarredCount = msgs.filter(m => !m.isStarred).length;
    return await loadChatMessages(peerHash, 50, nonStarredCount);
};

export const deleteMessage = async (peerHash: string, msgIds: string[]) => {
    msgIds.forEach(id => {
        attachmentCache.delete(id);
        invoke('vault_delete_media', { id }).catch(() => { });
    });
    messageStore.update(mStore => {
        if (mStore[peerHash]) mStore[peerHash] = mStore[peerHash].filter(m => !msgIds.includes(m.id));
        return { ...mStore };
    });
    await invoke('db_delete_messages', { ids: msgIds });
};

export const deleteChat = async (peerHash: string) => {
    userStore.update(s => {
        delete s.chats[peerHash];
        if (s.activeChatHash === peerHash) s.activeChatHash = null;
        return { ...s, chats: { ...s.chats } };
    });
    messageStore.update(mStore => {
        delete mStore[peerHash];
        return { ...mStore };
    });
    await invoke('db_delete_chat', { address: peerHash });
};

export const bulkDelete = deleteMessage;

export const bulkStar = async (peerHash: string, msgIds: string[]) => {
    messageStore.update(mStore => {
        if (mStore[peerHash]) {
            mStore[peerHash] = mStore[peerHash].map(m => msgIds.includes(m.id) ? { ...m, isStarred: !m.isStarred } : m);
        }
        return { ...mStore };
    });
    for (const id of msgIds) {
        const msgs = get(messageStore)[peerHash];
        const msg = msgs?.find(m => m.id === id);
        if (msg) invoke('db_update_messages', { ids: [id], isStarred: msg.isStarred }).catch(console.error);
    }
};

// --- SIGNAL HANDLERS ---
export const handleTypingSignal = (senderHash: string, payload: any) => {
    userStore.update(s => {
        if (s.chats[senderHash]) s.chats[senderHash] = { ...s.chats[senderHash], isTyping: payload.isTyping };
        return { ...s, chats: { ...s.chats } };
    });
};

export const updateMessageStatusUI = (peerHash: string, msgIds: string[], status: 'delivered' | 'read') => {
    messageStore.update(mStore => {
        if (!mStore[peerHash]) return mStore;
        mStore[peerHash] = mStore[peerHash].map(m => msgIds.includes(m.id) ? { ...m, status } : m);
        return { ...mStore };
    });
    userStore.update(s => {
        if (s.chats[peerHash]) {
            s.chats[peerHash] = {
                ...s.chats[peerHash],
                lastStatus: status
            };
        }
        return { ...s, chats: { ...s.chats } };
    });
};

export const updateSingleMessageStatusUI = (msgId: string, status: any) => {
    messageStore.update(mStore => {
        for (const peerHash of Object.keys(mStore)) {
            const index = mStore[peerHash].findIndex(m => m.id === msgId);
            if (index !== -1) {
                mStore[peerHash][index] = { ...mStore[peerHash][index], status };
                userStore.update(s => {
                    if (s.chats[peerHash]) {
                         s.chats[peerHash].lastStatus = status;
                    }
                    return { ...s, chats: { ...s.chats } };
                });
                break;
            }
        }
        return { ...mStore };
    });
};

export const markAsDownloaded = async (peerHash: string, msgId: string, exportedPath?: string) => {
    messageStore.update(mStore => {
        if (!mStore[peerHash]) return mStore;
        const msgs = mStore[peerHash].map(m => {
            if (m.id === msgId && m.attachment) {
                const updatedMsg = { 
                    ...m, 
                    attachment: { 
                        ...m.attachment, 
                        isDownloaded: true,
                        exportedPath: exportedPath || m.attachment.exportedPath
                    } 
                };
                invoke('db_update_messages', {
                    ids: [msgId],
                    attachmentJson: JSON.stringify(updatedMsg.attachment)
                }).catch(console.error);
                return updatedMsg;
            }
            return m;
        });
        return { ...mStore, [peerHash]: msgs };
    });
};

