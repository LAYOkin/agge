use agge_core::{Edit, Index, Kind, Project, Resource, store::Store};
use serde_json::json;

fn edit(json: serde_json::Value) -> Edit {
    serde_json::from_value(json).unwrap()
}

#[test]
fn grouped_layout_edits_intersect_filters_and_preserve_ids() {
    let mut project = Project::parse(r#"agge/1 {}
room main {"instances":[["row_0","pilot",0,0],["row_1","pilot",8,0],["row_2","other",4,0],["solo","pilot",3,0]],"camera":{"follow":"row_0"}}
object pilot {}
object other {}
tilemap floor {"cell":8,"tiles":{"a":[[0,0,0,255]],"b":[[255,255,255,255],true]},"rows":["aaa","aba","aaa"]}
"#).unwrap();
    edit(json!({"op":"move_placements","room":"main","object":"pilot","prefix":"row_","area":[0,0,8,1],"by":[12,-2]})).apply(&mut project).unwrap();
    edit(json!({"op":"replace_tiles","map":"floor","from":"a","to":"b","area":[1,0,2,2]}))
        .apply(&mut project)
        .unwrap();
    Index::build(&project).require_valid().unwrap();
    let Some(Resource::Room(room)) = project.get(Kind::Room, "main") else {
        panic!("room")
    };
    assert_eq!(
        room.instances
            .iter()
            .map(|i| [i.2, i.3])
            .collect::<Vec<_>>(),
        [[12., -2.], [8., 0.], [4., 0.], [3., 0.]]
    );
    assert_eq!(room.camera.follow.as_deref(), Some("row_0"));
    let Some(Resource::Tilemap(map)) = project.get(Kind::Tilemap, "floor") else {
        panic!("map")
    };
    assert_eq!(map.rows, ["abb", "abb", "aaa"]);
}

#[test]
fn global_rename_follows_script_tokens_and_hud_fields_without_merging_symbols() {
    let mut project = Project::parse(
        r#"agge/1 {}
room main {"ui":[{"at":[0,0],"text":"score","value":"score"}],"meters":[{"at":[0,0],"size":[40,4],"value":"score","max":10}]}
script drive
fn read() = global(score)
create:
  set score = 8
  global score = score + read() # score
  global other = global(score)
---
"#,
    ).unwrap();
    let original = project.encode();
    let conflict = edit(json!({"op":"rename_global","id":"score","to":"other"}));
    assert!(
        conflict
            .apply(&mut project)
            .unwrap_err()
            .to_string()
            .contains("global/other")
    );
    assert_eq!(project.encode(), original);
    edit(json!({"op":"rename_global","id":"score","to":"points"}))
        .apply(&mut project)
        .unwrap();
    let index = Index::build(&project);
    index.require_valid().unwrap();
    let points = index
        .edges
        .iter()
        .filter(|edge| edge.to == "global/points")
        .map(|edge| (edge.from.as_str(), edge.at.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        points,
        [
            ("room/main", "ui/0/value"),
            ("room/main", "meters/0/value"),
            ("script/drive", "line/1"),
            ("script/drive", "line/4"),
            ("script/drive", "line/5"),
        ]
    );
    assert!(index.edges.iter().all(|edge| edge.to != "global/score"));
    let Some(Resource::Script(source)) = project.get(Kind::Script, "drive") else {
        panic!("expected script");
    };
    assert!(source.contains("global points = score + read() # score"));
    let Some(Resource::Room(room)) = project.get(Kind::Room, "main") else {
        panic!("expected room");
    };
    assert_eq!(room.ui[0].text, "score");
}

#[test]
fn music_and_animation_renames_update_resources_and_typed_script_links() {
    let mut project = Project::parse(
        r#"agge/1 {}
room main {"music":"theme"}
audio theme {"file":"audio/music.wav"}
sprite sheet {"file":"art/animation.png","frames":[[0,0,16,16],[16,0,16,16],[32,0,16,16]]}
animation flash {"sprite":"sheet","frames":[0,1,2],"fps":12,"mode":"ping_pong"}
object hero {"script":"drive","animation":"flash"}
script drive
step:
  animate flash, pressed("fire")
  if expired(shield)
    sound theme
  end
---
"#,
    )
    .unwrap();
    for (kind, id, to) in [("audio", "theme", "music"), ("animation", "flash", "pulse")] {
        edit(json!({"op":"rename","kind":kind,"id":id,"to":to}))
            .apply(&mut project)
            .unwrap();
    }
    let index = Index::build(&project);
    index.require_valid().unwrap();
    assert!(
        index
            .edges
            .iter()
            .any(|edge| edge.from == "room/main" && edge.to == "audio/music")
    );
    assert!(
        index
            .edges
            .iter()
            .any(|edge| edge.from == "script/drive" && edge.to == "animation/pulse")
    );
    assert!(index.programs["drive"].symbols.contains_key("timer/shield"));
    let Some(Resource::Animation(clip)) = project.get(Kind::Animation, "pulse") else {
        panic!("missing animation");
    };
    assert_eq!(
        (0..7).map(|i| clip.slot(i).unwrap()).collect::<Vec<_>>(),
        [0, 1, 2, 1, 0, 1, 2]
    );
    assert!(!clip.done(10_000));
}

#[test]
fn oversized_edits_keep_the_original_project_readable() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path().join("test.agge"));
    store.init(&Project::default()).unwrap();
    let base = store.load().unwrap().revision;
    let source = format!("#{}\n", "x".repeat(2 * 1024 * 1024));
    let error = store
        .transact(
            Some(&base),
            vec![edit(json!({
                "op":"put","kind":"script","id":"large","data":source
            }))],
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "project exceeds 2 MiB");
    assert_eq!(store.load().unwrap().revision, base);
    assert!(store.travel(Some(&base), false).is_err());
}

#[test]
fn placement_edits_keep_the_camera_reference_and_other_instances() {
    let mut project = Project::default();
    for op in [
        json!({"op":"put","kind":"object","id":"actor","data":{}}),
        json!({"op":"place","room":"main","id":"hero","object":"actor","at":[12,16]}),
        json!({"op":"place","room":"main","id":"friend","object":"actor","at":[70,80]}),
        json!({"op":"camera","room":"main","follow":"hero"}),
        json!({"op":"placement","room":"main","id":"hero","to":"pilot","at":[24,32]}),
    ] {
        edit(op).apply(&mut project).unwrap();
    }
    Index::build(&project).require_valid().unwrap();
    let room = |project: &Project| project.get(Kind::Room, "main").unwrap().encode(false);
    let updated: serde_json::Value = serde_json::from_str(&room(&project)).unwrap();
    assert_eq!(updated["camera"]["follow"], "pilot");
    assert_eq!(
        updated["instances"],
        json!([
            ["pilot", "actor", 24.0, 32.0],
            ["friend", "actor", 70.0, 80.0]
        ])
    );
    edit(json!({"op":"placement","room":"main","id":"pilot","remove":true}))
        .apply(&mut project)
        .unwrap();
    Index::build(&project).require_valid().unwrap();
    let updated: serde_json::Value = serde_json::from_str(&room(&project)).unwrap();
    assert!(updated.get("camera").is_none());
    assert_eq!(
        updated["instances"],
        json!([["friend", "actor", 70.0, 80.0]])
    );
}

#[test]
fn transaction_rename_history_and_context() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.agge");
    let store = Store::new(&file);
    store.init(&Project::default()).unwrap();
    let old = store.load().unwrap();
    store.delta("agent", true).unwrap();

    let invalid = store.transact(
        Some(&old.revision),
        vec![
            edit(json!({"op":"place","room":"main","id":"ghost","object":"missing","at":[8,16]})),
            edit(json!({"op":"settings","fields":{"title":"Partial"}})),
        ],
    );
    assert!(
        invalid
            .unwrap_err()
            .to_string()
            .contains("missing_resource")
    );
    assert_eq!(old.project, store.load().unwrap().project);

    let created = store.transact(Some(&old.revision), vec![
        edit(json!({"op":"put","kind":"object","id":"player","data":{"script":"drive"}})),
        edit(json!({"op":"put","kind":"script","id":"drive","data":"step:\n  if touch(\"player\")\n    spawn player 1, 2 # player\n  end"})),
        edit(json!({"op":"grid","room":"main","object":"player","prefix":"unit","at":[8,16],"step":[4,8],"size":[2,2]})),
    ]).unwrap();
    assert_eq!(
        created.changed,
        ["object/player", "room/main", "script/drive"]
    );
    assert_eq!(store.delta("agent", true).unwrap().changed, created.changed);
    let renamed = store
        .transact(
            Some(&created.revision),
            vec![edit(
                json!({"op":"rename","kind":"object","id":"player","to":"pilot"}),
            )],
        )
        .unwrap();

    let loaded = store.load().unwrap();
    let refs = Index::build(&loaded.project);
    assert_eq!(
        refs.edges
            .iter()
            .filter(|edge| edge.to == "object/pilot")
            .count(),
        6
    );
    let Some(Resource::Script(source)) = loaded.project.get(Kind::Script, "drive") else {
        panic!()
    };
    assert!(source.contains("touch(\"pilot\")") && source.contains("spawn pilot 1, 2 # player"));
    assert_eq!(
        Project::parse(&loaded.project.encode()).unwrap(),
        loaded.project
    );
    let diff = store.delta("agent", false).unwrap();
    assert_eq!(diff.removed, ["object/player"]);
    assert_eq!(diff.changed, ["object/pilot", "room/main", "script/drive"]);
    assert!(store.travel(Some(&created.revision), false).is_err());
    store
        .remember("controls", Some(json!({"left":"A"})))
        .unwrap();
    assert_eq!(
        Store::new(&file).facts(Some("controls")).unwrap()["controls"],
        json!({"left":"A"})
    );

    assert_eq!(
        store
            .travel(Some(&renamed.revision), false)
            .unwrap()
            .revision,
        created.revision
    );
    assert_eq!(
        store
            .travel(Some(&created.revision), true)
            .unwrap()
            .revision,
        renamed.revision
    );
}

#[test]
fn simultaneous_writers_use_optimistic_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path().join("test.agge"));
    store.init(&Project::default()).unwrap();
    let base = store.load().unwrap().revision;
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            store.transact(
                Some(&base),
                vec![edit(json!({"op":"settings","fields":{"title":"First"}}))],
            )
        });
        let second = scope.spawn(|| {
            store.transact(
                Some(&base),
                vec![edit(json!({"op":"settings","fields":{"title":"Second"}}))],
            )
        });
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .flatten()
            .all(|receipt| receipt.changed == ["project"])
    );
    assert!(results.iter().any(|result| {
        result
            .as_ref()
            .is_err_and(|err| err.to_string().contains("stale revision"))
    }));
    assert_eq!(store.travel(None, false).unwrap().revision, base);
}

#[test]
fn fill_and_stamp_form_compact_collision_map() {
    let mut project = Project::default();
    let commands = [
        json!({"op":"put","kind":"tilemap","id":"tiles","data":{"cell":8,"tiles":{"#":[[1,2,3,255],true]},
            "rows":[".....",".....",".....","....."]}}),
        json!({"op":"fill","map":"tiles","area":[0,0,5,4],"tile":"#","border":true}),
        json!({"op":"stamp","map":"tiles","at":[2,0],"rows":[".","#"]}),
    ];
    for op in commands {
        edit(op).apply(&mut project).unwrap();
    }
    let Some(Resource::Tilemap(map)) = project.get(Kind::Tilemap, "tiles") else {
        panic!()
    };
    assert_eq!(map.rows, ["##.##", "#.#.#", "#...#", "#####"]);
    Index::build(&project).require_valid().unwrap();
}

#[test]
fn flood_replaces_only_connected_cells_and_is_undoable() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path().join("game.agge"));
    let mut project = Project::default();
    edit(json!({"op":"put","kind":"tilemap","id":"tiles","data":{
        "cell":8,"tiles":{"#":[[1,2,3,255],true],"+":[[2,3,4,255],false]},
        "rows":["..#.",".###","#..."]
    }}))
    .apply(&mut project)
    .unwrap();
    store.init(&project).unwrap();
    let initial = store.load().unwrap().revision;
    let flood = |at, tile| edit(json!({"op":"flood","map":"tiles","at":at,"tile":tile}));
    assert!(
        store
            .transact(Some(&initial), vec![flood([4, 0], "+")])
            .is_err()
    );
    assert!(
        store
            .transact(Some(&initial), vec![flood([0, 0], "x")])
            .is_err()
    );
    let changed = store
        .transact(Some(&initial), vec![flood([0, 0], "+")])
        .unwrap();
    let updated = store.load().unwrap();
    let Some(Resource::Tilemap(map)) = updated.project.get(Kind::Tilemap, "tiles") else {
        panic!("missing tilemap");
    };
    assert_eq!(map.rows, ["++#.", "+###", "#..."]);
    let no_op = store
        .transact(Some(&changed.revision), vec![flood([0, 0], "+")])
        .unwrap();
    assert_eq!(no_op.revision, changed.revision);
    assert_eq!(
        store
            .travel(Some(&changed.revision), false)
            .unwrap()
            .revision,
        initial
    );
}

#[test]
fn event_edits_keep_other_events_and_track_symbol_locations() {
    use agge_core::script::{Access, Event, compile, replace_event};

    let source = "# écran\ncreate: # starts\n  set speed = 4\n\n# steps\nstep:\n  move speed, 0\n  # keep this note";
    let updated = replace_event(
        source,
        Event::Step,
        Some("  global score = global(score) + speed\r\n  move speed, 0"),
    )
    .unwrap();
    assert_eq!(
        updated,
        "# écran\ncreate: # starts\n  set speed = 4\n\n# steps\nstep:\n  global score = global(score) + speed\n  move speed, 0\n  # keep this note"
    );

    let program = compile(&updated).unwrap();
    assert_eq!(
        program.symbols["var/speed"],
        [(3, Access::Write), (7, Access::Read), (8, Access::Read)]
    );
    assert_eq!(
        program.symbols["global/score"],
        [(7, Access::Read), (7, Access::Write)]
    );
    let step = &program.locations[&Event::Step];
    assert_eq!((step.line, step.count), (6, 3));
    assert_eq!(
        &updated[step.bytes()],
        "step:\n  global score = global(score) + speed\n  move speed, 0\n"
    );
    assert!(replace_event(source, Event::Step, Some("create:\n  set x = 1")).is_err());
    assert!(replace_event(source, Event::Step, Some("  if true")).is_err());

    let cleared = replace_event("step: # note", Event::Step, Some("")).unwrap();
    assert_eq!(cleared, "step: # note");
    let added = replace_event(&cleared, Event::Create, Some("  set speed = 4")).unwrap();
    assert_eq!(added, "step: # note\ncreate:\n  set speed = 4");
    assert_eq!(replace_event(&added, Event::Create, None).unwrap(), cleared);

    let missing_body =
        serde_json::from_value::<Edit>(json!({"op":"script_event","id":"drive","event":"step"}));
    assert!(missing_body.is_err());
    assert!(matches!(
        edit(json!({"op":"script_event","id":"drive","event":"step","body":null})),
        Edit::ScriptEvent { body: None, .. }
    ));
}

#[test]
fn checkpoints_detect_binary_edits_and_asset_references() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = Store::new(root.join("project.agge"));
    std::fs::write(root.join("ship.png"), b"one").unwrap();
    store.init(&Project::default()).unwrap();
    store
        .transact(
            None,
            vec![edit(
                json!({"op":"put","kind":"sprite","id":"ship","data":{"file":"ship.png"}}),
            )],
        )
        .unwrap();

    let first = store.delta("artist", true).unwrap();
    assert_eq!(
        first.changed,
        ["asset/ship.png", "project", "room/main", "sprite/ship"]
    );
    assert!(store.delta("artist", false).unwrap().changed.is_empty());
    let loaded = store.load().unwrap();
    assert!(
        Index::build(&loaded.project)
            .edges
            .iter()
            .any(|e| e.from == "sprite/ship" && e.to == "asset/ship.png")
    );

    std::fs::write(root.join("ship.png"), b"two").unwrap();
    let changed = store.delta("artist", true).unwrap();
    assert_eq!(changed.changed, ["asset/ship.png"]);
    assert_eq!(changed.revision, first.revision);
    std::fs::remove_file(root.join("ship.png")).unwrap();
    assert_eq!(
        store.delta("artist", true).unwrap().changed,
        ["asset/ship.png"]
    );

    store
        .transact(
            None,
            vec![edit(json!({"op":"remove","kind":"sprite","id":"ship"}))],
        )
        .unwrap();
    assert_eq!(
        store.delta("artist", false).unwrap().removed,
        ["asset/ship.png", "sprite/ship"]
    );
}

#[test]
fn script_splice_is_bounded_and_validates_pending_batches() {
    use agge_core::script::splice_lines;
    let source = "create:\n  # é\n  set x = 1\nstep:\n  move 0, 0";
    assert_eq!(
        splice_lines(source, [3, 1], "  set x = 2\r\n  set y = 4").unwrap(),
        "create:\n  # é\n  set x = 2\n  set y = 4\nstep:\n  move 0, 0"
    );
    assert_eq!(
        splice_lines(source, [5, 1], "").unwrap(),
        "create:\n  # é\n  set x = 1\nstep:"
    );
    assert_eq!(
        splice_lines(source, [6, 0], "  set x = 5").unwrap(),
        format!("{source}\n  set x = 5")
    );
    assert_eq!(splice_lines("", [1, 0], "step:").unwrap(), "step:");
    assert!(splice_lines(source, [0, 1], "").is_err());
    assert!(splice_lines(source, [5, usize::MAX], "").is_err());

    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path().join("game.agge"));
    let mut project = Project::default();
    project.put("drive".into(), Resource::Script(source.into()));
    store.init(&project).unwrap();
    let base = store.load().unwrap().revision;
    let invalid = edit(json!({"op":"splice","id":"drive","at":[5,1],"text":"  if true"}));
    assert!(store.transact(Some(&base), vec![invalid]).is_err());
    assert_eq!(store.load().unwrap().project, project);
    let remove = edit(json!({"op":"splice","id":"drive","at":[4,2],"text":"  set y = 3"}));
    let changed = store.transact(Some(&base), vec![remove]).unwrap();
    assert_eq!(changed.changed, ["script/drive"]);
    assert_ne!(changed.revision, base);
}

#[test]
fn linked_functions_preserve_module_scope_and_rename_references() {
    use agge_core::script::{Access, Event, compile, replace_event};

    let mut project = Project::default();
    project.put(
        "math".into(),
        Resource::Script("fn double(n) = add(n, n)\nfn add(a, b) = a + b".into()),
    );
    let drive = "# keep math.double(n) here\nstep:\n  move math.double(3), 0\nfn bonus(n) = n + 1";
    project.put("drive".into(), Resource::Script(drive.into()));
    let program = compile(drive).unwrap();
    assert_eq!(program.symbols["fn/bonus"], [(4, Access::Write)]);
    assert_eq!(program.locations[&Event::Step].count, 2);
    let edited =
        replace_event(drive, Event::Step, Some("  move math.double(bonus(4)), 0")).unwrap();
    assert!(edited.ends_with("fn bonus(n) = n + 1"));
    project.put("drive".into(), Resource::Script(edited));

    Index::build(&project).require_valid().unwrap();
    edit(json!({"op":"rename","kind":"script","id":"math","to":"numbers"}))
        .apply(&mut project)
        .unwrap();
    let Some(Resource::Script(updated)) = project.get(Kind::Script, "drive") else {
        panic!()
    };
    assert!(updated.starts_with("# keep math.double(n) here"));
    assert!(updated.contains("numbers.double(bonus(4))"));
    let index = Index::build(&project);
    index.require_valid().unwrap();
    assert!(index.edges.iter().any(|edge| {
        edge.from == "script/drive" && edge.to == "script/numbers/fn/double" && edge.at == "line/3"
    }));
}

#[test]
fn function_link_errors_are_reported_before_play() {
    use agge_core::script::compile;

    for source in [
        "fn dupe(a, a) = a",
        "fn named(x) = 1",
        "fn named() = unknown",
        "fn rand(n) = n",
        "step:\n  move absent(), 0",
        "fn one(a) = a\nstep:\n  move one(), 0",
    ] {
        assert!(compile(source).is_err(), "{source}");
    }
    let mut project = Project::default();
    project.put("foo".into(), Resource::Script("fn twice(n) = n * 2".into()));
    project.put(
        "bar".into(),
        Resource::Script("step:\n  move foo.twice(2, 1), missing.zero()".into()),
    );
    let index = Index::build(&project);
    assert!(index.diagnostics.iter().any(|d| d.code == "script_call"
        && d.at == "script/bar/line/2"
        && d.message.contains("foo.twice takes 1 arguments")));
    assert!(
        index
            .diagnostics
            .iter()
            .any(|d| d.code == "missing_resource" && d.message.contains("script/missing"))
    );
}

#[test]
fn atlas_edits_are_compact_ordered_and_transactional() {
    let folder = tempfile::tempdir().unwrap();
    let store = Store::new(folder.path().join("test.agge"));
    store.init(&Project::default()).unwrap();
    let before = store.load().unwrap().revision;
    let after = store.transact(Some(&before), vec![
        edit(json!({"op":"put","kind":"sprite","id":"terrain","data":{"file":"art/terrain.png"}})),
        edit(json!({"op":"slice","id":"terrain","at":[2,3],"cell":[8,8],"grid":[2,2],"gap":[1,2]})),
        edit(json!({"op":"put","kind":"tilemap","id":"layout","data":{"cell":8,"tiles":{"#":[[4,5,6,255],true]},"rows":["#..."]}})),
        edit(json!({"op":"atlas","map":"layout","sprite":"terrain","keys":"xyz","first":1,"solid":"z"})),
        edit(json!({"op":"stamp","map":"layout","at":[1,0],"rows":["xyz"]})),
    ]).unwrap();
    let project = store.load().unwrap().project;
    let Some(Resource::Sprite(sprite)) = project.get(Kind::Sprite, "terrain") else {
        panic!()
    };
    assert_eq!(
        sprite.frames,
        [[2, 3, 8, 8], [11, 3, 8, 8], [2, 13, 8, 8], [11, 13, 8, 8]]
    );
    let Some(Resource::Tilemap(map)) = project.get(Kind::Tilemap, "layout") else {
        panic!()
    };
    assert_eq!(map.atlas.as_deref(), Some("terrain"));
    assert_eq!(map.rows, ["#xyz"]);
    assert_eq!(map.tiles[&'#'].color(), [4, 5, 6, 255]);
    assert!(map.tiles[&'#'].solid() && map.tiles[&'z'].solid());
    assert_eq!(map.tiles[&'x'].frame(), Some(1));
    assert_eq!(map.tiles[&'z'].frame(), Some(3));
    assert!(!map.tiles[&'y'].solid());
    assert_eq!(Project::parse(&project.encode()).unwrap(), project);
    assert!(
        Index::build(&project)
            .edges
            .iter()
            .any(|e| e.from == "tilemap/layout" && e.to == "sprite/terrain" && e.at == "atlas")
    );

    let failed = store
        .transact(
            Some(&after.revision),
            vec![edit(
                json!({"op":"slice","id":"terrain","cell":[8,8],"grid":[1,1]}),
            )],
        )
        .unwrap_err();
    assert!(failed.to_string().contains("tile_frame"));
    assert_eq!(store.load().unwrap().revision, after.revision);
    let renamed = store
        .transact(
            Some(&after.revision),
            vec![edit(
                json!({"op":"rename","kind":"sprite","id":"terrain","to":"deck"}),
            )],
        )
        .unwrap();
    assert!(renamed.changed.iter().any(|id| id == "tilemap/layout"));
    assert!(
        store
            .load()
            .unwrap()
            .project
            .encode()
            .contains(r#""atlas":"deck""#)
    );
}

#[test]
fn atlas_bounds_and_legacy_palette_entries_are_validated() {
    let invalid = agge_core::FrameGrid {
        at: [0, 0],
        cell: [1, 1],
        grid: [10, 10],
        gap: [u32::MAX, 0],
    };
    assert!(invalid.frames().is_err());
    let legacy = serde_json::from_str::<agge_core::Tile>("[[7,8,9,255]]").unwrap();
    assert_eq!(serde_json::to_string(&legacy).unwrap(), "[[7,8,9,255]]");
    assert!(!legacy.solid());
    let mut project = Project::default();
    project.put(
        "layout".into(),
        Resource::parse(
            Kind::Tilemap,
            r#"{"cell":16,"tiles":{"a":[0]},"rows":["a"]}"#,
        )
        .unwrap(),
    );
    assert!(
        Index::build(&project)
            .diagnostics
            .iter()
            .any(|e| e.code == "tile_atlas")
    );
    let Some(Resource::Tilemap(map)) = project.get_mut(Kind::Tilemap, "layout") else {
        panic!()
    };
    assert!(map.set_atlas_palette("sprite", 3, "aa", 0, "").is_err());
    assert!(map.set_atlas_palette("sprite", 3, "a.", 0, "").is_err());
    assert!(map.set_atlas_palette("sprite", 3, "a", 0, "b").is_err());
    assert!(
        map.set_atlas_palette("sprite", 3, "a", usize::MAX, "")
            .is_err()
    );
    map.set_atlas_palette("sprite", 3, "a", 0, "").unwrap();
    assert!(map.set_atlas_palette("other", 3, "b", 0, "").is_err());
    assert_eq!(map.atlas.as_deref(), Some("sprite"));
}

#[test]
fn camera_configuration_links_to_the_room_placement() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path().join("game.agge"));
    store.init(&Project::default()).unwrap();
    let base = store.load().unwrap().revision;
    let staged = store.transact(Some(&base), vec![
        edit(json!({"op":"camera","room":"main","size":[1280,600],"follow":"hero","dead":[80,40],"smooth":12})),
        edit(json!({"op":"put","kind":"object","id":"pilot","data":{}})),
        edit(json!({"op":"place","room":"main","object":"pilot","id":"hero","at":[600,100]})),
    ]).unwrap();
    let loaded = store.load().unwrap();
    assert!(Index::build(&loaded.project).edges.iter().any(|edge| {
        edge.from == "room/main"
            && edge.to == "room/main/instances/hero"
            && edge.at == "camera/follow"
    }));
    let Some(Resource::Room(room)) = loaded.project.get(Kind::Room, "main") else {
        panic!()
    };
    assert_eq!(room.dimensions(loaded.project.settings.size), [1280, 600]);
    assert_eq!(room.camera.follow.as_deref(), Some("hero"));
    for (op, code) in [
        (
            json!({"op":"camera","room":"main","follow":"unknown"}),
            "camera_target",
        ),
        (
            json!({"op":"camera","room":"main","size":[12,24]}),
            "room_size",
        ),
        (
            json!({"op":"camera","room":"main","dead":[641,0]}),
            "camera_range",
        ),
    ] {
        assert!(
            store
                .transact(Some(&staged.revision), vec![edit(op)])
                .unwrap_err()
                .to_string()
                .contains(code)
        );
        assert_eq!(store.load().unwrap().revision, staged.revision);
    }
    store
        .transact(
            Some(&staged.revision),
            vec![edit(json!({"op":"camera","room":"main"}))],
        )
        .unwrap();
    let project = store.load().unwrap().project;
    let Some(Resource::Room(room)) = project.get(Kind::Room, "main") else {
        panic!()
    };
    assert_eq!(room.size, Some([1280, 600]));
    assert_eq!(room.camera, agge_core::Camera::default());
    assert!(!project.encode().contains(r#""camera":"#));
}
