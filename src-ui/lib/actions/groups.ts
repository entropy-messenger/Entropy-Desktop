import { get } from 'svelte/store';
import { userStore, messageStore } from '../stores/user';
import { invoke } from '@tauri-apps/api/core';

/**
 * Orchestrates group lifecycle via backend native commands.
 * This shifts member distribution and encryption to Rust for better performance and persistence.
 */
export const createGroup = async (name: string, members: string[]) => {
    const state = get(userStore);
    if (!state.identityHash) return;

    try {
        const groupId = await invoke<string>('create_group', { name, members });
        
        // Optimistically update the UI store. 
        // The backend also saves it to SQLite automatically.
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

/**
 * Extends an existing group by adding new members.
 */
export const addToGroup = async (groupId: string, newMembers: string[]) => {
    try {
        await invoke('add_to_group', { groupId, newMembers });
        
        // UI will be updated via the 'msg://group_update' event handler in background_handlers.ts
    } catch (e) {
        throw e;
    }
};

/**
 * Updates the group name for all members.
 */
export const updateGroupName = async (groupId: string, newName: string) => {
    try {
        await invoke('update_group_name', { groupId, newName });
        
        // Optimistically update local store
        userStore.update(s => {
            if (s.chats[groupId]) s.chats[groupId].localNickname = newName;
            return { ...s };
        });
    } catch (e) {
        throw e;
    }
};



/**
 * Removes the local user from a group and notifies remaining members.
 */
export const leaveGroup = async (groupId: string) => {
    try {
        await invoke('leave_group', { groupId });

        // Remove chat from local store
        userStore.update(s => {
            delete s.chats[groupId];
            if (s.activeChatHash === groupId) s.activeChatHash = null;
            return { ...s, chats: { ...s.chats } };
        });

        // Clear message history from memory
        messageStore.update(mStore => {
            delete mStore[groupId];
            return { ...mStore };
        });
    } catch (e) {
        throw e;
    }
};
