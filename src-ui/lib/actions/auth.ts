import { get } from 'svelte/store';
import { userStore } from '../stores/user';
import { invoke } from '@tauri-apps/api/core';
import { signalManager } from '../signal_manager';
import { network } from '../network';
import { startHeartbeat } from './contacts';
import { initVault, vaultLoad } from '../secure_storage';

let isAuthInProgress = false;
export const resetAuthStatus = () => { isAuthInProgress = false; };

export const initApp = async (password: string) => {
    userStore.update(s => ({ ...s, authError: null }));

    try {
        await initVault(password);
    } catch (e: any) {
        userStore.update(s => ({ ...s, authError: `System Error: ${e.message || e}` }));
        return;
    }

    let idHash: string | null = null;
    try {
        idHash = await signalManager.init(password);
    } catch (e: any) {
        userStore.update(s => ({ ...s, authError: `Initialization Failed: ${e.message || e}` }));
        return;
    }

    if (idHash) {
        const saved = await vaultLoad(`entropy_user_state`);
        const currentState = get(userStore);
        let chats = currentState.chats;
        let myAlias = currentState.myAlias;
        let myPfp = currentState.myPfp;
        let sessionToken = currentState.sessionToken;
        let blockedHashes = currentState.blockedHashes;
        let privacySettings = currentState.privacySettings;
        let relayUrl = currentState.relayUrl;

        if (saved) {
            try {
                const vault = JSON.parse(saved);
                chats = vault.chats || {};
                myAlias = vault.myAlias ?? myAlias;
                myPfp = vault.myPfp ?? myPfp;
                sessionToken = vault.sessionToken ?? sessionToken;
                blockedHashes = vault.blockedHashes ?? blockedHashes;
                privacySettings = vault.privacySettings ?? privacySettings;
                relayUrl = vault.relayUrl ?? relayUrl;
            } catch (e) {
                console.error("Failed to parse user state:", e);
            }
        }

        userStore.update(s => ({
            ...s,
            identityHash: idHash,
            chats,
            myAlias,
            myPfp,
            sessionToken,
            blockedHashes,
            privacySettings,
            relayUrl,
            authError: null
        }));

        network.connect();
        startHeartbeat();

        const serverUrl = get(userStore).relayUrl;
        try { await signalManager.ensureKeysUploaded(serverUrl); } catch (e) { }
    }
};

export const createIdentity = async (password: string) => {
    await initVault(password);
    let idHash = await signalManager.init(password);
    if (idHash) {
        userStore.update(s => ({ ...s, identityHash: idHash }));
        network.connect();
        startHeartbeat();
        const serverUrl = get(userStore).relayUrl;
        await signalManager.ensureKeysUploaded(serverUrl);
    }
};

export const authenticate = async (identityHash: string) => {
    if (isAuthInProgress) return;
    isAuthInProgress = true;

    try {
        const state = get(userStore);
        userStore.update(s => ({ ...s, connectionStatus: 'connecting' }));
        network.sendJSON({
            type: 'auth',
            payload: {
                identity_hash: identityHash,
                session_token: state.sessionToken
            }
        });
    } catch (e) {
        console.error("Authentication failed:", e);
    } finally {
        isAuthInProgress = false;
    }
};

export const exportVault = async () => {
    try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const bytes = await invoke('protocol_export_vault') as number[];
        const filePath = await save({
            filters: [{ name: 'Entropy Database', extensions: ['db'] }],
            defaultPath: 'entropy_backup.db'
        });

        if (filePath) {
            await invoke('protocol_save_vault_to_path', { path: filePath, bytes });
            alert("Vault exported successfully.");
        }
    } catch (e) {
        alert("Export failed: " + e);
    }
};

export const importVault = async () => {
    if (!confirm("Overwrite current data?")) return;
    try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const file = await open({
            multiple: false,
            filters: [{ name: 'Entropy Database', extensions: ['db'] }]
        });

        if (file) {
            const path = typeof file === 'string' ? file : (file as any).path;
            const bytes = await invoke('protocol_read_vault_from_path', { path }) as number[];
            await invoke('protocol_import_vault', { bytes });
            window.location.reload();
        }
    } catch (e) {
        alert("Import failed: " + e);
    }
};
