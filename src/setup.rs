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
const TRAY_TRANSCRIBING_ICON: &str =
    include_str!("../assets/icons/openwhisper-tray-transcribing.svg");
#[cfg(target_os = "linux")]
const TRAY_DEGRADED_ICON: &str = include_str!("../assets/icons/openwhisper-tray-degraded.svg");
#[cfg(target_os = "linux")]
const TRAY_ERROR_ICON: &str = include_str!("../assets/icons/openwhisper-tray-error.svg");
#[cfg(target_os = "linux")]
const SERVICE_UNIT: &str = include_str!("../systemd/openwhisper.service");
#[cfg(target_os = "linux")]
const UI_SERVICE_UNIT: &str = include_str!("../systemd/openwhisper-ui.service");
#[cfg(target_os = "linux")]
const TOGGLE_DESKTOP: &str = include_str!("../desktop/net.local.openwhisper.desktop");
#[cfg(target_os = "linux")]
const SETTINGS_DESKTOP: &str = include_str!("../desktop/net.local.openwhisper.settings.desktop");
#[cfg(target_os = "linux")]
const UI_DESKTOP: &str = include_str!("../desktop/net.local.openwhisper-ui.desktop");

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
    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;
    let home = get_home_dir();
    run_setup_for_home(&current_exe, &home)
}

pub fn run_setup_for_home(current_exe: &Path, home: &Path) -> Result<()> {
    println!("🚀 Running OpenWhisper System Setup...\n");

    // 1. Cross-Platform Binary Installation
    println!("📦 Step 1: Installing OpenWhisper binary...");
    install_binary(current_exe, home)?;

    // 2. Platform-Specific Desktop Integration
    #[cfg(target_os = "linux")]
    {
        install_linux_desktop(home)?;
        install_linux_systemd(home)?;
    }

    #[cfg(target_os = "macos")]
    {
        println!("🍎 macOS platform detected:");
        println!("   ✓ Binary is ready in user PATH (~/.local/bin)");
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

pub fn install_binary(current_exe: &Path, home: &Path) -> Result<()> {
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
    let _ = fs::write(
        status_icon_dir.join("openwhisper-tray-idle.svg"),
        TRAY_IDLE_ICON,
    );
    let _ = fs::write(
        status_icon_dir.join("openwhisper-tray-recording.svg"),
        TRAY_RECORDING_ICON,
    );
    let _ = fs::write(
        status_icon_dir.join("openwhisper-tray-transcribing.svg"),
        TRAY_TRANSCRIBING_ICON,
    );
    let _ = fs::write(
        status_icon_dir.join("openwhisper-tray-degraded.svg"),
        TRAY_DEGRADED_ICON,
    );
    let _ = fs::write(
        status_icon_dir.join("openwhisper-tray-error.svg"),
        TRAY_ERROR_ICON,
    );
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
    let _ = fs::write(
        app_dir.join("net.local.openwhisper.desktop"),
        TOGGLE_DESKTOP,
    );
    let _ = fs::write(
        app_dir.join("net.local.openwhisper.settings.desktop"),
        SETTINGS_DESKTOP,
    );
    let _ = fs::write(app_dir.join("net.local.openwhisper-ui.desktop"), UI_DESKTOP);
    println!("   ✓ Registered net.local.openwhisper.desktop (Dictation Toggle shortcut)");
    println!("   ✓ Registered net.local.openwhisper.settings.desktop (Settings GUI panel)");
    println!("   ✓ Registered net.local.openwhisper-ui.desktop (System Tray & Overlay UI)");

    // Refresh desktop/icon database and compositor rules for live user sessions
    if home == get_home_dir() {
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
        #[cfg(target_os = "linux")]
        if let Err(e) = configure_kwin_rules() {
            eprintln!("   ⚠️  KWin rule configuration could not be applied: {e}");
        }
    }

    Ok(())
}

/// Pure builder returning the canonical list of KWin rule properties for the OpenWhisper HUD window.
/// Includes `skipswitcher=true` / `skipswitcherrule=2` (Force) to exclude the HUD from KDE Plasma's Alt+Tab.
pub fn hud_kwin_rule() -> Vec<(&'static str, String)> {
    vec![
        ("Description", "OpenWhisper Floating HUD".to_string()),
        ("wmclass", "net.local.openwhisper.hud".to_string()),
        ("wmclassmatch", "1".to_string()),
        ("wmclasscomplete", "false".to_string()),
        ("above", "true".to_string()),
        ("aboverule", "2".to_string()),
        ("noborder", "true".to_string()),
        ("noborderrule", "2".to_string()),
        ("skiptaskbar", "true".to_string()),
        ("skiptaskbarrule", "2".to_string()),
        ("skippager", "true".to_string()),
        ("skippagerrule", "2".to_string()),
        ("skipswitcher", "true".to_string()),
        ("skipswitcherrule", "2".to_string()),
    ]
}

#[cfg(target_os = "linux")]
pub fn find_kwriteconfig() -> Option<&'static str> {
    if has_command_in_path("kwriteconfig6") {
        Some("kwriteconfig6")
    } else if has_command_in_path("kwriteconfig5") {
        Some("kwriteconfig5")
    } else if has_command_in_path("kwriteconfig") {
        Some("kwriteconfig")
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
pub fn find_kreadconfig() -> Option<&'static str> {
    if has_command_in_path("kreadconfig6") {
        Some("kreadconfig6")
    } else if has_command_in_path("kreadconfig5") {
        Some("kreadconfig5")
    } else if has_command_in_path("kreadconfig") {
        Some("kreadconfig")
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
pub fn write_kwin_config_key(file: &str, group: &str, key: &str, val: &str) -> Result<()> {
    let write_cmd = find_kwriteconfig()
        .context("Neither kwriteconfig6, kwriteconfig5, nor kwriteconfig found in PATH")?;
    let mut cmd = std::process::Command::new(write_cmd);
    cmd.args(["--file", file, "--group", group, "--key", key, val]);
    let path = Path::new(file);
    if let Some(parent) = path.parent().filter(|_| path.is_absolute()) {
        cmd.env("XDG_CONFIG_HOME", parent);
    }
    apply_display_env(&mut cmd);
    let output = cmd
        .output()
        .with_context(|| format!("Failed to execute {write_cmd} for key '{key}'"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "{write_cmd} failed (exit status {}): {stderr}",
            output.status
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn read_kwin_config_key(file: &str, group: &str, key: &str) -> Result<String> {
    let read_cmd = find_kreadconfig()
        .context("Neither kreadconfig6, kreadconfig5, nor kreadconfig found in PATH")?;
    let mut cmd = std::process::Command::new(read_cmd);
    cmd.args([
        "--file",
        file,
        "--group",
        group,
        "--key",
        key,
        "--default",
        "",
    ]);
    let path = Path::new(file);
    if let Some(parent) = path.parent().filter(|_| path.is_absolute()) {
        cmd.env("XDG_CONFIG_HOME", parent);
    }
    apply_display_env(&mut cmd);
    let output = cmd
        .output()
        .with_context(|| format!("Failed to execute {read_cmd} for key '{key}'"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "{read_cmd} failed (exit status {}): {stderr}",
            output.status
        );
    }
    let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(val)
}

#[cfg(target_os = "linux")]
pub fn verify_kwin_rules(file: &str) -> Result<()> {
    // 1. Verify that openwhisper_hud is in [General] rules
    let current_rules = read_kwin_config_key(file, "General", "rules")?;
    let is_present = current_rules
        .split(',')
        .any(|r| r.trim() == "openwhisper_hud");
    if !is_present {
        anyhow::bail!(
            "KWin rule 'openwhisper_hud' is missing from [General] rules (found: '{current_rules}')"
        );
    }

    // 2. Verify all properties from hud_kwin_rule()
    for (key, expected_val) in hud_kwin_rule() {
        let actual_val = read_kwin_config_key(file, "openwhisper_hud", key)?;
        if actual_val != expected_val {
            anyhow::bail!(
                "KWin rule property mismatch for '{key}': expected '{expected_val}', found '{actual_val}'"
            );
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn reconfigure_kwin() {
    let qdbus_cmd = if has_command_in_path("qdbus-qt6") {
        Some("qdbus-qt6")
    } else if has_command_in_path("qdbus") {
        Some("qdbus")
    } else {
        None
    };

    if let Some(cmd) = qdbus_cmd {
        let mut dbus_cmd = std::process::Command::new(cmd);
        dbus_cmd.args(["org.kde.KWin", "/KWin", "reconfigure"]);
        apply_display_env(&mut dbus_cmd);
        let _ = dbus_cmd.output();
    }
}

#[cfg(target_os = "linux")]
pub fn repair_kwin_rules(file: &str) -> Result<()> {
    find_kwriteconfig()
        .context("Neither kwriteconfig6, kwriteconfig5, nor kwriteconfig found in PATH")?;
    find_kreadconfig()
        .context("Neither kreadconfig6, kreadconfig5, nor kreadconfig found in PATH")?;

    // Upsert all rule keys from hud_kwin_rule()
    for (key, val) in hud_kwin_rule() {
        write_kwin_config_key(file, "openwhisper_hud", key, &val)?;
    }

    // Ensure openwhisper_hud is in [General] rules
    let current_rules = read_kwin_config_key(file, "General", "rules").unwrap_or_default();
    let mut rules: Vec<String> = current_rules
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if !rules.iter().any(|r| r == "openwhisper_hud") {
        rules.push("openwhisper_hud".to_string());
        let updated_rules = rules.join(",");
        write_kwin_config_key(file, "General", "rules", &updated_rules)?;
    }

    // Verify all keys with readback
    verify_kwin_rules(file)?;

    // If configuring live system rules, trigger KWin reconfigure
    if file == "kwinrulesrc" {
        reconfigure_kwin();
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn configure_kwin_rules() -> Result<()> {
    if find_kwriteconfig().is_none() || find_kreadconfig().is_none() {
        eprintln!("   ⚠️  kwriteconfig/kreadconfig not found in PATH; skipping KWin rules.");
        anyhow::bail!("kwriteconfig/kreadconfig not found in PATH");
    }

    println!("🪟 Step 4: Configuring KWin window rules for Wayland HUD overlay...");

    repair_kwin_rules("kwinrulesrc")?;

    // Set initial position based on current config (defaulting to BottomCenter)
    let cfg = crate::config::Config::load().unwrap_or_default();
    sync_kwin_hud_position(cfg.hud_position);

    println!(
        "   ✓ KWin rule 'openwhisper_hud' configured (Keep-Above, Skip-Taskbar, \
         Skip-Switcher & screen position forced for Wayland)"
    );

    Ok(())
}

/// Invoked at UI service start to ensure the KWin rule is present and verified.
/// This is cheap and read-only when the rules are already correct.
#[cfg(target_os = "linux")]
pub fn repair_kwin_rules_if_needed() {
    if find_kwriteconfig().is_none() || find_kreadconfig().is_none() {
        return;
    }

    if verify_kwin_rules("kwinrulesrc").is_ok() {
        return;
    }

    tracing::info!("KWin HUD window rules missing or incomplete; repairing...");
    if let Err(e) = repair_kwin_rules("kwinrulesrc") {
        tracing::warn!("Failed to repair KWin rules: {e}");
    } else {
        let cfg = crate::config::Config::load().unwrap_or_default();
        sync_kwin_hud_position(cfg.hud_position);
        tracing::info!("KWin HUD rules successfully repaired.");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn configure_kwin_rules() -> Result<()> {
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn verify_kwin_rules(_file: &str) -> Result<()> {
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn repair_kwin_rules_if_needed() {}

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
fn apply_display_env(cmd: &mut std::process::Command) -> bool {
    let mut has_display = false;

    // 1. Wayland display detection
    let wayland_disp = std::env::var("WAYLAND_DISPLAY")
        .ok()
        .filter(|s| !s.is_empty());
    if let Some(wayland_disp) = wayland_disp {
        cmd.env("WAYLAND_DISPLAY", &wayland_disp);
        has_display = true;
    } else {
        // Probe XDG_RUNTIME_DIR for wayland-* sockets
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
            let uid = unsafe { libc::getuid() };
            format!("/run/user/{uid}")
        });
        let r_path = std::path::Path::new(&runtime_dir);
        for i in 0..=4 {
            let socket_name = format!("wayland-{i}");
            if r_path.join(&socket_name).exists() {
                cmd.env("WAYLAND_DISPLAY", &socket_name);
                has_display = true;
                break;
            }
        }
    }

    // 2. X11 display detection
    let disp = std::env::var("DISPLAY").ok().filter(|s| !s.is_empty());
    if let Some(disp) = disp {
        cmd.env("DISPLAY", &disp);
        has_display = true;
    } else if std::path::Path::new("/tmp/.X11-unix/X0").exists() {
        cmd.env("DISPLAY", ":0");
        has_display = true;
    }

    // 3. XDG runtime directory and desktop environment variables
    if let Some(runtime_dir) = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|s| !s.is_empty())
    {
        cmd.env("XDG_RUNTIME_DIR", &runtime_dir);
    } else {
        let uid = unsafe { libc::getuid() };
        cmd.env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"));
    }

    if let Some(bus) = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .ok()
        .filter(|s| !s.is_empty())
    {
        cmd.env("DBUS_SESSION_BUS_ADDRESS", &bus);
    }

    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .filter(|s| !s.is_empty());
    if let Some(desktop) = desktop {
        cmd.env("XDG_CURRENT_DESKTOP", &desktop);
    } else {
        cmd.env("XDG_CURRENT_DESKTOP", "KDE");
    }

    has_display
}

#[cfg(target_os = "linux")]
pub fn detect_screen_geometry() -> (i32, i32) {
    // 1. Try kscreen-doctor on KDE Wayland
    let mut kscreen_cmd = std::process::Command::new("kscreen-doctor");
    kscreen_cmd.arg("-o");
    if apply_display_env(&mut kscreen_cmd) {
        kscreen_cmd.env("QT_QPA_PLATFORM", "wayland;xcb");
        if let Ok(output) = kscreen_cmd.output()
            && let Ok(text) = String::from_utf8(output.stdout)
        {
            for line in text.lines() {
                if line.contains("Geometry:") {
                    let clean = strip_ansi(line);
                    for token in clean.split_whitespace() {
                        if token.contains('x') && !token.contains('@') && !token.contains(',') {
                            let parts: Vec<&str> = token.split('x').collect();
                            if parts.len() == 2
                                && let (Ok(w), Ok(h)) =
                                    (parts[0].parse::<i32>(), parts[1].parse::<i32>())
                                && w >= 400
                                && h >= 300
                            {
                                return (w, h);
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Fallback: xrandr
    let mut xrandr_cmd = std::process::Command::new("xrandr");
    xrandr_cmd.arg("--current");
    if apply_display_env(&mut xrandr_cmd)
        && let Ok(output) = xrandr_cmd.output()
        && let Ok(text) = String::from_utf8(output.stdout)
    {
        for line in text.lines() {
            if line.contains(" connected") {
                for token in line.split_whitespace() {
                    if let Some((geom, _)) = token.split_once('+')
                        && let Some((w_str, h_str)) = geom.split_once('x')
                        && let (Ok(w), Ok(h)) = (w_str.parse::<i32>(), h_str.parse::<i32>())
                        && w >= 400
                        && h >= 300
                    {
                        return (w, h);
                    }
                }
            }
        }
    }

    (1920, 1080)
}

#[cfg(target_os = "linux")]
pub fn sync_kwin_hud_position(position: crate::config::HudPosition) {
    if find_kwriteconfig().is_none() {
        return;
    }

    let (screen_w, screen_h) = detect_screen_geometry();
    let win_w = 290;
    let win_h = 48;
    let margin_y = 60;
    let margin_x = 40;

    let (x, y) = match position {
        crate::config::HudPosition::BottomCenter => {
            ((screen_w - win_w) / 2, screen_h - win_h - margin_y)
        }
        crate::config::HudPosition::TopCenter => ((screen_w - win_w) / 2, margin_y),
        crate::config::HudPosition::BottomRight => {
            (screen_w - win_w - margin_x, screen_h - win_h - margin_y)
        }
        crate::config::HudPosition::TopRight => (screen_w - win_w - margin_x, margin_y),
    };

    let pos_str = format!("{x},{y}");
    let _ = write_kwin_config_key("kwinrulesrc", "openwhisper_hud", "position", &pos_str);
    let _ = write_kwin_config_key("kwinrulesrc", "openwhisper_hud", "positionrule", "2");

    reconfigure_kwin();
}

#[cfg(not(target_os = "linux"))]
pub fn sync_kwin_hud_position(_position: crate::config::HudPosition) {}

/// Cleans up any legacy XDG autostart entry from ~/.config/autostart.
/// Returns Ok(true) if a legacy file was found and removed, Ok(false) if absent.
/// Idempotent: safe to run multiple times.
pub fn cleanup_legacy_autostart(home: &Path) -> Result<bool> {
    let legacy_autostart = home.join(".config/autostart/net.local.openwhisper-ui.desktop");
    if legacy_autostart.exists() {
        fs::remove_file(&legacy_autostart).with_context(|| {
            format!(
                "Failed to remove legacy autostart file {}",
                legacy_autostart.display()
            )
        })?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(target_os = "linux")]
pub fn install_linux_systemd(home: &Path) -> Result<()> {
    println!("⚙️  Step 5: Configuring systemd user services...");
    let systemd_dir = home.join(".config/systemd/user");
    let _ = fs::create_dir_all(&systemd_dir);
    let unit_path = systemd_dir.join("openwhisper.service");
    fs::write(&unit_path, SERVICE_UNIT)
        .with_context(|| format!("Failed to write service unit to {}", unit_path.display()))?;
    println!("   ✓ Installed {}", unit_path.display());

    let ui_unit_path = systemd_dir.join("openwhisper-ui.service");
    fs::write(&ui_unit_path, UI_SERVICE_UNIT)
        .with_context(|| format!("Failed to write service unit to {}", ui_unit_path.display()))?;
    println!("   ✓ Installed {}", ui_unit_path.display());

    // Clean up any legacy XDG autostart entry left by previous versions
    if cleanup_legacy_autostart(home)? {
        println!("   ✓ Cleaned up legacy XDG Autostart entry in ~/.config/autostart");
    }

    if home == get_home_dir() && has_command_in_path("systemctl") {
        let mut reload_cmd = std::process::Command::new("systemctl");
        reload_cmd.arg("--user").arg("daemon-reload");
        apply_systemd_env(&mut reload_cmd);
        let _ = reload_cmd.output();

        let mut enable_cmd = std::process::Command::new("systemctl");
        enable_cmd
            .arg("--user")
            .arg("enable")
            .arg("openwhisper.service")
            .arg("openwhisper-ui.service");
        apply_systemd_env(&mut enable_cmd);
        let _ = enable_cmd.output();

        let mut restart_cmd = std::process::Command::new("systemctl");
        restart_cmd
            .arg("--user")
            .arg("restart")
            .arg("openwhisper.service")
            .arg("openwhisper-ui.service");
        apply_systemd_env(&mut restart_cmd);
        let _ = restart_cmd.output();

        println!("   ✓ systemd user services reloaded, enabled, and restarted.");
    } else if has_command_in_path("systemctl") && home != get_home_dir() {
        // In isolated or test environments with custom home prefix, skip live systemctl invocation
    } else {
        println!("   ⚠️  systemctl not found in PATH; skipping service activation.");
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn install_linux_systemd(_home: &Path) -> Result<()> {
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
    #[cfg(target_os = "linux")]
    fn test_detect_screen_geometry_bounds() {
        let (w, h) = detect_screen_geometry();
        assert!(w >= 640);
        assert!(h >= 480);
    }

    #[test]
    fn test_hud_kwin_rule_builder() {
        let rules = hud_kwin_rule();
        let map: std::collections::HashMap<&str, &str> =
            rules.iter().map(|(k, v)| (*k, v.as_str())).collect();

        // Primary fix: Alt+Tab task switcher exclusion
        assert_eq!(map.get("skipswitcher"), Some(&"true"));
        assert_eq!(map.get("skipswitcherrule"), Some(&"2"));

        // Taskbar and pager exclusion
        assert_eq!(map.get("skiptaskbar"), Some(&"true"));
        assert_eq!(map.get("skiptaskbarrule"), Some(&"2"));
        assert_eq!(map.get("skippager"), Some(&"true"));
        assert_eq!(map.get("skippagerrule"), Some(&"2"));

        // Window behavior
        assert_eq!(map.get("above"), Some(&"true"));
        assert_eq!(map.get("aboverule"), Some(&"2"));
        assert_eq!(map.get("noborder"), Some(&"true"));
        assert_eq!(map.get("noborderrule"), Some(&"2"));

        // Window identification matching
        assert_eq!(map.get("wmclass"), Some(&"net.local.openwhisper.hud"));
        assert_eq!(map.get("wmclassmatch"), Some(&"1"));
        assert_eq!(map.get("wmclasscomplete"), Some(&"false"));
        assert_eq!(map.get("Description"), Some(&"OpenWhisper Floating HUD"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_kwin_rules_readback_verifier_failure_on_missing_property() {
        if find_kwriteconfig().is_none() || find_kreadconfig().is_none() {
            return;
        }
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let temp_file = temp_dir.path().join("kwinrulesrc");
        let _ = std::fs::File::create(&temp_file);
        let file_str = temp_file.to_str().expect("valid utf8");

        // Write incomplete rule (missing skipswitcher and skipswitcherrule)
        let incomplete_rules = [
            ("Description", "OpenWhisper Floating HUD"),
            ("wmclass", "net.local.openwhisper.hud"),
            ("above", "true"),
            ("aboverule", "2"),
            ("skiptaskbar", "true"),
            ("skiptaskbarrule", "2"),
        ];
        for (k, v) in incomplete_rules {
            write_kwin_config_key(file_str, "openwhisper_hud", k, v).unwrap();
        }
        write_kwin_config_key(file_str, "General", "rules", "openwhisper_hud").unwrap();

        let res = verify_kwin_rules(file_str);
        assert!(
            res.is_err(),
            "Verifier must report failure when skipswitcher is missing"
        );
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("skipswitcher") || err.contains("mismatch"),
            "Error message should mention the missing or mismatched property: {err}"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_kwin_rules_readback_verifier_success_on_complete_group() {
        if find_kwriteconfig().is_none() || find_kreadconfig().is_none() {
            return;
        }
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let temp_file = temp_dir.path().join("kwinrulesrc");
        let _ = std::fs::File::create(&temp_file);
        let file_str = temp_file.to_str().expect("valid utf8");

        // Write complete canonical rule set
        for (k, v) in hud_kwin_rule() {
            write_kwin_config_key(file_str, "openwhisper_hud", k, &v).unwrap();
        }
        write_kwin_config_key(file_str, "General", "rules", "rule1,openwhisper_hud,rule2").unwrap();

        let res = verify_kwin_rules(file_str);
        assert!(
            res.is_ok(),
            "Verifier must succeed for complete group: {:?}",
            res
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_repair_incomplete_group_and_verify() {
        if find_kwriteconfig().is_none() || find_kreadconfig().is_none() {
            return;
        }
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let temp_file = temp_dir.path().join("kwinrulesrc");
        let _ = std::fs::File::create(&temp_file);
        let file_str = temp_file.to_str().expect("valid utf8");

        // Incomplete group with only a subset of keys and missing [General] rule entry
        write_kwin_config_key(file_str, "openwhisper_hud", "above", "true").unwrap();
        write_kwin_config_key(file_str, "openwhisper_hud", "aboverule", "2").unwrap();

        assert!(verify_kwin_rules(file_str).is_err());

        repair_kwin_rules(file_str).expect("repair must succeed");

        assert!(verify_kwin_rules(file_str).is_ok());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_repair_defective_rules_file() {
        if find_kwriteconfig().is_none() || find_kreadconfig().is_none() {
            return;
        }
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let temp_file = temp_dir.path().join("kwinrulesrc");
        let _ = std::fs::File::create(&temp_file);
        let file_str = temp_file.to_str().expect("valid utf8");

        // Defective state observed on host:
        // [General] rules is empty, [openwhisper_hud] keeps only position/positionrule
        write_kwin_config_key(file_str, "General", "rules", "").unwrap();
        write_kwin_config_key(file_str, "openwhisper_hud", "position", "815,972").unwrap();
        write_kwin_config_key(file_str, "openwhisper_hud", "positionrule", "2").unwrap();

        // Must fail verification before repair
        assert!(verify_kwin_rules(file_str).is_err());

        // Run repair path
        repair_kwin_rules(file_str).expect("repair must succeed");

        // Must pass verification after repair
        assert!(verify_kwin_rules(file_str).is_ok());

        // Verify required properties
        let skipswitcher =
            read_kwin_config_key(file_str, "openwhisper_hud", "skipswitcher").unwrap();
        assert_eq!(skipswitcher, "true");
        let skipswitcherrule =
            read_kwin_config_key(file_str, "openwhisper_hud", "skipswitcherrule").unwrap();
        assert_eq!(skipswitcherrule, "2");

        let general_rules = read_kwin_config_key(file_str, "General", "rules").unwrap();
        assert!(
            general_rules
                .split(',')
                .any(|r| r.trim() == "openwhisper_hud"),
            "[General] rules must contain openwhisper_hud: {general_rules}"
        );

        // Pre-existing position property must be preserved
        let position = read_kwin_config_key(file_str, "openwhisper_hud", "position").unwrap();
        assert_eq!(position, "815,972");
    }

    #[test]
    fn test_setup_path_does_not_create_xdg_autostart() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let temp_home = temp_dir.path();
        let dummy_exe = temp_home.join("dummy_openwhisper");
        fs::write(&dummy_exe, b"dummy").unwrap();

        run_setup_for_home(&dummy_exe, temp_home).expect("setup must succeed");

        let autostart_desktop =
            temp_home.join(".config/autostart/net.local.openwhisper-ui.desktop");
        assert!(
            !autostart_desktop.exists(),
            "Setup path must not create XDG autostart entry: {}",
            autostart_desktop.display()
        );

        #[cfg(target_os = "linux")]
        {
            assert!(
                temp_home
                    .join(".config/systemd/user/openwhisper.service")
                    .exists(),
                "openwhisper.service should be installed"
            );
            assert!(
                temp_home
                    .join(".config/systemd/user/openwhisper-ui.service")
                    .exists(),
                "openwhisper-ui.service should be installed"
            );
        }
    }

    #[test]
    fn test_cleanup_legacy_autostart_removes_file_and_second_run_is_noop() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let temp_home = temp_dir.path();
        let autostart_dir = temp_home.join(".config/autostart");
        fs::create_dir_all(&autostart_dir).expect("create autostart dir");
        let autostart_file = autostart_dir.join("net.local.openwhisper-ui.desktop");
        fs::write(&autostart_file, "[Desktop Entry]\nExec=openwhisper ui\n")
            .expect("write legacy file");
        assert!(autostart_file.exists());

        // First run: cleans up legacy autostart file
        let removed = cleanup_legacy_autostart(temp_home).expect("cleanup succeeds");
        assert!(removed, "First run should indicate file was removed");
        assert!(
            !autostart_file.exists(),
            "Legacy autostart file should have been deleted"
        );

        // Second run: idempotent no-op
        let removed_again = cleanup_legacy_autostart(temp_home).expect("second run succeeds");
        assert!(
            !removed_again,
            "Second run should be a no-op returning false"
        );
        assert!(!autostart_file.exists());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_setup_path_removes_existing_legacy_autostart() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let temp_home = temp_dir.path();
        let autostart_dir = temp_home.join(".config/autostart");
        fs::create_dir_all(&autostart_dir).expect("create autostart dir");
        let autostart_file = autostart_dir.join("net.local.openwhisper-ui.desktop");
        fs::write(&autostart_file, "[Desktop Entry]\nExec=openwhisper ui\n")
            .expect("write legacy file");
        assert!(autostart_file.exists());

        let dummy_exe = temp_home.join("dummy_openwhisper");
        fs::write(&dummy_exe, b"dummy").unwrap();

        run_setup_for_home(&dummy_exe, temp_home).expect("setup must succeed");
        assert!(
            !autostart_file.exists(),
            "Setup path must remove pre-existing legacy autostart entry"
        );

        // A second run on the same temp home is idempotent and does not recreate it
        run_setup_for_home(&dummy_exe, temp_home).expect("second setup must succeed");
        assert!(
            !autostart_file.exists(),
            "Second setup run must not recreate autostart entry"
        );
    }

    #[test]
    fn test_install_binary_never_writes_outside_user_prefix() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let temp_home = temp_dir.path();
        let dummy_exe = temp_home.join("dummy_openwhisper");
        fs::write(&dummy_exe, b"dummy").unwrap();

        install_binary(&dummy_exe, temp_home).expect("install_binary succeeds");

        let installed_bin = temp_home.join(".local/bin/openwhisper");
        assert!(
            installed_bin.exists(),
            "Binary must be installed in user prefix"
        );
    }
}
