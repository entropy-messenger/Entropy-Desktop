export type ToastType = 'info' | 'success' | 'error' | 'warning';

export interface Toast {
    id: string;
    message: string;
    type: ToastType;
    duration?: number;
}

export interface ModalOptions {
    title: string;
    message: string;
    type: 'confirm' | 'prompt';
    defaultValue?: string;
    confirmText?: string;
    cancelText?: string;
    onConfirm: (value?: string) => void;
    onCancel: () => void;
}

export interface LightboxOptions {
    id?: string;
    src: string | null;
    alt: string;
    fileName: string;
    fileType?: string;
    size: number;
    type: 'image' | 'video';
    content?: string;
    timestamp?: number;
    senderNickname?: string;
}

export interface ContextMenuOptions {
    x: number;
    y: number;
    visible: boolean;
    fileName: string;
    label: string;
    onSave: () => void;
}

import { writable } from 'svelte/store';

export const toasts = writable<Toast[]>([]);
export const modal = writable<ModalOptions | null>(null);
export const lightbox = writable<LightboxOptions | null>(null);
export const contextMenu = writable<ContextMenuOptions | null>(null);
export const mediaProxyPort = writable<number | null>(null);

export const addToast = (message: string, type: ToastType = 'info', duration = 3000) => {
    const id = Math.random().toString(36).substring(2, 9);
    toasts.update(all => [{ id, message, type, duration }, ...all]);
    if (duration > 0) {
        setTimeout(() => removeToast(id), duration);
    }
    return id;
};

export const removeToast = (id: string) => {
    toasts.update(all => all.filter(t => t.id !== id));
};

/**
 * Standard confirmation dialog.
 * Defaults button labels to "Yes" and "No" per user preference.
 */
export const showConfirm = (message: string, title = 'Confirm', confirmText = 'Yes', cancelText = 'No'): Promise<boolean> => {
    return new Promise((resolve) => {
        modal.set({
            title,
            message,
            type: 'confirm',
            confirmText,
            cancelText,
            onConfirm: () => {
                modal.set(null);
                resolve(true);
            },
            onCancel: () => {
                modal.set(null);
                resolve(false);
            }
        });
    });
};

export const showPrompt = (message: string, defaultValue = '', title = 'Input'): Promise<string | null> => {
    return new Promise((resolve) => {
        modal.set({
            title,
            message,
            defaultValue,
            type: 'prompt',
            onConfirm: (val) => {
                modal.set(null);
                resolve(val || '');
            },
            onCancel: () => {
                modal.set(null);
                resolve(null);
            }
        });
    });
};
