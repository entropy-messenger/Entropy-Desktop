
import { listen } from '@tauri-apps/api/event';
import { userStore } from '../stores/user';
import { addMessage } from './message_utils';
import type { Message } from '../types';
import { attachmentStore } from '../attachment_store';
import { fromHex } from '../crypto';

/**
 * Orchestrates responses to background events emitted by the Rust networking layer.
 * This shifts the "heavy lifting" (decryption, fragment assembly, DB persistence) to Rust.
 */
export function setupBackgroundHandlers() {
    console.log("[Background] Setting up event listeners for native orchestration.");

    listen('new-relational-message', (event: any) => {
        const dbMsg = event.payload;
        console.debug("[Background] Received new message from Rust:", dbMsg.id);

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

        // Handle Voicenotes and Files correctly!
        if ((msg.type === 'voice_note' || msg.type === 'file') && dbMsg.body) {
            try {
                const parsedBody = JSON.parse(dbMsg.body);
                // Depending on how Rust forwards the body, we check if it's the raw V2 payload
                if (parsedBody && (parsedBody.type === 'voice_note' || parsedBody.type === 'file')) {
                    const size = parsedBody.size || (parsedBody.bundle && parsedBody.bundle.file_size) || 0;
                    msg.content = msg.type === 'file' ? `File: ${parsedBody.bundle?.file_name || 'file'}` : "Voice Note";
                    msg.attachment = {
                        fileName: parsedBody.bundle?.file_name || (msg.type === 'voice_note' ? 'voice_note.wav' : 'file'),
                        fileType: parsedBody.bundle?.file_type || (msg.type === 'voice_note' ? 'audio/wav' : 'application/octet-stream'),
                        size: size,
                        bundle: parsedBody.bundle,
                        isV2: true
                    };

                    if (parsedBody.data) {
                        try {
                            const attachmentData = fromHex(parsedBody.data);
                            attachmentStore.put(msg.id, attachmentData).catch(e => console.error("[Background] Failed to save attachment:", e));
                            console.debug(`[Background] Extracted and stored ${attachmentData.length} bytes for attachment ${msg.id}`);
                        } catch (e) {
                            console.error("[Background] Error decoding hex data for attachment:", e);
                        }
                    } else {
                        console.warn("[Background] No encrypted hex data found in V2 payload for attachment.");
                    }
                }
            } catch (e) {
                // Not JSON or failed to parse, leave as is
                console.debug("[Background] Could not parse dbMsg.body as JSON for attachment recovery.");
            }
        }

        console.debug("[Background] Adding message to store:", msg.id, "Chat:", dbMsg.chat_id);
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
        import('./message_utils').then(m => m.updateMessageStatusUI(sender, msgIds, status));
    });

    listen('contact-update', (event: any) => {
        const { hash, alias } = event.payload;
        userStore.update(s => {
            if (s.chats[hash]) {
                s.chats[hash].peerAlias = alias;
            }
            return { ...s, chats: { ...s.chats } };
        });
    });

    listen('group-invite', (event: any) => {
        const { groupId, name, members, inviter } = event.payload;
        userStore.update(s => {
            if (!s.chats[groupId]) {
                s.chats[groupId] = {
                    peerHash: groupId,
                    peerAlias: name,
                    unreadCount: 1,
                    isGroup: true,
                    members
                };
            }
            return { ...s, chats: { ...s.chats } };
        });
    });
}
