import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { invoke } from '@tauri-apps/api/core';
import { signalManager } from '../signal_manager';
import { addToast, showConfirm } from '../stores/ui';
import { network } from '../network';
import { broadcastProfile } from './contacts';
import { initVault, vaultLoad } from '../persistence';
import { loadStarredMessages } from './chat';
import type { Chat } from '../types';

/**
 * Handles application-level authentication, identity lifecycle, and vault persistence.
 */
let isAuthInProgress = false;

/**
 * Initializes the application by unlocking the encrypted vault and starting key managers.
 */
export const initApp = async (password: string) => {
    userStore.update(s => ({ ...s, authError: null }));
    try {
        await initVault(password);
    } catch (e: any) {
        console.error("Vault init failed:", e);
        userStore.update(s => ({ ...s, authError: e.toString() || "Failed to open vault." }));
        return;
    }

    let idHash: string | null = null;
    try {
        idHash = await signalManager.init();
    } catch (e) {
        console.error("Signal init failed:", e);
    }

    if (idHash) {
        let chats: Record<string, Chat> = {};
        let globalNickname: string | null = null;
        let blockedHashes: string[] = [];
        let privacySettings: any = {
            typingStatus: 'everyone',
            readReceipts: true,
            routingMode: 'direct',
            proxyUrl: ''
        };

        // 1. Load Global Metadata (Settings) from KV Store
        const savedMeta = await vaultLoad(`entropy_meta_${idHash}`);
        if (savedMeta) {
            try {
                const meta = JSON.parse(savedMeta);
                globalNickname = meta.globalNickname || meta.myAlias || null;
                privacySettings = meta.privacySettings || privacySettings;
            } catch (e) {
                console.error("Failed to parse vault metadata:", e);
            }
        }

        // 2. Load relational Chat/Contact objects
        let nicknames: Record<string, string> = {};
        try {
            const dbContacts = await invoke<any[]>('db_get_contacts');
            blockedHashes = dbContacts.filter(c => c.isBlocked).map(c => c.hash);
            dbContacts.forEach(c => {
                // Priority: Local Alias > Global Nickname
                const name = c.alias || c.globalNickname;
                if (name) nicknames[c.hash] = name;
            });

            const dbChats = await invoke<any[]>('db_get_chats');
            for (const c of dbChats) {
                // Also pull nicknames from chat objects if not already in contacts
                const name = c.alias || c.globalNickname;
                if (name && !nicknames[c.address]) nicknames[c.address] = name;
                
                chats[c.address] = {
                    peerHash: c.address,
                    isGroup: c.is_group || c.isGroup || false,
                    localNickname: c.alias,
                    globalNickname: c.globalNickname,
                    members: c.members || undefined,
                    messages: [],
                    unreadCount: c.unread_count || c.unreadCount || 0,
                    isArchived: c.is_archived || c.isArchived || false,
                    isPinned: c.is_pinned || c.isPinned || false,
                    trustLevel: c.trust_level || c.trustLevel || 1,
                    isBlocked: c.is_blocked || c.isBlocked || false,
                    isTyping: false,
                    lastMsg: c.last_msg || c.lastMsg,
                    lastTimestamp: c.last_timestamp || c.lastTimestamp,
                    lastStatus: c.last_status || c.lastStatus,
                    lastIsMine: (c.last_sender_hash || c.lastSenderHash) === idHash,
                    lastSenderHash: c.last_sender_hash || c.lastSenderHash,
                    hasMore: true
                };
            }
        } catch (e) {
            console.error("Failed to load chats/contacts from DB:", e);
        }

        userStore.update(s => ({
            ...s,
            identityHash: idHash,
            chats,
            globalNickname,
            blockedHashes,
            privacySettings,
            nicknames,
            authError: null
        }));
        
        // 3. Prime the Starred Messages Cache
        loadStarredMessages().catch(console.error);
        
        network.connect();
    } else {
        userStore.update(s => ({ ...s, authError: "Identity not found. please create one." }));
    }
};

/**
 * Generates a new cryptographic identity and initializes a fresh encrypted vault.
 */
export const createIdentity = async (password: string) => {
    try {
        await initVault(password);
    } catch (e: any) {
        console.error("Vault initialization failed:", e);
        throw new Error(`Local vault setup failed: ${e.message || e}`);
    }

    let idHash;
    try {
        idHash = await signalManager.init();
        console.debug("Identity generated:", idHash);
    } catch (e: any) {
        console.error("Identity generation failed:", e);
        throw new Error(`Identity generation failed: ${e.message || e}`);
    }

    if (idHash) {
        userStore.update(s => ({ ...s, identityHash: idHash }));

        console.debug("Connecting to network...");
        // Status and handshake are now managed by native layer
        network.connect();
    } else {
        throw new Error("Identity generation returned null.");
    }
};

/**
 * Authenticates the local identity with the relay server.
 * Uses persistent session tokens if available, falling back to SHA-256 Proof-of-Work mining.
 */
// Manual 'authenticate' function removed. 
// Authentication is now handled autonomously by Rust within 'connect_network'.


/**
 * Permanently purges the local vault and sends a signed burn request to the relay.
 * Requires solving a high-difficulty PoW to authorize the network-wide erasure.
 */
export const burnAccount = async () => {
    if (!await showConfirm("TOTAL SCORCHED EARTH: This will permanently purge your local database and attempt to wipe your network presence. This CANNOT be undone. Are you sure?", "Nuclear Burn")) return;

    const state = get(userStore);
    
    // 1. Silent Background server burn
    if (state.identityHash) {
        invoke('burn_account').catch(() => {});
    }

    // 2. Immediate Local Wipe
    try {
        localStorage.clear();
        
        // 1s buffer for the network packet
        await new Promise(r => setTimeout(r, 1000));
        
        await invoke('reset_database');
    } catch (err) {
        console.error("[Account] Burn Failed:", err);
    }
}



/**
 * Exports the encrypted database file to the host system.
 */
export const exportVault = async () => {
    try {
        if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
            const { save } = await import('@tauri-apps/plugin-dialog');
            const path = await save({
                defaultPath: `entropy_backup_${Date.now()}.entropy`,
                filters: [{
                    name: 'Entropy Backup',
                    extensions: ['entropy']
                }]
            });

            if (path) {
                await invoke('export_database', { targetPath: path });
                addToast("Backup exported successfully!", 'success');
            }
        } else {
            addToast("Export not supported in web mode.", 'warning');
        }
    } catch (e) {
        console.error("Export failed:", e);
        addToast("Export failed: " + e, 'error');
    }
};

/**
 * Imports an encrypted backup, overwriting the current local state.
 */
export const importVault = async () => {
    if (!await showConfirm("WARNING: Importing a backup will OVERWRITE all current data. This cannot be undone. Continue?", "Restore Backup")) return;

    try {
        if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
            const { open } = await import('@tauri-apps/plugin-dialog');
            const path = await open({
                multiple: false,
                filters: [{
                    name: 'Entropy Backup',
                    extensions: ['entropy', 'zip']
                }]
            });

            if (path) {
                await invoke('import_database', { srcPath: path });
                addToast("Backup restored! The app will now reload.", 'success');
                setTimeout(() => window.location.reload(), 2000);
            }
        } else {
            addToast("Import not supported in web mode.", 'warning');
        }
    } catch (e) {
        console.error("Import failed:", e);
        addToast("Import failed: " + e, 'error');
    }
};
