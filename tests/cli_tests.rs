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
        "Unexpected status output: {}", combined
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
