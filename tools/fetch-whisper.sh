#!/usr/bin/env bash
# Fetch Whisper ggml models into models/whisper/. Idempotent.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/models/whisper"
HF="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"
MODELS=(
  ggml-large-v3-turbo-q5_0.bin   # primary, multilingual (~574 MB)
  ggml-small.en-q5_1.bin         # fallback, English only (~181 MB)
)
mkdir -p "$OUT"
for m in "${MODELS[@]}"; do
  if [ -s "$OUT/$m" ]; then
    echo "$m present, skipping"
  else
    echo "downloading $m"
    curl -fsSL --retry 3 -o "$OUT/$m.part" "$HF/$m"
    mv "$OUT/$m.part" "$OUT/$m"
  fi
done
echo "done: $OUT"
