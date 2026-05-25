#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS packaging requires macOS." >&2
  exit 1
fi

if [[ -d "$ROOT_DIR/apps/desktop" && -f "$ROOT_DIR/apps/desktop/package.json" ]]; then
  npm --prefix "$ROOT_DIR/apps/desktop" run build
fi

if cargo tauri --version >/dev/null 2>&1; then
  (cd "$ROOT_DIR/apps/desktop" && cargo tauri build --bundles app)
elif command -v cargo-tauri >/dev/null 2>&1; then
  (cd "$ROOT_DIR/apps/desktop" && cargo-tauri build --bundles app)
else
  echo "Install Tauri CLI first: cargo install tauri-cli --version '^2'" >&2
  exit 1
fi

"$ROOT_DIR/scripts/package-macos-dmg.sh"
