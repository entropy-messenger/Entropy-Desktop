import { describe, it, expect, vi, beforeEach } from 'vitest';
import { NetworkLayer } from '../lib/network';


vi.mock('@tauri-apps/api/core', () => ({
    invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn(),
}));

vi.mock('svelte/store', async (importOriginal) => {
    const actual = await importOriginal() as any;
    return {
        ...actual,
        get: vi.fn((store: any) => {
            if (store.mockData) return store.mockData;
            return { relayUrl: 'http://localhost:8080', privacySettings: { routingMode: 'direct' } };
        }),
    };
});

import { invoke } from '@tauri-apps/api/core';

describe('NetworkLayer', () => {
    let network: NetworkLayer;

    beforeEach(() => {
        vi.clearAllMocks();
        network = new NetworkLayer();
    });

    it('should attempt to connect via native invoke', async () => {
        vi.mocked(invoke).mockResolvedValue(undefined);






        (network as any).userStoreModule = {
            userStore: {
                subscribe: vi.fn().mockReturnValue(() => { }),
                update: vi.fn(),
            }
        };

        await network.connect();
        expect(invoke).toHaveBeenCalledWith('connect_network', expect.anything());
    });

    it('should queue messages when disconnected', async () => {
        (network as any).isConnected = false;
        network.sendJSON({ type: 'test' });
        expect((network as any).messageQueue.length).toBe(1);
    });
});
