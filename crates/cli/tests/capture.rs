use std::{fs, process::Command};

#[test]
fn capture_defaults_to_the_full_viewport() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("game.agge");
    let target = directory.path().join("capture.png");
    fs::write(&source, b"agge/1 {\"size\":[48,32]}\nroom main {}\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_agge"))
        .arg("capture")
        .arg(&source)
        .arg(r#"{"ticks":0}"#)
        .arg(&target)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["crop"], serde_json::json!([0, 0, 48, 32]));
    let png = fs::read(target).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&png[16..24], &[0, 0, 0, 48, 0, 0, 0, 32]);
}

#[test]
fn capture_on_failure_writes_the_image_and_preserves_the_failure_status() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("game.agge");
    let target = directory.path().join("capture.png");
    fs::write(&source, b"agge/1 {\"size\":[48,32]}\nroom main {}\n").unwrap();
    let mut args = serde_json::json!({
        "ticks":0,
        "expect":[{"at":0,"field":"room","is":"other"}],
    });
    let capture = |args: &serde_json::Value| {
        Command::new(env!("CARGO_BIN_EXE_agge"))
            .arg("capture")
            .arg(&source)
            .arg(args.to_string())
            .arg(&target)
            .output()
            .unwrap()
    };
    let skipped = capture(&args);
    assert_eq!(skipped.status.code(), Some(1), "{skipped:?}");
    assert!(!target.exists());

    args["capture_on_failure"] = serde_json::json!(true);
    let captured = capture(&args);
    assert_eq!(captured.status.code(), Some(1), "{captured:?}");
    let report: serde_json::Value = serde_json::from_slice(&captured.stdout).unwrap();
    assert_eq!(report["passed"], false);
    assert_eq!(report["report"]["failures"][0]["actual"], "main");
    assert!(fs::read(&target).unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn capture_refuses_to_overwrite_the_project_or_an_alias() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("game.agge");
    let hardlink = directory.path().join("hardlink.png");
    let contents = b"agge/1 {}\nroom main {}\n";
    fs::write(&source, contents).unwrap();
    fs::hard_link(&source, &hardlink).unwrap();
    #[cfg(unix)]
    let symlink = {
        let path = directory.path().join("symlink.png");
        std::os::unix::fs::symlink(&source, &path).unwrap();
        path
    };
    let targets = [
        &source,
        &hardlink,
        #[cfg(unix)]
        &symlink,
    ];
    for target in targets {
        let result = Command::new(env!("CARGO_BIN_EXE_agge"))
            .arg("capture")
            .arg(&source)
            .arg(r#"{"ticks":0,"crop":[0,0,1,1]}"#)
            .arg(target)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2), "{result:?}");
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .contains("capture output would overwrite the project")
        );
        assert_eq!(fs::read(&source).unwrap(), contents);
    }
}

#[test]
fn capture_loads_visible_atlases_and_ignores_unrelated_media() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("game.agge");
    let target = directory.path().join("capture.png");
    fs::write(
        &source,
        r#"agge/1 {"size":[48,32]}
room main {"maps":["floor"]}
sprite terrain {"file":"absent.png"}
audio distant {"file":"unrelated.wav"}
tilemap floor {"cell":16,"atlas":"terrain","tiles":{"t":[0]},"rows":["..t"]}
"#,
    )
    .unwrap();
    let agge = env!("CARGO_BIN_EXE_agge");
    let capture = |args: &str| {
        Command::new(agge)
            .arg("capture")
            .arg(&source)
            .arg(args)
            .arg(&target)
            .output()
            .unwrap()
    };
    let crop = capture(r#"{"ticks":0,"crop":[0,0,16,16]}"#);
    assert!(crop.status.success(), "{crop:?}");
    let visible = capture(r#"{"ticks":0,"crop":[32,0,16,16]}"#);
    assert_eq!(visible.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&visible.stderr).contains("sprite/terrain"));
    let check = Command::new(agge)
        .arg("check")
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1));
}

#[test]
fn asset_guard_rejects_incomplete_media_without_taking_the_candidate_live() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("game.agge");
    let before = b"agge/1 {}\nroom main {}\n";
    fs::write(&source, before).unwrap();
    let store = agge_core::store::Store::new(&source);
    let rev = store.load().unwrap().revision;
    let request = serde_json::json!({"base":rev,"verify_assets":true,"ops":[
        {"op":"put","kind":"sprite","id":"card","data":{"file":"missing.png"}}
    ]});
    let edit = Command::new(env!("CARGO_BIN_EXE_agge"))
        .arg("edit")
        .arg(&source)
        .arg(request.to_string())
        .output()
        .unwrap();
    assert_eq!(edit.status.code(), Some(1), "{edit:?}");
    let response: serde_json::Value = serde_json::from_slice(&edit.stdout).unwrap();
    assert_eq!(response["committed"], false);
    assert!(
        response["asset_error"]
            .as_str()
            .unwrap()
            .starts_with("sprite/card")
    );
    assert_eq!(fs::read(&source).unwrap(), before);
    assert!(!directory.path().join("game.agge.history").exists());
}
