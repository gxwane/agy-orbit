# Security Policy

## 1. Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

---

## 2. Reporting a Vulnerability

We take the security of `agy-orbit` seriously. If you discover a security vulnerability, please **DO NOT** create a public GitHub issue.

Instead, please report vulnerabilities via:
- **GitHub Private Vulnerability Reporting**: Go to the Security tab of the repository and click "Report a vulnerability".
- **Email**: Contact the maintainers directly at `gxwane@outlook.com`.

### Our Security SLA
- **Initial Acknowledgement**: Within 48 hours.
- **Triage & Impact Assessment**: Within 5 business days.
- **Patch Release & Advisory**: Delivered as rapidly as possible, typically within 14 days of confirmed severity.

---

## 3. Security Design Constraints & Anti-Malware Invariants

`agy-orbit` strictly adheres to verifiable engineering constraints to ensure transparent, non-malicious execution:

1. **No Credential Enumeration**: Only targets the specific Antigravity entry (`LegacyGeneric:target=gemini:antigravity`). Does not read, dump, or scan other user credentials.
2. **No Process Injection**: Never calls `CreateRemoteThread`, `ptrace`, or injects shellcode into foreign processes.
3. **No API Hooking**: Never hooks or intercepts Win32, glibc, or macOS system calls.
4. **No Binary Packing**: Distributed as a clean, deterministic Rust binary without UPX or obfuscation packers.
5. **Zero Outbound Credential Telemetry**: Never transmits tokens, keys, or user sessions across the network.
6. **No Silent Persistence**: Never installs background services, startup registry keys, or scheduled tasks.
7. **Documented APIs Only**: Relies exclusively on official Windows Win32 (DPAPI, CredMan), macOS Keychain Services, and Linux SecretService APIs.
