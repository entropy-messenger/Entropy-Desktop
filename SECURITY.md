# Security Policy

## Vulnerability Disclosure

At Entropy, the security of our users' communications is our highest priority. We appreciate all work in helping us keep Entropy safe.

If you discover a vulnerability, we ask that you disclose it to us privately to give us an opportunity to fix it before it is exploited.

## Reporting a Vulnerability

Please report vulnerabilities by sending a detailed description of the issue to:
**moyzy@entropymessenger.com**

Include the following in your report:
*   A clear description of the vulnerability.
*   The potential impact of the issue.
*   Steps to reproduce (a proof-of-concept is highly appreciated).
*   Your preferred name for credit in our CHANGELOG (optional).

We aim to acknowledge all reports within 48 hours and provide a fix for critical issues as rapidly as possible.

## Core Encryption Principles

The Entropy Desktop App relies on the following primitives for E2EE:
*   **PQ-XDH**: Post-Quantum Extended Diffie-Hellman using **Kyber-1024**.
*   **Double Ratchet**: Signal's Double Ratchet implementation for forward secrecy.
*   **VDF-PoW**: Verifiable Delay Functions used for Proof-of-Work to mitigate spam.
*   **Argon2id**: Industry-standard password-based key derivation (KDF) for secure vault initialization.
*   **SQLCipher**: Local state persistence encrypted with **AES-256** via SQLCipher.

## Out of Scope
*   Vulnerabilities in third-party services (e.g., Tor relays) unless they directly impact the Entropy client.
*   Physical attacks on the user's device.
*   Social engineering attacks.

Thank you for helping keep Entropy secure.
