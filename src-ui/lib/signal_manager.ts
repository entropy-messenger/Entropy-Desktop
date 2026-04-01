import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { toHex } from './crypto';
import { vaultLoad, vaultSave } from './persistence';
import { userStore } from './stores/user';

/**
 * Orchestrates the Signal Protocol lifecycle, including key management,
 * E2EE session establishment, and media encryption.
 * Now manages integrity session tracking previously handled by SignalStore.
 */
export class SignalManager {
    private userIdentity: string = "";

    /**
     * Initializes the native Signal state and retrieves the identity fingerprint.
     */
    async init(): Promise<string | null> {
        try {
            await invoke<string>('signal_init');
            const idHash = await invoke<string>('signal_get_identity_hash');
            this.userIdentity = idHash;
            console.log("[Signal] Protocol Initialized. Identity:", idHash);
            return idHash;
        } catch (e) {
            console.error("[Signal] Init failed:", e);
            return null;
        }
    }

    async getUserId(): Promise<string> {
        return this.userIdentity;
    }

    async ensureKeysUploaded() {
        // Save current status to restore after sync
        const currentStatus = get(userStore).connectionStatus;
        try {
            userStore.update(s => ({ ...s, connectionStatus: 'mining' }));
            await invoke('signal_sync_keys');
            userStore.update(s => ({ ...s, isSynced: true, connectionStatus: 'connected' }));
        } catch (e: any) {
            console.error("[Signal] Key sync failed:", e);
            userStore.update(s => ({ ...s, isSynced: false, connectionStatus: 'sync_error' }));
            throw e;
        }
    }

    async encrypt(recipientHash: string, message: string): Promise<any> {
        try {
            return await invoke<any>('signal_encrypt', {
                remoteHash: recipientHash,
                message
            });
        } catch (e: any) {
            console.error("[Signal] Encryption failed:", e);
            throw e;
        }
    }

    async signMessage(message: string): Promise<string> {
        return await invoke<string>('signal_sign_message', { message });
    }

    async decryptMedia(data: Uint8Array, bundle: any): Promise<Uint8Array> {
        try {
            const result = await invoke<number[]>('signal_decrypt_media', {
                data: Array.from(data),
                bundle
            });
            return new Uint8Array(result);
        } catch (e: any) {
            console.error("[Signal] Media decryption failed:", e);
            throw e;
        }
    }

    async getFingerprint(recipientHash: string): Promise<{ digits: string, trustLevel: number }> {
        try {
            const result = await invoke<any>('signal_get_fingerprint', {
                remoteHash: recipientHash
            });
            return {
                digits: result.digits,
                trustLevel: result.trustLevel
            };
        } catch (e: any) {
            console.error("[Signal] Fingerprint retrieval failed:", e);
            throw e;
        }
    }

    async verifySession(peerHash: string, trustLevel: number): Promise<void> {
        try {
            await invoke('signal_set_peer_trust', {
                address: peerHash,
                trustLevel // 0=untrusted, 1=trusted, 2=verified
            });
        } catch (e: any) {
            console.error("[Signal] Session verification failed:", e);
            throw e;
        }
    }

    // --- INTEGRITY & SESSION PERSISTENCE (Merged from SignalStore) ---

    async getLastMessageHash(localHash: string, peerHash: string, type: 'sent' | 'received'): Promise<string | null> {
        return await vaultLoad(`signal_integrity_${type}_${localHash}_${peerHash}`);
    }

    async putLastMessageHash(localHash: string, peerHash: string, type: 'sent' | 'received', hash: string): Promise<void> {
        await vaultSave(`signal_integrity_${type}_${localHash}_${peerHash}`, hash);
    }

    async isBlankSlate(): Promise<boolean> {
        const rustId = await vaultLoad('protocol_identity');
        return !rustId || rustId === 'null';
    }

    async deleteAllData(): Promise<void> {
        try {
            await invoke('clear_vault');
        } catch (e) {
            console.error("[Signal] Data purge failed:", e);
        }
    }

    // --- GROUP CONTEXT HELPERS ---

    async groupInit(groupId: string): Promise<string> {
        const seed = crypto.getRandomValues(new Uint8Array(16));
        return toHex(seed);
    }

    async createGroupDistribution(groupId: string): Promise<string> {
        const seed = crypto.getRandomValues(new Uint8Array(16));
        return toHex(seed);
    }
}

export const signalManager = new SignalManager();
