import { invoke } from '@tauri-apps/api/core';

export class AttachmentStore {
    async put(id: string, data: Uint8Array): Promise<void> {
        // Store directly in SQLite via Tauri command
        await invoke('protocol_blob_put', { id, data: Array.from(data) });
    }

    async get(id: string): Promise<Uint8Array | null> {
        try {
            const data = await invoke('protocol_blob_get', { id }) as number[];
            if (!data) return null;
            return new Uint8Array(data);
        } catch (e) {
            console.warn(`Blob ${id} not found in SQLite:`, e);
            return null;
        }
    }

    async delete(id: string): Promise<void> {
        await invoke('protocol_blob_delete', { id });
    }
}

export const attachmentStore = new AttachmentStore();
