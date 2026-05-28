param(
  [switch]$SkipChecks
)

$ErrorActionPreference = "Stop"

$RootDir = Resolve-Path (Join-Path $PSScriptRoot "..")
$DesktopDir = Join-Path $RootDir "apps\desktop"

Push-Location $RootDir
try {
  npm --prefix apps/desktop ci

  if (-not $SkipChecks) {
    npm --prefix apps/desktop run check
    npm --prefix apps/desktop run test
    npm run browser-extension:check
    npm run vscode-extension:check
    npm run test:scripts
    cargo test --workspace --all-targets
    cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets
  }

  npm --prefix apps/desktop run build

  Push-Location $DesktopDir
  try {
    npm run tauri -- build --bundles nsis,msi
  } finally {
    Pop-Location
  }

  $BundleDir = Join-Path $DesktopDir "src-tauri\target\release\bundle"
  Get-ChildItem -Path $BundleDir -Recurse -Include *.exe,*.msi |
    Where-Object { $_.FullName -match "\\(nsis|msi)\\" } |
    Select-Object -ExpandProperty FullName
} finally {
  Pop-Location
}
