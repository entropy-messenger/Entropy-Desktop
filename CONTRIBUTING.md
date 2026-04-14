# Contributing to Entropy

First off, thank you for considering contributing to Entropy. It's people like you that make Entropy a secure and robust messaging platform for everyone.

## Code of Conduct

By participating in this project, you agree to abide by the professional and respectful standards of the open-source community.

## How Can I Contribute?

### Reporting Bugs
*   Check the [Issues](https://github.com/entropy-messenger/Entropy-Desktop/issues) to see if the bug has already been reported.
*   If not, open a new issue. Include as much detail as possible, including your OS, and steps to reproduce.

### Suggesting Enhancements
*   Open an issue with the [Enhancement] tag.
*   Clearly describe the proposed change and why it would be beneficial.

### Pull Requests
1.  Fork the repository.
2.  Create a descriptive branch name (`feature/pqc-handshake-fix` or `bugfix/reassembly-overflow`).
3.  Ensure your code adheres to the existing styling and professional documentation standards.
4.  Submit a Pull Request targeting the `main` branch.

## Development Setup

### Prerequisites
*   **Rust**: [Standard installation](https://rustup.rs/) (latest stable).
*   **Node.js**: v18+ with NPM.
*   **System Dependencies**: 
    *   Linux: `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libssl-dev`, `libopus-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`.
    *   macOS: Xcode Command Line Tools.
    *   Windows: C++ Build Tools.

### Running Locally
1.  Clone the repository:
    ```bash
    git clone https://github.com/entropy-messenger/Entropy-Desktop.git
    cd DesktopApp
    ```
2.  Install dependencies:
    ```bash
    npm install
    ```
3.  Launch in development mode:
    ```bash
    npm run tauri dev
    ```

## Style Guide

### Rust
*   Run `cargo fmt` before committing.
*   Avoid informal or narrative comments. Keep technical documentation concise and professional.
*   Use `unwrap()` sparingly in production paths; prefer proper error handling.

### TypeScript / Svelte
*   Maintain the established Svelte 5 pattern for state management.
*   Ensure all new UI components are responsive and adhere to the project's high-aesthetic design system.

---

*Entropy is built with security as the first priority. All code changes related to the transit layer or cryptographic primitives will undergo rigorous review.*
