use agge_core::{Edit, Index, Kind, Project, Resource, pixel::Renderer, store::Store};
use serde_json::json;

const ASSEMBLY: &str = r#"agge/1 {}
room main {}
palette bright {"swatches":{"k":"162436","a":"31627a","b":"569894","c":"b9e7c4"},"ramps":["abc"],"ink":"k"}
palette warm {"swatches":{"k":"292037","a":"794357","b":"b37269","c":"f7bc8a"},"ramps":["abc"],"ink":"k"}
pixel clasp
canvas 3 2
style bright
layer metal
  rows 0 0 ab./c2.
---
pixel coat
canvas 8 6
style bright
layer seam
  use clasp:base 0 0 r90
  mirror x 0 0 2 3
layer fabric
  box b 1 1 3 3
  shade 1 1 3 3
  outline 0 1 3 3
frame still
frame raised still
layer fabric
  shift 0 -1
tween rise still 4 0 -3 fabric
---
pixel amber
canvas 8 6
style warm
base coat
layer trim
  px c 7 5
---
sprite coat {"pixel":"coat","poses":["rise_1","still","base"]}
animation bounce {"sprite":"coat","frames":[1,0,1,2],"fps":8}
"#;

fn edit(value: serde_json::Value) -> Edit {
    serde_json::from_value(value).unwrap()
}

#[test]
fn directional_ramp_dithers_only_the_selected_material() {
    let project = Project::parse(
        r#"agge/1 {}
room main {}
palette finish {"swatches":{"a":"182539","b":"38617a","c":"a9d1c2","k":"161b21"}}
pixel panel
canvas 5 4
style finish
layer metal
  rows 0 0 5b/5b/5b/bk.bb
  ramp abc 0 0 5 4 1 0 b
---
"#,
    )
    .unwrap();
    let art = Renderer::new(&project).render("panel").unwrap();
    let pixels = art.pixels(None, None, None).unwrap();
    assert_eq!(&pixels[15..], &[b'a', b'k', 0, b'c', b'c']);
    assert_eq!([pixels[0], pixels[2], pixels[4]], *b"abc");
    assert_eq!([pixels[1], pixels[6]], *b"ab");
}

#[test]
fn layers_reuse_rotated_parts_and_inherit_palette_glyphs() {
    let project = Project::parse(ASSEMBLY).unwrap();
    let index = Index::build(&project);
    index.require_valid().unwrap();
    let art = &index.pixels["coat"];
    let seam = art
        .pixels(Some("base"), Some("seam"), Some([0, 0, 4, 3]))
        .unwrap();
    assert_eq!(seam, b"caac\0bb\0\0\0\0\0");
    assert_eq!(
        art.frame_names().collect::<Vec<_>>(),
        ["still", "raised", "rise_0", "rise_1", "rise_2", "rise_3"]
    );
    assert_eq!(index.sprite_frame_count(&project, "coat"), Some(3));
    let amber = &index.pixels["amber"];
    assert_eq!(
        amber.layers(None).unwrap().collect::<Vec<_>>(),
        ["seam", "fabric", "trim"]
    );
    assert_eq!(
        amber
            .pixels(None, Some("seam"), Some([0, 0, 4, 3]))
            .unwrap(),
        seam
    );
    assert_ne!(amber.palette.color(b'b'), art.palette.color(b'b'));
    assert_eq!(
        amber.pixels(None, None, Some([7, 5, 1, 1])).unwrap(),
        [b'c']
    );
    assert_eq!(art.pixels(None, None, Some([7, 5, 1, 1])).unwrap(), [0]);
}

#[test]
fn shade_and_outline_use_pre_edit_neighbors_and_stay_in_the_region() {
    let project = Project::parse(ASSEMBLY).unwrap();
    let art = Renderer::new(&project).render("coat").unwrap();
    let key = |frame, x, y| {
        art.pixels(Some(frame), Some("fabric"), Some([x, y, 1, 1]))
            .unwrap()[0]
    };
    assert_eq!(key("still", 1, 1), b'c');
    assert_eq!(key("still", 3, 3), b'a');
    assert_eq!(key("still", 0, 1), b'k');
    assert_eq!(key("still", 1, 0), 0);
    assert_eq!(key("still", 4, 1), 0);
    assert_eq!(key("raised", 1, 0), b'c');
    assert_eq!(key("rise_0", 1, 1), b'c');
    assert_eq!(key("rise_3", 3, 0), b'a');
    assert!(art.pixels(None, None, Some([usize::MAX, 0, 1, 1])).is_err());
}

#[test]
fn patch_creates_local_edits_and_replaces_only_the_target_layer_block() {
    let mut project = Project::parse(ASSEMBLY).unwrap();
    edit(json!({"op":"pixel_patch","id":"coat","frame":"still","layer":"fabric","draw":"clear 1 1 2 1\npx a 5 3"})).apply(&mut project).unwrap();
    let art = Renderer::new(&project).render("coat").unwrap();
    let pair = |art: &agge_core::pixel::Art, frame| {
        art.pixels(Some(frame), Some("fabric"), Some([1, 1, 2, 1]))
            .unwrap()
    };
    assert_eq!(pair(&art, "still"), [0, 0]);
    assert_ne!(pair(&art, "base"), [0, 0]);
    assert_eq!(
        art.pixels(Some("still"), None, Some([5, 3, 1, 1])).unwrap(),
        [b'a']
    );
    edit(json!({"op":"pixel_patch","id":"coat","frame":"still","layer":"fabric","replace":true,"draw":"px c 5 3"})).apply(&mut project).unwrap();
    let art = Renderer::new(&project).render("coat").unwrap();
    assert_eq!(pair(&art, "still"), pair(&art, "base"));
    assert_eq!(
        art.pixels(Some("still"), None, Some([5, 3, 1, 1])).unwrap(),
        [b'c']
    );
    let no_generated = edit(
        json!({"op":"pixel_patch","id":"coat","frame":"rise_2","layer":"fabric","draw":"px a 0 0"}),
    );
    assert!(
        no_generated
            .apply(&mut project)
            .unwrap_err()
            .to_string()
            .contains("authored frame")
    );
    assert!(
        edit(json!({"op":"pixel_patch","id":"coat","layer":"fabric","draw":"frame injected"}))
            .apply(&mut project)
            .is_err()
    );
}

#[test]
fn typed_renames_preserve_reference_suffixes_and_pixel_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path().join("game.agge"));
    store.init(&Project::parse(ASSEMBLY).unwrap()).unwrap();
    let base = store.load().unwrap();
    let receipt = store
        .transact(
            Some(&base.revision),
            vec![
                edit(json!({"op":"rename","kind":"pixel","id":"clasp","to":"buckle"})),
                edit(json!({"op":"rename","kind":"pixel","id":"coat","to":"jacket"})),
                edit(json!({"op":"rename","kind":"palette","id":"bright","to":"sea"})),
            ],
        )
        .unwrap();
    let candidate = store.load().unwrap().project;
    let index = Index::build(&candidate);
    index.require_valid().unwrap();
    let Some(Resource::Pixel(source)) = candidate.get(Kind::Pixel, "jacket") else {
        panic!("absent renamed artwork")
    };
    assert!(source.contains("use buckle:base"));
    for (from, to) in [
        ("pixel/amber", "pixel/jacket"),
        ("sprite/coat", "pixel/jacket"),
        ("pixel/jacket", "palette/sea"),
    ] {
        assert!(
            index
                .edges
                .iter()
                .any(|edge| edge.from == from && edge.to == to)
        );
    }
    let invalid =
        edit(json!({"op":"pixel_patch","id":"jacket","layer":"fabric","draw":"rows 0 0 257a"}));
    assert!(
        store
            .transact(Some(&receipt.revision), vec![invalid])
            .unwrap_err()
            .to_string()
            .contains("row runs")
    );
    assert_eq!(store.load().unwrap().revision, receipt.revision);
    assert_eq!(
        store
            .travel(Some(&receipt.revision), false)
            .unwrap()
            .revision,
        base.revision
    );
}

#[test]
fn rejects_cycles_missing_swatches_and_bad_pose_links() {
    let mut project = Project::parse(ASSEMBLY).unwrap();
    project.put(
        "clasp".into(),
        Resource::Pixel("canvas 3 2\nstyle bright\nlayer metal\n  use coat 0 0".into()),
    );
    let index = Index::build(&project);
    assert!(
        index
            .diagnostics
            .iter()
            .any(|d| d.message.contains("dependency cycle"))
    );
    let mut project = Project::parse(ASSEMBLY).unwrap();
    project.put(
        "clasp".into(),
        Resource::Pixel("canvas 3 2\nstyle bright\nlayer metal\n  px z 1 0".into()),
    );
    let index = Index::build(&project);
    assert!(
        index
            .diagnostics
            .iter()
            .any(|d| d.at == "pixel/clasp/line/4" && d.message.contains("swatch z"))
    );
    let mut project = Project::parse(ASSEMBLY).unwrap();
    edit(json!({"op":"patch","kind":"sprite","id":"coat","fields":{"poses":["missing"]}}))
        .apply(&mut project)
        .unwrap();
    assert!(
        Index::build(&project)
            .diagnostics
            .iter()
            .any(|d| d.code == "sprite_pose")
    );
}

#[test]
fn motion_moves_tracks_independently_and_can_feed_authored_poses() {
    let mut project = Project::parse(ASSEMBLY).unwrap();
    let end = project
        .get(Kind::Pixel, "coat")
        .unwrap()
        .encode(false)
        .lines()
        .count()
        + 1;
    edit(json!({"op":"splice","kind":"pixel","id":"coat","at":[end,0],"text":"motion sweep still seam=0,0/1,0/0,1 fabric=0,0/0,-1/1,0\nframe final sweep_1\nlayer tag\n  px k 6 0"}))
        .apply(&mut project).unwrap();
    let art = Renderer::new(&project).render("coat").unwrap();
    let crop = |frame, layer, x, y| {
        art.pixels(Some(frame), Some(layer), Some([x, y, 1, 1]))
            .unwrap()[0]
    };
    assert_eq!(crop("sweep_0", "seam", 0, 0), b'c');
    assert_eq!(crop("sweep_1", "seam", 0, 0), 0);
    assert_eq!(crop("sweep_1", "seam", 1, 0), b'c');
    assert_eq!(crop("sweep_1", "fabric", 1, 0), b'c');
    assert_eq!(crop("sweep_2", "fabric", 2, 1), b'c');
    assert_eq!(crop("final", "tag", 6, 0), b'k');
}

#[test]
fn primitives_support_exact_contours_regional_copy_and_erasures() {
    let mut project = Project::parse(ASSEMBLY).unwrap();
    project.put(
        "panel".into(),
        Resource::Pixel(
            r#"canvas 11 8
style bright
layer paint
  rect k 1 1 5 4
  flood b 2 2
  rows 2 2 .-c
  copy 1 1 5 4 6 0 r90
  dither a c 1 5 2 2
  poly b 6,5 10,5 8,8
"#
            .into(),
        ),
    );
    let art = Renderer::new(&project).render("panel").unwrap();
    let crop = |area| art.pixels(None, None, Some(area)).unwrap();
    assert_eq!(crop([1, 2, 5, 1]), b"kb\0ck");
    assert_eq!(crop([6, 0, 4, 1]), b"kkkk");
    assert_eq!(crop([6, 2, 4, 1]), b"kb\0k");
    assert_eq!(crop([1, 5, 2, 2]), b"acca");
    assert_eq!(crop([7, 5, 2, 1]), b"bb");
}
