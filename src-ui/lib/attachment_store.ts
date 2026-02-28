import { toBase64, fromBase64 } from './crypto';
import { vaultSave, vaultLoad, vaultDelete } from './secure_storage';
import { invoke } from '@tauri-apps/api/core';

export class AttachmentStore {
    private cache = new Map<string, Uint8Array>();

    // No encryption key needed for plaintext storage
    setEncryptionKey(key: Uint8Array | null) {
        // No-op
    }

    async init(): Promise<void> {
        // No-op, vault is initialized at app start
    }

    async put(id: string, data: Uint8Array): Promise<void> {
        // Store in memory cache first for instant access
        this.cache.set(id, data);

        // Store as regular file in media/ folder via backend
        await invoke('vault_save_media', { id, data });
    }

    async get(id: string): Promise<Uint8Array | null> {
        // Check memory cache first
        if (this.cache.has(id)) {
            return this.cache.get(id)!;
        }

        try {
            const bytes = await invoke<number[]>('vault_load_media', { id });
            const uint8 = new Uint8Array(bytes);

            // Backfill cache
            this.cache.set(id, uint8);
            return uint8;
        } catch (e) {
            console.warn(`[AttachmentStore] Load failed for ${id}:`, e);
            return null;
        }
    }

    async delete(id: string): Promise<void> {
        this.cache.delete(id);
        await vaultDelete(`att_${id}`);
    }
}

export const attachmentStore = new AttachmentStore();
