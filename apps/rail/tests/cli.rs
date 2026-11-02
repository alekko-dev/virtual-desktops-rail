use std::process::Command;

#[test]
fn version_is_local_and_machine_readable() {
    let exe = env!("CARGO_BIN_EXE_virtual-desktop-rail");
    let output = Command::new(exe).arg("--version").output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with(concat!(
        "virtual-desktop-rail ",
        env!("CARGO_PKG_VERSION"),
        " ("
    )));
    assert_eq!(text.lines().count(), 1);
    let output = Command::new(exe)
        .args(["--version", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let build: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(build["version"], env!("CARGO_PKG_VERSION"));
    assert!(build["commit"].is_string() || build["commit"].is_null());
    assert!(build["dirty"].is_boolean() || build["dirty"].is_null());
    assert_eq!(build["commit"].is_null(), build["dirty"].is_null());
}

#[test]
fn help_and_invalid_arguments_exit_without_starting_a_rail() {
    let exe = env!("CARGO_BIN_EXE_virtual-desktop-rail");
    let help = Command::new(exe).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--settings"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--experimental-integration on|off"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("no longer affects build eligibility"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("26100.2605"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--auto-restore on|off"));
    let invalid = Command::new(exe).arg("--unknown-option").output().unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    let invalid = Command::new(exe)
        .args(["--auto-restore", "maybe"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    let invalid = Command::new(exe)
        .args(["--experimental-integration", "maybe"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
}

#[test]
fn integration_worker_rejects_a_missing_parent_without_shell_work() {
    for args in [
        vec!["--integration-worker", "0"],
        vec!["--integration-worker", "0", "--experimental-integration"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_virtual-desktop-rail"))
            .args(args)
            .output()
            .unwrap();
        let frame: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(frame["Failure"].is_string());
        assert!(frame.get("Frame").is_none());
    }
}
