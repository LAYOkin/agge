mod camera;
mod solids;
mod vm;

use agge_core::{Index, Kind, Object, Project, Resource, Result, Room, fail, script::Event};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

pub type Input = BTreeSet<String>;
type Queued = (String, Option<String>, f32, f32);
const ENTITY_LIMIT: usize = 4096;
const PARTICLE_LIMIT: usize = 8192;

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}
impl Bounds {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        }
    }
    pub fn overlaps(self, other: Self) -> bool {
        self.left < other.right
            && self.right > other.left
            && self.top < other.bottom
            && self.bottom > other.top
    }
    fn swept(self, axis: usize, amount: f32) -> Self {
        let mut result = self;
        if axis == 0 {
            result.left += amount.min(0.);
            result.right += amount.max(0.);
        } else {
            result.top += amount.min(0.);
            result.bottom += amount.max(0.);
        }
        result
    }
    fn clamp_motion(self, obstacle: Self, axis: usize, delta: f32) -> f32 {
        let (min, max, near, far, cross) = if axis == 0 {
            (
                self.left,
                self.right,
                obstacle.left,
                obstacle.right,
                self.top < obstacle.bottom && self.bottom > obstacle.top,
            )
        } else {
            (
                self.top,
                self.bottom,
                obstacle.top,
                obstacle.bottom,
                self.left < obstacle.right && self.right > obstacle.left,
            )
        };
        if !cross {
            return delta;
        }
        if delta > 0. && max <= near {
            delta.min(near - max)
        } else if delta < 0. && min >= far {
            delta.max(far - min)
        } else {
            delta
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Entity {
    pub id: u64,
    pub object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, f32>,
    #[serde(skip)]
    pub born: u64,
    #[serde(skip)]
    dead: bool,
}
impl Entity {
    pub fn bounds(&self, object: &Object) -> Bounds {
        Bounds::new(self.x, self.y, object.size[0], object.size[1])
    }
}
#[derive(Clone, Debug)]
pub struct Spark {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub age: u32,
    pub style: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct State<'a> {
    pub tick: u64,
    pub room: &'a str,
    #[serde(skip_serializing_if = "agge_core::zero_position")]
    pub camera: [f32; 2],
    pub globals: &'a BTreeMap<String, f32>,
    pub entities: Vec<&'a Entity>,
    pub particles: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct Fault {
    pub tick: u64,
    pub entity: u64,
    pub event: &'static str,
    pub message: String,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Box<FaultOrigin>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FaultOrigin {
    pub object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
    pub source: ScriptLocation,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stack: Vec<ScriptFrame>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted_frames: Option<usize>,
}

impl Fault {
    pub fn location(&self) -> Option<(&str, usize)> {
        let origin = self.origin.as_ref()?;
        Some(match origin.stack.last() {
            Some(frame) => (&frame.script, frame.line),
            None => (&origin.source.script, origin.source.line),
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ScriptLocation {
    pub script: String,
    pub line: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScriptFrame {
    pub script: String,
    pub function: String,
    pub line: usize,
}
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tick {} entity {} {}",
            self.tick, self.entity, self.event
        )?;
        if let Some(origin) = &self.origin {
            let source = &origin.source;
            write!(f, " in {}:{}", source.script, source.line)?;
        }
        write!(f, ": {}", self.message)?;
        if let Some(frame) = self.origin.as_ref().and_then(|origin| origin.stack.last()) {
            write!(
                f,
                " (in {}.{}:{})",
                frame.script, frame.function, frame.line
            )?;
        }
        Ok(())
    }
}
impl std::error::Error for Fault {}

pub struct World {
    pub(crate) project: Project,
    index: Arc<Index>,
    room: String,
    tick: u64,
    next_id: u64,
    pub(crate) entities: BTreeMap<u64, Entity>,
    objects: BTreeMap<String, Object>,
    members: BTreeMap<String, BTreeSet<u64>>,
    placements: BTreeMap<String, u64>,
    camera: camera::ViewCamera,
    solids: solids::SolidIndex,
    pub(crate) globals: BTreeMap<String, f32>,
    pub(crate) particles: Vec<Spark>,
    audio: Vec<String>,
    input: Input,
    previous: Input,
    queued: VecDeque<Queued>,
    next_room: Option<String>,
    rng: u64,
    fx_rng: u64,
    budget: u32,
    fault: Option<Fault>,
}
impl World {
    pub fn new(project: Project, room: Option<&str>) -> Result<Self> {
        let world = Self::preview(project, room)?;
        if let Some(fault) = world.fault() {
            return Err(Box::new(fault.clone()));
        }
        Ok(world)
    }

    /// Construct a world for inspection, retaining the partial state if a create event fails.
    pub fn preview(project: Project, room: Option<&str>) -> Result<Self> {
        let mut world = Self::prepare(project, room)?;
        world.fault = world.enter_room().err();
        Ok(world)
    }

    pub fn fault(&self) -> Option<&Fault> {
        self.fault.as_ref()
    }

    fn prepare(project: Project, room: Option<&str>) -> Result<Self> {
        let index = Index::build(&project);
        index.require_valid()?;
        let room = room.unwrap_or(&project.settings.start).to_owned();
        if project.get(Kind::Room, &room).is_none() {
            return Err(fail(format!("absent room/{room}")));
        }
        let seed = project.settings.seed;
        let objects = project
            .resources
            .iter()
            .filter_map(|((_, id), resource)| {
                if let Resource::Object(object) = resource {
                    Some((id.clone(), object.clone()))
                } else {
                    None
                }
            })
            .collect();
        Ok(Self {
            project,
            index: Arc::new(index),
            room,
            tick: 0,
            next_id: 1,
            entities: BTreeMap::new(),
            objects,
            members: BTreeMap::new(),
            placements: BTreeMap::new(),
            camera: camera::ViewCamera::default(),
            solids: solids::SolidIndex::default(),
            globals: BTreeMap::new(),
            particles: vec![],
            audio: vec![],
            input: Input::new(),
            previous: Input::new(),
            queued: VecDeque::new(),
            next_room: None,
            rng: seed.max(1),
            fx_rng: (seed ^ 0xA0761D6478BD642F).max(1),
            budget: 250_000,
            fault: None,
        })
    }
    pub(crate) fn snapshot_tick(&self) -> u64 {
        self.tick
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn room(&self) -> &Room {
        match self.project.get(Kind::Room, &self.room) {
            Some(Resource::Room(room)) => room,
            _ => unreachable!(),
        }
    }
    pub fn room_id(&self) -> &str {
        &self.room
    }
    pub fn camera(&self) -> [f32; 2] {
        self.camera.origin
    }
    pub fn view(&self) -> Bounds {
        let [x, y] = self.camera();
        let [w, h] = self.project.settings.size;
        Bounds::new(x, y, w as f32, h as f32)
    }
    pub(crate) fn placed(&self, id: &str) -> Option<&Entity> {
        self.entities.get(self.placements.get(id)?)
    }
    pub(crate) fn count(&self, object: &str) -> usize {
        self.members.get(object).map_or(0, BTreeSet::len)
    }
    pub fn snapshot(&self, object: Option<&str>, placement: Option<&str>) -> State<'_> {
        let entities = if let Some(placement) = placement {
            self.placed(placement)
                .filter(|entity| object.is_none_or(|name| name == entity.object))
                .into_iter()
                .collect()
        } else if let Some(object) = object {
            self.members
                .get(object)
                .into_iter()
                .flatten()
                .filter_map(|id| self.entities.get(id))
                .collect()
        } else {
            self.entities.values().collect()
        };
        State {
            tick: self.tick,
            room: &self.room,
            camera: self.camera(),
            globals: &self.globals,
            entities,
            particles: self.particles.len(),
        }
    }
    pub fn drain_audio(&mut self) -> Vec<String> {
        std::mem::take(&mut self.audio)
    }
    pub(crate) fn object(&self, name: &str) -> &Object {
        &self.objects[name]
    }
    pub fn tick(&mut self, input: &Input) -> std::result::Result<(), Fault> {
        if let Some(fault) = &self.fault {
            return Err(fault.clone());
        }
        let result = self.advance(input);
        self.fault = result.as_ref().err().cloned();
        result
    }
    fn advance(&mut self, input: &Input) -> std::result::Result<(), Fault> {
        self.tick += 1;
        self.budget = 250_000;
        self.audio.clear();
        self.input.clone_from(input);
        let dt = 1. / self.project.settings.hz as f32;
        for spark in &mut self.particles {
            let Some(Resource::Particle(style)) = self.project.get(Kind::Particle, &spark.style)
            else {
                unreachable!()
            };
            spark.age += 1;
            spark.vy += style.gravity * dt;
            spark.x += spark.vx * dt;
            spark.y += spark.vy * dt;
        }
        self.particles.retain(|spark| {
            matches!(self.project.get(Kind::Particle, &spark.style), Some(Resource::Particle(style)) if spark.age < style.life)
        });
        let ids = self.entities.keys().copied().collect::<Vec<_>>();
        let index = self.index.clone();
        for id in ids {
            let mut entity = self
                .entities
                .remove(&id)
                .expect("only current entity is removed");
            if let Err(fault) = self.run(&index, &mut entity, Event::Step) {
                self.keep_entity(entity);
                return Err(fault);
            }
            if !entity.dead {
                let gravity = self.object(&entity.object).gravity;
                entity.vy = (entity.vy + gravity * dt).clamp(-1_000_000., 1_000_000.);
                let [dx, dy] = [entity.vx * dt, entity.vy * dt];
                if self.motion(&mut entity, 0, dx) {
                    entity.vx = 0.;
                }
                if self.motion(&mut entity, 1, dy) {
                    entity.vy = 0.;
                }
                self.keep_entity(entity);
            } else {
                self.unregister(&entity);
            }
        }
        self.spawn_queued()?;
        if let Some(room) = self.next_room.take() {
            self.room = room;
            self.enter_room()?;
        } else {
            self.update_camera(false);
        }
        self.previous.clone_from(input);
        Ok(())
    }
    fn enter_room(&mut self) -> std::result::Result<(), Fault> {
        self.camera = camera::ViewCamera::new(self.room(), self.project.settings.size);
        self.entities.clear();
        self.members.clear();
        self.placements.clear();
        self.solids.clear();
        self.particles.clear();
        self.queued.clear();
        let room = self.room().instances.clone();
        for i in room {
            self.queued.push_back((i.1, Some(i.0), i.2, i.3));
        }
        self.spawn_queued()?;
        self.update_camera(true);
        Ok(())
    }
    fn update_camera(&mut self, entering: bool) {
        let target = self
            .camera
            .follow
            .as_deref()
            .and_then(|id| self.placed(id))
            .map(|entity| {
                let [w, h] = self.object(&entity.object).size;
                [entity.x + w / 2., entity.y + h / 2.]
            });
        self.camera
            .update(target, 1. / self.project.settings.hz as f32, entering);
    }
    fn spawn_queued(&mut self) -> std::result::Result<(), Fault> {
        let index = self.index.clone();
        while !self.queued.is_empty() {
            let batch = std::mem::take(&mut self.queued);
            if self.entities.len() + batch.len() > ENTITY_LIMIT || self.budget < batch.len() as u32
            {
                return Err(self.error(0, Event::Create, "spawn or instruction limit exceeded"));
            }
            self.budget -= batch.len() as u32;
            let first = self.next_id;
            // Register each placement before running any create events in this batch.
            for (object, placement, x, y) in batch {
                let id = self.next_id;
                self.next_id += 1;
                if let Some(placement) = &placement {
                    self.placements.insert(placement.clone(), id);
                }
                self.members.entry(object.clone()).or_default().insert(id);
                self.keep_entity(Entity {
                    id,
                    object,
                    placement,
                    x,
                    y,
                    vx: 0.,
                    vy: 0.,
                    vars: BTreeMap::new(),
                    born: self.tick,
                    dead: false,
                });
            }
            for id in first..self.next_id {
                let mut entity = self.entities.remove(&id).expect("queued entity exists");
                if let Err(fault) = self.run(&index, &mut entity, Event::Create) {
                    self.keep_entity(entity);
                    return Err(fault);
                }
                if entity.dead {
                    self.unregister(&entity);
                } else {
                    self.keep_entity(entity);
                }
            }
        }
        Ok(())
    }
    fn keep_entity(&mut self, entity: Entity) {
        let object = self.object(&entity.object);
        if object.solid || object.one_way {
            self.solids.update(entity.id, entity.bounds(object));
        }
        self.entities.insert(entity.id, entity);
    }

    fn unregister(&mut self, entity: &Entity) {
        self.solids.remove(entity.id);
        if let Some(placement) = &entity.placement {
            self.placements.remove(placement);
        }
        if let Some(ids) = self.members.get_mut(&entity.object) {
            ids.remove(&entity.id);
        }
    }
    pub fn grounded(&self, entity: &Entity) -> bool {
        entity.vy >= 0. && self.blocked(entity, 0., 0.25)
    }

    fn blocked(&self, entity: &Entity, x: f32, y: f32) -> bool {
        let object = self.object(&entity.object);
        if x != 0. && self.sweep(entity.id, entity.bounds(object), 0, x) != x {
            return true;
        }
        let horizontal = (entity.x + x).clamp(-1_000_000., 1_000_000.);
        let bounds = Bounds::new(horizontal, entity.y, object.size[0], object.size[1]);
        y != 0. && self.sweep(entity.id, bounds, 1, y) != y
    }

    fn motion(&self, entity: &mut Entity, axis: usize, delta: f32) -> bool {
        if delta == 0. {
            return false;
        }
        let bounds = entity.bounds(self.object(&entity.object));
        let moved = self.sweep(entity.id, bounds, axis, delta);
        let position = if axis == 0 {
            &mut entity.x
        } else {
            &mut entity.y
        };
        *position = (*position + moved).clamp(-1_000_000., 1_000_000.);
        moved != delta
    }

    fn sweep(&self, mover: u64, bounds: Bounds, axis: usize, delta: f32) -> f32 {
        let swept = bounds.swept(axis, delta);
        let mut move_by = delta;
        self.solids.visit(swept, |id| {
            if id != mover
                && let Some(other) = self.entities.get(&id)
            {
                let object = self.object(&other.object);
                if !object.one_way || (axis == 1 && delta > 0.) {
                    move_by = bounds.clamp_motion(other.bounds(object), axis, move_by);
                }
            }
        });
        for map in &self.room().maps {
            let Some(Resource::Tilemap(map)) = self.project.get(Kind::Tilemap, map) else {
                unreachable!()
            };
            let cell = map.cell as f32;
            let column = |x: f32| ((x - map.at[0] as f32) / cell).floor().max(0.) as usize;
            let row = |y: f32| ((y - map.at[1] as f32) / cell).floor().max(0.) as usize;
            for y in row(swept.top)..=row(swept.bottom).min(map.rows.len().saturating_sub(1)) {
                let tiles = map.rows[y].as_bytes();
                for (x, &tile) in tiles
                    .iter()
                    .enumerate()
                    .take(column(swept.right).saturating_add(1))
                    .skip(column(swept.left))
                {
                    if map
                        .tiles
                        .get(&(tile as char))
                        .is_some_and(agge_core::Tile::solid)
                    {
                        let obstacle = Bounds::new(
                            map.at[0] as f32 + x as f32 * cell,
                            map.at[1] as f32 + y as f32 * cell,
                            cell,
                            cell,
                        );
                        move_by = bounds.clamp_motion(obstacle, axis, move_by);
                    }
                }
            }
        }
        move_by
    }
    fn emit(&mut self, id: &str, entity: &Entity) {
        let Some(Resource::Particle(style)) = self.project.get(Kind::Particle, id) else {
            unreachable!()
        };
        for _ in 0..(style.count as usize).min(PARTICLE_LIMIT - self.particles.len()) {
            let direction = random(&mut self.fx_rng) * std::f32::consts::TAU;
            let speed = random(&mut self.fx_rng) * style.speed;
            self.particles.push(Spark {
                x: entity.x,
                y: entity.y,
                vx: direction.cos() * speed,
                vy: direction.sin() * speed,
                age: 0,
                style: id.into(),
            });
        }
    }
}
fn flag(value: bool) -> f32 {
    u8::from(value) as f32
}
fn bounded(value: f32) -> std::result::Result<f32, &'static str> {
    if (-1_000_000. ..=1_000_000.).contains(&value) {
        Ok(value)
    } else {
        Err("position or speed out of range")
    }
}
fn random(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 40) as f32 * (1. / 16_777_216.)
}
