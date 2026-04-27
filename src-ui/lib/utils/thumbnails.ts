
/**
 * Generates a high-quality micro-thumbnail (base64) for images and videos.
 * Targeted size: 5-8KB.
 */
export async function generateThumbnail(file: File): Promise<string | null> {
    const isImage = file.type.startsWith('image/');
    const isVideo = file.type.startsWith('video/');

    if (!isImage && !isVideo) return null;

    return new Promise((resolve) => {
        if (isImage) {
            const reader = new FileReader();
            reader.onload = (e) => {
                const img = new Image();
                img.onload = () => {
                    const canvas = document.createElement('canvas');
                    // We want a small but recognizable thumbnail
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
                        // Low quality but looks good blurred
                        resolve(canvas.toDataURL('image/webp', 0.6));
                    } else {
                        resolve(null);
                    }
                };
                img.src = e.target?.result as string;
            };
            reader.readAsDataURL(file);
        } else if (isVideo) {
            const video = document.createElement('video');
            video.preload = 'metadata';
            video.muted = true;
            video.src = URL.createObjectURL(file);
            
            video.onloadedmetadata = () => {
                video.currentTime = 0.5; // Seek a bit to avoid black frame
            };

            video.onseeked = () => {
                const canvas = document.createElement('canvas');
                const maxDim = 120;
                let width = video.videoWidth;
                let height = video.videoHeight;

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
                    ctx.drawImage(video, 0, 0, width, height);
                    const data = canvas.toDataURL('image/webp', 0.6);
                    URL.revokeObjectURL(video.src);
                    resolve(data);
                } else {
                    URL.revokeObjectURL(video.src);
                    resolve(null);
                }
            };

            video.onerror = () => {
                URL.revokeObjectURL(video.src);
                resolve(null);
            };
        }
    });
}
