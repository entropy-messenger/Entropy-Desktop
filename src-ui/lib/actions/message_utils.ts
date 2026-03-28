import { get, writable } from 'svelte/store';
import { userStore, messageStore } from '../stores/user';
import { attachmentStore } from '../attachment_store';
import { invoke } from '@tauri-apps/api/core';
import type { Message, Chat } from '../types';

/**
 * Commits a message to the relational database and the local reactive stores.
 * Separates metadata (Sidebar) from history (Chat Window).
 */
export const addMessage = async (peerHash: string, msg: Message) => {
    // 1. Index attachments
    if (msg.attachment?.data && !(msg.isMine && msg.status === 'sending')) {
        await attachmentStore.put(msg.id, msg.attachment.data).catch(() => { });
    }

    let updatedChatMetadata: Chat | null = null;

    // 2. Update Chat Metadata (Sidebar)
    userStore.update(s => {
        if (!s.chats[peerHash]) {
            s.chats[peerHash] = {
                peerHash,
                peerAlias: peerHash.slice(0, 8),
                unreadCount: 0,
                isGroup: !!msg.groupId
            };
        }

        const chat = { ...s.chats[peerHash] };
        chat.lastMsg = msg.content;
        chat.lastTimestamp = msg.timestamp;
        chat.lastStatus = msg.status;
        chat.lastIsMine = msg.isMine;
        chat.lastSenderHash = msg.senderHash;
        chat.isTyping = false; // Reset typing on message

        if (!msg.isMine && s.activeChatHash !== peerHash) {
            chat.unreadCount = (chat.unreadCount || 0) + 1;
        }

        // Clear typing timeout if message received
        const sender = msg.senderHash || peerHash;
        if (typingTimeouts[sender]) {
            clearTimeout(typingTimeouts[sender]);
            delete typingTimeouts[sender];
        }

        s.chats[peerHash] = chat;
        updatedChatMetadata = chat;
        return { ...s, chats: { ...s.chats } };
    });

    // 3. Update Message History (Chat Window)
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
        if (!msg.isMine && s.activeChatHash !== peerHash) {
            triggerNativeNotification(updatedChatMetadata, msg);
        } else if (!msg.isMine && s.activeChatHash === peerHash) {
            invoke('db_update_messages_status', { chatAddress: peerHash, ids: [msg.id], status: 'read' }).catch(() => { });
            sendReceipt(peerHash, [msg.id], 'read');
        }
    }
};

const triggerNativeNotification = (chat: Chat, msg: Message) => {
    if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
        import('@tauri-apps/plugin-notification').then(({ sendNotification, isPermissionGranted }) => {
            isPermissionGranted().then((granted: boolean) => {
                if (granted) {
                    sendNotification({
                        title: `Message from ${chat.peerAlias || chat.localNickname || 'Peer'}`,
                        body: msg.content.length > 50 ? msg.content.substring(0, 47) + '...' : msg.content
                    });
                }
            });
        });
    }
};

/**
 * Updates an existing message's status or metadata.
 */
export const commitMessageUpdate = async (chatId: string, msgId: string, patch: Partial<Message>) => {
    let finalMsg: Message | null = null;

    messageStore.update(mStore => {
        const msgs = mStore[chatId];
        if (msgs) {
            const idx = msgs.findIndex(m => m.id === msgId);
            if (idx !== -1) {
                msgs[idx] = { ...msgs[idx], ...patch };
                finalMsg = msgs[idx];
            }
        }
        return { ...mStore };
    });

    if (finalMsg) {
        const m: Message = finalMsg;
        userStore.update(s => {
            const chat = s.chats[chatId];
            if (chat && m.timestamp === chat.lastTimestamp) {
                chat.lastStatus = m.status;
                chat.lastMsg = m.content;
                syncChatToDb(chat);
            }
            return { ...s, chats: { ...s.chats } };
        });

        try {
            const dbMsg = {
                id: m.id,
                chatAddress: chatId,
                senderHash: m.senderHash,
                content: m.content,
                timestamp: m.timestamp,
                type: m.type,
                status: m.status,
                attachmentJson: m.attachment ? JSON.stringify({ ...m.attachment, data: undefined }) : null,
                isStarred: !!m.isStarred,
                isGroup: !!m.groupId,
                replyToJson: m.replyTo ? JSON.stringify(m.replyTo) : null
            };
            await invoke('db_save_message', { msg: dbMsg });
        } catch (e) {
            console.error("[DB] Failed to sync message update:", e);
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
                alias: chat.peerAlias || null,
                pfp: chat.pfp || null,
                lastMsg: chat.lastMsg || null,
                lastTimestamp: chat.lastTimestamp || null,
                lastSenderHash: chat.lastIsMine ? state.identityHash : (chat.lastSenderHash || null),
                lastStatus: chat.lastStatus || null,
                unreadCount: chat.unreadCount || 0,
                isArchived: !!chat.isArchived,
                isPinned: !!chat.isPinned
            }
        });
    } catch (e) {
        console.error("[DB] Failed to sync chat:", e);
    }
};

export const loadChatMessages = async (peerHash: string, limit = 50, offset = 0, prepend = true) => {
    try {
        const identityHash = get(userStore).identityHash;
        const rawMsgs = await invoke<any[]>('db_get_messages', { 
            chatAddress: peerHash, 
            limit, 
            offset, 
            includeAttachments: true 
        });

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
            const existingIds = new Set(existing.map(m => m.id));
            const newMsgs = messages.filter(m => !existingIds.has(m.id));
            
            const updatedMessages = prepend 
                ? [...newMsgs, ...existing]
                : [...existing, ...newMsgs];

            return { ...mStore, [peerHash]: updatedMessages.sort((a, b) => a.timestamp - b.timestamp) };
        });

        userStore.update(s => {
            if (!s.chats[peerHash]) {
                s.chats[peerHash] = {
                    peerHash,
                    peerAlias: peerHash.slice(0, 8),
                    unreadCount: 0,
                };
            }
            const chat = { ...s.chats[peerHash] };
            chat.hasMore = messages.length === limit;

            // Sync sidebar preview from the actual newest DB message (fixes stale preview on first open)
            const allMsgs = get(messageStore)[peerHash] || [];
            if (allMsgs.length > 0) {
                const newest = allMsgs[allMsgs.length - 1];
                chat.lastMsg = newest.content;
                chat.lastTimestamp = newest.timestamp;
                chat.lastStatus = newest.status;
                chat.lastIsMine = newest.isMine;
                chat.lastSenderHash = newest.senderHash;
            }

            if (s.activeChatHash === peerHash) {
                const unreadIds = allMsgs.filter(m => !m.isMine && m.status !== 'read').map(m => m.id);
                if (unreadIds.length > 0) {
                    chat.unreadCount = 0;
                    invoke('db_update_messages_status', { chatAddress: peerHash, ids: unreadIds, status: 'read' }).catch(() => { });
                    // Send read receipt to peer so they get the blue tick
                    sendReceipt(peerHash, unreadIds, 'read');
                    syncChatToDb(chat);
                }
            }

            s.chats[peerHash] = chat;
            return { ...s, chats: { ...s.chats } };
        });
        
        return messages.length;
    } catch (e) {
        console.error("[DB] Failed to load messages:", e);
        return 0;
    }
};

export const loadMoreMessages = async (peerHash: string) => {
    const msgs = get(messageStore)[peerHash] || [];
    return await loadChatMessages(peerHash, 50, msgs.length, true); 
};

export const deleteMessage = (peerHash: string, msgId: string) => bulkDelete(peerHash, [msgId]);

export const bulkDelete = (peerHash: string, msgIds: string[]) => {
    msgIds.forEach(id => attachmentStore.delete(id).catch(() => { }));
    messageStore.update(mStore => {
        if (mStore[peerHash]) {
            mStore[peerHash] = mStore[peerHash].filter(m => !msgIds.includes(m.id));
        }
        return { ...mStore };
    });
};

export const sendReceipt = async (peerHash: string, msgIds: string[], status: 'delivered' | 'read') => {
    const state = get(userStore);
    if (state.blockedHashes.includes(peerHash)) return;
    if (status === 'read' && !state.privacySettings.readReceipts) return;
    if (msgIds.length === 0) return;
    try {
        await invoke('send_receipt', { peerHash, msgIds, status });
    } catch (e) {
        console.error(`[Receipt] Failed to send to ${peerHash}:`, e);
    }
};

export const markAsDownloaded = (chatId: string, msgId: string) => {
    messageStore.update(mStore => {
        const msgs = mStore[chatId];
        if (msgs) {
            const m = msgs.find(x => x.id === msgId);
            if (m && m.attachment) m.attachment.isDownloaded = true;
        }
        return { ...mStore };
    });
};

export const downloadAttachment = async (chatId: string, msgId: string) => {
    // Placeholder for background download logic
    console.debug(`[Attachment] Initiating download for ${msgId} in ${chatId}`);
};

export const updateMessageStatusUI = (senderHash: string, ids: string[], status: string) => {
    const state = get(userStore);
    if (status === 'read' && !state.privacySettings.readReceipts) return;
    const idSet = new Set(ids);

    messageStore.update(mStore => {
        for (const chatId in mStore) {
            const msgs = mStore[chatId];
            let changed = false;

            const updated = msgs.map(m => {
                if (!idSet.has(m.id)) return m;
                const oldPriority = m.status === 'read' ? 3 : m.status === 'delivered' ? 2 : 1;
                const newPriority = status === 'read' ? 3 : status === 'delivered' ? 2 : 1;
                if (newPriority > oldPriority) {
                    changed = true;
                    return { ...m, status: status as any };
                }
                return m;
            });

            if (changed) {
                mStore[chatId] = updated;

                // Update sidebar preview if the last message is one that changed
                userStore.update(s => {
                    const chat = s.chats[chatId];
                    if (chat) {
                        const lastMsgInStore = updated[updated.length - 1];
                        // If the message being updated is the same as the one shown in the sidebar (by timestamp or ID)
                        // OR if it is the absolute newest in the store
                        const isLastInSidebar = (lastMsgInStore && idSet.has(lastMsgInStore.id)) || 
                                              msgs.some(m => idSet.has(m.id) && m.timestamp === chat.lastTimestamp);
                        
                        if (isLastInSidebar) {
                            const updatedChat = { ...chat, lastStatus: status as any };
                            s.chats[chatId] = updatedChat;
                            syncChatToDb(updatedChat);
                        }
                    }
                    return { ...s, chats: { ...s.chats } };
                });
            }
        }
        return { ...mStore };
    });
};


const typingTimeouts: Record<string, any> = {};
export const handleTypingSignal = (senderHash: string, payload: any) => {
    const state = get(userStore);
    if (state.privacySettings.typingStatus !== 'everyone') return;
    if (typingTimeouts[senderHash]) clearTimeout(typingTimeouts[senderHash]);

    userStore.update(s => {
        if (s.chats[senderHash]) {
            s.chats[senderHash] = { ...s.chats[senderHash], isTyping: payload.isTyping };
        }
        return { ...s, chats: { ...s.chats } };
    });

    if (payload.isTyping) {
        typingTimeouts[senderHash] = setTimeout(() => {
            userStore.update(s => {
                if (s.chats[senderHash]) {
                    s.chats[senderHash] = { ...s.chats[senderHash], isTyping: false };
                }
                return { ...s, chats: { ...s.chats } };
            });
            delete typingTimeouts[senderHash];
        }, 6000);
    }
};

export const handleProfileUpdate = (senderHash: string, payload: any) => {
    userStore.update(s => {
        if (s.chats[senderHash]) {
            const chat = { ...s.chats[senderHash] };
            if (payload.alias) chat.peerAlias = payload.alias;
            if (payload.pfp) chat.pfp = payload.pfp;
            s.chats[senderHash] = chat;
            syncChatToDb(chat);
        }
        return { ...s, chats: { ...s.chats } };
    });
};
