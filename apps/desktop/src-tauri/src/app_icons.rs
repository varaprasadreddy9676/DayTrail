/// App icon extraction for macOS.
///
/// Uses `NSWorkspace.icon(forFile:)` via an inline `osascript -l JavaScript`
/// call, which gives the exact same icons that macOS and Trace display. Results
/// are cached in memory so each icon is only extracted once per session.
use std::{
    collections::HashMap,
    path::PathBuf,
    process::Command,
    sync::{Mutex, OnceLock},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

// ── In-memory cache: app_name → data URL (or empty = failed, don't retry) ──

static ICON_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn icon_cache() -> &'static Mutex<HashMap<String, String>> {
    ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Canonical `.app` bundle folder names for apps whose display name differs
/// from their bundle file name.
fn canonical_bundle_folder(app_name: &str) -> &str {
    match app_name {
        "Visual Studio Code" | "VS Code" | "Code" => "Visual Studio Code",
        "VS Code Insiders" | "Code - Insiders" => "Visual Studio Code - Insiders",
        "Cursor" => "Cursor",
        "Google Chrome" => "Google Chrome",
        "Brave Browser" => "Brave Browser",
        "Microsoft Edge" => "Microsoft Edge",
        "zoom.us" | "Zoom" => "zoom.us",
        "iTerm2" => "iTerm2",
        "ChatGPT" | "ChatGPT Atlas" => "ChatGPT",
        "Claude" => "Claude",
        "Codex" => "Codex",
        "Slack" => "Slack",
        "WhatsApp" => "WhatsApp",
        "Windows App" => "Windows App",
        other => other,
    }
}

/// Search common macOS install locations for `<name>.app`.
#[cfg(target_os = "macos")]
fn find_app_bundle(app_name: &str) -> Option<PathBuf> {
    let folder = canonical_bundle_folder(app_name);
    let filename = format!("{}.app", folder);

    let mut search_dirs: Vec<PathBuf> = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
    ];
    if let Some(home) = dirs::home_dir() {
        search_dirs.push(home.join("Applications"));
    }

    for dir in &search_dirs {
        let candidate = dir.join(&filename);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // Spotlight fallback for non-standard install paths.
    let output = Command::new("/usr/bin/mdfind")
        .args([&format!(
            "kMDItemContentType == 'com.apple.application-bundle' && kMDItemFSName == '{}'",
            filename
        )])
        .output()
        .ok()?;

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(PathBuf::from)
}

/// Extract a 64×64 PNG icon for the given app bundle using the macOS
/// `NSWorkspace.icon(forFile:)` API via an inline JXA (JavaScript for
/// Automation) script. This is the same API Finder and other apps use.
#[cfg(target_os = "macos")]
fn extract_icon_via_jxa(bundle_path: &str) -> Option<String> {
    // Escape any double quotes in the path (shouldn't normally occur but be safe).
    let safe_path = bundle_path.replace('"', "\\\"");

    // Write to a temp PNG file to avoid large stdout payloads.
    let safe_name: String = bundle_path
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    let out_png = std::env::temp_dir().join(format!(
        "daytrail_icon_v3_{}.png",
        &safe_name[safe_name.len().saturating_sub(40)..safe_name.len()]
    ));

    // If a cached PNG already exists (and is non-empty), skip extraction.
    if out_png.exists() && out_png.metadata().map(|m| m.len() > 100).unwrap_or(false) {
        let bytes = std::fs::read(&out_png).ok()?;
        let encoded = BASE64.encode(&bytes);
        return Some(format!("data:image/png;base64,{}", encoded));
    }

    // Prefer the bundle's declared `.icns` file. It produces the real app
    // icon reliably for Electron, SwiftUI, and native apps. JXA is kept as a
    // fallback for unusual bundles that do not expose an icon resource.
    if let Some(icon) = extract_icon_via_sips(bundle_path, &out_png) {
        return Some(icon);
    }

    let jxa = format!(
        r#"ObjC.import('AppKit');ObjC.import('Foundation');
var icon=$.NSWorkspace.sharedWorkspace.iconForFile($("{p}"));
icon.setSize($.NSMakeSize(64,64));
var rep=$.NSBitmapImageRep.imageRepWithData(icon.TIFFRepresentation);
var png=rep.representationUsingTypeProperties($.NSBitmapImageFileTypePNG,$.NSDictionary.dictionary);
var b64=ObjC.unwrap(png.base64EncodedStringWithOptions(0));
b64"#,
        p = safe_path
    );

    let output = Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", &jxa])
        .output()
        .ok()?;

    if !output.status.success() {
        // JXA failed — fall back to sips pipeline.
        return extract_icon_via_sips(bundle_path, &out_png);
    }

    let b64 = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if b64.is_empty() || b64.starts_with("error") {
        return extract_icon_via_sips(bundle_path, &out_png);
    }

    // Decode and re-encode to validate, then cache to disk.
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .ok()?;
    if bytes.len() < 100 {
        return None;
    }
    let _ = std::fs::write(&out_png, &bytes);
    Some(format!("data:image/png;base64,{}", b64))
}

/// Fallback: use `sips` to convert the bundle's `.icns` to a 64×64 PNG.
#[cfg(target_os = "macos")]
fn extract_icon_via_sips(bundle_path: &str, out_png: &std::path::Path) -> Option<String> {
    // Read CFBundleIconFile from Info.plist. `defaults read` is unreliable for
    // some modern bundles; PlistBuddy reads the file directly.
    let plist = format!("{}/Contents/Info.plist", bundle_path);
    let defaults_out = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleIconFile", &plist])
        .output()
        .ok()?;

    let mut icon_name = String::from_utf8_lossy(&defaults_out.stdout)
        .trim()
        .to_string();
    if icon_name.is_empty() {
        // CFBundleIconFile not set — try common names.
        let resources = format!("{}/Contents/Resources", bundle_path);
        for name in &[
            "AppIcon.icns",
            "Code.icns",
            "electron.icns",
            "icon.icns",
            "application.icns",
        ] {
            let candidate = format!("{}/{}", resources, name);
            if std::path::Path::new(&candidate).exists() {
                icon_name = (*name).to_string();
                break;
            }
        }
    }
    if icon_name.is_empty() {
        return None;
    }
    if !icon_name.ends_with(".icns") {
        icon_name.push_str(".icns");
    }

    let icns_path = format!("{}/Contents/Resources/{}", bundle_path, icon_name);
    if !std::path::Path::new(&icns_path).exists() {
        return None;
    }

    let ok = Command::new("/usr/bin/sips")
        .args([
            "-s",
            "format",
            "png",
            &icns_path,
            "--out",
            &out_png.to_string_lossy(),
            "--resampleHeightWidth",
            "64",
            "64",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !ok {
        return None;
    }

    let bytes = std::fs::read(out_png).ok()?;
    if bytes.len() < 100 {
        return None;
    }
    let encoded = BASE64.encode(&bytes);
    Some(format!("data:image/png;base64,{}", encoded))
}

/// Public entry point. Returns a `data:image/png;base64,...` URL suitable for
/// use directly in an `<img src>` attribute, or `None` if extraction failed.
#[cfg(target_os = "macos")]
pub fn app_icon_data_url(app_name: &str) -> Option<String> {
    // Check in-memory cache first.
    {
        let cache = icon_cache().lock().ok()?;
        if let Some(cached) = cache.get(app_name) {
            return if cached.is_empty() {
                None
            } else {
                Some(cached.clone())
            };
        }
    }

    let bundle = find_app_bundle(app_name)?;
    let result = extract_icon_via_jxa(&bundle.to_string_lossy());

    // Cache result (empty = failed, don't retry this session).
    if let Ok(mut cache) = icon_cache().lock() {
        cache.insert(app_name.to_string(), result.clone().unwrap_or_default());
    }

    result
}

// ── Windows icon extraction ─────────────────────────────────────────────────
//
// Strategy: locate the app's `.exe` (running process by name → common install
// dirs fallback), then ask PowerShell to extract the associated icon via the
// .NET `System.Drawing.Icon.ExtractAssociatedIcon` API and emit a base64 PNG.
// No new crate dependencies — uses the PowerShell that ships with Windows.

/// Sanitize an app name into a safe filename stem for disk caching.
#[cfg(target_os = "windows")]
fn sanitize_for_filename(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

#[cfg(target_os = "windows")]
fn extract_icon_via_powershell(app_name: &str) -> Option<String> {
    // Reuse a cached PNG on disk if we already extracted this app's icon before.
    let safe = sanitize_for_filename(app_name);
    if safe.is_empty() {
        return None;
    }
    let out_png = std::env::temp_dir().join(format!("daytrail_icon_v1_{}.png", safe));

    if out_png.exists() && out_png.metadata().map(|m| m.len() > 100).unwrap_or(false) {
        let bytes = std::fs::read(&out_png).ok()?;
        let encoded = BASE64.encode(&bytes);
        return Some(format!("data:image/png;base64,{}", encoded));
    }

    // PowerShell single-shot script:
    //   1. Try to find the exe path of a running process matching the app name.
    //   2. Fall back to walking common install locations for `<name>.exe`.
    //   3. Extract associated icon, resample to 64x64, write PNG, emit base64.
    //
    // All input is parameterized via -ArgumentList so the app name cannot be
    // interpreted as PowerShell syntax.
    let script = r#"
param([string]$AppName, [string]$OutPng)
$ErrorActionPreference = 'Stop'
try {
  Add-Type -AssemblyName System.Drawing | Out-Null

  function Get-Candidate {
    param([string]$Name)

    # 1. Running process whose ProcessName (without .exe) equals the app name.
    $p = Get-Process -Name $Name -ErrorAction SilentlyContinue |
         Where-Object { $_.Path } | Select-Object -First 1
    if ($p) { return $p.Path }

    # 2. Common install directories.
    $roots = @()
    if ($env:ProgramFiles)         { $roots += $env:ProgramFiles }
    if (${env:ProgramFiles(x86)})  { $roots += ${env:ProgramFiles(x86)} }
    if ($env:LOCALAPPDATA)         { $roots += (Join-Path $env:LOCALAPPDATA 'Programs') }
    if ($env:APPDATA)              { $roots += $env:APPDATA }
    $exeName = "$Name.exe"

    foreach ($root in $roots) {
      if (-not (Test-Path -LiteralPath $root)) { continue }
      try {
        $found = Get-ChildItem -LiteralPath $root -Filter $exeName -Recurse -ErrorAction SilentlyContinue -Depth 4 |
                 Select-Object -First 1
        if ($found) { return $found.FullName }
      } catch {}
    }
    return $null
  }

  $exePath = Get-Candidate -Name $AppName
  if (-not $exePath) {
    # Special-case Windows Explorer.
    if ($AppName -ieq 'explorer') { $exePath = "$env:WINDIR\explorer.exe" }
  }
  if (-not $exePath) { exit 2 }

  $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($exePath)
  if (-not $icon) { exit 3 }

  $bmp = $icon.ToBitmap()
  $target = New-Object System.Drawing.Bitmap 64, 64
  $g = [System.Drawing.Graphics]::FromImage($target)
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
  $g.PixelOffsetMode   = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.DrawImage($bmp, 0, 0, 64, 64)
  $g.Dispose()
  $bmp.Dispose()
  $icon.Dispose()
  $target.Save($OutPng, [System.Drawing.Imaging.ImageFormat]::Png)
  $target.Dispose()
  exit 0
} catch {
  Write-Error $_.Exception.Message
  exit 1
}
"#;

    // Write the script to a temp file so we can invoke it with -File (avoids
    // long -EncodedCommand and quoting headaches).
    let script_path = std::env::temp_dir().join("daytrail_extract_icon.ps1");
    if std::fs::write(&script_path, script).is_err() {
        return None;
    }

    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script_path.to_string_lossy(),
            "-AppName",
            app_name,
            "-OutPng",
            &out_png.to_string_lossy(),
        ])
        .output()
        .ok()?;

    if !status.status.success() {
        return None;
    }

    let bytes = std::fs::read(&out_png).ok()?;
    if bytes.len() < 100 {
        return None;
    }
    let encoded = BASE64.encode(&bytes);
    Some(format!("data:image/png;base64,{}", encoded))
}

#[cfg(target_os = "windows")]
pub fn app_icon_data_url(app_name: &str) -> Option<String> {
    {
        let cache = icon_cache().lock().ok()?;
        if let Some(cached) = cache.get(app_name) {
            return if cached.is_empty() {
                None
            } else {
                Some(cached.clone())
            };
        }
    }

    let result = extract_icon_via_powershell(app_name);

    if let Ok(mut cache) = icon_cache().lock() {
        cache.insert(app_name.to_string(), result.clone().unwrap_or_default());
    }

    result
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn app_icon_data_url(_app_name: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{app_icon_data_url, canonical_bundle_folder};

    #[test]
    fn maps_common_display_names_to_bundle_folders() {
        assert_eq!(canonical_bundle_folder("VS Code"), "Visual Studio Code");
        assert_eq!(
            canonical_bundle_folder("Visual Studio Code"),
            "Visual Studio Code"
        );
        assert_eq!(canonical_bundle_folder("ChatGPT"), "ChatGPT");
        assert_eq!(canonical_bundle_folder("Warp"), "Warp");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn extracts_icons_for_installed_common_apps() {
        for (app_name, bundle_path) in [
            ("VS Code", "/Applications/Visual Studio Code.app"),
            ("ChatGPT", "/Applications/ChatGPT.app"),
            ("Codex", "/Applications/Codex.app"),
            ("Warp", "/Applications/Warp.app"),
        ] {
            if Path::new(bundle_path).exists() {
                let icon = app_icon_data_url(app_name)
                    .unwrap_or_else(|| panic!("expected icon for {app_name}"));
                assert!(icon.starts_with("data:image/png;base64,"));
                assert!(icon.len() > 1_000);
            }
        }
    }
}
