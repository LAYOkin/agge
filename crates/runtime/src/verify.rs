use crate::{CompiledGame, Fault, Input, World};
use agge_core::{Project, Result, fail, script::PointerProperty, valid_id};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet},
    iter::Peekable,
    vec::IntoIter,
};

const TICK_LIMIT: u32 = 60_000;
const SPAN_LIMIT: usize = 4096;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputSpan(pub u32, pub u32, pub String);

#[derive(Debug, Deserialize, Serialize)]
pub struct PointerSpan(pub u32, pub u32, pub u32, pub u32);

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
    pub pointer: Vec<PointerSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expect: Vec<Assertion>,
    #[serde(default, skip_serializing_if = "Samples::is_empty")]
    pub sample: Samples,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<Watch>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Watch {
    pub fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct WatchReport {
    pub fields: Vec<String>,
    pub changes: Vec<(u32, usize, Option<Scalar>)>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

struct WatchState {
    at: [u32; 2],
    limit: usize,
    last: Vec<usize>,
}

impl WatchState {
    fn start(watch: &Watch, ticks: u32) -> Result<Self> {
        let at @ [first, last] = watch.at.unwrap_or([0, ticks]);
        let limit = watch.limit.unwrap_or(64);
        if first > last
            || last > ticks
            || !(1..=16).contains(&watch.fields.len())
            || !(watch.fields.len()..=256).contains(&limit)
            || watch
                .fields
                .iter()
                .any(|key| key.is_empty() || key.len() > 128)
            || watch.fields.iter().collect::<BTreeSet<_>>().len() != watch.fields.len()
        {
            return Err(fail(
                "watch needs 1–16 unique fields, an in-range interval, and a limit from field count to 256",
            ));
        }
        Ok(Self {
            at,
            limit,
            last: vec![0; watch.fields.len()],
        })
    }

    fn observe(&mut self, time: u32, world: &World, report: &mut WatchReport) {
        let [first, last] = self.at;
        if time < first || time > last || report.truncated {
            return;
        }
        for (index, (field, old)) in report.fields.iter().zip(&mut self.last).enumerate() {
            let value = read(world, field);
            if time != first && same_reading(report.changes[*old].2.as_ref(), value) {
                continue;
            }
            if report.changes.len() == self.limit {
                report.truncated = true;
                return;
            }
            *old = report.changes.len();
            report.changes.push((time, index, value.map(Reading::own)));
        }
    }
}

/// Named replay cases are decoded individually so a caller can select one case
/// even when another case in the file is malformed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaySuite<T> {
    pub base: Option<String>,
    #[serde(default)]
    pub detail: bool,
    pub cases: BTreeMap<String, T>,
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

    fn point(&self, index: usize) -> (u32, &str) {
        match self {
            Self::Points(points) => (points[index].0, &points[index].1),
            Self::Grid { at, fields } => (at[index / fields.len()], &fields[index % fields.len()]),
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
                pointer: vec![],
                expect: vec![],
                sample: Samples::default(),
                watch: None,
            },
            held: BTreeMap::new(),
            full: false,
        }
    }

    pub fn record(&mut self, input: &Input) {
        self.record_frame(input, None);
    }

    pub fn record_frame(&mut self, input: &Input, pointer: Option<[u32; 2]>) {
        if self.full {
            return;
        }
        self.held.retain(|key, _| input.contains(key));
        let added = input.len() - self.held.len();
        let extend_pointer = self
            .spec
            .pointer
            .last()
            .is_some_and(|span| pointer == Some([span.2, span.3]) && span.1 == self.spec.ticks);
        if self.spec.ticks == TICK_LIMIT
            || self.spec.input.len() + added > SPAN_LIMIT
            || (pointer.is_some() && !extend_pointer && self.spec.pointer.len() == SPAN_LIMIT)
        {
            self.full = true;
            return;
        }
        self.spec.ticks += 1;
        if let Some([x, y]) = pointer {
            if extend_pointer {
                self.spec.pointer.last_mut().unwrap().1 = self.spec.ticks;
            } else {
                self.spec
                    .pointer
                    .push(PointerSpan(self.spec.ticks, self.spec.ticks, x, y));
            }
        }
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

#[derive(Clone, Copy)]
enum Reading<'a> {
    Number(f32),
    Text(&'a str),
    Glyph(char),
}

impl Reading<'_> {
    fn own(self) -> Scalar {
        match self {
            Self::Number(n) => Scalar::Number(n),
            Self::Text(text) => Scalar::Text(text.into()),
            Self::Glyph(glyph) => Scalar::Text(glyph.into()),
        }
    }
}

fn same_reading(old: Option<&Scalar>, value: Option<Reading<'_>>) -> bool {
    match (old, value) {
        (Some(Scalar::Number(old)), Some(Reading::Number(value))) => *old == value,
        (Some(Scalar::Text(old)), Some(Reading::Text(value))) => old == value,
        (Some(Scalar::Text(old)), Some(Reading::Glyph(value))) => old.chars().eq([value]),
        (None, None) => true,
        _ => false,
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch: Option<WatchReport>,
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
    Ok(finish(ReplayPlayback::new(game, spec)?))
}

pub fn replay_from(world: World, spec: &Replay) -> Result<(World, Report)> {
    Ok(finish(ReplayPlayback::resume(world, spec)?))
}

fn finish(mut playback: ReplayPlayback<&Replay>) -> (World, Report) {
    while !playback.finished() {
        playback.step();
    }
    (playback.world, playback.report)
}

/// Drive a replay one tick at a time and verify observations before the caller
/// renders the resulting world or consumes its queued audio.
pub struct ReplayPlayback<S> {
    spec: S,
    world: World,
    report: Report,
    assertions: Peekable<IntoIter<usize>>,
    samples: Peekable<IntoIter<(u32, usize)>>,
    changes: Peekable<IntoIter<(u32, usize, bool)>>,
    counts: BTreeMap<String, usize>,
    input: Input,
    pointer_index: usize,
    watch: Option<WatchState>,
}

impl<S: Borrow<Replay>> ReplayPlayback<S> {
    pub fn new(game: &CompiledGame, spec: S) -> Result<Self> {
        Self::start(spec, |replay| {
            game.preview_seeded(replay.room.as_deref(), replay.seed)
        })
    }

    pub fn resume(world: World, spec: S) -> Result<Self> {
        let replay = spec.borrow();
        if replay.room.is_some() || replay.seed.is_some() {
            return Err(fail("resumed replay already has a room and random state"));
        }
        Self::start(spec, |_| Ok(world))
    }

    fn start(spec: S, make_world: impl FnOnce(&Replay) -> Result<World>) -> Result<Self> {
        let replay = spec.borrow();
        if replay.ticks > TICK_LIMIT
            || replay.input.len() > SPAN_LIMIT
            || replay.pointer.len() > SPAN_LIMIT
            || replay.expect.len() > 4096
            || replay.sample.len() > 256
            || replay
                .input
                .iter()
                .any(|InputSpan(a, b, key)| *a == 0 || a > b || *b > replay.ticks || !valid_id(key))
            || replay
                .expect
                .iter()
                .any(|a| a.at > replay.ticks || !a.tolerance.is_finite() || a.tolerance < 0.)
        {
            return Err(fail("invalid replay ticks, input ranges or assertions"));
        }
        let mut samples = (0..replay.sample.len())
            .map(|index| (replay.sample.point(index).0, index))
            .collect::<Vec<_>>();
        if samples.iter().any(|(tick, _)| *tick > replay.ticks) {
            return Err(fail("sample tick exceeds replay length"));
        }
        samples.sort_unstable();
        let mut assertions = (0..replay.expect.len()).collect::<Vec<_>>();
        assertions.sort_by_key(|&index| replay.expect[index].at);
        let mut changes = replay
            .input
            .iter()
            .enumerate()
            .flat_map(|(index, InputSpan(first, last, _))| {
                [(*first, index, true), (*last + 1, index, false)]
            })
            .collect::<Vec<_>>();
        changes.sort_unstable_by_key(|(tick, _, _)| *tick);
        let world = make_world(replay)?;
        let [w, h] = world.project().settings.size;
        if replay.pointer.iter().any(|PointerSpan(a, b, x, y)| {
            *a == 0 || a > b || *b > replay.ticks || *x >= w || *y >= h
        }) || replay.pointer.windows(2).any(|pair| pair[0].1 >= pair[1].0)
        {
            return Err(fail(
                "pointer spans must be ordered, disjoint, and within the replay and viewport",
            ));
        }
        let watch = replay
            .watch
            .as_ref()
            .map(|watch| WatchState::start(watch, replay.ticks))
            .transpose()?;
        let report = Report {
            ticks: 0,
            assertions: 0,
            samples: vec![None; replay.sample.len()],
            watch: replay.watch.as_ref().map(|watch| WatchReport {
                fields: watch.fields.clone(),
                changes: Vec::new(),
                truncated: false,
            }),
            failures: vec![],
            fault: world.fault().cloned(),
        };
        let mut playback = Self {
            spec,
            world,
            report,
            assertions: assertions.into_iter().peekable(),
            samples: samples.into_iter().peekable(),
            changes: changes.into_iter().peekable(),
            counts: BTreeMap::new(),
            input: Input::new(),
            pointer_index: 0,
            watch,
        };
        if playback.report.fault.is_none() {
            playback.observe(0);
        }
        Ok(playback)
    }

    pub fn spec(&self) -> &S {
        &self.spec
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn report(&self) -> &Report {
        &self.report
    }

    pub fn finished(&self) -> bool {
        self.report.fault.is_some() || self.report.ticks == u64::from(self.spec.borrow().ticks)
    }

    pub fn drain_audio(&mut self) -> Vec<String> {
        self.world.drain_audio()
    }

    pub fn step(&mut self) {
        if self.finished() {
            return;
        }
        let time = self.report.ticks as u32 + 1;
        let spec = self.spec.borrow();
        while let Some((_, index, start)) = self.changes.next_if(|(tick, _, _)| *tick == time) {
            let action = &spec.input[index].2;
            if start {
                if let Some(count) = self.counts.get_mut(action) {
                    *count += 1;
                } else {
                    self.counts.insert(action.clone(), 1);
                    self.input.insert(action.clone());
                }
            } else {
                let count = self.counts.get_mut(action).expect("span already started");
                *count -= 1;
                if *count == 0 {
                    self.input.remove(action);
                    self.counts.remove(action);
                }
            }
        }
        if spec
            .pointer
            .get(self.pointer_index)
            .is_some_and(|span| time > span.1)
        {
            self.pointer_index += 1;
        }
        let pointer = spec
            .pointer
            .get(self.pointer_index)
            .filter(|span| time >= span.0)
            .map(|span| [span.2, span.3]);
        if let Err(error) = self.world.tick_with_pointer(&self.input, pointer) {
            self.report.fault = Some(error);
            return;
        }
        self.report.ticks = u64::from(time);
        self.observe(time);
    }

    fn observe(&mut self, time: u32) {
        let spec = self.spec.borrow();
        while let Some(index) = self
            .assertions
            .next_if(|index| spec.expect[*index].at == time)
        {
            let assertion = &spec.expect[index];
            self.report.assertions += 1;
            let actual = read(&self.world, &assertion.field);
            let equal = match (actual, &assertion.is) {
                (Some(Reading::Number(actual)), Scalar::Number(expected)) => {
                    (actual - expected).abs() <= assertion.tolerance
                }
                (actual, expected) => same_reading(Some(expected), actual),
            };
            if !equal {
                self.report.failures.push(Failure {
                    at: time,
                    field: assertion.field.clone(),
                    expected: assertion.is.clone(),
                    actual: actual.map(Reading::own),
                });
            }
        }
        while let Some((_, index)) = self.samples.next_if(|(tick, _)| *tick == time) {
            self.report.samples[index] =
                read(&self.world, spec.sample.point(index).1).map(Reading::own);
        }
        if let (Some(watch), Some(report)) = (&mut self.watch, &mut self.report.watch) {
            watch.observe(time, &self.world, report);
        }
    }
}

fn read<'a>(world: &'a World, field: &str) -> Option<Reading<'a>> {
    let mut parts = field.split('/');
    let selector = (parts.next()?, parts.next(), parts.next(), parts.next());
    if parts.next().is_some() {
        return None;
    }
    let value = match selector {
        ("tile", Some(map), Some(x), Some(y)) => {
            let cell = [x.parse().ok()?, y.parse().ok()?];
            return Some(Reading::Glyph(world.tile_layer(map)?.symbol(cell)?));
        }
        ("room", None, None, None) => return Some(Reading::Text(world.room_id())),
        ("music", None, None, None) => return Some(Reading::Text(world.room().music.as_deref()?)),
        ("sound", Some(id), None, None) => world.queued_sound_count(id)? as f32,
        ("particles", None, None, None) => world.particles.len() as f32,
        ("tiles_changed", None, None, None) => world.changed_tile_count() as f32,
        ("count", Some(object), None, None) => world.count(object) as f32,
        ("grounded", Some(placement), None, None) => {
            u8::from(world.grounded(world.placed(placement)?)) as f32
        }
        ("animation", Some(placement), None, None) => {
            return Some(Reading::Text(world.animation(world.placed(placement)?)?.0));
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
        ("pointer", Some(property), None, None) => {
            world.pointer_property(PointerProperty::parse(property)?)
        }
        ("global", Some(key), None, None) => world.globals.get(key).copied().unwrap_or(0.),
        ("entity", Some(placement), Some(property), None) => {
            let entity = world.placed(placement)?;
            match property {
                "object" => return Some(Reading::Text(&entity.object)),
                "x" => entity.x,
                "y" => entity.y,
                "vx" => entity.vx,
                "vy" => entity.vy,
                key => *entity.vars.get(key)?,
            }
        }
        _ => return None,
    };
    Some(Reading::Number(value))
}
