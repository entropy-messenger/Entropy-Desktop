
import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { signalManager } from '../signal_manager';
import { network } from '../network';
import { minePoW } from '../crypto';
import { bulkDelete, sendReceipt } from './message_utils';
import type { PrivacySettings } from '../types';

/**
 * Manages peer presence, profile synchronization, and contact metadata updates.
 */
export const statusTimeouts: Record<string, any> = {};
let heartbeatInterval: any = null;

export const markOnline = (peerHash: string) => {
    if (statusTimeouts[peerHash]) clearTimeout(statusTimeouts[peerHash]);

    userStore.update(s => {
        if (s.chats[peerHash]) {
            const updated = { ...s.chats[peerHash] };
            updated.isOnline = true;
            updated.lastSeen = undefined;
            s.chats[peerHash] = updated;
        }
        return { ...s, chats: { ...s.chats } };
    });

    statusTimeouts[peerHash] = setTimeout(() => {
        userStore.update(s => {
            if (s.chats[peerHash]) {
                const updated = { ...s.chats[peerHash] };
                updated.isOnline = false;
                updated.lastSeen = Date.now();
                s.chats[peerHash] = updated;
            }
            return { ...s, chats: { ...s.chats } };
        });
        delete statusTimeouts[peerHash];
    }, 25000);
};

/**
 * Initiates the background heartbeat for presence broadcasting and message expiry.
 */
export const startHeartbeat = () => {
    if (heartbeatInterval) clearInterval(heartbeatInterval);

    heartbeatInterval = setInterval(() => {
        const state = get(userStore);
        if (state.identityHash && state.isConnected) {
            Object.keys(state.chats).forEach(peerHash => {
                if (!state.chats[peerHash].isGroup && state.privacySettings.lastSeen === 'everyone' && state.privacySettings.readReceipts) {
                    setOnlineStatus(peerHash, true);
                }
            });
        } else if (!state.isConnected) {
            userStore.update(s => {
                Object.keys(s.chats).forEach(h => s.chats[h].isOnline = false);
                return s;
            });
        }
    }, 12000);
};

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

    const profile = {
        type: 'profile_update',
        alias: state.myAlias,
        pfp: state.myPfp
    };

    try {
        const ciphertext = await signalManager.encrypt(peerHash, JSON.stringify(profile), get(userStore).relayUrl, true);
        network.sendVolatile(peerHash, new TextEncoder().encode(JSON.stringify(ciphertext)));
    } catch (e) { }
};

export const sendTypingStatus = async (peerIdentityHash: string, isTyping: boolean) => {
    const state = get(userStore);
    if (state.chats[peerIdentityHash]?.isGroup || state.blockedHashes.includes(peerIdentityHash)) return;
    if (!state.privacySettings.readReceipts) return;

    const contentObj = { type: 'typing', isTyping };
    const ciphertextObj = await signalManager.encrypt(peerIdentityHash, JSON.stringify(contentObj), get(userStore).relayUrl, true);
    network.sendVolatile(peerIdentityHash, new TextEncoder().encode(JSON.stringify(ciphertextObj)));
};

export const setOnlineStatus = async (peerIdentityHash: string, isOnline: boolean) => {
    const state = get(userStore);
    if (state.blockedHashes.includes(peerIdentityHash) || !state.privacySettings.readReceipts) return;
    const contentObj = { type: 'presence', isOnline };
    const ciphertextObj = await signalManager.encrypt(peerIdentityHash, JSON.stringify(contentObj), get(userStore).relayUrl, true);
    network.sendVolatile(peerIdentityHash, new TextEncoder().encode(JSON.stringify(ciphertextObj)));
};

export const togglePin = (peerHash: string) => userStore.update(s => { if (s.chats[peerHash]) s.chats[peerHash] = { ...s.chats[peerHash], isPinned: !s.chats[peerHash].isPinned }; return { ...s, chats: { ...s.chats } }; });
export const toggleArchive = (peerHash: string) => userStore.update(s => { if (s.chats[peerHash]) s.chats[peerHash] = { ...s.chats[peerHash], isArchived: !s.chats[peerHash].isArchived }; return { ...s, chats: { ...s.chats } }; });
export const toggleMute = (peerHash: string) => userStore.update(s => { if (s.chats[peerHash]) s.chats[peerHash] = { ...s.chats[peerHash], isMuted: !s.chats[peerHash].isMuted }; return { ...s, chats: { ...s.chats } }; });
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
export const toggleStar = (peerHash: string, msgId: string) => userStore.update(s => {
    if (s.chats[peerHash]) {
        s.chats[peerHash] = {
            ...s.chats[peerHash],
            messages: s.chats[peerHash].messages.map(m =>
                m.id === msgId ? { ...m, isStarred: !m.isStarred } : m
            )
        };
    }
    return { ...s, chats: { ...s.chats } };
});


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

export const bulkStar = (peerHash: string, msgIds: string[]) => userStore.update(s => {
    if (s.chats[peerHash]) {
        s.chats[peerHash].messages.forEach(m => { if (msgIds.includes(m.id)) m.isStarred = true; });
    }
    return { ...s, chats: { ...s.chats } };
});

export const toggleBlock = (peerHash: string) => userStore.update(s => {
    const isBlocked = s.blockedHashes.includes(peerHash);
    if (isBlocked) s.blockedHashes = s.blockedHashes.filter(h => h !== peerHash);
    else s.blockedHashes = [...s.blockedHashes, peerHash];
    return { ...s };
});

export const updatePrivacy = (settings: Partial<PrivacySettings>) => userStore.update(s => ({ ...s, privacySettings: { ...s.privacySettings, ...settings } }));

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
        return input;
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

export const startChat = (peerHash: string, alias?: string) => {
    userStore.update(s => {
        if (!s.chats[peerHash]) {
            s.chats[peerHash] = {
                peerHash,
                peerAlias: alias || peerHash.slice(0, 8),
                messages: [],
                unreadCount: 0
            };
        } else if (alias && s.chats[peerHash].peerAlias === s.chats[peerHash].peerHash.slice(0, 8)) {
            s.chats[peerHash].peerAlias = alias;
        }

        const unreadIds: string[] = [];
        s.chats[peerHash].messages.forEach(m => {
            if (!m.isMine && m.status !== 'read') {
                m.status = 'read';
                unreadIds.push(m.id);
            }
        });

        s.chats[peerHash].unreadCount = 0;
        if (unreadIds.length > 0) {
            sendReceipt(peerHash, unreadIds, 'read');
        }

        return { ...s, activeChatHash: peerHash, chats: { ...s.chats } };
    });
};

export const updateAlias = (peerHash: string, newAlias: string) => {
    userStore.update(s => { if (s.chats[peerHash]) s.chats[peerHash].peerAlias = newAlias; return s; });
};
