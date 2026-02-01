import { describe, it, expect } from 'vitest';
import { parseLinkPreview } from '../lib/utils';

describe('utils', () => {
    describe('parseLinkPreview', () => {
        it('should extract simple URLs', async () => {
            const text = "Check this out: https://example.com";
            const preview = await parseLinkPreview(text);
            expect(preview).not.toBeNull();
            expect(preview.url).toBe("https://example.com");
            expect(preview.siteName).toBe("example.com");
        });

        it('should return null if no URL found', async () => {
            const text = "Hello world";
            const preview = await parseLinkPreview(text);
            expect(preview).toBeNull();
        });
    });
});
