import { invoke } from '@tauri-apps/api/core';
import _sodium from 'libsodium-wrappers';

let sodium: typeof _sodium;

export const initCrypto = async () => {
    await _sodium.ready;
    sodium = _sodium;
};



export const deriveVaultKey = async (password: string, salt: string): Promise<Uint8Array> => {
    return await invoke('crypto_pbkdf2', { password, salt });
};

export const encryptWithKey = async (plaintext: string, key: Uint8Array): Promise<string> => {
    const encoder = new TextEncoder();
    return await invoke('crypto_encrypt', { key, plaintext: Array.from(encoder.encode(plaintext)) });
};

export const decryptWithKey = async (hex: string, key: Uint8Array): Promise<string | null> => {
    try {
        const decoded: number[] = await invoke('crypto_decrypt', { key, hexData: hex });
        return new TextDecoder().decode(new Uint8Array(decoded));
    } catch (e) {
        return null;
    }
};

export const encryptBinary = async (data: Uint8Array, key: Uint8Array): Promise<Uint8Array> => {
    const hex: string = await invoke('crypto_encrypt', { key, plaintext: Array.from(data) });
    return fromHex(hex);
};

export const decryptBinary = async (combined: Uint8Array, key: Uint8Array): Promise<Uint8Array | null> => {
    try {
        const decoded: number[] = await invoke('crypto_decrypt', { key, hexData: toHex(combined) });
        return new Uint8Array(decoded);
    } catch (e) {
        return null;
    }
};

export const sha256 = async (input: Uint8Array | string): Promise<string> => {
    const data = typeof input === 'string' ? input : toHex(input);
    return await invoke('crypto_sha256', { data });
};

export const minePoW = async (seed: string, difficulty: number = 3, context: string = ""): Promise<{ nonce: number, seed: string }> => {
    const result: any = await invoke('crypto_mine_pow', { seed, difficulty, context });
    return { nonce: result.nonce, seed: seed };
};



export const toHex = (bytes: Uint8Array): string => {
    if (!sodium) return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
    return sodium.to_hex(bytes);
};

export const fromHex = (hex: string): Uint8Array => {
    if (!sodium) return new Uint8Array(hex.match(/.{1,2}/g)!.map(byte => parseInt(byte, 16)));
    return sodium.from_hex(hex);
};

export const toBase64 = (bytes: Uint8Array): string => {
    if (!sodium) return btoa(String.fromCharCode(...bytes));
    return sodium.to_base64(bytes);
};

export const fromBase64 = (base64: string): Uint8Array => {
    if (!sodium) return Uint8Array.from(atob(base64), c => c.charCodeAt(0));
    try {
        return sodium.from_base64(base64);
    } catch (e) {
        
        
        const cleaned = base64.replace(/[\s\n\r]/g, '');
        return Uint8Array.from(atob(cleaned), c => c.charCodeAt(0));
    }
};
