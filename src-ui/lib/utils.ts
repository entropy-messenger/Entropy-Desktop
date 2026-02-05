import { invoke } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import { userStore } from './stores/user';

export const parseLinkPreview = async (text: string): Promise<any> => {
    const urlRegex = /(https?:\/\/[^\s]+)/g;
    const match = text.match(urlRegex);
    if (!match) return null;

    const url = match[0];
    const store = get(userStore);
    const proxyUrl = store.privacySettings.routingMode !== 'direct' ? store.privacySettings.proxyUrl : undefined;

    try {
        const preview = await invoke('get_link_preview', {
            url,
            proxyUrl
        });
        return preview;
    } catch (e) {
        return { url, title: url, siteName: new URL(url).hostname };
    }
};

export const fromHex = (hex: string): Uint8Array => {
    return new Uint8Array(hex.match(/.{1,2}/g)!.map(byte => parseInt(byte, 16)));
};

export const toHex = (bytes: Uint8Array): string => {
    return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
};

export const fromBase64 = (s: string): Uint8Array => {
    return new Uint8Array(atob(s).split('').map(c => c.charCodeAt(0)));
};

export const toBase64 = (b: Uint8Array): string => {
    return btoa(String.fromCharCode(...b));
};
