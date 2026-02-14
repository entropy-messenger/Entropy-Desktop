import { toBase64, fromBase64 } from './crypto';
import { vaultSave, vaultLoad, vaultDelete } from './secure_storage';
import { invoke } from '@tauri-apps/api/core';

export class AttachmentStore {
    // No encryption key needed for plaintext storage
    setEncryptionKey(key: Uint8Array | null) {
        // No-op
    }

    async init(): Promise<void> {
        // No-op, vault is initialized at app start
    }

    async put(id: string, data: Uint8Array): Promise<void> {
        // Store as regular file in media/ folder via backend
        await invoke('vault_save_media', { id, data });
    }

    async get(id: string): Promise<Uint8Array | null> {
        try {
            const bytes = await invoke<number[]>('vault_load_media', { id });
            return new Uint8Array(bytes);
        } catch (e) {
            console.warn(`[AttachmentStore] Load failed for ${id}:`, e);
            return null;
        }
    }

    async delete(id: string): Promise<void> {
        await vaultDelete(`att_${id}`);
    }
}

export const attachmentStore = new AttachmentStore();
