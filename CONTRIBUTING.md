# Contributing to agy-orbit

Thank you for your interest in contributing to `agy-orbit` (`agyo`)!

All code contributions are expected to pass the automated quality verification suite (`verify_gauntlet.ps1` / `verify_gauntlet.sh`) with zero warnings.

---

## 1. Code of Invariants (The Contractual Invariant)

When contributing, you **must never violate** our core invariants:
1. **Never touch, mirror, or delete** `~/.gemini/antigravity-cli/brain/`, `plugins/`, or `presence/`.
2. **Never modify** `HOME` or `USERPROFILE` in ways that break developer toolchains (Git, OpenSSH, Cargo, npm, Docker).
3. **Strictly scope authentication mutations** to the 3 audited targets:
   - `~/.gemini/oauth_creds.json`
   - `~/.gemini/google_accounts.json`
   - OS Keyring (`LegacyGeneric:target=gemini:antigravity`)
4. **All persistent state must live in `~/.agyo/`**, never in `~/.gemini/`.

---

## 2. Development Workflow

1. Fork the repository and create your branch from `main`:
   ```bash
   git checkout -b feat/your-feature-name
   ```
2. Implement your changes following **Test-Driven Development (TDD)**:
   - Write failing unit/integration tests first.
   - Implement the minimal production code to pass.
   - Refactor cleanly.
3. Run the quality checks before submitting:
   ```powershell
   # On Windows
   powershell -ExecutionPolicy Bypass -File .\scripts\verify_gauntlet.ps1

   # On Linux / macOS
   ./scripts/verify_gauntlet.sh
   ```
   **Every gate must pass 100% green with 0 Clippy warnings.**

---

## 3. Commit Message Guidelines

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```text
<type>(<scope>): <short summary>

[optional body]

[optional footer(s)]
```

Types:
- `feat`: A new feature
- `fix`: A bug fix
- `docs`: Documentation only changes
- `refactor`: A code change that neither fixes a bug nor adds a feature
- `perf`: A code change that improves performance
- `test`: Adding missing tests or correcting existing tests
- `ci`: Changes to CI configuration files and scripts
- `chore`: Other changes that don't modify src or test files
