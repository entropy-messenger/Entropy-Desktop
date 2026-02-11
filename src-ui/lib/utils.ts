
/**
 * Extracts and metadata from a URL within a text string to generate a preview object.
 */
/**
 * Common cryptographic and UI utilities.
 */
import { network } from './network';

export const parseLinkPreview = async (text: string): Promise<any> => {
    const urlRegex = /(https?:\/\/[^\s]+)/g;
    const match = text.match(urlRegex);
    if (!match) return null;

    const url = match[0];
    try {
        console.debug(`[Privacy] Requesting proxied link preview for ${url}`);
        const response = await network.request('link_preview', { url });

        if (response && !response.error) {
            return {
                url,
                title: response.title || url.replace(/https?:\/\/(www\.)?/, '').split('/')[0],
                description: response.description,
                image: response.image,
                siteName: response.siteName || new URL(url).hostname
            };
        }
    } catch (e) {
        console.warn("Proxied link preview failed:", e);
    }
    return { url, title: url, siteName: new URL(url).hostname };
};

import { toHex as fastToHex, fromHex as fastFromHex } from './crypto';

export const fromHex = fastFromHex;
export const toHex = fastToHex;
