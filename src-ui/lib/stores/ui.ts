/**
 * Manages global UI notifications and asynchronous modal dialogs.
 */
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

import { writable } from 'svelte/store';

/** 
 * Collection of active notifications. 
 */
export const toasts = writable<Toast[]>([]);

/** 
 * Current active modal configuration. 
 */
export const modal = writable<ModalOptions | null>(null);

/**
 * Dispatches a new toast notification.
 */
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
 * Triggers a confirmation dialog and returns a promise resolving to the user's choice.
 */
export const showConfirm = (message: string, title = 'Confirm'): Promise<boolean> => {
    return new Promise((resolve) => {
        modal.set({
            title,
            message,
            type: 'confirm',
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

/**
 * Triggers an input prompt and returns a promise resolving to the user's input or null if cancelled.
 */
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
