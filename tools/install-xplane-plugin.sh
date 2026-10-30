#!/usr/bin/env bash
# Build the on-screen overlay plugin and install it into X-Plane 11 as
# Resources/plugins/atc-overlay/64/lin.xpl, with the follow-me car's roof object (sign,
# beacon, turn signals) in Resources/plugins/atc-overlay/objects/.
# Usage: tools/install-xplane-plugin.sh [X-Plane dir]
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
OBJECTS="$XP/Resources/plugins/atc-overlay/objects"
mkdir -p "$OBJECTS"
cp "$ROOT/crates/atc-xplane-plugin/assets/follow_me_roof.obj" \
  "$ROOT/crates/atc-xplane-plugin/assets/follow_me_roof.png" "$OBJECTS/"
echo "installed: $DEST/lin.xpl and $OBJECTS/follow_me_roof.{obj,png} (restart X-Plane to load them)"
