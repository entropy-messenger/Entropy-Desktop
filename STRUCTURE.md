# 📂 Project Structure

This document provides a high-level overview of the Entropy Desktop codebase.

```text
.
├── src-tauri/               # Rust (Tauri) Backend
│   ├── src/
│   │   ├── protocol/        # Core Cryptography & Logic
│   │   │   ├── crypto.rs    # X3DH, PQ, and Ratchet math
│   │   │   ├── mod.rs       # Protocol entry point
│   │   │   ├── types.rs     # Structs and DB Schema
│   │   │   └── utils.rs     # Encoding helpers
│   │   ├── tests/           # Integration & Security Tests
│   │   ├── commands.rs      # Tauri IPC Command handlers
│   │   └── main.rs          # App lifecycle & Tray config
│   └── Cargo.toml           # Backend dependencies
│
├── src-ui/                  # Svelte 5 Frontend
│   ├── components/          # Reusable UI Components
│   │   ├── Sidebar.svelte   # Chat list & search
│   │   ├── ChatWindow.svelte# Message rendering
│   │   └── TitleBar.svelte  # Frameless window controls
│   ├── lib/
│   │   ├── signal_manager.ts# JS Glue for Rust Protocol
│   │   ├── user_store.ts    # Global state management
│   │   └── network.ts       # WebSocket client logic
│   ├── tests/               # Frontend Unit Tests
│   └── App.svelte           # Main Entry & Routing
│
├── .github/                 # CI/CD Workflows
├── README.md                # Project Overview
├── SPECS.md                 # Protocol Specifications
├── API.md                   # Network API Documentation
├── SECURITY.md              # Security Policy
└── CONTRIBUTING.md          # Development Roadmap
```

## 🛠️ Key Design Patterns

1.  **Strict IPC Separation**: No business logic exists in the UI layer. All cryptographic operations are handled by the Rust backend via asynchronous IPC calls.
2.  **Stateless UI**: The frontend reflects the state of the backend vault. On app reload, the session is re-ratcheted from the local database.
3.  **Cross-Language Resilience**: Errors from Rust results are gracefully mapped to UI notifications (using a unified `Result<T, String>` pattern).
