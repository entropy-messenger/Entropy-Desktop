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
            return idHash;
        } catch (e) {
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
            throw e;
        }
    }

    // --- SESSION CONTEXT HELPERS ---

    // --- GROUP CONTEXT HELPERS ---
}

export const signalManager = new SignalManager();
