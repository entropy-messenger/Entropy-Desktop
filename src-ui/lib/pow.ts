
export async function minePoW(seed: string, difficulty: number, context: string = ""): Promise<{ nonce: string }> {
    let nonce = 0;
    const encoder = new TextEncoder();
    const seedBytes = encoder.encode(seed);
    const contextBytes = encoder.encode(context);

    while (true) {
        const nonceStr = nonce.toString();
        const nonceBytes = encoder.encode(nonceStr);

        // Combine seed + context + nonce
        const combined = new Uint8Array(seedBytes.length + contextBytes.length + nonceBytes.length);
        combined.set(seedBytes);
        combined.set(contextBytes, seedBytes.length);
        combined.set(nonceBytes, seedBytes.length + contextBytes.length);

        const hashBuffer = await crypto.subtle.digest('SHA-256', combined);
        const hashArray = new Uint8Array(hashBuffer);

        let zeros = 0;
        for (const byte of hashArray) {
            if (byte === 0) {
                zeros += 2;
            } else {
                if ((byte & 0xF0) === 0) zeros += 1;
                break;
            }
        }

        if (zeros >= difficulty) {
            return { nonce: nonceStr };
        }

        nonce++;
        if (nonce % 2000 === 0) {
            // Yield to main thread
            await new Promise(resolve => setTimeout(resolve, 0));
        }
    }
}
