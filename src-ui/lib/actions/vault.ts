import { invoke } from '@tauri-apps/api/core';
import { userStore } from '../stores/user';
import { addToast } from '../stores/ui';

/**
 * High-level orchestration for vault lifecycle events.
 * Bridges the UI to the native SQLCipher-backed database layer.
 */

export const exportVault = async (targetPath: string) => {
    try {
        await invoke('export_database', { targetPath });
        addToast("Backup exported successfully!", 'success');
        return true;
    } catch (e: any) {
        console.error("[Vault] Export failed:", e);
        addToast("Export failed: " + e.toString(), 'error');
        return false;
    }
};

export const importVault = async (srcPath: string) => {
    try {
        await invoke('import_database', { srcPath });
        addToast("Backup restored! Identity is being re-synchronized.", 'success');
        // Allow time for native cleanup before reload
        setTimeout(() => window.location.reload(), 2000);
        return true;
    } catch (e: any) {
        console.error("[Vault] Import failed:", e);
        addToast("Import failed: " + e.toString(), 'error');
        return false;
    }
};

export const resetDatabase = async () => {
    try {
        await invoke('reset_database');
        localStorage.clear();
        addToast("Vault wiped. Entropy is restarting...", 'info');
        // Native process will likely terminate/restart
        return true;
    } catch (e: any) {
        console.error("[Vault] Nuclear reset failed:", e);
        addToast("Reset failed: " + e.toString(), 'error');
        return false;
    }
};
