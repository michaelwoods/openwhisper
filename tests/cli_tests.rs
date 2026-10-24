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
