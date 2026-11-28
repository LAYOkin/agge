use agge_core::Project;
use agge_runtime::{CompiledGame, Input, Replay, replay_compiled};

const GAME: &str = r#"agge/1 {"progress":{"id":"harbor","version":2,"globals":{"stars":0,"chapter":1},"migrations":{"1":{"rename":{"shells":"stars"}}}}}
room main {"instances":[["office","office",0,0]]}
object office {"script":"award"}
script award
create:
  global initial = global(stars)
step:
  if pressed("fire")
    save
    global stars = global(stars) + 1
    global transient = 901
  end
  if pressed("confirm")
    save
    global error = 1 / 0
  end
---
"#;

#[test]
fn old_progress_loads_before_create_and_saves_only_declared_final_tick_values() {
    let project = Project::parse(GAME).unwrap();
    let game = CompiledGame::new(project).unwrap();
    let replay: Replay = serde_json::from_str(
        r#"{"ticks":2,"progress":{"id":"harbor","version":1,"values":{"shells":12}},
        "input":[[2,2,"fire"]],"expect":[
          {"at":0,"field":"global/initial","is":12},
          {"at":0,"field":"global/chapter","is":1},
          {"at":1,"field":"progress_saved","is":0},
          {"at":2,"field":"progress_saved","is":1},
          {"at":2,"field":"global/transient","is":901}
        ]}"#,
    )
    .unwrap();
    let (world, report) = replay_compiled(&game, &replay).unwrap();
    assert!(report.passed(), "{report:?}");
    let save = world.progress_to_save().unwrap();
    assert_eq!(save.version, 2);
    assert_eq!(
        save.values,
        [("chapter".into(), 1.), ("stars".into(), 13.)].into()
    );
    let mut restored = game.restore(world.checkpoint().unwrap()).unwrap();
    assert_eq!(restored.progress_to_save(), Some(save));
    restored.tick(&Input::new()).unwrap();
    assert!(restored.progress_to_save().is_none());
    restored.tick(&Input::from(["confirm".into()])).unwrap_err();
    assert!(restored.progress_to_save().is_none());
}

#[test]
fn save_is_validated_at_compile_time() {
    let source = GAME.replacen(GAME.lines().next().unwrap(), "agge/1 {}", 1);
    assert!(CompiledGame::new(Project::parse(&source).unwrap()).is_err());
}
