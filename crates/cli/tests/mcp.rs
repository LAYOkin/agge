use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn mcp_discovers_tools_edits_and_returns_cropped_image() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("game.agge");
    let binary = env!("CARGO_BIN_EXE_agge");
    assert!(
        Command::new(binary)
            .arg("init")
            .arg(&file)
            .output()
            .unwrap()
            .status
            .success()
    );
    let mut child = Command::new(binary)
        .arg("mcp")
        .arg(&file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"agge_edit","arguments":{"ops":[
            {"op":"put","kind":"script","id":"motion","data":"fn double(n) = n * 2"},
            {"op":"put","kind":"script","id":"drive","data":"create:\n  set speed = 4\n\n# movement\nstep:\n  move 0, 0"},
            {"op":"put","kind":"object","id":"hero","data":{"script":"drive"}},
            {"op":"place","room":"main","object":"hero","id":"hero","at":[10,12]}]}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"agge_run","arguments":{
            "ticks":1,"select":{"placement":"hero"},"crop":[0,0,32,32],"expect":[{"at":1,"field":"entity/hero/y","is":12}]}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"agge_query","arguments":{"get":"room/main","path":"/instances/0"}}}),
        json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"agge_edit","arguments":{"ops":[
            {"op":"script_event","id":"drive","event":"step","body":"  global score = global(score) + 1\n  move motion.double(speed / 2), 0"}
        ]}}}),
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"agge_query","arguments":{"outline":"script/drive"}}}),
        json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"agge_query","arguments":{"outline":"script/drive","symbol":"var/speed"}}}),
        json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"agge_query","arguments":{"get":"script/drive","event":"step"}}}),
        json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"agge_run","arguments":{
            "ticks":2,"expect":[{"at":2,"field":"entity/hero/x","is":18},{"at":2,"field":"global/score","is":2}]}}}),
        json!({"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"agge_query","arguments":{"get":"script/motion","function":"double"}}}),
        json!({"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"agge_query","arguments":{"refs":"script/motion/fn/double"}}}),
        json!({"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"agge_query","arguments":{"outline":"script/motion","symbol":"fn/double"}}}),
        json!({"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"agge_edit","arguments":{"ops":[
            {"op":"splice","id":"motion","at":[1,1],"text":"fn double(n) = n / 0"}
        ]}}}),
        json!({"jsonrpc":"2.0","id":15,"method":"tools/call","params":{"name":"agge_run","arguments":{"ticks":1,"select":{"placement":"hero"}}}}),
        json!({"jsonrpc":"2.0","id":16,"method":"tools/call","params":{"name":"agge_edit","arguments":{"ops":[
            {"op":"put","kind":"sprite","id":"terrain","data":{"file":"terrain.png"}},
            {"op":"slice","id":"terrain","cell":[16,16],"grid":[2,1]},
            {"op":"put","kind":"tilemap","id":"floor","data":{"cell":16,"tiles":{},"rows":[".."]}},
            {"op":"atlas","map":"floor","sprite":"terrain","keys":"ab","solid":"b"},
            {"op":"stamp","map":"floor","at":[0,0],"rows":["ab"]}
        ]}}}),
        json!({"jsonrpc":"2.0","id":17,"method":"tools/call","params":{"name":"agge_query","arguments":{"refs":"sprite/terrain"}}}),
        json!({"jsonrpc":"2.0","id":18,"method":"tools/call","params":{"name":"agge_query","arguments":{"get":"tilemap/floor","path":"/tiles/b"}}}),
        json!({"jsonrpc":"2.0","id":19,"method":"tools/call","params":{"name":"agge_edit","arguments":{"ops":[
            {"op":"camera","room":"main","size":[1280,360],"at":[200,0]}
        ]}}}),
        json!({"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"agge_run","arguments":{
            "ticks":0,"select":{"placement":"hero"},"expect":[{"at":0,"field":"camera/x","is":200}]
        }}}),
        json!({"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"agge_edit","arguments":{"ops":[
            {"op":"put","kind":"object","id":"ledge","data":{"size":[60,8],"one_way":true}},
            {"op":"place","room":"main","object":"ledge","id":"deck","at":[0,28]}
        ]}}}),
        json!({"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"agge_run","arguments":{
            "ticks":0,"expect":[{"at":0,"field":"grounded/hero","is":1}]
        }}}),
        json!({"jsonrpc":"2.0","id":23,"method":"tools/call","params":{"name":"agge_edit","arguments":{
            "ops":[{"op":"placement","room":"main","id":"hero","at":[80,12]}],
            "verify":[{"ticks":0,"expect":[{"at":0,"field":"grounded/hero","is":1}]}]
        }}}),
        json!({"jsonrpc":"2.0","id":24,"method":"tools/call","params":{"name":"agge_query","arguments":{
            "read":[{"get":"room/main/instances/hero"},{"get":"room/main/instances/deck"}]
        }}}),
        json!({"jsonrpc":"2.0","id":25,"method":"tools/call","params":{"name":"agge_run","arguments":{
            "ticks":0,"sample":{"at":[0],"fields":["grounded/hero","entity/hero/x"]}
        }}}),
        json!({"jsonrpc":"2.0","id":26,"method":"tools/call","params":{"name":"agge_run","arguments":{
            "cases":{"starts":{"ticks":0,"expect":[{"at":0,"field":"room","is":"main"}]},
                "missing":{"room":"absent","ticks":0}}
        }}}),
        json!({"jsonrpc":"2.0","id":27,"method":"tools/call","params":{"name":"agge_edit","arguments":{"ops":[
            {"op":"rename","kind":"sprite","id":"terrain","to":"ground"}
        ]}}}),
        json!({"jsonrpc":"2.0","id":28,"method":"tools/call","params":{"name":"agge_query","arguments":{"refs":"sprite/ground"}}}),
        json!({"jsonrpc":"2.0","id":29,"method":"tools/call","params":{"name":"agge_query","arguments":{"refs":"sprite/terrain"}}}),
    ];
    {
        let mut input = child.stdin.take().unwrap();
        for message in messages {
            writeln!(input, "{message}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 29);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(responses[1]["result"]["tools"].as_array().unwrap().len(), 6);
    assert_eq!(responses[2]["result"]["isError"], false);
    assert_eq!(responses[3]["result"]["isError"], false);
    assert_eq!(
        responses[3]["result"]["content"][1]["mimeType"],
        "image/png"
    );
    let text = responses[4]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap()["data"],
        json!(["hero", "hero", 10.0, 12.0])
    );

    let data = |id: usize| -> Value {
        let result = &responses[id - 1]["result"];
        assert_eq!(result["isError"], false, "{result}");
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    assert_eq!(data(6)["changed"], json!(["script/drive"]));
    assert_eq!(data(7)["events"], json!({"create":[1,2],"step":[5,3]}));
    assert_eq!(data(7)["items"], json!(["global/score", "var/speed"]));
    assert_eq!(data(8)["items"], json!([[2, "write"], [7, "read"]]));
    assert_eq!(
        data(9)["data"],
        "step:\n  global score = global(score) + 1\n  move motion.double(speed / 2), 0"
    );
    assert_eq!(data(10)["passed"], true);
    assert_eq!(data(11)["data"], "fn double(n) = n * 2");
    assert_eq!(data(11)["lines"], json!([1, 1]));
    assert_eq!(
        data(12)["items"][0],
        json!({"from":"script/drive","to":"script/motion/fn/double","at":"line/7"})
    );
    assert_eq!(data(13)["parameters"], json!(["n"]));
    assert_eq!(data(14)["changed"], json!(["script/motion"]));
    let failure = &responses[14]["result"];
    assert_eq!(failure["isError"], true);
    let failed: Value =
        serde_json::from_str(failure["content"][0]["text"].as_str().unwrap()).unwrap();
    let fault = &failed["report"]["fault"];
    assert_eq!(fault["source"], json!({"script":"drive","line":7}));
    assert_eq!(
        fault["stack"],
        json!([{"script":"motion","function":"double","line":1}])
    );
    assert_eq!(fault["placement"], "hero");
    assert_eq!(failed["state"]["entities"][0]["x"], 10.0);
    assert_eq!(
        data(16)["changed"],
        json!(["sprite/terrain", "tilemap/floor"])
    );
    assert!(data(17)["items"].as_array().unwrap().iter().any(|edge| {
        edge["from"] == "tilemap/floor" && edge["to"] == "sprite/terrain" && edge["at"] == "atlas"
    }));
    assert_eq!(data(18)["data"], json!([1, true]));
    assert_eq!(data(19)["changed"], json!(["room/main"]));
    assert_eq!(data(20)["state"]["camera"], json!([200.0, 0.0]));
    assert_eq!(data(20)["state"]["entities"][0]["x"], 10.0);
    assert_eq!(data(21)["changed"], json!(["object/ledge", "room/main"]));
    assert_eq!(data(22)["report"]["assertions"], 1);
    let guard = &responses[22]["result"];
    assert_eq!(guard["isError"], true);
    let rejected: Value =
        serde_json::from_str(guard["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(rejected["committed"], false);
    assert_eq!(rejected["reports"][0]["failures"][0]["actual"], 0.0);
    assert_eq!(data(24)["data"][0]["data"][2], 10.0);
    assert_eq!(data(24)["data"][1]["data"][1], "ledge");
    assert_eq!(data(25)["report"]["samples"], json!([1.0, 10.0]));
    let suite = &responses[25]["result"];
    assert_eq!(suite["isError"], true);
    let summary: Value =
        serde_json::from_str(suite["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(summary["assertions"], 1);
    assert_eq!(
        summary["reports"],
        json!({"missing":{"error":"absent room/absent"}})
    );
    assert_eq!(data(27)["revision"], data(28)["revision"]);
    assert!(
        data(28)["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["from"] == "tilemap/floor")
    );
    assert_eq!(data(29)["items"], json!([]));
}

#[test]
fn cli_batches_reads_and_leaves_failed_or_previewed_edits_uncommitted() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("game.agge");
    let run = |command: &str, args: Option<Value>| {
        let mut process = Command::new(env!("CARGO_BIN_EXE_agge"));
        process.arg(command).arg(&file);
        if let Some(args) = args {
            process.arg(args.to_string());
        }
        process.output().unwrap()
    };
    let parse =
        |output: &std::process::Output| serde_json::from_slice::<Value>(&output.stdout).unwrap();
    assert!(run("init", None).status.success());
    let original = std::fs::read(&file).unwrap();
    let revision = parse(&run("query", None))["revision"].clone();
    let ops = json!([{"op":"patch","kind":"room","id":"main","fields":{"bg":[12,18,24,255]}}]);
    let test = |room| json!([{"ticks":0,"expect":[{"at":0,"field":"room","is":room}]}]);
    let failed = run(
        "edit",
        Some(json!({"base":revision,"ops":ops,"verify":test("wrong")})),
    );
    assert_eq!(failed.status.code(), Some(1));
    let failure = parse(&failed);
    assert_eq!(failure["committed"], false);
    assert_eq!(failure["revision"], revision);
    assert_eq!(failure["reports"][0]["failures"][0]["actual"], "main");
    let preview = run(
        "edit",
        Some(json!({"base":revision,"ops":ops,"verify":test("main"),"preview":true})),
    );
    assert!(preview.status.success());
    assert_eq!(parse(&preview)["committed"], false);
    assert_eq!(std::fs::read(&file).unwrap(), original);
    assert!(!dir.path().join("game.agge.history").exists());
    let saved = parse(&run(
        "edit",
        Some(json!({"base":revision,"ops":ops,"verify":test("main")})),
    ));
    assert_eq!(saved["committed"], true);
    let reads =
        json!([{"get":"room/main","path":"/bg"},{"get":"object/missing"},{"refs":"room/main"}]);
    let stale = run("query", Some(json!({"base":revision,"read":reads})));
    assert_eq!(stale.status.code(), Some(2));
    let batch = parse(&run(
        "query",
        Some(json!({"base":saved["revision"],"read":reads})),
    ));
    assert_eq!(batch["data"][0]["data"], json!([12, 18, 24, 255]));
    assert_eq!(batch["data"][1]["error"], "absent object/missing");
    assert!(batch["data"][2]["items"].is_array());
    assert!(run("undo", None).status.success());
    assert_eq!(std::fs::read(&file).unwrap(), original);
}

#[test]
fn cli_can_pin_a_single_replay_to_a_project_revision() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("game.agge");
    let store = agge_core::store::Store::new(&file);
    store.init(&agge_core::Project::default()).unwrap();
    let original = store.load().unwrap().revision;
    let updated = store
        .update(Some(&original), |project| {
            project.settings.title = "Updated title".into();
            Ok(())
        })
        .unwrap()
        .revision;
    let replay = |base: &str| {
        Command::new(env!("CARGO_BIN_EXE_agge"))
            .arg("test")
            .arg(&file)
            .arg(json!({"ticks":0,"base":base}).to_string())
            .output()
            .unwrap()
    };

    let stale = replay(&original);
    assert_eq!(stale.status.code(), Some(2));
    assert!(stale.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&stale.stderr).trim_end(),
        format!("stale revision; current is {updated}")
    );

    let current = replay(&updated);
    assert!(current.status.success());
    let report: Value = serde_json::from_slice(&current.stdout).unwrap();
    assert_eq!(report["revision"], updated);
    assert_eq!(report["passed"], true);
}

#[test]
fn malformed_replay_suite_reports_the_case_name() {
    let example =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/orbit/game.agge");
    let result = Command::new(env!("CARGO_BIN_EXE_agge"))
        .arg("test")
        .arg(example)
        .arg(r#"{"cases":{"startup":{"ticks":0},"wrong_shape":{"ticks":"1"}}}"#)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(
        error.starts_with("case wrong_shape: invalid type"),
        "{error}"
    );
}

#[test]
fn asset_queries_inspect_relative_media_without_returning_binary_data() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("game.agge");
    let binary = env!("CARGO_BIN_EXE_agge");
    assert!(
        Command::new(binary)
            .args(["init"])
            .arg(&file)
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::write(
        dir.path().join("sheet.png"),
        include_bytes!("../../../examples/cinder/art/cinder.png"),
    )
    .unwrap();
    std::fs::write(dir.path().join("invalid.wav"), b"RIFF\x04\x00\x00\x00WAVE").unwrap();
    let get = |asset| {
        Command::new(binary)
            .arg("query")
            .arg(&file)
            .arg(json!({"asset":asset}).to_string())
            .output()
            .unwrap()
    };
    let image = get("sheet.png");
    assert!(image.status.success());
    let metadata: Value = serde_json::from_slice(&image.stdout).unwrap();
    assert_eq!(metadata["data"]["size"], json!([160, 16]));
    assert_eq!(metadata["data"]["type"], "image");
    assert!(!get("invalid.wav").status.success());
    assert!(!get("../sheet.png").status.success());
}
