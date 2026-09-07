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

    let _ = fs::create_dir_all(&apps_icon_dir);
    let _ = fs::create_dir_all(&status_icon_dir);

    let _ = fs::write(apps_icon_dir.join("openwhisper.svg"), APP_ICON);
    let _ = fs::write(status_icon_dir.join("openwhisper-tray-idle.svg"), TRAY_IDLE_ICON);
    let _ = fs::write(status_icon_dir.join("openwhisper-tray-recording.svg"), TRAY_RECORDING_ICON);
    let _ = fs::write(status_icon_dir.join("openwhisper-tray-transcribing.svg"), TRAY_TRANSCRIBING_ICON);
    let _ = fs::write(status_icon_dir.join("openwhisper-tray-error.svg"), TRAY_ERROR_ICON);
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

    Ok(())
}

#[cfg(target_os = "linux")]
fn install_linux_systemd(home: &Path) -> Result<()> {
    println!("⚙️  Step 4: Configuring systemd user service...");
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
