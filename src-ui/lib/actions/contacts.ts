
import { get } from 'svelte/store';
import { userStore, messageStore } from '../stores/user';
import { signalManager } from '../signal_manager';
import { network } from '../network';
import { invoke } from '@tauri-apps/api/core';
import { sendReceipt, syncChatToDb } from './chat';
import type { PrivacySettings } from '../types';

/**
 * Manages profile synchronization and contact metadata updates.
 */

/**
 * Transmits current user profile (alias) to a specific peer.
 */
export const broadcastProfile = async (peerHash: string) => {
    const state = get(userStore);
    if (!state.globalNickname) return;
    if (state.blockedHashes.includes(peerHash)) return;

    try {
        await invoke('send_profile_update', {
            peerHash,
            alias: state.globalNickname || undefined
        });
    } catch (e) {
        console.warn(`[Profile] Failed to broadcast to ${peerHash}:`, e);
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
        invoke('db_set_chat_pinned', { address: peerHash, is_pinned: nextPinned }).catch(console.error);
        syncChatToDb(s.chats[peerHash]);
    }
    return { ...s, chats: { ...s.chats } };
});

export const toggleArchive = (peerHash: string) => userStore.update(s => {
    if (s.chats[peerHash]) {
        const nextArchived = !s.chats[peerHash].isArchived;
        s.chats[peerHash] = { ...s.chats[peerHash], isArchived: nextArchived };
        invoke('db_set_chat_archived', { address: peerHash, is_archived: nextArchived }).catch(console.error);
        syncChatToDb(s.chats[peerHash]);
    }
    return { ...s, chats: { ...s.chats } };
});
export const setTrustLevel = async (peerHash: string, trustLevel: number) => {
    const state = get(userStore);
    if (!state.chats[peerHash]) return;

    try {
        // 1. Update Native Trust Store (Persistent Database)
        await signalManager.verifySession(peerHash, trustLevel);

        // 2. Update Local Store
        userStore.update(s => {
            if (s.chats[peerHash]) {
                s.chats[peerHash] = { ...s.chats[peerHash], trustLevel: trustLevel };
            }
            return { ...s, chats: { ...s.chats } };
        });
    } catch (e) {
        console.error("Failed to update trust level:", e);
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
                peerNickname: nickname || undefined
            };
        }
        return { ...s, chats: { ...s.chats } };
    });
    invoke('db_set_contact_nickname', { hash: peerHash, alias: nickname }).catch(console.error);
};

export const toggleBlock = (peerHash: string) => userStore.update(s => {
    const isBlocked = s.blockedHashes.includes(peerHash);
    const nextStatus = !isBlocked;

    if (nextStatus) s.blockedHashes = [...s.blockedHashes, peerHash];
    else s.blockedHashes = s.blockedHashes.filter(h => h !== peerHash);

    if (s.chats[peerHash]) {
        s.chats[peerHash] = { ...s.chats[peerHash], isBlocked: nextStatus };
    }

    invoke('db_set_contact_blocked', { hash: peerHash, is_blocked: nextStatus }).catch(console.error);
    return { ...s };
});

export const updatePrivacy = (settings: Partial<PrivacySettings>) => {
    userStore.update(s => {
        const oldTypingStatus = s.privacySettings.typingStatus;
        const newState = { ...s, privacySettings: { ...s.privacySettings, ...settings } };
        return newState;
    });
};

export const registerGlobalNickname = async (nickname: string) => {
    try {
        const response = await invoke<any>('register_nickname', { nickname });
        if (response.status === 'success') {
            userStore.update(s => {
                s.globalNickname = nickname;
                // Broadcast to active chats instead of loop-all
                Object.keys(s.chats).forEach(h => {
                    if (!s.chats[h].isGroup) broadcastProfile(h);
                });
                return { ...s };
            });
            return { success: true };
        }
        return { success: false, error: response.error };
    } catch (e: any) {
        return { success: false, error: e.toString() };
    }
};

export const lookupNickname = async (nickname: string): Promise<string | null> => {
    const input = nickname.trim();
    if (!input) return null;
    if (input.length === 64 && /^[0-9a-fA-F]+$/.test(input)) return input.toLowerCase();
    try {
        const data = await network.request('nickname_lookup', { name: input });
        return data?.identity_hash || null;
    } catch (e) {
        return null;
    }
};

/**
 * Queries the relay for the global nickname associated with a specific identity hash.
 */
export const resolveIdentity = async (peerHash: string): Promise<string | null> => {
    try {
        const data = await network.request('identity_resolve', { identity_hash: peerHash });
        if (data?.status === 'success' && data.nickname) {
            // Update the local chat store if this peer exists
            userStore.update(s => {
                if (s.chats[peerHash]) {
                    s.chats[peerHash] = { ...s.chats[peerHash], peerNickname: data.nickname };
                }
                return { ...s, chats: { ...s.chats } };
            });
            return data.nickname;
        }
        return null;
    } catch (e) {
        console.warn(`[Identity] Failed to resolve nickname for ${peerHash}:`, e);
        return null;
    }
};

export const startChat = (peerHashRaw: string, alias?: string) => {
    const peerHash = peerHashRaw.toLowerCase();
    userStore.update(s => {
        let chat = s.chats[peerHash];
        if (!chat) {
            chat = { 
                peerHash, 
                peerNickname: alias || peerHash.slice(0, 8), 
                unreadCount: 0, 
                trustLevel: 1 
            };
        } else if (alias) {
            chat.peerNickname = alias;
        }
        
        chat.unreadCount = 0;
        syncChatToDb(chat);
        
        s.chats[peerHash] = chat;
        return { ...s, activeChatHash: peerHash, chats: { ...s.chats } };
    });
    // 📣 FIRST CONTACT: Tell the new peer who we are immediately
    broadcastProfile(peerHashRaw);
};

// Profile updates are now automated via Rust Background Handlers + SQL persistence.
