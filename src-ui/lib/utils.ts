
export const parseLinkPreview = async (text: string): Promise<any> => {
    const urlRegex = /(https?:\/\/[^\s]+)/g;
    const match = text.match(urlRegex);
    if (!match) return null;

    const url = match[0];
    try {
        const response = await fetch(url, { mode: 'no-cors' });

        return {
            url,
            title: url.replace(/https?:\/\/(www\.)?/, '').split('/')[0],
            siteName: new URL(url).hostname
        };
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
