# Changelog

All notable changes to the Entropy Desktop Application will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.7] - 2026-04-16
### Hardening & Technical Debt Resolution
- **Networking Persistence**:
    - Integrated a 60-second sentinel timeout on WebSocket intake to mitigate stalled TCP states and semi-open connections.
- **Backend Architecture**:
    - Optimized the `src-tauri` codebase for full `clippy` and `rustfmt` compliance, adhering to the latest Rust stable standards.
    - Refactored disparate conditional chains into idiomatic Rust `&&` and `&& let` guard patterns for improved logic flow and reduced cognitive complexity.
    - Performed a workspace-wide whitespace sanitization to ensure consistent formatting across all build environments.
- **Application UX & Onboarding**:
    - Conditionalized the onboarding sequence to trigger exclusively for new identities, as determined by local vault existence.
    - Corrected the chat sidebar preview logic to dynamically re-evaluate and display the subsequent latest message upon parent message deletion.
    - Standardized application branding by replacing colloquial reset terminology with "Permanent Identity Reset."
    - Modernized the UI visual language with the integration of high-fidelity Lucide-Svelte iconography.
- **Improved Multimedia Support**:
    - Increased media file transfer limits from 10MB to 100MB for both 1:1 and group messaging.
    - Optimized reassembly buffers and tightened network frame limits (256KB) to align with the 1400-byte fragmentation architecture, enhancing DoS resilience.
    - Implemented real-time media transfer progress indicators (0-100%) for both uploads and downloads with high-fidelity UI feedback.

## [0.0.6] - 2026-04-14
- Fixed file path traversal vulnerability in message attachments.
- Professionalized and sanitized codebase for open-source release.
- Resolved sidebar state persistence issue (sent messages appearing as pending on restart).
- Fixed unread counter synchronization bug.

## [0.0.5] - 2026-04-13
- Official open-source release.
