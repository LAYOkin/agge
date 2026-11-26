use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

#[test]
fn cli_resizes_and_reads_only_the_requested_map_area_and_placements() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("game.agge");
    std::fs::write(&file, concat!(
        "agge/1 {}\n",
        "room main {\"instances\":[[\"first\",\"pilot\",0,0],[\"wall\",\"block\",8,0],[\"last\",\"pilot\",16,0]]}\n",
        "object pilot {}\nobject block {}\n",
        "tilemap map {\"cell\":8,\"at\":[16,24],\"tiles\":{\"#\":[[255,255,255,255]],\"a\":[[0,0,0,255]]},\"rows\":[\"#..\",\".a#\"]}\n",
    )).unwrap();
    let run = |command: &str, payload: Value| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agge"))
            .arg(command)
            .arg(&file)
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        write!(child.stdin.take().unwrap(), "{payload}").unwrap();
        child.wait_with_output().unwrap()
    };
    let receipt = run(
        "edit",
        json!({"ops":[
            {"op":"resize_map","map":"map","size":[3,3],"offset":[-1,1],"tile":"#"}
        ]}),
    );
    assert!(receipt.status.success(), "{receipt:?}");
    let receipt: Value = serde_json::from_slice(&receipt.stdout).unwrap();
    let revision = &receipt["revision"];
    let result = run(
        "query",
        json!({"base":revision,"read":[
            {"get":"tilemap/map","area":[0,1,2,2]},
            {"placements":"main","object":"pilot","limit":1},
            {"get":"tilemap/map","area":[2,2,2,1]},
            {"get":"tilemap/map","path":"/at"}
        ]}),
    );
    assert!(result.status.success(), "{result:?}");
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["data"][0]["data"], json!(["..", "a#"]));
    assert_eq!(
        result["data"][1]["items"],
        json!([["first", "pilot", 0., 0.]])
    );
    assert_eq!(result["data"][1]["next"], 1);
    assert!(result["data"][2]["error"].is_string());
    assert_eq!(result["data"][3]["data"], json!([16, 24]));
    let next = run(
        "query",
        json!({"base":revision,"placements":"main","object":"pilot","offset":1,"limit":1}),
    );
    assert!(next.status.success(), "{next:?}");
    let next: Value = serde_json::from_slice(&next.stdout).unwrap();
    assert_eq!(next["items"], json!([["last", "pilot", 16., 0.]]));
    assert!(next.get("next").is_none());
    let stale = run("query", json!({"base":"old","get":"project"}));
    assert_eq!(stale.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&stale.stderr).contains("stale revision"));
}

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
        ("pixel_foundry", None, 6),
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

#[test]
fn failed_single_replays_and_guards_keep_reports_small() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("game.agge");
    std::fs::write(&file, "agge/1 {}\nroom main {}\n").unwrap();
    let args = folder.path().join("args.json");
    let run = |command, spec: Value| {
        std::fs::write(&args, spec.to_string()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_agge"))
            .arg(command)
            .arg(&file)
            .arg(format!("@{}", args.display()))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let replay = json!({
        "ticks":0,
        "expect":(0..12).map(|i| json!({"at":0,"field":"room","is":format!("wrong_{i}")})).collect::<Vec<_>>()
    });
    let tested = run("test", replay.clone());
    let guarded = run(
        "edit",
        json!({
            "ops":[{"op":"settings","fields":{"title":"changed"}}], "verify":[replay]
        }),
    );
    assert_eq!(guarded["committed"], false);
    for report in [&tested["report"], &guarded["reports"][0]] {
        assert_eq!(report["assertions"], 12);
        assert_eq!(report["failures"].as_array().unwrap().len(), 8);
        assert_eq!(report["failures"][7]["expected"], "wrong_7");
        assert_eq!(report["omitted_failures"], 4);
    }
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "agge/1 {}\nroom main {}\n"
    );
}
