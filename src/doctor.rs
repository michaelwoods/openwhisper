use anyhow::{Context, Result};
use hound::{SampleFormat, WavSpec, WavWriter};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::audio::AudioRecorder;
use crate::config::Config;
use crate::hotkey::ipc::{IpcCommand, send_ipc_command};
use crate::output::clipboard::has_command_in_path;
use crate::transcribe::TranscriptionClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub status: CheckStatus,
    pub summary: String,
    pub details: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub timestamp: String,
    pub overall_status: CheckStatus,
    pub checks: Vec<CheckResult>,
}

impl DoctorReport {
    pub fn new() -> Self {
        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            overall_status: CheckStatus::Pass,
            checks: Vec::new(),
        }
    }

    pub fn add(&mut self, check: CheckResult) {
        match check.status {
            CheckStatus::Fail => self.overall_status = CheckStatus::Fail,
            CheckStatus::Warn if self.overall_status != CheckStatus::Fail => {
                self.overall_status = CheckStatus::Warn;
            }
            _ => {}
        }
        self.checks.push(check);
    }
}

impl Default for DoctorReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs the full diagnostic suite and returns a structured DoctorReport.
pub async fn diagnose(config: &Config) -> DoctorReport {
    let mut report = DoctorReport::new();

    report.add(check_config(config));
    report.add(check_audio_hardware(config));
    report.add(check_stt_backend(config).await);
    report.add(check_wayland_and_input());
    report.add(check_daemon_and_ipc(config).await);
    report.add(check_desktop_integration());

    report
}

/// 1. Configuration file check
pub fn check_config(config: &Config) -> CheckResult {
    let config_path = Config::config_path();
    let mut details = vec![
        format!("Model: {}", config.model),
        format!("Server URL: {}", config.server_url),
        format!("Output Mode: {:?}", config.output_mode),
        format!(
            "Hotkey Mode: PTT / Toggle (threshold: {}ms)",
            config.ptt_threshold_ms
        ),
        format!(
            "Sound Feedback: {}",
            if config.sound_feedback {
                "Enabled"
            } else {
                "Disabled"
            }
        ),
        format!(
            "HUD Overlay: {}",
            if config.hud_enabled {
                "Enabled"
            } else {
                "Disabled"
            }
        ),
    ];

    if let Some(ref dir) = config.save_audio_dir {
        details.push(format!("Audio Dataset Directory: {}", dir));
    }

    match config_path {
        Ok(path) => {
            if path.exists() {
                CheckResult {
                    name: "Configuration".to_string(),
                    status: CheckStatus::Pass,
                    summary: format!("Loaded: {}", path.display()),
                    details,
                    remediation: None,
                }
            } else {
                CheckResult {
                    name: "Configuration".to_string(),
                    status: CheckStatus::Warn,
                    summary: format!(
                        "Using default configuration (file not found at {})",
                        path.display()
                    ),
                    details,
                    remediation: Some(
                        "Create a custom configuration file with: openwhisper init-config"
                            .to_string(),
                    ),
                }
            }
        }
        Err(e) => CheckResult {
            name: "Configuration".to_string(),
            status: CheckStatus::Warn,
            summary: format!("Could not resolve user config path: {e}"),
            details,
            remediation: Some(
                "Ensure $HOME or standard XDG directories are configured".to_string(),
            ),
        },
    }
}

/// 2. Audio input hardware and capture probe
pub fn check_audio_hardware(config: &Config) -> CheckResult {
    let devices = match AudioRecorder::list_input_devices() {
        Ok(d) => d,
        Err(e) => {
            return CheckResult {
                name: "Audio Hardware".to_string(),
                status: CheckStatus::Fail,
                summary: format!("Failed to enumerate audio input devices: {e}"),
                details: vec![
                    "Cannot access audio subsystem (ALSA / PipeWire / PulseAudio)".to_string(),
                ],
                remediation: Some(
                    "Check system audio service (e.g. systemctl --user status pipewire)"
                        .to_string(),
                ),
            };
        }
    };

    if devices.is_empty() {
        return CheckResult {
            name: "Audio Hardware".to_string(),
            status: CheckStatus::Fail,
            summary: "No audio input devices (microphones) detected".to_string(),
            details: vec!["System reports 0 available capture devices".to_string()],
            remediation: Some("Connect a microphone or verify audio permissions".to_string()),
        };
    }

    let active_name = config.audio_device.as_deref().unwrap_or("(System Default)");
    let mut details = vec![
        format!("Selected device: {}", active_name),
        format!(
            "Available devices: {} detected ({})",
            devices.len(),
            devices.join(", ")
        ),
    ];

    // Probe 150ms capture stream
    let level_test = AudioRecorder::test_input_levels(
        config.audio_device.clone(),
        Duration::from_millis(150),
        None,
        |_, _| {},
    );

    match level_test {
        Ok(peak) => {
            details.push(format!(
                "Capture stream probe: Succeeded (peak RMS level: {:.3})",
                peak
            ));
            CheckResult {
                name: "Audio Hardware".to_string(),
                status: CheckStatus::Pass,
                summary: format!("Audio capture stream verified on {}", active_name),
                details,
                remediation: None,
            }
        }
        Err(e) => {
            details.push(format!("Capture stream probe failed: {e}"));
            CheckResult {
                name: "Audio Hardware".to_string(),
                status: CheckStatus::Warn,
                summary: format!("Device list queried, but opening stream on {} failed: {e}", active_name),
                details,
                remediation: Some("Ensure no exclusive access locks the device and test with: openwhisper record --duration 2".to_string()),
            }
        }
    }
}

/// 3. STT Inference backend probe
pub async fn check_stt_backend(config: &Config) -> CheckResult {
    let mut details = vec![
        format!("Endpoint URL: {}", config.server_url),
        format!("Target Model: {}", config.model),
        format!(
            "API Key: {}",
            if config.api_key.is_some() {
                "Configured"
            } else {
                "None / Local"
            }
        ),
    ];

    // Generate synthetic 16kHz test tone (0.5 sec)
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut buf = Cursor::new(Vec::new());
    if let Ok(mut writer) = WavWriter::new(&mut buf, spec) {
        for i in 0..8000 {
            let sample = (0.05
                * (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16000.0).sin()
                * 32767.0) as i16;
            let _ = writer.write_sample(sample);
        }
        let _ = writer.finalize();
    }
    let wav_bytes = buf.into_inner();

    let client = TranscriptionClient::new(config);
    let start = Instant::now();

    match client.transcribe(wav_bytes).await {
        Ok(transcription) => {
            let latency = start.elapsed();
            details.push(format!(
                "Round-trip latency: {:.1}ms",
                latency.as_secs_f64() * 1000.0
            ));
            details.push(format!("Probe response: {:?}", transcription.trim()));

            CheckResult {
                name: "STT Inference Backend".to_string(),
                status: CheckStatus::Pass,
                summary: format!(
                    "Endpoint responsive ({:.0}ms) at {}",
                    latency.as_secs_f64() * 1000.0,
                    config.server_url
                ),
                details,
                remediation: None,
            }
        }
        Err(err) => {
            let err_str = err.to_string();
            let (status, summary, remediation) = if err_str.contains("Connection refused")
                || err_str.contains("Failed to connect")
            {
                (
                    CheckStatus::Fail,
                    format!("Connection refused to {}", config.server_url),
                    Some("Ensure your local inference server (OVMS, Whisper.cpp, vLLM, Ollama) is running on the specified port. Test with: openwhisper test-ovms".to_string()),
                )
            } else if err_str.contains("401") || err_str.contains("403") {
                (
                    CheckStatus::Fail,
                    "Authentication failed (HTTP 401/403)".to_string(),
                    Some("Verify the api_key in ~/.config/openwhisper/config.toml or export OPENAI_API_KEY".to_string()),
                )
            } else if err_str.contains("404") {
                (
                    CheckStatus::Fail,
                    format!(
                        "Model or endpoint path not found (HTTP 404) on {}",
                        config.server_url
                    ),
                    Some(format!(
                        "Verify that model '{}' is loaded on the inference server",
                        config.model
                    )),
                )
            } else {
                (
                    CheckStatus::Warn,
                    format!("Inference probe returned error: {}", err_str),
                    Some(
                        "Check server logs or test endpoint with: openwhisper test-ovms"
                            .to_string(),
                    ),
                )
            };

            details.push(format!("Error message: {}", err_str));
            CheckResult {
                name: "STT Inference Backend".to_string(),
                status,
                summary,
                details,
                remediation,
            }
        }
    }
}

/// 4. Wayland and input permissions
pub fn check_wayland_and_input() -> CheckResult {
    let mut details = Vec::new();
    let mut warnings = Vec::new();

    // Check Wayland environment
    if let Ok(wayland_disp) = std::env::var("WAYLAND_DISPLAY") {
        details.push(format!("Wayland Display: {}", wayland_disp));
    } else if let Ok(disp) = std::env::var("DISPLAY") {
        details.push(format!(
            "X11 Display: {} (Running under X11 / XWayland)",
            disp
        ));
    } else {
        warnings.push("No WAYLAND_DISPLAY or DISPLAY environment variable found");
    }

    // Check /dev/uinput
    #[cfg(target_os = "linux")]
    let uinput_ok = {
        let uinput_path = Path::new("/dev/uinput");
        if !uinput_path.exists() {
            warnings.push("/dev/uinput does not exist (kernel module 'uinput' may not be loaded)");
            false
        } else {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(uinput_path)
            {
                Ok(_) => {
                    details.push(
                        "/dev/uinput: Read/write access verified (virtual keyboard enabled)"
                            .to_string(),
                    );
                    true
                }
                Err(e) => {
                    warnings.push(&format!("/dev/uinput permission denied ({e})"));
                    false
                }
            }
        }
    };

    #[cfg(not(target_os = "linux"))]
    let uinput_ok = true;

    // Check clipboard tools
    let has_wl_copy = has_command_in_path("wl-copy");
    let has_wl_paste = has_command_in_path("wl-paste");
    let has_xclip = has_command_in_path("xclip");
    details.push(format!(
        "Clipboard utilities: wl-copy ({}), wl-paste ({}), xclip ({})",
        if has_wl_copy {
            "available"
        } else {
            "not found"
        },
        if has_wl_paste {
            "available"
        } else {
            "not found"
        },
        if has_xclip { "available" } else { "not found" },
    ));

    // Check typing tools
    let has_wtype = has_command_in_path("wtype");
    let has_ydotool = has_command_in_path("ydotool");
    details.push(format!(
        "Virtual typing fallback: wtype ({}), ydotool ({})",
        if has_wtype { "available" } else { "not found" },
        if has_ydotool {
            "available"
        } else {
            "not found"
        },
    ));

    if uinput_ok {
        CheckResult {
            name: "Virtual Input & Wayland".to_string(),
            status: CheckStatus::Pass,
            summary: "Direct /dev/uinput text injection and clipboard tools available".to_string(),
            details,
            remediation: None,
        }
    } else {
        CheckResult {
            name: "Virtual Input & Wayland".to_string(),
            status: CheckStatus::Warn,
            summary: "/dev/uinput is not writable; falling back to clipboard-only insertion".to_string(),
            details,
            remediation: Some(
                "Grant uinput access by adding your user to the input group or creating a udev rule:\n  sudo usermod -aG input $USER\n  echo 'KERNEL==\"uinput\", SUBSYSTEM==\"misc\", TAG+=\"uaccess\"' | sudo tee /etc/udev/rules.d/70-uinput.rules && sudo udevadm trigger".to_string(),
            ),
        }
    }
}

/// 5. Background Daemon & IPC Socket check
pub async fn check_daemon_and_ipc(config: &Config) -> CheckResult {
    let mut details = vec![format!("Socket path: {}", config.socket_path)];
    let autostart_st = crate::autostart::status();
    details.push(format!(
        "Autostart status: Daemon systemd: (enabled={}, active={}), UI systemd: (enabled={}, active={}), XDG autostart: (installed={})",
        autostart_st.daemon_systemd_enabled,
        autostart_st.daemon_systemd_active,
        autostart_st.ui_systemd_enabled,
        autostart_st.ui_systemd_active,
        autostart_st.xdg_autostart_installed,
    ));

    match send_ipc_command(&config.socket_path, IpcCommand::Status).await {
        Ok(resp) => {
            details.push(format!("IPC Response: {} ({})", resp.status, resp.message));
            CheckResult {
                name: "Daemon & IPC Service".to_string(),
                status: CheckStatus::Pass,
                summary: format!(
                    "OpenWhisper daemon is running and responsive on {}",
                    config.socket_path
                ),
                details,
                remediation: None,
            }
        }
        Err(e) => {
            details.push(format!("Socket connection probe: {e}"));

            // Check systemd user service status if available
            #[cfg(target_os = "linux")]
            let service_status = std::process::Command::new("systemctl")
                .args(["--user", "is-active", "openwhisper.service"])
                .output()
                .ok()
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());

            #[cfg(not(target_os = "linux"))]
            let service_status: Option<String> = None;

            if let Some(status_str) = service_status {
                details.push(format!(
                    "systemd user service 'openwhisper.service': {}",
                    status_str
                ));
            }

            CheckResult {
                name: "Daemon & IPC Service".to_string(),
                status: CheckStatus::Warn,
                summary: "OpenWhisper background daemon is not running".to_string(),
                details,
                remediation: Some("Start the daemon with: openwhisper (or enable service: systemctl --user start openwhisper.service)".to_string()),
            }
        }
    }
}

/// 6. Desktop integration, icons, and shortcuts
pub fn check_desktop_integration() -> CheckResult {
    let home = directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        });

    let icon_path = home.join(".local/share/icons/hicolor/scalable/apps/openwhisper.svg");
    let desktop_path = home.join(".local/share/applications/net.local.openwhisper.desktop");
    let mut details = Vec::new();
    let mut missing = Vec::new();

    if icon_path.exists() {
        details.push(format!(
            "Application icon: Found at {}",
            icon_path.display()
        ));
    } else {
        missing.push("Scalable icon missing in ~/.local/share/icons/hicolor/scalable/apps");
    }

    if desktop_path.exists() {
        details.push(format!(
            "Desktop entry: Found at {}",
            desktop_path.display()
        ));
    } else {
        missing.push("Desktop entry missing in ~/.local/share/applications");
    }

    let ui_desktop_path = home.join(".local/share/applications/net.local.openwhisper-ui.desktop");
    if ui_desktop_path.exists() {
        details.push(format!(
            "UI service desktop entry: Found at {}",
            ui_desktop_path.display()
        ));
    }

    let autostart_desktop = home.join(".config/autostart/net.local.openwhisper-ui.desktop");
    if autostart_desktop.exists() {
        details.push(format!(
            "XDG Autostart entry: Active at {}",
            autostart_desktop.display()
        ));
    }

    // Check KWin rules on KDE
    let kwin_rules = home.join(".config/kwinrulesrc");
    if let Ok(content) = std::fs::read_to_string(&kwin_rules) {
        if content.contains("openwhisper") {
            details.push("KDE Plasma KWin rules: HUD positioning rule detected".to_string());
        } else {
            details.push(
                "KDE Plasma KWin rules: kwinrulesrc exists (HUD rule will sync automatically)"
                    .to_string(),
            );
        }
    }

    // Check hardware evdev access
    #[cfg(target_os = "linux")]
    {
        let evdev_devices = evdev::enumerate().collect::<Vec<_>>();
        details.push(format!(
            "Evdev hardware input devices: {} accessible",
            evdev_devices.len()
        ));
    }

    if missing.is_empty() {
        CheckResult {
            name: "Desktop Integration".to_string(),
            status: CheckStatus::Pass,
            summary: "Icons, desktop shortcuts, and window rules verified".to_string(),
            details,
            remediation: None,
        }
    } else {
        CheckResult {
            name: "Desktop Integration".to_string(),
            status: CheckStatus::Warn,
            summary: format!(
                "Some desktop assets are not installed ({})",
                missing.join(", ")
            ),
            details,
            remediation: Some(
                "Deploy all icons, desktop entries, and systemd units with: openwhisper setup"
                    .to_string(),
            ),
        }
    }
}

/// Formats the DoctorReport as a colorized terminal dashboard.
pub fn format_terminal_report(report: &DoctorReport) -> String {
    let mut out = String::new();
    out.push_str("\n🩺 OpenWhisper System Diagnostics\n");
    out.push_str("==================================\n\n");

    let mut pass_count = 0;
    let mut warn_count = 0;
    let mut fail_count = 0;
    let mut remediations = Vec::new();

    for check in &report.checks {
        let (icon, prefix) = match check.status {
            CheckStatus::Pass => {
                pass_count += 1;
                ("\x1b[32m✓\x1b[0m", "\x1b[1;32m[Pass]\x1b[0m")
            }
            CheckStatus::Warn => {
                warn_count += 1;
                ("\x1b[33m⚠️\x1b[0m", "\x1b[1;33m[Warn]\x1b[0m")
            }
            CheckStatus::Fail => {
                fail_count += 1;
                ("\x1b[31m✗\x1b[0m", "\x1b[1;31m[Fail]\x1b[0m")
            }
        };

        out.push_str(&format!(
            "{} {} \x1b[1m{}\x1b[0m: {}\n",
            icon, prefix, check.name, check.summary
        ));
        for detail in &check.details {
            out.push_str(&format!("   • {}\n", detail));
        }
        if let Some(ref rem) = check.remediation {
            remediations.push((check.name.clone(), rem.clone()));
        }
        out.push('\n');
    }

    out.push_str("==================================\n");
    let summary_line = format!(
        "Summary: {} passed, {} warnings, {} failures.\n",
        pass_count, warn_count, fail_count
    );
    out.push_str(&summary_line);

    if !remediations.is_empty() {
        out.push_str("\n💡 Suggested Actions:\n");
        for (name, rem) in remediations {
            out.push_str(&format!(
                " • \x1b[1m{}\x1b[0m:\n   {}\n",
                name,
                rem.replace('\n', "\n   ")
            ));
        }
    }

    if fail_count == 0 && warn_count == 0 {
        out.push_str("\n\x1b[32m✨ All system diagnostics passed! OpenWhisper is fully operational.\x1b[0m\n");
    } else if fail_count == 0 {
        out.push_str("\n\x1b[33m⚠️ OpenWhisper is operational with minor warnings.\x1b[0m\n");
    } else {
        out.push_str("\n\x1b[31m❌ OpenWhisper encountered issues that may prevent speech dictation.\x1b[0m\n");
    }

    out
}

/// Executes the doctor command from the CLI.
pub async fn run_doctor(config: &Config, json: bool) -> Result<()> {
    let report = diagnose(config).await;

    if json {
        let json_str = serde_json::to_string_pretty(&report)
            .context("Failed to serialize diagnostic report to JSON")?;
        println!("{}", json_str);
    } else {
        print!("{}", format_terminal_report(&report));
    }

    if report.overall_status == CheckStatus::Fail {
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doctor_report_status_aggregation() {
        let mut report = DoctorReport::new();
        assert_eq!(report.overall_status, CheckStatus::Pass);

        report.add(CheckResult {
            name: "PassCheck".to_string(),
            status: CheckStatus::Pass,
            summary: "All good".to_string(),
            details: vec![],
            remediation: None,
        });
        assert_eq!(report.overall_status, CheckStatus::Pass);

        report.add(CheckResult {
            name: "WarnCheck".to_string(),
            status: CheckStatus::Warn,
            summary: "Minor issue".to_string(),
            details: vec![],
            remediation: Some("Fix it".to_string()),
        });
        assert_eq!(report.overall_status, CheckStatus::Warn);

        report.add(CheckResult {
            name: "FailCheck".to_string(),
            status: CheckStatus::Fail,
            summary: "Major issue".to_string(),
            details: vec![],
            remediation: Some("Must fix".to_string()),
        });
        assert_eq!(report.overall_status, CheckStatus::Fail);
    }

    #[test]
    fn test_doctor_terminal_formatting() {
        let mut report = DoctorReport::new();
        report.add(CheckResult {
            name: "TestCheck".to_string(),
            status: CheckStatus::Pass,
            summary: "Test summary".to_string(),
            details: vec!["Detail A".to_string(), "Detail B".to_string()],
            remediation: None,
        });

        let formatted = format_terminal_report(&report);
        assert!(formatted.contains("OpenWhisper System Diagnostics"));
        assert!(formatted.contains("TestCheck"));
        assert!(formatted.contains("Test summary"));
        assert!(formatted.contains("Detail A"));
        assert!(formatted.contains("1 passed"));
    }

    #[test]
    fn test_check_config() {
        let config = Config::default();
        let res = check_config(&config);
        assert_eq!(res.name, "Configuration");
        assert!(res.details.iter().any(|d| d.contains("Model:")));
        assert!(res.details.iter().any(|d| d.contains("Server URL:")));
    }

    #[test]
    fn test_check_desktop_integration() {
        let res = check_desktop_integration();
        assert_eq!(res.name, "Desktop Integration");
    }
}
