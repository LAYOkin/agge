use serde_json::{Value, json};
use std::{fs, process::Command};

#[test]
fn export_preserves_the_revision_of_a_noncanonical_project() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("original.agge");
    let output = directory.path().join("bundle");
    let player = directory.path().join("player");
    let agge = env!("CARGO_BIN_EXE_agge");
    let text = b"agge/1 {  \"title\" : \"Export demo\" }\n\nroom main {}\n";
    fs::write(&source, text).unwrap();
    fs::write(&player, b"test player").unwrap();

    let checkout = Command::new(agge)
        .arg("query")
        .arg(&source)
        .output()
        .unwrap();
    assert!(checkout.status.success(), "{checkout:?}");
    let revision = serde_json::from_slice::<Value>(&checkout.stdout).unwrap()["revision"].clone();
    let export = Command::new(agge)
        .arg("export")
        .args([&source, &output, &player])
        .output()
        .unwrap();
    assert!(export.status.success(), "{export:?}");
    let receipt: Value = serde_json::from_slice(&export.stdout).unwrap();
    let bundled_project = output.join("game.agge");
    assert_eq!(fs::read(&bundled_project).unwrap(), text);
    assert_eq!(receipt["revision"], revision);

    let replay = Command::new(agge)
        .arg("test")
        .arg(&bundled_project)
        .arg(json!({"ticks":0,"base":revision}).to_string())
        .output()
        .unwrap();
    assert!(replay.status.success(), "{replay:?}");
}

// The raw-byte fixture needs filenames that aren't valid UTF-8. macOS
// filesystems such as APFS don't accept those names.
#[cfg(target_os = "linux")]
#[test]
fn cli_accepts_native_paths_for_projects_and_outputs() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join(OsStr::from_bytes(b"game-\xff.agge"));
    let image = directory.path().join(OsStr::from_bytes(b"image-\xff.png"));
    let player = directory.path().join(OsStr::from_bytes(b"player-\xff"));
    let bundle = directory.path().join(OsStr::from_bytes(b"bundle-\xff"));
    let agge = env!("CARGO_BIN_EXE_agge");

    let init = Command::new(agge)
        .arg("init")
        .arg(&source)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let capture = Command::new(agge)
        .arg("capture")
        .arg(&source)
        .arg(r#"{"ticks":0,"crop":[0,0,1,1]}"#)
        .arg(&image)
        .output()
        .unwrap();
    assert!(capture.status.success(), "{capture:?}");
    assert!(fs::read(&image).unwrap().starts_with(b"\x89PNG"));

    fs::write(&player, b"test player").unwrap();
    let export = Command::new(agge)
        .arg("export")
        .args([&source, &bundle, &player])
        .output()
        .unwrap();
    assert!(export.status.success(), "{export:?}");
    assert_eq!(
        fs::read(bundle.join("game.agge")).unwrap(),
        fs::read(source).unwrap()
    );
    assert_eq!(fs::read(bundle.join("play")).unwrap(), b"test player");
}
