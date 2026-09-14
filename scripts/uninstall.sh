#!/usr/bin/env bash
set -euo pipefail

# agy-orbit Safe Uninstaller (POSIX Bash)
# Compliant with Linux & macOS environments

FORCE=0
KEEP_DATA=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    -f|--force)
      FORCE=1
      shift
      ;;
    --keep-data|--keep-vault)
      KEEP_DATA=1
      shift
      ;;
    *)
      echo "Unknown option: $1"
      echo "Usage: ./uninstall.sh [-f|--force] [--keep-data]"
      exit 1
      ;;
  esac
done

echo ""
echo "==============================================="
echo "       agy-orbit (agyo) Safe Uninstaller       "
echo "==============================================="
echo ""

# 1. Guarded safe directory removal function (anti-empty, anti-short, anti-root)
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
  if [[ "$base_name" != *"$expected_name"* ]]; then
    echo "[-] Security Guard: Target '$target_path' does not match expected pattern '$expected_name'. Skipped." >&2
    return 0
  fi

  if [[ -d "$target_path" ]]; then
    echo "[-] Removing directory: $target_path"
    rm -rf "$target_path"
  fi
}

# 2. Uninstall binary
echo "[1/4] Uninstalling binary..."
if command -v cargo >/dev/null 2>&1; then
  if cargo uninstall agy-orbit >/dev/null 2>&1; then
    echo "  ✓ Successfully uninstalled agy-orbit via cargo."
  else
    echo "  ✓ No cargo-managed agy-orbit found (or already removed)."
  fi
elif command -v agyo >/dev/null 2>&1; then
  AGYO_BIN="$(command -v agyo)"
  echo "  Notice: Standalone binary located at: $AGYO_BIN"
  echo "  Please remove it manually or via your system package manager."
fi

# 3. Clean ephemeral runtime locks
echo ""
echo "[2/4] Cleaning ephemeral runtime locks..."
CURRENT_UID="$(id -u)"

# Linux XDG_RUNTIME_DIR
if [[ -n "${XDG_RUNTIME_DIR:-}" ]]; then
  safe_remove_dir "$XDG_RUNTIME_DIR/agyo" "agyo"
fi

# macOS & Unix fallback: /tmp/agyo-run-${UID}
SYS_TMP="${TMPDIR:-/tmp}"
SYS_TMP="${SYS_TMP%/}" # Strip trailing slash
if [[ -n "$SYS_TMP" && -d "$SYS_TMP" ]]; then
  safe_remove_dir "$SYS_TMP/agyo-run-$CURRENT_UID" "agyo-run-$CURRENT_UID"
fi

# 4. Clean Orbit storage directory (~/.agyo)
echo ""
echo "[3/4] Checking Orbit storage data (~/.agyo)..."
TARGET_HOME="${HOME:?Error: HOME variable is unset or empty}"
AGYO_STORAGE_DIR="$TARGET_HOME/.agyo"

if [[ -d "$AGYO_STORAGE_DIR" ]]; then
  if [[ $KEEP_DATA -eq 1 ]]; then
    echo "  [i] Preserving Orbit storage at: $AGYO_STORAGE_DIR"
  else
    PROCEED_DELETE=$FORCE
    if [[ $FORCE -eq 0 ]]; then
      echo "  ⚠️  WARNING: Storage directory contains all encrypted multi-account credentials!"
      read -rp "  Do you want to permanently delete '$AGYO_STORAGE_DIR'? [y/N]: " CONFIRM_VAL
      if [[ "$CONFIRM_VAL" =~ ^[yY] ]]; then
        PROCEED_DELETE=1
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

# 5. Shell completion inspection & warning (Non-destructive: never silently mutate user profile)
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

# Clean fish completion file if present
FISH_COMP="$TARGET_HOME/.config/fish/completions/agyo.fish"
if [[ -f "$FISH_COMP" ]]; then
  echo "[-] Removing fish static completion: $FISH_COMP"
  rm -f "$FISH_COMP"
fi

echo ""
echo "==============================================="
echo "✓ agy-orbit uninstallation completed."
echo "Note: Official Google Antigravity credentials in ~/.gemini/ are kept intact."
echo "To log out from Antigravity entirely, run: 'agy auth logout'"
echo ""
