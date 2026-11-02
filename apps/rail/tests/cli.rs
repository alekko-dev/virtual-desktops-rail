use std::process::Command;

#[test]
fn help_and_invalid_arguments_exit_without_starting_a_rail() {
    let exe = env!("CARGO_BIN_EXE_virtual-desktop-rail");
    let help = Command::new(exe).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--settings"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--experimental-integration on|off"));
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
