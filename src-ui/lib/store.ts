/**
 * Central aggregate for application state, asynchronous actions, and persistence logic.
 */
export { userStore } from './stores/user';
export type { AppState } from './stores/user';
export * from './actions/auth';
export * from './actions/messaging';
export * from './actions/message_utils';
export * from './actions/contacts';
export * from './actions/groups';
export * from './utils';
export * from './types';

import { userStore } from './stores/user';
import { vaultSave } from './secure_storage';

let saveTimeout: any = null;

/**
 * Serializes and persists global application metadata (settings, identity) to the vault.
 * Chats and messages are now managed separately in SQLite.
 */
async function performSave(state: any) {
    if (!state.identityHash) return;
    try {
        const metadata = {
            myAlias: state.myAlias,
            myPfp: state.myPfp,
            blockedHashes: state.blockedHashes,
            privacySettings: state.privacySettings,
            sessionToken: state.sessionToken
        };

        await vaultSave(`entropy_meta_${state.identityHash}`, JSON.stringify(metadata));
        console.debug("[Persistence] Saved global metadata successfully.");
    } catch (e) {
        console.error("[Persistence] Failed to persist metadata:", e);
    }
}

/**
 * Global subscription to the user store to trigger debounced persistence.
 */
userStore.subscribe(state => {
    if (!state.identityHash) return;

    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => {
        performSave(state);
    }, 1000);
});
