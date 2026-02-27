
import { invoke } from '@tauri-apps/api/core';
import { minePoW, toBase64, fromBase64, toHex, fromHex } from './crypto';
import { secureStore, secureLoad, vaultLoad, vaultSave } from './secure_storage';

/**
 * Derives a unique identity fingerprint from the Signal public key.
 * Used as the primary lookup key for peers globally.
 */
async function calculateIdentityHash(idKeyHex: string): Promise<string> {
    const bytes = fromHex(idKeyHex);
    const hashBuffer = await crypto.subtle.digest('SHA-256', bytes as any);
    return Array.from(new Uint8Array(hashBuffer))
        .map(b => b.toString(16).padStart(2, '0'))
        .join('');
}


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
            const idKeyHex = await invoke<string>('signal_init');
            const idHash = await calculateIdentityHash(idKeyHex);
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
            console.log("Keys synchronized successfully.");
        } catch (e: any) {
            console.error("Key synchronization failed:", e);
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

    async decrypt(senderHash: string, ciphertext: any): Promise<any> {
        if (!ciphertext || typeof ciphertext !== 'object' || !ciphertext.body) {
            return null;
        }

        // Handle server-side wrapping for group messages
        let signalMsg = ciphertext;
        if (ciphertext.type === 'sealed_message' && ciphertext.msg_type !== undefined) {
            signalMsg = {
                ...ciphertext,
                type: ciphertext.msg_type
            };
        }

        if (signalMsg.type === undefined) {
            return null;
        }

        try {
            const plaintext = await invoke<string>('signal_decrypt', {
                remoteHash: senderHash,
                msgObj: signalMsg
            });
            return JSON.parse(plaintext);
        } catch (e) {
            console.error("Signal decryption failed:", e);
            return null;
        }
    }

    /**
     * Encrypts binary media using native Rust AES-GCM-256 bridge.
     * Prevents UI thread blocking and memory spikes in the browser layer.
     */
    async encryptMedia(data: Uint8Array, fileName: string, fileType: string): Promise<{ ciphertext: string, bundle: any }> {
        const result = await invoke<any>('crypto_encrypt_media', { data });

        return {
            ciphertext: result.ciphertext,
            bundle: {
                type: 'signal_media_v2',
                key: result.key,
                file_name: fileName,
                file_type: fileType,
                file_size: data.length
            }
        };
    }

    async encryptFile(path: string, fileName: string, fileType: string, fileSize: number): Promise<{ ciphertext: string, bundle: any }> {
        const result = await invoke<any>('crypto_encrypt_file', { path });

        return {
            ciphertext: result.ciphertext,
            bundle: {
                type: 'signal_media_v2',
                key: result.key,
                file_name: fileName,
                file_type: fileType,
                file_size: result.file_size
            }
        };
    }

    /**
     * Decrypts binary media using native Rust AES-GCM-256 bridge.
     */
    async decryptMedia(data: Uint8Array | string, bundle: any): Promise<Uint8Array> {
        if (!bundle || !bundle.key) {
            if (data instanceof Uint8Array) return data;
            return fromHex(data);
        }

        try {
            const ciphertextHex = typeof data === 'string' ? data : toHex(data);
            const plaintext = await invoke<number[]>('crypto_decrypt_media', {
                ciphertextHex,
                keyB64: bundle.key
            });
            return new Uint8Array(plaintext);
        } catch (e) {
            console.error("[SignalManager] Native media decryption failed:", e);
            throw e;
        }
    }

    /**
     * Generates a numeric "Safety Number" (Fingerprint) for a peer.
     * Combines both identity public keys and hashes them into a scannable format.
     */
    async getFingerprint(remoteHash: string): Promise<{ digits: string, isVerified: boolean } | null> {
        try {
            const ownIdBytes = await invoke<Uint8Array | number[]>('signal_get_own_identity');
            const peerData = await invoke<[Uint8Array | number[], number] | null>('signal_get_peer_identity', {
                address: `${remoteHash}:1`
            });

            if (!peerData) return null;

            const [peerIdBytes, trustLevel] = peerData;

            const ownArr = new Uint8Array(ownIdBytes);
            const peerArr = new Uint8Array(peerIdBytes);

            let combined: Uint8Array;
            const ownHash = await this.getUserId();
            if (remoteHash < ownHash) {
                combined = new Uint8Array([...peerArr, ...ownArr]);
            } else {
                combined = new Uint8Array([...ownArr, ...peerArr]);
            }

            const hashBuffer = await crypto.subtle.digest('SHA-256', combined as any);
            const hashArray = Array.from(new Uint8Array(hashBuffer));

            // Take bytes and convert to blocks of 5 digits
            let digits = "";
            for (let i = 0; i < 12; i++) {
                const chunk = hashArray.slice(i * 2, (i * 2) + 2);
                const val = (chunk[0] << 8) | chunk[1];
                digits += (val % 100000).toString().padStart(5, '0') + (i === 5 ? '\n' : ' ');
            }

            return {
                digits: digits.trim(),
                isVerified: trustLevel === 1
            };
        } catch (e) {
            console.error("Failed to generate fingerprint:", e);
            return null;
        }
    }

    async verifySession(remoteHash: string, isVerified: boolean): Promise<void> {
        await invoke('signal_set_peer_trust', {
            address: `${remoteHash}:1`,
            trustLevel: isVerified ? 1 : 0
        });
    }

    async replenishPreKeys(serverUrl: string): Promise<void> {
        // No-op
    }

    async groupInit(groupId: string): Promise<any> {
        return { status: 'plaintext_group' };
    }

    async groupEncrypt(groupId: string, message: string): Promise<any> {
        return {
            type: 'plaintext_group',
            body: message,
            nonce: '0',
            key_id: 0
        };
    }

    async groupDecrypt(groupId: string, senderHash: string, msgObj: any): Promise<string> {
        if (msgObj && msgObj.body) {
            return msgObj.body;
        }
        return "";
    }

    async processGroupDistribution(senderHash: string, distObj: any): Promise<void> {
        // No-op
    }

    async createGroupDistribution(groupId: string): Promise<any> {
        return {};
    }

    async seal(remoteIdentityKey: string, message: any): Promise<any> {
        return message;
    }

    async unseal(sealedObj: any): Promise<any> {
        return sealedObj;
    }

    async exportIdentity(): Promise<Uint8Array> {
        return new Uint8Array([]);
    }

    async importIdentity(data: Uint8Array | string): Promise<void> {
        // No-op
    }

    async remoteBurn(serverUrl: string): Promise<boolean> {
        try {
            // 1. Prove ownership to server to delete remote/mesh copy
            const timestamp = Date.now().toString();
            const signature = await this.signMessage(`BURN_ACCOUNT:${this.userIdentity}:${timestamp}`);

            await fetch(`${serverUrl}/account`, {
                method: 'DELETE',
                headers: {
                    'X-Identity': this.userIdentity,
                    'X-Signature': signature,
                    'X-Timestamp': timestamp
                }
            });
        } catch (e) {
            console.error("Remote burn failed (network):", e);
            // Proceed to local burn anyway
        }

        localStorage.clear();
        await invoke('nuclear_reset');
        return true;
    }

    async signMessage(message: string): Promise<string> {
        return await invoke<string>('signal_sign_message', { message });
    }
}

export const signalManager = new SignalManager();
