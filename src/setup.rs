use std::fs;
use std::path::PathBuf;
use anyhow::{Context, Result};

const APP_ICON: &str = include_str!("../assets/icons/openwhisper.svg");
const TRAY_IDLE_ICON: &str = include_str!("../assets/icons/openwhisper-tray-idle.svg");
const TRAY_RECORDING_ICON: &str = include_str!("../assets/icons/openwhisper-tray-recording.svg");
const TRAY_TRANSCRIBING_ICON: &str = include_str!("../assets/icons/openwhisper-tray-transcribing.svg");
const TRAY_ERROR_ICON: &str = include_str!("../assets/icons/openwhisper-tray-error.svg");
const SERVICE_UNIT: &str = include_str!("../systemd/openwhisper.service");
const TOGGLE_DESKTOP: &str = include_str!("../desktop/net.local.openwhisper.desktop");
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

    // 1. Binary Installation
    println!("📦 Step 1: Installing OpenWhisper binary...");
    let user_bin_dir = home.join(".local/bin");
    let _ = fs::create_dir_all(&user_bin_dir);
    let user_bin = user_bin_dir.join("openwhisper");
    let tmp_bin = user_bin_dir.join(".openwhisper.tmp");
    if fs::copy(&current_exe, &tmp_bin).is_ok() && fs::rename(&tmp_bin, &user_bin).is_ok() {
        println!("   ✓ Installed to {}", user_bin.display());
    } else if let Err(e) = fs::copy(&current_exe, &user_bin) {
        println!("   ⚠️  Could not copy to {}: {e}", user_bin.display());
    } else {
        println!("   ✓ Installed to {}", user_bin.display());
    }

    // Try /usr/local/bin if writable
    let usr_bin = PathBuf::from("/usr/local/bin/openwhisper");
    let tmp_usr = PathBuf::from("/usr/local/bin/.openwhisper.tmp");
    if fs::copy(&current_exe, &tmp_usr).is_ok() && fs::rename(&tmp_usr, &usr_bin).is_ok() {
        println!("   ✓ Installed to /usr/local/bin/openwhisper");
    }


    // 2. Install Scalable & Status Icons
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

    // 3. Install Desktop Applications
    println!("🖥️  Step 3: Registering desktop entries...");
    let app_dir = home.join(".local/share/applications");
    let _ = fs::create_dir_all(&app_dir);
    let _ = fs::write(app_dir.join("net.local.openwhisper.desktop"), TOGGLE_DESKTOP);
    let _ = fs::write(app_dir.join("net.local.openwhisper.settings.desktop"), SETTINGS_DESKTOP);
    println!("   ✓ Registered net.local.openwhisper.desktop (Dictation Toggle shortcut)");
    println!("   ✓ Registered net.local.openwhisper.settings.desktop (Settings GUI panel)");

    // Refresh desktop/icon database
    let _ = std::process::Command::new("kbuildsycoca6")
        .arg("--noincremental")
        .output();
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&app_dir)
        .output();
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .arg("-q")
        .arg("-t")
        .arg("-f")
        .arg(&icons_dir)
        .output();

    // 4. Install & Enable Systemd User Service
    println!("⚙️  Step 4: Configuring systemd user service...");
    let systemd_dir = home.join(".config/systemd/user");
    let _ = fs::create_dir_all(&systemd_dir);
    let _ = fs::write(systemd_dir.join("openwhisper.service"), SERVICE_UNIT);
    println!("   ✓ Installed {}", systemd_dir.join("openwhisper.service").display());

    let uid = std::env::var("UID").unwrap_or_else(|_| "1000".into());
    let _ = std::process::Command::new("systemctl")
        .arg("--user")
        .arg("daemon-reload")
        .env("XDG_RUNTIME_DIR", format!("/run/user/{}", uid))
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path=/run/user/{}/bus", uid))
        .output();

    let _ = std::process::Command::new("systemctl")
        .arg("--user")
        .arg("enable")
        .arg("openwhisper.service")
        .env("XDG_RUNTIME_DIR", format!("/run/user/{}", uid))
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path=/run/user/{}/bus", uid))
        .output();

    let _ = std::process::Command::new("systemctl")
        .arg("--user")
        .arg("restart")
        .arg("openwhisper.service")
        .env("XDG_RUNTIME_DIR", format!("/run/user/{}", uid))
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path=/run/user/{}/bus", uid))
        .output();
    println!("   ✓ systemd user service reloaded, enabled, and restarted.");

    println!("\n✅ OpenWhisper setup completed successfully!");
    Ok(())
}
