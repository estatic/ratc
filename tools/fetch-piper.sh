#!/usr/bin/env bash
# Fetch the Piper TTS binary and the English and Polish medium voices into models/piper/.
# Idempotent.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/models/piper"
VOICES="$OUT/voices"
REL="https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_linux_x86_64.tar.gz"
HF="https://huggingface.co/rhasspy/piper-voices/resolve/main"
# lang/locale/speaker/quality — see tools/MODELS.md
VOICE_LIST=(
  "en/en_US/ryan/medium/en_US-ryan-medium"
  "en/en_GB/alan/medium/en_GB-alan-medium"
  "en/en_US/amy/medium/en_US-amy-medium"
  "en/en_US/lessac/medium/en_US-lessac-medium"
  "en/en_GB/northern_english_male/medium/en_GB-northern_english_male-medium"
  "en/en_US/joe/medium/en_US-joe-medium"
  "pl/pl_PL/darkman/medium/pl_PL-darkman-medium"
  "pl/pl_PL/mc_speech/medium/pl_PL-mc_speech-medium"
  "pl/pl_PL/gosia/medium/pl_PL-gosia-medium"
)
mkdir -p "$OUT" "$VOICES"

if [ ! -x "$OUT/piper/piper" ]; then
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  echo "downloading piper binary"
  curl -fsSL --retry 3 -o "$TMP/piper.tar.gz" "$REL"
  tar -xzf "$TMP/piper.tar.gz" -C "$OUT"
else
  echo "piper binary present, skipping"
fi

for v in "${VOICE_LIST[@]}"; do
  name="$(basename "$v")"
  for ext in onnx onnx.json; do
    if [ -s "$VOICES/$name.$ext" ]; then
      echo "$name.$ext present, skipping"
    else
      echo "downloading $name.$ext"
      curl -fsSL --retry 3 -o "$VOICES/$name.$ext.part" "$HF/$v.$ext"
      mv "$VOICES/$name.$ext.part" "$VOICES/$name.$ext"
    fi
  done
done
echo "done: $OUT"
