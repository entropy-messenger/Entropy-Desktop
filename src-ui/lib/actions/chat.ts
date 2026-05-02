import { get, writable } from 'svelte/store';
import { userStore, messageStore } from '../stores/user';
import { network } from '../network';
import { invoke } from '@tauri-apps/api/core';
import type { Message, Chat } from '../types';

/**
 * Cache and attachment orchestration:
 * Memory cache for media assets and indexing logic.
 */
let proxyPort: number | null = null;

export const getMediaUrl = async (id: string, type: string): Promise<string> => {
    if (!proxyPort) {
        try {
            proxyPort = await invoke<number>('get_media_proxy_port');
        } catch (e) {

            return "";
        }
    }
    return `http://localhost:${proxyPort}/media/${id}?type=${encodeURIComponent(type)}`;
};

export const setReplyingTo = (msg: Message | null) => userStore.update(s => ({ ...s, replyingTo: msg }));

/**
 * Force a reactive UI refresh for a specific message.
 * Used when a background process (like media re-encryption) completes.
 */
export const refreshMessageUI = (msgId: string) => {
    messageStore.update(m => {
        for (const chatAddress in m) {
            const list = m[chatAddress];
            const idx = list.findIndex(x => x.id === msgId);
            if (idx !== -1) {
                list[idx] = { ...list[idx] };
                break;
            }
        }
        return { ...m };
    });
};

export const markMessageAsError = (msgId: string, error: string) => {
    messageStore.update(m => {
        for (const chatAddress in m) {
            const list = m[chatAddress];
            const idx = list.findIndex(x => x.id === msgId);
            if (idx !== -1) {
                list[idx] = { ...list[idx], status: 'failed', error };
                break;
            }
        }
        return { ...m };
    });
};

export const sendMessage = async (destIdRaw: string, content: string) => {
    const destId = destIdRaw.toLowerCase();
    const state = get(userStore);
    if (!state.identityHash || state.blockedHashes.includes(destId)) return;
    const chat = state.chats[destId];

    try {
        const command = chat?.isGroup ? 'process_outgoing_group_text' : 'process_outgoing_text';
        await invoke(command, {
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
                groupMembers: chat?.members || null,
                groupName: chat?.isGroup ? (chat?.peerNickname || null) : null
            }
        });
        setReplyingTo(null);
    } catch (e) {
        // Send failed
    }
};

export const sendFile = async (destIdRaw: string, file: { name: string, type: string, path?: string, data?: Uint8Array | ArrayBuffer }, type: 'file' | 'voice_note' = 'file', duration?: number, thumbnail?: string) => {
    const destId = destIdRaw.toLowerCase();
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];

    // 1. Create an Optimistic Message for immediate UI feedback
    const tempId = 'opt-' + Math.random().toString(36).substring(2, 9);
    const optimisticMsg: Message = {
        id: tempId,
        content: type === 'voice_note' ? 'Voice Note' : file.name,
        senderHash: state.identityHash,
        timestamp: Date.now(),
        isMine: true,
        status: 'sending',
        type: type,
        attachment: {
            fileName: file.name,
            fileType: file.type,
            size: (file as any).size || 0,
            originalPath: file.path,
            vaultPath: '', // Will be filled by backend
            thumbnail: thumbnail
        }
    };

    // Add to UI immediately
    addMessage(destId, optimisticMsg);

    const command = chat?.isGroup ? 'process_outgoing_group_media' : 'process_outgoing_media';
    
    // Resolve data if path is missing (e.g. for recorded voice notes)
    let finalData: Uint8Array | null = null;
    if (!file.path) {
        if (file instanceof Blob) {
            const buffer = await file.arrayBuffer();
            finalData = new Uint8Array(buffer);
        } else if (file.data) {
            finalData = file.data instanceof Uint8Array ? file.data : new Uint8Array(file.data);
        }
    }

    invoke(command, {
        payload: {
            recipient: destId,
            filePath: file.path || null,
            fileData: finalData,
            fileName: file.name,
            fileType: file.type,
            msgType: type,
            duration: duration,
            thumbnail: thumbnail || null,
            isGroup: !!chat?.isGroup,
            groupMembers: chat?.members || null,
            groupName: chat?.isGroup ? (chat?.peerNickname || null) : null,
            replyTo: state.replyingTo ? {
                id: state.replyingTo.id,
                content: state.replyingTo.content,
                senderHash: state.replyingTo.senderHash,
                senderAlias: state.replyingTo.senderAlias,
                type: state.replyingTo.type
            } : null
        }
    }).catch(async (e: any) => {
        // Handle failure
        messageStore.update(mStore => {
            if (mStore[destId]) {
                mStore[destId] = mStore[destId].map(m => m.id === tempId ? { ...m, status: 'failed' } : m);
            }
            return { ...mStore };
        });
        const { addToast } = await import('../stores/ui');
        addToast("Media failed to send: " + e.toString(), 'error');
    });

    setReplyingTo(null);
};

export const addMessage = async (peerHashRaw: string, msg: Message) => {
    const peerHash = peerHashRaw.toLowerCase();

    // Early-exit if this message ID is already known — prevents duplicate events
    // from retried packets inflating unread counters or adding ghost entries.
    const existingMsgs = get(messageStore)[peerHash] || [];
    if (existingMsgs.some(m => m.id === msg.id)) return;

    let updatedChatMetadata: Chat | null = null;

    // Update Chat Metadata
    let isNewChat = false;
    userStore.update(s => {
        if (!s.chats[peerHash]) {
            const chatData = {
                peerHash,
                peerNickname: msg.chatAlias || peerHash.slice(0, 8),
                unreadCount: 0,
                isGroup: !!msg.isGroup,
                trustLevel: 1,
                members: msg.chatMembers || []
            };
            s.chats[peerHash] = chatData;
            if (!msg.isMine && !msg.isGroup && !msg.chatAlias) {
                import('./contacts').then(m => m.resolveIdentity(peerHash));
            }
            isNewChat = true;
        }
        const chat = { ...s.chats[peerHash] };

        // Update alias if we got a better one from the message metadata
        if (msg.chatAlias && (!chat.peerNickname || chat.peerNickname === peerHash.slice(0, 8))) {
            chat.peerNickname = msg.chatAlias;
        }

        // Update members if provided (for self-healing)
        if (msg.chatMembers && msg.chatMembers.length > 0) {
            chat.members = msg.chatMembers;
            chat.isGroup = true;
        }

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

    // Update History
    let needsSnapToPresent = false;
    messageStore.update(mStore => {
        const msgs = mStore[peerHash] || [];
        if (msgs.some(m => m.id === msg.id)) return mStore;

        // De-duplicate Optimistic messages
        let finalMsgs = msgs;
        if (msg.isMine && !msg.id.startsWith('opt-')) {
            const incomingFileName = (msg.attachment?.fileName || "").toLowerCase();
            const cleanIncomingContent = (msg.content.startsWith('File: ') ? msg.content.substring(6) : msg.content).toLowerCase();
            
            const ghostIndex = msgs.findIndex(m => {
                if (!m.id.startsWith('opt-') || m.status !== 'sending') return false;
                
                // Match by attachment filename (case-insensitive)
                const ghostFileName = (m.attachment?.fileName || "").toLowerCase();
                if (incomingFileName && ghostFileName === incomingFileName) return true;
                
                // Fallback: Match by content string (case-insensitive)
                const ghostContent = (m.content || "").toLowerCase();
                return ghostContent === cleanIncomingContent || ghostContent === msg.content.toLowerCase();
            });

            if (ghostIndex !== -1) {
                finalMsgs = [...msgs];
                finalMsgs[ghostIndex] = msg;
                return { ...mStore, [peerHash]: finalMsgs.sort((a, b) => a.timestamp - b.timestamp) };
            }
        }

        const chat = get(userStore).chats[peerHash];

        if (chat?.hasMoreNewer && !msg.isMine) {
            return mStore;
        }
        if (chat?.hasMoreNewer && msg.isMine) {
            needsSnapToPresent = true;
            return mStore;
        }

        const combined = [...finalMsgs, msg].sort((a, b) => a.timestamp - b.timestamp);
        let final = combined;
        if (combined.length > 2000) {
            if (chat?.hasMoreNewer) {
                const mid = Math.floor(combined.length / 2);
                final = combined.slice(mid - 500, mid + 500);
            } else {
                final = combined.slice(-1000);
            }
        }

        return { ...mStore, [peerHash]: final };
    });

    if (needsSnapToPresent) {
        jumpToPresent(peerHash);
    }

    // Persistence & Notifications
    if (updatedChatMetadata) {
        syncChatToDb(updatedChatMetadata);
        const s = get(userStore);
        if (!msg.isMine && s.activeChatHash !== peerHash) triggerNativeNotification(updatedChatMetadata, msg);

    }
};

export const syncChatToDb = async (chat: Chat) => {
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
                // Unread count is managed exclusively by Rust.
                // Always pass 0 here so the DB count is never overwritten by stale frontend values.
                unreadCount: 0,
                isArchived: !!chat.isArchived,
                isPinned: !!chat.isPinned,
                isBlocked: !!chat.isBlocked,
                members: chat.members || null
            }
        });
    } catch (e) {
        // DB sync failed
    }
};

const pendingReceipts: Record<string, { ids: Set<string>, timeout: any }> = {};

export const sendReceipt = async (peerHash: string, msgIds: string[], status: 'delivered' | 'read') => {
    const key = `${peerHash}:${status}`;

    if (!pendingReceipts[key]) {
        pendingReceipts[key] = { ids: new Set(), timeout: null };
    }

    const entry = pendingReceipts[key];
    msgIds.forEach(id => entry.ids.add(id));

    if (entry.timeout) clearTimeout(entry.timeout);

    entry.timeout = setTimeout(async () => {
        const finalIds = Array.from(entry.ids);
        delete pendingReceipts[key];

        const state = get(userStore);
        if (state.blockedHashes.includes(peerHash)) return;
        if (status === 'read' && !state.privacySettings.readReceipts) return;

        try {
            await invoke('send_receipt', { peerHash, msgIds: finalIds, status });

            messageStore.update(mStore => {
                if (!mStore[peerHash]) return mStore;
                mStore[peerHash] = mStore[peerHash].map(m => finalIds.includes(m.id) ? { ...m, status } : m);
                return { ...mStore };
            });

            if (status === 'read') {
                userStore.update(s => {
                    const chat = s.chats[peerHash];
                    if (chat) s.chats[peerHash] = { ...chat, unreadCount: 0 };
                    return { ...s, chats: { ...s.chats } };
                });
                invoke('db_reset_unread_count', { address: peerHash }).catch(() => { });
                invoke('db_update_messages', { ids: finalIds, status: 'read' }).catch(() => { });
            }
        } catch (e) {
            // Receipt failed
        }
    }, 300);
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

export const loadChatMessages = async (peerHash: string, limit = 50, offset = 0, direction: 'jump' | 'older' | 'newer' = 'jump') => {
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

            const sorted = combined.sort((a, b) => a.timestamp - b.timestamp);

            let final = sorted;
            if (sorted.length > 2000) {
                const chat = get(userStore).chats[peerHash];
                if (chat?.hasMoreNewer) {
                    const mid = Math.floor(sorted.length / 2);
                    final = sorted.slice(mid - 500, mid + 500);
                } else {
                    final = sorted.slice(-1000);
                }
            }

            return {
                ...mStore,
                [peerHash]: final
            };
        });

        userStore.update(s => {
            if (s.chats[peerHash]) {
                const chat = s.chats[peerHash];
                const newTop = direction === 'older' || direction === 'jump' ? offset + messages.length : chat.topOffset || 0;
                const newBottom = direction === 'newer' || direction === 'jump' ? offset : chat.bottomOffset || 0;

                s.chats[peerHash] = {
                    ...chat,
                    hasMore: direction === 'older' || direction === 'jump' ? messages.length === limit : chat.hasMore,
                    hasMoreNewer: newBottom > 0,
                    topOffset: newTop,
                    bottomOffset: newBottom
                };
            }
            return { ...s, chats: { ...s.chats } };
        });

        return messages.length;
    } catch (e) {
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
        return 0;
    }
};

export const loadMoreMessages = async (peerHash: string) => {
    const chat = get(userStore).chats[peerHash];
    const topOffset = chat?.topOffset || 0;

    return await loadChatMessages(peerHash, 50, topOffset, 'older');
};

export const loadNewerMessages = async (peerHash: string) => {
    const chat = get(userStore).chats[peerHash];
    if (!chat || !chat.hasMoreNewer) return 0;

    const bottomOffset = chat.bottomOffset || 0;
    const fetchLimit = Math.min(50, bottomOffset);
    const newOffset = bottomOffset - fetchLimit;

    return await loadChatMessages(peerHash, fetchLimit, newOffset, 'newer');
};

export const jumpToMessage = async (peerHash: string, msgId: string) => {
    try {
        const offset = await invoke<number>('db_get_message_offset', { chatAddress: peerHash, messageId: msgId });

        const startOffset = Math.max(0, offset - 25);

        messageStore.update(mStore => {
            return { ...mStore, [peerHash]: [] };
        });

        userStore.update(s => {
            if (s.chats[peerHash]) {
                const c = s.chats[peerHash];
                s.chats[peerHash] = { ...c, topOffset: startOffset + 100, bottomOffset: startOffset };
            }
            return { ...s, activeChatHash: peerHash };
        });

        await loadChatMessages(peerHash, 100, startOffset, 'jump');

        return true;
    } catch (e) {
        return false;
    }
};

export const jumpToPresent = async (peerHash: string) => {
    messageStore.update(mStore => {
        return { ...mStore, [peerHash]: [] };
    });

    userStore.update(s => {
        if (s.chats[peerHash]) {
            s.chats[peerHash] = { ...s.chats[peerHash], topOffset: 0, bottomOffset: 0, hasMoreNewer: false };
        }
        return { ...s };
    });

    await loadChatMessages(peerHash, 50, 0, 'jump');
};

export const deleteMessage = async (peerHash: string, msgIds: string[]) => {
    msgIds.forEach(id => {
        invoke('vault_delete_media', { id }).catch(() => { });
    });

    let newLastMsg: Message | undefined;
    let deletedUnreadCount = 0;

    messageStore.update(mStore => {
        if (mStore[peerHash]) {
            const toDelete = mStore[peerHash].filter(m => msgIds.includes(m.id));
            deletedUnreadCount = toDelete.filter(m => !m.isMine && m.status !== 'read').length;

            mStore[peerHash] = mStore[peerHash].filter(m => !msgIds.includes(m.id));
            newLastMsg = mStore[peerHash][mStore[peerHash].length - 1];
        }
        return { ...mStore };
    });

    userStore.update(s => {
        const chat = s.chats[peerHash];
        if (chat) {
            const updatedChat = {
                ...chat,
                unreadCount: Math.max(0, (chat.unreadCount || 0) - deletedUnreadCount)
            };

            if (newLastMsg) {
                s.chats[peerHash] = {
                    ...updatedChat,
                    lastMsg: newLastMsg.content,
                    lastTimestamp: newLastMsg.timestamp,
                    lastStatus: newLastMsg.status,
                    lastIsMine: newLastMsg.isMine,
                    lastSenderHash: newLastMsg.senderHash
                };
            } else {
                s.chats[peerHash] = {
                    ...updatedChat,
                    lastMsg: undefined,
                    lastTimestamp: undefined,
                    lastStatus: undefined,
                    lastIsMine: undefined,
                    lastSenderHash: undefined
                };
            }
        }
        return { ...s, chats: { ...s.chats } };
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
        if (msg) invoke('db_update_messages', { ids: [id], isStarred: msg.isStarred }).catch(() => { });
    }
};

let typingTimeouts: Record<string, any> = {};

export const handleTypingSignal = (senderHash: string, payload: any) => {
    if (typingTimeouts[senderHash]) {
        clearTimeout(typingTimeouts[senderHash]);
        delete typingTimeouts[senderHash];
    }

    userStore.update(s => {
        if (s.chats[senderHash]) s.chats[senderHash] = { ...s.chats[senderHash], isTyping: payload.isTyping };
        return { ...s, chats: { ...s.chats } };
    });

    if (payload.isTyping) {
        typingTimeouts[senderHash] = setTimeout(() => {
            userStore.update(s => {
                if (s.chats[senderHash]) s.chats[senderHash] = { ...s.chats[senderHash], isTyping: false };
                return { ...s, chats: { ...s.chats } };
            });
            delete typingTimeouts[senderHash];
        }, 4000); // 4s safety timeout (sender pulses every 2s)
    }
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

export const updateSingleMessageStatusUI = (msgId: string, status: any, chatAddress?: string, attachment?: any) => {
    messageStore.update(mStore => {
        // Direct lookup if chatAddress is known
        if (chatAddress && mStore[chatAddress]) {
            const index = mStore[chatAddress].findIndex(m => m.id === msgId);
            if (index !== -1) {
                const updatedMessages = [...mStore[chatAddress]];
                updatedMessages[index] = { 
                    ...updatedMessages[index], 
                    status,
                    ...(attachment ? { attachment: { ...updatedMessages[index].attachment, ...attachment } } : {})
                };
                mStore[chatAddress] = updatedMessages;

                userStore.update(s => {
                    if (s.chats[chatAddress]) {
                        // Immutably update the chat object same way
                        s.chats[chatAddress] = { ...s.chats[chatAddress], lastStatus: status };
                    }
                    return { ...s, chats: { ...s.chats } };
                });
                return { ...mStore };
            }
        }

        // Search all chats if address is missing
        for (const peerHash of Object.keys(mStore)) {
            const index = mStore[peerHash].findIndex(m => m.id === msgId);
            if (index !== -1) {
                const updatedMessages = [...mStore[peerHash]];
                updatedMessages[index] = { 
                    ...updatedMessages[index], 
                    status,
                    ...(attachment ? { attachment: { ...updatedMessages[index].attachment, ...attachment } } : {})
                };
                mStore[peerHash] = updatedMessages;

                userStore.update(s => {
                    if (s.chats[peerHash]) {
                        s.chats[peerHash] = { ...s.chats[peerHash], lastStatus: status };
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
                }).catch(() => { });
                return updatedMsg;
            }
            return m;
        });
        return { ...mStore, [peerHash]: msgs };
    });
};

