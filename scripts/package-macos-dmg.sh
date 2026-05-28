#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="${APP_NAME:-DayTrail}"
APP_BUNDLE="${APP_BUNDLE:-$ROOT_DIR/apps/desktop/src-tauri/target/release/bundle/macos/$APP_NAME.app}"
OUT_DIR="${OUT_DIR:-$ROOT_DIR/apps/desktop/src-tauri/target/release/bundle/dmg}"
DMG_PATH="${DMG_PATH:-$OUT_DIR/$APP_NAME.dmg}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "DMG packaging requires macOS." >&2
  exit 1
fi

if [[ ! -d "$APP_BUNDLE" ]]; then
  echo "App bundle not found: $APP_BUNDLE" >&2
  echo "Build the Tauri app first, or set APP_BUNDLE=/path/to/App.app." >&2
  exit 1
fi

command -v hdiutil >/dev/null 2>&1 || {
  echo "hdiutil is required to create a DMG." >&2
  exit 1
}

mkdir -p "$OUT_DIR"
STAGING_DIR="$(mktemp -d "${TMPDIR:-/tmp}/daytrail-dmg.XXXXXX")"
trap 'rm -rf "$STAGING_DIR"' EXIT

cp -R "$APP_BUNDLE" "$STAGING_DIR/"
ln -s /Applications "$STAGING_DIR/Applications"

hdiutil create \
  -volname "$APP_NAME" \
  -srcfolder "$STAGING_DIR" \
  -ov \
  -format UDZO \
  "$DMG_PATH"

echo "$DMG_PATH"
