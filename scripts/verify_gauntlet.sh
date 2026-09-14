#!/usr/bin/env bash
# agy-orbit Quality Verification Automation (Linux / macOS)
set -euo pipefail

export RUSTFLAGS="-D warnings"

echo "========================================"
echo "  agy-orbit Quality Verification Suite  "
echo "========================================"

# Gate 1: Clippy Static Analysis (Zero tolerance for warnings)
echo -e "\n[Gate 1/3] Running Cargo Clippy (-D warnings)..."
cargo clippy --all-targets -- -D warnings

# Gate 2: Code Formatting Verification
echo -e "\n[Gate 2/3] Checking Code Formatting (cargo fmt)..."
cargo fmt --all --check

# Gate 3: Full Test Suite
echo -e "\n[Gate 3/3] Running Cargo Test Suite (all targets & doc tests)..."
cargo test --all-targets
cargo test --doc

echo -e "\n✅ [ALL PASSED] All 3 quality checks are 100% GREEN!"
exit 0
