import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { invoke } from '@tauri-apps/api/core';
import { signalManager } from '../signal_manager';
import { addToast, showConfirm, mediaProxyPort } from '../stores/ui';
import { network } from '../network';
import { broadcastProfile } from './contacts';
import { initVault, vaultLoad } from '../persistence';
import { loadStarredMessages } from './chat';
import type { Chat } from '../types';

let isAuthInProgress = false;

export const initApp = async (password: string) => {
    userStore.update(s => ({ ...s, authError: null }));
    try {
        await initVault(password);
        
        // Fetch and set the media proxy port immediately after vault init
        const port = await invoke<number>('get_media_proxy_port');
        mediaProxyPort.set(port);
    } catch (e: any) {
        userStore.update(s => ({ ...s, authError: e.toString() || "Failed to open vault." }));
        return;
    }

    let idHash: string | null = null;
    try {
        idHash = await signalManager.init();
    } catch (e) {
        // Signal init failed
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

        // Load Global Metadata
        const savedMeta = await vaultLoad(`entropy_meta_${idHash}`);
        if (savedMeta) {
            try {
                const meta = JSON.parse(savedMeta);
                globalNickname = meta.globalNickname || meta.myAlias || null;
                privacySettings = meta.privacySettings || privacySettings;
            } catch (e) {
                // Parse failed
            }
        }

        // Load relational data
        let nicknames: Record<string, string> = {};
        try {
            const dbContacts = await invoke<any[]>('db_get_contacts');
            blockedHashes = dbContacts.filter(c => c.isBlocked).map(c => c.hash);
            dbContacts.forEach(c => {
                const name = c.alias || c.globalNickname;
                if (name) nicknames[c.hash] = name;
            });

            const dbChats = await invoke<any[]>('db_get_chats');
            for (const c of dbChats) {
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
            // DB load failed
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

        loadStarredMessages().catch(() => { });
        network.connect();
    } else {
        userStore.update(s => ({ ...s, authError: "Identity not found. please create one." }));
    }
};

export const createIdentity = async (password: string) => {
    try {
        await initVault(password);
    } catch (e: any) {
        throw new Error(`Local vault setup failed: ${e.message || e}`);
    }

    let idHash;
    try {
        idHash = await signalManager.init();
    } catch (e: any) {
        throw new Error(`Identity generation failed: ${e.message || e}`);
    }

    if (idHash) {
        userStore.update(s => ({ ...s, identityHash: idHash }));
        network.connect();
    } else {
        throw new Error("Identity generation returned null.");
    }
};

export const purgeIdentity = async () => {
    if (!await showConfirm("PERMANENT RESET: This will permanently purge your local database and attempt to wipe your network presence. This CANNOT be undone. Are you sure?", "Reset Identity")) return;

    const state = get(userStore);

    if (state.identityHash) {
        invoke('burn_account').catch(() => { });
    }

    try {
        localStorage.clear();
        await new Promise(r => setTimeout(r, 1000));
        await invoke('reset_database');
    } catch (err) {
        // Reset failed
    }
}

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
        addToast("Export failed: " + e, 'error');
    }
};

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
        addToast("Import failed: " + e, 'error');
    }
};
