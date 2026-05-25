#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$ROOT_DIR"

npm --prefix apps/desktop run check
npm --prefix apps/desktop run test
npm --prefix apps/desktop run build
npm run browser-extension:check
npm run vscode-extension:check
npm run test:scripts
cargo test --workspace --all-targets
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets
