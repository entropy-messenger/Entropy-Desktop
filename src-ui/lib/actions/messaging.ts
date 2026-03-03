
import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { signalManager } from '../signal_manager';
import { network } from '../network';
import { attachmentStore } from '../attachment_store';
import { invoke } from '@tauri-apps/api/core';
import type { Message, ServerMessage } from '../types';
import { parseLinkPreview, fromHex } from '../utils';
import { fromBase64, toBase64 } from '../crypto';
import { markOnline, broadcastProfile, statusTimeouts } from './contacts';
import { addMessage, commitMessageUpdate, sendReceipt, syncChatToDb } from './message_utils';

export { addMessage, bulkDelete, deleteMessage, downloadAttachment, sendReceipt } from './message_utils';

/**
 * Handles end-to-end encrypted messaging, media distribution, and fragment orchestration.
 */
export const setReplyingTo = (msg: Message | null) => userStore.update(s => ({ ...s, replyingTo: msg }));
export const typingTimeouts: Record<string, any> = {};

const MEDIA_CHUNK_SIZE = 100 * 1024;
const fragmentReassembly: Record<string, {
    total: number,
    received: number,
    chunks: Record<number, Uint8Array>,
    timestamp: number
}> = {};

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
        let replyToData = undefined;
        if (state.replyingTo) {
            replyToData = {
                id: state.replyingTo.id,
                content: state.replyingTo.content,
                sender_alias: state.replyingTo.senderAlias,
                type: state.replyingTo.type
            };
        }

        // ONE SINGLE NATIVE CALL for both 1:1 and Groups
        await invoke<any>('process_outgoing_text', {
            payload: {
                recipient: destId,
                content,
                reply_to: replyToData,
                is_group: !!chat?.isGroup,
                group_members: chat?.members || null
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
                    is_group: !!chat?.isGroup,
                    group_members: chat?.members || null,
                    reply_to: state.replyingTo ? {
                        id: state.replyingTo.id,
                        content: state.replyingTo.content,
                        sender_alias: state.replyingTo.senderAlias,
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
                is_group: !!chat?.isGroup,
                group_members: chat?.members || null,
                reply_to: state.replyingTo ? {
                    id: state.replyingTo.id,
                    content: state.replyingTo.content,
                    sender_alias: state.replyingTo.senderAlias,
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
                is_group: !!chat?.isGroup,
                group_members: chat?.members || null,
                reply_to: state.replyingTo ? {
                    id: state.replyingTo.id,
                    content: state.replyingTo.content,
                    sender_alias: state.replyingTo.senderAlias,
                    type: state.replyingTo.type
                } : null
            }
        });
        setReplyingTo(null);
    } catch (e) {
        console.error("[Messaging] Native voice note send failed:", e);
    }
};

/**
 * Processes a decrypted payload, updating the local store and handling secondary effects (typing, presence, etc).
 */
const processPayload = async (senderHash: string, payloadStr: string, groupId?: string, msgId?: string, replyToIn?: any) => {
    const state = get(userStore);
    if (state.blockedHashes.includes(senderHash)) return;

    let content = payloadStr;
    let type: Message['type'] = 'text';
    let attachment: any = undefined;
    let actualGroupId: string | undefined = groupId;
    let incomingMsgId = msgId || crypto.randomUUID();
    let replyTo = replyToIn;
    let linkPreview = undefined;

    try {
        const parsed = JSON.parse(payloadStr);
        if (parsed.id) incomingMsgId = parsed.id;
        if (parsed.replyTo) replyTo = parsed.replyTo;
        if (parsed.linkPreview) linkPreview = parsed.linkPreview;
        if (parsed.sender) senderHash = parsed.sender;

        if (parsed.type === 'group_invite' || parsed.type === 'group_invite_v2') {
            userStore.update(s => {
                if (!s.chats[parsed.groupId]) {
                    s.chats[parsed.groupId] = {
                        peerHash: parsed.groupId,
                        peerAlias: parsed.name,
                        messages: [],
                        unreadCount: 0,
                        isGroup: true,
                        members: parsed.members
                    };
                }
                return s;
            });

            const inviterAlias = state.chats[senderHash]?.localNickname || state.chats[senderHash]?.peerAlias || senderHash.slice(0, 8);
            const inviteMsg: Message = {
                id: incomingMsgId,
                timestamp: Date.now(),
                senderHash: senderHash,
                senderAlias: inviterAlias,
                content: `${inviterAlias} invited you to the group "${parsed.name}"`,
                type: 'system',
                groupId: parsed.groupId,
                isMine: false,
                status: 'delivered'
            };
            addMessage(parsed.groupId, inviteMsg);
            return;
        }

        if (parsed.type === 'group_leave') {
            const gid = parsed.groupId;
            const leaver = parsed.sender || senderHash;
            userStore.update(s => {
                const chat = s.chats[gid];
                if (chat && chat.isGroup && chat.members) {
                    chat.members = chat.members.filter(m => m !== leaver);
                    s.chats[gid] = { ...chat };
                }
                return { ...s, chats: { ...s.chats } };
            });

            const leaveMsg: Message = {
                id: incomingMsgId,
                timestamp: Date.now(),
                senderHash: leaver,
                senderAlias: state.chats[leaver]?.peerAlias || leaver.slice(0, 8),
                content: `${state.chats[leaver]?.peerAlias || leaver.slice(0, 8)} left the group`,
                type: 'system',
                groupId: gid,
                isMine: false,
                status: 'delivered'
            };
            addMessage(gid, leaveMsg);
            return;
        }

        if (parsed.type === 'group_update') {
            const gid = parsed.groupId;
            userStore.update(s => {
                if (s.chats[gid]) {
                    s.chats[gid].members = parsed.members;
                }
                return { ...s, chats: { ...s.chats } };
            });

            const updateMsg: Message = {
                id: incomingMsgId,
                timestamp: Date.now(),
                senderHash: senderHash,
                senderAlias: 'System',
                content: `Group membership updated`,
                type: 'system',
                groupId: gid,
                isMine: false,
                status: 'delivered'
            };
            addMessage(gid, updateMsg);
            return;
        }

        actualGroupId = parsed.groupId || groupId;
        if (parsed.type === 'group_message' || parsed.type === 'group_message_v2' || parsed.type === 'text_msg') {
            content = parsed.content || parsed.body || parsed.m || content;
        } else if (parsed.type === 'file' || parsed.type === 'voice_note') {
            type = (parsed.type === 'voice_note' || parsed.fileName === 'voice_note.wav') ? 'voice_note' : 'file';
            content = type === 'file' ? `File: ${parsed.fileName}` : "Voice Note";
            const attachmentData = fromBase64(parsed.data);
            attachment = {
                fileName: parsed.fileName || (parsed.type === 'voice_note' ? 'voice_note.wav' : 'file'),
                fileType: parsed.fileType || (parsed.type === 'voice_note' ? 'audio/wav' : 'application/octet-stream'),
                size: parsed.size || attachmentData.length,
                data: attachmentData
            };
            await attachmentStore.put(incomingMsgId, attachmentData);
        } else if (parsed.type === 'file_v2' || parsed.type === 'voice_note_v2') {
            type = (parsed.type === 'voice_note_v2' || (parsed.bundle && parsed.bundle.file_name === 'voice_note.wav')) ? 'voice_note' : 'file';
            const size = parsed.size || (parsed.bundle && parsed.bundle.file_size) || 0;
            content = type === 'file' ? `File: ${parsed.bundle.file_name}` : "Voice Note";
            attachment = {
                fileName: parsed.bundle.file_name,
                fileType: parsed.bundle.file_type,
                size: size,
                bundle: parsed.bundle,
                isV2: true
            };
            await attachmentStore.put(incomingMsgId, fromHex(parsed.data));
        } else if (parsed.type === 'typing') {
            if (get(userStore).privacySettings.lastSeen !== 'everyone') return;
            if (typingTimeouts[senderHash]) clearTimeout(typingTimeouts[senderHash]);

            userStore.update(s => {
                if (s.chats[senderHash]) {
                    const updated = { ...s.chats[senderHash] };
                    updated.isTyping = parsed.isTyping;
                    s.chats[senderHash] = updated;
                }
                return { ...s, chats: { ...s.chats } };
            });

            if (parsed.isTyping) {
                typingTimeouts[senderHash] = setTimeout(() => {
                    userStore.update(s => {
                        if (s.chats[senderHash]) {
                            const updated = { ...s.chats[senderHash] };
                            updated.isTyping = false;
                            s.chats[senderHash] = updated;
                        }
                        return { ...s, chats: { ...s.chats } };
                    });
                    delete typingTimeouts[senderHash];
                }, 6000);
            }
            return;
        } else if (parsed.type === 'presence') {
            if (state.privacySettings.lastSeen !== 'everyone') return;
            if (parsed.isOnline) {
                markOnline(senderHash);
                if (!state.chats[senderHash]?.pfp) broadcastProfile(senderHash);
            } else {
                if (statusTimeouts[senderHash]) clearTimeout(statusTimeouts[senderHash]);
                userStore.update(s => {
                    if (s.chats[senderHash]) {
                        const updated = { ...s.chats[senderHash] };
                        updated.isOnline = false;
                        updated.lastSeen = Date.now();
                        s.chats[senderHash] = updated;
                    }
                    return { ...s, chats: { ...s.chats } };
                });
            }
            return;
        } else if (parsed.type === 'profile_update') {
            userStore.update(s => {
                if (s.chats[senderHash]) {
                    const updated = { ...s.chats[senderHash] };
                    if (parsed.alias) updated.peerAlias = parsed.alias;
                    if (parsed.pfp) updated.pfp = parsed.pfp;
                    s.chats[senderHash] = updated;
                }
                return { ...s, chats: { ...s.chats } };
            });
            return;
        } else if (parsed.type === 'receipt') {
            if (parsed.status === 'read' && !state.privacySettings.readReceipts) return;
            const ids = Array.isArray(parsed.msgIds) ? parsed.msgIds : [parsed.msgId];

            // 1. Persist to DB
            try {
                await invoke('db_update_messages_status', { chatAddress: senderHash, ids, status: parsed.status });
            } catch (e) {
                console.error("[DB] Status update failed:", e);
            }

            // 2. Update reactive store
            userStore.update(s => {
                let anyChanged = false;
                // Clone chats for top-level reactivity
                const nextChats = { ...s.chats };

                for (const chatId in nextChats) {
                    const chat = { ...nextChats[chatId] };
                    let chatChanged = false;

                    // Update messages in memory if they exist
                    if (chat.messages && chat.messages.length > 0) {
                        const newMessages = chat.messages.map(m => {
                            if (ids.includes(m.id)) {
                                const oldPriority = m.status === 'read' ? 3 : m.status === 'delivered' ? 2 : 1;
                                const newPriority = parsed.status === 'read' ? 3 : parsed.status === 'delivered' ? 2 : 1;
                                if (newPriority > oldPriority) {
                                    chatChanged = true;
                                    return { ...m, status: parsed.status };
                                }
                            }
                            return m;
                        });
                        if (chatChanged) {
                            chat.messages = newMessages;
                            // Also update sidebar if it's the last one
                            const lastId = newMessages[newMessages.length - 1].id;
                            if (ids.includes(lastId)) {
                                chat.lastStatus = parsed.status;
                            }
                        }
                    } else if (chat.lastMsg) {
                        // Heuristic for sidebar preview even if messages aren't loaded
                        // For 1:1, the sender is the peer. For groups, we might need more info but we try anyway
                        if (chatId === senderHash || (parsed.groupId && chatId === parsed.groupId)) {
                            chat.lastStatus = parsed.status;
                            chatChanged = true;
                        }
                    }

                    if (chatChanged) {
                        anyChanged = true;
                        nextChats[chatId] = chat;
                        syncChatToDb(chat);
                    }
                }

                if (anyChanged) {
                    return { ...s, chats: nextChats };
                }
                return s;
            });
            return;
        }
    } catch (e) {
        console.error("[Messaging] Error in processPayload:", e);
    }

    const msg: Message = {
        id: incomingMsgId, timestamp: Date.now(), senderHash,
        senderAlias: state.chats[senderHash]?.peerAlias, content, type, attachment,
        groupId: actualGroupId, isMine: false, status: 'delivered', replyTo, linkPreview
    };
    addMessage(actualGroupId || senderHash, msg);

    if (!actualGroupId) {
        sendReceipt(senderHash, [incomingMsgId], 'delivered');
    }
};

/**
 * Entry point for all incoming network payloads.
 * Handles binary header extraction, fragmentation reassembly, and Signal decryption.
 */
export const handleIncomingMessage = async (payload: Uint8Array | ServerMessage, overrideSender?: string): Promise<void> => {
    try {
        const state = get(userStore);
        if (!state.identityHash) return;

        let senderHashPrefix: string | undefined = undefined;
        let incomingObj: any;

        if (payload instanceof Uint8Array) {
            let lastIndex = payload.length;
            while (lastIndex > 0 && payload[lastIndex - 1] === 0) lastIndex--;
            const trimmedPayload = payload.slice(0, lastIndex);

            if (trimmedPayload.length >= 64) {
                // Extract 64-byte header and trim null bytes
                const headerBytes = payload.slice(0, 64);
                let headerEnd = 0;
                while (headerEnd < 64 && headerBytes[headerEnd] !== 0) headerEnd++;

                const potentialHeader = new TextDecoder().decode(headerBytes.slice(0, headerEnd));

                // If it looks like a hash or alias, treat it as a header
                if (potentialHeader.length > 0 && /^[0-9a-fA-Z]+$/i.test(potentialHeader)) {
                    senderHashPrefix = potentialHeader;
                    const remainingData = trimmedPayload.slice(64);
                    const payloadStr = new TextDecoder().decode(remainingData);
                    try { incomingObj = JSON.parse(payloadStr); } catch (e) { return; }
                } else {
                    const payloadStr = new TextDecoder().decode(trimmedPayload);
                    try { incomingObj = JSON.parse(payloadStr); } catch (e) { return; }
                }
            } else {
                const payloadStr = new TextDecoder().decode(trimmedPayload);
                try { incomingObj = JSON.parse(payloadStr); } catch (e) { return; }
            }
        } else {
            incomingObj = payload;
        }

        if (!incomingObj) return;

        const skipTypes = ['relay_success', 'delivery_status', 'auth_success', 'error', 'ping', 'pong', 'dummy_ack', 'dummy_pacing'];
        if (incomingObj.type && typeof incomingObj.type === 'string' && skipTypes.includes(incomingObj.type)) {
            return;
        }

        if (incomingObj.type === 'binary_payload' && incomingObj.data_hex) {
            const decoded = fromHex(incomingObj.data_hex);
            return handleIncomingMessage(decoded, incomingObj.sender);
        }

        const finalSenderHash: string = (overrideSender || senderHashPrefix || incomingObj.sender || "unknown").toLowerCase();

        if (incomingObj.type === 'msg_fragment') {
            const fragId = incomingObj.fragmentId;
            if (!fragmentReassembly[fragId]) {
                fragmentReassembly[fragId] = {
                    total: incomingObj.total,
                    received: 0,
                    chunks: {},
                    timestamp: Date.now()
                };
            }
            const assembly = fragmentReassembly[fragId];
            if (assembly.chunks[incomingObj.index]) return;

            assembly.chunks[incomingObj.index] = fromBase64(incomingObj.data);
            assembly.received++;

            if (assembly.received % 10 === 0 || assembly.received === assembly.total) {
                console.debug(`[Messaging] Receiving fragment ${fragId}: ${assembly.received}/${assembly.total} chunks...`);
            }

            if (assembly.received === assembly.total) {
                setTimeout(() => {
                    if (!fragmentReassembly[fragId]) return;
                    console.log(`[Messaging] Reassembling fragment ${fragId} (${assembly.total} chunks)...`);
                    let totalLen = 0;
                    const chunkList = [];
                    for (let i = 0; i < assembly.total; i++) {
                        const chunk = assembly.chunks[i];
                        if (!chunk) return;
                        chunkList.push(chunk);
                        totalLen += chunk.length;
                    }
                    const fullData = new Uint8Array(totalLen);
                    let offset = 0;
                    for (const chunk of chunkList) {
                        fullData.set(chunk, offset);
                        offset += chunk.length;
                    }
                    delete fragmentReassembly[fragId];
                    handleIncomingMessage(fullData, finalSenderHash);
                }, 0);
                return;
            }
            return;
        }

        if (incomingObj.type === 'group_message_v2') {
            await processPayload(incomingObj.sender, incomingObj.body, incomingObj.groupId, incomingObj.id, incomingObj.replyTo);
            return;
        }

        // Recursively handle multi-layered payloads (e.g. fragments or volatile wrappers)
        if (incomingObj.type === 'volatile_relay') {
            const sender = incomingObj.from || finalSenderHash;
            let innerBody;
            try {
                innerBody = typeof incomingObj.body === 'string' ? JSON.parse(incomingObj.body) : incomingObj.body;
            } catch (e) {
                return;
            }
            return handleIncomingMessage(innerBody, sender);
        }

        if (incomingObj.type === 'binary_payload' && incomingObj.data_hex) {
            const decoded = fromHex(incomingObj.data_hex);
            return handleIncomingMessage(decoded, incomingObj.sender);
        }

        const decrypted = await signalManager.decrypt(finalSenderHash, incomingObj);

        if (decrypted) {
            // Privacy check: Should we mark them as online based on this interaction?
            // We skip marking online for volatile heartbeats/typing as they handle their own state
            // and for anyone if our own lastSeen is set to 'nobody' (reciprocity)
            const parsed = typeof decrypted === 'string' ? JSON.parse(decrypted) : decrypted;
            const isVolatile = parsed.type === 'presence' || parsed.type === 'typing' || parsed.type === 'receipt' || parsed.type === 'profile_update';

            // Passive online marking removed to respect stealth/privacy settings.
            // Online status is now exclusively driven by explicit presence updates.

            const bodyStr = typeof decrypted === 'string' ? decrypted : JSON.stringify(decrypted);
            const msgId = incomingObj.id || incomingObj.msgId || incomingObj.relay_msg_id;
            await processPayload(finalSenderHash, bodyStr, incomingObj.groupId, msgId);
        } else {
            // Filter out internal signaling types to avoid spamming the log
            const isNoisy = incomingObj.type === undefined || typeof incomingObj.type === 'number';
            if (!isNoisy && incomingObj.type !== 'dummy_pacing') {
                console.warn(`[Messaging] Failed to decrypt direct message from ${finalSenderHash}`, incomingObj);
            }
        }
    } catch (e) {
        console.error("[Messaging] Critical error in handleIncomingMessage:", e);
    }
};
