# Security Policy

## Security Audits

The Entropy project is currently in **Alpha**.

## Vulnerability Disclosure

If you discover a security vulnerability, please do NOT open a public issue. Instead, follow our disclosure process:

1.  **Draft a detailed report** including the nature of the vulnerability, steps to reproduce, and potential impact.
2.  **Email the report** to `realmoyzy@gmail.com`.

We aim to acknowledge all reports within 48 hours and provide a fix or mitigation within 10 business days.

## Core Security Principles

- **Client-Side Only**: All cryptographic material is generated on the client. We do not support "cloud backups" of private keys.
- **Perfect Forward Secrecy**: We use a Double Ratchet algorithm to ensure that the compromise of one key does not reveal past or future messages.
- **Quantum Resistance**: We anticipate the future threat of quantum computing by integrating Kyber1024 at the initial handshake layer.
- **Zero-Trace Metadata**: We minimize metadata on relay nodes through Sealed Sender and Blinded Routing protocols.
