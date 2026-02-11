
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
        const rawBundle = await invoke<any>('signal_get_bundle');

        if (!this.userIdentity && rawBundle.identityKey) {
            this.userIdentity = await calculateIdentityHash(rawBundle.identityKey);
        }

        if (!this.userIdentity) {
            throw new Error("Cannot upload keys: No identity hash available");
        }

        const bundle = {
            identity_hash: this.userIdentity,
            registrationId: rawBundle.registrationId,
            identityKey: toBase64(fromHex(rawBundle.identityKey)),
            signedPreKey: {
                id: rawBundle.signedPreKey.id,
                publicKey: toBase64(fromHex(rawBundle.signedPreKey.publicKey)),
                signature: toBase64(fromHex(rawBundle.signedPreKey.signature)),
                pq_publicKey: toBase64(fromHex(rawBundle.kyberPreKey.publicKey))
            },
            preKeys: [{
                id: rawBundle.preKey.id,
                publicKey: toBase64(fromHex(rawBundle.preKey.publicKey))
            }],
            pq_identityKey: toBase64(fromHex(rawBundle.kyberPreKey.publicKey)),
            kyberPreKey: {
                id: rawBundle.kyberPreKey.id,
                publicKey: toBase64(fromHex(rawBundle.kyberPreKey.publicKey)),
                signature: toBase64(fromHex(rawBundle.kyberPreKey.signature))
            }
        };

        const { network } = await import('./network');

        // Fetch challenge via WebSocket
        const challenge = await network.request('pow_challenge', { identity_hash: this.userIdentity });
        const { seed, difficulty } = challenge;
        const { nonce } = await minePoW(seed, difficulty, this.userIdentity);

        // Upload keys via WebSocket
        const res = await network.request('keys_upload', {
            ...bundle,
            seed,
            nonce
        });

        if (res.status !== 'success') {
            console.error("Key upload failed:", res.error);
            throw new Error(`Critical: Key upload failed: ${res.error}`);
        }
        console.log("Keys uploaded successfully.");
    }

    private sessionLocks: Map<string, Promise<string | null>> = new Map();

    /**
     * Fetches a peer's identity bundle and establishes a Double Ratchet session.
     * Implements Plausible Deniability by mixing the true recipient with decoy hashes.
     */
    async establishSession(recipientHash: string, serverUrl: string): Promise<string | null> {
        if (this.sessionLocks.has(recipientHash)) {
            return this.sessionLocks.get(recipientHash)!;
        }

        const sessionPromise = (async () => {
            try {
                const state = get(userStore);
                let targetParam = recipientHash;

                if (state.privacySettings.decoyMode && state.decoyHashes.length > 0) {
                    const decoys = [...state.decoyHashes]
                        .sort(() => 0.5 - Math.random())
                        .slice(0, 10)
                        .filter(h => h !== recipientHash);

                    const mixed = [recipientHash, ...decoys].sort(() => 0.5 - Math.random());
                    targetParam = mixed.join(',');
                    console.debug(`[Signal] Establish session with decoys: Requesting ${mixed.length} bundles`);
                }

                console.log(`[Signal] Fetching pre-key bundle for ${recipientHash}...`);

                const { network } = await import('./network');
                const res = await network.request('fetch_key', { target_hash: targetParam });

                if (!res.found) {
                    console.warn(`[Signal] Failed to fetch bundle for ${recipientHash}: Not Found`);
                    return null;
                }

                const bundle = res.bundles ? res.bundles[recipientHash] : res.bundle;

                if (!bundle) {
                    console.warn(`[Signal] Response found but bundle for ${recipientHash} is null`);
                    return null;
                }

                await invoke('signal_establish_session', {
                    remoteHash: recipientHash,
                    bundle
                });
                return "established";
            } catch (e: any) {
                console.error("Session establishment failed:", e);
                return null;
            }
        })();

        this.sessionLocks.set(recipientHash, sessionPromise);
        try {
            return await sessionPromise;
        } finally {
            this.sessionLocks.delete(recipientHash);
        }
    }

    /**
     * Encrypts a message for a peer. Automatically initiates session establishment if required.
     */
    async encrypt(recipientHash: string, message: string, serverUrl: string, skipIntegrity: boolean = false): Promise<any> {
        try {
            const encrypted = await invoke<any>('signal_encrypt', {
                remoteHash: recipientHash,
                message
            });
            return encrypted;
        } catch (e: any) {
            if (e.toString().includes("session") && e.toString().includes("not found")) {
                const status = await this.establishSession(recipientHash, serverUrl);
                if (status === "established") {
                    return await invoke<any>('signal_encrypt', {
                        remoteHash: recipientHash,
                        message
                    });
                }
            }
            console.error("Signal encryption failed after retry:", e);
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
     * Encrypts binary media using AES-GCM-256 and packages the key for Signal transmission.
     */
    async encryptMedia(data: Uint8Array, fileName: string, fileType: string): Promise<{ ciphertext: string, bundle: any }> {
        const key = await crypto.subtle.generateKey(
            { name: "AES-GCM", length: 256 },
            true,
            ["encrypt", "decrypt"]
        );

        const iv = crypto.getRandomValues(new Uint8Array(12));

        const encryptedBuffer = await crypto.subtle.encrypt(
            { name: "AES-GCM", iv },
            key,
            data as any
        );

        const exportedKey = await crypto.subtle.exportKey("raw", key);
        const keyBase64 = toBase64(new Uint8Array(exportedKey));

        const combined = new Uint8Array(iv.length + encryptedBuffer.byteLength);
        combined.set(iv, 0);
        combined.set(new Uint8Array(encryptedBuffer), iv.length);

        return {
            ciphertext: toHex(combined),
            bundle: {
                type: 'signal_media_v2',
                key: keyBase64,
                file_name: fileName,
                file_type: fileType,
                file_size: data.length
            }
        };
    }

    async decryptMedia(data: Uint8Array | string, bundle: any): Promise<Uint8Array> {
        if (!bundle || !bundle.key) {
            if (data instanceof Uint8Array) return data;
            return fromHex(data);
        }

        try {
            const combined = data instanceof Uint8Array ? data : fromHex(data);
            const keyBytes = fromBase64(bundle.key);

            const iv = combined.slice(0, 12);
            const encryptedData = combined.slice(12);

            const key = await crypto.subtle.importKey(
                "raw",
                keyBytes as any,
                "AES-GCM",
                true,
                ["decrypt"]
            );

            const decryptedBuffer = await crypto.subtle.decrypt(
                { name: "AES-GCM", iv },
                key,
                encryptedData as any
            );

            return new Uint8Array(decryptedBuffer);
        } catch (e) {
            console.error("[SignalManager] Media decryption failed:", e);
            throw e;
        }
    }

    async verifySession(remoteHash: string, isVerified: boolean): Promise<void> {
        // No-op
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
