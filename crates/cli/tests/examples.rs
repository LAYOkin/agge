use serde_json::Value;
use std::{path::Path, process::Command};

#[test]
fn cli_can_run_one_named_case_from_a_suite_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/pulse_lock");
    let run = |case| {
        Command::new(env!("CARGO_BIN_EXE_agge"))
            .args(["test", "game.agge", "@replay.json", case])
            .current_dir(&root)
            .output()
            .unwrap()
    };
    let selected = run("wrong_direction");
    assert!(selected.status.success(), "{selected:?}");
    let report: Value = serde_json::from_slice(&selected.stdout).unwrap();
    assert_eq!(report["cases"], 1);
    assert_eq!(report["assertions"], 6);
    assert_eq!(report["ticks"], 221);
    let missing = run("absent");
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("unknown replay case \"absent\""));
}

#[test]
fn bundled_replays_pass_through_the_cli() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for (game, cases, assertions) in [
        ("orbit", None, 5),
        ("paddle", None, 3),
        ("relay", None, 7),
        ("switchback", None, 9),
        ("bankshot", None, 12),
        ("cinder", Some(4), 35),
        ("pulse_lock", Some(4), 23),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agge"))
            .args(["test", "game.agge", "@replay.json"])
            .current_dir(root.join(game))
            .output()
            .unwrap();
        assert!(output.status.success(), "{game}: {output:?}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["passed"], true, "{game}: {report}");
        if let Some(cases) = cases {
            assert_eq!(report["cases"], cases, "{game}: {report}");
            assert_eq!(report["assertions"], assertions, "{game}: {report}");
        } else {
            assert_eq!(
                report["report"]["assertions"], assertions,
                "{game}: {report}"
            );
        }
    }
}
