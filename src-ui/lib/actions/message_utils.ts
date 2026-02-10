import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { attachmentStore } from '../attachment_store';
import { signalManager } from '../signal_manager';
import { network } from '../network';
import { toHex } from '../utils';
import type { Message } from '../types';

/**
 * Commits a message to the local store.
 * Handles attachment indexing, deduplication, unread tracking, and system notifications.
 */
export const addMessage = (peerHash: string, msg: Message) => {
    if (msg.attachment?.data) attachmentStore.put(msg.id, msg.attachment.data).catch(e => { });

    userStore.update(s => {
        const chat = s.chats[peerHash];
        if (!chat) {
            s.chats[peerHash] = { peerHash, peerAlias: peerHash.slice(0, 8), messages: [], unreadCount: 0 };
        } else if (chat.messages.some(m => m.id === msg.id)) {
            return s;
        }

        const updatedChat = { ...s.chats[peerHash] };
        updatedChat.messages = [...updatedChat.messages, msg];

        if (!msg.isMine) {
            if (s.activeChatHash === peerHash) {
                msg.status = 'read';
                sendReceipt(peerHash, [msg.id], 'read');
            } else {
                updatedChat.unreadCount = (updatedChat.unreadCount || 0) + 1;

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
        return { ...s, chats: { ...s.chats } };
    });
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
    if (status === 'read' && !state.privacySettings.readReceipts) return;
    if (msgIds.length === 0) return;
    const receipt = { type: 'receipt', msgIds, status };
    try {
        const ciphertext = await signalManager.encrypt(peerHash, JSON.stringify(receipt), get(userStore).relayUrl, true);
        network.sendVolatile(peerHash, new TextEncoder().encode(JSON.stringify(ciphertext)));
    } catch (e) { }
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
