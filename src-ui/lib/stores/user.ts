import { writable } from 'svelte/store';
import type { Chat, Message, PrivacySettings } from '../types';

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
    myGlobalNickname: string | null;
    nicknameExpiry: number | null;
    connectionStatus: 'disconnected' | 'connecting' | 'mining' | 'connected';
    authError: string | null;
    keysMissing: boolean;
    relayUrl: string;
    activeAudioId: string | null;
    activeAudioChatId: string | null;
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
    myGlobalNickname: null,
    nicknameExpiry: null,
    privacySettings: {
        readReceipts: true,
        lastSeen: 'everyone',
        profilePhoto: 'everyone',
        routingMode: 'direct',
        proxyUrl: '',
        decoyMode: false,
        forceTurn: false,
        iceServers: []
    },
    sessionToken: null,
    connectionStatus: 'disconnected',
    authError: null,
    keysMissing: false,
    relayUrl: import.meta.env.VITE_RELAY_URL || 'http://localhost:8080',
    activeAudioId: null,
    activeAudioChatId: null
};

export const userStore = writable<AppState>(initialState);
