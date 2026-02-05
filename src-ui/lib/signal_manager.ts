import { invoke } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import { userStore } from './stores/user';
import { minePoW } from './pow';

export class SignalManager {
    private userIdentity: string = "";
    private operationLock: Promise<any> = Promise.resolve();

    private async lock<T>(fn: () => Promise<T>): Promise<T> {
        const result = this.operationLock.then(fn);
        this.operationLock = result.catch(() => { });
        return result;
    }

    async init(_password: string): Promise<string | null> {
        return this.lock(async () => {
            const identityBundle: any = await invoke('protocol_init');
            this.userIdentity = identityBundle.registration_id.toString();
            return this.userIdentity;
        });
    }

    getUserId(): string {
        return this.userIdentity;
    }

    async ensureKeysUploaded(serverUrl: string, force: boolean = false) {
        return this.lock(async () => {
            const state = get(userStore);
            if (!force && !state.keysMissing) return;

            userStore.update(s => ({ ...s, connectionStatus: 'mining' }));

            try {
                // 1. Get PoW challenge
                const challengeRes = await fetch(`${serverUrl}/pow/challenge?identity_hash=${this.userIdentity}`);
                const { seed, difficulty } = await challengeRes.json();

                // 2. Mine PoW
                const { nonce } = await minePoW(seed, difficulty, this.userIdentity);

                // 3. Prepare Upload Bundle (minimal but valid for server)
                const identityBundle: any = await invoke('protocol_init');
                const ik_b64 = await this.getPublicIdentityKey();

                // Build object for signature (server checks this)
                const bundleToSign = {
                    identityKey: ik_b64,
                    pq_identityKey: ik_b64, // Reuse ik for dummy PQ
                    signedPreKey: { pq_publicKey: ik_b64 }, // Reuse ik
                    preKeys: []
                };

                const signature = await this.signMessage(JSON.stringify(bundleToSign));

                const bundle = {
                    ...bundleToSign,
                    identity_hash: this.userIdentity,
                    registrationId: identityBundle.registration_id,
                    bundle_signature: signature
                };

                const response = await fetch(`${serverUrl}/keys/upload`, {
                    method: 'POST',
                    headers: {
                        'Content-Type': 'application/json',
                        'X-PoW-Seed': seed,
                        'X-PoW-Nonce': nonce
                    },
                    body: JSON.stringify(bundle)
                });

                if (!response.ok) {
                    const err = await response.json();
                    throw new Error(err.error || `Upload failed: ${response.statusText}`);
                }

                userStore.update(s => ({ ...s, connectionStatus: 'connected', keysMissing: false }));
            } catch (e) {
                console.error("Key upload failed:", e);
                userStore.update(s => ({ ...s, connectionStatus: 'connected' }));
                throw e;
            }
        });
    }

    async encrypt(recipientHash: string, message: string, _relayUrl?: string, _isVolatile: boolean = false): Promise<any> {
        // Plaintext envelope
        return {
            type: 1,
            body: message,
            sender: this.userIdentity,
            to: recipientHash
        };
    }

    async decrypt(_senderHash: string, ciphertext: any): Promise<any> {
        // Just extract body
        try {
            return JSON.parse(ciphertext.body);
        } catch (e) {
            return ciphertext.body;
        }
    }

    async signMessage(message: string): Promise<string> {
        return await invoke('protocol_sign', { message });
    }

    async getPublicIdentityKey(): Promise<string> {
        return await invoke('protocol_get_identity_key');
    }

    async verifySession(remoteHash: string, verified: boolean): Promise<void> {
        await invoke('protocol_verify_session', { remoteHash, verified });
    }

    async encryptMedia(data: Uint8Array, fileName: string, fileType: string): Promise<{ ciphertext: string, bundle: any }> {
        const bundle = { file_name: fileName, file_type: fileType };
        // We use hex to avoid base64 overhead in bridge
        const hexData = Array.from(data).map(b => b.toString(16).padStart(2, '0')).join('');
        return { ciphertext: hexData, bundle };
    }

    async decryptMedia(hexData: string, _bundle: any): Promise<Uint8Array> {
        const bytes = new Uint8Array(hexData.match(/.{1,2}/g)!.map(byte => parseInt(byte, 16)));
        return bytes;
    }

    async groupInit(groupId: string): Promise<any> {
        return { group_id: groupId, members: [] };
    }

    async createGroupDistribution(_groupId: string): Promise<any> {
        return { type: 'dummy_distribution' };
    }

    async groupEncrypt(_groupId: string, message: string): Promise<any> {
        return { body: message };
    }

    async groupDecrypt(_groupId: string, _senderHash: string, msgObj: any): Promise<string> {
        return msgObj.body || "";
    }

    async exportIdentity(): Promise<Uint8Array> {
        const bytes = await invoke('protocol_export_vault') as number[];
        return new Uint8Array(bytes);
    }

    async importIdentity(bytes: Uint8Array): Promise<void> {
        await invoke('protocol_import_vault', { bytes: Array.from(bytes) });
    }
}

export const signalManager = new SignalManager();
