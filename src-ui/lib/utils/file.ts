export function getFileMimeType(fileName: string): string {
    const ext = fileName.split('.').pop()?.toLowerCase();
    if (['jpg', 'jpeg', 'png', 'webp', 'gif'].includes(ext || '')) return `image/${ext === 'jpg' ? 'jpeg' : ext}`;
    if (['mp4', 'webm', 'mov', 'ogg'].includes(ext || '')) return `video/${ext === 'mov' ? 'quicktime' : ext}`;
    return 'application/octet-stream';
}
