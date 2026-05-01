import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { toHex } from './crypto';
import { vaultLoad, vaultSave } from './persistence';
import { userStore } from './stores/user';


export class SignalManager {
    private userIdentity: string = "";

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
}

export const signalManager = new SignalManager();
