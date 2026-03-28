import { writable } from 'svelte/store';
import type { Chat, Message, PrivacySettings } from '../types';

/**
 * Primary state manifest for the Entropy desktop client.
 */
export interface AppState {
    identityHash: string | null;
    myAlias: string | null;
    myPfp: string | null;
    chats: Record<string, Chat>;
    isConnected: boolean;
    activeChatHash: string | null;
    searchQuery: string;
    replyingTo: Message | null;
    blockedHashes: string[];
    privacySettings: PrivacySettings;
    sessionToken: string | null;
    isSynced: boolean;
    connectionStatus: 'disconnected' | 'connecting' | 'mining' | 'connected' | 'sync_error';
    authError: string | null;
    relayUrl: string;
}

const initialState: AppState = {
    identityHash: null,
    myAlias: null,
    myPfp: null,
    chats: {},
    isConnected: false,
    activeChatHash: null,
    searchQuery: "",
    replyingTo: null,
    blockedHashes: [],
    privacySettings: {
        readReceipts: true,
        typingStatus: 'everyone',
        profilePhoto: 'everyone',
        routingMode: 'direct',
        proxyUrl: 'socks5://127.0.0.1:9050',
        theme: 'dark'
    },
    sessionToken: null,
    isSynced: false,
    connectionStatus: 'disconnected',
    authError: null,
    relayUrl: import.meta.env.VITE_RELAY_URL || 'http://localhost:8080'
};

/**
 * Global reactive store for the application state.
 */
export const userStore = writable<AppState>(initialState);

/**
 * Transient store for message histories.
 * Holds messages only for loaded conversations to prevent OOM and store bloat.
 */
export const messageStore = writable<Record<string, Message[]>>({});
