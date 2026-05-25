#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST_NAME="ai.daytrail.desktop"
APP_BIN="${APP_BIN:-/Applications/DayTrail.app/Contents/MacOS/daytrail}"
HOST_WRAPPER_DIR="${HOST_WRAPPER_DIR:-$HOME/.daytrail}"
HOST_WRAPPER="${HOST_BIN:-$HOST_WRAPPER_DIR/$HOST_NAME-native-host}"
CHROME_EXTENSION_ID="${CHROME_EXTENSION_ID:-__EXTENSION_ID__}"
EDGE_EXTENSION_ID="${EDGE_EXTENSION_ID:-$CHROME_EXTENSION_ID}"
BRAVE_EXTENSION_ID="${BRAVE_EXTENSION_ID:-$CHROME_EXTENSION_ID}"

if [[ "$CHROME_EXTENSION_ID" == "__EXTENSION_ID__" || -z "$CHROME_EXTENSION_ID" ]]; then
  echo "Set CHROME_EXTENSION_ID to the installed Chrome extension id before installing the native host." >&2
  exit 1
fi

if [[ "$EDGE_EXTENSION_ID" == "__EXTENSION_ID__" || -z "$EDGE_EXTENSION_ID" ]]; then
  echo "Set EDGE_EXTENSION_ID to the installed Edge extension id before installing the native host." >&2
  exit 1
fi

if [[ "$BRAVE_EXTENSION_ID" == "__EXTENSION_ID__" || -z "$BRAVE_EXTENSION_ID" ]]; then
  echo "Set BRAVE_EXTENSION_ID to the installed Brave extension id before installing the native host." >&2
  exit 1
fi

case "$(uname -s)" in
  Darwin)
    CHROME_HOST_DIR="$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts"
    BRAVE_HOST_DIR="$HOME/Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts"
    EDGE_HOST_DIR="$HOME/Library/Application Support/Microsoft Edge/NativeMessagingHosts"
    FIREFOX_HOST_DIR="$HOME/Library/Application Support/Mozilla/NativeMessagingHosts"
    ;;
  Linux)
    CHROME_HOST_DIR="$HOME/.config/google-chrome/NativeMessagingHosts"
    BRAVE_HOST_DIR="$HOME/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts"
    EDGE_HOST_DIR="$HOME/.config/microsoft-edge/NativeMessagingHosts"
    FIREFOX_HOST_DIR="$HOME/.mozilla/native-messaging-hosts"
    ;;
  *)
    echo "Unsupported OS for native host installation." >&2
    exit 1
    ;;
esac

mkdir -p "$CHROME_HOST_DIR" "$BRAVE_HOST_DIR" "$EDGE_HOST_DIR" "$FIREFOX_HOST_DIR"
mkdir -p "$(dirname "$HOST_WRAPPER")"

cat > "$HOST_WRAPPER" <<EOF
#!/usr/bin/env bash
exec "$APP_BIN" --native-messaging-host "\$@"
EOF
chmod 700 "$HOST_WRAPPER"

node "$ROOT_DIR/scripts/write-native-host-manifest.mjs" \
  chrome "$HOST_WRAPPER" "$CHROME_EXTENSION_ID" \
  "$CHROME_HOST_DIR/$HOST_NAME.json"

node "$ROOT_DIR/scripts/write-native-host-manifest.mjs" \
  brave "$HOST_WRAPPER" "$BRAVE_EXTENSION_ID" \
  "$BRAVE_HOST_DIR/$HOST_NAME.json"

node "$ROOT_DIR/scripts/write-native-host-manifest.mjs" \
  edge "$HOST_WRAPPER" "$EDGE_EXTENSION_ID" \
  "$EDGE_HOST_DIR/$HOST_NAME.json"

node "$ROOT_DIR/scripts/write-native-host-manifest.mjs" \
  firefox "$HOST_WRAPPER" "$CHROME_EXTENSION_ID" \
  "$FIREFOX_HOST_DIR/$HOST_NAME.json"

echo "$HOST_WRAPPER"
echo "$CHROME_HOST_DIR/$HOST_NAME.json"
echo "$BRAVE_HOST_DIR/$HOST_NAME.json"
echo "$EDGE_HOST_DIR/$HOST_NAME.json"
echo "$FIREFOX_HOST_DIR/$HOST_NAME.json"
