use agge_core::Project;
use agge_runtime::{Media, Replay, capture, replay};
use std::path::Path;

#[test]
fn bundled_games_are_playable_and_repeatable() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for game in ["orbit", "paddle", "relay", "switchback"] {
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
    let media = Media {
        sprites: std::collections::BTreeMap::from([("atlas".into(), atlas)]),
    };
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
