import { writable } from 'svelte/store';

export interface Transfer {
    id: number;
    sender?: string;
    current: number;
    total: number;
    direction: 'upload' | 'download';
    msgId?: string;
}

export const transfers = writable<Record<number, Transfer>>({});

export const updateTransferProgress = (id: number, current: number, total: number, direction: 'upload' | 'download', sender?: string, msgId?: string) => {
    transfers.update(t => {
        t[id] = { id, current, total, direction, sender, msgId };
        return { ...t };
    });
};

export const removeTransfer = (id: number) => {
    transfers.update(t => {
        delete t[id];
        return { ...t };
    });
};
