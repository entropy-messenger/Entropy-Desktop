import { get } from 'svelte/store';
import { userStore, messageStore } from '../stores/user';
import { signalManager } from '../signal_manager';
import { invoke } from '@tauri-apps/api/core';
import { loadChatMessages, syncChatToDb } from './chat';
import type { PrivacySettings } from '../types';



export const broadcastProfile = async (peerHash: string) => {
    const state = get(userStore);
    if (!state.globalNickname) return;
    if (state.blockedHashes.includes(peerHash)) return;
    try {
        await invoke('send_profile_update', { peerHash, alias: state.globalNickname || undefined });
    } catch (e) {
        // Failed to broadcast
    }
};

export const sendTypingStatus = async (peerHash: string, isTyping: boolean) => {
    const state = get(userStore);
    if (state.chats[peerHash]?.isGroup || state.blockedHashes.includes(peerHash)) return;
    if (state.privacySettings.typingStatus !== 'everyone') return;
    try {
        await invoke('send_typing_status', { peerHash, isTyping });
    } catch (e) {

    }
};

export const togglePin = (peerHash: string) => userStore.update(s => {
    if (s.chats[peerHash]) {
        const nextPinned = !s.chats[peerHash].isPinned;
        s.chats[peerHash] = { ...s.chats[peerHash], isPinned: nextPinned };
        invoke('db_set_chat_pinned', { address: peerHash, isPinned: nextPinned }).catch(() => { });
        syncChatToDb(s.chats[peerHash]);
    }
    return { ...s, chats: { ...s.chats } };
});

export const toggleArchive = (peerHash: string) => userStore.update(s => {
    if (s.chats[peerHash]) {
        const nextArchived = !s.chats[peerHash].isArchived;
        s.chats[peerHash] = { ...s.chats[peerHash], isArchived: nextArchived };
        invoke('db_set_chat_archived', { address: peerHash, isArchived: nextArchived }).catch(() => { });
        syncChatToDb(s.chats[peerHash]);
    }
    return { ...s, chats: { ...s.chats } };
});

export const setTrustLevel = async (peerHash: string, trustLevel: number) => {
    try {
        await signalManager.verifySession(peerHash, trustLevel);
        userStore.update(s => {
            if (s.chats[peerHash]) s.chats[peerHash].trustLevel = trustLevel;
            return { ...s, chats: { ...s.chats } };
        });
    } catch (e) {

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
            invoke('db_update_messages', { ids: [msgId], isStarred: nextStarred }).catch(() => { });
            return { ...mStore, [peerHash]: msgs };
        }
        return mStore;
    });
};

export const setLocalNickname = (peerHash: string, nickname: string | null) => {
    userStore.update(s => {
        if (s.chats[peerHash]) s.chats[peerHash].localNickname = nickname || undefined;
        // Priority update: Local Alias > Global Nickname
        if (nickname) {
            s.nicknames[peerHash] = nickname;
        } else {
            const global = s.chats[peerHash]?.globalNickname;
            if (global) s.nicknames[peerHash] = global;
            else delete s.nicknames[peerHash];
        }
        return { ...s, chats: { ...s.chats }, nicknames: { ...s.nicknames } };
    });
    invoke('db_set_contact_nickname', { hash: peerHash, alias: nickname }).catch(() => { });
};

export const toggleBlock = (peerHash: string) => userStore.update(s => {
    const isBlocked = s.blockedHashes.includes(peerHash);
    const nextStatus = !isBlocked;
    if (nextStatus) s.blockedHashes = [...s.blockedHashes, peerHash];
    else s.blockedHashes = s.blockedHashes.filter(h => h !== peerHash);
    if (s.chats[peerHash]) s.chats[peerHash].isBlocked = nextStatus;
    invoke('db_set_contact_blocked', { hash: peerHash, isBlocked: nextStatus }).catch(() => { });
    return { ...s };
});

export const updatePrivacy = (settings: Partial<PrivacySettings>) => {
    userStore.update(s => ({ ...s, privacySettings: { ...s.privacySettings, ...settings } }));
};

export const registerGlobalNickname = async (nickname: string) => {
    try {
        const response = await invoke<any>('register_nickname', { nickname });
        if (response.status === 'success') {
            userStore.update(s => {
                s.globalNickname = nickname;
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
        const res = await invoke<any>('nickname_lookup', { name: input });
        return res?.identity_hash || null;
    } catch (e) {
        return null;
    }
};

const pendingResolutions = new Set<string>();

export const resolveIdentity = async (peerHash: string): Promise<string | null> => {
    if (pendingResolutions.has(peerHash)) return null;
    pendingResolutions.add(peerHash);

    try {
        const res = await invoke<any>('identity_resolve', { identityHash: peerHash });
        const name = res?.nickname;
        if (name) {
            invoke('db_set_contact_global_nickname', { hash: peerHash, nickname: name }).catch(() => { });

            userStore.update(s => {
                if (s.chats[peerHash]) s.chats[peerHash].globalNickname = name;
                if (!s.chats[peerHash]?.localNickname) {
                    s.nicknames[peerHash] = name;
                }
                return { ...s, chats: { ...s.chats }, nicknames: { ...s.nicknames } };
            });
            return name;
        }
        return null;
    } catch (e) {
        return null;
    } finally {
        pendingResolutions.delete(peerHash);
    }
};

export const startChat = (peerHashRaw: string, alias?: string) => {
    const peerHash = peerHashRaw.toLowerCase();
    userStore.update(s => {
        let chat = s.chats[peerHash];
        if (!chat) {
            chat = { peerHash, localNickname: alias || undefined, unreadCount: 0, trustLevel: 1 };
        } else if (alias) {
            chat.localNickname = alias;
        }

        const display = chat.localNickname || chat.globalNickname;
        if (display) s.nicknames[peerHash] = display;

        chat.unreadCount = 0;
        syncChatToDb(chat);
        s.chats[peerHash] = chat;
        return { ...s, activeChatHash: peerHash, chats: { ...s.chats } };
    });
    broadcastProfile(peerHashRaw);
};
