import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { signalManager } from '../signal_manager';
import { network } from '../network';
import { attachmentStore } from '../attachment_store';
import { callManager } from '../call_manager';
import { invoke } from '@tauri-apps/api/core';
import type { Message, ServerMessage } from '../types';
import { parseLinkPreview, fromHex } from '../utils';
import { fromBase64, toBase64 } from '../crypto';
import { markOnline, setOnlineStatus, broadcastProfile, statusTimeouts } from './contacts';
import { addMessage, sendReceipt } from './message_utils';

export { addMessage, bulkDelete, deleteMessage, downloadAttachment, sendReceipt } from './message_utils';

export const setReplyingTo = (msg: Message | null) => userStore.update(s => ({ ...s, replyingTo: msg }));

export const sendMessage = async (destId: string, content: string) => {
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];
    if (state.blockedHashes.includes(destId)) return;
    if (chat?.isGroup) return sendGroupMessage(destId, content);

    try {
        const msgId = crypto.randomUUID();
        const linkPreview = await parseLinkPreview(content);
        let replyToData = undefined;
        if (state.replyingTo) {
            replyToData = {
                id: state.replyingTo.id,
                content: state.replyingTo.content,
                senderAlias: state.replyingTo.senderAlias,
                type: state.replyingTo.type
            };
        }

        const payload = { type: 'text_msg', content, id: msgId, replyTo: replyToData, linkPreview: undefined };
        const ciphertextObj = await signalManager.encrypt(destId, JSON.stringify(payload), get(userStore).relayUrl);

        await invoke('protocol_save_pending', {
            msg: {
                id: msgId,
                recipient_hash: destId,
                body: JSON.stringify({
                    type: 'binary_routing',
                    to: destId,
                    body: ciphertextObj
                }),
                timestamp: Date.now(),
                retries: 0
            }
        });

        network.sendBinary(destId, new TextEncoder().encode(JSON.stringify(ciphertextObj)));

        const msg: Message = {
            id: msgId,
            timestamp: Date.now(),
            senderHash: state.identityHash,
            content,
            type: 'text',
            isMine: true,
            status: 'sent',
            replyTo: replyToData,
            linkPreview: undefined
        };
        addMessage(destId, msg);
        setReplyingTo(null);
    } catch (e) {
        console.error("Send failed", e);
    }
};

export const sendGroupMessage = async (groupId: string, content: string) => {
    const state = get(userStore);
    const group = state.chats[groupId];
    if (!group?.isGroup || !group.members) return;

    const msgId = crypto.randomUUID();
    const linkPreview = await parseLinkPreview(content);
    let replyToData = undefined;
    if (state.replyingTo) {
        replyToData = {
            id: state.replyingTo.id,
            content: state.replyingTo.content,
            senderAlias: state.replyingTo.senderAlias,
            type: state.replyingTo.type
        };
    }

    try {
        const ciphertext = await signalManager.groupEncrypt(groupId, content);
        const payload = {
            type: 'group_message_v2',
            groupId,
            sender: state.identityHash,
            body: ciphertext.body,
            nonce: ciphertext.nonce,
            key_id: ciphertext.key_id,
            id: msgId,
            replyTo: replyToData
        };
        network.sendJSON(payload);
    } catch (e) {
        console.error("Group Send Failed:", e);
    }

    const msg: Message = {
        id: msgId,
        timestamp: Date.now(),
        senderHash: state.identityHash!,
        content,
        type: 'text',
        groupId,
        isMine: true,
        status: 'sent',
        replyTo: replyToData,
        linkPreview: undefined
    };
    addMessage(groupId, msg);
    setReplyingTo(null);
};

export const sendFile = async (destId: string, file: File) => {
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];

    const reader = new FileReader();
    reader.onload = async () => {
        const buffer = reader.result as ArrayBuffer;
        const uint8 = new Uint8Array(buffer);


        const { ciphertext, bundle } = await signalManager.encryptMedia(uint8, file.name, file.type);
        const msgId = crypto.randomUUID();

        const contentObj = {
            type: 'file_v2',
            id: msgId,
            bundle,
            data: ciphertext
        };

        if (chat?.isGroup) {
            const targets = [];
            for (const member of chat.members!) {
                if (member === state.identityHash) continue;
                const payload = { ...contentObj, groupId: destId };
                const ciphertext = await signalManager.encrypt(member, JSON.stringify(payload), state.relayUrl);
                targets.push({ to: member, body: ciphertext.body, msg_type: ciphertext.type });
            }
            network.sendJSON({ type: 'group_multicast', targets });
        } else {
            const ciphertext = await signalManager.encrypt(destId, JSON.stringify(contentObj), state.relayUrl);
            await invoke('protocol_save_pending', {
                msg: {
                    id: msgId,
                    recipient_hash: destId,
                    body: JSON.stringify({ type: 'binary_routing', to: destId, body: ciphertext }),
                    timestamp: Date.now(),
                    retries: 0
                }
            });

            network.sendBinary(destId, new TextEncoder().encode(JSON.stringify(ciphertext)));
        }

        const msg: Message = {
            id: msgId,
            timestamp: Date.now(),
            senderHash: state.identityHash!,
            content: `File: ${file.name}`,
            type: 'file',
            groupId: chat?.isGroup ? destId : undefined,
            attachment: { fileName: file.name, fileType: file.type, size: file.size, data: uint8 },
            isMine: true, status: 'sent',
            linkPreview: undefined
        };
        addMessage(destId, msg);
    };
    reader.readAsArrayBuffer(file);
};

export const sendVoiceNote = async (destId: string, audioBlob: Blob) => {
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];

    const buffer = await audioBlob.arrayBuffer();
    const uint8 = new Uint8Array(buffer);
    const base64 = toBase64(uint8);
    const msgId = crypto.randomUUID();
    const contentObj = { type: 'voice_note', data: base64, id: msgId };

    if (chat?.isGroup) {
        const targets = [];
        for (const member of chat.members!) {
            if (member === state.identityHash) continue;
            const payload = { ...contentObj, groupId: destId };
            const ciphertext = await signalManager.encrypt(member, JSON.stringify(payload), state.relayUrl);
            targets.push({ to: member, body: ciphertext.body, msg_type: ciphertext.type });
        }
        network.sendJSON({ type: 'group_multicast', targets });
    } else {
        const ciphertext = await signalManager.encrypt(destId, JSON.stringify(contentObj), state.relayUrl);
        network.sendBinary(destId, new TextEncoder().encode(JSON.stringify(ciphertext)));
    }

    const msg: Message = {
        id: msgId,
        timestamp: Date.now(),
        senderHash: state.identityHash,
        content: "[Voice Note]",
        type: 'voice_note',
        groupId: chat?.isGroup ? destId : undefined,
        attachment: { fileName: 'voice_note.webm', fileType: 'audio/webm', size: uint8.length, data: uint8 },
        isMine: true,
        status: 'sent',
        linkPreview: undefined
    };
    addMessage(destId, msg);
};

const processPlaintext = async (senderHash: string, plaintext: string, groupId?: string, msgId?: string, replyToIn?: any) => {
    const state = get(userStore);
    if (state.blockedHashes.includes(senderHash)) return;

    let content = plaintext;
    let type: Message['type'] = 'text';
    let attachment: any = undefined;
    let actualGroupId: string | undefined = groupId;
    let incomingMsgId = msgId || crypto.randomUUID();
    let replyTo = replyToIn;
    let linkPreview = undefined;

    try {
        const parsed = JSON.parse(plaintext);
        if (parsed.id) incomingMsgId = parsed.id;
        if (parsed.replyTo) replyTo = parsed.replyTo;
        if (parsed.linkPreview) linkPreview = parsed.linkPreview;

        if (parsed.type === 'group_invite' || parsed.type === 'group_invite_v2') {
            userStore.update(s => {
                if (!s.chats[parsed.groupId]) {
                    s.chats[parsed.groupId] = { peerHash: parsed.groupId, peerAlias: parsed.name, messages: [], unreadCount: 1, isGroup: true, members: parsed.members };
                }
                return s;
            });
            if (parsed.type === 'group_invite_v2' && parsed.distribution) {
                await signalManager.processGroupDistribution(senderHash, parsed.distribution);
            }
            return;
        }

        actualGroupId = parsed.groupId || groupId;
        if (parsed.type === 'group_message' || parsed.type === 'group_message_v2' || parsed.type === 'text_msg') {
            content = parsed.content || parsed.m || content;
        } else if (parsed.type === 'file' || parsed.type === 'voice_note') {
            type = parsed.type;
            content = parsed.type === 'file' ? `File: ${parsed.fileName}` : "Voice Note";
            attachment = { fileName: parsed.fileName, fileType: parsed.fileType, data: fromBase64(parsed.data) };
        } else if (parsed.type === 'file_v2') {
            type = 'file';
            content = `File: ${parsed.bundle.file_name}`;
            attachment = {
                fileName: parsed.bundle.file_name,
                fileType: parsed.bundle.file_type,
                bundle: parsed.bundle,
                isV2: true
            };
            await attachmentStore.put(incomingMsgId, fromHex(parsed.data));
        } else if (parsed.type === 'typing') {
            userStore.update(s => {
                if (s.chats[senderHash]) s.chats[senderHash].isTyping = parsed.isTyping;
                return s;
            });
            return;
        } else if (parsed.type === 'presence') {
            if (parsed.isOnline) {
                markOnline(senderHash);
                if (!state.chats[senderHash]?.pfp) broadcastProfile(senderHash);
            } else {
                if (statusTimeouts[senderHash]) clearTimeout(statusTimeouts[senderHash]);
                userStore.update(s => {
                    if (s.chats[senderHash]) {
                        s.chats[senderHash].isOnline = false;
                        s.chats[senderHash].lastSeen = Date.now();
                    }
                    return s;
                });
            }
            return;
        } else if (parsed.type === 'profile_update') {
            userStore.update(s => {
                if (s.chats[senderHash]) {
                    if (parsed.alias) s.chats[senderHash].peerAlias = parsed.alias;
                    if (parsed.pfp) s.chats[senderHash].pfp = parsed.pfp;
                }
                return s;
            });
            return;
        } else if (parsed.type === 'receipt') {
            userStore.update(s => {
                const chat = s.chats[senderHash];
                if (chat) {
                    const ids = Array.isArray(parsed.msgIds) ? parsed.msgIds : [parsed.msgId];
                    ids.forEach((id: string) => {
                        const msg = chat.messages.find(m => m.id === id);
                        if (msg) {
                            if (parsed.status === 'read' || msg.status === 'sent') {
                                msg.status = parsed.status;
                            }
                        }
                    });
                    s.chats = { ...s.chats };
                }
                return s;
            });
            return;
        } else if (parsed.type === 'signaling') {
            callManager.handleSignaling(senderHash, parsed.data);
            return;
        } else if (parsed.type === 'disappearing_sync') {
            userStore.update(s => {
                if (s.chats[senderHash]) s.chats[senderHash].disappearingTimer = parsed.seconds || undefined;
                return { ...s, chats: { ...s.chats } };
            });
            return;
        } else if (parsed.type === 'call_log') {
            const msg: Message = {
                id: parsed.callId || incomingMsgId,
                timestamp: Date.now(),
                senderHash: senderHash,
                senderAlias: state.chats[senderHash]?.peerAlias,
                content: parsed.content,
                type: 'call_log',
                call_duration: parsed.duration,
                call_status: parsed.status,
                isMine: false,
                status: 'read'
            };
            addMessage(senderHash, msg);
            return;
        }
    } catch (e) { }

    const msg: Message = {
        id: incomingMsgId, timestamp: Date.now(), senderHash,
        senderAlias: state.chats[senderHash]?.peerAlias, content, type, attachment,
        groupId: actualGroupId, isMine: false, status: 'delivered', replyTo, linkPreview
    };
    addMessage(actualGroupId || senderHash, msg);

    if (!actualGroupId) {
        const currentActive = get(userStore).activeChatHash;
        const finalStatus = (currentActive === senderHash) ? 'read' : 'delivered';
        sendReceipt(senderHash, [incomingMsgId], finalStatus);

        if (finalStatus === 'read') {
            userStore.update(s => {
                if (s.chats[senderHash]) {
                    const m = s.chats[senderHash].messages.find(x => x.id === incomingMsgId);
                    if (m) m.status = 'read';
                }
                return s;
            });
        }
    }
};

export const handleIncomingMessage = async (payload: Uint8Array | ServerMessage) => {
    try {
        const state = get(userStore);
        if (!state.identityHash) return;

        let ciphertextObj: any;
        if (payload instanceof Uint8Array) {
            let lastIndex = payload.length;
            while (lastIndex > 0 && payload[lastIndex - 1] === 0) lastIndex--;
            const trimmedPayload = payload.slice(0, lastIndex);
            const payloadStr = new TextDecoder().decode(trimmedPayload);
            try { ciphertextObj = JSON.parse(payloadStr); } catch (e) { return; }
        } else {
            ciphertextObj = payload;
        }

        if (ciphertextObj.sealed) {
            try {
                const unsealed = await signalManager.unseal(ciphertextObj);
                ciphertextObj = unsealed.message;
                ciphertextObj.sender = unsealed.sender_hash || unsealed.sender;
            } catch (e) {
                console.error("Failed to unseal message:", e);
                return;
            }
        }

        if (ciphertextObj.type === 'group_message_v2') {
            const { groupId, sender, id } = ciphertextObj;
            try {
                const plaintext = await signalManager.groupDecrypt(groupId, sender, ciphertextObj);
                await processPlaintext(sender, plaintext, groupId, id, ciphertextObj.replyTo);
            } catch (e) {
                console.error("Failed to decrypt group message:", e);
            }
            return;
        }

        let senderHash = ciphertextObj.sender || "unknown";
        let signalMessage = ciphertextObj.message || ciphertextObj;
        if (!signalMessage.body && ciphertextObj.body) signalMessage = ciphertextObj;

        const isPreKey = (signalMessage.type === 3 || signalMessage.event === 3);
        let result: any = null;

        if (senderHash === "unknown" && !isPreKey) {
            for (const hash of Object.keys(state.chats)) {
                if (state.chats[hash].isGroup) continue;
                try {
                    const testResult = await signalManager.decrypt(hash, signalMessage);
                    if (testResult) { senderHash = hash; result = testResult; break; }
                } catch (e) { }
            }
        }

        try {
            if (!result) result = await signalManager.decrypt(senderHash, signalMessage);
        } catch (e: any) {
            if (e.toString().includes("IDENTITY_CHANGED")) {
                userStore.update(s => {
                    if (s.chats[senderHash]) s.chats[senderHash].isVerified = false;
                    return { ...s };
                });
                const msg: Message = {
                    id: crypto.randomUUID(), timestamp: Date.now(), senderHash,
                    content: "SECURITY ALERT: This contact's identity has changed. This could be a new device or a security threat.",
                    type: 'text', isMine: false, status: 'delivered'
                };
                addMessage(senderHash, msg);
            }
        }

        if (result && (result.m || result.type)) {
            await processPlaintext(result.s || senderHash, result.m, undefined, undefined, undefined);
        }
    } catch (e) { }
};
