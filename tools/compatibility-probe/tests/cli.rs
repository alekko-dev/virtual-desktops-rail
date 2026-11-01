use serde_json::Value;
use std::process::Command;

const PROBE: &str = env!("CARGO_BIN_EXE_rail-compatibility-probe");

#[test]
fn invalid_arguments_and_help_have_distinct_exit_statuses() {
    let invalid = Command::new(PROBE)
        .args(["--observe-seconds", "0"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
    let help = Command::new(PROBE).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("not certification")
    );
}

#[test]
fn internal_worker_cannot_write_a_report() {
    let output = Command::new(PROBE)
        .args(["--worker", "--output", "must-not-exist.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("does not write files")
    );
}

#[test]
fn unsupported_host_is_rejected_in_both_parent_and_worker() {
    // Select this test using the registry, without invoking the shell adapter.
    let version = Command::new("reg.exe")
        .args([
            "query",
            "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
            "/v",
            "CurrentBuildNumber",
        ])
        .output()
        .unwrap();
    assert!(version.status.success());
    let text = String::from_utf8(version.stdout).unwrap();
    let build: u32 = text.split_whitespace().last().unwrap().parse().unwrap();
    if build >= 26100 {
        return;
    } // Newer-host shell queries are an interactive check.
    let output = Command::new(PROBE).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "unavailable");
    assert_eq!(report["os"]["build"], build);
    assert!(report["observation"].is_null());
    assert!(
        report["explanation"]
            .as_str()
            .unwrap()
            .contains("No shell COM calls")
    );
    let worker = Command::new(PROBE).arg("--worker").output().unwrap();
    assert!(worker.status.success());
    let result: Value = serde_json::from_slice(&worker.stdout).unwrap();
    assert!(
        result["Err"]
            .as_str()
            .unwrap()
            .contains("COM was not invoked")
    );
}
