
import { invoke } from '@tauri-apps/api/core';

/**
 * Detects if the application is running within the Tauri environment.
 */
const isTauri = () => typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__;

/**
 * Initializes the encrypted SQLCipher-backed vault using the provided passphrase.
 */
export const initVault = async (passphrase: string): Promise<void> => {
    if (isTauri()) {
        await invoke('init_vault', { passphrase });
    }
};

/**
 * Persists a key-value pair into the encrypted vault (SQLCipher Key-Value).
 */
export const vaultSave = async (key: string, value: string): Promise<void> => {
    if (isTauri()) {
        try {
            await invoke('vault_save', { key, value });
        } catch (e) {
            console.error(`[Vault] Save failed for ${key}:`, e);
            throw e;
        }
    } else {
        if (import.meta.env.DEV) localStorage.setItem(`vlt:${key}`, value);
    }
};

/**
 * Retrieves a value from the encrypted vault by key.
 */
export const vaultLoad = async (key: string): Promise<string | null> => {
    if (isTauri()) {
        try {
            const val = await invoke('vault_load', { key });
            return val as string | null;
        } catch (e) {
            return null;
        }
    } else {
        return import.meta.env.DEV ? localStorage.getItem(`vlt:${key}`) : null;
    }
};

/**
 * Removes a key from the encrypted vault.
 */
export const vaultDelete = async (key: string): Promise<void> => {
    if (isTauri()) {
        try {
            await invoke('vault_delete', { key });
        } catch (e) {
            console.error(`[Vault] Delete failed for ${key}:`, e);
        }
    } else {
       if (import.meta.env.DEV) localStorage.removeItem(`vlt:${key}`);
    }
};

/**
 * Checks for the existence of an encrypted vault on the local filesystem.
 */
export const hasVault = async (): Promise<boolean> => {
    if (isTauri()) {
        try {
            return await invoke('vault_exists');
        } catch (e) {
            return false;
        }
    }
    return !!localStorage.getItem('vlt:plaintext_identity');
};
