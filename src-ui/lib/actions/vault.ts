import { invoke } from '@tauri-apps/api/core';
import { userStore } from '../stores/user';
import { addToast, showConfirm } from '../stores/ui';

export const exportVault = async (targetPath: string, includeMedia: boolean) => {
    try {
        await invoke('export_database', { targetPath, includeMedia });
        addToast("Backup exported successfully!", 'success');
        return true;
    } catch (e: any) {
        addToast("Export failed: " + e.toString(), 'error');
        return false;
    }
};

export const importVault = async (srcPath: string, includeMedia: boolean) => {
    try {
        await invoke('import_database', { srcPath, includeMedia });
        addToast("Backup restored! Identity is being re-synchronized.", 'success');
        setTimeout(() => window.location.reload(), 2000);
        return true;
    } catch (e: any) {
        addToast("Import failed: " + e.toString(), 'error');
        return false;
    }
};

export async function exportVaultWithDialog() {
    if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const path = await save({
            defaultPath: `entropy_backup_${Date.now()}.entropy`,
            filters: [{ name: 'Entropy Backup', extensions: ['entropy'] }]
        });
        if (path) {
            const includeMedia = await showConfirm("Do you want to include all media files (photos/videos) in this backup?", "Backup Options");
            await exportVault(path, includeMedia);
        } else {
            addToast("Export cancelled.", 'info');
        }
    } else {
        addToast("Export not supported in web mode.", 'warning');
    }
}

export async function importVaultWithDialog() {
    if (!await showConfirm("WARNING: Importing a backup will OVERWRITE all current data. This cannot be undone. Continue?", "Restore Backup")) return;
    if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const path = await open({
            multiple: false,
            filters: [{ name: 'Entropy Backup', extensions: ['entropy', 'zip'] }]
        });
        if (path) {
            const includeMedia = await showConfirm("This backup may contain media files. Do you want to extract and restore them as well?", "Restore Options");
            await importVault(path, includeMedia);
        }
    } else {
        addToast("Import not supported in web mode.", 'warning');
    }
}

export const resetDatabase = async () => {
    try {
        await invoke('reset_database');
        localStorage.clear();
        addToast("Vault wiped. Entropy is restarting...", 'info');
        return true;
    } catch (e: any) {
        addToast("Reset failed: " + e.toString(), 'error');
        return false;
    }
};
