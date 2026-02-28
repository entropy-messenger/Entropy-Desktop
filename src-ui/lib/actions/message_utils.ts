import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { attachmentStore } from '../attachment_store';
import { signalManager } from '../signal_manager';
import { network } from '../network';
import { invoke } from '@tauri-apps/api/core';
import { toHex } from '../utils';
import type { Message } from '../types';

/**
 * Commits a message to the local store.
 * Handles attachment indexing, deduplication, unread tracking, and system notifications.
 */
/**
 * Commits a message to the relational database and the local reactive store.
 */
export const addMessage = async (peerHash: string, msg: Message) => {
    // Sender messages handle vault storage post-encryption to avoid race conditions
    if (msg.attachment?.data && !(msg.isMine && msg.status === 'sending')) {
        await attachmentStore.put(msg.id, msg.attachment.data).catch(() => { });
    }

    // 1. Persist to Relational DB
    try {
        const dbMsg = {
            id: msg.id,
            chat_address: peerHash,
            sender_hash: msg.senderHash,
            content: msg.content,
            timestamp: msg.timestamp,
            type: msg.type,
            status: msg.status,
            attachment_json: msg.attachment ? JSON.stringify({ ...msg.attachment, data: undefined }) : null
        };
        await invoke('db_save_message', { msg: dbMsg });
    } catch (e) {
        console.error("[DB] Failed to save message:", e);
    }

    // 2. Reactive Update
    userStore.update(s => {
        const chat = s.chats[peerHash];
        if (!chat) {
            const newChat: any = {
                peerHash,
                peerAlias: peerHash.slice(0, 8),
                messages: [],
                unreadCount: 0,
                isGroup: !!msg.groupId
            };
            s.chats[peerHash] = newChat;
            // Async sync new chat to DB
            syncChatToDb(newChat);
        } else if (chat.messages.some(m => m.id === msg.id)) {
            return s;
        }

        const updatedChat = { ...s.chats[peerHash] };
        // Update sidebar preview metadata
        updatedChat.lastMsg = msg.content;
        updatedChat.lastTimestamp = msg.timestamp;
        updatedChat.lastStatus = msg.status;
        updatedChat.lastIsMine = msg.isMine;
        updatedChat.lastSenderHash = msg.senderHash;

        // We only keep a small buffer of messages in memory
        updatedChat.messages = [...updatedChat.messages, msg].slice(-100);

        if (!msg.isMine) {
            if (s.activeChatHash === peerHash) {
                msg.status = 'read';
                sendReceipt(peerHash, [msg.id], 'read');
            } else {
                updatedChat.unreadCount = (updatedChat.unreadCount || 0) + 1;
                // Native Notification Logic...
                if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
                    import('@tauri-apps/plugin-notification').then(({ sendNotification, isPermissionGranted }) => {
                        isPermissionGranted().then((granted: boolean) => {
                            if (granted) {
                                sendNotification({
                                    title: `Message from ${updatedChat.peerAlias}`,
                                    body: msg.content.length > 50 ? msg.content.substring(0, 47) + '...' : msg.content
                                });
                            }
                        });
                    });
                }
            }
        }

        s.chats[peerHash] = updatedChat;
        // 3. Persist updated metadata (last message, unread count) to DB
        syncChatToDb(updatedChat);
        return { ...s, chats: { ...s.chats } };
    });
};

/**
 * Updates an existing message's status or metadata in both store and DB.
 */
export const commitMessageUpdate = async (chatId: string, msgId: string, patch: Partial<Message>) => {
    let finalMsg: Message | null = null;

    userStore.update(s => {
        const chat = s.chats[chatId];
        if (chat) {
            const idx = chat.messages.findIndex(m => m.id === msgId);
            if (idx !== -1) {
                chat.messages[idx] = { ...chat.messages[idx], ...patch };
                finalMsg = chat.messages[idx];

                // If it's the last message, update sidebar preview
                if (idx === chat.messages.length - 1) {
                    chat.lastStatus = finalMsg.status;
                    chat.lastMsg = finalMsg.content;
                    chat.lastTimestamp = finalMsg.timestamp;
                    chat.lastIsMine = finalMsg.isMine;
                    syncChatToDb(chat);
                }
            }
        }
        return { ...s, chats: { ...s.chats } };
    });

    if (finalMsg) {
        try {
            const m: Message = finalMsg;
            const dbMsg = {
                id: m.id,
                chat_address: chatId,
                sender_hash: m.senderHash,
                content: m.content,
                timestamp: m.timestamp,
                type: m.type,
                status: m.status,
                attachment_json: m.attachment ? JSON.stringify({ ...m.attachment, data: undefined }) : null
            };
            await invoke('db_save_message', { msg: dbMsg });
        } catch (e) {
            console.error("[DB] Failed to sync message update:", e);
        }
    }
};

export const syncChatToDb = async (chat: any) => {
    const state = get(userStore);
    try {
        await invoke('db_upsert_chat', {
            chat: {
                address: chat.peerHash,
                is_group: !!chat.isGroup,
                alias: chat.peerAlias || null,
                pfp: chat.pfp || null,
                last_msg: chat.lastMsg || (chat.messages?.length > 0 ? chat.messages[chat.messages.length - 1].content : null),
                last_timestamp: chat.lastTimestamp || (chat.messages?.length > 0 ? chat.messages[chat.messages.length - 1].timestamp : null),
                last_sender_hash: chat.lastIsMine ? state.identityHash : (chat.lastSenderHash || null),
                last_status: chat.lastStatus || (chat.messages?.length > 0 ? chat.messages[chat.messages.length - 1].status : null),
                unread_count: chat.unreadCount || 0,
                is_archived: !!chat.isArchived
            }
        });
    } catch (e) {
        console.error("[DB] Failed to sync chat:", e);
    }
};

export const loadChatMessages = async (peerHash: string, limit = 50, offset = 0) => {
    try {
        const rawMsgs = await invoke<any[]>('db_get_messages', { chatAddress: peerHash, limit, offset });
        const identityHash = get(userStore).identityHash;
        const messages: Message[] = rawMsgs.map(m => {
            let attachment = undefined;
            if (m.attachment_json) {
                try {
                    attachment = JSON.parse(m.attachment_json);
                } catch (e) {
                    console.error("[DB] Failed to parse attachment JSON:", e);
                }
            }
            return {
                id: m.id,
                timestamp: m.timestamp,
                senderHash: m.sender_hash,
                content: m.content,
                type: m.type as any,
                isMine: m.sender_hash === identityHash,
                status: m.status as any,
                attachment
            };
        });

        userStore.update(s => {
            console.debug(`[DB Load] Updating store for ${peerHash} with ${messages.length} messages.`);

            // Ensure chat exists in memory - this avoids the "requires sending a message to show" bug
            if (!s.chats[peerHash]) {
                console.debug(`[DB Load] Creating missing chat in-memory for ${peerHash}.`);
                s.chats[peerHash] = {
                    peerHash,
                    peerAlias: peerHash.slice(0, 8),
                    messages: [],
                    unreadCount: 0,
                    isOnline: false,
                    isTyping: false
                };
            }

            const chat = s.chats[peerHash];
            const updatedChat = {
                ...chat,
                messages: messages.sort((a, b) => a.timestamp - b.timestamp) // Safety re-sort
            };

            // If this is the active chat, mark as read immediately
            if (s.activeChatHash === peerHash) {
                const unreadIds: string[] = [];
                updatedChat.messages.forEach(m => {
                    if (!m.isMine && m.status !== 'read') {
                        m.status = 'read';
                        unreadIds.push(m.id);
                    }
                });

                if (unreadIds.length > 0) {
                    updatedChat.unreadCount = 0;
                    sendReceipt(peerHash, unreadIds, 'read');
                    invoke('db_update_messages_status', { chatAddress: peerHash, ids: unreadIds, status: 'read' }).catch(() => { });
                    syncChatToDb(updatedChat);
                }
            }
            s.chats[peerHash] = updatedChat;
            return { ...s, chats: { ...s.chats } };
        });
    } catch (e) {
        console.error("[DB] Failed to load messages:", e);
    }
};

/**
 * Deletes multiple messages from the store and indexes.
 */
export const bulkDelete = (peerHash: string, msgIds: string[]) => {
    msgIds.forEach(id => attachmentStore.delete(id).catch(() => { }));
    userStore.update(s => {
        if (s.chats[peerHash]) {
            s.chats[peerHash].messages = s.chats[peerHash].messages.filter(m => !msgIds.includes(m.id));
        }
        return { ...s, chats: { ...s.chats } };
    });
};

export const deleteMessage = (peerHash: string, msgId: string) => bulkDelete(peerHash, [msgId]);

/**
 * Transmits an encrypted delivery or read receipt to a peer.
 */
export const sendReceipt = async (peerHash: string, msgIds: string[], status: 'delivered' | 'read') => {
    const state = get(userStore);
    if (state.blockedHashes.includes(peerHash)) return;

    // Delivery receipts are necessary for the P2P handshake/sync, 
    // so we only respect the toggle for 'read' receipts.
    if (status === 'read' && !state.privacySettings.readReceipts) return;

    if (msgIds.length === 0) return;
    try {
        await invoke('send_receipt', { peerHash, msgIds, status });
    } catch (e) {
        console.error(`[Receipt] Failed to send to ${peerHash}:`, e);
    }
};

/**
 * Decrypts a stored attachment and triggers a browser-level download for the host system.
 */
export const downloadAttachment = async (msgId: string, bundle: any) => {
    try {
        const encrypted = await attachmentStore.get(msgId);
        if (!encrypted) {
            throw new Error("Attachment not found locally");
        }

        const decrypted = await signalManager.decryptMedia(encrypted, bundle);

        const blob = new Blob([decrypted as any], { type: bundle.file_type || 'application/octet-stream' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = bundle.file_name || 'download';
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
    } catch (e) {
        console.error("[Download] Failed:", e);
    }
};

/**
 * Updates the local state to flag an attachment as physically retrieved.
 */
export const markAsDownloaded = (chatId: string, msgId: string) => {
    userStore.update(s => {
        const chat = s.chats[chatId];
        if (chat) {
            const m = chat.messages.find(x => x.id === msgId);
            if (m && m.attachment) {
                m.attachment.isDownloaded = true;
            }
        }
        return { ...s, chats: { ...s.chats } };
    });
};
