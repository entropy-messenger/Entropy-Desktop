# 🌐 Entropy Network API Reference

The Entropy Desktop client interacts with decentralized relay nodes via a set of REST and WebSocket endpoints. This document summarizes the expected interaction patterns.

## 1. REST API

These endpoints are used for initialization and identity synchronization.

### `GET /pow/challenge`
Fetches a Proof-of-Work challenge.
- **Query Params**: `type` (e.g., `decoy`, `key_upload`), `identity_hash` (optional)
- **Response**: `{ "seed": "...", "difficulty": 20 }`

### `POST /keys/upload`
Uploads the user's cryptographic bundle.
- **Headers**: `X-PoW-Seed`, `X-PoW-Nonce`
- **Body**: Standard X3DH bundle including identity keys, pre-keys, and signatures.

### `GET /keys/fetch`
Fetches the key bundle for a specific user hash.
- **Query Params**: `user` (Comma-separated list of hashes)
- **Response**: Dictionary of identity bundles.

### `POST /account/burn`
Deletes the account and keys from the server. Requires a valid signature to prevent unauthorized deletion.

---

## 2. WebSocket Protocol

The primary channel for real-time messaging.

### Connection
`WS /connect`
Clients must authenticate by providing their identity hash and solving an initial PoW challenge.

### Message Events (Incoming)
- `msg`: Encrypted message payload.
- `receipt`: Delivery confirmation.
- `presence`: Peer online/offline status.

### Message Events (Outgoing)
- `send`: Sends an encrypted envelope to a recipient hash.
- `ack`: Acknowledges receipt of a message.

---

## 3. Blinded Routing

Entropy does not use public keys for routing. Instead:
1.  Clients compute `SHA256(PublicKey)` to create an **Identity Hash**.
2.  Relay nodes route based on this hash.
3.  The server never sees the raw public key of either the sender or the receiver if **Sealed Sender** is used correctly.
