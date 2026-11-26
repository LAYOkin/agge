use agge_core::Project;
use agge_runtime::{CompiledGame, Input, Media, Replay, capture, replay_compiled};

#[test]
fn parallax_and_flips_match_whole_view_and_cropped_captures() {
    let project = Project::parse(
        r#"agge/1 {"size":[8,6]}
room main {"size":[32,6],"camera":{"at":[2,0]},"backdrops":[{"sprite":"ribbon","size":[2,1],"scroll":[0.5,0],"repeat":[true,true]}],"instances":[["marker","marker",2,0]]}
object marker {"size":[2,1],"sprite":"ribbon","script":"turn"}
sprite ribbon {"pixel":"ribbon"}
palette inks {"swatches":{"r":"ff0000","b":"0000ff"}}
pixel ribbon
canvas 2 1
style inks
layer strip
  rows 0 0 rb
---
script turn
step:
  flip true, false
---
"#,
    )
    .unwrap();
    let media = Media::load(&project, std::path::Path::new(".")).unwrap();
    let mut world = CompiledGame::new(project).unwrap().preview(None).unwrap();
    let decode = |area| {
        image::load_from_memory(&capture(&world, &media, area).unwrap())
            .unwrap()
            .to_rgba8()
    };
    let red = image::Rgba([255, 0, 0, 255]);
    let blue = image::Rgba([0, 0, 255, 255]);
    let initial = decode([0, 0, 8, 6]);
    assert_eq!(*initial.get_pixel(0, 0), red);
    assert_eq!(*initial.get_pixel(0, 1), blue);
    assert_eq!(*initial.get_pixel(1, 1), red);
    assert_eq!(
        decode([1, 1, 5, 3]),
        image::imageops::crop_imm(&initial, 1, 1, 5, 3).to_image()
    );
    world.tick(&Input::new()).unwrap();
    let flipped = image::load_from_memory(&capture(&world, &media, [0, 0, 2, 1]).unwrap())
        .unwrap()
        .to_rgba8();
    assert_eq!(*flipped.get_pixel(0, 0), blue);
    assert_eq!(*flipped.get_pixel(1, 0), red);
}

#[test]
fn custom_hitboxes_land_and_drop_through_one_way_platforms_only() {
    let game = CompiledGame::new(
        Project::parse(
            r#"agge/1 {"hz":1}
room main {"instances":[["runner","runner",0,0],["ledge","ledge",0,10],["floor","floor",0,16]]}
object runner {"size":[8,8],"hitbox":[2,1,4,3],"gravity":8,"script":"runner"}
object ledge {"size":[40,2],"one_way":true}
object floor {"size":[40,2],"solid":true}
script runner
step:
  global standing = grounded()
  if pressed("down")
    drop 3
  end
---
"#,
        )
        .unwrap(),
    )
    .unwrap();
    let spec: Replay = serde_json::from_str(
        r#"{"ticks":4,"input":[[3,3,"down"]],"expect":[
            {"at":1,"field":"entity/runner/y","is":6},
            {"at":2,"field":"global/standing","is":1},
            {"at":3,"field":"entity/runner/y","is":12},
            {"at":4,"field":"global/standing","is":1}
        ]}"#,
    )
    .unwrap();
    let (world, report) = replay_compiled(&game, &spec).unwrap();
    assert!(report.passed(), "{report:?}");
    let save = world.checkpoint().unwrap().encode().unwrap();
    let restored = game
        .restore(agge_runtime::Checkpoint::decode(&save).unwrap())
        .unwrap();
    assert_eq!(restored.checkpoint().unwrap().encode().unwrap(), save);
}
