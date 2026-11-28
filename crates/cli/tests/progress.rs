use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn export_and_preview_portable_progress_from_a_migrated_replay() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("game.agge");
    let output = directory.path().join("data.json");
    std::fs::write(
        &project,
        concat!(
            "agge/1 {\"progress\":{\"id\":\"works\",\"version\":2,\"globals\":{\"stars\":0},",
            "\"migrations\":{\"1\":{\"rename\":{\"coins\":\"stars\"}}}}}\n",
            "room main {\"instances\":[[\"office\",\"office\",0,0]]}\n",
            "object office {\"script\":\"award\"}\n",
            "script award\ncreate:\n  global stars = global(stars) + 1\n  save\n---\n",
        ),
    )
    .unwrap();
    let invoke = |command: &str, spec: &Value, file: Option<&std::path::Path>| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agge"));
        child.arg(command).arg(&project).arg("-");
        if let Some(file) = file {
            child.arg(file);
        }
        let mut child = child
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "{spec}").unwrap();
        child.wait_with_output().unwrap()
    };
    let mut replay = json!({
        "ticks":0,
        "progress":{"id":"works","version":1,"values":{"coins":9}},
        "expect":[{"at":0,"field":"progress_saved","is":1}]
    });
    let run = invoke("progress", &replay, Some(&output));
    assert!(run.status.success(), "{run:?}");
    let bytes = std::fs::read(&output).unwrap();
    let saved: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        saved,
        json!({"id":"works","version":2,"values":{"stars":10.0}})
    );
    assert!(!invoke("progress", &replay, Some(&output)).status.success());
    assert_eq!(std::fs::read(&output).unwrap(), bytes);
    replay["progress_out"] = true.into();
    let preview = invoke("inspect", &replay, None);
    assert!(preview.status.success(), "{preview:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&preview.stdout).unwrap()["progress"],
        saved
    );
}
