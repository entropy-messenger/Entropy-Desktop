# Entropy · Desktop

Entropy is a post-quantum resistant, metadata-private messaging platform designed for maximum security without compromise. Built with Tauri and Rust, it provides a high-performance desktop experience powered by the Signal protocol and advanced transit-layer masking.

---

## Key Features

- **Post-Quantum Security**: Signal protocol integration with **Kyber-1024** PQ-XDH for resistance against FUTURE quantum computing threats.
- **Metadata Resistance**: Uniform **1400-byte packet padding** and dummy pacing to disrupt traffic analysis.
- **True Sovereignty**: Zero-knowledge local storage. All data is encrypted at rest using AES-256 (SQLCipher).
- **Flexible Routing**: Native support for Direct, Tor, and custom relay paths to bypass regional censorship.
- **Rich Media**: Secure E2EE file transfers with specialized reassembly and vault storage.
- **Panic Mechanism**: Instant database destruction and "Nuke" capabilities for physical security scenarios.

## Technical Architecture

Entropy is built on a "Rust-first" philosophy where all cryptographic and networking logic resides in a hardened backend, isolated from the UI layer.

- **Frontend**: Svelte 5 (Runes) + Vanilla CSS.
- **Backend**: Rust (Tauri).
- **Cryptography**: Signal (libsignal-protocol-rust), Kyber-1024, SQLCipher.
- **Desktop**: Cross-platform (Linux, macOS, Windows).

## Getting Started

### Prerequisites

You will need **Rust**, **Node.js**, and your system's Tauri dependencies. See [CONTRIBUTING.md](CONTRIBUTING.md) for a full checklist.

### Build from Source

1.  **Clone & Install**:
    ```bash
    git clone https://github.com/entropy-messenger/Entropy-Desktop.git
    cd DesktopApp
    npm install
    ```

2.  **Dev Mode**:
    ```bash
    npm run tauri dev
    ```

3.  **Build Binary**:
    ```bash
    npm run tauri build
    ```

## Project Structure

```text
├── src-tauri/             # Rust Backend (Security & Core logic)
│   ├── src/
│   │   ├── commands/      # Tauri command bridge
│   │   │   ├── messaging/ # Inbox, Outbox, and Group logic
│   │   │   ├── network/   # Transit layer, fragmentation, and pacing
│   │   │   ├── identity/  # Nicknames and Trust management
│   │   │   └── vault/     # SQLCipher persistence and media storage
│   │   ├── app_state.rs   # Thread-safe global state containers
│   │   ├── main.rs        # Application entry and window lifecycle
│   │   └── signal_store.rs # SQLite-backed Signal protocol storage
├── src-ui/                # Svelte 5 Frontend (Design & UX)
    ├── lib/               # Global stores, Network layer, and IPC actions
    ├── components/        # Svelte components and modular UI elements
    └── assets/            # Static assets and branding


## Security & Contributions

Please read our [SECURITY.md](SECURITY.md) for vulnerability disclosure and our [CONTRIBUTING.md](CONTRIBUTING.md) for information on how to help grow the project.

---

## License

Entropy is open-source software licensed under the [GNU Affero General Public License (AGPL)](LICENSE).

