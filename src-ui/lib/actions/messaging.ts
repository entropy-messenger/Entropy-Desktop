
import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { signalManager } from '../signal_manager';
import { network } from '../network';
import { attachmentStore } from '../attachment_store';
import { invoke } from '@tauri-apps/api/core';
import type { Message, ServerMessage } from '../types';
import { parseLinkPreview, fromHex } from '../utils';
import { fromBase64, toBase64 } from '../crypto';
import { broadcastProfile } from './contacts';
import { addMessage, commitMessageUpdate, sendReceipt, syncChatToDb, bulkDelete, deleteMessage, downloadAttachment } from './message_utils';

export { addMessage, commitMessageUpdate, sendReceipt, syncChatToDb, bulkDelete, deleteMessage, downloadAttachment };

/**
 * Handles end-to-end encrypted messaging, media distribution, and fragment orchestration.
 */
export const setReplyingTo = (msg: Message | null) => userStore.update(s => ({ ...s, replyingTo: msg }));
export const typingTimeouts: Record<string, any> = {};


/**
 * Wraps arbitrary content in a standard message payload container.
 */
const createPayload = (type: string, content: any, id: string, replyTo?: any) => {
    return { type, content, id, replyTo, timestamp: Date.now() };
};

/**
 * Sends an end-to-end encrypted text message to a peer or group.
 */
export const sendMessage = async (destIdRaw: string, content: string) => {
    const destId = destIdRaw.toLowerCase();
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];
    if (state.blockedHashes.includes(destId)) return;

    try {
        // ONE SINGLE NATIVE CALL for both 1:1 and Groups
        await invoke<any>('process_outgoing_text', {
            payload: {
                recipient: destId,
                content,
                replyTo: state.replyingTo ? {
                    id: state.replyingTo.id,
                    content: state.replyingTo.content,
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
        console.error("Native send failed", e);
    }
};

/**
 * Encrypts and transmits a file using AES-GCM-256 and Signal key encapsulation.
 */
export const sendFile = async (destIdRaw: string, file: File) => {
    const destId = destIdRaw.toLowerCase();
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];

    const reader = new FileReader();
    reader.onload = async () => {
        const buffer = reader.result as ArrayBuffer;
        const uint8 = new Uint8Array(buffer);

        try {
            await invoke('process_outgoing_media', {
                payload: {
                    recipient: destId,
                    file_data: Array.from(uint8),
                    file_name: file.name,
                    file_type: file.type,
                    msg_type: 'file',
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
            console.error("[Messaging] Native media send failed:", e);
        }
    };
    reader.readAsArrayBuffer(file);
};

/**
 * Encrypts and transmits a file given its local filesystem path.
 * Optimal for large files as it avoids browser-layer memory buffers.
 */
export const sendLargeFile = async (destIdRaw: string, path: string, fileName: string) => {
    const destId = destIdRaw.toLowerCase();
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];

    // Basic MIME type detection for previews
    let fileType = 'application/octet-stream';
    const ext = fileName.split('.').pop()?.toLowerCase();
    if (ext === 'png') fileType = 'image/png';
    else if (ext === 'jpg' || ext === 'jpeg') fileType = 'image/jpeg';
    else if (ext === 'gif') fileType = 'image/gif';
    else if (ext === 'webp') fileType = 'image/webp';

    try {
        await invoke('process_outgoing_media', {
            payload: {
                recipient: destId,
                file_path: path,
                file_name: fileName,
                file_type: fileType,
                msg_type: 'file',
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
        console.error("[Messaging] Native large file send failed:", e);
    }
};

/**
 * Packages and transmits an audio recording as an encrypted voice note.
 */
export const sendVoiceNote = async (destIdRaw: string, audioBlob: Blob) => {
    const destId = destIdRaw.toLowerCase();
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];

    const buffer = await audioBlob.arrayBuffer();
    const uint8 = new Uint8Array(buffer);

    try {
        await invoke('process_outgoing_media', {
            payload: {
                recipient: destId,
                file_data: Array.from(uint8),
                file_name: 'voice_note.wav',
                file_type: 'audio/wav',
                msg_type: 'voice_note',
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
        console.error("[Messaging] Native voice note send failed:", e);
    }
};

