#!/usr/bin/env bash
# Uninstalls macula-desktop for Linux and macOS: removes the binary from
# the directory install.sh put it in (or a custom one).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/macula-io/macula-desktop/main/uninstall.sh | bash
#
# Env overrides:
#   MACULA_DESKTOP_INSTALL_DIR  the directory install.sh was pointed at
#                               (default: $HOME/.local/bin)
set -euo pipefail

BIN="macula-desktop"
INSTALL_DIR="${MACULA_DESKTOP_INSTALL_DIR:-$HOME/.local/bin}"

if [ ! -f "$INSTALL_DIR/$BIN" ]; then
  printf 'macula-desktop is not installed at %s/%s — nothing to remove.\n' "$INSTALL_DIR" "$BIN"
  exit 0
fi

rm -f "$INSTALL_DIR/$BIN"
printf 'removed %s/%s\n' "$INSTALL_DIR" "$BIN"
