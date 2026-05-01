
/**
 * Generates a high-quality micro-thumbnail (base64) for images and videos.
 * Targeted size: 5-8KB.
 */
export async function generateThumbnail(url: string, type: string): Promise<string | null> {
    const isImage = type.startsWith('image/');
    const isVideo = type.startsWith('video/');

    if (!isImage && !isVideo) return null;

    return new Promise((resolve) => {
        if (isImage) {
            const img = new Image();
            img.crossOrigin = 'anonymous';
            img.onload = () => {
                    const canvas = document.createElement('canvas');
                    const maxDim = 120; 
                    let width = img.width;
                    let height = img.height;

                    if (width > height) {
                        height *= maxDim / width;
                        width = maxDim;
                    } else {
                        width *= maxDim / height;
                        height = maxDim;
                    }

                    canvas.width = width;
                    canvas.height = height;
                    const ctx = canvas.getContext('2d');
                    if (ctx) {
                        ctx.imageSmoothingEnabled = true;
                        ctx.imageSmoothingQuality = 'high';
                        ctx.drawImage(img, 0, 0, width, height);
                        resolve(canvas.toDataURL('image/webp', 0.6));
                    } else {
                        resolve(null);
                    }
                };
                img.onerror = () => resolve(null);
                img.src = url;
        } else if (isVideo) {
            const video = document.createElement('video');
            video.preload = 'metadata';
            video.muted = true;
            video.crossOrigin = 'anonymous';
            video.playsInline = true;

            const timeout = setTimeout(() => resolve(null), 8000);

            const captureFrame = () => {
                clearTimeout(timeout);
                const canvas = document.createElement('canvas');
                const maxDim = 120;
                const w = video.videoWidth || 320;
                const h = video.videoHeight || 240;
                const ratio = Math.min(maxDim / w, maxDim / h);
                canvas.width = Math.round(w * ratio);
                canvas.height = Math.round(h * ratio);
                const ctx = canvas.getContext('2d');
                if (ctx && canvas.width > 0 && canvas.height > 0) {
                    ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
                    try {
                        resolve(canvas.toDataURL('image/webp', 0.6));
                    } catch {
                        resolve(null);
                    }
                } else {
                    resolve(null);
                }
                video.src = '';
            };

            video.onloadedmetadata = () => {
                video.currentTime = Math.min(0.5, (video.duration * 0.1) || 0.5);
            };

            // Register capture AFTER seek completes, not before
            video.onseeked = () => {
                if ('requestVideoFrameCallback' in video) {
                    (video as any).requestVideoFrameCallback(captureFrame);
                } else {
                    captureFrame();
                }
            };

            video.onerror = () => {
                clearTimeout(timeout);
                resolve(null);
            };

            video.src = url;
        }
    });
}
