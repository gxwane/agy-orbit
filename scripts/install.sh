#!/usr/bin/env bash
# agy-orbit Unix Installer (POSIX Bash)
# Installs prebuilt binary from GitHub Releases or from local source
set -euo pipefail

main() {
  local VERSION="${AGYO_VERSION:-latest}"
  local FROM_SOURCE=0

  while [[ $# -gt 0 ]]; do
    case "$1" in
      -v|--version)
        VERSION="$2"
        shift 2
        ;;
      --from-source)
        FROM_SOURCE=1
        shift
        ;;
      -h|--help)
        echo "agy-orbit Installer"
        echo "Usage: install.sh [--version <tag>] [--from-source]"
        exit 0
        ;;
      *)
        echo "Unknown option: $1"
        exit 1
        ;;
    esac
  done

  echo ""
  echo "==============================================="
  echo "        agy-orbit (agyo) Unix Installer        "
  echo "==============================================="
  echo ""

  if [[ $FROM_SOURCE -eq 1 ]]; then
    echo "Installing agy-orbit from local source via Cargo..."
    if ! command -v cargo >/dev/null 2>&1; then
      echo "Error: Cargo is not installed or not in PATH. Please install Rust first." >&2
      exit 1
    fi
    cargo install --path . --force
    echo ""
    echo "✓ agyo has been successfully installed to Cargo bin directory!"
    echo "Run 'agyo --help' to get started."
    echo ""
    return 0
  fi

  # 1. Detect OS & architecture
  local OS_NAME
  local ARCH_NAME
  OS_NAME="$(uname -s)"
  ARCH_NAME="$(uname -m)"

  local TARGET=""
  case "$OS_NAME" in
    Darwin)
      case "$ARCH_NAME" in
        arm64|aarch64)
          TARGET="aarch64-apple-darwin"
          ;;
        x86_64)
          TARGET="x86_64-apple-darwin"
          ;;
        *)
          echo "Error: Unsupported macOS architecture '$ARCH_NAME'." >&2
          exit 1
          ;;
      esac
      ;;
    Linux)
      case "$ARCH_NAME" in
        x86_64|amd64)
          TARGET="x86_64-unknown-linux-gnu"
          ;;
        aarch64|arm64)
          echo "[-] Note: Precompiled release binaries for Linux ARM64 are not currently published." >&2
          echo "[-] Please install agy-orbit from source using Cargo:" >&2
          echo "    cargo install agy-orbit" >&2
          exit 1
          ;;
        *)
          echo "Error: Unsupported Linux architecture '$ARCH_NAME'." >&2
          exit 1
          ;;
      esac
      ;;
    *)
      echo "Error: Unsupported operating system '$OS_NAME'. Use install.ps1 on Windows." >&2
      exit 1
      ;;
  esac

  local REPO_OWNER="gxwane"
  local REPO_NAME="agy-orbit"
  local ASSET_NAME="agyo-${TARGET}.tar.gz"
  local CHECKSUM_NAME="agyo-${TARGET}.sha256"
  local BASE_URL

  if [[ "$VERSION" == "latest" ]]; then
    BASE_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/latest/download"
  else
    local CLEAN_VER="$VERSION"
    [[ "$CLEAN_VER" != v* ]] && CLEAN_VER="v$CLEAN_VER"
    BASE_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${CLEAN_VER}"
  fi

  local ASSET_URL="${BASE_URL}/${ASSET_NAME}"
  local CHECKSUM_URL="${BASE_URL}/${CHECKSUM_NAME}"

  # 2. Prepare temporary directory
  local TMP_DIR
  TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'agyo-install')"
  trap 'rm -rf "$TMP_DIR"' EXIT

  echo "[1/4] Downloading release assets ($VERSION)..."
  echo "  Target triple: $TARGET"
  echo "  Asset URL: $ASSET_URL"

  local ARCHIVE_PATH="${TMP_DIR}/${ASSET_NAME}"
  local CHECKSUM_PATH="${TMP_DIR}/${CHECKSUM_NAME}"

  # Download checksum and archive
  if command -v curl >/dev/null 2>&1; then
    if ! curl -fsSL "$CHECKSUM_URL" -o "$CHECKSUM_PATH" 2>/dev/null; then
      if ! curl -fsSL "${BASE_URL}/${ASSET_NAME}.sha256" -o "$CHECKSUM_PATH"; then
        echo "❌ Download failed for checksum: $CHECKSUM_URL" >&2
        echo "Please verify that version '$VERSION' exists and asset is available." >&2
        exit 1
      fi
    fi
    if ! curl -fsSL "$ASSET_URL" -o "$ARCHIVE_PATH"; then
      echo "❌ Download failed for release asset: $ASSET_URL" >&2
      exit 1
    fi
  elif command -v wget >/dev/null 2>&1; then
    if ! wget -q "$CHECKSUM_URL" -O "$CHECKSUM_PATH" 2>/dev/null; then
      if ! wget -q "${BASE_URL}/${ASSET_NAME}.sha256" -O "$CHECKSUM_PATH"; then
        echo "❌ Download failed for checksum: $CHECKSUM_URL" >&2
        echo "Please verify that version '$VERSION' exists and asset is available." >&2
        exit 1
      fi
    fi
    if ! wget -q "$ASSET_URL" -O "$ARCHIVE_PATH"; then
      echo "❌ Download failed for release asset: $ASSET_URL" >&2
      exit 1
    fi
  else
    echo "Error: Neither curl nor wget was found. Please install curl or wget first." >&2
    exit 1
  fi

  # 3. Verify SHA-256
  echo ""
  echo "[2/4] Verifying SHA-256 checksum..."
  local EXPECTED_HASH
  EXPECTED_HASH="$(awk '{print $1}' "$CHECKSUM_PATH" | tr '[:upper:]' '[:lower:]')"

  local ACTUAL_HASH=""
  if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL_HASH="$(sha256sum "$ARCHIVE_PATH" | awk '{print $1}' | tr '[:upper:]' '[:lower:]')"
  elif command -v shasum >/dev/null 2>&1; then
    ACTUAL_HASH="$(shasum -a 256 "$ARCHIVE_PATH" | awk '{print $1}' | tr '[:upper:]' '[:lower:]')"
  else
    echo "Warning: No sha256sum or shasum found on host. Skipping checksum verification." >&2
  fi

  if [[ -n "$ACTUAL_HASH" ]]; then
    if [[ "$ACTUAL_HASH" != "$EXPECTED_HASH" ]]; then
      echo "❌ SHA-256 verification failed!" >&2
      echo "Expected: $EXPECTED_HASH" >&2
      echo "Actual:   $ACTUAL_HASH" >&2
      echo "The downloaded file may be corrupted or tampered with." >&2
      exit 1
    fi
    echo "  ✓ Checksum verified: $ACTUAL_HASH"
  fi

  # 4. Extract and install binary
  echo ""
  echo "[3/4] Installing executable..."
  local EXTRACT_DIR="${TMP_DIR}/extracted"
  mkdir -p "$EXTRACT_DIR"
  tar -xzf "$ARCHIVE_PATH" -C "$EXTRACT_DIR"

  local EXTRACTED_BIN="${EXTRACT_DIR}/agyo"
  if [[ ! -f "$EXTRACTED_BIN" ]]; then
    echo "Error: Archive did not contain 'agyo' binary." >&2
    exit 1
  fi

  local INSTALL_DIR
  if [[ "$(id -u)" -eq 0 ]]; then
    INSTALL_DIR="/usr/local/bin"
  else
    INSTALL_DIR="${HOME}/.local/bin"
  fi
  mkdir -p "$INSTALL_DIR"

  local TARGET_BIN="${INSTALL_DIR}/agyo"
  cp "$EXTRACTED_BIN" "$TARGET_BIN"
  chmod 0755 "$TARGET_BIN"
  echo "  ✓ Installed binary to: $TARGET_BIN"

  # 5. PATH check and user guidance
  echo ""
  echo "[4/4] Verifying environment PATH..."
  case ":$PATH:" in
    *":${INSTALL_DIR}:"*)
      echo "  ✓ '$INSTALL_DIR' is already in your PATH."
      ;;
    *)
      echo "  ⚠️  ATTENTION: '$INSTALL_DIR' is not in your current PATH!"
      echo "      To use 'agyo' from any terminal session, add the following line to your shell configuration:"
      echo "        export PATH=\"\$HOME/.local/bin:\$PATH\""
      echo "      (e.g., in ~/.bashrc or ~/.zshrc, then run 'source ~/.bashrc' or restart your terminal)."
      ;;
  esac

  local INSTALLED_VER
  INSTALLED_VER="$("$TARGET_BIN" --version 2>&1 || true)"
  echo ""
  echo "==============================================="
  echo "✓ Installation completed successfully!"
  echo "  Version:  $INSTALLED_VER"
  echo "  Location: $TARGET_BIN"
  echo "==============================================="
  echo "Run 'agyo --help' to get started."
  echo ""
}

main "$@"
