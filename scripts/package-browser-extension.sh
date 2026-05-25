#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXT_DIR="$ROOT_DIR/apps/browser-extension"
OUT_DIR="${OUT_DIR:-$ROOT_DIR/dist/browser-extension}"
OUT_FILE="${OUT_FILE:-$OUT_DIR/daytrail-browser-bridge.zip}"

command -v zip >/dev/null 2>&1 || {
  echo "zip is required to package the browser extension." >&2
  exit 1
}

mkdir -p "$OUT_DIR"
rm -f "$OUT_FILE"

(cd "$EXT_DIR" && zip -qr "$OUT_FILE" manifest.json src native-host)

echo "$OUT_FILE"
