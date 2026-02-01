# Contributing to Entropy Desktop

First off, thank you for considering contributing to Entropy Desktop! We need people like you to keep the messaging ecosystem private and resilient.

## Code of Conduct

By participating in this project, you agree to abide by our standards of professional and respectful collaboration.

## How Can I Contribute?

### Reporting Bugs
*   Check the Issues tab to see if the bug has already been reported.
*   If not, open a new issue. Include your environment details (OS, Tauri version).
*   Provide a clear reproduction case.

### Suggesting Enhancements
*   Open an issue with the tag `enhancement`.
*   Explain why this feature is useful and how it aligns with Entropy's "Zero-Knowledge" philosophy.

### Pull Requests
1.  Fork the repo and create your branch from `main`.
2.  If you've added code that should be tested, add tests.
3.  If you've changed APIs, update the documentation.
4.  Make sure your code follows the existing style (Modern Rust/TypeScript standards).

## Technical Standards

*   **Core (Rust)**: Stable Rust, `async` driven, stronger error typing via `anyhow` or `thiserror` (avoid raw `unwrap` where possible). 
*   **Frontend (Svelte)**: TypeScript, TailwindCSS/Vanilla for styling. Clean component architecture.
*   **Protocol**: Strict adherence to the blinded-routing protocol defined in the server specs.

## Style Guide

We prefer a clean, coding style:
*   Use Doxygen-style (///) comments for Rust public interfaces.
*   Avoid commenting the obvious.
*   Keep functions focused and small.

---

*Entropy is a community-driven project. Your contributions help protect digital freedom for everyone.*
