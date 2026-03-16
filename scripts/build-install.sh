#!/usr/bin/env bash
set -euo pipefail

INSTALL_DIR="${1:-$HOME/bin}"
BIN_NAME="void"

echo "==> Building release binary…"
cargo build --release

SRC="target/release/$BIN_NAME"
DEST="$INSTALL_DIR/$BIN_NAME"

if [ ! -f "$SRC" ]; then
  echo "Error: release binary not found at $SRC" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"

# Use a temp file + atomic rename so zombie processes holding the old inode
# don't block new executions (macOS keeps the old inode alive for them).
TMP_DEST="$INSTALL_DIR/.$BIN_NAME.tmp.$$"
cp "$SRC" "$TMP_DEST"
chmod 755 "$TMP_DEST"

# macOS: strip quarantine / provenance attributes that block unsigned binaries
if [ "$(uname)" = "Darwin" ]; then
  xattr -cr "$TMP_DEST" 2>/dev/null || true
fi

mv -f "$TMP_DEST" "$DEST"

echo "==> Installed $BIN_NAME → $DEST"

# Check PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
  SHELL_RC="$HOME/.bashrc"
  if [ -n "${ZSH_VERSION:-}" ] || [[ "${SHELL:-}" == */zsh ]]; then
    SHELL_RC="$HOME/.zshrc"
  fi
  echo ""
  echo "Warning: $INSTALL_DIR is not on your PATH. Add it with:"
  echo "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> $SHELL_RC && source $SHELL_RC"
fi
