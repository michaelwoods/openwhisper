use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::output::clipboard::has_command_in_path;

#[cfg(target_os = "linux")]
const APP_ICON: &str = include_str!("../assets/icons/openwhisper.svg");
#[cfg(target_os = "linux")]
const TRAY_IDLE_ICON: &str = include_str!("../assets/icons/openwhisper-tray-idle.svg");
#[cfg(target_os = "linux")]
const TRAY_RECORDING_ICON: &str = include_str!("../assets/icons/openwhisper-tray-recording.svg");
#[cfg(target_os = "linux")]
const TRAY_TRANSCRIBING_ICON: &str = include_str!("../assets/icons/openwhisper-tray-transcribing.svg");
#[cfg(target_os = "linux")]
const TRAY_DEGRADED_ICON: &str = include_str!("../assets/icons/openwhisper-tray-degraded.svg");
#[cfg(target_os = "linux")]
const TRAY_ERROR_ICON: &str = include_str!("../assets/icons/openwhisper-tray-error.svg");
#[cfg(target_os = "linux")]
const SERVICE_UNIT: &str = include_str!("../systemd/openwhisper.service");
#[cfg(target_os = "linux")]
const TOGGLE_DESKTOP: &str = include_str!("../desktop/net.local.openwhisper.desktop");
#[cfg(target_os = "linux")]
const SETTINGS_DESKTOP: &str = include_str!("../desktop/net.local.openwhisper.settings.desktop");

fn get_home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .unwrap_or_else(|| {
            std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."))
        })
}

pub fn run_setup() -> Result<()> {
    println!("🚀 Running OpenWhisper System Setup...\n");

    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;
    let home = get_home_dir();

    // 1. Cross-Platform Binary Installation
    println!("📦 Step 1: Installing OpenWhisper binary...");
    install_binary(&current_exe, &home)?;

    // 2. Platform-Specific Desktop Integration
    #[cfg(target_os = "linux")]
    {
        install_linux_desktop(&home)?;
        install_linux_systemd(&home)?;
    }

    #[cfg(target_os = "macos")]
    {
        println!("🍎 macOS platform detected:");
        println!("   ✓ Binary is ready in user PATH (~/.local/bin or /usr/local/bin)");
        println!("   💡 To run on startup, add a LaunchAgent plist to ~/Library/LaunchAgents");
    }

    #[cfg(target_os = "windows")]
    {
        println!("🪟 Windows platform detected:");
        println!("   ✓ Binary is installed in user executable path");
        println!("   💡 To run on startup, place a shortcut in your Startup folder");
    }

    println!("\n✅ OpenWhisper setup completed successfully!");
    Ok(())
}

fn install_binary(current_exe: &Path, home: &Path) -> Result<()> {
    let user_bin_dir = home.join(".local/bin");
    let _ = fs::create_dir_all(&user_bin_dir);
    let user_bin = user_bin_dir.join("openwhisper");
    let tmp_bin = user_bin_dir.join(".openwhisper.tmp");

    if fs::copy(current_exe, &tmp_bin).is_ok() && fs::rename(&tmp_bin, &user_bin).is_ok() {
        println!("   ✓ Installed to {}", user_bin.display());
    } else if let Err(e) = fs::copy(current_exe, &user_bin) {
        println!("   ⚠️  Could not copy to {}: {e}", user_bin.display());
    } else {
        println!("   ✓ Installed to {}", user_bin.display());
    }

    // Attempt system-wide install if /usr/local/bin is writable
    let usr_bin = PathBuf::from("/usr/local/bin/openwhisper");
    let tmp_usr = PathBuf::from("/usr/local/bin/.openwhisper.tmp");
    if fs::copy(current_exe, &tmp_usr).is_ok() && fs::rename(&tmp_usr, &usr_bin).is_ok() {
        println!("   ✓ Installed to /usr/local/bin/openwhisper");
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn install_linux_desktop(home: &Path) -> Result<()> {
    // Deploy Scalable & Status Icons
    println!("🎨 Step 2: Deploying icon theme assets...");
    let icons_dir = home.join(".local/share/icons/hicolor");
    let apps_icon_dir = icons_dir.join("scalable/apps");
    let status_icon_dir = icons_dir.join("scalable/status");
    let pixmaps_dir = home.join(".local/share/pixmaps");

    let _ = fs::create_dir_all(&apps_icon_dir);
    let _ = fs::create_dir_all(&status_icon_dir);
    let _ = fs::create_dir_all(&pixmaps_dir);

    // Ensure hicolor index.theme exists so icon loaders and caches discover the theme
    let index_theme_path = icons_dir.join("index.theme");
    if !index_theme_path.exists() {
        if Path::new("/usr/share/icons/hicolor/index.theme").exists() {
            let _ = fs::copy("/usr/share/icons/hicolor/index.theme", &index_theme_path);
        } else {
            let fallback_index = "[Icon Theme]\nName=Hicolor\nComment=Fallback icon theme\nHidden=true\nDirectories=16x16/apps,24x24/apps,32x32/apps,48x48/apps,64x64/apps,128x128/apps,256x256/apps,512x512/apps,scalable/apps,scalable/status\n";
            let _ = fs::write(&index_theme_path, fallback_index);
        }
    }

    let _ = fs::write(apps_icon_dir.join("openwhisper.svg"), APP_ICON);
    let _ = fs::write(status_icon_dir.join("openwhisper-tray-idle.svg"), TRAY_IDLE_ICON);
    let _ = fs::write(status_icon_dir.join("openwhisper-tray-recording.svg"), TRAY_RECORDING_ICON);
    let _ = fs::write(status_icon_dir.join("openwhisper-tray-transcribing.svg"), TRAY_TRANSCRIBING_ICON);
    let _ = fs::write(status_icon_dir.join("openwhisper-tray-degraded.svg"), TRAY_DEGRADED_ICON);
    let _ = fs::write(status_icon_dir.join("openwhisper-tray-error.svg"), TRAY_ERROR_ICON);
    let _ = fs::write(pixmaps_dir.join("openwhisper.svg"), APP_ICON);

    let convert_cmd = if has_command_in_path("magick") {
        Some("magick")
    } else if has_command_in_path("convert") {
        Some("convert")
    } else {
        None
    };

    if let Some(cmd) = convert_cmd {
        for size in [16, 24, 32, 48, 64, 128, 256, 512] {
            let size_dir = icons_dir.join(format!("{}x{}/apps", size, size));
            let _ = fs::create_dir_all(&size_dir);
            let out_png = size_dir.join("openwhisper.png");
            let _ = std::process::Command::new(cmd)
                .args(["-background", "none"])
                .arg(apps_icon_dir.join("openwhisper.svg"))
                .args(["-resize", &format!("{}x{}", size, size)])
                .arg(&out_png)
                .output();
        }
        let _ = fs::copy(
            icons_dir.join("256x256/apps/openwhisper.png"),
            pixmaps_dir.join("openwhisper.png"),
        );
    }
    println!("   ✓ Icons deployed to {}", icons_dir.display());

    // Register Desktop Applications
    println!("🖥️  Step 3: Registering desktop entries...");
    let app_dir = home.join(".local/share/applications");
    let _ = fs::create_dir_all(&app_dir);
    let _ = fs::write(app_dir.join("net.local.openwhisper.desktop"), TOGGLE_DESKTOP);
    let _ = fs::write(app_dir.join("net.local.openwhisper.settings.desktop"), SETTINGS_DESKTOP);
    println!("   ✓ Registered net.local.openwhisper.desktop (Dictation Toggle shortcut)");
    println!("   ✓ Registered net.local.openwhisper.settings.desktop (Settings GUI panel)");

    // Refresh desktop/icon database if utilities are installed
    if has_command_in_path("kbuildsycoca6") {
        let _ = std::process::Command::new("kbuildsycoca6")
            .arg("--noincremental")
            .output();
    }
    if has_command_in_path("update-desktop-database") {
        let _ = std::process::Command::new("update-desktop-database")
            .arg(&app_dir)
            .output();
    }
    if has_command_in_path("gtk-update-icon-cache") {
        let _ = std::process::Command::new("gtk-update-icon-cache")
            .arg("-q")
            .arg("-t")
            .arg("-f")
            .arg(&icons_dir)
            .output();
    }

    // Wayland / Compositor Floating Window Rules (KWin for KDE Plasma)
    configure_kwin_rules();

    Ok(())
}

#[cfg(target_os = "linux")]
fn configure_kwin_rules() {
    let write_cmd = if has_command_in_path("kwriteconfig6") {
        "kwriteconfig6"
    } else if has_command_in_path("kwriteconfig5") {
        "kwriteconfig5"
    } else if has_command_in_path("kwriteconfig") {
        "kwriteconfig"
    } else {
        return;
    };

    let read_cmd = if has_command_in_path("kreadconfig6") {
        "kreadconfig6"
    } else if has_command_in_path("kreadconfig5") {
        "kreadconfig5"
    } else if has_command_in_path("kreadconfig") {
        "kreadconfig"
    } else {
        return;
    };

    println!("🪟 Step 4: Configuring KWin window rules for Wayland HUD overlay...");

    let rules_to_set = [
        ("Description", "OpenWhisper Floating HUD"),
        ("wmclass", "net.local.openwhisper.hud"),
        ("wmclassmatch", "1"),
        ("wmclasscomplete", "false"),
        ("above", "true"),
        ("aboverule", "2"),
        ("noborder", "true"),
        ("noborderrule", "2"),
        ("skiptaskbar", "true"),
        ("skiptaskbarrule", "2"),
        ("skippager", "true"),
        ("skippagerrule", "2"),
    ];

    for (key, val) in rules_to_set {
        let _ = std::process::Command::new(write_cmd)
            .args(["--file", "kwinrulesrc", "--group", "openwhisper_hud", "--key", key, val])
            .output();
    }

    // Ensure openwhisper_hud is in the active rules list in [General]
    let current_rules_output = std::process::Command::new(read_cmd)
        .args(["--file", "kwinrulesrc", "--group", "General", "--key", "rules"])
        .output();

    let existing_rules = current_rules_output
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let existing_rules = existing_rules.trim();

    let rule_name = "openwhisper_hud";
    let is_present = existing_rules
        .split(',')
        .any(|r| r.trim() == rule_name);

    if !is_present {
        let updated_rules = if existing_rules.is_empty() {
            rule_name.to_string()
        } else {
            format!("{existing_rules},{rule_name}")
        };
        let _ = std::process::Command::new(write_cmd)
            .args(["--file", "kwinrulesrc", "--group", "General", "--key", "rules", &updated_rules])
            .output();
    }

    // Reconfigure KWin if qdbus or qdbus-qt6 is available
    let qdbus_cmd = if has_command_in_path("qdbus-qt6") {
        Some("qdbus-qt6")
    } else if has_command_in_path("qdbus") {
        Some("qdbus")
    } else {
        None
    };

    if let Some(cmd) = qdbus_cmd {
        let _ = std::process::Command::new(cmd)
            .args(["org.kde.KWin", "/KWin", "reconfigure"])
            .output();
    }

    // Set initial position based on current config (defaulting to BottomCenter)
    let cfg = crate::config::Config::load().unwrap_or_default();
    sync_kwin_hud_position(cfg.hud_position);

    println!("   ✓ KWin rule 'openwhisper_hud' configured (Keep-Above & screen position forced for Wayland)");
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(target_os = "linux")]
pub fn detect_screen_geometry() -> (i32, i32) {
    // 1. Try kscreen-doctor on KDE Wayland
    if let Ok(output) = std::process::Command::new("kscreen-doctor").arg("-o").output() {
        if let Ok(text) = String::from_utf8(output.stdout) {
            for line in text.lines() {
                if line.contains("Geometry:") {
                    let clean = strip_ansi(line);
                    for token in clean.split_whitespace() {
                        if token.contains('x') && !token.contains('@') && !token.contains(',') {
                            let parts: Vec<&str> = token.split('x').collect();
                            if parts.len() == 2 {
                                if let (Ok(w), Ok(h)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                                    if w >= 400 && h >= 300 {
                                        return (w, h);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Fallback: xrandr
    if let Ok(output) = std::process::Command::new("xrandr").arg("--current").output() {
        if let Ok(text) = String::from_utf8(output.stdout) {
            for line in text.lines() {
                if line.contains(" connected") {
                    for token in line.split_whitespace() {
                        if let Some((geom, _)) = token.split_once('+') {
                            if let Some((w_str, h_str)) = geom.split_once('x') {
                                if let (Ok(w), Ok(h)) = (w_str.parse::<i32>(), h_str.parse::<i32>()) {
                                    if w >= 400 && h >= 300 {
                                        return (w, h);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    (1920, 1080)
}

#[cfg(target_os = "linux")]
pub fn sync_kwin_hud_position(position: crate::config::HudPosition) {
    let write_cmd = if has_command_in_path("kwriteconfig6") {
        "kwriteconfig6"
    } else if has_command_in_path("kwriteconfig5") {
        "kwriteconfig5"
    } else if has_command_in_path("kwriteconfig") {
        "kwriteconfig"
    } else {
        return;
    };

    let (screen_w, screen_h) = detect_screen_geometry();
    let win_w = 290;
    let win_h = 48;
    let margin_y = 60;
    let margin_x = 40;

    let (x, y) = match position {
        crate::config::HudPosition::BottomCenter => {
            ((screen_w - win_w) / 2, screen_h - win_h - margin_y)
        }
        crate::config::HudPosition::TopCenter => {
            ((screen_w - win_w) / 2, margin_y)
        }
        crate::config::HudPosition::BottomRight => {
            (screen_w - win_w - margin_x, screen_h - win_h - margin_y)
        }
        crate::config::HudPosition::TopRight => {
            (screen_w - win_w - margin_x, margin_y)
        }
    };

    let pos_str = format!("{x},{y}");
    let _ = std::process::Command::new(write_cmd)
        .args(["--file", "kwinrulesrc", "--group", "openwhisper_hud", "--key", "position", &pos_str])
        .output();

    let _ = std::process::Command::new(write_cmd)
        .args(["--file", "kwinrulesrc", "--group", "openwhisper_hud", "--key", "positionrule", "2"])
        .output();

    let qdbus_cmd = if has_command_in_path("qdbus-qt6") {
        Some("qdbus-qt6")
    } else if has_command_in_path("qdbus") {
        Some("qdbus")
    } else {
        None
    };

    if let Some(cmd) = qdbus_cmd {
        let _ = std::process::Command::new(cmd)
            .args(["org.kde.KWin", "/KWin", "reconfigure"])
            .output();
    }
}

#[cfg(target_os = "linux")]
fn install_linux_systemd(home: &Path) -> Result<()> {
    println!("⚙️  Step 5: Configuring systemd user service...");
    let systemd_dir = home.join(".config/systemd/user");
    let _ = fs::create_dir_all(&systemd_dir);
    let unit_path = systemd_dir.join("openwhisper.service");
    fs::write(&unit_path, SERVICE_UNIT)
        .with_context(|| format!("Failed to write service unit to {}", unit_path.display()))?;
    println!("   ✓ Installed {}", unit_path.display());

    if has_command_in_path("systemctl") {
        let mut reload_cmd = std::process::Command::new("systemctl");
        reload_cmd.arg("--user").arg("daemon-reload");
        apply_systemd_env(&mut reload_cmd);
        let _ = reload_cmd.output();

        let mut enable_cmd = std::process::Command::new("systemctl");
        enable_cmd.arg("--user").arg("enable").arg("openwhisper.service");
        apply_systemd_env(&mut enable_cmd);
        let _ = enable_cmd.output();

        let mut restart_cmd = std::process::Command::new("systemctl");
        restart_cmd.arg("--user").arg("restart").arg("openwhisper.service");
        apply_systemd_env(&mut restart_cmd);
        let _ = restart_cmd.output();

        println!("   ✓ systemd user service reloaded, enabled, and restarted.");
    } else {
        println!("   ⚠️  systemctl not found in PATH; skipping service activation.");
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_systemd_env(cmd: &mut std::process::Command) {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        cmd.env("XDG_RUNTIME_DIR", &runtime_dir);
    }
    if let Ok(bus) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        cmd.env("DBUS_SESSION_BUS_ADDRESS", &bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        let input = "\x1b[01;33m\tGeometry: \x1b[0;0m0,0 1707x1067";
        let cleaned = strip_ansi(input);
        assert_eq!(cleaned, "\tGeometry: 0,0 1707x1067");
    }

    #[test]
    fn test_detect_screen_geometry_bounds() {
        let (w, h) = detect_screen_geometry();
        assert!(w >= 640);
        assert!(h >= 480);
    }
}

