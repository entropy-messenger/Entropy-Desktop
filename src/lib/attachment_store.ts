import { encryptBinary, decryptBinary } from './crypto';

export class AttachmentStore {
    private dbName = 'entropy_attachments';
    private storeName = 'files';
    private db: IDBDatabase | null = null;
    private encryptionKey: Uint8Array | null = null;

    setEncryptionKey(key: Uint8Array | null) {
        this.encryptionKey = key;
    }

    async init(): Promise<void> {
        return new Promise((resolve, reject) => {
            const request = indexedDB.open(this.dbName, 1);
            request.onupgradeneeded = (e: any) => {
                const db = e.target.result;
                if (!db.objectStoreNames.contains(this.storeName)) {
                    db.createObjectStore(this.storeName);
                }
            };
            request.onsuccess = (e: any) => {
                this.db = e.target.result;
                resolve();
            };
            request.onerror = (e) => reject(e);
        });
    }

    async put(id: string, data: Uint8Array): Promise<void> {
        if (!this.db) await this.init();

        let finalData: Uint8Array = data;
        if (this.encryptionKey) {
            finalData = await encryptBinary(data, this.encryptionKey);
        }

        return new Promise((resolve, reject) => {
            const transaction = this.db!.transaction([this.storeName], 'readwrite');
            const store = transaction.objectStore(this.storeName);
            const request = store.put(finalData, id);
            request.onsuccess = () => resolve();
            request.onerror = (e) => reject(e);
        });
    }

    async get(id: string): Promise<Uint8Array | null> {
        if (!this.db) await this.init();
        return new Promise((resolve, reject) => {
            const transaction = this.db!.transaction([this.storeName], 'readonly');
            const store = transaction.objectStore(this.storeName);
            const request = store.get(id);
            request.onsuccess = async (e: any) => {
                const data = e.target.result;
                if (!data) {
                    resolve(null);
                    return;
                }

                if (this.encryptionKey) {
                    const decrypted = await decryptBinary(data, this.encryptionKey);
                    resolve(decrypted);
                } else {
                    resolve(data);
                }
            };
            request.onerror = (e) => reject(e);
        });
    }

    async delete(id: string): Promise<void> {
        if (!this.db) await this.init();
        return new Promise((resolve, reject) => {
            const transaction = this.db!.transaction([this.storeName], 'readwrite');
            const store = transaction.objectStore(this.storeName);
            const request = store.delete(id);
            request.onsuccess = () => resolve();
            request.onerror = (e) => reject(e);
        });
    }
}

export const attachmentStore = new AttachmentStore();
