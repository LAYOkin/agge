use agge_core::Project;
use agge_runtime::{CompiledGame, Input, Replay, ReplayPlayback, ReplayRecorder, replay_compiled};

#[test]
fn pointer_spans_drive_buttons_camera_coordinates_and_restore() {
    let game = CompiledGame::new(
        Project::parse(
            r#"agge/1 {"size":[32,24],"pointer":true}
room main {"size":[100,64],"camera":{"at":[12,5]},"instances":[["probe","probe",0,0]]}
object probe {"script":"probe"}
script probe
step:
  if pressed("mouse_left")
    global click = pointer("world_x")
  end
  global y = pointer("world_y")
---
"#,
        )
        .unwrap(),
    )
    .unwrap();
    let click = Input::from(["mouse_left".into()]);
    let none = Input::new();
    let mut recorder = ReplayRecorder::new("main");
    recorder.record_frame(&click, Some([6, 7]));
    recorder.record_frame(&none, Some([6, 7]));
    recorder.record_frame(&none, None);
    recorder.record_frame(&none, Some([7, 8]));
    let mut replay = serde_json::to_value(recorder.replay()).unwrap();
    assert_eq!(
        replay["pointer"],
        serde_json::json!([[1, 2, 6, 7], [4, 4, 7, 8]])
    );
    replay["expect"] = serde_json::json!([
        {"at":0,"field":"pointer/active","is":0},
        {"at":1,"field":"global/click","is":18},
        {"at":2,"field":"global/y","is":12},
        {"at":3,"field":"global/y","is":-1},
        {"at":4,"field":"pointer/x","is":7}
    ]);
    let (world, report) =
        replay_compiled(&game, &serde_json::from_value(replay.clone()).unwrap()).unwrap();
    assert!(report.passed(), "{report:?}");
    let loaded = game.restore(world.checkpoint().unwrap()).unwrap();
    assert_eq!(loaded.snapshot(None, None).pointer, Some([7, 8]));

    for invalid in [
        serde_json::json!([[1, 2, 32, 0]]),
        serde_json::json!([[1, 2, 6, 7], [2, 3, 1, 1]]),
        serde_json::json!([[3, 4, 6, 7], [1, 2, 1, 1]]),
    ] {
        replay["pointer"] = invalid;
        let spec: Replay = serde_json::from_value(replay.clone()).unwrap();
        assert!(ReplayPlayback::new(&game, spec).is_err());
    }
}
