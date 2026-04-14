import { get } from 'svelte/store';
import { userStore, messageStore } from '../stores/user';
import { invoke } from '@tauri-apps/api/core';

export const createGroup = async (name: string, members: string[]) => {
    const state = get(userStore);
    if (!state.identityHash) return;

    try {
        const groupId = await invoke<string>('create_group', { name, members });
        const allMembers = Array.from(new Set([state.identityHash.toLowerCase(), ...members.map(m => m.toLowerCase())]));
        userStore.update(s => {
            s.chats[groupId] = {
                peerHash: groupId,
                localNickname: name,
                unreadCount: 0,
                isGroup: true,
                members: allMembers,
                trustLevel: 1
            };
            return { ...s, activeChatHash: groupId };
        });
    } catch (e) {
        throw e;
    }
};

export const addToGroup = async (groupId: string, newMembers: string[]) => {
    try {
        await invoke('add_to_group', { groupId, newMembers });
    } catch (e) {
        throw e;
    }
};

export const updateGroupName = async (groupId: string, newName: string) => {
    try {
        await invoke('update_group_name', { groupId, newName });
        userStore.update(s => {
            if (s.chats[groupId]) s.chats[groupId].localNickname = newName;
            return { ...s };
        });
    } catch (e) {
        throw e;
    }
};

export const leaveGroup = async (groupId: string) => {
    try {
        await invoke('leave_group', { groupId });
        userStore.update(s => {
            delete s.chats[groupId];
            if (s.activeChatHash === groupId) s.activeChatHash = null;
            return { ...s, chats: { ...s.chats } };
        });
        messageStore.update(mStore => {
            delete mStore[groupId];
            return { ...mStore };
        });
    } catch (e) {
        throw e;
    }
};
