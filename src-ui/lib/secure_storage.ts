import { invoke } from '@tauri-apps/api/core';

declare global {
    interface Window {
        __TAURI_INTERNALS__?: unknown;
    }
}

const isTauri = () => typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__;

export const secureStore = async (key: string, value: string): Promise<void> => {
    if (isTauri()) {
        await invoke('store_secret', { key, value });
    }
};

export const secureLoad = async (key: string): Promise<string | null> => {
    if (isTauri()) {
        try {
            return await invoke('get_secret', { key }) as string;
        } catch (e) {
            return null;
        }
    }
    return null;
};

export const initVault = async (passphrase: string): Promise<void> => {
    if (isTauri()) {
        await invoke('init_vault', { passphrase });
    }
};

export const vaultSave = async (key: string, value: string): Promise<void> => {
    if (isTauri()) {
        await invoke('vault_save', { key, value });
    }
};

export const vaultLoad = async (key: string): Promise<string | null> => {
    if (isTauri()) {
        try {
            return await invoke('vault_load', { key }) as string | null;
        } catch (e) {
            return null;
        }
    }
    return null;
};

export const hasStoredSalt = async (): Promise<boolean> => {
    return true;
};
