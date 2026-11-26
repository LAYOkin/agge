use crate::{Camera, MAX_ROOM_DIMENSION};
use crate::{
    FrameGrid, Kind, Project, Resource, Result, Tilemap, asset_path, fail, script, valid_id,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub at: String,
    pub message: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub at: String,
}
#[derive(Debug, Default)]
pub struct Index {
    pub symbols: BTreeMap<String, String>,
    pub edges: Vec<Edge>,
    pub diagnostics: Vec<Diagnostic>,
    pub programs: BTreeMap<String, script::Program>,
}
impl Index {
    fn error(&mut self, code: &'static str, at: &str, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            code,
            at: at.into(),
            message: message.into(),
        });
    }
    fn link(&mut self, from: &str, kind: Kind, id: &str, at: &str) {
        let to = format!("{kind}/{id}");
        self.edges.push(Edge {
            from: from.into(),
            to: to.clone(),
            at: at.into(),
        });
        if !self.symbols.contains_key(&to) {
            self.error(
                "missing_resource",
                from,
                format!("{at} refers to absent {to}"),
            );
        }
    }
    pub fn manifest(project: &Project) -> BTreeMap<String, String> {
        let mut symbols = BTreeMap::new();
        symbols.insert(
            "project".into(),
            digest(&serde_json::to_string(&project.settings).expect("settings serialize")),
        );
        for ((kind, id), resource) in &project.resources {
            symbols.insert(format!("{kind}/{id}"), digest(&resource.encode(false)));
        }
        symbols
    }

    pub fn build(project: &Project) -> Self {
        let mut index = Self {
            symbols: Self::manifest(project),
            ..Self::default()
        };
        index.link("project", Kind::Room, &project.settings.start, "start");
        if !(1..=240).contains(&project.settings.hz)
            || !project
                .settings
                .size
                .into_iter()
                .all(|x| (1..=4096).contains(&x))
        {
            index.error(
                "settings_range",
                "project",
                "size must be 1–4096 per axis and hz must be 1–240",
            );
        }
        for ((kind, id), resource) in &project.resources {
            let key = format!("{kind}/{id}");
            if !valid_id(id) || *kind != resource.kind() {
                index.error(
                    "invalid_symbol",
                    &key,
                    "resource key and data disagree or ID is invalid",
                );
            }
            match resource {
                Resource::Room(room) => {
                    let mut ids = BTreeSet::new();
                    if room.instances.len() > 4096
                        || room.maps.len() > 128
                        || room.lights.len() > 64
                        || room.ui.len() > 128
                    {
                        index.error(
                            "room_limit",
                            &key,
                            "too many instances, maps, lights or labels",
                        );
                    }
                    for instance in &room.instances {
                        index.link(
                            &key,
                            Kind::Object,
                            &instance.1,
                            &format!("instances/{}", instance.0),
                        );
                        if !valid_id(&instance.0)
                            || !ids.insert(instance.0.as_str())
                            || !coord(instance.2)
                            || !coord(instance.3)
                        {
                            index.error(
                                "invalid_instance",
                                &key,
                                format!("invalid or duplicate placement {}", instance.0),
                            );
                        }
                    }
                    let viewport = project.settings.size;
                    if room.size.is_some_and(|size| {
                        size.into_iter()
                            .zip(viewport)
                            .any(|(room, view)| room < view || room > MAX_ROOM_DIMENSION)
                    }) {
                        index.error(
                            "room_size",
                            &format!("{key}/size"),
                            format!("room must contain the viewport and be at most {MAX_ROOM_DIMENSION} pixels per axis"),
                        );
                    }
                    if let Some(follow) = &room.camera.follow {
                        let to = format!("{key}/instances/{follow}");
                        index.edges.push(Edge {
                            from: key.clone(),
                            to: to.clone(),
                            at: "camera/follow".into(),
                        });
                        if !ids.contains(follow.as_str()) {
                            index.error(
                                "camera_target",
                                &format!("{key}/camera/follow"),
                                format!("missing placement {to}"),
                            );
                        }
                    }
                    if !room.camera.at.into_iter().all(coord)
                        || !(0. ..=240.).contains(&room.camera.smooth)
                        || room
                            .camera
                            .dead
                            .into_iter()
                            .zip(viewport)
                            .any(|(dead, view)| !(0. ..=view as f32).contains(&dead))
                    {
                        index.error(
                            "camera_range",
                            &format!("{key}/camera"),
                            "invalid camera origin, dead zone or smoothing",
                        );
                    }
                    for (i, map) in room.maps.iter().enumerate() {
                        index.link(&key, Kind::Tilemap, map, &format!("maps/{i}"));
                    }
                    if let Some(music) = &room.music {
                        index.link(&key, Kind::Audio, music, "music");
                    }
                    for (i, meter) in room.meters.iter().enumerate() {
                        if !meter.at.into_iter().all(coord)
                            || !meter.size.into_iter().all(|v| (0.01..=4096.).contains(&v))
                            || !(0. ..=1_000_000.).contains(&meter.max)
                            || meter.max == 0.
                            || !valid_id(&meter.value)
                        {
                            index.error(
                                "hud_meter",
                                &format!("{key}/meters/{i}"),
                                "invalid meter position, size, maximum or global",
                            );
                        }
                    }
                    if !(0. ..=1.).contains(&room.darkness)
                        || room.lights.iter().any(|light| {
                            !light.at.into_iter().all(coord)
                                || !(1. ..=4096.).contains(&light.radius)
                        })
                        || room.ui.iter().any(|label| {
                            !label.at.into_iter().all(coord)
                                || !(1..=192).contains(&label.size)
                                || !label.value.is_empty() && !valid_id(&label.value)
                        })
                    {
                        index.error(
                            "room_style",
                            &key,
                            "invalid light, ambient darkness or label",
                        );
                    }
                }
                Resource::Object(object) => {
                    for (kind, target) in [
                        (Kind::Sprite, &object.sprite),
                        (Kind::Animation, &object.animation),
                        (Kind::Script, &object.script),
                    ] {
                        if let Some(id) = target {
                            index.link(&key, kind, id, kind.name());
                        }
                    }
                    if object.sprite.is_some() && object.animation.is_some() {
                        index.error("visual_conflict", &key, "choose a sprite or animation");
                    }
                    if object.solid && object.one_way {
                        index.error("collision_mode", &key, "choose a solid or one-way collider");
                    }
                    if !object.size.into_iter().all(|x| (0.01..=4096.).contains(&x))
                        || !coord(object.gravity)
                    {
                        index.error("object_range", &key, "invalid size or gravity");
                    }
                }
                Resource::Sprite(sprite) => {
                    index.file(&key, &sprite.file, &["png"]);
                    if sprite.frames.iter().any(|&[x, y, w, h]| {
                        w == 0 || h == 0 || x.saturating_add(w) > 8192 || y.saturating_add(h) > 8192
                    }) {
                        index.error("sprite_frame", &key, "invalid sprite frame bounds");
                    }
                }
                Resource::Animation(anim) => {
                    index.link(&key, Kind::Sprite, &anim.sprite, "sprite");
                    if anim.frames.is_empty()
                        || !(1..=240).contains(&anim.fps)
                        || matches!(project.get(Kind::Sprite, &anim.sprite), Some(Resource::Sprite(sprite)) if anim.frames.iter().any(|&i| i >= sprite.frame_count()))
                    {
                        index.error(
                            "animation_frames",
                            &key,
                            "invalid frame index, empty animation or fps outside 1–240",
                        );
                    }
                }
                Resource::Tilemap(map) => {
                    if let Some(id) = &map.atlas {
                        index.link(&key, Kind::Sprite, id, "atlas");
                    }
                    let atlas = map
                        .atlas
                        .as_ref()
                        .and_then(|id| project.get(Kind::Sprite, id));
                    for (symbol, tile) in &map.tiles {
                        if let Some(frame) = tile.frame() {
                            let at = format!("{key}/tiles/{symbol}");
                            match atlas {
                                Some(Resource::Sprite(sprite)) if frame >= sprite.frame_count() => {
                                    index.error(
                                        "tile_frame",
                                        &at,
                                        format!("atlas frame {frame} is outside the sprite"),
                                    );
                                }
                                None if map.atlas.is_none() => {
                                    index.error(
                                        "tile_atlas",
                                        &at,
                                        "frame tile requires an atlas sprite",
                                    );
                                }
                                _ => (),
                            }
                        }
                    }
                    let width = map.rows.first().map_or(0, |row| row.chars().count());
                    if !(1..=512).contains(&map.cell)
                        || width == 0
                        || width > 1024
                        || map.rows.is_empty()
                        || map.rows.len() > 1024
                        || map.tiles.keys().any(|c| !c.is_ascii_graphic() || *c == '.')
                        || map.rows.iter().any(|row| {
                            row.chars().count() != width
                                || row.chars().any(|c| c != '.' && !map.tiles.contains_key(&c))
                        })
                    {
                        index.error("tilemap_shape", &key, "map must be rectangular with 1–1024 rows and columns and defined ASCII tiles");
                    }
                }
                Resource::Script(source) => match script::compile(source) {
                    Ok(program) => {
                        for reference in &program.references {
                            index.link(
                                &key,
                                reference.kind,
                                &reference.id,
                                &format!("line/{}", reference.line),
                            );
                        }
                        index.programs.insert(id.clone(), program);
                    }
                    Err(error) => {
                        let message = error.to_string();
                        let (line, detail) = message.split_once(": ").unwrap_or(("1", &message));
                        index.error("script_syntax", &format!("{key}/line/{line}"), detail);
                    }
                },
                Resource::Audio(audio) => {
                    index.file(&key, &audio.file, &["wav", "ogg"]);
                    if !(0. ..=4.).contains(&audio.gain) {
                        index.error("audio_gain", &key, "gain must be 0–4");
                    }
                }
                Resource::Particle(p) => {
                    if !(1..=1024).contains(&p.count)
                        || !(1..=3600).contains(&p.life)
                        || !(0. ..=4096.).contains(&p.speed)
                        || !(0.1..=512.).contains(&p.radius)
                        || !coord(p.gravity)
                    {
                        index.error(
                            "particle_range",
                            &key,
                            "invalid count, lifetime, speed, radius or gravity",
                        );
                    }
                }
            }
        }
        for (script, program) in &index.programs {
            for call in &program.calls {
                let module = call.target.module.as_deref().unwrap_or(script);
                let at = format!("line/{}", call.line);
                index.edges.push(Edge {
                    from: format!("script/{script}"),
                    to: format!("script/{module}/fn/{}", call.target.name),
                    at: at.clone(),
                });
                if call.target.module.is_some()
                    && let Some(target) = index.programs.get(module)
                    && let Some(message) = target.call_error(call)
                {
                    index.diagnostics.push(Diagnostic {
                        code: "script_call",
                        at: format!("script/{script}/{at}"),
                        message,
                    });
                }
            }
        }
        index
    }
    fn file(&mut self, key: &str, file: &str, extensions: &[&str]) {
        if asset_path(file).is_ok_and(|path| {
            path.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|ext| extensions.contains(&ext))
        }) {
            self.edges.push(Edge {
                from: key.into(),
                to: format!("asset/{file}"),
                at: "file".into(),
            });
        } else {
            self.error(
                "asset_path",
                key,
                format!("expected a relative {} asset path", extensions.join("/")),
            );
        }
    }
    pub fn require_valid(&self) -> Result<()> {
        if self.diagnostics.is_empty() {
            return Ok(());
        }
        Err(fail(serde_json::to_string(&self.diagnostics)?))
    }
}
pub fn digest(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}
fn coord(v: f32) -> bool {
    (-1_000_000. ..=1_000_000.).contains(&v)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    Put {
        kind: Kind,
        id: String,
        data: Value,
    },
    Patch {
        kind: Kind,
        id: String,
        fields: BTreeMap<String, Value>,
    },
    Remove {
        kind: Kind,
        id: String,
    },
    Rename {
        kind: Kind,
        id: String,
        to: String,
    },
    Settings {
        fields: BTreeMap<String, Value>,
    },
    Splice {
        id: String,
        at: [usize; 2],
        text: String,
    },
    ScriptEvent {
        id: String,
        event: script::Event,
        #[serde(deserialize_with = "Deserialize::deserialize")]
        body: Option<String>,
    },
    Camera {
        room: String,
        #[serde(default)]
        size: Option<[u32; 2]>,
        #[serde(default)]
        at: [f32; 2],
        #[serde(default)]
        follow: Option<String>,
        #[serde(default)]
        dead: [f32; 2],
        #[serde(default)]
        smooth: f32,
    },
    Place {
        room: String,
        object: String,
        id: String,
        at: [f32; 2],
    },
    Placement {
        room: String,
        id: String,
        to: Option<String>,
        at: Option<[f32; 2]>,
        object: Option<String>,
        #[serde(default)]
        remove: bool,
    },
    Grid {
        room: String,
        object: String,
        prefix: String,
        at: [f32; 2],
        step: [f32; 2],
        size: [u32; 2],
    },
    Slice {
        id: String,
        #[serde(default)]
        at: [u32; 2],
        cell: [u32; 2],
        grid: [u32; 2],
        #[serde(default)]
        gap: [u32; 2],
    },
    Clip {
        id: String,
        sprite: String,
        #[serde(default)]
        first: usize,
        count: usize,
        fps: u32,
        #[serde(default)]
        mode: crate::AnimationMode,
    },
    Atlas {
        map: String,
        sprite: String,
        keys: String,
        #[serde(default)]
        first: usize,
        #[serde(default)]
        solid: String,
    },
    Fill {
        map: String,
        area: [usize; 4],
        tile: char,
        #[serde(default)]
        border: bool,
    },
    Stamp {
        map: String,
        at: [usize; 2],
        rows: Vec<String>,
    },
}

impl Edit {
    pub fn apply(self, project: &mut Project) -> Result<()> {
        match self {
            Self::Put { kind, id, data } => {
                let value = if kind == Kind::Script {
                    data.as_str()
                        .ok_or_else(|| fail("script data must be a string"))?
                        .to_owned()
                } else {
                    data.to_string()
                };
                project.put(id, Resource::parse(kind, &value)?);
            }
            Self::Patch { kind, id, fields } => {
                let resource = project
                    .get(kind, &id)
                    .ok_or_else(|| fail(format!("absent {kind}/{id}")))?;
                let merged = patch(
                    Value::from(serde_json::from_str::<serde_json::Map<String, Value>>(
                        &resource.encode(false),
                    )?),
                    fields,
                )?;
                project.put(id, Resource::parse(kind, &merged.to_string())?);
            }
            Self::Remove { kind, id } => {
                project
                    .resources
                    .remove(&(kind, id.clone()))
                    .ok_or_else(|| fail(format!("absent {kind}/{id}")))?;
            }
            Self::Rename { kind, id, to } => rename(project, kind, &id, &to)?,
            Self::Splice { id, at, text } => {
                let source = script_mut(project, &id)?;
                *source = script::splice_lines(source, at, &text)?;
            }
            Self::ScriptEvent { id, event, body } => {
                let source = script_mut(project, &id)?;
                *source = script::replace_event(source, event, body.as_deref())?;
            }
            Self::Settings { fields } => {
                project.settings = serde_json::from_value(patch(
                    serde_json::to_value(&project.settings)?,
                    fields,
                )?)?
            }
            Self::Camera {
                room,
                size,
                at,
                follow,
                dead,
                smooth,
            } => {
                let room = room_mut(project, &room)?;
                if let Some(size) = size {
                    room.size = Some(size);
                }
                room.camera = Camera {
                    at,
                    follow,
                    dead,
                    smooth,
                };
            }
            Self::Place {
                room,
                object,
                id,
                at,
            } => {
                let room = room_mut(project, &room)?;
                room.instances
                    .push(crate::Instance(id, object, at[0], at[1]));
            }
            Self::Placement {
                room,
                id,
                to,
                at,
                object,
                remove,
            } => {
                let layout = room_mut(project, &room)?;
                let index = layout
                    .instances
                    .iter()
                    .position(|item| item.0 == id)
                    .ok_or_else(|| fail(format!("absent room/{room}/instances/{id}")))?;
                let tracked = layout.camera.follow.as_deref() == Some(&id);
                if remove {
                    if to.is_some() || at.is_some() || object.is_some() {
                        return Err(fail("remove cannot be combined with placement changes"));
                    }
                    layout.instances.remove(index);
                    if tracked {
                        layout.camera.follow = None;
                    }
                } else {
                    let item = &mut layout.instances[index];
                    if let Some(to) = to {
                        if tracked {
                            layout.camera.follow = Some(to.clone());
                        }
                        item.0 = to;
                    }
                    if let Some([x, y]) = at {
                        (item.2, item.3) = (x, y);
                    }
                    if let Some(object) = object {
                        item.1 = object;
                    }
                }
            }
            Self::Grid {
                room,
                object,
                prefix,
                at,
                step,
                size,
            } => {
                let room = room_mut(project, &room)?;
                if size.contains(&0)
                    || size[0].saturating_mul(size[1]) as usize
                        > 4096usize.saturating_sub(room.instances.len())
                {
                    return Err(fail("grid size exceeds room capacity"));
                }
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        room.instances.push(crate::Instance(
                            format!("{prefix}_{x}_{y}"),
                            object.clone(),
                            at[0] + x as f32 * step[0],
                            at[1] + y as f32 * step[1],
                        ));
                    }
                }
            }
            Self::Slice {
                id,
                at,
                cell,
                grid,
                gap,
            } => {
                let Some(Resource::Sprite(sprite)) = project.get_mut(Kind::Sprite, &id) else {
                    return Err(fail(format!("absent sprite/{id}")));
                };
                sprite.frames = FrameGrid {
                    at,
                    cell,
                    grid,
                    gap,
                }
                .frames()?;
            }
            Self::Clip {
                id,
                sprite,
                first,
                count,
                fps,
                mode,
            } => {
                let end = first
                    .checked_add(count)
                    .filter(|_| (1..=4096).contains(&count))
                    .ok_or_else(|| fail("clip requires 1–4096 consecutive frames"))?;
                project.put(
                    id,
                    Resource::Animation(crate::Animation {
                        sprite,
                        frames: (first..end).collect(),
                        fps,
                        mode,
                    }),
                );
            }
            Self::Atlas {
                map,
                sprite,
                keys,
                first,
                solid,
            } => {
                let Some(Resource::Sprite(source)) = project.get(Kind::Sprite, &sprite) else {
                    return Err(fail(format!("absent sprite/{sprite}")));
                };
                let frame_count = source.frame_count();
                map_mut(project, &map)?.set_atlas_palette(
                    &sprite,
                    frame_count,
                    &keys,
                    first,
                    &solid,
                )?;
            }
            Self::Fill {
                map,
                area: [x, y, w, h],
                tile,
                border,
            } => {
                let map = map_mut(project, &map)?;
                let rows = area_mut(map, [x, y, w, h])?;
                if !tile.is_ascii() {
                    return Err(fail("tile must be ASCII"));
                }
                for (i, row) in rows.iter_mut().enumerate() {
                    let replacement = if border && i != 0 && i + 1 != h && w > 2 {
                        format!("{tile}{}{tile}", &row[x + 1..x + w - 1])
                    } else {
                        tile.to_string().repeat(w)
                    };
                    row.replace_range(x..x + w, &replacement);
                }
            }
            Self::Stamp {
                map,
                at: [x, y],
                rows,
            } => {
                let map = map_mut(project, &map)?;
                let width = rows.first().map_or(0, String::len);
                if rows.iter().any(|row| !row.is_ascii() || row.len() != width) {
                    return Err(fail("stamp rows must be rectangular ASCII"));
                }
                for (target, stamp) in area_mut(map, [x, y, width, rows.len()])?
                    .iter_mut()
                    .zip(rows)
                {
                    target.replace_range(x..x + width, &stamp);
                }
            }
        }
        Ok(())
    }
}
fn patch(mut value: Value, fields: BTreeMap<String, Value>) -> Result<Value> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| fail("expected object data"))?;
    for (key, value) in fields {
        if value.is_null() {
            object.remove(&key);
        } else {
            object.insert(key, value);
        }
    }
    Ok(value)
}
fn script_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut String> {
    match project.get_mut(Kind::Script, id) {
        Some(Resource::Script(source)) => Ok(source),
        _ => Err(fail(format!("absent script/{id}"))),
    }
}
fn room_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut crate::Room> {
    match project.get_mut(Kind::Room, id) {
        Some(Resource::Room(room)) => Ok(room),
        _ => Err(fail(format!("absent room/{id}"))),
    }
}
fn map_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut Tilemap> {
    match project.get_mut(Kind::Tilemap, id) {
        Some(Resource::Tilemap(map)) => Ok(map),
        _ => Err(fail(format!("absent tilemap/{id}"))),
    }
}
fn area_mut(map: &mut Tilemap, [x, y, w, h]: [usize; 4]) -> Result<&mut [String]> {
    let rows = map
        .rows
        .get_mut(y..y.saturating_add(h))
        .ok_or_else(|| fail("area exceeds map height"))?;
    if w == 0
        || h == 0
        || rows
            .iter()
            .any(|row| !row.is_ascii() || x.saturating_add(w) > row.len())
    {
        return Err(fail("empty area, non-ASCII map or area exceeds map width"));
    }
    Ok(rows)
}

fn rename(project: &mut Project, kind: Kind, id: &str, to: &str) -> Result<()> {
    if !valid_id(to) || project.get(kind, to).is_some() {
        return Err(fail("rename target is invalid or already exists"));
    }
    let original = project
        .resources
        .remove(&(kind, id.into()))
        .ok_or_else(|| fail(format!("absent {kind}/{id}")))?;
    project.put(to.into(), original);
    if kind == Kind::Room && project.settings.start == id {
        project.settings.start = to.into();
    }
    let replace = |value: &mut String| {
        if value == id {
            *value = to.into();
        }
    };
    for resource in project.resources.values_mut() {
        match resource {
            Resource::Room(room) if kind == Kind::Object => {
                room.instances.iter_mut().for_each(|i| replace(&mut i.1))
            }
            Resource::Room(room) if kind == Kind::Tilemap => room.maps.iter_mut().for_each(replace),
            Resource::Room(room) if kind == Kind::Audio => {
                if let Some(music) = &mut room.music {
                    replace(music);
                }
            }
            Resource::Object(object) => {
                let target = match kind {
                    Kind::Animation => &mut object.animation,
                    Kind::Sprite => &mut object.sprite,
                    Kind::Script => &mut object.script,
                    _ => continue,
                };
                if let Some(target) = target {
                    replace(target);
                }
            }
            Resource::Animation(animation) if kind == Kind::Sprite => {
                replace(&mut animation.sprite)
            }
            Resource::Tilemap(map) if kind == Kind::Sprite => {
                if let Some(atlas) = &mut map.atlas {
                    replace(atlas);
                }
            }
            Resource::Script(source) => {
                let refs = script::parse(source)?.references;
                for reference in refs
                    .into_iter()
                    .rev()
                    .filter(|r| r.kind == kind && r.id == id)
                {
                    let value = if reference.quoted {
                        format!("\"{to}\"")
                    } else {
                        to.into()
                    };
                    source.replace_range(reference.span, &value);
                }
            }
            _ => {}
        }
    }
    Ok(())
}
