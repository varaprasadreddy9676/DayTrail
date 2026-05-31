use std::{fs, process::Command};

#[cfg(target_os = "linux")]
use std::{io::Write, process::Stdio};

use anyhow::{anyhow, Context, Result};

const SERVICE_NAME: &str = "ai.daytrail.desktop";

/// Build a `Command` that never flashes a console window on Windows. DayTrail is
/// a GUI/tray app, so any helper process it spawns (git, powershell, reg) must be
/// windowless — otherwise a cmd window blinks on every poll. No-op on other OSes.
/// A tasteful, consistent notification chime per platform — DayTrail's "signature
/// sound" so a nudge feels intentional, not like a silent system blip. macOS
/// "Glass" is a crisp, premium-feeling chime; Windows/Linux use their pleasant
/// default notification sound.
pub(crate) fn notification_sound() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        return "Glass";
    }
    #[cfg(target_os = "windows")]
    {
        return "Default";
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "message-new-instant"
    }
}

pub(crate) fn hidden_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

pub trait KeychainAdapter {
    fn keychain_get(&self, key: &str) -> Result<Option<String>>;
    fn keychain_set(&self, key: &str, value: &str) -> Result<()>;
}

pub struct SystemKeychain;

impl KeychainAdapter for SystemKeychain {
    fn keychain_get(&self, key: &str) -> Result<Option<String>> {
        keychain_get(key)
    }

    fn keychain_set(&self, key: &str, value: &str) -> Result<()> {
        keychain_set(key, value)
    }
}

pub fn keychain_key_for_ai_provider(provider: &str) -> String {
    let mut normalized = provider
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    while normalized.contains("--") {
        normalized = normalized.replace("--", "-");
    }
    let normalized = normalized.trim_matches('-');
    format!(
        "ai-provider-{}",
        if normalized.is_empty() {
            "default"
        } else {
            normalized
        }
    )
}

pub fn keychain_key_from_ref(value: &str) -> Option<&str> {
    value.trim().strip_prefix("keychain:")
}

pub fn set_launch_at_login(enabled: bool) -> Result<()> {
    if should_skip_launch_at_login_mutation() {
        return Ok(());
    }
    set_launch_at_login_platform(enabled)
}

fn should_skip_launch_at_login_mutation() -> bool {
    if std::env::var_os("DAYTRAIL_DISABLE_AUTOSTART_MUTATION").is_some() {
        return true;
    }

    std::env::current_exe()
        .ok()
        .map(|path| is_cargo_test_binary_path(&path.to_string_lossy()))
        .unwrap_or(false)
}

fn is_cargo_test_binary_path(path: &str) -> bool {
    path.replace('\\', "/").contains("/target/debug/deps/")
}

#[cfg(target_os = "macos")]
fn set_launch_at_login_platform(enabled: bool) -> Result<()> {
    let launch_agents_dir = dirs::home_dir()
        .context("failed to resolve home directory")?
        .join("Library")
        .join("LaunchAgents");
    let plist_path = launch_agents_dir.join(format!("{SERVICE_NAME}.plist"));

    if !enabled {
        match fs::remove_file(&plist_path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).context("failed to remove login item plist"),
        }
    }

    fs::create_dir_all(&launch_agents_dir).context("failed to create LaunchAgents directory")?;
    let executable = std::env::current_exe()
        .context("failed to resolve current executable for login item")?
        .display()
        .to_string();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
        xml_escape(SERVICE_NAME),
        xml_escape(&executable)
    );
    fs::write(plist_path, plist).context("failed to write login item plist")
}

#[cfg(target_os = "linux")]
fn set_launch_at_login_platform(enabled: bool) -> Result<()> {
    let autostart_dir = dirs::config_dir()
        .context("failed to resolve config directory")?
        .join("autostart");
    let desktop_path = autostart_dir.join(format!("{SERVICE_NAME}.desktop"));

    if !enabled {
        match fs::remove_file(&desktop_path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).context("failed to remove autostart desktop file"),
        }
    }

    fs::create_dir_all(&autostart_dir).context("failed to create autostart directory")?;
    let executable = std::env::current_exe()
        .context("failed to resolve current executable for autostart")?
        .display()
        .to_string();
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=DayTrail\nExec={}\nX-GNOME-Autostart-enabled=true\n",
        executable
    );
    fs::write(desktop_path, desktop).context("failed to write autostart desktop file")
}

#[cfg(target_os = "windows")]
fn set_launch_at_login_platform(enabled: bool) -> Result<()> {
    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "DayTrail";

    let legacy_cmd_path = std::env::var_os("APPDATA").map(|appdata| {
        std::path::PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
            .join("DayTrail.cmd")
    });

    if !enabled {
        let _ = hidden_command("reg")
            .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
            .status()
            .context("failed to invoke reg.exe to remove startup entry")?;
        if let Some(cmd_path) = legacy_cmd_path {
            match fs::remove_file(&cmd_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("failed to remove legacy startup command"),
            }
        }
        return Ok(());
    }

    let executable = std::env::current_exe()
        .context("failed to resolve current executable for startup")?
        .display()
        .to_string();
    let command = format!("\"{executable}\"");
    let status = hidden_command("reg")
        .args([
            "add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &command, "/f",
        ])
        .status()
        .context("failed to invoke reg.exe to create startup entry")?;
    if !status.success() {
        return Err(anyhow!("reg.exe failed to create startup entry"));
    }

    if let Some(cmd_path) = legacy_cmd_path {
        match fs::remove_file(&cmd_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("failed to remove legacy startup command"),
        }
    }
    Ok(())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
fn keychain_set(key: &str, value: &str) -> Result<()> {
    let status = Command::new("security")
        .args([
            "add-generic-password",
            "-a",
            key,
            "-s",
            SERVICE_NAME,
            "-w",
            value,
            "-U",
        ])
        .status()
        .context("failed to invoke macOS keychain")?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("macOS keychain rejected secret for {key}"))
    }
}

#[cfg(target_os = "macos")]
fn keychain_get(key: &str) -> Result<Option<String>> {
    let output = Command::new("security")
        .args(["find-generic-password", "-a", key, "-s", SERVICE_NAME, "-w"])
        .output()
        .context("failed to invoke macOS keychain")?;

    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string(),
        ));
    }

    if output.status.code() == Some(44) {
        return Ok(None);
    }

    Err(anyhow!("macOS keychain lookup failed for {key}"))
}

#[cfg(target_os = "linux")]
fn keychain_set(key: &str, value: &str) -> Result<()> {
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label",
            "DayTrail",
            "application",
            SERVICE_NAME,
            "account",
            key,
        ])
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to invoke Secret Service via secret-tool")?;
    child
        .stdin
        .as_mut()
        .context("secret-tool stdin unavailable")?
        .write_all(value.as_bytes())
        .context("failed to write secret to secret-tool")?;
    let status = child.wait().context("failed to wait for secret-tool")?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("Secret Service rejected secret for {key}"))
    }
}

#[cfg(target_os = "linux")]
fn keychain_get(key: &str) -> Result<Option<String>> {
    let output = Command::new("secret-tool")
        .args(["lookup", "application", SERVICE_NAME, "account", key])
        .output()
        .context("failed to invoke Secret Service via secret-tool")?;

    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string(),
        ));
    }

    Ok(None)
}

#[cfg(target_os = "windows")]
fn keychain_set(key: &str, value: &str) -> Result<()> {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::FILETIME,
        Security::Credentials::{
            CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        },
    };

    let mut target_name = windows_wide(&format!("{SERVICE_NAME}:{key}"));
    let blob = value.as_bytes();
    anyhow::ensure!(
        blob.len() <= 5 * 512,
        "Windows Credential Manager secret is too large"
    );

    let mut credential = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target_name.as_mut_ptr(),
        Comment: ptr::null_mut(),
        LastWritten: FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        },
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_ptr() as *mut u8,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: ptr::null_mut(),
        TargetAlias: ptr::null_mut(),
        UserName: ptr::null_mut(),
    };

    let ok = unsafe { CredWriteW(&mut credential, 0) };
    if ok == 0 {
        Err(anyhow!(
            "Windows Credential Manager rejected secret for {key}"
        ))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn keychain_get(key: &str) -> Result<Option<String>> {
    use std::{ptr, slice};
    use windows_sys::Win32::Security::Credentials::{CredFree, CredReadW, CRED_TYPE_GENERIC};

    let target_name = windows_wide(&format!("{SERVICE_NAME}:{key}"));
    let mut credential = ptr::null_mut();
    let ok = unsafe { CredReadW(target_name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
    if ok == 0 || credential.is_null() {
        return Ok(None);
    }

    let value = unsafe {
        let credential_ref = &*credential;
        let bytes = slice::from_raw_parts(
            credential_ref.CredentialBlob,
            credential_ref.CredentialBlobSize as usize,
        );
        let value = String::from_utf8_lossy(bytes).to_string();
        CredFree(credential as _);
        value
    };
    Ok(Some(value))
}

#[cfg(target_os = "windows")]
fn windows_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn keychain_set(_key: &str, _value: &str) -> Result<()> {
    Err(anyhow!(
        "OS keychain storage is not implemented for this platform"
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn keychain_get(_key: &str) -> Result<Option<String>> {
    Err(anyhow!(
        "OS keychain lookup is not implemented for this platform"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_keychain_references_are_stable_and_redacted() {
        assert_eq!(
            keychain_key_for_ai_provider("OpenAI Compatible"),
            "ai-provider-openai-compatible"
        );
        assert_eq!(
            keychain_key_for_ai_provider("  LM Studio / Local  "),
            "ai-provider-lm-studio-local"
        );
        assert_eq!(keychain_key_for_ai_provider(""), "ai-provider-default");
    }

    #[test]
    fn test_binaries_do_not_mutate_login_items() {
        assert!(should_skip_launch_at_login_mutation());
    }

    #[test]
    fn detects_unix_and_windows_cargo_test_binary_paths() {
        assert!(is_cargo_test_binary_path(
            "/repo/apps/desktop/src-tauri/target/debug/deps/core_behavior-abc123"
        ));
        assert!(is_cargo_test_binary_path(
            r"C:\repo\apps\desktop\src-tauri\target\debug\deps\core_behavior-abc123.exe"
        ));
        assert!(!is_cargo_test_binary_path(
            r"C:\Program Files\DayTrail\daytrail.exe"
        ));
        assert!(!is_cargo_test_binary_path(
            "/Applications/DayTrail.app/Contents/MacOS/daytrail"
        ));
    }
}
