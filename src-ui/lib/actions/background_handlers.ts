import { listen } from '@tauri-apps/api/event';
import { userStore } from '../stores/user';
import { addMessage, updateMessageStatusUI } from './chat';
import type { Message } from '../types';
import { fromHex } from '../crypto';

/**
 * Orchestrates responses to background events emitted by the Rust networking layer.
 * This shifts the "heavy lifting" (decryption, fragment assembly, DB persistence) to Rust.
 */
export function setupBackgroundHandlers() {

    listen('new-relational-message', (event: any) => {
        const dbMsg = event.payload;

        const msg: Message = {
            id: dbMsg.id,
            timestamp: dbMsg.timestamp * 1000,
            senderHash: dbMsg.sender,
            content: dbMsg.body || "",
            type: dbMsg.type as any,
            isMine: false,
            status: dbMsg.status as any,
            replyTo: dbMsg.reply_to_id ? { id: dbMsg.reply_to_id, content: "", type: 'text' as any } : undefined
        };

        // Handle Voicenotes and Files correctly
        if ((msg.type === 'voice_note' || msg.type === 'file') && dbMsg.body) {
            try {
                const parsedBody = JSON.parse(dbMsg.body);
                if (parsedBody && (parsedBody.type === 'voice_note' || parsedBody.type === 'file')) {
                    const size = parsedBody.size || (parsedBody.bundle && parsedBody.bundle.file_size) || 0;
                    msg.content = msg.type === 'file' ? `File: ${parsedBody.bundle?.file_name || 'file'}` : "Voice Note";
                    msg.attachment = {
                        fileName: parsedBody.bundle?.file_name || (msg.type === 'voice_note' ? 'voice_note.wav' : 'file'),
                        fileType: parsedBody.bundle?.file_type || (msg.type === 'voice_note' ? 'audio/wav' : 'application/octet-stream'),
                        size: size,
                        bundle: parsedBody.bundle
                    };

                    if (parsedBody.data && msg.attachment) {
                        try {
                            msg.attachment.data = fromHex(parsedBody.data);
                        } catch (e) {
                            // Error decoding hex data
                        }
                    }
                }
            } catch (e) {
                // Could not parse dbMsg.body
            }
        }

        addMessage(dbMsg.chat_id, msg);
    });

    listen('peer-typing', (event: any) => {
        const { sender, isTyping } = event.payload;
        userStore.update(s => {
            if (s.chats[sender]) {
                s.chats[sender].isTyping = isTyping;
            }
            return { ...s, chats: { ...s.chats } };
        });
    });


    listen('receipt-update', (event: any) => {
        const { sender, status, msgIds } = event.payload;
        updateMessageStatusUI(sender, msgIds, status);
    });

    listen('contact-update', (event: any) => {
        const { hash, alias } = event.payload;
        userStore.update(s => {
            if (s.chats[hash]) {
                s.chats[hash].peerNickname = alias;
            }
            return { ...s, chats: { ...s.chats } };
        });
    });

    listen('group-invite', (event: any) => {
        const { groupId, name, members } = event.payload;
        userStore.update(s => {
            if (!s.chats[groupId]) {
                s.chats[groupId] = {
                    peerHash: groupId,
                    peerNickname: name,
                    unreadCount: 1,
                    isGroup: true,
                    members,
                    trustLevel: 1
                };
            }
            return { ...s, chats: { ...s.chats } };
        });
    });

    listen('group-update', (event: any) => {
        const { groupId, name, members } = event.payload;
        userStore.update(s => {
            if (s.chats[groupId]) {
                if (name) s.chats[groupId].peerNickname = name;
                if (members) s.chats[groupId].members = members;
            }
            return { ...s, chats: { ...s.chats } };
        });
    });

    listen('group-leave', (event: any) => {
        const { groupId, member } = event.payload;
        userStore.update(s => {
            if (s.chats[groupId] && s.chats[groupId].members) {
                s.chats[groupId].members = s.chats[groupId].members.filter(m => m !== member);
            }
            return { ...s, chats: { ...s.chats } };
        });
    });
}
