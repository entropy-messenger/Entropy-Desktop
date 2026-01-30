
import { invoke } from '@tauri-apps/api/core';
import { vaultSave, vaultLoad } from './secure_storage';

export class SignalStore {
    private encryptionKey: Uint8Array | null = null;

    setEncryptionKey(key: Uint8Array) {
        this.encryptionKey = key;
    }

    async deleteAllData(): Promise<void> {
        try {
            await invoke('clear_vault');
            console.log("SignalStore: Vault cleared successfully.");
        } catch (e) {
            console.error("SignalStore: Failed to clear vault:", e);
        }
    }

    async getLastMessageHash(localHash: string, peerHash: string, type: 'sent' | 'received'): Promise<string | null> {
        return await vaultLoad(`signal_integrity_${type}_${localHash}_${peerHash}`);
    }

    async putLastMessageHash(localHash: string, peerHash: string, type: 'sent' | 'received', hash: string): Promise<void> {
        await vaultSave(`signal_integrity_${type}_${localHash}_${peerHash}`, hash);
    }

    async moveIntegrityHash(localHash: string, oldPeer: string, newPeer: string): Promise<void> {
        const types: ('sent' | 'received')[] = ['sent', 'received'];
        for (const type of types) {
            const oldKey = `signal_integrity_${type}_${localHash}_${oldPeer}`;
            const newKey = `signal_integrity_${type}_${localHash}_${newPeer}`;
            const data = await vaultLoad(oldKey);
            if (data) {
                await vaultSave(newKey, data);
                await vaultSave(oldKey, 'null');
            }
        }
    }

    async isBlankSlate(): Promise<boolean> {
        
        const legacyId = await vaultLoad('signal_identity_key');
        const rustId = await vaultLoad('protocol_identity');

        const legacyExists = legacyId !== null && legacyId !== 'null';
        const rustExists = rustId !== null && rustId !== 'null';

        return !legacyExists && !rustExists;
    }
}
