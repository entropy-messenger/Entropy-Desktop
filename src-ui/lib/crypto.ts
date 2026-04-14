/**
 * Utils for any frontend Encoding conversion
 */

const HEX_TABLE = Array.from({ length: 256 }, (_, i) => i.toString(16).padStart(2, '0'));

export const toHex = (bytes: Uint8Array | number[]): string => {
    let res = '';
    for (let i = 0; i < bytes.length; i++) {
        res += HEX_TABLE[bytes[i]];
    }
    return res;
};

export const fromHex = (hex: string): Uint8Array => {
    const cleanHex = hex.trim();
    const bytes = new Uint8Array(cleanHex.length / 2);
    for (let i = 0; i < bytes.length; i++) {
        bytes[i] = parseInt(cleanHex.substring(i * 2, i * 2 + 2), 16);
    }
    return bytes;
};

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

export const fromBase64 = (base64: string): Uint8Array => {
    try {
        const binString = atob(base64.replace(/[\s\n\r]/g, ''));
        const len = binString.length;
        const bytes = new Uint8Array(len);
        for (let i = 0; i < len; i++) {
            bytes[i] = binString.charCodeAt(i);
        }
        return bytes;
    } catch (e) {
        return new Uint8Array();
    }
};
