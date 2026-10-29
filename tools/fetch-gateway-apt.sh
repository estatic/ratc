#!/usr/bin/env bash
# Fetch the recommended X-Plane Scenery Gateway apt.dat for an airport (default EPKK).
set -euo pipefail
ICAO="${1:-EPKK}"
OUT="$(cd "$(dirname "$0")/.." && pwd)/data/airports/$ICAO"
API="https://gateway.x-plane.com/apiv1"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$OUT"

ID="$(curl -fsS "$API/airport/$ICAO" | jq -r '.airport.recommendedSceneryId')"
[ -n "$ID" ] && [ "$ID" != null ] || { echo "no recommendedSceneryId for $ICAO" >&2; exit 1; }
curl -fsS "$API/scenery/$ID" | jq -r '.scenery.masterZipBlob' | base64 -d > "$TMP/scenery.zip"
unzip -o -j "$TMP/scenery.zip" "$ICAO.dat" -d "$TMP" >/dev/null
cp "$TMP/$ICAO.dat" "$OUT/apt.dat"
echo "scenery id: $ID"
echo "wrote $OUT/apt.dat"
