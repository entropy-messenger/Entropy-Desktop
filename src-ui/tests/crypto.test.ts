import { describe, it, expect, beforeAll } from 'vitest';
import { toHex, fromHex, toBase64, fromBase64, initCrypto } from '../lib/crypto';

describe('crypto utilities', () => {
    beforeAll(async () => {
        await initCrypto();
    });

    it('should convert to hex and back', () => {
        const bytes = new Uint8Array([72, 101, 108, 108, 111]);
        const hex = toHex(bytes);
        expect(hex).toBe('48656c6c6f');
        const back = fromHex(hex);
        expect(back).toEqual(bytes);
    });

    it('should convert to base64 and back', () => {
        const bytes = new Uint8Array([72, 101, 108, 108, 111]);
        const b64 = toBase64(bytes);


        expect(b64.startsWith('SGVsbG8')).toBe(true);
        const back = fromBase64(b64);
        expect(back).toEqual(bytes);
    });

    it('should handle complex base64 inputs', () => {
        const complex = 'SGVsbG8gV29ybGQh';
        const back = fromBase64(complex);
        expect(new TextDecoder().decode(back)).toBe('Hello World!');
    });
});
