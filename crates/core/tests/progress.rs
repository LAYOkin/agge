use agge_core::{Edit, Index, Progress, ProgressSchema, ProgressSlot, Project};
use serde_json::json;

fn schema() -> ProgressSchema {
    serde_json::from_value(json!({
        "id":"works","version":3,"globals":{"stars":0,"rating":1},
        "migrations":{
            "1":{"rename":{"coins":"score"},"remove":["retired"]},
            "2":{"rename":{"score":"stars"}}
        }
    }))
    .unwrap()
}

fn legacy() -> Progress {
    serde_json::from_value(json!({
        "id":"works","version":1,"values":{"coins":17,"retired":99}
    }))
    .unwrap()
}

#[test]
fn migration_chains_keep_values_and_refuse_ambiguous_or_future_saves() {
    let schema = schema();
    schema.validate().unwrap();
    let mut progress = legacy();
    let values = schema.initial_values(Some(&progress)).unwrap();
    assert_eq!(
        values,
        [("rating".into(), 1.), ("stars".into(), 17.)].into()
    );
    progress.values.insert("score".into(), 8.);
    assert!(
        schema
            .initial_values(Some(&progress))
            .unwrap_err()
            .to_string()
            .contains("overwrite score")
    );
    progress.version = 4;
    assert!(schema.initial_values(Some(&progress)).is_err());
    progress.version = 1;
    progress.id = "different".into();
    assert!(schema.initial_values(Some(&progress)).is_err());
}

#[test]
fn slot_publishes_atomically_and_refuses_stale_sessions_and_corrupt_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profiles/first.json");
    let schema = schema();
    let (mut first, none) = ProgressSlot::open(path.clone(), &schema).unwrap();
    let (mut stale, _) = ProgressSlot::open(path.clone(), &schema).unwrap();
    assert!(none.is_none());
    let initial = schema.initial_values(Some(&legacy())).unwrap();
    let mut next = schema.capture(&initial);
    first.write(&next).unwrap();
    next.values.insert("stars".into(), 21.);
    assert!(
        stale
            .write(&next)
            .unwrap_err()
            .to_string()
            .contains("another session")
    );
    first.write(&next).unwrap();
    let (_, current) = ProgressSlot::open(path.clone(), &schema).unwrap();
    assert_eq!(current, Some(next));
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(ProgressSlot::open(path.clone(), &schema).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"corrupt");
}

#[test]
fn typed_global_rename_adds_a_migration_for_existing_players() {
    let mut project = Project::parse(
        r#"agge/1 {"progress":{"id":"works","version":1,"globals":{"coins":0}}}
room main {"meters":[{"at":[16,32],"size":[96,8],"value":"coins","max":100}]}
script award
create:
  global coins = global(coins) + 1
  save
---
"#,
    )
    .unwrap();
    assert!(
        Index::build(&project)
            .edges
            .iter()
            .any(|edge| { edge.from == "project" && edge.to == "global/coins" })
    );
    Edit::RenameGlobal {
        id: "coins".into(),
        to: "medals".into(),
    }
    .apply(&mut project)
    .unwrap();
    Index::build(&project).require_valid().unwrap();
    let schema = project.settings.progress.as_ref().unwrap();
    let old = Progress {
        id: "works".into(),
        version: 1,
        values: [("coins".into(), 17.)].into(),
    };
    assert_eq!(schema.version, 2);
    assert_eq!(schema.initial_values(Some(&old)).unwrap()["medals"], 17.);
    assert!(
        project
            .encode()
            .contains("global medals = global(medals) + 1")
    );
}
