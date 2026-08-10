#!/usr/bin/env bash
# Usage: ./package.sh /path/to/cloned-site
#
# Copies a folder produced by `webcloner download` into dist/, then builds a
# native, fully offline desktop executable with Tauri. Run this on your own
# machine (Windows/macOS/Linux) with the Tauri prerequisites installed —
# see README.md in this folder for the one-time setup.

set -euo pipefail

if [ $# -lt 1 ]; then
  echo "Usage: $0 /path/to/cloned-site"
  exit 1
fi

SRC="$1"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIST="$HERE/dist"

if [ ! -d "$SRC" ]; then
  echo "Error: '$SRC' is not a directory (point this at the folder webcloner downloaded)"
  exit 1
fi

echo "→ Clearing $DIST"
rm -rf "$DIST"
mkdir -p "$DIST"

echo "→ Copying $SRC into $DIST"
cp -R "$SRC"/. "$DIST"/

if [ ! -f "$DIST/index.html" ]; then
  echo "Warning: no index.html found at the root of $SRC — the window may open blank."
fi

echo "→ Building the desktop app (this needs the Tauri CLI: 'cargo install tauri-cli --version ^1')"
cd "$HERE/src-tauri"
cargo tauri build

echo "✅ Done. Find your installer/executable under src-tauri/target/release/bundle/"
