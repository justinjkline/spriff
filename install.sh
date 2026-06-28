#!/usr/bin/env bash
# Build spriff in release mode and put it on your PATH.
#
# spriff must be callable from any repo your agents work in, so we install a
# single static binary to a directory that's typically already on PATH.
set -euo pipefail

cd "$(dirname "$0")"

echo "==> Building spriff (release)…"
cargo build --release

BIN="target/release/spriff"
[ -x "$BIN" ] || { echo "build did not produce $BIN" >&2; exit 1; }

# Prefer ~/.cargo/bin (almost always on PATH for Rust users); fall back to
# ~/.local/bin. Override with: DEST=/somewhere/on/PATH ./install.sh
DEST="${DEST:-$HOME/.cargo/bin}"
[ -d "$DEST" ] || DEST="$HOME/.local/bin"
mkdir -p "$DEST"

install -m 0755 "$BIN" "$DEST/spriff"
echo "==> Installed: $DEST/spriff"

if ! command -v spriff >/dev/null 2>&1; then
  echo
  echo "NOTE: $DEST is not on your PATH. Add this to your shell profile:"
  echo "    export PATH=\"$DEST:\$PATH\""
fi

echo
echo "Done. Try:"
echo "    spriff --version"
echo "    spriff init demo --agents 2"
echo "    spriff skill"
