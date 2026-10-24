use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutostartStatus {
    pub daemon_systemd_enabled: bool,
    pub daemon_systemd_active: bool,
    pub ui_systemd_enabled: bool,
    pub ui_systemd_active: bool,
    pub xdg_autostart_installed: bool,
    pub xdg_autostart_path: String,
}

fn get_home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string())))
}

fn autostart_desktop_path() -> PathBuf {
    get_home_dir().join(".config/autostart/net.local.openwhisper-ui.desktop")
}

pub fn status() -> AutostartStatus {
    let xdg_path = autostart_desktop_path();
    let xdg_installed = xdg_path.exists();

    #[cfg(target_os = "linux")]
    {
        let daemon_enabled = check_systemd_unit_state("is-enabled", "openwhisper.service");
        let daemon_active = check_systemd_unit_state("is-active", "openwhisper.service");
        let ui_enabled = check_systemd_unit_state("is-enabled", "openwhisper-ui.service");
        let ui_active = check_systemd_unit_state("is-active", "openwhisper-ui.service");

        AutostartStatus {
            daemon_systemd_enabled: daemon_enabled,
            daemon_systemd_active: daemon_active,
            ui_systemd_enabled: ui_enabled,
            ui_systemd_active: ui_active,
            xdg_autostart_installed: xdg_installed,
            xdg_autostart_path: xdg_path.to_string_lossy().to_string(),
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        AutostartStatus {
            daemon_systemd_enabled: false,
            daemon_systemd_active: false,
            ui_systemd_enabled: false,
            ui_systemd_active: false,
            xdg_autostart_installed: xdg_installed,
            xdg_autostart_path: xdg_path.to_string_lossy().to_string(),
        }
    }
}

pub fn is_enabled() -> bool {
    let s = status();
    s.daemon_systemd_enabled || s.ui_systemd_enabled
}

#[cfg(target_os = "linux")]
fn check_systemd_unit_state(verb: &str, unit: &str) -> bool {
    let output = std::process::Command::new("systemctl")
        .args(["--user", verb, unit])
        .output();

    if let Ok(out) = output {
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        text == "enabled" || text == "active"
    } else {
        false
    }
}

pub fn enable() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let home = get_home_dir();
        let systemd_dir = home.join(".config/systemd/user");
        let _ = fs::create_dir_all(&systemd_dir);

        // Ensure systemd units exist
        let daemon_unit = systemd_dir.join("openwhisper.service");
        let _ = fs::write(&daemon_unit, include_str!("../systemd/openwhisper.service"));

        let ui_unit = systemd_dir.join("openwhisper-ui.service");
        let _ = fs::write(&ui_unit, include_str!("../systemd/openwhisper-ui.service"));

        // Clean up legacy autostart entry if present; systemd user units own startup exclusively
        let autostart_path = autostart_desktop_path();
        if autostart_path.exists() {
            let _ = fs::remove_file(&autostart_path);
        }

        // Enable systemd user units
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();

        let _ = std::process::Command::new("systemctl")
            .args([
                "--user",
                "enable",
                "openwhisper.service",
                "openwhisper-ui.service",
            ])
            .output();

        // Start / restart services in active session
        let _ = std::process::Command::new("systemctl")
            .args([
                "--user",
                "restart",
                "openwhisper.service",
                "openwhisper-ui.service",
            ])
            .output();
    }

    #[cfg(not(target_os = "linux"))]
    {
        let autostart_path = autostart_desktop_path();
        if autostart_path.exists() {
            let _ = fs::remove_file(&autostart_path);
        }
    }

    Ok(())
}

pub fn disable() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("systemctl")
            .args([
                "--user",
                "disable",
                "openwhisper.service",
                "openwhisper-ui.service",
            ])
            .output();

        let _ = std::process::Command::new("systemctl")
            .args([
                "--user",
                "stop",
                "openwhisper-ui.service",
                "app-net.local.openwhisper\\x2dui@autostart.service",
            ])
            .output();
    }

    let autostart_path = autostart_desktop_path();
    if autostart_path.exists() {
        let _ = fs::remove_file(autostart_path);
    }

    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();
    }

    Ok(())
}

pub fn format_status_report(st: &AutostartStatus) -> String {
    let mut out = String::new();
    out.push_str("\n🚀 OpenWhisper Autostart Status\n");
    out.push_str("==============================\n");

    let daemon_en_str = if st.daemon_systemd_enabled {
        "\x1b[32mEnabled\x1b[0m"
    } else {
        "\x1b[33mDisabled\x1b[0m"
    };
    let daemon_act_str = if st.daemon_systemd_active {
        "\x1b[32mActive (Running)\x1b[0m"
    } else {
        "\x1b[37mInactive\x1b[0m"
    };
    out.push_str(&format!(
        " • Headless Daemon (systemd): {} | {}\n",
        daemon_en_str, daemon_act_str
    ));

    let ui_en_str = if st.ui_systemd_enabled {
        "\x1b[32mEnabled\x1b[0m"
    } else {
        "\x1b[33mDisabled\x1b[0m"
    };
    let ui_act_str = if st.ui_systemd_active {
        "\x1b[32mActive (Running)\x1b[0m"
    } else {
        "\x1b[37mInactive\x1b[0m"
    };
    out.push_str(&format!(
        " • UI Service (systemd):      {} | {}\n",
        ui_en_str, ui_act_str
    ));

    out.push_str("==============================\n");
    if st.daemon_systemd_enabled || st.ui_systemd_enabled {
        out.push_str("Launch at startup is \x1b[1;32mENABLED\x1b[0m.\n");
    } else {
        out.push_str(
            "Launch at startup is \x1b[1;33mDISABLED\x1b[0m. Use `openwhisper autostart --enable` to activate.\n",
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_enabled_reports_systemd_only_state() {
        let st_xdg_only = AutostartStatus {
            daemon_systemd_enabled: false,
            daemon_systemd_active: false,
            ui_systemd_enabled: false,
            ui_systemd_active: false,
            xdg_autostart_installed: true,
            xdg_autostart_path: "/some/path/net.local.openwhisper-ui.desktop".to_string(),
        };

        let report = format_status_report(&st_xdg_only);
        assert!(
            report.contains("DISABLED"),
            "Autostart report must not count XDG desktop file as enabled: {report}"
        );
        assert!(
            !report.contains("XDG Autostart Desktop"),
            "Autostart report must be systemd-only: {report}"
        );

        let st_enabled = AutostartStatus {
            daemon_systemd_enabled: true,
            daemon_systemd_active: true,
            ui_systemd_enabled: true,
            ui_systemd_active: true,
            xdg_autostart_installed: false,
            xdg_autostart_path: String::new(),
        };
        let report_enabled = format_status_report(&st_enabled);
        assert!(
            report_enabled.contains("ENABLED"),
            "Autostart report must report enabled when systemd units are enabled: {report_enabled}"
        );
    }
}
