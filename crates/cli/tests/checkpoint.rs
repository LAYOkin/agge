use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn cli_saves_and_branches_without_replaying_the_prefix() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let game = root.join("game.agge");
    let save = root.join("branch.aggesave");
    let source = "agge/1 {}\nroom main {\"instances\":[[\"hero\",\"hero\",0,0]]}\nobject hero {\"script\":\"move\"}\nscript move\nstep:\n  move 2, 0\n---\n";
    std::fs::write(&game, source).unwrap();
    let run = |command: &str, spec: Value, output: Option<&std::path::Path>| {
        let mut tool = Command::new(env!("CARGO_BIN_EXE_agge"));
        tool.args([command]).arg(&game).arg("-");
        if let Some(output) = output {
            tool.arg(output);
        }
        let mut child = tool
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        write!(child.stdin.take().unwrap(), "{spec}").unwrap();
        child.wait_with_output().unwrap()
    };
    let result = run("checkpoint", json!({"ticks":10}), Some(&save));
    assert!(result.status.success(), "{result:?}");
    let checkpoint: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(checkpoint["checkpoint"]["tick"], 10);
    assert!(
        !run("checkpoint", json!({"ticks":11}), Some(&save))
            .status
            .success()
    );
    let resumed = run(
        "inspect",
        json!({"from":"branch.aggesave","ticks":2,"sample":[[0,"entity/hero/x"],[2,"entity/hero/x"]],"select":{"placement":"hero"}}),
        None,
    );
    assert!(resumed.status.success(), "{resumed:?}");
    let resumed: Value = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(resumed["state"]["tick"], 12);
    assert_eq!(resumed["report"]["samples"], json!([20., 24.]));
    std::fs::write(&game, source.replace("move 2", "move 3")).unwrap();
    let rejected = run("test", json!({"from":"branch.aggesave","ticks":1}), None);
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("different project revision"));
}
