use anyhow::{Context, Result};
use arboard::Clipboard;

pub fn set_clipboard(text: &str) -> Result<()> {
    // On Linux/Wayland, prioritize wl-copy for instant, native Wayland clipboard handling
    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};
        if std::env::var("WAYLAND_DISPLAY").is_ok() || which_cmd("wl-copy") {
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

#[cfg(target_os = "linux")]
fn which_cmd(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
