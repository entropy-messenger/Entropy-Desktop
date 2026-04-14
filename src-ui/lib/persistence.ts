import { invoke } from '@tauri-apps/api/core';

const isTauri = () => typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__;

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
            throw e;
        }
    }
};

export const vaultLoad = async (key: string): Promise<string | null> => {
    if (isTauri()) {
        try {
            const val = await invoke('vault_load', { key });
            return val as string | null;
        } catch (e) {
            return null;
        }
    }
    return null;
};

export const vaultDelete = async (key: string): Promise<void> => {
    if (isTauri()) {
        try {
            await invoke('vault_delete', { key });
        } catch (e) {
        }
    }
};

export const hasVault = async (): Promise<boolean> => {
    if (isTauri()) {
        try {
            return await invoke('vault_exists');
        } catch (e) {
            return false;
        }
    }
    return false;
};
