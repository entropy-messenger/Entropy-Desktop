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

export const sendFile = async (destIdRaw: string, file: File | { name: string, type: string, path: string }, type: 'file' | 'voice_note' = 'file') => {
    const destId = destIdRaw.toLowerCase();
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];

    let fileData: number[] | null = null;
    let filePath: string | null = null;

    if ('path' in file) {
        filePath = file.path;
    } else {
        const buffer = await file.arrayBuffer();
        fileData = Array.from(new Uint8Array(buffer));
    }

    try {
        await invoke('process_outgoing_media', {
            payload: {
                recipient: destId,
                fileData: fileData,
                filePath: filePath,
                fileName: file.name,
                fileType: file.type,
                msgType: type,
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
    } catch (e) {
        console.error("[Chat] Media send failed:", e);
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

    // 3. Update History (Transient Store)
    messageStore.update(mStore => {
        const msgs = mStore[peerHash] || [];
        if (msgs.some(m => m.id === msg.id)) return mStore;
        const updated = [...msgs, msg].sort((a, b) => a.timestamp - b.timestamp).slice(-100);
        return { ...mStore, [peerHash]: updated };
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
                is_group: !!chat.isGroup,
                alias: chat.peerNickname || null,
                last_msg: chat.lastMsg || null,
                last_timestamp: chat.lastTimestamp || null,
                last_sender_hash: chat.lastSenderHash || null,
                last_status: chat.lastStatus || null,
                unread_count: chat.unreadCount || 0,
                is_archived: !!chat.isArchived,
                is_pinned: !!chat.isPinned,
                is_blocked: !!chat.isBlocked,
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
            invoke('db_update_messages_status', { chatAddress: peerHash, ids: msgIds, status: 'read' }).catch(console.error);
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
        const identityHash = get(userStore).identityHash;
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

        messageStore.update(mStore => ({
            ...mStore,
            [peerHash]: messages.sort((a, b) => a.timestamp - b.timestamp)
        }));

        return messages.length;
    } catch (e) {
        console.error("[Chat] Load failed:", e);
        return 0;
    }
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
        if (msg) invoke('db_set_message_starred', { id, isStarred: msg.isStarred }).catch(console.error);
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

export const markAsDownloaded = async (peerHash: string, msgId: string) => {
    messageStore.update(mStore => {
        if (!mStore[peerHash]) return mStore;
        const msgs = mStore[peerHash].map(m => {
            if (m.id === msgId && m.attachment) {
                const updatedMsg = { ...m, attachment: { ...m.attachment, isDownloaded: true } };
                invoke('db_save_message', {
                    msg: {
                        id: updatedMsg.id,
                        chatAddress: peerHash,
                        senderHash: updatedMsg.senderHash,
                        content: updatedMsg.content,
                        timestamp: updatedMsg.timestamp,
                        type: updatedMsg.type,
                        status: updatedMsg.status,
                        attachmentJson: JSON.stringify(updatedMsg.attachment),
                        isStarred: !!updatedMsg.isStarred,
                        isGroup: !!updatedMsg.groupId,
                        replyToJson: updatedMsg.replyTo ? JSON.stringify(updatedMsg.replyTo) : null
                    }
                }).catch(console.error);
                return updatedMsg;
            }
            return m;
        });
        return { ...mStore, [peerHash]: msgs };
    });
};
