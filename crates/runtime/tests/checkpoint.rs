use agge_core::{Kind, Project, Resource};
use agge_runtime::{Checkpoint, CompiledGame, Input, Media, Replay, capture, replay_from};

const GAME: &str = r#"agge/1 {"size":[32,24],"hz":10,"seed":519}
room main {"size":[128,48],"camera":{"follow":"hero","smooth":2},"instances":[["hero","hero",10,0],["beam","beam",0,12]]}
object hero {"size":[4,4],"gravity":100,"script":"drive","animation":"idle"}
object beam {"size":[40,4],"solid":true}
object spark {"size":[1,1]}
sprite box {"file":"box.png"}
animation idle {"sprite":"box","frames":[0,0],"fps":7}
animation active {"sprite":"box","frames":[0,0,0],"fps":9}
particle burst {"count":4,"life":18,"speed":1.8,"radius":1,"color":[245,128,90,250]}
script drive
create:
  global creates = global(creates) + 1
step:
  if pressed("fire")
    global presses = global(presses) + 1
    animate active, 1
    after pulse, 5
    emit burst
    spawn spark x, y
  end
  if expired(pulse)
    global pulses = global(pulses) + 1
  end
  move 2 + rand(), 0
---
"#;

#[test]
fn split_replay_matches_an_uninterrupted_world_exactly() {
    let project = Project::parse(GAME).unwrap();
    let game = CompiledGame::new(project).unwrap();
    let mut world = game.preview(None).unwrap();
    let held = Input::from(["fire".into()]);
    for _ in 0..3 {
        world.tick(&held).unwrap();
    }
    let save = world.checkpoint().unwrap();
    let bytes = save.encode().unwrap();
    let restored = game.restore(Checkpoint::decode(&bytes).unwrap()).unwrap();
    assert_eq!(restored.checkpoint().unwrap().encode().unwrap(), bytes);
    let replay: Replay = serde_json::from_str(
        r#"{"ticks":6,"input":[[1,4,"fire"]],"expect":[
        {"at":1,"field":"global/presses","is":1},
        {"at":3,"field":"global/pulses","is":1},
        {"at":6,"field":"global/creates","is":1}
    ]}"#,
    )
    .unwrap();
    let (branch, report) = replay_from(restored, &replay).unwrap();
    assert!(report.passed(), "{report:?}");
    for _ in 0..4 {
        world.tick(&held).unwrap();
    }
    for _ in 0..2 {
        world.tick(&Input::new()).unwrap();
    }
    assert_eq!(
        world.checkpoint().unwrap().encode().unwrap(),
        branch.checkpoint().unwrap().encode().unwrap()
    );
    let media = Media::default();
    assert_eq!(
        capture(&world, &media, [0, 0, 32, 24]).unwrap(),
        capture(&branch, &media, [0, 0, 32, 24]).unwrap()
    );
}

#[test]
fn load_validates_revision_and_entity_state_before_adopting() {
    let mut project = Project::parse(GAME).unwrap();
    let game = CompiledGame::new(project.clone()).unwrap();
    let mut world = game.preview(None).unwrap();
    world.tick(&Input::from(["fire".into()])).unwrap();
    let mut json = serde_json::to_value(world.checkpoint().unwrap()).unwrap();
    json["entities"][0]["animation"][1] = 100.into();
    let bad = Checkpoint::decode(&serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(
        game.restore(bad)
            .err()
            .unwrap()
            .to_string()
            .contains("invalid entity")
    );
    json["version"] = 100.into();
    assert!(Checkpoint::decode(&serde_json::to_vec(&json).unwrap()).is_err());
    let save = world.checkpoint().unwrap();
    project.put(
        "drive".into(),
        Resource::Script("step:\n  move 4, 0".into()),
    );
    let edited = CompiledGame::new(project.clone()).unwrap();
    assert!(
        edited
            .restore(save)
            .err()
            .unwrap()
            .to_string()
            .contains("revision")
    );
    let mut hot = edited.reload_live(&world).unwrap();
    assert_eq!(hot.snapshot(None, None).tick, 1);
    let x = hot.snapshot(None, Some("hero")).entities[0].x;
    hot.tick(&Input::new()).unwrap();
    assert_eq!(hot.snapshot(None, Some("hero")).entities[0].x, x + 4.);
    assert_eq!(hot.snapshot(None, None).globals["creates"], 1.);
    let Some(Resource::Room(room)) = project.get_mut(Kind::Room, "main") else {
        panic!("expected room")
    };
    room.instances[0].2 += 1.;
    assert!(
        CompiledGame::new(project)
            .unwrap()
            .reload_live(&world)
            .is_err()
    );
}
