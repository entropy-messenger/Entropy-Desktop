import { invoke } from '@tauri-apps/api/core';

declare global {
    interface Window {
        __TAURI_INTERNALS__?: unknown;
    }
}

const isTauri = () => typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__;


export const secureStore = async (key: string, value: string): Promise<void> => {
    if (isTauri()) {
        try {
            await invoke('store_secret', { key, value });
        } catch (e) {
            console.error(`[SecureStorage] Native keyring store failed for ${key}:`, e);
            throw new Error(`System Keyring failure: ${e}`);
        }
    } else {
        throw new Error("Secure storage requires a native environment.");
    }
};

export const secureLoad = async (key: string): Promise<string | null> => {
    if (isTauri()) {
        try {
            const secret = await invoke('get_secret', { key });
            return secret as string | null;
        } catch (e) {
            return null;
        }
    } else {
        return null;
    }
};


export const initVault = async (passphrase: string): Promise<void> => {
    if (isTauri()) {
        await invoke('init_vault', { passphrase });
    }
};

export const vaultSave = async (key: string, value: string): Promise<void> => {
    if (isTauri()) {
        try {
            await invoke('vault_save', { key, value });
        } catch (e) {
            console.error("[SecureStorage] SQLCipher save failed:", e);
            throw e;
        }
    } else {
        if (import.meta.env.DEV) {
            localStorage.setItem(`vlt:${key}`, value);
        }
    }
};

export const vaultLoad = async (key: string): Promise<string | null> => {
    if (isTauri()) {
        try {
            const val = await invoke('vault_load', { key });
            return val as string | null;
        } catch (e) {
            console.warn("[SecureStorage] SQLCipher load failed:", e);
            return null;
        }
    } else {
        if (import.meta.env.DEV) {
            return localStorage.getItem(`vlt:${key}`);
        }
        return null;
    }
};


export const hasStoredSalt = async (): Promise<boolean> => {
    const salt = await secureLoad('entropy_vault_salt');
    return salt !== null;
};
