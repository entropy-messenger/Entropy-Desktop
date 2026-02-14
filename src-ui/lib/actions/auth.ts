
import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { invoke } from '@tauri-apps/api/core';
import { signalManager } from '../signal_manager';
import { addToast, showConfirm } from '../stores/ui';
import { network } from '../network';
import { minePoW, initCrypto, toBase64, fromHex } from '../crypto';
import { statusTimeouts, setOnlineStatus, startHeartbeat, broadcastProfile } from './contacts';
import { initVault, vaultLoad, vaultSave } from '../secure_storage';
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
    await initCrypto();
    try {
        await initVault(password);
    } catch (e: any) {
        console.error("Vault init failed:", e);
        userStore.update(s => ({ ...s, authError: e.toString() || "Failed to open vault." }));
        return;
    }

    let idHash: string | null = null;
    try {
        idHash = await signalManager.init(password);
    } catch (e) {
        console.error("Signal init failed:", e);
    }

    if (idHash) {
        let chats: Record<string, Chat> = {};
        let myAlias: string | null = null;
        let myPfp: string | null = null;
        let sessionToken: string | null = null;

        const saved = await vaultLoad(`entropy_chats_${idHash}`);

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
            } catch (e) {
                userStore.update(s => ({ ...s, authError: "Corrupted vault metadata." }));
                return;
            }
        }

        userStore.update(s => ({ ...s, identityHash: idHash, chats, myAlias, myPfp, sessionToken, authError: null }));
        network.connect();
        startHeartbeat();

        const serverUrl = get(userStore).relayUrl;
        try { await signalManager.ensureKeysUploaded(serverUrl); } catch (e) { }
    } else {
        userStore.update(s => ({ ...s, authError: "Identity not found. please create one." }));
    }
};

/**
 * Generates a new cryptographic identity and initializes a fresh encrypted vault.
 */
export const createIdentity = async (password: string) => {
    try {
        await initCrypto();
        await initVault(password);
    } catch (e: any) {
        console.error("Vault initialization failed:", e);
        throw new Error(`Local vault setup failed: ${e.message || e}`);
    }

    let idHash;
    try {
        idHash = await signalManager.init(password);
        console.debug("Identity generated:", idHash);
    } catch (e: any) {
        console.error("Identity generation failed:", e);
        throw new Error(`Identity generation failed: ${e.message || e}`);
    }

    if (idHash) {
        userStore.update(s => ({ ...s, identityHash: idHash }));

        console.debug("Connecting to network...");
        network.connect();
        startHeartbeat();
    } else {
        throw new Error("Identity generation returned null.");
    }
};

/**
 * Authenticates the local identity with the relay server.
 * Uses persistent session tokens if available, falling back to SHA-256 Proof-of-Work mining.
 */
export const authenticate = async (identityHash: string) => {
    if (isAuthInProgress) return;
    isAuthInProgress = true;

    try {
        const state = get(userStore);
        const serverUrl = state.relayUrl;

        if (state.sessionToken) {
            console.debug("Attempting session-token authentication...");
            userStore.update(s => ({ ...s, connectionStatus: 'connecting' }));
            network.sendJSON({
                type: 'auth',
                payload: {
                    identity_hash: identityHash,
                    session_token: state.sessionToken
                }
            });
        } else {
            console.debug("No session token. Starting PoW mining via WebSocket...");
            userStore.update(s => ({ ...s, connectionStatus: 'mining' }));

            // Fetch challenge via normalized WebSocket instead of HTTP
            const challenge = await network.request('pow_challenge', { identity_hash: identityHash });
            const { seed, difficulty } = challenge;

            const pow = await minePoW(seed, difficulty, identityHash);

            network.sendJSON({
                type: 'auth',
                payload: { identity_hash: identityHash, seed: pow.seed, nonce: pow.nonce }
            });
        }
    } catch (e) {
        console.error("Authentication failed:", e);
    } finally {
        isAuthInProgress = false;
    }
};

export const refreshDecoys = async (serverUrl: string) => {
    try {
        const state = get(userStore);
        if (state.isConnected) {
            const data = await network.request('fetch_key_random', { count: 20 });
            if (data && data.hashes) {
                console.log(`[Privacy] Refreshed decoy pool: ${data.hashes.length} entries`);
                userStore.update(s => ({ ...s, decoyHashes: data.hashes }));
            }
        }
    } catch (e) {
        console.error("Failed to refresh decoys:", e);
    }
};

/**
 * Permanently purges the local vault and sends a signed burn request to the relay.
 * Requires solving a high-difficulty PoW to authorize the network-wide erasure.
 */
export const burnAccount = async (serverUrl: string) => {
    if (await showConfirm("DANGER: This will permanently purge your account from the network and your local data. This cannot be undone. Are you sure?", "Nuclear Burn")) {
        const state = get(userStore);
        if (state.identityHash) {
            try {
                addToast("Authenticating burn request...", 'info');

                // Get challenge via WebSocket
                const challenge = await network.request('pow_challenge', { identity_hash: state.identityHash, intent: 'burn' });
                const { seed, difficulty } = challenge;

                // 1. Solve PoW
                const { nonce } = await minePoW(seed, 4, state.identityHash);

                // 2. Sign the canonical string "BURN_ACCOUNT:<hash>" (consistent with server)
                const signature = await signalManager.signMessage("BURN_ACCOUNT:" + state.identityHash);

                // 3. Send burn request via WebSocket
                const response = await network.request('account_burn', {
                    identity_hash: state.identityHash,
                    signature,
                    seed,
                    nonce
                });

                if (response.status === 'success') {
                    console.log("Network account burn successful.");
                    addToast("Server account deleted.", 'success');
                } else {
                    console.error("Network account burn failed:", response.error);
                    addToast("Server deletion failed: " + response.error, 'error');
                    if (!await showConfirm("Server-side deletion failed. Wipe local data and restart anyway?", "Relay Error")) return;
                }
            } catch (e) {
                console.error("Failed to burn network account:", e);
                if (!await showConfirm("Relay burn failed (Network Error). Wipe local data anyway?", "Relay Error")) return;
            }
        }
    }

    try {
        localStorage.clear();
        addToast("Local data wiped. Restarting Entropy...", 'info');
        await new Promise(r => setTimeout(r, 2000));
        await invoke('nuclear_reset');
    } catch (err) {
        console.error("Local wipe/restart failed:", err);
        addToast("Critical: Local reset failed - " + err, 'error');
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
