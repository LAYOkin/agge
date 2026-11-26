use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn dependency_walk_follows_palette_to_launch_room_and_paginates() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("game.agge");
    std::fs::write(
        &file,
        r#"agge/1 {}
room main {"instances":[["hero","unit",0,0]]}
object unit {"animation":"idle"}
sprite icon {"pixel":"root"}
animation idle {"sprite":"icon","frames":[0],"fps":8}
palette sea {"swatches":{"c":"ff2255"}}
pixel root
canvas 1 1
style sea
layer base
px c 0 0
---
pixel alt
canvas 1 1
style sea
base root
---
"#,
    )
    .unwrap();
    let query = |spec: Value| -> Value {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agge"))
            .arg("query")
            .arg(&file)
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        write!(child.stdin.take().unwrap(), "{spec}").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let page = query(json!({"walk":"palette/sea","limit":3}));
    assert_eq!(
        page["items"],
        json!([["pixel/alt", 1], ["pixel/root", 1], ["sprite/icon", 2]])
    );
    assert_eq!(page["next"], 3);
    let next = query(json!({"walk":"palette/sea","offset":3,"base":page["revision"]}));
    assert_eq!(
        next["items"],
        json!([
            ["animation/idle", 3],
            ["object/unit", 4],
            ["room/main", 5],
            ["project", 6]
        ])
    );
    let scoped = query(json!({"read":[
        {"walk":["pixel/root","palette/sea"],"depth":1},
        {"refs":"pixel/root","direction":"out"}
    ]}));
    assert_eq!(
        scoped["data"][0]["items"],
        json!([["pixel/alt", 1], ["sprite/icon", 1]])
    );
    assert_eq!(scoped["data"][1]["items"][0]["to"], "palette/sea");

    let source = std::fs::read_to_string(&file)
        .unwrap()
        .replace("px c 0 0", "px z 0 0");
    std::fs::write(&file, source).unwrap();
    let bad = query(json!({"diagnostics":"pixel/root"}));
    assert_eq!(bad["items"][0]["at"], "pixel/root/line/4");
}
