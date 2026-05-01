# Changelog

All notable changes to the Entropy Desktop Application will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-05-01
- Validation of the Signal-protocol pipeline with some tests.
- Integrated `cargo fmt` and `cargo clippy` into the CI pipeline to enforce strict code quality standards.
- Enabled opening exported files directly from the app.
- Added Emoji support.
- Implemented database connection pooling for faster parallel operations.
- Improved thread safety for cryptographic operations.
- Implemented "Just-In-Time" media decryption for smoother chat scrolling.
- Added thumbnails for encrypted images and videos.
- Unique visual waveforms for voice notes.
- Fixed path traversal security vulnerabilities.
- Significant performance boost for large file transfers.

## [0.1.0] - 2026-04-17
- **Cryptography & Identity**:
    - Implemented a robust **Session Revocation** mechanism allowing users to invalidate their active identity tokens from both Redis and local SQLite storage.
- **Typography & UX Elegance**:
    - Overhauled the application typography stack with **Plus Jakarta Sans** (UI/Headings) and **JetBrains Mono** (Technical/Input) for a nicer UI.
- **Networking & Backend**:
    - Integrated a 60-second sentinel timeout on WebSocket intake to mitigate stalled TCP states and semi-open connections.
    - Increased media file transfer limits from 10MB to 100MB with real-time progress indicators (0-100%).
    - Corrected chat sidebar preview logic for dynamic message deletion handling.

## [0.0.6] - 2026-04-14
- Fixed file path traversal vulnerability in message attachments.
- Professionalized and sanitized codebase for open-source release.
- Resolved sidebar state persistence issue (sent messages appearing as pending on restart).
- Fixed unread counter synchronization bug.

## [0.0.5] - 2026-04-13
- Official open-source release.
