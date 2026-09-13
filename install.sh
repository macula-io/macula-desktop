#!/usr/bin/env bash
# Installs macula-desktop for Linux and macOS: downloads the release
# archive matching this machine's OS/arch from GitHub Releases, verifies
# it against the release's own checksums.txt, and installs the binary
# into a user-local directory (no sudo).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/macula-io/macula-desktop/main/install.sh | bash
#
# Env overrides:
#   MACULA_DESKTOP_VERSION      pin a version (e.g. "v0.1.0") instead of latest
#   MACULA_DESKTOP_INSTALL_DIR  install directory (default: $HOME/.local/bin)
#
# Uninstall with the matching script:
#   curl -fsSL https://raw.githubusercontent.com/macula-io/macula-desktop/main/uninstall.sh | bash
set -euo pipefail

REPO="macula-io/macula-desktop"
BIN="macula-desktop"
INSTALL_DIR="${MACULA_DESKTOP_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${MACULA_DESKTOP_VERSION:-latest}"

log() { printf '%s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not found on PATH"; }
need curl
need tar

detect_os() {
  case "$(uname -s)" in
    Linux)  echo "linux" ;;
    Darwin) echo "macos" ;;
    *) die "unsupported OS: $(uname -s) — this script covers Linux and macOS; see install.ps1 for Windows" ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo "x64" ;;
    aarch64|arm64) echo "arm64" ;;
    *) die "unsupported architecture: $(uname -m)" ;;
  esac
}

OS="$(detect_os)"
ARCH="$(detect_arch)"
ARCHIVE="${BIN}-${VERSION}-${OS}-${ARCH}.tar.gz"

api_url="https://api.github.com/repos/${REPO}/releases/${VERSION}"
if [ "$VERSION" = "latest" ]; then
  download_url="https://github.com/${REPO}/releases/latest/download/${ARCHIVE}"
  checksum_url="https://github.com/${REPO}/releases/latest/download/checksums.txt"
else
  download_url="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"
  checksum_url="https://github.com/${REPO}/releases/download/${VERSION}/checksums.txt"
fi
log "installing ${BIN} ${VERSION} for ${OS}/${ARCH} into ${INSTALL_DIR}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

log "downloading ${ARCHIVE}"
curl -fsSL --retry 3 "$download_url" -o "$tmp/${ARCHIVE}"

log "verifying checksum"
curl -fsSL --retry 3 "$checksum_url" -o "$tmp/checksums.txt"
expected="$(awk -v a="$ARCHIVE" '$2 == a { print $1 }' "$tmp/checksums.txt")"
[ -n "$expected" ] || die "no checksum entry for ${ARCHIVE} in checksums.txt"
case "$(uname -s)" in
  Darwin) actual="$(shasum -a 256 "$tmp/${ARCHIVE}" | awk '{ print $1 }')" ;;
  Linux)  actual="$(sha256sum "$tmp/${ARCHIVE}" | awk '{ print $1 }')" ;;
esac
[ "$actual" = "$expected" ] || die "checksum mismatch for ${ARCHIVE}: expected ${expected}, got ${actual}"

mkdir -p "$INSTALL_DIR"
tar xzf "$tmp/${ARCHIVE}" -C "$tmp"
install -m 0755 "$tmp/${BIN}" "$INSTALL_DIR/${BIN}"

log "installed ${INSTALL_DIR}/${BIN}"
command -v "$BIN" >/dev/null 2>&1 || log "note: ${INSTALL_DIR} is not on your PATH — add it or run ${INSTALL_DIR}/${BIN}"
