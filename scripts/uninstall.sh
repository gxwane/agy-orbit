#!/usr/bin/env bash
# agy-orbit Safe Uninstaller (POSIX Bash)
# Compliant with Linux & macOS environments, safe path guards, and pipeline stdin protection
set -euo pipefail

main() {
  local FORCE=0
  local KEEP_DATA=0

  while [[ $# -gt 0 ]]; do
    case "$1" in
      -f|--force|-y|--yes)
        FORCE=1
        shift
        ;;
      --keep-data|--keep-vault)
        KEEP_DATA=1
        shift
        ;;
      -h|--help)
        echo "agy-orbit Uninstaller"
        echo "Usage: uninstall.sh [-f|--force] [--keep-vault]"
        exit 0
        ;;
      *)
        echo "Unknown option: $1" >&2
        echo "Usage: ./uninstall.sh [-f|--force] [--keep-vault]" >&2
        exit 1
        ;;
    esac
  done

  echo ""
  echo "==============================================="
  echo "       agy-orbit (agyo) Safe Uninstaller       "
  echo "==============================================="
  echo ""

  # Helper for safe directory removal
  safe_remove_dir() {
    local target_path="${1:-}"
    local expected_name="${2:-}"

    if [[ -z "$target_path" || -z "$expected_name" ]]; then
      return 0
    fi

    # Must be an absolute path
    if [[ "$target_path" != /* ]]; then
      echo "[-] Security Guard: Non-absolute path ignored: $target_path" >&2
      return 0
    fi

    # Strictly forbidden system root directories
    case "$target_path" in
      /|/root|/home|/home/|/Users|/Users/|/tmp|/tmp/|/var|/usr|/etc)
        echo "[-] Security Guard: Target '$target_path' is a protected system directory. Skipped." >&2
        return 0
        ;;
    esac

    # Basename must match expected pattern
    local base_name
    base_name="$(basename "$target_path")"
    if [[ "$base_name" != "$expected_name" ]]; then
      echo "[-] Security Guard: Directory name mismatch ('$base_name' != '$expected_name'). Skipped." >&2
      return 0
    fi

    # Check for symlink: if symlink, remove the link itself, do not recurse into target
    if [[ -L "$target_path" ]]; then
      echo "[-] Removing symlink: $target_path"
      rm -f "$target_path"
      return 0
    fi

    if [[ -d "$target_path" ]]; then
      echo "[-] Removing directory: $target_path"
      rm -rf "$target_path"
    fi
  }

  local TARGET_HOME="${HOME:-}"
  if [[ -z "$TARGET_HOME" ]]; then
    echo "Error: HOME environment variable is not set." >&2
    exit 1
  fi

  # 1. Remove binary files
  echo "[1/4] Removing executable binaries..."
  local LOCAL_BIN="${TARGET_HOME}/.local/bin/agyo"
  if [[ -f "$LOCAL_BIN" || -L "$LOCAL_BIN" ]]; then
    rm -f "$LOCAL_BIN"
    echo "  ✓ Removed binary: $LOCAL_BIN"
  fi

  if [[ "$(id -u)" -eq 0 && (-f "/usr/local/bin/agyo" || -L "/usr/local/bin/agyo") ]]; then
    rm -f "/usr/local/bin/agyo"
    echo "  ✓ Removed system binary: /usr/local/bin/agyo"
  fi

  # Check if installed via Cargo (supports CARGO_INSTALL_ROOT, CARGO_HOME, and default ~/.cargo/bin)
  local CARGO_BIN_DIR="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$TARGET_HOME/.cargo}}/bin"
  local CARGO_AGYO="${CARGO_BIN_DIR}/agyo"
  if [[ (-f "$CARGO_AGYO" || -L "$CARGO_AGYO") && $(command -v cargo 2>/dev/null) ]]; then
    if cargo uninstall agy-orbit >/dev/null 2>&1; then
      echo "  ✓ Successfully uninstalled agy-orbit via Cargo."
    elif [[ -f "$CARGO_AGYO" || -L "$CARGO_AGYO" ]]; then
      # Fallback: remove orphaned binary if not tracked by Cargo metadata
      rm -f "$CARGO_AGYO"
      echo "  ✓ Removed orphaned executable from Cargo bin: $CARGO_AGYO"
    fi
  fi

  # 2. Clean runtime lock directories
  echo ""
  echo "[2/4] Cleaning ephemeral runtime locks..."
  local CURRENT_UID
  CURRENT_UID="$(id -u)"
  safe_remove_dir "/run/user/${CURRENT_UID}/agyo" "agyo"
  safe_remove_dir "/tmp/agyo-run-${CURRENT_UID}" "agyo-run-${CURRENT_UID}"

  # 3. Clean Orbit storage (~/.agyo)
  echo ""
  echo "[3/4] Checking Orbit storage data (~/.agyo)..."
  local AGYO_STORAGE_DIR="${TARGET_HOME}/.agyo"

  if [[ -d "$AGYO_STORAGE_DIR" || -L "$AGYO_STORAGE_DIR" ]]; then
    if [[ $KEEP_DATA -eq 1 ]]; then
      echo "  [i] Preserving Orbit storage at: $AGYO_STORAGE_DIR"
    else
      local PROCEED_DELETE=$FORCE
      if [[ $FORCE -eq 0 ]]; then
        # Check if stdin is a terminal or if /dev/tty is accessible
        if [ -e /dev/tty ]; then
          echo "  ⚠️  WARNING: Storage directory contains all encrypted multi-account credentials!"
          read -rp "  Do you want to permanently delete '$AGYO_STORAGE_DIR'? [y/N]: " CONFIRM_VAL < /dev/tty
          if [[ "$CONFIRM_VAL" =~ ^[yY] ]]; then
            PROCEED_DELETE=1
          fi
        else
          echo "Error: Running in non-interactive pipeline without -f/--force." >&2
          echo "To confirm deletion of storage data in automated scripts, pass -f or --force." >&2
          exit 1
        fi
      fi

      if [[ $PROCEED_DELETE -eq 1 ]]; then
        safe_remove_dir "$AGYO_STORAGE_DIR" ".agyo"
        echo "  ✓ Orbit storage permanently removed."
      else
        echo "  [i] Preserved storage at $AGYO_STORAGE_DIR."
      fi
    fi
  else
    echo "  ✓ No storage directory found at ~/.agyo"
  fi

  # 4. Shell completion inspection
  echo ""
  echo "[4/4] Inspecting Shell RC files..."
  for rc_file in "$TARGET_HOME/.bashrc" "$TARGET_HOME/.zshrc" "$TARGET_HOME/.config/fish/config.fish"; do
    if [[ -f "$rc_file" ]]; then
      if grep -q "agyo completion" "$rc_file" 2>/dev/null; then
        echo "  ⚠️  ATTENTION: Found 'agyo completion' directive in $rc_file"
        echo "      Please remove this line manually to prevent 'command not found' errors on shell startup."
      fi
    fi
  done

  local FISH_COMP="$TARGET_HOME/.config/fish/completions/agyo.fish"
  if [[ -f "$FISH_COMP" ]]; then
    rm -f "$FISH_COMP"
    echo "  ✓ Removed fish static completion: $FISH_COMP"
  fi

  echo ""
  echo "==============================================="
  echo "✓ agy-orbit uninstallation completed."
  echo "Note: Official Google Antigravity credentials in ~/.gemini/ are kept intact."
  echo "To log out from Antigravity entirely, run: 'agy auth logout'"
  echo ""
}

main "$@"
