use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_cli_help() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: openwhisper"))
        .stdout(predicate::str::contains("daemon"))
        .stdout(predicate::str::contains("history"));
}

#[test]
fn test_cli_version() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("openwhisper"));
}

#[test]
fn test_cli_history_help() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    cmd.args(["history", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Persistent transcription history"));
}

#[test]
fn test_cli_list_devices() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    cmd.arg("list-devices")
        .assert()
        .success()
        .stdout(predicate::str::contains("Available audio input devices:"));
}

#[test]
fn test_cli_status_runs_without_panic() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    // Verify `openwhisper status` handles both live daemon and offline daemon cleanly without panic
    let assert = cmd.arg("status").assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("daemon is not running")
            || combined.contains("command processed")
            || combined.contains("ok")
            || combined.contains("Status")
            || combined.contains("Error"),
        "Unexpected status output: {}",
        combined
    );
}

#[test]
fn test_cli_invalid_argument() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    cmd.arg("--nonexistent-flag-xyz")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument"));
}

#[test]
fn test_cli_doctor_text() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    let output = cmd.arg("doctor").output().expect("run doctor");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);
    assert!(combined.contains("OpenWhisper System Diagnostics"));
    assert!(combined.contains("Configuration"));
    assert!(combined.contains("Audio Hardware"));
    assert!(combined.contains("Summary:"));
}

#[test]
fn test_cli_doctor_json() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    let output = cmd
        .args(["doctor", "--json"])
        .output()
        .expect("run doctor json");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("doctor --json must output valid JSON");
    assert!(parsed.get("timestamp").is_some());
    assert!(parsed.get("overall_status").is_some());
    assert!(parsed.get("checks").is_some());
    let checks = parsed["checks"].as_array().expect("checks array");
    assert!(checks.iter().any(|c| c["name"] == "Configuration"));
    assert!(checks.iter().any(|c| c["name"] == "Audio Hardware"));
}

#[test]
fn test_cli_autostart_help() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    cmd.args(["autostart", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("startup"));
}

#[test]
fn test_cli_autostart_json() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    let output = cmd
        .args(["autostart", "--json"])
        .output()
        .expect("run autostart --json");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("autostart --json must output valid JSON");
    assert!(parsed.get("daemon_systemd_enabled").is_some());
    assert!(parsed.get("ui_systemd_enabled").is_some());
}

#[test]
fn test_cli_ui_help() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    cmd.args(["ui", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HUD"));
}

#[test]
fn test_cli_stop_help() {
    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    cmd.args(["stop", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Stop the running OpenWhisper daemon cleanly",
        ));
}

#[test]
fn test_cli_stop_without_daemon_reports_not_running() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let runtime_dir = temp.path().join("run");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
    std::fs::create_dir_all(&config_home).expect("create config dir");

    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    cmd.env("XDG_RUNTIME_DIR", &runtime_dir)
        .env("XDG_CONFIG_HOME", &config_home)
        .arg("stop")
        .assert()
        .failure()
        .stderr(predicate::str::contains("daemon is not running"));
}

#[test]
fn test_install_script_contains_no_sudo_execution() {
    let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/install.sh");
    let content = std::fs::read_to_string(&script_path).expect("read scripts/install.sh");

    for (line_no, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        assert!(
            !trimmed.contains("sudo"),
            "scripts/install.sh line {} contains unapproved sudo: {}",
            line_no + 1,
            line
        );
    }
}

#[test]
fn test_setup_rs_contains_no_sudo_execution() {
    let setup_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/setup.rs");
    let content = std::fs::read_to_string(&setup_path).expect("read src/setup.rs");

    for (line_no, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        assert!(
            !trimmed.contains("sudo"),
            "src/setup.rs line {} contains unapproved sudo: {}",
            line_no + 1,
            line
        );
    }
}

/// Builds a hermetic `openwhisper doctor` invocation: isolated HOME/XDG dirs, no display
/// server variables, and a config whose STT endpoint is a closed local port.
fn hermetic_doctor(args: &[&str]) -> (Command, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("tempdir");
    let config_dir = home.path().join("config").join("openwhisper");
    std::fs::create_dir_all(&config_dir).unwrap();
    // Reserve then release a port so connecting to it yields "Connection refused".
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    std::fs::write(
        config_dir.join("config.toml"),
        format!("server_url = \"http://127.0.0.1:{port}/v1/audio/transcriptions\"\n"),
    )
    .unwrap();
    let runtime = home.path().join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();

    let mut cmd = Command::cargo_bin("openwhisper").unwrap();
    cmd.env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .args(args);
    (cmd, home)
}

#[test]
fn test_cli_doctor_ci_json_schema_and_exit_zero() {
    let (mut cmd, _home) = hermetic_doctor(&["doctor", "--ci", "--json"]);
    let output = cmd.output().expect("run doctor --ci --json");
    assert_eq!(
        output.status.code(),
        Some(0),
        "doctor --ci must exit 0 on a bare host; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // stdout is reserved for exactly one JSON document.
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("doctor --ci --json must output valid JSON");

    let root = parsed.as_object().expect("root object");
    let mut root_keys: Vec<&str> = root.keys().map(String::as_str).collect();
    root_keys.sort_unstable();
    assert_eq!(root_keys, ["checks", "overall_status", "timestamp"]);
    assert!(root["timestamp"].as_str().is_some_and(|t| !t.is_empty()));
    let statuses = ["pass", "warn", "fail"];
    let overall = root["overall_status"].as_str().expect("overall_status str");
    assert!(statuses.contains(&overall));
    assert_ne!(overall, "fail");

    let checks = root["checks"].as_array().expect("checks array");
    assert!(!checks.is_empty(), "CI mode must not emit empty checks");
    let mut names = Vec::new();
    for check in checks {
        let obj = check.as_object().expect("check object");
        for key in obj.keys() {
            assert!(
                ["name", "status", "summary", "details", "remediation"].contains(&key.as_str()),
                "unexpected key {key}"
            );
        }
        let name = obj["name"].as_str().expect("name str");
        assert!(!name.is_empty());
        names.push(name.to_string());
        assert!(statuses.contains(&obj["status"].as_str().expect("status str")));
        assert!(obj["summary"].as_str().is_some_and(|s| !s.is_empty()));
        let details = obj["details"].as_array().expect("details array");
        assert!(
            !details.is_empty(),
            "{name}: real check logic must populate details"
        );
        assert!(details.iter().all(|d| d.is_string()));
        if let Some(rem) = obj.get("remediation") {
            assert!(rem.is_string());
        }
    }
    assert_eq!(
        names,
        [
            "Configuration",
            "Virtual Input & Wayland",
            "Daemon & IPC Service",
            "Desktop Integration"
        ]
    );

    // The retained daemon check ran for real against the (absent) socket.
    let daemon = checks
        .iter()
        .find(|c| c["name"] == "Daemon & IPC Service")
        .unwrap();
    assert_eq!(daemon["status"], "warn");
    // The configuration check really loaded the hermetic config file.
    let config_check = &checks[0];
    assert!(
        config_check["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap().contains("127.0.0.1")),
        "Configuration check must reflect the loaded config.toml"
    );
}

#[test]
fn test_cli_doctor_default_connection_refused_still_fails() {
    let (mut cmd, _home) = hermetic_doctor(&["doctor", "--json"]);
    let output = cmd.output().expect("run doctor --json");
    assert_eq!(
        output.status.code(),
        Some(1),
        "default doctor must exit 1 when the STT endpoint refuses connections"
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON on failure");
    assert_eq!(parsed["overall_status"], "fail");
    let stt = parsed["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "STT Inference Backend")
        .expect("default mode keeps the STT check");
    assert_eq!(stt["status"], "fail");
    assert!(
        stt["summary"]
            .as_str()
            .unwrap()
            .contains("Connection refused"),
        "unexpected STT summary: {}",
        stt["summary"]
    );
}
