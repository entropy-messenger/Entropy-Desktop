export { userStore } from './stores/user';
export type { AppState } from './stores/user';
export * from './actions/auth';
export * from './actions/messaging'; // Exports core messaging logic
export * from './actions/message_utils'; // Exports utils like addMessage, bulkDelete used by UI
export * from './actions/contacts';
export * from './actions/groups';
export * from './utils';

// Re-export types if needed by consumers of store.ts
export * from './types';

import { userStore } from './stores/user';
import { signalManager } from './signal_manager';
import { vaultSave } from './secure_storage';

// -----------------------------------------------------------------------------
// Global Audio Management
// -----------------------------------------------------------------------------
export const setActiveAudio = (audioId: string, chatId: string) => {
    userStore.update(s => ({ ...s, activeAudioId: audioId, activeAudioChatId: chatId }));
};

export const clearActiveAudio = (audioId?: string) => {
    userStore.update(s => {
        if (audioId && s.activeAudioId !== audioId) return s;
        return { ...s, activeAudioId: null, activeAudioChatId: null };
    });
};

// -----------------------------------------------------------------------------
// Subscriptions & Background Logic
// -----------------------------------------------------------------------------

// 1. Upload keys when connected if keys missing
userStore.subscribe(s => {
    if (s.connectionStatus === 'connected' && s.keysMissing) {
        signalManager.ensureKeysUploaded(s.relayUrl, true).catch(e => console.error("Re-upload failed:", e));
        userStore.update(state => ({ ...state, keysMissing: false }));
    }
});

// 2. Persistence (Auto-save)
let saveTimeout: any = null;
let isSaving = false;

userStore.subscribe(state => {
    if (!state.identityHash || isSaving) return;

    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(async () => {
        try {
            isSaving = true;

            // Fast, shallow-ish clone. We only need to strip volatile UI state.
            const chatsToSave: Record<string, any> = {};
            for (const [h, chat] of Object.entries(state.chats)) {
                chatsToSave[h] = {
                    ...chat,
                    isOnline: undefined,
                    isTyping: undefined
                };
            }

            const vault = {
                chats: chatsToSave,
                myAlias: state.myAlias,
                myPfp: state.myPfp,
                blockedHashes: state.blockedHashes,
                privacySettings: state.privacySettings,
                sessionToken: state.sessionToken,
                relayUrl: state.relayUrl
            };

            await vaultSave(`entropy_vault_${state.identityHash}`, JSON.stringify(vault));
        } catch (e) {
            console.error("Auto-save failed:", e);
        } finally {
            isSaving = false;
        }
    }, 1000); // 1 second debounce for stability
});
