use super::{
    ActorEffects, CompiledGame, ENTITY_LIMIT, Entity, Input, PARTICLE_LIMIT, Spark, World, bounded,
    tiles::SavedTiles,
};
use agge_core::{
    Kind, Project, Resource, Result, clamp_view_origin, fail, store::write_atomic, valid_id,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
};

const VERSION: u32 = 1;
const MAX_BYTES: usize = 16 * 1024 * 1024;

/// Simulation state at a successful tick boundary. Project source and media stay external.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    version: u32,
    project: String,
    room: String,
    tick: u64,
    next_id: u64,
    rng: [u64; 2],
    camera: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pointer: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_room: Option<String>,
    globals: BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    tiles: SavedTiles,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    progress_requested: bool,
    entities: Vec<SavedActor>,
    particles: Vec<Spark>,
    held: Input,
    sounds: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SavedActor {
    id: u64,
    object: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    placement: Option<String>,
    pos: [f32; 2],
    vel: [f32; 2],
    born: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    vars: BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    animation: Option<(String, u64)>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    timers: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    flip: Option<[bool; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tint: Option<agge_core::Color>,
    #[serde(default, skip_serializing_if = "is_zero")]
    drop_until: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl Checkpoint {
    pub fn tick(&self) -> u64 {
        self.tick
    }
    pub fn room(&self) -> &str {
        &self.room
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        check_size(bytes)?;
        let checkpoint: Self = serde_json::from_slice(bytes)?;
        if checkpoint.version != VERSION {
            return Err(fail(format!(
                "unsupported checkpoint version {}",
                checkpoint.version
            )));
        }
        Ok(checkpoint)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self)?;
        check_size(&bytes)?;
        Ok(bytes)
    }

    pub fn read(path: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        Self::decode(&bytes)
    }

    pub fn write(&self, path: &Path, replace: bool) -> Result<()> {
        write_atomic(path, &self.encode()?, replace)
    }
}

fn check_size(bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_BYTES {
        return Err(fail("checkpoint exceeds 16 MiB"));
    }
    Ok(())
}

fn fingerprint(project: &Project) -> String {
    // Canonical source permits harmless JSON spacing and resource reordering.
    blake3::hash(project.encode().as_bytes())
        .to_hex()
        .to_string()
}

impl World {
    pub fn checkpoint(&self) -> Result<Checkpoint> {
        if self.fault.is_some() {
            return Err(fail("a faulted world cannot be checkpointed"));
        }
        Ok(Checkpoint {
            version: VERSION,
            project: fingerprint(&self.project),
            room: self.room.clone(),
            tick: self.tick,
            next_id: self.next_id,
            rng: [self.rng, self.fx_rng],
            camera: self.camera.origin,
            next_room: self.next_room.clone(),
            globals: self.globals.clone(),
            tiles: self.save_tiles(),
            progress_requested: self.progress_requested,
            entities: self
                .entities
                .values()
                .map(|actor| SavedActor {
                    id: actor.id,
                    object: actor.object.clone(),
                    placement: actor.placement.clone(),
                    pos: [actor.x, actor.y],
                    vel: [actor.vx, actor.vy],
                    born: actor.born,
                    vars: actor.vars.clone(),
                    animation: actor
                        .active_animation()
                        .map(|(id, since)| (id.into(), since)),
                    timers: actor
                        .effects
                        .as_ref()
                        .map(|effects| effects.timers.clone())
                        .unwrap_or_default(),
                    flip: actor.effects.as_ref().and_then(|effects| effects.flip),
                    tint: actor.effects.as_ref().and_then(|effects| effects.tint),
                    drop_until: actor
                        .effects
                        .as_ref()
                        .map_or(0, |effects| effects.drop_until),
                })
                .collect(),
            particles: self.particles.clone(),
            pointer: self.pointer,
            held: self.previous.clone(),
            sounds: self.audio.clone(),
        })
    }
}

impl CompiledGame {
    pub fn restore(&self, save: Checkpoint) -> Result<World> {
        if save.project != fingerprint(&self.project) {
            return Err(fail("checkpoint belongs to a different project revision"));
        }
        self.restore_state(save)
    }

    /// Prepare a new live world using the old tick state. Authored placements and
    /// the clock must still match. Runtime resources are validated against the new project.
    pub fn reload_live(&self, old: &World) -> Result<World> {
        let settings = &self.project.settings;
        let current = &old.project.settings;
        if current.hz != settings.hz || current.size != settings.size {
            return Err(fail("restart after changing tick rate or viewport size"));
        }
        if current.progress != settings.progress {
            return Err(fail("restart after changing the progress schema"));
        }
        let Some(Resource::Room(room)) = self.project.get(Kind::Room, old.room_id()) else {
            return Err(fail("restart after removing the live room"));
        };
        if room.instances != old.room().instances {
            return Err(fail(
                "restart after changing authored placements in the live room",
            ));
        }
        self.restore_state(old.checkpoint()?)
    }

    fn restore_state(&self, save: Checkpoint) -> Result<World> {
        if save.version != VERSION
            || save.tick > u64::MAX / 1024
            || save.next_id == 0
            || save.next_id >= u64::MAX / 2
            || save.rng.contains(&0)
            || save.entities.len() > ENTITY_LIMIT
            || save.particles.len() > PARTICLE_LIMIT
            || save.sounds.len() > 256
            || save.held.len() > 4096
            || !save.held.iter().all(|key| valid_id(key))
            || !valid_vars(&save.globals)
            || (save.progress_requested && self.project.settings.progress.is_none())
            || !save.camera.iter().all(|v| v.is_finite())
            || save.pointer.is_some_and(|[x, y]| {
                let [w, h] = self.project.settings.size;
                x >= w || y >= h
            })
        {
            return Err(fail("checkpoint has invalid world state"));
        }
        if save
            .next_room
            .as_ref()
            .is_some_and(|room| self.project.get(Kind::Room, room).is_none())
        {
            return Err(fail("checkpoint has a missing pending room"));
        }
        let mut world = World::prepare(self, Some(&save.room), None)?;
        let authored: BTreeMap<_, _> = world
            .room()
            .instances
            .iter()
            .map(|instance| (instance.0.clone(), instance.1.clone()))
            .collect();
        world.prepare_room_geometry();
        world.camera.origin =
            clamp_view_origin(save.camera, world.camera.room, world.project.settings.size);
        world.tick = save.tick;
        world.next_id = save.next_id;
        [world.rng, world.fx_rng] = save.rng;
        world.next_room = save.next_room;
        world.previous = save.held;
        world.input.clone_from(&world.previous);
        world.pointer = save.pointer;
        world.globals = save.globals;
        world.progress_requested = save.progress_requested;
        world.restore_tiles(save.tiles)?;
        let (mut last_id, mut placements) = (0, BTreeSet::new());
        for actor in save.entities {
            if actor.id <= last_id
                || actor.id >= save.next_id
                || actor.born > save.tick
                || !self.objects.contains_key(&actor.object)
                || !actor
                    .pos
                    .into_iter()
                    .chain(actor.vel)
                    .all(|value| bounded(value).is_ok())
                || !valid_vars(&actor.vars)
                || actor.timers.len() > 16
                || actor.drop_until > save.tick + 600
                || actor
                    .timers
                    .iter()
                    .any(|(key, &due)| !valid_id(key) || due > save.tick + 60_000)
                || actor.animation.as_ref().is_some_and(|(clip, since)| {
                    !self.animations.contains_key(clip) || *since < actor.born || *since > save.tick
                })
                || actor.placement.as_ref().is_some_and(|name| {
                    authored.get(name) != Some(&actor.object) || !placements.insert(name.clone())
                })
            {
                return Err(fail(format!("checkpoint has invalid entity {}", actor.id)));
            }
            last_id = actor.id;
            let effects = (actor.animation.is_some()
                || !actor.timers.is_empty()
                || actor.flip.is_some()
                || actor.tint.is_some()
                || actor.drop_until != 0)
                .then(|| {
                    let (animation, animation_since) = actor
                        .animation
                        .map_or((None, 0), |(clip, since)| (Some(clip), since));
                    Box::new(ActorEffects {
                        animation,
                        animation_since,
                        timers: actor.timers,
                        flip: actor.flip,
                        tint: actor.tint,
                        drop_until: actor.drop_until,
                    })
                });
            world.register_entity(Entity {
                id: actor.id,
                runs_step: world.runs_step(&actor.object),
                object: actor.object,
                placement: actor.placement,
                x: actor.pos[0],
                y: actor.pos[1],
                vx: actor.vel[0],
                vy: actor.vel[1],
                vars: actor.vars,
                born: actor.born,
                dead: false,
                effects,
            });
        }
        for particle in &save.particles {
            if !matches!(self.project.get(Kind::Particle, &particle.style), Some(Resource::Particle(style)) if particle.age < style.life)
                || ![particle.x, particle.y, particle.vx, particle.vy]
                    .iter()
                    .all(|value| value.is_finite())
            {
                return Err(fail("checkpoint has an invalid particle"));
            }
        }
        if save
            .sounds
            .iter()
            .any(|id| self.project.get(Kind::Audio, id).is_none())
        {
            return Err(fail("checkpoint contains a missing sound"));
        }
        world.particles = save.particles;
        world.audio = save.sounds;
        Ok(world)
    }
}

fn valid_vars(vars: &BTreeMap<String, f32>) -> bool {
    vars.iter().all(|(key, v)| valid_id(key) && v.is_finite())
}
