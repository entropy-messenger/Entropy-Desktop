import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SignalStore } from '../lib/signal_store';
import * as secureStorage from '../lib/secure_storage';

vi.mock('../lib/secure_storage', () => ({
    vaultSave: vi.fn(),
    vaultLoad: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
    invoke: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';

describe('SignalStore', () => {
    let store: SignalStore;

    beforeEach(() => {
        vi.clearAllMocks();
        store = new SignalStore();
    });

    it('should set encryption key', () => {
        const key = new Uint8Array([1, 2, 3]);
        store.setEncryptionKey(key);

    });

    it('should call clear_vault on deleteAllData', async () => {
        await store.deleteAllData();
        expect(invoke).toHaveBeenCalledWith('clear_vault');
    });

    it('should save and load message hashes', async () => {
        const localHash = 'local';
        const peerHash = 'peer';
        const msgHash = 'hash123';

        await store.putLastMessageHash(localHash, peerHash, 'sent', msgHash);
        expect(secureStorage.vaultSave).toHaveBeenCalledWith(
            'signal_integrity_sent_local_peer',
            msgHash
        );

        vi.mocked(secureStorage.vaultLoad).mockResolvedValue(msgHash);
        const loaded = await store.getLastMessageHash(localHash, peerHash, 'sent');
        expect(loaded).toBe(msgHash);
        expect(secureStorage.vaultLoad).toHaveBeenCalledWith('signal_integrity_sent_local_peer');
    });

    it('should move integrity hashes', async () => {
        const localHash = 'local';
        const oldPeer = 'old';
        const newPeer = 'new';
        const msgHash = 'hash123';

        vi.mocked(secureStorage.vaultLoad).mockImplementation(async (key) => {
            if (key.includes('old')) return msgHash;
            return null;
        });

        await store.moveIntegrityHash(localHash, oldPeer, newPeer);

        expect(secureStorage.vaultSave).toHaveBeenCalledWith('signal_integrity_sent_local_new', msgHash);
        expect(secureStorage.vaultSave).toHaveBeenCalledWith('signal_integrity_sent_local_old', 'null');
    });

    it('should identify a blank slate correctly', async () => {
        vi.mocked(secureStorage.vaultLoad).mockResolvedValue(null);
        let isBlank = await store.isBlankSlate();
        expect(isBlank).toBe(true);

        vi.mocked(secureStorage.vaultLoad).mockResolvedValue('some-key');
        isBlank = await store.isBlankSlate();
        expect(isBlank).toBe(false);
    });
});
