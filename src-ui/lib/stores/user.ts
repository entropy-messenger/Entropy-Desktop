import { writable } from 'svelte/store';
import type { Chat, Message, PrivacySettings } from '../types';
import { vaultSave } from '../persistence';

export interface AppState {
    identityHash: string | null;
    globalNickname: string | null;
    chats: Record<string, Chat>;
    isConnected: boolean;
    activeChatHash: string | null;
    searchQuery: string;
    replyingTo: Message | null;
    blockedHashes: string[];
    privacySettings: PrivacySettings;
    isSynced: boolean;
    connectionStatus: 'disconnected' | 'connecting' | 'mining' | 'connected' | 'sync_error' | 'reconnecting' | 'jailed';
    reconnectTimer: number | null;
    jailTimeRemaining: number | null;
    authError: string | null;
    nicknames: Record<string, string>;
}

const initialState: AppState = {
    identityHash: null,
    globalNickname: null,
    chats: {},
    isConnected: false,
    activeChatHash: null,
    searchQuery: "",
    replyingTo: null,
    blockedHashes: [],
    privacySettings: {
        readReceipts: true,
        typingStatus: 'everyone',
        routingMode: 'direct',
        proxyUrl: 'socks5://127.0.0.1:9050',
        theme: 'dark'
    },
    isSynced: false,
    connectionStatus: 'disconnected',
    reconnectTimer: null,
    jailTimeRemaining: null,
    authError: null,
    nicknames: {}
};

export const userStore = writable<AppState>(initialState);

export const messageStore = writable<Record<string, Message[]>>({});

let saveTimeout: any = null;

async function performSave(state: AppState) {
    if (!state.identityHash) return;
    try {
        const metadata = {
            globalNickname: state.globalNickname,
            blockedHashes: state.blockedHashes,
            privacySettings: state.privacySettings
        };
        await vaultSave(`entropy_meta_${state.identityHash}`, JSON.stringify(metadata));
    } catch (e) {
        // Auto-save failed
    }
}

userStore.subscribe(state => {
    if (!state.identityHash) return;
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => performSave(state), 1000);
});
