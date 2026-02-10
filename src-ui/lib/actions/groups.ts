import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { signalManager } from '../signal_manager';
import { network } from '../network';

/**
 * Orchestrates group lifecycle, membership distribution, and invite signaling.
 */
export const createGroup = async (name: string, members: string[]) => {
    const state = get(userStore);
    if (!state.identityHash) return;

    const groupId = crypto.randomUUID();
    const allMembers = [state.identityHash, ...members];

    userStore.update(s => {
        s.chats[groupId] = { peerHash: groupId, peerAlias: name, messages: [], unreadCount: 0, isGroup: true, members: allMembers };
        return { ...s, activeChatHash: groupId };
    });

    const distMsg = await signalManager.groupInit(groupId);
    const invite = { type: 'group_invite_v2', groupId, name, members: allMembers, distribution: distMsg };

    for (const member of members) {
        try {
            const ciphertext = await signalManager.encrypt(member, JSON.stringify(invite), get(userStore).relayUrl, true);
            network.sendBinary(member, new TextEncoder().encode(JSON.stringify(ciphertext)));
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
    const invite = { type: 'group_invite_v2', groupId, name: chat.peerAlias, members: uniqueMembers, distribution: distMsg };

    for (const member of newMembers) {
        try {
            const ciphertext = await signalManager.encrypt(member, JSON.stringify(invite), get(userStore).relayUrl, true);
            network.sendBinary(member, new TextEncoder().encode(JSON.stringify(ciphertext)));
        } catch (e) { }
    }
};
