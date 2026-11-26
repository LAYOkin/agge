use agge_core::{
    Direction, Edit, Index, Kind, Resource, Result, fail,
    script::{self, Event},
    store::{Loaded, PROJECT_LIMIT, Store},
};
use agge_runtime::{
    Checkpoint, CompiledGame, Media, Replay, ReplaySuite, Report, asset_info, capture, encode_png,
    pixel_compare, pixel_sheet, replay, replay_compiled, replay_from,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{cell::OnceCell, collections::BTreeMap};

fn require_revision(loaded: &Loaded, base: Option<&str>) -> Result<()> {
    if base.is_some_and(|base| base != loaded.revision) {
        return Err(fail(format!(
            "stale revision; current is {}",
            loaded.revision
        )));
    }
    Ok(())
}

pub fn check(store: &Store) -> Result<(Value, bool)> {
    let loaded = store.load()?;
    let index = Index::build(&loaded.project);
    let mut diagnostics = json!(index.diagnostics);
    if index.diagnostics.is_empty()
        && let Err(error) = Media::load_indexed(&loaded.project, store.root(), &index)
    {
        diagnostics = json!([{"code":"asset","message":error.to_string()}]);
    }
    let ok = diagnostics.as_array().is_some_and(Vec::is_empty);
    Ok((
        json!({"revision":loaded.revision,"diagnostics":diagnostics}),
        ok,
    ))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    get: Option<String>,
    pixel: Option<String>,
    frame: Option<String>,
    layer: Option<String>,
    base: Option<String>,
    asset: Option<String>,
    schema: Option<String>,
    path: Option<String>,
    lines: Option<[usize; 2]>,
    event: Option<Event>,
    function: Option<String>,
    area: Option<[usize; 4]>,
    placements: Option<String>,
    object: Option<String>,
    outline: Option<String>,
    symbol: Option<String>,
    refs: Option<String>,
    walk: Option<Roots>,
    diagnostics: Option<String>,
    direction: Option<Direction>,
    depth: Option<u8>,
    prefix: Option<String>,
    session: Option<String>,
    #[serde(default)]
    advance: bool,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Roots {
    One(String),
    Many(Vec<String>),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reads {
    read: Vec<Query>,
    base: Option<String>,
}
fn default_limit() -> usize {
    64
}

#[derive(Default)]
pub struct QueryCache {
    snapshot: Option<(Loaded, OnceCell<Index>)>,
}

impl QueryCache {
    fn snapshot(&mut self, store: &Store) -> Result<(&Loaded, &OnceCell<Index>)> {
        let known = self
            .snapshot
            .as_ref()
            .map(|(loaded, _)| loaded.revision.as_str());
        if let Some(loaded) = store.load_if_changed(known)? {
            self.snapshot = Some((loaded, OnceCell::new()));
        }
        let (loaded, index) = self
            .snapshot
            .as_ref()
            .expect("first load always returns a snapshot");
        Ok((loaded, index))
    }
}

pub fn query(store: &Store, args: Value) -> Result<Value> {
    query_with_cache(store, args, &mut QueryCache::default())
}

pub fn query_with_cache(store: &Store, args: Value, cache: &mut QueryCache) -> Result<Value> {
    if args.get("read").is_some() {
        let batch: Reads = serde_json::from_value(args)?;
        if !(1..=16).contains(&batch.read.len()) {
            return Err(fail("read must contain 1–16 queries"));
        }
        let (loaded, index) = cache.snapshot(store)?;
        require_revision(loaded, batch.base.as_deref())?;
        let mut data = Vec::with_capacity(batch.read.len());
        let mut bytes = 0;
        for args in batch.read {
            let response = if args.base.is_some() {
                Err(fail("set base on the outer read request"))
            } else {
                query_data(args, loaded, index)
            };
            let value = match response {
                Ok(value) => value,
                Err(error) => json!({"error":error.to_string()}),
            };
            bytes += value.to_string().len();
            if bytes > 2 * 1024 * 1024 {
                return Err(fail("batched response exceeds 2 MiB; narrow the reads"));
            }
            data.push(value);
        }
        return Ok(json!({"revision":loaded.revision,"data":data}));
    }
    let args: Query = serde_json::from_value(args)?;
    args.validate()?;
    if let Some(schema) = args.schema {
        return guide(&schema);
    }
    if let Some(file) = args.asset {
        return Ok(json!({"asset":file,"data":asset_info(store.root(), &file)?}));
    }
    if let Some(session) = args.session {
        if args.offset > 0 {
            return Err(fail("session delta must be queried by itself"));
        }
        return Ok(json!(store.delta(&session, args.advance)?));
    }
    let (loaded, index) = cache.snapshot(store)?;
    require_revision(loaded, args.base.as_deref())?;
    let mut result = query_data(args, loaded, index)?;
    result["revision"] = json!(loaded.revision);
    Ok(result)
}

impl Query {
    fn validate(&self) -> Result<()> {
        let args = self;
        let modes = [
            &args.get,
            &args.pixel,
            &args.asset,
            &args.refs,
            &args.outline,
            &args.placements,
            &args.prefix,
            &args.schema,
            &args.session,
        ];
        let selectors = usize::from(args.path.is_some())
            + usize::from(args.lines.is_some())
            + usize::from(args.event.is_some())
            + usize::from(args.function.is_some())
            + usize::from(args.area.is_some() && args.pixel.is_none());
        if modes.into_iter().filter(|option| option.is_some()).count()
            + usize::from(args.walk.is_some())
            + usize::from(args.diagnostics.is_some())
            > 1
            || selectors > 1
            || selectors != 0 && args.get.is_none()
            || args.symbol.is_some() && args.outline.is_none()
            || args.direction.is_some() && args.walk.is_none() && args.refs.is_none()
            || args.depth.is_some() && args.walk.is_none()
            || (args.frame.is_some() || args.layer.is_some()) && args.pixel.is_none()
            || args.object.is_some() && args.placements.is_none()
            || args.base.is_some()
                && (args.schema.is_some() || args.asset.is_some() || args.session.is_some())
        {
            return Err(fail("choose one query mode and its applicable selectors"));
        }

        if args.advance && args.session.is_none() || args.limit == 0 || args.limit > 256 {
            return Err(fail("invalid query combination or limit outside 1–256"));
        }
        Ok(())
    }
}

fn query_data(args: Query, loaded: &Loaded, index: &OnceCell<Index>) -> Result<Value> {
    args.validate()?;
    if args.session.is_some() || args.schema.is_some() || args.asset.is_some() {
        return Err(fail(
            "schema, asset and session queries must be requested separately",
        ));
    }
    let mut result = json!({});
    if let Some(prefix) = args.diagnostics {
        let index = index.get_or_init(|| Index::build(&loaded.project));
        let matches = index
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.at.starts_with(&prefix));
        page(&mut result, matches, args.offset, args.limit);
        return Ok(result);
    }
    if let Some(roots) = args.walk {
        let roots = match roots {
            Roots::One(root) => vec![root],
            Roots::Many(roots) => roots,
        };
        let depth = args.depth.unwrap_or(8);
        if !(1..=16).contains(&roots.len()) || !(1..=16).contains(&depth) {
            return Err(fail("walk requires 1–16 roots and depth 1–16"));
        }
        let index = index.get_or_init(|| Index::build(&loaded.project));
        for root in &roots {
            if !index.has_node(root) {
                return Err(fail(format!("absent graph node {root}")));
            }
        }
        let visited = index.walk(&roots, args.direction.unwrap_or(Direction::In), depth);
        page(&mut result, visited.into_iter(), args.offset, args.limit);
        return Ok(result);
    }
    if let Some(id) = args.pixel {
        let index = index.get_or_init(|| Index::build(&loaded.project));
        let art = art_at(index, &id)?;
        let frame = args.frame.as_deref();
        if let Some(area @ [_, _, w, h]) = args.area {
            if w.saturating_mul(h) > 4096 {
                return Err(fail("area exceeds 4096 pixels"));
            }
            let pixels = art.pixels(frame, args.layer.as_deref(), Some(area))?;
            let rows = pixels
                .chunks_exact(w)
                .map(|row| {
                    row.iter()
                        .map(|&key| if key == 0 { '.' } else { char::from(key) })
                        .collect::<String>()
                })
                .collect::<Vec<_>>();
            let mut result = json!({"id":format!("pixel/{id}"),"area":area,"data":rows});
            if let Some(layer) = args.layer {
                result["layer"] = json!(layer);
            }
            if let Some(frame) = frame {
                result["frame"] = json!(frame);
            }
            return Ok(result);
        }
        if args.layer.is_some() {
            return Err(fail("layer requires a pixel area"));
        }
        return Ok(json!({
            "id":format!("pixel/{id}"), "size":art.size, "style":art.style,
            "frames":art.frame_names().collect::<Vec<_>>(),
            "layers":art.layers(frame)?.collect::<Vec<_>>()
        }));
    }
    if let Some(id) = args.placements {
        let Some(Resource::Room(room)) = loaded.project.get(Kind::Room, &id) else {
            return Err(fail(format!("absent room/{id}")));
        };
        result["id"] = json!(format!("room/{id}"));
        let placements = room
            .instances
            .iter()
            .filter(|placement| args.object.as_deref().is_none_or(|id| id == placement.1));
        page(&mut result, placements, args.offset, args.limit);
        return Ok(result);
    }
    if let Some(key) = args.outline {
        let source = script_source(&loaded.project, &key)?;
        let program = script::compile(source)?;
        result["id"] = json!(key);
        if let Some(symbol) = args.symbol {
            let entries = program
                .symbols
                .get(&symbol)
                .ok_or_else(|| fail(format!("absent symbol {symbol}")))?;
            if let Some(function) = symbol
                .strip_prefix("fn/")
                .and_then(|name| program.functions.get(name))
            {
                result["parameters"] = json!(function.parameters);
            }
            result["symbol"] = json!(symbol);
            page(&mut result, entries.iter(), args.offset, args.limit);
        } else {
            let events = program
                .locations
                .iter()
                .map(|(event, at)| (event.name(), [at.line, at.count]))
                .collect::<std::collections::BTreeMap<_, _>>();
            result["events"] = json!(events);
            page(&mut result, program.symbols.keys(), args.offset, args.limit);
        }
        return Ok(result);
    }
    if let Some(key) = args.get {
        result["data"] = if let Some(area @ [_, _, width, height]) = args.area {
            let Some(map) = key.strip_prefix("tilemap/") else {
                return Err(fail("area requires tilemap/id"));
            };
            if width.saturating_mul(height) > 4096 {
                return Err(fail("area exceeds 4096 tile cells"));
            }
            let Some(Resource::Tilemap(map)) = loaded.project.get(Kind::Tilemap, map) else {
                return Err(fail(format!("absent {key}")));
            };
            result["area"] = json!(area);
            json!(map.read_area(area)?)
        } else if let Some(event) = args.event {
            let source = script_source(&loaded.project, &key)?;
            let program = script::compile(source)?;
            let location = program
                .locations
                .get(&event)
                .ok_or_else(|| fail(format!("absent {} event", event.name())))?;
            result["lines"] = json!([location.line, location.count]);
            json!(source[location.bytes()].trim_end_matches(['\r', '\n']))
        } else if let Some(name) = args.function {
            let source = script_source(&loaded.project, &key)?;
            let program = script::compile(source)?;
            let function = program
                .functions
                .get(&name)
                .ok_or_else(|| fail(format!("absent function {name}")))?;
            result["lines"] = json!([function.line, 1]);
            json!(
                source
                    .lines()
                    .nth(function.line - 1)
                    .expect("compiled source line")
            )
        } else if key == "project" {
            json!(loaded.project.settings)
        } else if let Some((room, placement)) = key
            .strip_prefix("room/")
            .and_then(|key| key.split_once("/instances/"))
        {
            let Some(Resource::Room(room)) = loaded.project.get(Kind::Room, room) else {
                return Err(fail(format!("absent {key}")));
            };
            json!(
                room.instances
                    .iter()
                    .find(|item| item.0 == placement)
                    .ok_or_else(|| fail(format!("absent {key}")))?
            )
        } else {
            let (kind, id) = key
                .split_once('/')
                .ok_or_else(|| fail("expected kind/id"))?;
            let kind: Kind = kind.parse()?;
            let resource = loaded
                .project
                .get(kind, id)
                .ok_or_else(|| fail(format!("absent {key}")))?;
            if kind.is_source() {
                json!(resource.encode(false))
            } else {
                serde_json::from_str(&resource.encode(false))?
            }
        };
        if let Some(path) = args.path {
            result["data"] = result["data"]
                .pointer(&path)
                .cloned()
                .ok_or_else(|| fail("JSON Pointer does not resolve"))?;
        }
        if let Some([first, count]) = args.lines {
            if first == 0 || !(1..=256).contains(&count) {
                return Err(fail("lines must be [first, count] with count up to 256"));
            }
            let source = result["data"]
                .as_str()
                .ok_or_else(|| fail("lines requires a source resource"))?;
            result["data"] = json!(
                source
                    .lines()
                    .skip(first - 1)
                    .take(count)
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }

        result["id"] = json!(key);
        return Ok(result);
    }
    if let Some(key) = args.refs {
        let index = index.get_or_init(|| Index::build(&loaded.project));
        let links = index
            .references(&key, args.direction.unwrap_or(Direction::Both))
            .into_iter();
        page(&mut result, links, args.offset, args.limit);
    } else {
        let prefix = args.prefix.as_deref().unwrap_or_default();
        let ids = loaded
            .project
            .resources
            .keys()
            .map(|(kind, id)| format!("{kind}/{id}"))
            .chain(std::iter::once("project".into()))
            .filter(|name| name.starts_with(prefix));
        page(&mut result, ids, args.offset, args.limit);
    }
    Ok(result)
}

fn art_at<'a>(index: &'a Index, id: &str) -> Result<&'a agge_core::pixel::Art> {
    if let Some(art) = index.pixels.get(id) {
        return Ok(art);
    }
    let key = format!("pixel/{id}");
    let detail = index.diagnostics.iter().find(|d| {
        d.at == key
            || d.at
                .strip_prefix(&key)
                .is_some_and(|suffix| suffix.starts_with('/'))
    });
    Err(match detail {
        Some(d) => fail(format!("{}: {}", d.at, d.message)),
        None => fail(format!("absent {key}")),
    })
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ArtFrames {
    Name(String),
    List(Vec<String>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtRequest {
    pixel: String,
    base: Option<String>,
    frames: Option<ArtFrames>,
    layer: Option<String>,
    area: Option<[usize; 4]>,
    scale: Option<usize>,
    columns: Option<usize>,
    #[serde(default)]
    compare: bool,
    #[serde(default)]
    ops: Vec<Edit>,
}

pub fn art(store: &Store, args: Value, cache: &mut QueryCache) -> Result<Simulation> {
    let request: ArtRequest = serde_json::from_value(args)?;
    if request.ops.len() > 16 {
        return Err(fail("art preview accepts up to 16 edits"));
    }
    let (loaded, memo) = cache.snapshot(store)?;
    require_revision(loaded, request.base.as_deref())?;
    let preview = !request.ops.is_empty();
    if request.compare && (!preview || request.columns.is_some()) {
        return Err(fail("art comparison requires preview edits and no columns"));
    }
    let candidate;
    let index = if preview {
        let mut project = loaded.project.clone();
        for (i, op) in request.ops.into_iter().enumerate() {
            op.apply(&mut project)
                .map_err(|e| fail(format!("op {}: {e}", i + 1)))?;
        }
        if project.encode().len() > PROJECT_LIMIT {
            return Err(fail("project exceeds 2 MiB"));
        }
        candidate = Index::build(&project);
        candidate.require_valid()?;
        &candidate
    } else {
        memo.get_or_init(|| Index::build(&loaded.project))
    };
    let art = art_at(index, &request.pixel)?;
    let frames = match &request.frames {
        Some(ArtFrames::Name(name)) if name == "*" => art.frame_names().collect(),
        Some(ArtFrames::Name(name)) => vec![name.as_str()],
        Some(ArtFrames::List(names)) => names.iter().map(String::as_str).collect(),
        None => art.frame_names().take(1).collect(),
    };
    let scale = request.scale.unwrap_or(1);
    if request.compare {
        let [frame] = frames.as_slice() else {
            return Err(fail("art comparison accepts one pose"));
        };
        let baseline = memo.get_or_init(|| Index::build(&loaded.project));
        let before = art_at(baseline, &request.pixel)?;
        let diff = pixel_compare(
            before,
            art,
            frame,
            request.layer.as_deref(),
            request.area,
            scale,
            512,
        )?;
        let mut result = json!({
            "revision":loaded.revision,"pixel":request.pixel,"frames":frames,
            "preview":true,"changed":diff.changed
        });
        let png = if let Some(image) = diff.image {
            result["area"] = json!(diff.area);
            result["size"] = json!([image.width(), image.height()]);
            Some(encode_png(&image)?)
        } else {
            None
        };
        return Ok((result, true, png));
    }
    let area = art.area(request.area)?;
    let columns = request.columns.unwrap_or_else(|| frames.len().clamp(1, 4));
    let png = pixel_sheet(
        art,
        &frames,
        request.layer.as_deref(),
        Some(area),
        scale,
        columns,
        512,
    )?;
    let mut result = json!({"revision":loaded.revision,"pixel":request.pixel,"frames":frames,"area":area,"size":[png.width(),png.height()]});
    if preview {
        result["preview"] = json!(true);
    }
    Ok((result, true, Some(encode_png(&png)?)))
}

fn script_source<'a>(project: &'a agge_core::Project, key: &str) -> Result<&'a str> {
    let id = key
        .strip_prefix("script/")
        .ok_or_else(|| fail("expected script/id"))?;
    match project.get(Kind::Script, id) {
        Some(Resource::Script(source)) => Ok(source),
        _ => Err(fail(format!("absent {key}"))),
    }
}

fn page(
    result: &mut Value,
    entries: impl Iterator<Item = impl serde::Serialize>,
    offset: usize,
    limit: usize,
) {
    let mut values = entries.skip(offset).take(limit + 1).collect::<Vec<_>>();
    if values.len() > limit {
        values.pop();
        result["next"] = json!(offset + limit);
    }
    result["items"] = json!(values);
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    #[serde(default)]
    base: Option<String>,
    ops: Vec<Edit>,
    #[serde(default)]
    preview: bool,
    #[serde(default)]
    verify: Vec<Replay>,
}
pub fn edit(store: &Store, args: Value) -> Result<(Value, bool)> {
    let batch: Batch = serde_json::from_value(args)?;
    if batch.ops.is_empty() || batch.ops.len() > 256 {
        return Err(fail("batch must contain 1–256 edits"));
    }
    if batch.verify.len() > 4
        || batch
            .verify
            .iter()
            .map(|spec| u64::from(spec.ticks))
            .sum::<u64>()
            > 6000
        || batch.verify.iter().any(|spec| spec.expect.is_empty())
    {
        return Err(fail(
            "verify accepts up to four replays with assertions and 6000 total ticks",
        ));
    }
    if !batch.preview && batch.verify.is_empty() {
        return Ok((
            json!(store.transact(batch.base.as_deref(), batch.ops)?),
            true,
        ));
    }
    let mut reports = Vec::new();
    let (receipt, passed) =
        store.transact_checked(batch.base.as_deref(), batch.ops, batch.preview, |project| {
            if batch.verify.is_empty() {
                return Ok(true);
            }
            let game = CompiledGame::new(project.clone())?;
            for spec in &batch.verify {
                let (_, report) = replay_compiled(&game, spec)?;
                let passed = report.passed();
                reports.push(report_json(&report, true));
                if !passed {
                    return Ok(false);
                }
            }
            Ok(true)
        })?;
    let mut result = json!(receipt);
    result["committed"] = json!(
        passed && !batch.preview && (!receipt.changed.is_empty() || !receipt.removed.is_empty())
    );
    if !reports.is_empty() {
        result["reports"] = json!(reports);
    }
    Ok((result, passed))
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Knowledge {
    Get { key: Option<String> },
    Put { key: String, value: Value },
    Remove { key: String },
}
pub fn knowledge(store: &Store, args: Value) -> Result<Value> {
    Ok(match serde_json::from_value(args)? {
        Knowledge::Get { key } => json!(store.facts(key.as_deref())?),
        Knowledge::Put { key, value } => {
            store.remember(&key, Some(value))?;
            json!({"key":key})
        }
        Knowledge::Remove { key } => {
            store.remember(&key, None)?;
            json!({"key":key})
        }
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    object: Option<String>,
    placement: Option<String>,
}
pub type Simulation = (Value, bool, Option<Vec<u8>>);

fn run_suite(store: &Store, args: Value, selected: Option<&str>) -> Result<Simulation> {
    let ReplaySuite {
        base,
        detail,
        mut cases,
    }: ReplaySuite<Value> = serde_json::from_value(args)?;
    if !(1..=32).contains(&cases.len()) {
        return Err(fail("suite requires 1–32 named cases"));
    }
    if let Some(selected) = selected {
        if !cases.contains_key(selected) {
            return Err(fail(format!("unknown replay case {selected:?}")));
        }
        cases.retain(|name, _| name == selected);
    }
    let mut replays = Vec::with_capacity(cases.len());
    for (name, case) in cases {
        if !agge_core::valid_id(&name) {
            return Err(fail(format!("invalid replay case name {name:?}")));
        }
        let replay: Replay =
            serde_json::from_value(case).map_err(|error| fail(format!("case {name}: {error}")))?;
        replays.push((name, replay));
    }
    if replays
        .iter()
        .map(|(_, case)| u64::from(case.ticks))
        .sum::<u64>()
        > 120_000
    {
        return Err(fail("suite accepts at most 120000 total ticks"));
    }
    let loaded = store.load()?;
    require_revision(&loaded, base.as_deref())?;
    let game = CompiledGame::new(loaded.project)?;
    let cases = replays.len();
    let (mut assertions, mut ticks, mut failed) = (0, 0, 0);
    let mut reports = BTreeMap::new();
    for (name, case) in replays {
        match replay_compiled(&game, &case) {
            Ok((_, report)) => {
                assertions += report.assertions;
                ticks += report.ticks;
                let passed = report.passed();
                failed += usize::from(!passed);
                if detail || !passed {
                    reports.insert(name, report_json(&report, detail));
                }
            }
            Err(error) => {
                failed += 1;
                reports.insert(name, json!({"error": error.to_string()}));
            }
        }
    }
    let passed = failed == 0;
    let mut result = json!({"revision":loaded.revision,"passed":passed,"cases":cases,"failed":failed,"assertions":assertions,"ticks":ticks});
    if !reports.is_empty() {
        result["reports"] = json!(reports);
    }
    Ok((result, passed, None))
}

fn report_json(report: &Report, include_samples: bool) -> Value {
    const FAILURE_LIMIT: usize = 8;
    let mut result = json!({"ticks":report.ticks,"assertions":report.assertions});
    if include_samples && !report.samples.is_empty() {
        result["samples"] = json!(report.samples);
    }
    if !report.failures.is_empty() {
        let shown = report.failures.len().min(FAILURE_LIMIT);
        result["failures"] = json!(&report.failures[..shown]);
        let omitted = report.failures.len() - shown;
        if omitted > 0 {
            result["omitted_failures"] = json!(omitted);
        }
    }
    if let Some(fault) = &report.fault {
        result["fault"] = json!(fault);
    }
    result
}

#[derive(PartialEq)]
pub enum SimulationMode<'a> {
    Test,
    TestCase(&'a str),
    Inspect,
    Capture,
    Checkpoint,
}

pub fn simulate(store: &Store, mut args: Value, mode: SimulationMode<'_>) -> Result<Simulation> {
    if let SimulationMode::TestCase(selected) = mode {
        if args.get("cases").is_none() {
            return Err(fail("a case name requires a replay suite"));
        }
        return run_suite(store, args, Some(selected));
    }
    if args.get("cases").is_some() {
        if matches!(mode, SimulationMode::Capture | SimulationMode::Checkpoint) {
            return Err(fail("capture and checkpoint require a single replay"));
        }
        return run_suite(store, args, None);
    }
    let fields = args
        .as_object_mut()
        .ok_or_else(|| fail("expected replay object"))?;
    let base: Option<String> =
        serde_json::from_value(fields.remove("base").unwrap_or(Value::Null))?;
    let from: Option<String> =
        serde_json::from_value(fields.remove("from").unwrap_or(Value::Null))?;
    let selection: Option<Selection> = fields
        .remove("select")
        .map(serde_json::from_value)
        .transpose()?;
    let area: Option<[u32; 4]> = fields
        .remove("crop")
        .map(serde_json::from_value)
        .transpose()?;
    let capture_on_failure = mode == SimulationMode::Capture
        && fields
            .remove("capture_on_failure")
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or(false);
    if matches!(mode, SimulationMode::Test | SimulationMode::Checkpoint)
        && (selection.is_some() || area.is_some())
    {
        return Err(fail("test accepts replay fields only"));
    }
    let replay_spec: Replay = serde_json::from_value(args)?;
    let loaded = store.load()?;
    require_revision(&loaded, base.as_deref())?;
    let (world, report) = if let Some(path) = from {
        let bytes = agge_core::read_asset(store.root(), &path)?;
        let checkpoint = Checkpoint::decode(&bytes)?;
        let world = CompiledGame::new(loaded.project)?.restore(checkpoint)?;
        replay_from(world, &replay_spec)?
    } else {
        replay(loaded.project, &replay_spec)?
    };
    let ok = report.passed();
    let mut result =
        json!({"revision":loaded.revision,"passed":ok,"report":report_json(&report,true)});
    if mode == SimulationMode::Checkpoint {
        let bytes = if ok {
            let saved = world.checkpoint()?;
            result["checkpoint"] = json!({"tick": saved.tick(), "room":saved.room()});
            Some(saved.encode()?)
        } else {
            None
        };
        return Ok((result, ok, bytes));
    }
    if let Some(selection) = selection {
        result["state"] =
            json!(world.snapshot(selection.object.as_deref(), selection.placement.as_deref()));
    }
    let area = area.or_else(|| {
        (mode == SimulationMode::Capture).then(|| {
            let [width, height] = world.project().settings.size;
            [0, 0, width, height]
        })
    });
    let bytes = if let Some(area) = area.filter(|_| ok || capture_on_failure) {
        let media = Media::load(world.project(), store.root())?;
        result["crop"] = json!(area);
        Some(capture(&world, &media, area)?)
    } else {
        None
    };
    Ok((result, ok, bytes))
}

fn guide(key: &str) -> Result<Value> {
    if let Some(op) = key.strip_prefix("edit/") {
        let examples = guide("edits")?;
        return examples
            .as_array()
            .and_then(|items| items.iter().find(|item| item["op"] == op))
            .cloned()
            .ok_or_else(|| fail("unknown edit operation"));
    }
    Ok(match key {
        "edits" => json!([
            {"op":"put","kind":"object","id":"player","data":{"size":[20,20]}},
            {"op":"patch","kind":"object","id":"player","fields":{"gravity":480}},
            {"op":"remove","kind":"object","id":"player"},
            {"op":"rename","kind":"object","id":"player","to":"pilot"},
            {"op":"rename_global","id":"score","to":"points"},
            {"op":"pixel_patch","id":"scout","frame":"idle","layer":"face","draw":"rows 7 5 L.K"},
            {"op":"settings","fields":{"hz":60}},
            {"op":"script_event","id":"drive","event":"step","body":"  move 120 * dt, 0"},
            {"op":"splice","id":"drive","at":[3,1],"text":"  move 180 * dt, 0"},
            {"op":"place","room":"main","object":"player","id":"hero","at":[32,48]},
            {"op":"placement","room":"main","id":"hero","to":"pilot","at":[48,48]},
            {"op":"move_placements","room":"main","object":"player","area":[0,0,96,64],"by":[16,0]},
            {"op":"grid","room":"main","object":"player","prefix":"row","at":[0,0],"step":[32,32],"size":[5,2]},
            {"op":"camera","room":"main","size":[1280,720],"follow":"hero","dead":[80,40],"smooth":12},
            {"op":"slice","id":"terrain","cell":[16,16],"grid":[4,2]},
            {"op":"clip","id":"pulse","sprite":"ship","first":4,"count":6,"fps":16,"mode":"once"},
            {"op":"atlas","map":"layout","sprite":"terrain","keys":"abcd","solid":"ab"},
            {"op":"fill","map":"layout","area":[0,0,8,8],"tile":"#","border":true},
            {"op":"resize_map","map":"layout","size":[24,16],"offset":[2,1]},
            {"op":"flood","map":"layout","at":[2,3],"tile":"#"},
            {"op":"replace_tiles","map":"layout","area":[2,3,8,6],"from":"a","to":"b"},
            {"op":"stamp","map":"layout","at":[1,1],"rows":[".#","#."]}
        ]),
        "room" => {
            json!({"bg":[24,30,42,255],"instances":[["hero","player",32,48]],"size":[1280,720],"camera":{"follow":"hero","dead":[80,40],"smooth":12},"maps":["layout"],"lights":[{"at":[64,64],"radius":60,"color":[90,180,240,255]}],"darkness":0.3,"music":"theme","y_sort":true,"meters":[{"at":[8,24],"size":[80,8],"value":"health","max":100}],"ui":[{"at":[8,8],"text":"Score ","value":"score"}]})
        }
        "object" => {
            json!({"size":[24,24],"color":[255,255,255,255],"shape":"rect","sprite":"ship","script":"drive","solid":false,"one_way":false,"hidden":false,"gravity":0,"depth":0})
        }
        "sprite" => json!({"file":"art/ship.png","frames":[[0,0,16,16]]}),
        "sprite/pixel" => json!({"pixel":"scout","poses":["idle","blink"]}),
        "palette" => {
            json!({"swatches":{"k":"172135","t":"417982","T":"90cbd0"},"ramps":["ktT"],"ink":"k","light":[-1,-1]})
        }
        "pixel" => json!({
            "source":"canvas 16 16\nstyle coast\nlayer body\n  oval t 3 3 10 11\n  shade 0 0 16 16\n  outline 0 0 16 16\nframe idle\nframe blink idle\nlayer eyes\n  rows 6 7 k.k\ntween bob idle 4 0 -2",
            "header":["canvas w h","style palette","base pixel","layer name","frame name [from]","tween prefix from count dx dy [layer]","motion prefix from layer=dx,dy/dx,dy/... [layer=...]"],
            "draw":["px c x y","line c x y x2 y2","box|rect|oval c x y w h","poly c x,y ...","rows x y 3a.b/a3b","dither a b x y w h","flood c x y","clear x y w h","recolor a b x y w h","copy x y w h dx dy [transform]","mirror x|y x y w h","use pixel[:pose] x y [transform]","shift dx dy","shade x y w h","outline|outline8 x y w h [ink]"],
            "transforms":["mx","my","r90","r180","r270"],
            "inspect":{"pixel":"scout","frame":"idle","area":[6,5,6,4]},
            "png":{"pixel":"scout","frames":"*","columns":4,"scale":2}
        }),
        "animation" => json!({"sprite":"ship","frames":[0,1],"fps":10,"mode":"ping_pong"}),
        "script" => {
            json!({"events":["create","step","hit_x","hit_y"],"example":"create:\n  set vx = 120\nhit_x:\n  set vx = -impact()","commands":["set name = value","global name = value","move dx, dy","camera x, y","animate clip[, restart]","after name, ticks","spawn object x, y","sound audio","emit particle","goto room","destroy","if value / else / end","repeat count / end","fn name(parameters) = expression"],"builtins":["x","y","vx","vy","dt","tick","width","height","down(\"action\")","pressed(\"action\")","released(\"action\")","touch(\"object\"[,dx,dy])","global(name)","grounded()","blocked(dx,dy)","impact()","anim_done()","remaining(name)","expired(name)","rand()","abs(x)","floor(x)","ceil(x)","sin(x)","cos(x)","sqrt(x)","min(x,y)","max(x,y)","clamp(x,min,max)","choose(test,yes,no)","view(\"x\")","view(\"y\")","view(\"width\")","view(\"height\")"]})
        }
        "tilemap" => {
            json!({"cell":16,"at":[0,0],"atlas":"terrain","tiles":{"#":[[120,140,160,255],true],"a":[0],"b":[1,true]},"rows":["###","#a#","b##"]})
        }
        "audio" => json!({"file":"sound/ping.wav","gain":0.8}),
        "particle" => {
            json!({"count":12,"life":30,"speed":120,"radius":2,"color":[230,200,110,255],"gravity":80})
        }
        "replay" => {
            json!({"ticks":20,"input":[[1,5,"right"]],"expect":[{"at":20,"field":"entity/hero/x","is":48,"tolerance":0.01}],"sample":[[0,"entity/hero/x"]],"fields":["room","particles","camera/x","camera/y","grounded/placement","animation/placement","frame/placement","timer/placement/name","expired/placement/name","music","sound/audio","count/object","global/name","entity/placement/property"]})
        }
        "suite" => {
            json!({"cases":{"arrival":{"ticks":0,"expect":[{"at":0,"field":"room","is":"main"}]}},"detail":false})
        }
        "project" => json!(agge_core::Settings::default()),
        _ => {
            return Err(fail(
                "unknown schema; use project, edits, replay, or a resource kind",
            ));
        }
    })
}
