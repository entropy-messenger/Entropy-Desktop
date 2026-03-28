
import { get } from 'svelte/store';
import { userStore, messageStore } from '../stores/user';
import { signalManager } from '../signal_manager';
import { network } from '../network';
import { minePoW } from '../crypto';
import { invoke } from '@tauri-apps/api/core';
import { bulkDelete, sendReceipt, syncChatToDb } from './message_utils';
import type { PrivacySettings } from '../types';

/**
 * Manages profile synchronization and contact metadata updates.
 */

export const updateMyProfile = (alias: string, pfp: string | null) => {
    userStore.update(s => ({ ...s, myAlias: alias, myPfp: pfp }));
    const state = get(userStore);
    Object.keys(state.chats).forEach(peerHash => {
        if (!state.chats[peerHash].isGroup) broadcastProfile(peerHash);
    });
};

/**
 * Transmits current user profile (alias/pfp) to a specific peer.
 */
export const broadcastProfile = async (peerHash: string) => {
    const state = get(userStore);
    if (!state.myAlias && !state.myPfp) return;
    if (state.blockedHashes.includes(peerHash)) return;
    if (state.privacySettings.typingStatus !== 'everyone') return;

    try {
        await invoke('send_profile_update', {
            peerHash,
            alias: state.myAlias || undefined,
            pfp: state.myPfp || undefined
        });
    } catch (e) {
        console.error(`[Profile] Failed to broadcast to ${peerHash}:`, e);
    }
};

export const sendTypingStatus = async (peerHash: string, isTyping: boolean) => {
    const state = get(userStore);
    if (state.chats[peerHash]?.isGroup || state.blockedHashes.includes(peerHash)) return;
    if (state.privacySettings.typingStatus !== 'everyone') return;

    try {
        await invoke('send_typing_status', { peerHash, isTyping });
    } catch (e) {
        console.error(`[Typing] Failed to send to ${peerHash}:`, e);
    }
};


export const togglePin = (peerHash: string) => userStore.update(s => {
    if (s.chats[peerHash]) {
        const nextPinned = !s.chats[peerHash].isPinned;
        s.chats[peerHash] = { ...s.chats[peerHash], isPinned: nextPinned };
        invoke('db_set_chat_pinned', { address: peerHash, isPinned: nextPinned }).catch(console.error);
        syncChatToDb(s.chats[peerHash]);
    }
    return { ...s, chats: { ...s.chats } };
});

export const toggleArchive = (peerHash: string) => userStore.update(s => {
    if (s.chats[peerHash]) {
        const nextArchived = !s.chats[peerHash].isArchived;
        s.chats[peerHash] = { ...s.chats[peerHash], isArchived: nextArchived };
        invoke('db_set_chat_archived', { address: peerHash, isArchived: nextArchived }).catch(console.error);
        syncChatToDb(s.chats[peerHash]);
    }
    return { ...s, chats: { ...s.chats } };
});
export const toggleVerification = async (peerHash: string, verified?: boolean) => {
    const state = get(userStore);
    const chat = state.chats[peerHash];
    if (!chat || chat.isGroup) return;

    const nextStatus = verified !== undefined ? verified : !chat.isVerified;

    try {
        // 1. Update Native Trust Store (Persistent Database)
        await signalManager.verifySession(peerHash, nextStatus);

        // 2. Update Local Store
        userStore.update(s => {
            if (s.chats[peerHash]) {
                s.chats[peerHash] = { ...s.chats[peerHash], isVerified: nextStatus };
            }
            return { ...s, chats: { ...s.chats } };
        });
    } catch (e) {
        console.error("Failed to update verification status:", e);
    }
};
export const toggleStar = (peerHash: string, msgId: string) => {
    messageStore.update(mStore => {
        if (!mStore[peerHash]) return mStore;
        
        const msgs = [...mStore[peerHash]];
        const idx = msgs.findIndex(m => m.id === msgId);
        if (idx !== -1) {
            const nextStarred = !msgs[idx].isStarred;
            msgs[idx] = { ...msgs[idx], isStarred: nextStarred };
            invoke('db_set_message_starred', { id: msgId, isStarred: nextStarred }).catch(console.error);
            return { ...mStore, [peerHash]: msgs };
        }
        return mStore;
    });
};


export const setLocalNickname = (peerHash: string, nickname: string | null) => {
    userStore.update(s => {
        if (s.chats[peerHash]) {
            s.chats[peerHash] = {
                ...s.chats[peerHash],
                localNickname: nickname || undefined
            };
        }
        return { ...s, chats: { ...s.chats } };
    });
};

export const bulkStar = (peerHash: string, msgIds: string[]) => {
    messageStore.update(mStore => {
        const msgs = mStore[peerHash];
        if (msgs) {
            msgs.forEach(m => {
                if (msgIds.includes(m.id)) {
                    m.isStarred = true;
                    invoke('db_set_message_starred', { id: m.id, isStarred: true }).catch(console.error);
                }
            });
        }
        return { ...mStore };
    });
};

export const toggleBlock = (peerHash: string) => userStore.update(s => {
    const isBlocked = s.blockedHashes.includes(peerHash);
    const nextStatus = !isBlocked;

    if (nextStatus) s.blockedHashes = [...s.blockedHashes, peerHash];
    else s.blockedHashes = s.blockedHashes.filter(h => h !== peerHash);

    if (s.chats[peerHash]) {
        s.chats[peerHash] = { ...s.chats[peerHash], isBlocked: nextStatus };
    }

    invoke('db_set_contact_blocked', { hash: peerHash, isBlocked: nextStatus }).catch(console.error);
    return { ...s };
});

export const updatePrivacy = (settings: Partial<PrivacySettings>) => {
    userStore.update(s => {
        const oldTypingStatus = s.privacySettings.typingStatus;
        const newState = { ...s, privacySettings: { ...s.privacySettings, ...settings } };

        if (settings.typingStatus && oldTypingStatus !== settings.typingStatus) {
            // Typing status visibility changed. 
        }

        return newState;
    });
};

/**
 * Registers a global nickname on the relay server.
 * Requires PoW validation and a signed proof of identity.
 */
export const registerGlobalNickname = async (nickname: string) => {
    const state = get(userStore);
    if (!state.identityHash) return;

    try {
        // Fetch challenge via WebSocket
        const challenge = await network.request('pow_challenge', { nickname, identity_hash: state.identityHash });
        const { seed, difficulty } = challenge;
        const { nonce } = await minePoW(seed, difficulty, nickname);

        const signature = await signalManager.signMessage(nickname);

        const response = await network.request('nickname_register', {
            nickname,
            identity_hash: state.identityHash,
            signature,
            seed,
            nonce
        });

        if (response.status === 'success') {
            console.log("Global nickname registered:", nickname);
            userStore.update(s => ({ ...s, myAlias: nickname }));
            return { success: true };
        } else {
            console.error("Nickname registration failed:", response.error);
            return { success: false, error: response.error };
        }
    } catch (e) {
        console.error("Nickname registration error:", e);
        return { success: false, error: "Network error" };
    }
};

/**
 * Retrieves the identity hash associated with a global nickname.
 */
export const lookupNickname = async (nickname: string): Promise<string | null> => {
    const input = nickname.trim();
    if (!input) return null;

    if (input.length === 64 && /^[0-9a-fA-F]+$/.test(input)) {
        return input.toLowerCase();
    }

    try {
        const data = await network.request('nickname_lookup', { name: input });
        if (data && !data.error) {
            return data.identity_hash || null;
        }
        return null;
    } catch (e) {
        return null;
    }
};

export const verifyContact = async (peerHash: string, isVerified: boolean) => {
};

export const startChat = (peerHashRaw: string, alias?: string) => {
    const peerHash = peerHashRaw.toLowerCase();
    userStore.update(s => {
        let chat = s.chats[peerHash];
        if (!chat) {
            chat = {
                peerHash,
                peerAlias: alias || peerHash.slice(0, 8),
                unreadCount: 0,
                isSynced: false
            } as any;
        } else if (alias && chat.peerAlias === chat.peerHash.slice(0, 8)) {
            chat = { ...chat, peerAlias: alias };
        }

        const msgs = get(messageStore)[peerHash] || [];
        const unreadIds = msgs.filter(m => !m.isMine && m.status !== 'read').map(m => m.id);

        // ALWAYS RESET AND PERSIST UNREAD STATUS
        chat = { ...chat, unreadCount: 0 };
        syncChatToDb(chat);

        if (unreadIds.length > 0) {
            sendReceipt(peerHash, unreadIds, 'read');
            // Update UI store first for responsiveness
            messageStore.update(mStore => {
                if (mStore[peerHash]) {
                    mStore[peerHash] = mStore[peerHash].map(m => unreadIds.includes(m.id) ? { ...m, status: 'read' } : m);
                }
                return { ...mStore };
            });
            // Persist read status to relational DB
            invoke('db_update_messages_status', { chatAddress: peerHash, ids: unreadIds, status: 'read' }).catch(e => console.error("[DB] Failed to update read status:", e));
        }

        s.chats[peerHash] = chat;
        return { ...s, activeChatHash: peerHash, chats: { ...s.chats } };
    });
};


export const updateAlias = (peerHash: string, newAlias: string) => {
    userStore.update(s => { if (s.chats[peerHash]) s.chats[peerHash].peerAlias = newAlias; return s; });
};
