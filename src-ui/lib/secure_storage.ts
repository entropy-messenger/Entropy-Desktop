
import { invoke } from '@tauri-apps/api/core';

declare global {
    interface Window {
        __TAURI_INTERNALS__?: unknown;
    }
}

/**
 * Detects if the application is running within the Tauri environment.
 */
const isTauri = () => typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__;

/**
 * Legacy secure storage interface. 
 * Operations are currently delegated to the encrypted database (Vault).
 */
export const secureStore = async (key: string, value: string): Promise<void> => {
    // No-op or log warning
    console.warn("Secure store called in plaintext mode. Ignoring.");
};

export const secureLoad = async (key: string): Promise<string | null> => {
    // Return null
    return null;
};

/**
 * Initializes the encrypted SQLCipher-backed vault using the provided passphrase.
 */
export const initVault = async (passphrase: string): Promise<void> => {
    if (isTauri()) {
        await invoke('init_vault', { passphrase });
    }
};

/**
 * Persists a key-value pair into the encrypted vault.
 */
export const vaultSave = async (key: string, value: string): Promise<void> => {
    if (isTauri()) {
        try {
            await invoke('vault_save', { key, value });
        } catch (e) {
            console.error("[Vault] Save failed:", e);
            throw e;
        }
    } else {
        // Fallback for browser-only development environments
        if (import.meta.env.DEV) {
            localStorage.setItem(`vlt:${key}`, value);
        }
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
            console.warn("[Vault] Load failed (or empty):", e);
            return null;
        }
    } else {
        if (import.meta.env.DEV) {
            return localStorage.getItem(`vlt:${key}`);
        }
        return null;
    }
};

export const vaultDelete = async (key: string): Promise<void> => {
    if (isTauri()) {
        try {
            await invoke('vault_delete', { key });
        } catch (e) {
            console.error("[Vault] Delete failed:", e);
        }
    } else {
        if (import.meta.env.DEV) {
            localStorage.removeItem(`vlt:${key}`);
        }
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
            console.error("[hasVault] Check failed:", e);
            return false;
        }
    }
    return !!localStorage.getItem('plaintext_identity');
};
