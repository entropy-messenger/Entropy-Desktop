import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { signalManager } from '../signal_manager';
import { invoke } from '@tauri-apps/api/core';

/**
 * Orchestrates group lifecycle, membership distribution, and invite signaling.
 */
export const createGroup = async (name: string, members: string[]) => {
    const state = get(userStore);
    if (!state.identityHash) return;

    const groupId = crypto.randomUUID();
    const allMembers = Array.from(new Set([state.identityHash.toLowerCase(), ...members.map(m => m.toLowerCase())]));

    userStore.update(s => {
        s.chats[groupId] = { 
            peerHash: groupId, 
            peerNickname: name, 
            unreadCount: 0, 
            isGroup: true, 
            members: allMembers,
            trustLevel: 1
        };
        return { ...s, activeChatHash: groupId };
    });

    const distMsg = await signalManager.groupInit(groupId);
    const invite = { type: 'group_invite', groupId, name, members: allMembers, distribution: distMsg };

    for (const member of members) {
        try {
            const ciphertext = await signalManager.encrypt(member, JSON.stringify(invite));
            await invoke('send_to_network', { 
                routingHash: member.split('.')[0], 
                msg: null, 
                data: Array.from(new TextEncoder().encode(JSON.stringify(ciphertext))), 
                isBinary: true, 
                isMedia: false 
            });
        } catch (e) { }
    }
};

/**
 * Extends an existing group by adding new members and synchronizing the membership list.
 */
export const addToGroup = async (groupId: string, newMembers: string[]) => {
    const state = get(userStore);
    const chat = state.chats[groupId];
    if (!chat || !chat.isGroup) return;

    const allMembers = [...(chat.members || []), ...newMembers];

    const uniqueMembers = Array.from(new Set(allMembers));

    userStore.update(s => {
        if (s.chats[groupId]) s.chats[groupId].members = uniqueMembers;
        return { ...s };
    });

    const distMsg = await signalManager.createGroupDistribution(groupId);
    const invite = { type: 'group_invite', groupId, name: chat.peerNickname, members: uniqueMembers, distribution: distMsg };
    const update = { type: 'group_update', groupId, members: uniqueMembers };

    // Invite new members
    for (const member of newMembers) {
        try {
            const ciphertext = await signalManager.encrypt(member, JSON.stringify(invite));
            await invoke('send_to_network', { 
                routingHash: member.split('.')[0], 
                msg: null, 
                data: Array.from(new TextEncoder().encode(JSON.stringify(ciphertext))), 
                isBinary: true, 
                isMedia: false 
            });
        } catch (e) { }
    }

    // Notify existing members
    for (const member of chat.members || []) {
        if (member === state.identityHash || newMembers.includes(member)) continue;
        try {
            const ciphertext = await signalManager.encrypt(member, JSON.stringify(update));
            await invoke('send_to_network', { 
                routingHash: member.split('.')[0], 
                msg: null, 
                data: Array.from(new TextEncoder().encode(JSON.stringify(ciphertext))), 
                isBinary: true, 
                isMedia: false 
            });
        } catch (e) { }
    }
};

/**
 * Removes the local user from a group and notifies remaining members.
 */
export const leaveGroup = async (groupId: string) => {
    const state = get(userStore);
    const chat = state.chats[groupId];
    if (!chat || !chat.isGroup) return;

    const payload = {
        type: 'group_leave',
        groupId,
        sender: state.identityHash
    };

    // Notify others
    for (const member of chat.members || []) {
        if (member === state.identityHash) continue;
        try {
            const ciphertext = await signalManager.encrypt(member, JSON.stringify(payload));
            await invoke('send_to_network', { 
                routingHash: member.split('.')[0], 
                msg: null, 
                data: Array.from(new TextEncoder().encode(JSON.stringify(ciphertext))), 
                isBinary: true, 
                isMedia: false 
            });
        } catch (e) { }
    }

    // Remove chat from local store
    userStore.update(s => {
        delete s.chats[groupId];
        if (s.activeChatHash === groupId) s.activeChatHash = null;
        return { ...s, chats: { ...s.chats } };
    });
};
