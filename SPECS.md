# Entropy Desktop Specification

This document defines the cryptographic and architectural specifications for the Entropy Desktop client.

## 1. Cryptographic Primitive Layer

Entropy uses a hybrid cryptographic stack combining classical elliptic curve primitives with post-quantum key encapsulation mechanisms.

*   **Identity (Classical)**: Ed25519 (using `ed25519-dalek`)
*   **Key Agreement (Classical)**: X25519 (using `x25519-dalek`)
*   **Key Agreement (PQ)**: Kyber1024 (`NIST PQC Round 3` finalist)
*   **Symmetric Encryption**: AES-256-GCM (using `aes-gcm`)
*   **Hashing**: SHA-256
*   **KDF**: HKDF-SHA256

---

## 2. Session Establishment (X3DH+PQ)

Before any message can be exchanged, a session is established using an extended **X3DH (Extended Triple Diffie-Hellman)** handshake, augmented with **Post-Quantum (PQ)** safety.

### 2.1 The Classic Bundle
- `IK_b`: Bob's static Identity Key.
- `SPK_b`: Bob's Signed Pre-Key.
- `OPK_b`: Bob's One-Time Pre-Key (optional).

### 2.2 The PQ Bundle
- `PQ_IK_b`: Bob's Post-Quantum Identity Key (Kyber1024).
- `PQ_SPK_b`: Bob's Post-Quantum Signed Pre-Key (Kyber1024).

### 2.3 Key Derivation
The shared secret `SK` is computed by concatenating several Diffie-Hellman shared secrets and Kyber shared secrets:
```
KM = DH1 || DH2 || DH3 [|| DH4] || PQ_SS1 || PQ_SS2
```
Where:
- `DH1` = `DH(IK_a, SPK_b)`
- `DH2` = `DH(EK_a, IK_b)`
- `DH3` = `DH(EK_a, SPK_b)`
- `DH4` = `DH(EK_a, OPK_b)` (if OPK present)
- `PQ_SS1` = `KEM_Encapsulate(PQ_IK_b)`
- `PQ_SS2` = `KEM_Encapsulate(PQ_SPK_b)`

The root key is then derived:
`RootKey = HKDF(KM, salt=None, info="EntropyV1 X3DH+PQ")`

---

## 3. Message Continuity Lock (Hash Chain)

To prevent message reordering, deletion, or "history splitting" attacks, Entropy implements a **Continuity Lock** (Hash Chain) mechanism.

### 3.1 The Mechanism
Every message payload includes an `lh` (Last Hash) field.
- For Message `n`, `lh = SHA256(Ciphertext(n-1))`.
- When a client receives Message `n`, it checks if `lh` matches the hash of the last message it processed.

### 3.2 Continuity Breaks
If `lh` does not match, a `CONTINUITY_BREAK` error is triggered. The client will reject the message until the chain is healed. This functions similarly to a blockchain's block hash, ensuring the linear integrity of the conversation.

---

## 4. Sealed Sender Flow

Entropy uses a "Sealed Sender" mechanism to minimize metadata exposure to relay nodes.

1.  **Preparation**: The sender encrypts the message payload and the sender's identity using a ephemeral shared key derived from the recipient's public key.
2.  **Blinding**: The resulting "Envelope" is signed using the sender's identity.
3.  **Relay Logic**: The relay node sees a package addressed to a `TargetHash` but cannot verify who the sender is without decrypting the outer layer.

---

## 5. Storage Security (Vault)

All local data is stored in an SQLite database encrypted via **SQLCipher**.
- **PBKDF2**: Used to derive the database encryption key from the user's master password.
- **Salt**: A unique, machine-specific salt is stored in the OS Keyring (using the `keyring` crate) to prevent offline brute-force attacks on the database file without access to the local machine.

---

## 6. Proof of Work (PoW)

To prevent spam and DDoS on the relay network, specific actions (Account Creation, Key Upload) require solving a SHA-256 partial collision challenge.
- **Target**: Number of leading zeros (difficulty) is defined by the relay server.
- **Context**: Challenges are tied to the requester's IP and a server-provided seed to prevent replay attacks.

---

## 7. Client-Side Traffic Normalization

The desktop app contributes to metadata resistance on its end:

### 7.1 Message Padding
- All outgoing messages are padded with random data before encryption to hide the original message length
- Padding follows a bucketing strategy (e.g., messages rounded up to 512B, 1KB, 5KB, 10KB thresholds)
- Server-side further normalizes to 1536 bytes, ensuring consistent wire format

### 7.2 Dummy Traffic Generation
- Client automatically sends `ping` messages to the server during idle periods
- Responds to server `dummy_pacing` packets with acknowledgments
- Maintains constant connection activity to mask when real messages are being sent
- **Note**: This feature is currently disabled by default to save bandwidth but can be enabled in settings

### 7.3 Decoy Requests
- Client periodically fetches random user key bundles (via `/keys/random`) even when not initiating conversations
- Hides which users you're actually talking to from network observers
- Decoy traffic is indistinguishable from legitimate key fetches

### 7.4 Send Queue Timing
- Messages aren't sent immediately when typed
- Small random delay (100-300ms) added before encryption and transmission
- Breaks correlation between typing patterns and network activity

---

## 8. Message Flow & Handling

### Sending Messages
1. User types a message in the UI (Svelte component)
2. Frontend calls Rust backend via Tauri commands
3. Rust encrypts the message using the Double Ratchet session key
4. Encrypted envelope is sent to relay server via WebSocket
5. Server routes to recipient or stores offline

### Receiving Messages
1. WebSocket receives encrypted envelope from server
2. Rust backend decrypts and verifies the message
3. Continuity hash (`lh`) is checked against previous message
4. If valid, message is stored in SQLCipher vault and emitted to UI
5. Desktop notification is triggered (if enabled)

---

## 9. File Attachment Handling

Files are encrypted client-side before upload:

1. **Chunking**: Files >5MB are split into chunks of 5MB each
2. **Encryption**: Each chunk is encrypted with AES-256-GCM using a random file key
3. **Upload**: Chunks are sent to the relay server (stored in Redis with TTL)
4. **Recipient Download**: Recipient fetches chunks, decrypts locally, and reassembles
5. **Burn After Read**: Chunks are deleted from server after successful download

---

## 10. Group Chat Mechanics

Group messages use **individual encryption** (no shared group keys):

1. Sender encrypts the message separately for each group member using their pairwise session
2. Server receives N encrypted envelopes (one per member)
3. Each envelope is routed independently to its recipient
4. Recipients decrypt using their own session key

This ensures that compromising one member's key doesn't expose the entire group history.

---

## 11. Application Architecture

### Rust Backend (Tauri)
- **`protocol_init()`**: Generates Ed25519 identity and X25519/Kyber key pairs
- **`protocol_sign()`**: Signs messages with identity key
- **`init_vault()` / `vault_save()` / `vault_load()`**: SQLCipher database operations
- **`store_secret()` / `get_secret()`**: OS keyring integration for salt storage
- **`crypto_sha256()`**: Hashing utility exposed to frontend

### TypeScript Frontend
- **`SignalManager`**: Manages session establishment, key rotation, and ratcheting
- **`Network`**: WebSocket connection, authentication, and message relay
- **`Auth`**: Identity creation, password verification, vault initialization
- **`ChatStore`**: Svelte reactive store for UI state (messages, contacts, typing indicators)
- **`AttachmentStore`**: File encryption, chunking, download management

---

## 12. Build Targets & CI

The desktop app builds for:
- **Linux**: `.deb`, `.AppImage`
- **macOS**: `.dmg`, `.app`
- **Windows**: `.msi`, `.exe`

CI runs on GitHub Actions with test suites for both frontend (Vitest) and backend (Rust `cargo test`).
