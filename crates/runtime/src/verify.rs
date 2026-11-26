use crate::{CompiledGame, Fault, Input, World};
use agge_core::{Project, Result, fail, valid_id};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const TICK_LIMIT: u32 = 60_000;
const SPAN_LIMIT: usize = 4096;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputSpan(pub u32, pub u32, pub String);

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Replay {
    pub ticks: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<InputSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expect: Vec<Assertion>,
    #[serde(default, skip_serializing_if = "Samples::is_empty")]
    pub sample: Samples,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Sample(pub u32, pub String);

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Samples {
    Points(Vec<Sample>),
    Grid { at: Vec<u32>, fields: Vec<String> },
}

impl Default for Samples {
    fn default() -> Self {
        Self::Points(vec![])
    }
}

impl Samples {
    fn len(&self) -> usize {
        match self {
            Self::Points(points) => points.len(),
            Self::Grid { at, fields } => at.len().saturating_mul(fields.len()),
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn points(&self) -> Vec<(u32, &str)> {
        match self {
            Self::Points(points) => points.iter().map(|s| (s.0, s.1.as_str())).collect(),
            Self::Grid { at, fields } => at
                .iter()
                .flat_map(|&tick| fields.iter().map(move |field| (tick, field.as_str())))
                .collect(),
        }
    }
}

pub struct ReplayRecorder {
    spec: Replay,
    held: BTreeMap<String, usize>,
    pub full: bool,
}

impl ReplayRecorder {
    /// Record against the project's starting room and seed unless a room is selected.
    pub fn for_project(project: &Project, room: Option<&str>) -> Self {
        let mut recorder = Self::new(room.unwrap_or(&project.settings.start));
        recorder.spec.seed = Some(project.settings.seed);
        recorder
    }

    pub fn new(room: &str) -> Self {
        Self {
            spec: Replay {
                ticks: 0,
                room: Some(room.into()),
                seed: None,
                input: vec![],
                expect: vec![],
                sample: Samples::default(),
            },
            held: BTreeMap::new(),
            full: false,
        }
    }

    pub fn record(&mut self, input: &Input) {
        if self.full {
            return;
        }
        self.held.retain(|key, _| input.contains(key));
        let added = input.len() - self.held.len();
        if self.spec.ticks == TICK_LIMIT || self.spec.input.len() + added > SPAN_LIMIT {
            self.full = true;
            return;
        }
        self.spec.ticks += 1;
        for key in input {
            let span = if let Some(&span) = self.held.get(key) {
                span
            } else {
                let index = self.spec.input.len();
                self.spec
                    .input
                    .push(InputSpan(self.spec.ticks, self.spec.ticks, key.clone()));
                self.held.insert(key.clone(), index);
                index
            };
            self.spec.input[span].1 = self.spec.ticks;
        }
    }

    pub fn replay(&self) -> &Replay {
        &self.spec
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub at: u32,
    pub field: String,
    pub is: Scalar,
    #[serde(default)]
    pub tolerance: f32,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum Scalar {
    Number(f32),
    Text(String),
}

#[derive(Debug, Serialize)]
pub struct Failure {
    pub at: u32,
    pub field: String,
    pub expected: Scalar,
    pub actual: Option<Scalar>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub ticks: u64,
    pub assertions: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<Option<Scalar>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<Failure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault: Option<Fault>,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.failures.is_empty() && self.fault.is_none()
    }
}

pub fn replay(project: Project, spec: &Replay) -> Result<(World, Report)> {
    replay_compiled(&CompiledGame::new(project)?, spec)
}

pub fn replay_compiled(game: &CompiledGame, spec: &Replay) -> Result<(World, Report)> {
    if spec.ticks > TICK_LIMIT
        || spec.input.len() > SPAN_LIMIT
        || spec.expect.len() > 4096
        || spec.sample.len() > 256
        || spec
            .input
            .iter()
            .any(|InputSpan(a, b, key)| *a == 0 || a > b || *b > spec.ticks || !valid_id(key))
        || spec
            .expect
            .iter()
            .any(|a| a.at > spec.ticks || !a.tolerance.is_finite() || a.tolerance < 0.)
    {
        return Err(fail("invalid replay ticks, input ranges or assertions"));
    }
    let mut samples = spec
        .sample
        .points()
        .into_iter()
        .enumerate()
        .collect::<Vec<_>>();
    if samples.iter().any(|(_, (tick, _))| *tick > spec.ticks) {
        return Err(fail("sample tick exceeds replay length"));
    }
    let mut world = game.preview_seeded(spec.room.as_deref(), spec.seed)?;
    let mut report = Report {
        ticks: 0,
        assertions: 0,
        samples: vec![None; spec.sample.len()],
        failures: vec![],
        fault: world.fault().cloned(),
    };
    if report.fault.is_some() {
        return Ok((world, report));
    }
    let mut assertions = spec.expect.iter().collect::<Vec<_>>();
    assertions.sort_by_key(|a| a.at);
    let mut assertions = assertions.into_iter().peekable();
    samples.sort_unstable_by_key(|(_, (tick, _))| *tick);
    let mut samples = samples.into_iter().peekable();
    let mut changes = spec
        .input
        .iter()
        .flat_map(|InputSpan(first, last, action)| {
            [(*first, action, true), (*last + 1, action, false)]
        })
        .collect::<Vec<_>>();
    changes.sort_unstable_by_key(|(tick, _, _)| *tick);
    let mut changes = changes.into_iter().peekable();
    let mut counts = BTreeMap::new();
    let mut input = Input::new();
    for time in 0..=spec.ticks {
        if time > 0 {
            while let Some((_, action, start)) = changes.next_if(|(tick, _, _)| *tick == time) {
                let count = counts.entry(action).or_insert(0usize);
                if start {
                    if *count == 0 {
                        input.insert(action.clone());
                    }
                    *count += 1;
                } else {
                    *count -= 1;
                    if *count == 0 {
                        input.remove(action);
                    }
                }
            }
            if let Err(error) = world.tick(&input) {
                report.fault = Some(error);
                break;
            }
            report.ticks = u64::from(time);
        }
        while let Some(assertion) = assertions.next_if(|a| a.at == time) {
            report.assertions += 1;
            let actual = read(&world, &assertion.field);
            let equal = match (&actual, &assertion.is) {
                (Some(Scalar::Number(actual)), Scalar::Number(expected)) => {
                    (actual - expected).abs() <= assertion.tolerance
                }
                (Some(actual), expected) => actual == expected,
                (None, _) => false,
            };
            if !equal {
                report.failures.push(Failure {
                    at: time,
                    field: assertion.field.clone(),
                    expected: assertion.is.clone(),
                    actual,
                });
            }
        }
        while let Some((position, (_, field))) = samples.next_if(|(_, (tick, _))| *tick == time) {
            report.samples[position] = read(&world, field);
        }
    }
    Ok((world, report))
}

fn read(world: &World, field: &str) -> Option<Scalar> {
    let mut parts = field.split('/');
    let value = match (parts.next()?, parts.next(), parts.next(), parts.next()) {
        ("room", None, None, None) => return Some(Scalar::Text(world.room_id().into())),
        ("music", None, None, None) => return Some(Scalar::Text(world.room().music.clone()?)),
        ("sound", Some(id), None, None) => world.queued_sound_count(id)? as f32,
        ("particles", None, None, None) => world.particles.len() as f32,
        ("count", Some(object), None, None) => world.count(object) as f32,
        ("grounded", Some(placement), None, None) => {
            u8::from(world.grounded(world.placed(placement)?)) as f32
        }
        ("animation", Some(placement), None, None) => {
            return Some(Scalar::Text(
                world.animation(world.placed(placement)?)?.0.into(),
            ));
        }
        ("frame", Some(placement), None, None) => {
            let (_, clip, elapsed) = world.animation(world.placed(placement)?)?;
            clip.frames[clip.slot(elapsed)?] as f32
        }
        ("timer", Some(placement), Some(name), None) => {
            world
                .placed(placement)?
                .timer(name, world.snapshot_tick())
                .0 as f32
        }
        ("expired", Some(placement), Some(name), None) => {
            let (_, due) = world.placed(placement)?.timer(name, world.snapshot_tick());
            u8::from(due) as f32
        }
        ("camera", Some("x"), None, None) => world.camera()[0],
        ("camera", Some("y"), None, None) => world.camera()[1],
        ("global", Some(key), None, None) => world.globals.get(key).copied().unwrap_or(0.),
        ("entity", Some(placement), Some(property), None) => {
            let entity = world.placed(placement)?;
            match property {
                "object" => return Some(Scalar::Text(entity.object.clone())),
                "x" => entity.x,
                "y" => entity.y,
                "vx" => entity.vx,
                "vy" => entity.vy,
                key => *entity.vars.get(key)?,
            }
        }
        _ => return None,
    };
    Some(Scalar::Number(value))
}
