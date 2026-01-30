
import { writable, get } from 'svelte/store';
import type { UserIdentity, Chat, Message, ServerMessage, PrivacySettings } from './types';
import { signalManager } from './signal_manager';
import { network } from './network';
import { minePoW, initCrypto, toBase64, fromBase64, encryptBinary, decryptBinary } from './crypto';
import { callManager } from './call_manager';
import { attachmentStore } from './attachment_store';
import { parseLinkPreview } from './utils';

import { userStore } from './user_store';
import type { AppState } from './user_store';
import { invoke } from '@tauri-apps/api/core';
import { secureLoad, secureStore, initVault, vaultLoad, vaultSave } from './secure_storage';

userStore.subscribe(s => {
    if (s.connectionStatus === 'connected' && s.keysMissing) {
        
        signalManager.ensureKeysUploaded(s.relayUrl, true).catch(e => console.error("Re-upload failed:", e));
        userStore.update(state => ({ ...state, keysMissing: false }));
    }
});

const statusTimeouts: Record<string, any> = {};
let heartbeatInterval: any = null;

const markOnline = (peerHash: string) => {
    if (statusTimeouts[peerHash]) clearTimeout(statusTimeouts[peerHash]);

    userStore.update(s => {
        if (s.chats[peerHash]) {
            s.chats[peerHash].isOnline = true;
            s.chats[peerHash].lastSeen = undefined;
        }
        return s;
    });

    statusTimeouts[peerHash] = setTimeout(() => {
        userStore.update(s => {
            if (s.chats[peerHash]) {
                s.chats[peerHash].isOnline = false;
                s.chats[peerHash].lastSeen = Date.now();
            }
            return s;
        });
        delete statusTimeouts[peerHash];
    }, 40000);
};

export const fromHex = (hex: string): Uint8Array => {
    return new Uint8Array(hex.match(/.{1,2}/g)!.map(byte => parseInt(byte, 16)));
};

export const toHex = (bytes: Uint8Array): string => {
    return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
};

export const downloadAttachment = async (msgId: string, bundle: any) => {
    try {
        const encrypted = await attachmentStore.get(msgId);
        if (!encrypted) throw new Error("Attachment not found locally");

        const decrypted = await signalManager.decryptMedia(toHex(encrypted), bundle);

        const blob = new Blob([decrypted as any], { type: bundle.file_type || 'application/octet-stream' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = bundle.file_name || 'download';
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
    } catch (e) {
        console.error("Download failed:", e);
    }
};

export const initApp = async (password: string) => {
    userStore.update(s => ({ ...s, authError: null }));
    await initCrypto();
    const salt = await secureLoad('entropy_vault_salt');
    const attemptsKey = salt ? `entropy_failed_attempts_${salt.slice(0, 8)}` : 'entropy_failed_attempts_global';

    try {
        await initVault(password);
    } catch (e) {
        console.error("Vault init failed (wrong password?):", e);
        handleFailedAttempt(attemptsKey);
        return;
    }

    let idHash: string | null = null;
    try {
        idHash = await signalManager.init(password, false);
    } catch (e) {
        console.error("Signal init failed:", e);
    }

    if (idHash) {
        let chats: Record<string, Chat> = {};
        let myAlias: string | null = null;
        let myPfp: string | null = null;
        let sessionToken: string | null = null;

        const saved = await vaultLoad(`entropy_chats_${idHash}`);
        const vaultKey = await signalManager.getLocalEncryptionKey(password);
        if (vaultKey) {
            let storageKey = vaultKey; 
            attachmentStore.setEncryptionKey(storageKey);

            if (saved) {
                if (saved) {
                    try {
                        const vault = JSON.parse(saved);
                        const rawChats = vault.chats || vault;
                        
                        for (const h in rawChats) {
                            rawChats[h].isOnline = false;
                            rawChats[h].isTyping = false;
                        }
                        chats = rawChats;
                        myAlias = vault.myAlias || null;
                        myPfp = vault.myPfp || null;
                        sessionToken = vault.sessionToken || null;

                        
                        localStorage.removeItem(attemptsKey);
                        localStorage.removeItem(`entropy_failed_attempts_${idHash}`);
                    } catch (e) {
                        userStore.update(s => ({ ...s, authError: "Corrupted vault data." }));
                        return;
                    }
                }
            }
        }
        storageKey = vaultKey;
        userStore.update(s => ({ ...s, identityHash: idHash, chats, myAlias, myPfp, sessionToken, authError: null }));
        network.connect();
        startHeartbeat();

        const serverUrl = get(userStore).relayUrl;
        try { await signalManager.ensureKeysUploaded(serverUrl); } catch (e) { }
        signalManager.replenishPreKeys(serverUrl).catch(e => console.error("Prekey replenishment failed:", e));
    } else {
        handleFailedAttempt(attemptsKey);
    }
};

const handleFailedAttempt = (key: string) => {
    const attempts = parseInt(localStorage.getItem(key) || "0") + 1;
    localStorage.setItem(key, attempts.toString());

    if (attempts >= 10) {
        
        invoke('nuclear_reset').catch(() => { });
        const keys = [];
        for (let i = 0; i < localStorage.length; i++) {
            const k = localStorage.key(i);
            if (k && (k.startsWith('entropy_') || k.startsWith('signal_'))) keys.push(k);
        }
        keys.forEach(k => localStorage.removeItem(k));
        userStore.update(s => ({ ...s, authError: "Vault wiped after 10 failed attempts." }));
    } else {
        userStore.update(s => ({ ...s, authError: `Wrong password. Attempts: ${attempts}/10` }));
    }
};



const startHeartbeat = () => {
    if (heartbeatInterval) clearInterval(heartbeatInterval);
    heartbeatInterval = setInterval(() => {
        const state = get(userStore);
        if (state.identityHash && state.isConnected) {
            Object.keys(state.chats).forEach(peerHash => {
                if (!state.chats[peerHash].isGroup && state.privacySettings.lastSeen === 'everyone') {
                    setOnlineStatus(peerHash, true);
                }

                
            });
        } else if (!state.isConnected) {
            userStore.update(s => {
                Object.keys(s.chats).forEach(h => s.chats[h].isOnline = false);
                return s;
            });
        }
    }, 30000);

    
    setInterval(() => {
        const state = get(userStore);
        const now = Date.now();

        Object.keys(state.chats).forEach(h => {
            const chat = state.chats[h];
            if (chat.disappearingTimer && chat.disappearingTimer > 0) {
                const expiryTime = chat.disappearingTimer * 1000;
                const expiredIds = chat.messages
                    .filter(m => !m.isStarred && (now - m.timestamp) >= expiryTime)
                    .map(m => m.id);

                if (expiredIds.length > 0) {
                    bulkDelete(h, expiredIds);
                }
            }
        });
    }, 3000);
};

export const createIdentity = async (password: string) => {
    try {
        console.log("Starting identity creation...");
        await initCrypto();
        console.log("Crypto initialized.");
        await initVault(password);
        console.log("Vault initialized.");
    } catch (e: any) {
        console.error("Vault initialization failed:", e);
        throw new Error(`Local vault setup failed: ${e.message || e}`);
    }

    let idHash;
    try {
        console.log("Initializing Signal manager...");
        idHash = await signalManager.init(password, true);
        console.log("Signal identity generated:", idHash);
    } catch (e: any) {
        console.error("Identity generation failed:", e);
        throw new Error(`Cryptographic identity generation failed: ${e.message || e}`);
    }

    if (idHash) {
        try {
            let vaultKey = await signalManager.getLocalEncryptionKey(password);
            if (vaultKey) {
                storageKey = vaultKey;
                attachmentStore.setEncryptionKey(storageKey);
            }
            userStore.update(s => ({ ...s, identityHash: idHash }));

            console.log("Connecting to network...");
            network.connect();
            startHeartbeat();

            console.log("Uploading keys to server...");
            await signalManager.ensureKeysUploaded(get(userStore).relayUrl);
            console.log("Keys uploaded.");
        } catch (e: any) {
            console.warn("Post-creation tasks failed (non-critical):", e);
        }
    } else {
        throw new Error("Identity generation returned null.");
    }
};

let isAuthInProgress = false;

export const authenticate = async (identityHash: string) => {
    if (isAuthInProgress) return;
    isAuthInProgress = true;

    try {

        const state = get(userStore);
        const serverUrl = state.relayUrl;

        if (state.sessionToken) {
            console.log("Attempting session-token authentication...");
            userStore.update(s => ({ ...s, connectionStatus: 'connecting' }));
            network.sendJSON({
                type: 'auth',
                payload: {
                    identity_hash: identityHash,
                    session_token: state.sessionToken
                }
            });
        } else {
            console.log("No session token. Starting PoW mining...");
            userStore.update(s => ({ ...s, connectionStatus: 'mining' }));
            const challengeRes = await fetch(`${serverUrl}/pow/challenge?identity_hash=${identityHash}`);
            const { seed, difficulty } = await challengeRes.json();

            
            const pow = await minePoW(seed, difficulty, identityHash);

            network.sendJSON({
                type: 'auth',
                payload: { identity_hash: identityHash, seed: pow.seed, nonce: pow.nonce }
            });
        }


        
        setTimeout(() => {
            const state = get(userStore);
            Object.keys(state.chats).forEach(peerHash => {
                if (!state.chats[peerHash].isGroup) {
                    setOnlineStatus(peerHash, true);
                    broadcastProfile(peerHash);
                }
            });
        }, 1000);
    } catch (e) {
        console.error("Authentication failed:", e);
    } finally {
        isAuthInProgress = false;
    }
};


export const updateMyProfile = (alias: string, pfp: string | null) => {
    userStore.update(s => ({ ...s, myAlias: alias, myPfp: pfp }));
    const state = get(userStore);
    Object.keys(state.chats).forEach(peerHash => {
        if (!state.chats[peerHash].isGroup) broadcastProfile(peerHash);
    });
};

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

    const contentObj = { type: 'typing', isTyping };
    const ciphertextObj = await signalManager.encrypt(peerIdentityHash, JSON.stringify(contentObj), get(userStore).relayUrl, true);
    network.sendVolatile(peerIdentityHash, new TextEncoder().encode(JSON.stringify(ciphertextObj)));
};

export const setOnlineStatus = async (peerIdentityHash: string, isOnline: boolean) => {
    const state = get(userStore);
    if (state.blockedHashes.includes(peerIdentityHash)) return;
    const contentObj = { type: 'presence', isOnline };
    const ciphertextObj = await signalManager.encrypt(peerIdentityHash, JSON.stringify(contentObj), get(userStore).relayUrl, true);
    network.sendVolatile(peerIdentityHash, new TextEncoder().encode(JSON.stringify(ciphertextObj)));
};

export const togglePin = (peerHash: string) => userStore.update(s => { if (s.chats[peerHash]) s.chats[peerHash].isPinned = !s.chats[peerHash].isPinned; return { ...s, chats: { ...s.chats } }; });
export const toggleArchive = (peerHash: string) => userStore.update(s => { if (s.chats[peerHash]) s.chats[peerHash].isArchived = !s.chats[peerHash].isArchived; return { ...s, chats: { ...s.chats } }; });
export const toggleMute = (peerHash: string) => userStore.update(s => { if (s.chats[peerHash]) s.chats[peerHash].isMuted = !s.chats[peerHash].isMuted; return { ...s, chats: { ...s.chats } }; });
export const toggleVerification = (peerHash: string) => userStore.update(s => { if (s.chats[peerHash]) s.chats[peerHash].isVerified = !s.chats[peerHash].isVerified; return { ...s, chats: { ...s.chats } }; });
export const toggleStar = (peerHash: string, msgId: string) => userStore.update(s => {
    if (s.chats[peerHash]) {
        const msg = s.chats[peerHash].messages.find(m => m.id === msgId);
        if (msg) msg.isStarred = !msg.isStarred;
    }
    return { ...s, chats: { ...s.chats } };
});
export const setDisappearingTimer = async (peerHash: string, seconds: number | null) => {
    userStore.update(s => {
        if (s.chats[peerHash]) s.chats[peerHash].disappearingTimer = seconds || undefined;
        return { ...s, chats: { ...s.chats } };
    });

    
    const syncMsg = { type: 'disappearing_sync', seconds };
    try {
        const ciphertext = await signalManager.encrypt(peerHash, JSON.stringify(syncMsg), get(userStore).relayUrl, true);
        network.sendBinary(peerHash, new TextEncoder().encode(JSON.stringify(ciphertext)));
    } catch (e) { }
};

export const setLocalNickname = (peerHash: string, nickname: string | null) => {
    userStore.update(s => {
        if (s.chats[peerHash]) s.chats[peerHash].localNickname = nickname || undefined;
        return { ...s, chats: { ...s.chats } };
    });
};

export const setReplyingTo = (msg: Message | null) => userStore.update(s => ({ ...s, replyingTo: msg }));

export const bulkStar = (peerHash: string, msgIds: string[]) => userStore.update(s => {
    if (s.chats[peerHash]) {
        s.chats[peerHash].messages.forEach(m => { if (msgIds.includes(m.id)) m.isStarred = true; });
    }
    return { ...s, chats: { ...s.chats } };
});

export const bulkDelete = (peerHash: string, msgIds: string[]) => {
    msgIds.forEach(id => attachmentStore.delete(id).catch(() => { }));
    userStore.update(s => {
        if (s.chats[peerHash]) {
            s.chats[peerHash].messages = s.chats[peerHash].messages.filter(m => !msgIds.includes(m.id));
        }
        return { ...s, chats: { ...s.chats } };
    });
};

export const deleteMessage = (peerHash: string, msgId: string) => bulkDelete(peerHash, [msgId]);


export const toggleBlock = (peerHash: string) => userStore.update(s => {
    const isBlocked = s.blockedHashes.includes(peerHash);
    if (isBlocked) s.blockedHashes = s.blockedHashes.filter(h => h !== peerHash);
    else s.blockedHashes = [...s.blockedHashes, peerHash];
    return { ...s };
});

export const updatePrivacy = (settings: Partial<PrivacySettings>) => userStore.update(s => ({ ...s, privacySettings: { ...s.privacySettings, ...settings } }));

export const refreshDecoys = async (serverUrl: string) => {
    await signalManager.refreshDecoyPool(serverUrl);
};

export const burnAccount = async (serverUrl: string) => {
    if (confirm("DANGER: This will permanently purge your account from the server AND your local device. This cannot be undone. Are you absolutely sure?")) {
        const success = await signalManager.remoteBurn(serverUrl);
        if (success) {
            window.location.reload();
        } else {
            alert("Forensic burn failed. The server might be unreachable.");
        }
    }
};

export const sendReceipt = async (peerHash: string, msgIds: string[], status: 'delivered' | 'read') => {
    const state = get(userStore);
    if (state.blockedHashes.includes(peerHash)) return;
    if (status === 'read' && !state.privacySettings.readReceipts) return;
    if (msgIds.length === 0) return;
    const receipt = { type: 'receipt', msgIds, status };
    try {
        const ciphertext = await signalManager.encrypt(peerHash, JSON.stringify(receipt), get(userStore).relayUrl, true);
        network.sendVolatile(peerHash, new TextEncoder().encode(JSON.stringify(ciphertext)));
    } catch (e) { }
};

export const sendMessage = async (destId: string, content: string) => {
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];
    if (state.blockedHashes.includes(destId)) return;
    if (chat?.isGroup) return sendGroupMessage(destId, content);

    try {
        const msgId = crypto.randomUUID();
        const linkPreview = await parseLinkPreview(content);
        let replyToData = undefined;
        if (state.replyingTo) {
            replyToData = {
                id: state.replyingTo.id,
                content: state.replyingTo.content,
                senderAlias: state.replyingTo.senderAlias,
                type: state.replyingTo.type
            };
        }

        const payload = { type: 'text_msg', content, id: msgId, replyTo: replyToData, linkPreview: undefined };
        const ciphertextObj = await signalManager.encrypt(destId, JSON.stringify(payload), get(userStore).relayUrl);

        
        await invoke('protocol_save_pending', {
            msg: {
                id: msgId,
                recipient_hash: destId,
                body: JSON.stringify({
                    type: 'binary_routing', 
                    to: destId,
                    body: ciphertextObj
                }),
                timestamp: Date.now(),
                retries: 0
            }
        });

        network.sendBinary(destId, new TextEncoder().encode(JSON.stringify(ciphertextObj)));

        const msg: Message = {
            id: msgId,
            timestamp: Date.now(),
            senderHash: state.identityHash,
            content,
            type: 'text',
            isMine: true,
            status: 'sent',
            replyTo: replyToData,
            linkPreview: undefined
        };
        addMessage(destId, msg);
        setReplyingTo(null);
    } catch (e) {
        console.error("Send failed", e);
    }
};

export const verifyContact = async (peerHash: string, isVerified: boolean) => {
    await signalManager.verifySession(peerHash, isVerified);
    userStore.update(s => {
        if (s.chats[peerHash]) s.chats[peerHash].isVerified = isVerified;
        return { ...s };
    });
};

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

export const sendGroupMessage = async (groupId: string, content: string) => {
    const state = get(userStore);
    const group = state.chats[groupId];
    if (!group?.isGroup || !group.members) return;

    const msgId = crypto.randomUUID();
    const linkPreview = await parseLinkPreview(content);
    let replyToData = undefined;
    if (state.replyingTo) {
        replyToData = {
            id: state.replyingTo.id,
            content: state.replyingTo.content,
            senderAlias: state.replyingTo.senderAlias,
            type: state.replyingTo.type
        };
    }

    try {
        const ciphertext = await signalManager.groupEncrypt(groupId, content);
        const payload = {
            type: 'group_message_v2',
            groupId,
            sender: state.identityHash,
            body: ciphertext.body,
            nonce: ciphertext.nonce,
            key_id: ciphertext.key_id,
            id: msgId,
            replyTo: replyToData
        };
        network.sendJSON(payload);
    } catch (e) {
        console.error("Group Send Failed:", e);
    }

    const msg: Message = {
        id: msgId,
        timestamp: Date.now(),
        senderHash: state.identityHash!,
        content,
        type: 'text',
        groupId,
        isMine: true,
        status: 'sent',
        replyTo: replyToData,
        linkPreview: undefined
    };
    addMessage(groupId, msg);
    setReplyingTo(null);
};

export const sendFile = async (destId: string, file: File) => {
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];

    const reader = new FileReader();
    reader.onload = async () => {
        const buffer = reader.result as ArrayBuffer;
        const uint8 = new Uint8Array(buffer);

        
        const { ciphertext, bundle } = await signalManager.encryptMedia(uint8, file.name, file.type);
        const msgId = crypto.randomUUID();

        const contentObj = {
            type: 'file_v2',
            id: msgId,
            bundle,
            data: ciphertext 
        };

        if (chat?.isGroup) {
            
            const targets = [];
            for (const member of chat.members!) {
                if (member === state.identityHash) continue;
                const payload = { ...contentObj, groupId: destId };
                const ciphertext = await signalManager.encrypt(member, JSON.stringify(payload), state.relayUrl);
                targets.push({ to: member, body: ciphertext.body, msg_type: ciphertext.type });
            }
            network.sendJSON({ type: 'group_multicast', targets });
        } else {
            const ciphertext = await signalManager.encrypt(destId, JSON.stringify(contentObj), state.relayUrl);

            
            await invoke('protocol_save_pending', {
                msg: {
                    id: msgId,
                    recipient_hash: destId,
                    body: JSON.stringify({ type: 'binary_routing', to: destId, body: ciphertext }),
                    timestamp: Date.now(),
                    retries: 0
                }
            });

            network.sendBinary(destId, new TextEncoder().encode(JSON.stringify(ciphertext)));
        }

        const msg: Message = {
            id: msgId,
            timestamp: Date.now(),
            senderHash: state.identityHash!,
            content: `File: ${file.name}`,
            type: 'file',
            groupId: chat?.isGroup ? destId : undefined,
            attachment: { fileName: file.name, fileType: file.type, size: file.size, data: uint8 },
            isMine: true, status: 'sent',
            linkPreview: undefined
        };
        addMessage(destId, msg);
    };
    reader.readAsArrayBuffer(file);
};

export const sendVoiceNote = async (destId: string, audioBlob: Blob) => {
    const state = get(userStore);
    if (!state.identityHash) return;
    const chat = state.chats[destId];

    const buffer = await audioBlob.arrayBuffer();
    const uint8 = new Uint8Array(buffer);
    const base64 = toBase64(uint8);
    const msgId = crypto.randomUUID();
    const contentObj = { type: 'voice_note', data: base64, id: msgId };

    if (chat?.isGroup) {
        const targets = [];
        for (const member of chat.members!) {
            if (member === state.identityHash) continue;
            const payload = { ...contentObj, groupId: destId };
            const ciphertext = await signalManager.encrypt(member, JSON.stringify(payload), state.relayUrl);
            targets.push({ to: member, body: ciphertext.body, msg_type: ciphertext.type });
        }
        network.sendJSON({ type: 'group_multicast', targets });
    } else {
        const ciphertext = await signalManager.encrypt(destId, JSON.stringify(contentObj), state.relayUrl);
        network.sendBinary(destId, new TextEncoder().encode(JSON.stringify(ciphertext)));
    }

    const msg: Message = {
        id: msgId,
        timestamp: Date.now(),
        senderHash: state.identityHash,
        content: "[Voice Note]",
        type: 'voice_note',
        groupId: chat?.isGroup ? destId : undefined,
        attachment: { fileName: 'voice_note.webm', fileType: 'audio/webm', size: uint8.length, data: uint8 },
        isMine: true,
        status: 'sent',
        linkPreview: undefined
    };
    addMessage(destId, msg);
};


export const registerGlobalNickname = async (nickname: string) => {
    const state = get(userStore);
    if (!state.identityHash) return;

    try {
        const serverUrl = get(userStore).relayUrl;
        const challengeRes = await fetch(`${serverUrl}/pow/challenge?nickname=${encodeURIComponent(nickname)}&identity_hash=${state.identityHash}`);
        const { seed, difficulty } = await challengeRes.json();
        const { nonce } = await minePoW(seed, difficulty, nickname);
        const signature = await signalManager.signMessage(nickname);

        const response = await fetch(`${serverUrl}/nickname/register`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'X-PoW-Seed': seed,
                'X-PoW-Nonce': nonce.toString()
            },
            body: JSON.stringify({
                nickname,
                identity_hash: state.identityHash,
                identityKey: await signalManager.getPublicIdentityKey(),
                signature
            })
        });


        const result = await response.json();
        if (result.status === 'success') {
            console.log("Global nickname registered:", nickname);
            userStore.update(s => ({ ...s, myAlias: nickname }));
            return { success: true };
        } else {
            console.error("Nickname registration failed:", result.error);
            return { success: false, error: result.error };
        }
    } catch (e) {
        console.error("Nickname registration error:", e);
        return { success: false, error: "Network error" };
    }
};

export const lookupNickname = async (nickname: string): Promise<string | null> => {
    const useDecoys = get(userStore).privacySettings.decoyMode;
    let names = [nickname];
    if (useDecoys) {
        
        names = [nickname, "alice", "crypto", "signal", "privacy", "matrix"].sort(() => 0.5 - Math.random()).slice(0, 4);
        if (!names.includes(nickname)) names[0] = nickname;
    }

    try {
        const serverUrl = get(userStore).relayUrl;
        const response = await fetch(`${serverUrl}/nickname/lookup?name=${names.map(n => encodeURIComponent(n)).join(',')}`);
        if (response.status === 200) {
            const data = await response.json();
            return data[nickname] || data.identity_hash || null;
        }
        return null;
    } catch (e) {
        return null;
    }
};


const processPlaintext = async (senderHash: string, plaintext: string, groupId?: string, msgId?: string, replyToIn?: any) => {
    const state = get(userStore);
    if (state.blockedHashes.includes(senderHash)) return;

    let content = plaintext;
    let type: Message['type'] = 'text';
    let attachment: any = undefined;
    let actualGroupId: string | undefined = groupId;
    let incomingMsgId = msgId || crypto.randomUUID();
    let replyTo = replyToIn;
    let linkPreview = undefined;

    try {
        const parsed = JSON.parse(plaintext);
        if (parsed.id) incomingMsgId = parsed.id;
        if (parsed.replyTo) replyTo = parsed.replyTo;
        if (parsed.linkPreview) linkPreview = parsed.linkPreview;

        if (parsed.type === 'group_invite' || parsed.type === 'group_invite_v2') {
            userStore.update(s => {
                if (!s.chats[parsed.groupId]) {
                    s.chats[parsed.groupId] = { peerHash: parsed.groupId, peerAlias: parsed.name, messages: [], unreadCount: 1, isGroup: true, members: parsed.members };
                }
                return s;
            });
            if (parsed.type === 'group_invite_v2' && parsed.distribution) {
                await signalManager.processGroupDistribution(senderHash, parsed.distribution);
            }
            return;
        }

        actualGroupId = parsed.groupId || groupId;
        if (parsed.type === 'group_message' || parsed.type === 'group_message_v2' || parsed.type === 'text_msg') {
            content = parsed.content || parsed.m || content;
        } else if (parsed.type === 'file' || parsed.type === 'voice_note') {
            type = parsed.type;
            content = parsed.type === 'file' ? `File: ${parsed.fileName}` : "Voice Note";
            attachment = { fileName: parsed.fileName, fileType: parsed.fileType, data: fromBase64(parsed.data) };
        } else if (parsed.type === 'file_v2') {
            type = 'file';
            content = `File: ${parsed.bundle.file_name}`;
            attachment = {
                fileName: parsed.bundle.file_name,
                fileType: parsed.bundle.file_type,
                bundle: parsed.bundle,
                isV2: true
            };
            await attachmentStore.put(incomingMsgId, fromHex(parsed.data));
        } else if (parsed.type === 'typing') {
            userStore.update(s => {
                if (s.chats[senderHash]) s.chats[senderHash].isTyping = parsed.isTyping;
                return s;
            });
            return;
        } else if (parsed.type === 'presence') {
            if (parsed.isOnline) {
                markOnline(senderHash);
                if (!state.chats[senderHash]?.pfp) broadcastProfile(senderHash);
            } else {
                if (statusTimeouts[senderHash]) clearTimeout(statusTimeouts[senderHash]);
                userStore.update(s => {
                    if (s.chats[senderHash]) {
                        s.chats[senderHash].isOnline = false;
                        s.chats[senderHash].lastSeen = Date.now();
                    }
                    return s;
                });
            }
            return;
        } else if (parsed.type === 'profile_update') {
            userStore.update(s => {
                if (s.chats[senderHash]) {
                    if (parsed.alias) s.chats[senderHash].peerAlias = parsed.alias;
                    if (parsed.pfp) s.chats[senderHash].pfp = parsed.pfp;
                }
                return s;
            });
            return;
        } else if (parsed.type === 'receipt') {
            userStore.update(s => {
                const chat = s.chats[senderHash];
                if (chat) {
                    const ids = Array.isArray(parsed.msgIds) ? parsed.msgIds : [parsed.msgId];
                    ids.forEach((id: string) => {
                        const msg = chat.messages.find(m => m.id === id);
                        if (msg) {
                            if (parsed.status === 'read' || msg.status === 'sent') {
                                msg.status = parsed.status;
                            }
                        }
                    });
                    s.chats = { ...s.chats };
                }
                return s;
            });
            return;
        } else if (parsed.type === 'signaling') {
            callManager.handleSignaling(senderHash, parsed.data);
            return;
        } else if (parsed.type === 'disappearing_sync') {
            userStore.update(s => {
                if (s.chats[senderHash]) s.chats[senderHash].disappearingTimer = parsed.seconds || undefined;
                return { ...s, chats: { ...s.chats } };
            });
            return;
        } else if (parsed.type === 'call_log') {
            const msg: Message = {
                id: parsed.callId || incomingMsgId,
                timestamp: Date.now(),
                senderHash: senderHash,
                senderAlias: state.chats[senderHash]?.peerAlias,
                content: parsed.content,
                type: 'call_log',
                call_duration: parsed.duration,
                call_status: parsed.status,
                isMine: false,
                status: 'read'
            };
            addMessage(senderHash, msg);
            return;
        }
    } catch (e) { }

    const msg: Message = {
        id: incomingMsgId, timestamp: Date.now(), senderHash,
        senderAlias: state.chats[senderHash]?.peerAlias, content, type, attachment,
        groupId: actualGroupId, isMine: false, status: 'delivered', replyTo, linkPreview
    };
    addMessage(actualGroupId || senderHash, msg);

    if (!actualGroupId) {
        const currentActive = get(userStore).activeChatHash;
        const finalStatus = (currentActive === senderHash) ? 'read' : 'delivered';
        sendReceipt(senderHash, [incomingMsgId], finalStatus);

        if (finalStatus === 'read') {
            userStore.update(s => {
                if (s.chats[senderHash]) {
                    const m = s.chats[senderHash].messages.find(x => x.id === incomingMsgId);
                    if (m) m.status = 'read';
                }
                return s;
            });
        }
    }
};

export const handleIncomingMessage = async (payload: Uint8Array | ServerMessage) => {
    try {
        const state = get(userStore);
        if (!state.identityHash) return;

        let ciphertextObj: any;
        if (payload instanceof Uint8Array) {
            let lastIndex = payload.length;
            while (lastIndex > 0 && payload[lastIndex - 1] === 0) lastIndex--;
            const trimmedPayload = payload.slice(0, lastIndex);
            const payloadStr = new TextDecoder().decode(trimmedPayload);
            try { ciphertextObj = JSON.parse(payloadStr); } catch (e) { return; }
        } else {
            ciphertextObj = payload;
        }

        if (ciphertextObj.sealed) {
            try {
                const unsealed = await signalManager.unseal(ciphertextObj);
                ciphertextObj = unsealed.message;
                ciphertextObj.sender = unsealed.sender_hash || unsealed.sender;
            } catch (e) {
                console.error("Failed to unseal message:", e);
                return;
            }
        }

        if (ciphertextObj.type === 'group_message_v2') {
            const { groupId, sender, id } = ciphertextObj;
            try {
                const plaintext = await signalManager.groupDecrypt(groupId, sender, ciphertextObj);
                await processPlaintext(sender, plaintext, groupId, id, ciphertextObj.replyTo);
            } catch (e) {
                console.error("Failed to decrypt group message:", e);
            }
            return;
        }

        let senderHash = ciphertextObj.sender || "unknown";
        let signalMessage = ciphertextObj.message || ciphertextObj;
        if (!signalMessage.body && ciphertextObj.body) signalMessage = ciphertextObj;

        const isPreKey = (signalMessage.type === 3 || signalMessage.event === 3);
        let result: any = null;

        if (senderHash === "unknown" && !isPreKey) {
            for (const hash of Object.keys(state.chats)) {
                if (state.chats[hash].isGroup) continue;
                try {
                    const testResult = await signalManager.decrypt(hash, signalMessage);
                    if (testResult) { senderHash = hash; result = testResult; break; }
                } catch (e) { }
            }
        }

        try {
            if (!result) result = await signalManager.decrypt(senderHash, signalMessage);
        } catch (e: any) {
            if (e.toString().includes("IDENTITY_CHANGED")) {
                userStore.update(s => {
                    if (s.chats[senderHash]) s.chats[senderHash].isVerified = false;
                    return { ...s };
                });
                const msg: Message = {
                    id: crypto.randomUUID(), timestamp: Date.now(), senderHash,
                    content: "SECURITY ALERT: This contact's identity has changed. This could be a new device or a security threat.",
                    type: 'text', isMine: false, status: 'delivered'
                };
                addMessage(senderHash, msg);
            }
        }

        if (result && (result.m || result.type)) {
            await processPlaintext(result.s || senderHash, result.m, undefined, undefined, undefined);
        }
    } catch (e) { }
};

export const addMessage = (peerHash: string, msg: Message) => {
    if (msg.attachment?.data) attachmentStore.put(msg.id, msg.attachment.data).catch(e => { });

    userStore.update(s => {
        const chat = s.chats[peerHash];
        if (!chat) {
            s.chats[peerHash] = { peerHash, peerAlias: peerHash.slice(0, 8), messages: [], unreadCount: 0 };
        } else if (chat.messages.some(m => m.id === msg.id)) {
            return s;
        }

        s.chats[peerHash].messages.push(msg);

        if (!msg.isMine && s.activeChatHash !== peerHash) {
            s.chats[peerHash].unreadCount = (s.chats[peerHash].unreadCount || 0) + 1;

            
            if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
                import('@tauri-apps/plugin-notification').then(({ sendNotification, isPermissionGranted }) => {
                    isPermissionGranted().then((granted: boolean) => {
                        if (granted) {
                            sendNotification({
                                title: `Message from ${s.chats[peerHash].peerAlias}`,
                                body: msg.content.length > 50 ? msg.content.substring(0, 47) + '...' : msg.content
                            });
                        }
                    });
                });
            }
        }

        return { ...s, chats: { ...s.chats } };
    });
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

        if (unreadIds.length > 0) {
            s.chats[peerHash].unreadCount = 0;
            sendReceipt(peerHash, unreadIds, 'read');
        }

        return { ...s, activeChatHash: peerHash, chats: { ...s.chats } };
    });
};

export const updateAlias = (peerHash: string, newAlias: string) => {
    userStore.update(s => { if (s.chats[peerHash]) s.chats[peerHash].peerAlias = newAlias; return s; });
};


let storageKey: Uint8Array | null = null;
let isSaving = false;
let saveTimeout: any = null;

userStore.subscribe(state => {
    if (!state.identityHash || !storageKey || isSaving) return;

    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(async () => {
        try {
            isSaving = true;
            const chatsCopy = JSON.parse(JSON.stringify(state.chats));
            for (const h in chatsCopy) {
                delete chatsCopy[h].isOnline;
                delete chatsCopy[h].isTyping;
                for (const m of chatsCopy[h].messages) {
                    if (m.attachment?.data) delete m.attachment.data;
                }
            }

            const vault = {
                chats: chatsCopy,
                myAlias: state.myAlias,
                myPfp: state.myPfp,
                blockedHashes: state.blockedHashes,
                privacySettings: state.privacySettings,
                sessionToken: state.sessionToken
            };

            await vaultSave(`entropy_chats_${state.identityHash}`, JSON.stringify(vault));
        } catch (e) {
            console.error("Failed to persist vault:", e);
        } finally {
            isSaving = false;
        }
    }, 2000); 
});
export const exportVault = async () => {
    try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const bytes = await invoke('protocol_export_vault') as number[];
        const uint8 = new Uint8Array(bytes);

        const filePath = await save({
            filters: [{ name: 'Entropy Vault', extensions: ['db'] }],
            defaultPath: 'entropy_backup.db'
        });

        if (filePath) {
            await invoke('protocol_save_vault_to_path', { path: filePath, bytes: Array.from(uint8) });
            alert("Vault exported successfully to: " + filePath);
        }
    } catch (e) {
        console.error("Export failed:", e);
        alert("Export failed: " + e);
    }
};

export const importVault = async () => {
    if (!confirm("DANGER: Importing a vault will overwrite your current local data. This will purge all existing chats on this device. Continue?")) return;

    try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const file = await open({
            multiple: false,
            filters: [{ name: 'Entropy Vault', extensions: ['db'] }]
        });

        if (file) {
            const path = typeof file === 'string' ? file : (file as any).path;
            const bytes = await invoke('protocol_read_vault_from_path', { path }) as number[];
            await invoke('protocol_import_vault', { bytes });
            alert("Vault imported. The application will now restart.");
            window.location.reload();
        }
    } catch (e) {
        console.error("Import failed:", e);
        alert("Import failed: " + e);
    }
};
