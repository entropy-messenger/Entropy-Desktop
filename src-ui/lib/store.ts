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
            const chatsCopy = JSON.parse(JSON.stringify(state.chats));
            for (const h in chatsCopy) {
                delete chatsCopy[h].isOnline;
                delete chatsCopy[h].isTyping;
                for (const m of chatsCopy[h].messages) {
                    if (m.attachment?.data) delete m.attachment.data;
                }
            }

            const vault = {
                chats: chatsCopy,
                myAlias: state.myAlias,
                myPfp: state.myPfp,
                blockedHashes: state.blockedHashes,
                privacySettings: state.privacySettings,
                sessionToken: state.sessionToken
            };

            await vaultSave(`entropy_chats_${state.identityHash}`, JSON.stringify(vault));
        } catch (e) {
            console.error("Failed to persist vault:", e);
        } finally {
            isSaving = false;
        }
    }, 2000);
});
