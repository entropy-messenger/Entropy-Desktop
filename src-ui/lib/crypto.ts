/**
 * Cryptographic and serialization utilities for the Entropy client.
 * Provides optimized codecs and native bridge interfaces for computationally intensive tasks.
 */

export const initCrypto = async () => {
};

const HEX_TABLE = Array.from({ length: 256 }, (_, i) => i.toString(16).padStart(2, '0'));

/**
 * Encodes a byte array to a hexadecimal string.
 */
export const toHex = (bytes: Uint8Array): string => {
    let res = '';
    for (let i = 0; i < bytes.length; i++) {
        res += HEX_TABLE[bytes[i]];
    }
    return res;
};

/**
 * Decodes a hexadecimal string to a byte array.
 */
export const fromHex = (hex: string): Uint8Array => {
    const cleanHex = hex.trim();
    const bytes = new Uint8Array(cleanHex.length / 2);
    for (let i = 0; i < bytes.length; i++) {
        bytes[i] = parseInt(cleanHex.substring(i * 2, i * 2 + 2), 16);
    }
    return bytes;
};

/**
 * Encodes a byte array to a Base64 string.
 * Uses chunked processing to avoid stack overflow on large buffers.
 */
export const toBase64 = (bytes: Uint8Array): string => {
    let binary = '';
    const len = bytes.byteLength;
    const CHUNK_SIZE = 0x4000;
    for (let i = 0; i < len; i += CHUNK_SIZE) {
        const chunk = bytes.subarray(i, i + CHUNK_SIZE);
        binary += String.fromCharCode(...chunk);
    }
    return btoa(binary);
};

/**
 * Decodes a Base64 string to a byte array.
 * Includes fallback logic for unpadded or whitespace-polluted strings.
 */
export const fromBase64 = (base64: string): Uint8Array => {
    try {
        const binString = atob(base64);
        const len = binString.length;
        const bytes = new Uint8Array(len);
        for (let i = 0; i < len; i++) {
            bytes[i] = binString.charCodeAt(i);
        }
        return bytes;
    } catch (e) {
        try {
            const cleaned = base64.replace(/[\s\n\r]/g, '');
            const binString = atob(cleaned);
            const bytes = new Uint8Array(binString.length);
            for (let i = 0; i < binString.length; i++) {
                bytes[i] = binString.charCodeAt(i);
            }
            return bytes;
        } catch (e2) {
            console.error("Base64 decode failed", e2);
            return new Uint8Array(0);
        }
    }
};

import { invoke } from '@tauri-apps/api/core';

/**
 * Computes a SHA-256 hash via the native Rust bridge.
 */
export const sha256 = async (input: Uint8Array | string): Promise<string> => {
    const data = typeof input === 'string' ? new TextEncoder().encode(input) : input;
    return await invoke('crypto_sha256', { data: Array.from(data) });
};

/**
 * Solves a Proof-of-Work challenge using the native multi-threaded miner.
 * @param seed The server-provided challenge seed.
 * @param difficulty Number of leading zero nibbles required.
 * @param context Additional data (e.g., identity hash) to bind to the seed.
 */
export const minePoW = async (seed: string, difficulty: number = 3, context: string = ""): Promise<{ nonce: number, seed: string }> => {
    const result: any = await invoke('crypto_mine_pow', { seed, difficulty, context });
    return { nonce: result.nonce, seed: seed };
};
