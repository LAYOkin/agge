use serde_json::Value;
use std::{path::Path, process::Command};

#[test]
fn pulse_lock_suite_checks_timing_and_restart() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/pulse_lock");
    let output = Command::new(env!("CARGO_BIN_EXE_agge"))
        .args(["test", "game.agge", "@replay.json"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["passed"], true);
    assert_eq!(report["cases"], 4);
    assert_eq!(report["assertions"], 23);
}
