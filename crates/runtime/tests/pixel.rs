use agge_core::{Index, Project};
use agge_runtime::{Media, capture, replay};
use std::{path::Path, sync::Arc};

#[test]
fn generated_pose_sheets_are_shared_and_render_in_atlases_and_clips() {
    let project = Project::parse(
        r#"agge/1 {"size":[8,4],"hz":2}
room main {"bg":[0,0,0,0],"maps":["map"],"instances":[["hero","hero",4,0]]}
object hero {"size":[2,2],"animation":"flash"}
sprite foil {"pixel":"foil","poses":["glow","still"]}
sprite alias {"pixel":"foil","poses":["glow","still"]}
tilemap map {"cell":2,"atlas":"alias","rows":["a"],"tiles":{"a":[1,true]}}
animation flash {"sprite":"foil","fps":2,"frames":[0,1]}
palette neon {"swatches":{"a":"cc8844","b":"33aaff80"}}
pixel foil
canvas 2 2
style neon
layer foil
  rows 0 0 a./.a
frame still
frame glow still
layer foil
  recolor a b 0 0 2 2
---
"#,
    )
    .unwrap();
    assert!(project.asset_paths().is_empty());
    let index = Index::build(&project);
    index.require_valid().unwrap();
    let media = Media::load_indexed(&project, Path::new("."), &index).unwrap();
    assert!(Arc::ptr_eq(&media.sprites["alias"], &media.sprites["foil"]));
    assert_eq!(media.frame(&project, "foil", 1).unwrap().0, [2, 0, 2, 2]);
    assert!(media.frame(&project, "foil", 2).is_none());
    for (ticks, rgba) in [(0, [51, 170, 255, 128]), (1, [204, 136, 68, 255])] {
        let spec = serde_json::from_value(serde_json::json!({
            "ticks":ticks,"expect":[{"at":ticks,"field":"frame/hero","is":ticks}]
        }))
        .unwrap();
        let (world, report) = replay(project.clone(), &spec).unwrap();
        assert!(report.passed());
        let image = image::load_from_memory(&capture(&world, &media, [0, 0, 8, 4]).unwrap())
            .unwrap()
            .into_rgba8();
        assert_eq!(image.get_pixel(4, 0).0, rgba);
        assert_eq!(image.get_pixel(0, 0).0, [204, 136, 68, 255]);
        assert_eq!(image.get_pixel(5, 0).0, [0, 0, 0, 0]);
    }
}
