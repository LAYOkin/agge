use agge_core::{Index, Kind, Project};
use agge_runtime::{CompiledGame, Input, Media, Replay, RoomCollider, capture, replay_compiled};

const LEVEL: &str = r##"agge/1 {"size":[48,16],"hz":1}
room main {"bg":[0,0,0,255],"maps":["course"],"instances":[["hero","hero",0,0]]}
room other {"maps":["course"],"instances":[["door","door",0,12]]}
tilemap course {"cell":8,"tiles":{"#":[[219,144,43,255],true],"i":[[0,130,255,255]]},"rows":["..#...","......"]}
object hero {"size":[4,4],"script":"drive","hidden":true}
object door {"hidden":true,"script":"return"}
script drive
step:
  if pressed("fire")
    if tile_is("course", 2, 0, "#")
      tile course 2, 0, "."
      tile course 4, 0, "#"
      tile course 5, 0, "i"
    end
    global solid = tile_solid("course", 4, 0)
  end
  if pressed("confirm")
    goto other
  end
  move 40, 0
---
script return
step:
  if pressed("cancel")
    goto main
  end
---
"##;

#[test]
fn tile_writes_update_collision_rendering_and_room_local_state_in_the_same_tick() {
    let game = CompiledGame::new(Project::parse(LEVEL).unwrap()).unwrap();
    let replay: Replay = serde_json::from_str(
        r##"{"ticks":2,"input":[[2,2,"fire"]],"expect":[
        {"at":0,"field":"tile/course/2/0","is":"#"},
        {"at":1,"field":"entity/hero/x","is":12},
        {"at":2,"field":"tile/course/2/0","is":"."},
        {"at":2,"field":"tile/course/4/0","is":"#"},
        {"at":2,"field":"global/solid","is":1},
        {"at":2,"field":"entity/hero/x","is":28}
    ]}"##,
    )
    .unwrap();
    let (mut world, report) = replay_compiled(&game, &replay).unwrap();
    assert!(report.passed(), "{report:?}");
    let save = world.checkpoint().unwrap();
    let restored = game.restore(save).unwrap();
    assert_eq!(
        restored.tile_region("course", [1, 0, 5, 2]).unwrap(),
        ["...#i", "....."]
    );
    assert!(
        restored
            .tile_region("course", [1, 0, usize::MAX, 2])
            .is_err()
    );
    let full = capture(&world, &Media::default(), [0, 0, 48, 16]).unwrap();
    assert_eq!(
        full,
        capture(&restored, &Media::default(), [0, 0, 48, 16]).unwrap()
    );
    let image = image::load_from_memory(&full).unwrap().to_rgba8();
    assert_eq!(image.get_pixel(17, 4).0, [0, 0, 0, 255]);
    assert_eq!(image.get_pixel(34, 4).0, [219, 144, 43, 255]);
    assert_eq!(image.get_pixel(41, 4).0, [0, 130, 255, 255]);
    let mut solid_x = Vec::new();
    world.visit_colliders(|collider| {
        if let RoomCollider::Solid(bounds) = collider {
            solid_x.push(bounds.left);
        }
    });
    assert_eq!(solid_x, [32.]);
    world.tick(&Input::from(["confirm".into()])).unwrap();
    assert_eq!(world.room_id(), "other");
    assert_eq!(
        world.tile_region("course", [0, 0, 6, 1]).unwrap(),
        ["..#..."]
    );
    world.tick(&Input::from(["cancel".into()])).unwrap();
    assert_eq!(world.room_id(), "main");
    assert_eq!(
        world.tile_region("course", [0, 0, 6, 1]).unwrap(),
        ["....#i"]
    );
}

#[test]
fn palette_links_validate_glyphs_and_follow_tilemap_renames() {
    let mut project = Project::parse(LEVEL).unwrap();
    assert!(
        Index::build(&project)
            .edges
            .iter()
            .any(|edge| edge.to == "tilemap/course/tiles/i")
    );
    agge_core::Edit::Rename {
        kind: Kind::Tilemap,
        id: "course".into(),
        to: "runway".into(),
    }
    .apply(&mut project)
    .unwrap();
    Index::build(&project).require_valid().unwrap();
    let broken = Project::parse(&LEVEL.replace("5, 0, \"i\"", "5, 0, \"z\"")).unwrap();
    assert!(
        Index::build(&broken)
            .diagnostics
            .iter()
            .any(|item| item.code == "script_tile_key")
    );
}

#[test]
fn tile_writes_outside_the_room_or_grid_fault_at_the_statement() {
    for (source, expected) in [
        (
            LEVEL.replace("tile course 2, 0", "tile course 2.5, 0"),
            "whole cells",
        ),
        (
            LEVEL.replace("tile course 2, 0", "tile course 200, 0"),
            "outside its bounds",
        ),
        (
            LEVEL
                .replace("room other {\"maps\":[\"course\"],", "room other {")
                .replace("goto main", "tile course 2, 0, \".\""),
            "absent from room/other",
        ),
    ] {
        let game = CompiledGame::new(Project::parse(&source).unwrap()).unwrap();
        let mut world = game.preview(None).unwrap();
        let fault = if expected.ends_with("other") {
            world.tick(&Input::from(["confirm".into()])).unwrap();
            world.tick(&Input::from(["cancel".into()])).unwrap_err()
        } else {
            world.tick(&Input::from(["fire".into()])).unwrap_err()
        };
        assert!(fault.message.contains(expected), "{fault:?}");
        assert!(fault.location().is_some());
    }
}
