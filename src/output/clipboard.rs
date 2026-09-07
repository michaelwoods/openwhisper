use anyhow::{Context, Result};
use arboard::Clipboard;

pub fn set_clipboard(text: &str) -> Result<()> {
    // On Linux/Wayland, prioritize wl-copy for instant, native Wayland clipboard handling
    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};
        if std::env::var("WAYLAND_DISPLAY").is_ok() || has_command_in_path("wl-copy") {
            if let Ok(mut child) = Command::new("wl-copy")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                if let Ok(status) = child.wait() {
                    if status.success() {
                        return Ok(());
                    }
                }
            }
        }
    }

    // Standard cross-platform clipboard using arboard (Windows, macOS, X11)
    let mut clipboard = Clipboard::new().context("Failed to open system clipboard")?;
    clipboard
        .set_text(text.to_string())
        .context("Failed to set clipboard text")?;

    Ok(())
}

/// Reads the current text from the system clipboard.
pub fn get_clipboard() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        if has_command_in_path("wl-paste") {
            if let Ok(output) = Command::new("wl-paste").arg("--no-newline").output() {
                if output.status.success() {
                    if let Ok(s) = String::from_utf8(output.stdout) {
                        return Some(s);
                    }
                }
            }
        }
    }

    if let Ok(mut clipboard) = Clipboard::new() {
        return clipboard.get_text().ok();
    }

    None
}

/// Checks if an executable command exists in standard system PATH using Rust stdlib.
pub fn has_command_in_path(cmd: &str) -> bool {
    if let Some(path_os) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_os) {
            let bin_path = dir.join(cmd);
            if bin_path.is_file() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_command_in_path_existing_and_nonexistent() {
        // Standard unix tools like 'sh' or 'ls' exist in PATH
        #[cfg(unix)]
        assert!(has_command_in_path("sh"));

        assert!(!has_command_in_path("non_existent_binary_xyz_12345"));
    }
}
