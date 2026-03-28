
import { invoke } from '@tauri-apps/api/core';
import { minePoW, toBase64, fromBase64, toHex, fromHex } from './crypto';
import { secureStore, secureLoad, vaultLoad, vaultSave } from './secure_storage';

import { get } from 'svelte/store';
import { userStore } from './stores/user';

/**
 * Orchestrates the Signal Protocol lifecycle, including key management,
 * E2EE session establishment, and media encryption.
 */
export class SignalManager {
    private userIdentity: string = "";

    constructor() {
    }

    /**
     * Initializes the native Signal state and retrieves the identity fingerprint.
     */
    async init(password: string): Promise<string | null> {
        try {
            await invoke<string>('signal_init');
            const idHash = await invoke<string>('signal_get_identity_hash');
            this.userIdentity = idHash;
            console.log("Initialized Signal Protocol. Identity Hash:", this.userIdentity);
            return this.userIdentity;
        } catch (e) {
            console.error("Signal init failed:", e);
            return null;
        }
    }

    async getUserId(): Promise<string> {
        return this.userIdentity;
    }

    /**
     * Synchronizes local pre-key bundles with the relay server.
     * Includes X3DH bundle preparation and Proof-of-Work to satisfy anti-spam requirements.
     */
    async ensureKeysUploaded(serverUrl: string, force: boolean = false) {
        console.debug("[Signal] Synchronizing keys via native layer...");
        try {
            await invoke('signal_sync_keys');
            userStore.update(s => ({ ...s, isSynced: true }));
            console.log("Keys synchronized successfully.");
        } catch (e: any) {
            console.error("Key synchronization failed:", e);
            userStore.update(s => ({ ...s, isSynced: false, connectionStatus: 'sync_error' }));
            throw e;
        }
    }


    /**
     * Encrypts a message for a peer. Rust automatically handles session establishment (smart negotiation)
     * including Decoy Mode for privacy.
     */
    async encrypt(recipientHash: string, message: string): Promise<any> {
        try {
            return await invoke<any>('signal_encrypt', {
                remoteHash: recipientHash,
                message
            });
        } catch (e: any) {
            console.error("Signal encryption failed:", e);
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
            console.error("Media decryption failed:", e);
            throw e;
        }
    }

    async getFingerprint(recipientHash: string): Promise<{ digits: string, isVerified: boolean }> {
        try {
            const result = await invoke<any>('signal_get_fingerprint', {
                remoteHash: recipientHash
            });
            return {
                digits: result.digits,
                isVerified: !!result.isVerified
            };
        } catch (e: any) {
            console.error("Fingerprint retrieval failed:", e);
            throw e;
        }
    }

    async verifySession(peerHash: string, isVerified: boolean): Promise<void> {
        try {
            await invoke('signal_set_peer_trust', {
                address: `${peerHash}:1`,
                trustLevel: isVerified ? 1 : 0
            });
        } catch (e: any) {
            console.error("Session verification failed:", e);
            throw e;
        }
    }

    /**
     * Group sessions in this architecture use Multi-Recipient Unicast via 1:1 sessions.
     * groupInit and createGroupDistribution provide unique context for group invites.
     */
    async groupInit(groupId: string): Promise<string> {
        // We generate a deterministic but unique key for the group session context
        const seed = crypto.getRandomValues(new Uint8Array(16));
        return toHex(seed);
    }

    async createGroupDistribution(groupId: string): Promise<string> {
        const seed = crypto.getRandomValues(new Uint8Array(16));
        return toHex(seed);
    }
}

export const signalManager = new SignalManager();
