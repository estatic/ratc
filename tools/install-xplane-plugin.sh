#!/usr/bin/env bash
# Build the on-screen overlay plugin and install it into X-Plane 11 as
# Resources/plugins/atc-overlay/64/lin.xpl. Usage: tools/install-xplane-plugin.sh [X-Plane dir]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
XP="${1:-$HOME/.local/share/Steam/steamapps/common/X-Plane 11}"
if [ ! -d "$XP/Resources/plugins" ]; then
  echo "no X-Plane 11 at $XP (pass its directory as the argument)" >&2
  exit 1
fi
cargo build --release -p atc-xplane-plugin --manifest-path "$ROOT/Cargo.toml"
DEST="$XP/Resources/plugins/atc-overlay/64"
mkdir -p "$DEST"
cp "$ROOT/target/release/libatc_overlay.so" "$DEST/lin.xpl"
echo "installed: $DEST/lin.xpl (restart X-Plane to load it)"
