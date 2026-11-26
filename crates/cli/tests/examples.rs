use serde_json::Value;
use std::{path::Path, process::Command};

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
