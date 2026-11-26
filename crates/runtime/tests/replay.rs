use agge_core::Project;
use agge_runtime::{
    CompiledGame, Input, Media, Replay, ReplayPlayback, ReplayRecorder, Scalar, capture,
    pick_placement, replay, replay_compiled,
};
use std::path::Path;

#[test]
fn stepped_playback_observes_input_edges_and_sounds_before_audio_is_drained() {
    let project = Project::parse(
        r#"agge/1 {}
room main {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
audio ping {"file":"ping.wav"}
script drive
create:
  sound ping
step:
  if pressed("fire") || released("fire")
    sound ping
  end
  move down("fire"), 0
---
"#,
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(
        r#"{"ticks":4,"input":[[1,2,"fire"],[2,3,"fire"],[3,3,"fire"]],"expect":[
            {"at":0,"field":"sound/ping","is":1},
            {"at":1,"field":"sound/ping","is":1},
            {"at":3,"field":"sound/ping","is":0},
            {"at":4,"field":"sound/ping","is":1},
            {"at":4,"field":"entity/hero/x","is":3}
        ],"sample":{"at":[4,0],"fields":["entity/hero/x","sound/ping"]}}"#,
    )
    .unwrap();
    let game = CompiledGame::new(project).unwrap();
    let (_, expected) = replay_compiled(&game, &spec).unwrap();
    let mut stepped = ReplayPlayback::new(&game, spec).unwrap();
    assert_eq!(stepped.report().assertions, 1);
    assert_eq!(stepped.drain_audio(), ["ping"]);
    while !stepped.finished() {
        stepped.step();
        stepped.drain_audio();
    }
    stepped.step();
    assert_eq!(stepped.world().snapshot(None, None).tick, 4);
    assert!(stepped.report().passed(), "{:?}", stepped.report());
    assert_eq!(
        serde_json::to_value(stepped.report()).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[test]
fn watch_records_initial_and_changed_values_and_flags_the_first_omission() {
    let project = Project::parse(
        r#"agge/1 {}
room main {"instances":[["probe","probe",0,0]]}
room clear {}
object probe {"script":"drive"}
script drive
step:
  if tick == 3
    set x = 3
  end
  if tick == 5
    goto clear
  end
---
"#,
    )
    .unwrap();
    let game = CompiledGame::new(project).unwrap();
    let mut spec = serde_json::json!({
        "ticks":8,"watch":{"at":[2,6],"fields":["room","entity/probe/x"]},
        "sample":[[4,"room"]],"expect":[{"at":7,"field":"room","is":"clear"}]
    });
    let run = |data: &serde_json::Value| {
        let replay: Replay = serde_json::from_value(data.clone()).unwrap();
        let (_, report) = replay_compiled(&game, &replay).unwrap();
        assert!(report.passed());
        serde_json::to_value(report).unwrap()
    };
    let full = run(&spec);
    assert_eq!(full["samples"], serde_json::json!(["main"]));
    assert_eq!(
        full["watch"]["changes"],
        serde_json::json!([
            [2, 0, "main"],
            [2, 1, 0.0],
            [3, 1, 3.0],
            [5, 0, "clear"],
            [5, 1, null]
        ])
    );
    assert!(full["watch"].get("truncated").is_none());
    spec["watch"]["limit"] = 5.into();
    assert_eq!(run(&spec)["watch"], full["watch"]);
    spec["watch"]["limit"] = 4.into();
    assert_eq!(run(&spec)["watch"]["truncated"], true);
    spec["watch"]["fields"] = serde_json::json!(["room", "room"]);
    let invalid: Replay = serde_json::from_value(spec).unwrap();
    assert!(ReplayPlayback::new(&game, invalid).is_err());
}

#[test]
fn touch_probes_follow_moving_and_destroyed_targets_across_cells() {
    let project = Project::parse(
        r#"agge/1 {}
room main {"instances":[["target","target",63,0],["probe","probe",56,0]]}
object target {"size":[6,6],"layer":0,"script":"target"}
object probe {"size":[6,6],"mask":0,"script":"probe"}
script target
step:
  if tick == 2
    destroy
  else
    set x = 129
  end
---
script probe
fn ahead(dx) = touch("target", dx, 0)
create:
  global near = ahead(2)
step:
  global near = ahead(2)
  global far = ahead(75)
  global self = touch("probe")
---
"#,
    )
    .unwrap();
    let spec = serde_json::from_value(serde_json::json!({"ticks":2,"expect":[
        {"at":0,"field":"global/near","is":1},
        {"at":1,"field":"global/near","is":0},
        {"at":1,"field":"global/far","is":1},
        {"at":1,"field":"global/self","is":0},
        {"at":2,"field":"global/far","is":0}
    ]}))
    .unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
}

#[test]
fn create_only_actors_keep_moving_and_hit_static_geometry() {
    let project = Project::parse(
        r#"agge/1 {"hz":1}
room main {"instances":[["mover","mover",0,0],["wall","wall",10,0]]}
object mover {"script":"launch","size":[2,2]}
object wall {"solid":true,"size":[2,2]}
script launch
create:
  set vx = 4
hit_x:
  global impact_speed = impact()
---
"#,
    )
    .unwrap();
    let spec = serde_json::from_str(
        r#"{"ticks":4,"expect":[
        {"at":1,"field":"entity/mover/x","is":4},
        {"at":3,"field":"entity/mover/x","is":8},
        {"at":4,"field":"entity/mover/vx","is":0},
        {"at":4,"field":"global/impact_speed","is":4}
    ]}"#,
    )
    .unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
}

#[test]
fn bundled_games_are_playable_and_repeatable() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for game in ["orbit", "paddle", "relay", "switchback", "bankshot"] {
        let root = examples.join(game);
        let project =
            Project::parse(&std::fs::read_to_string(root.join("game.agge")).unwrap()).unwrap();
        let spec: Replay =
            serde_json::from_slice(&std::fs::read(root.join("replay.json")).unwrap()).unwrap();
        let (first, report) = replay(project.clone(), &spec).unwrap();
        let (second, _) = replay(project, &spec).unwrap();
        assert!(report.passed(), "{report:?}");
        assert_eq!(report.assertions, spec.expect.len());
        assert_eq!(
            serde_json::to_value(first.snapshot(None, None)).unwrap(),
            serde_json::to_value(second.snapshot(None, None)).unwrap()
        );
        let media = Media::load(first.project(), &root).unwrap();
        let crop = capture(&first, &media, [16, 16, 64, 48]).unwrap();
        let decoded = image::load_from_memory(&crop).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (64, 48));
    }
}

#[test]
fn cinder_suite_runs_with_isolated_state_and_renders_its_hud() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/cinder");
    let mut project = Project::parse(include_str!("../../../examples/cinder/game.agge")).unwrap();
    let alias = project
        .get(agge_core::Kind::Sprite, "cinder")
        .unwrap()
        .clone();
    project.put("alias".into(), alias);
    let media = Media::load(&project, &root).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &media.sprites["cinder"],
        &media.sprites["alias"]
    ));
    let suite: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/cinder/replay.json")).unwrap();
    let cases: std::collections::BTreeMap<String, Replay> =
        serde_json::from_value(suite["cases"].clone()).unwrap();
    let game = CompiledGame::new(project).unwrap();
    for name in ["win", "filter", "loss", "cooldown", "win"] {
        let (_, report) = replay_compiled(&game, &cases[name]).unwrap();
        assert!(report.passed(), "{name}: {report:?}");
        assert_eq!(report.assertions, cases[name].expect.len());
    }
    let spec: Replay =
        serde_json::from_str(r#"{"room":"yard","ticks":8,"input":[[1,1,"fire"]]}"#).unwrap();
    let (world, report) = replay_compiled(&game, &spec).unwrap();
    assert!(report.passed());
    let png = capture(&world, &media, [124, 206, 84, 6]).unwrap();
    let frame = image::load_from_memory(&png).unwrap().into_rgba8();
    assert_eq!(frame.get_pixel(10, 2).0, [85, 236, 199, 255]);
    assert_eq!(frame.get_pixel(75, 2).0, [22, 28, 43, 255]);
}

#[test]
fn captures_preserve_background_color_under_low_opacity_objects() {
    let project = Project::parse(
        r#"agge/1 {"size":[16,16]}
room main {"bg":[0,0,255,1],"instances":[["overlay","overlay",0,0]]}
object overlay {"size":[8,8],"color":[255,0,0,1]}
"#,
    )
    .unwrap();
    let spec = serde_json::from_str(r#"{"ticks":0}"#).unwrap();
    let (world, _) = replay(project, &spec).unwrap();
    let png = capture(&world, &Media::default(), [7, 7, 2, 1]).unwrap();
    let image = image::load_from_memory(&png).unwrap().into_rgba8();
    assert_eq!(image.get_pixel(0, 0).0, [127, 0, 127, 2]);
    assert_eq!(image.get_pixel(1, 0).0, [0, 0, 255, 1]);
}

#[test]
fn top_down_order_sorts_feet_within_depth_and_keeps_layer_precedence() {
    let project = Project::parse(r#"agge/1 {"size":[64,64]}
room main {"y_sort":true,"instances":[["front","red",8,14],["high","blue",8,10],["back","green",8,10]]}
object red {"color":[210,20,70,255]}
object green {"color":[20,210,70,255]}
object blue {"size":[8,8],"color":[20,70,210,255],"depth":1}
"#).unwrap();
    let spec = serde_json::from_str(r#"{"ticks":0}"#).unwrap();
    let (world, _) = replay(project, &spec).unwrap();
    let png = capture(&world, &Media::default(), [8, 14, 16, 10]).unwrap();
    let image = image::load_from_memory(&png).unwrap().into_rgba8();
    assert_eq!(image.get_pixel(2, 2).0, [20, 70, 210, 255]);
    assert_eq!(image.get_pixel(9, 2).0, [210, 20, 70, 255]);
    assert_eq!(
        pick_placement(world.project(), world.room(), [10., 16.]),
        Some("high")
    );
    assert_eq!(
        pick_placement(world.project(), world.room(), [17., 16.]),
        Some("front")
    );
    assert_eq!(
        pick_placement(world.project(), world.room(), [40., 40.]),
        None
    );
}

#[test]
fn room_overlay_reports_visible_solid_tiles_and_one_way_objects() {
    use agge_runtime::{Bounds, RoomCollider, room_colliders};
    let project = Project::parse(
        r##"agge/1 {"size":[32,16]}
room main {"maps":["floor"],"instances":[["ledge","platform",12,4],["outside","block",80,4],["prop","scenery",4,4]]}
tilemap floor {"cell":8,"at":[-4,0],"tiles":{"#":[[255,255,255,255],true],"+":[[255,255,255,255],false]},"rows":["#+"]}
object platform {"size":[10,4],"one_way":true,"hidden":true,"script":"slide"}
object block {"size":[8,8],"solid":true}
object scenery {"size":[4,4]}
script slide
step:
  move 2, 0
---
"##,
    )
    .unwrap();
    let spec = serde_json::from_str(r#"{"ticks":0}"#).unwrap();
    let (mut world, _) = replay(project, &spec).unwrap();
    let bounds = |collider| match collider {
        RoomCollider::Solid(bounds) => (false, bounds.left, bounds.top, bounds.right),
        RoomCollider::OneWay(bounds) => (true, bounds.left, bounds.top, bounds.right),
        RoomCollider::Hitbox(_) => unreachable!("fixture uses default shape"),
    };
    let mut outlines = Vec::new();
    room_colliders(
        world.project(),
        world.room(),
        Bounds::new(0., 0., 30., 10.),
        |collider| {
            outlines.push(bounds(collider));
        },
    );
    assert_eq!(outlines, [(false, -4., 0., 4.), (true, 12., 4., 22.)]);
    world.tick(&Input::new()).unwrap();
    outlines.clear();
    world.visit_colliders(|collider| outlines.push(bounds(collider)));
    assert_eq!(outlines, [(false, -4., 0., 4.), (true, 14., 4., 24.)]);
}

#[test]
fn placement_picking_uses_ellipse_coverage_and_sprite_bounds() {
    let project = Project::parse(
        r#"agge/1 {"size":[64,64]}
room main {"instances":[["back","rect",8,8],["circle","oval",8,8],["art","sprite_oval",32,8]]}
object rect {"size":[20,12]}
object oval {"size":[20,12],"shape":"circle"}
object sprite_oval {"size":[20,12],"shape":"circle","sprite":"sheet"}
sprite sheet {"file":"art.png"}
"#,
    )
    .unwrap();
    let spec = serde_json::from_str(r#"{"ticks":0}"#).unwrap();
    let (world, _) = replay(project, &spec).unwrap();
    let pick = |point| pick_placement(world.project(), world.room(), point);
    assert_eq!(pick([8.5, 8.5]), Some("back"));
    assert_eq!(pick([18., 14.]), Some("circle"));
    assert_eq!(pick([32.5, 8.5]), Some("art"));
}

#[test]
fn impact_handlers_receive_signed_speed_in_axis_order() {
    let project = Project::parse(
        r#"agge/1 {"start":"test","hz":64}
room test {"instances":[["hero","hero",0,0],["wall","wall",12,0],["floor","floor",0,12]]}
object hero {"size":[8,8],"script":"drive"}
object wall {"size":[8,12],"solid":true}
object floor {"size":[12,8],"solid":true}
script drive
fn rebound() = -impact()
create:
  set vx = 320
  set vy = 320
hit_x:
  global order = 1
  set vx = rebound()
hit_y:
  global order = global(order) * 10 + 2
  set vy = rebound()
---
"#,
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(
        r#"{"ticks":2,"sample":[[2,"entity/hero/x"],
        [1,"entity/hero/vx"],[0,"entity/hero/vy"],[1,"global/order"],[2,"entity/missing/x"]]}"#,
    )
    .unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
    assert_eq!(
        report.samples,
        vec![
            Some(Scalar::Number(-1.)),
            Some(Scalar::Number(-320.)),
            Some(Scalar::Number(320.)),
            Some(Scalar::Number(12.)),
            None
        ]
    );
}

#[test]
fn recorded_input_spans_round_trip_and_replay() {
    let project = Project::parse(
        r#"agge/1 {"start":"test"}
room test {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
step:
  move down("right"), down("down")
  global presses = global(presses) + pressed("right")
---
"#,
    )
    .unwrap();
    let mut recorder = ReplayRecorder::for_project(&project, None);
    for keys in [
        &["right"][..],
        &["right", "down"],
        &["down"],
        &[],
        &["right"],
    ] {
        let input: Input = keys.iter().map(|key| (*key).into()).collect();
        recorder.record(&input);
    }
    let mut spec = serde_json::to_value(recorder.replay()).unwrap();
    assert_eq!(spec["room"], "test");
    assert_eq!(spec["seed"], project.settings.seed);
    assert_eq!(
        spec["input"],
        serde_json::json!([[1, 2, "right"], [2, 3, "down"], [5, 5, "right"]])
    );
    spec["sample"] = serde_json::json!([
        [5, "entity/hero/x"],
        [5, "entity/hero/y"],
        [5, "global/presses"]
    ]);
    let (_, report) = replay(project, &serde_json::from_value(spec).unwrap()).unwrap();
    assert_eq!(
        report.samples,
        [3., 2., 2.].map(|n| Some(Scalar::Number(n)))
    );
}

#[test]
fn replay_seed_overrides_create_and_step_randomness_without_changing_the_project() {
    let project = Project::parse(
        r#"agge/1 {"seed":5581}
room main {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
create:
  global roll = rand()
step:
  global roll = rand()
---
"#,
    )
    .unwrap();
    let game = CompiledGame::new(project).unwrap();
    let run = |seed: Option<u64>| {
        let spec: Replay = serde_json::from_value(serde_json::json!({
            "seed": seed, "ticks": 1, "sample": [[0, "global/roll"], [1, "global/roll"]]
        }))
        .unwrap();
        let (world, report) = replay_compiled(&game, &spec).unwrap();
        assert!(report.passed());
        assert_eq!(world.project().settings.seed, 5581);
        report.samples
    };

    let default = run(None);
    assert_eq!(default, run(Some(5581)));
    let alternate = run(Some(9843));
    assert_ne!(default[0], alternate[0]);
    assert_ne!(default[1], alternate[1]);
    assert_eq!(alternate, run(Some(9843)));
    assert_eq!(default, run(None));
}

#[test]
fn replay_timer_expiry_is_visible_only_on_the_due_tick() {
    let project = Project::parse(
        r#"agge/1 {}
room main {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
create:
  after ready, 2
  after reset, 2
step:
  if expired(reset)
    after reset, 3
  end
---
"#,
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(
        r#"{"ticks":3,"expect":[
          {"at":1,"field":"expired/hero/ready","is":0},
          {"at":2,"field":"expired/hero/ready","is":1},
          {"at":2,"field":"timer/hero/ready","is":0},
          {"at":2,"field":"expired/hero/reset","is":0},
          {"at":2,"field":"timer/hero/reset","is":3},
          {"at":3,"field":"expired/hero/ready","is":0}
        ]}"#,
    )
    .unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
    assert_eq!(report.assertions, spec.expect.len());
}

#[test]
fn replay_counts_queued_sounds_by_tick_and_clip() {
    let project = Project::parse(
        r#"agge/1 {}
room main {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
audio ping {"file":"ping.wav"}
audio chime {"file":"ping.wav"}
script drive
create:
  sound ping
step:
  if pressed("fire")
    sound ping
    sound chime
    sound ping
  end
---
"#,
    )
    .unwrap();
    let spec = serde_json::from_str(
        r#"{"ticks":3,"input":[[2,3,"fire"]],"expect":[
          {"at":0,"field":"sound/ping","is":1},
          {"at":1,"field":"sound/ping","is":0},
          {"at":2,"field":"sound/ping","is":2},
          {"at":2,"field":"sound/chime","is":1},
          {"at":3,"field":"sound/ping","is":0}
        ],"sample":[[2,"sound/missing"],[2,"sound/ping"]]}"#,
    )
    .unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
    assert_eq!(report.assertions, 5);
    assert_eq!(report.samples, [None, Some(Scalar::Number(2.))]);
}

#[test]
fn release_is_seen_once_per_held_span_and_keeps_user_functions_working() {
    let project = Project::parse(
        r#"agge/1 {}
room main {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
fn released(scale) = scale * 2
create:
  global startup = released(3)
step:
  if released("fire")
    global releases = global(releases) + 1
  end
---
"#,
    )
    .unwrap();
    let spec = serde_json::from_str(
        r#"{"ticks":6,"input":[[1,2,"fire"],[5,5,"fire"]],"expect":[
          {"at":0,"field":"global/startup","is":6},
          {"at":2,"field":"global/releases","is":0},
          {"at":3,"field":"global/releases","is":1},
          {"at":4,"field":"global/releases","is":1},
          {"at":6,"field":"global/releases","is":2}
        ]}"#,
    )
    .unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
    assert_eq!(report.assertions, 5);
}

#[test]
fn movement_sweeps_multiple_cells_and_pressed_fires_once() {
    let project = Project::parse(
        r##"agge/1 {"start":"test","size":[80,64]}
room test {"instances":[["hero","hero",0,16]],"maps":["wall"]}
object hero {"size":[8,8],"script":"drive"}
tilemap wall {"cell":16,"tiles":{"#":[[30,60,90,255],true]},"rows":[".....","..#..","....."]}
script drive
step:
  if pressed("fire")
    global count = global(count) + 1
  end
  move 100, 0
---
"##,
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(
        r#"{"ticks":3,"input":[[1,3,"fire"]],"expect":[
        {"at":1,"field":"entity/hero/x","is":24},{"at":3,"field":"entity/hero/x","is":24},
        {"at":3,"field":"global/count","is":1}]}"#,
    )
    .unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
}

#[test]
fn runtime_fault_identifies_tick_entity_and_event() {
    let project = Project::parse(
        r#"agge/1 {"start":"test"}
room test {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
step:
  set x = 1 / 0
---
"#,
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(r#"{"ticks":10}"#).unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    let fault = report.fault.unwrap();
    assert_eq!((fault.tick, fault.entity, fault.event), (1, 1, "step"));
}

#[test]
fn bounded_repeat_and_math_compose() {
    let project = Project::parse(
        r#"agge/1 {"start":"test"}
room test {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
create:
  repeat 3
    if global(total) < 20
      global total = global(total) + clamp(sqrt(64), 0, max(1, 5))
    end
  end
---
"#,
    )
    .unwrap();
    let spec: Replay =
        serde_json::from_str(r#"{"ticks":0,"expect":[{"at":0,"field":"global/total","is":15}]}"#)
            .unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
}

#[test]
fn helpers_execute_lazily_and_in_the_declaring_module() {
    let project = Project::parse(
        r#"agge/1 {"start":"test"}
room test {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
create:
  set speed = 4
step:
  move math.steps(speed), 0
fn bump(n) = n + 100
---
script math
fn steps(n) = choose(n <= 0, 0, bump(n) + steps(n - 1))
fn bump(n) = n + 1
---
"#,
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(
        r#"{"ticks":2,"expect":[{"at":1,"field":"entity/hero/x","is":14},{"at":2,"field":"entity/hero/x","is":28}]}"#
    ).unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
}

#[test]
fn create_fault_returns_partial_state_and_a_precise_call_trace() {
    let project = Project::parse(
        r#"agge/1 {"start":"test"}
room test {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
create:
  set x = 5
  if true
    set x = math.checked(1)
  end
---
script math
# routine
fn checked(n) = divide(n, 0)
fn divide(a, b) = a / b
---
"#,
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(r#"{"ticks":1}"#).unwrap();
    let (world, report) = replay(project, &spec).unwrap();
    assert!(!report.passed());
    assert_eq!(report.ticks, 0);
    let fault = report.fault.unwrap();
    assert_eq!((fault.tick, fault.entity, fault.event), (0, 1, "create"));
    let origin = fault.origin.unwrap();
    assert_eq!(origin.placement.as_deref(), Some("hero"));
    let at = origin.source;
    assert_eq!((at.script.as_str(), at.line), ("drive", 4));
    assert_eq!(
        (origin.stack[0].function.as_str(), origin.stack[0].line),
        ("checked", 2)
    );
    assert_eq!(
        (origin.stack[1].function.as_str(), origin.stack[1].line),
        ("divide", 3)
    );
    assert_eq!(world.snapshot(None, Some("hero")).entities[0].x, 5.0);
}

#[test]
fn runaway_recursion_stops_and_its_report_stays_small() {
    let project = Project::parse(
        r#"agge/1 {"start":"test"}
room test {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
fn again(n) = again(n + 1)
step:
  move again(1), 0
---
"#,
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(r#"{"ticks":1}"#).unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    let fault = report.fault.unwrap();
    assert!(fault.message.contains("nesting limit"));
    let origin = fault.origin.unwrap();
    assert_eq!(origin.stack.len(), 16);
    assert!(origin.omitted_frames.is_some_and(|count| count > 0));
}

#[test]
fn overlapping_input_spans_keep_the_action_held_until_the_last_end() {
    let project = Project::parse(
        r#"agge/1 {"start":"test"}
room test {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
step:
  if pressed("fire")
    global hits = global(hits) + 1
  end
  move down("fire"), 0
---
"#,
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(
        r#"{"ticks":9,
        "input":[[6,6,"fire"],[1,2,"fire"],[2,5,"fire"],[8,8,"fire"]],
        "expect":[{"at":6,"field":"global/hits","is":1},{"at":6,"field":"entity/hero/x","is":6},
                  {"at":9,"field":"global/hits","is":2},{"at":9,"field":"entity/hero/x","is":7}]}"#,
    )
    .unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
}

#[test]
fn atlas_frames_render_with_tint_and_keep_tile_collision() {
    let project = Project::parse(r##"agge/1 {"start":"test","size":[24,12]}
room test {"bg":[10,14,18,255],"maps":["layout"],"instances":[["hero","hero",0,4]]}
object hero {"size":[2,2],"script":"drive","hidden":true}
sprite atlas {"file":"test.png","frames":[[0,0,2,1],[2,0,2,1]]}
tilemap layout {"cell":4,"at":[4,4],"atlas":"atlas","tiles":{"a":[0],"b":[1,true,[255,128,255,255]],"#":[[12,99,30,255]]},"rows":["ab#"]}
script drive
step:
  move 30, 0
---
"##).unwrap();
    let spec: Replay =
        serde_json::from_str(r#"{"ticks":1,"expect":[{"at":1,"field":"entity/hero/x","is":6}]}"#)
            .unwrap();
    let (world, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
    let atlas = image::RgbaImage::from_raw(
        4,
        1,
        vec![
            200, 0, 0, 255, 0, 0, 180, 255, 40, 200, 100, 255, 0, 0, 0, 0,
        ],
    )
    .unwrap();
    let mut media = Media::default();
    media.sprites.insert("atlas".into(), atlas.into());
    let png = capture(&world, &media, [4, 4, 12, 4]).unwrap();
    let pixels = image::load_from_memory(&png).unwrap().into_rgba8();
    assert_eq!(pixels.get_pixel(0, 0).0, [200, 0, 0, 255]);
    assert_eq!(pixels.get_pixel(3, 0).0, [0, 0, 180, 255]);
    assert_eq!(pixels.get_pixel(4, 0).0, [40, 100, 100, 255]);
    assert_eq!(pixels.get_pixel(7, 0).0, [10, 14, 18, 255]);
    assert_eq!(pixels.get_pixel(8, 0).0, [12, 99, 30, 255]);
    assert!(media.frame(world.project(), "atlas", 2).is_none());
}

#[test]
fn camera_follows_deterministically_and_accepts_a_script_override() {
    let project = Project::parse(r#"agge/1 {"start":"field","size":[64,32]}
room field {"size":[320,80],"camera":{"follow":"hero","dead":[16,8],"smooth":15},"instances":[["hero","hero",52,12],["panel","panel",0,0]]}
room end {}
object hero {"size":[8,8],"script":"driver"}
object panel {"hidden":true,"script":"portal"}
script driver
create:
  global room_width = width
  global room_height = height
  global view_width = view("width")
step:
  move 20 * down("right"), 0
  if pressed("pan")
    camera view("x") + 200, 500
  end
  if pressed("remove")
    destroy
  end
---
script portal
step:
  if pressed("exit")
    camera 100, 30
    goto end
  end
---
"#).unwrap();
    let spec: Replay = serde_json::from_str(
        r#"{"ticks":5,"input":[
        [1,2,"right"],[3,3,"pan"],[4,4,"remove"],[5,5,"exit"]
    ],"expect":[
        {"at":0,"field":"camera/x","is":16},{"at":0,"field":"camera/y","is":0},
        {"at":0,"field":"global/room_width","is":320},{"at":0,"field":"global/room_height","is":80},
        {"at":0,"field":"global/view_width","is":64},
        {"at":1,"field":"camera/x","is":21},{"at":2,"field":"camera/x","is":29.75},
        {"at":3,"field":"camera/x","is":229.75},{"at":3,"field":"camera/y","is":48},
        {"at":4,"field":"camera/x","is":229.75},{"at":4,"field":"count/hero","is":0},
        {"at":5,"field":"room","is":"end"},{"at":5,"field":"camera/x","is":0},
        {"at":5,"field":"camera/y","is":0}
    ]}"#,
    )
    .unwrap();
    let (world, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
    assert!(world.snapshot(None, Some("hero")).entities.is_empty());
    assert!(
        serde_json::to_value(world.snapshot(None, None))
            .unwrap()
            .get("camera")
            .is_none()
    );
}

#[test]
fn scrolling_capture_uses_world_coordinates_for_lights_and_screen_coordinates_for_hud() {
    let project = Project::parse(r#"agge/1 {"start":"test","size":[24,16]}
room test {"size":[96,40],"camera":{"at":[40,4]},"bg":[100,100,100,255],"darkness":0.75,"instances":[["marker","marker",48,8]],"lights":[{"at":[48,8],"radius":6,"color":[0,0,0,255]}],"ui":[{"at":[1,1],"text":"A","size":8}]}
object marker {"size":[4,4],"color":[220,44,11,255]}
"#).unwrap();
    let world = agge_runtime::World::new(project, None).unwrap();
    let media = Media::default();
    let full = capture(&world, &media, [0, 0, 24, 16]).unwrap();
    let full = image::load_from_memory(&full).unwrap().into_rgba8();
    let crop = capture(&world, &media, [6, 3, 8, 6]).unwrap();
    let crop = image::load_from_memory(&crop).unwrap().into_rgba8();
    assert_eq!(world.camera(), [40., 4.]);
    assert_eq!(full.get_pixel(8, 4).0, [220, 44, 11, 255]);
    assert_eq!(full.get_pixel(23, 15).0, [25, 25, 25, 255]);
    assert!((0..8).any(|x| (1..9).any(|y| full.get_pixel(x, y).0 == [255, 255, 255, 255])));
    for (x, y, pixel) in crop.enumerate_pixels() {
        assert_eq!(pixel, full.get_pixel(x + 6, y + 3));
    }
}

#[test]
fn one_way_platforms_land_from_above_and_probes_do_not_move_the_entity() {
    let project = Project::parse(
        r#"agge/1 {"start":"yard","size":[96,96]}
room yard {"instances":[["hero","hero",24,64],["ledge","platform",10,40]]}
object hero {"size":[8,8],"script":"drive"}
object platform {"size":[70,4],"one_way":true}
script drive
step:
  if pressed("jump")
    global ceiling = blocked(0, -60)
    move 0, -60
  end
  if pressed("fall")
    global floor = blocked(0, 80)
    global original = y
    move 0, 80
    global contact = grounded()
  end
  if pressed("right")
    move 72, 0
  end
---
"#,
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(
        r#"{"ticks":3,"input":[[1,1,"jump"],[2,2,"fall"],[3,3,"right"]],"expect":[
        {"at":0,"field":"grounded/hero","is":0},
        {"at":1,"field":"global/ceiling","is":0},{"at":1,"field":"entity/hero/y","is":4},
        {"at":2,"field":"global/floor","is":1},{"at":2,"field":"global/original","is":4},
        {"at":2,"field":"entity/hero/y","is":32},{"at":2,"field":"global/contact","is":1},
        {"at":2,"field":"grounded/hero","is":1},{"at":3,"field":"grounded/hero","is":0}
    ]}"#,
    )
    .unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
}

#[test]
fn solid_movement_is_visible_to_later_entities_in_the_same_tick() {
    let project = Project::parse(
        r#"agge/1 {"start":"yard"}
room yard {"instances":[["wall","wall",-200,30],["hero","hero",200,30]],"maps":["pad"]}
object hero {"size":[8,8],"script":"drive"}
object wall {"size":[8,8],"solid":true,"script":"wall"}
tilemap pad {"at":[264,38],"cell":16,"tiles":{"!":[[90,90,90,255],true]},"rows":["!!!!"]}
script wall
step:
  if tick == 1
    set x = 272
  else
    destroy
  end
---
script drive
step:
  move 100, 0
---
"#,
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(
        r#"{"ticks":2,"expect":[
        {"at":1,"field":"entity/hero/x","is":264},{"at":1,"field":"grounded/hero","is":1},
        {"at":2,"field":"count/wall","is":0},{"at":2,"field":"entity/hero/x","is":364}
    ]}"#,
    )
    .unwrap();
    let (_, report) = replay(project, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
}
