#!/bin/bash
# DayTrail macOS installer — downloads the latest DMG, installs to /Applications,
# and strips the Gatekeeper quarantine flag so the app opens without any warning.
set -euo pipefail

REPO="varaprasadreddy9676/DayTrail"
APPDIR="/Applications"

echo "Fetching latest DayTrail release..."
TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' | head -1 | cut -d'"' -f4)

if [ -z "$TAG" ]; then
  echo "Error: could not determine latest release tag." >&2
  exit 1
fi

VERSION="${TAG#v}"
DMG_NAME="DayTrail_${VERSION}_aarch64.dmg"
DMG_URL="https://github.com/${REPO}/releases/download/${TAG}/${DMG_NAME}"

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading DayTrail ${TAG}..."
curl -fL "$DMG_URL" -o "$TMPDIR/$DMG_NAME"

MOUNT="$TMPDIR/mnt"
mkdir -p "$MOUNT"
hdiutil attach "$TMPDIR/$DMG_NAME" -mountpoint "$MOUNT" -quiet -nobrowse

echo "Installing to ${APPDIR}..."
# Remove previous install if present
[ -d "$APPDIR/DayTrail.app" ] && rm -rf "$APPDIR/DayTrail.app"
cp -R "$MOUNT/DayTrail.app" "$APPDIR/"

hdiutil detach "$MOUNT" -quiet

# Strip Gatekeeper quarantine so the app opens without "damaged" warning
xattr -dr com.apple.quarantine "$APPDIR/DayTrail.app" 2>/dev/null || true

echo ""
echo "DayTrail ${TAG} installed to ${APPDIR}/DayTrail.app"
echo "Launching..."
open "$APPDIR/DayTrail.app"
