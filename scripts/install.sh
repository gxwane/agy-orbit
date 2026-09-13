#!/usr/bin/env bash
set -euo pipefail

echo "Installing agy-orbit (agyo) from source..."

cargo install --path . --force

echo ""
echo "✓ agyo has been successfully installed to Cargo bin directory!"
echo "Run 'agyo --help' to get started."
