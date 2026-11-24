use agge_core::{
    Edit, Index, Kind, Resource, Result, fail,
    script::{self, Event},
    store::Store,
};
use agge_runtime::{Media, Replay, capture, replay};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn check(store: &Store) -> Result<(Value, bool)> {
    let loaded = store.load()?;
    let index = Index::build(&loaded.project);
    let mut diagnostics = json!(index.diagnostics);
    if index.diagnostics.is_empty()
        && let Err(error) = Media::load(&loaded.project, store.root())
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
    schema: Option<String>,
    path: Option<String>,
    lines: Option<[usize; 2]>,
    event: Option<Event>,
    function: Option<String>,
    outline: Option<String>,
    symbol: Option<String>,
    refs: Option<String>,
    prefix: Option<String>,
    session: Option<String>,
    #[serde(default)]
    advance: bool,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    64
}

pub fn query(store: &Store, args: Value) -> Result<Value> {
    let args: Query = serde_json::from_value(args)?;
    let modes = [
        &args.get,
        &args.refs,
        &args.outline,
        &args.prefix,
        &args.schema,
        &args.session,
    ];
    let selectors = usize::from(args.path.is_some())
        + usize::from(args.lines.is_some())
        + usize::from(args.event.is_some())
        + usize::from(args.function.is_some());
    if modes.into_iter().filter(|option| option.is_some()).count() > 1
        || selectors > 1
        || selectors != 0 && args.get.is_none()
        || args.symbol.is_some() && args.outline.is_none()
    {
        return Err(fail("choose one query mode and its applicable selectors"));
    }

    if let Some(schema) = args.schema {
        return guide(&schema);
    }
    if let Some(session) = args.session {
        if args.get.is_some() || args.refs.is_some() || args.prefix.is_some() || args.offset > 0 {
            return Err(fail("session delta must be queried by itself"));
        }
        return Ok(json!(store.delta(&session, args.advance)?));
    }
    if args.advance
        || args.limit == 0
        || args.limit > 256
        || args.get.is_some() && args.refs.is_some()
    {
        return Err(fail("invalid query combination or limit outside 1–256"));
    }
    let loaded = store.load()?;
    let mut result = json!({"revision":loaded.revision});
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
        result["data"] = if let Some(event) = args.event {
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
        } else {
            let (kind, id) = key
                .split_once('/')
                .ok_or_else(|| fail("expected kind/id"))?;
            let kind: Kind = kind.parse()?;
            let resource = loaded
                .project
                .get(kind, id)
                .ok_or_else(|| fail(format!("absent {key}")))?;
            if kind == Kind::Script {
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
                .ok_or_else(|| fail("lines requires a script"))?;
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
        let index = Index::build(&loaded.project);
        let links = index
            .edges
            .iter()
            .filter(|edge| edge.from == key || edge.to == key);
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
}
pub fn edit(store: &Store, args: Value) -> Result<Value> {
    let batch: Batch = serde_json::from_value(args)?;
    if batch.ops.is_empty() || batch.ops.len() > 256 {
        return Err(fail("batch must contain 1–256 edits"));
    }
    Ok(json!(store.transact(batch.base.as_deref(), batch.ops)?))
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

pub fn simulate(store: &Store, mut args: Value, test_only: bool) -> Result<Simulation> {
    let fields = args
        .as_object_mut()
        .ok_or_else(|| fail("expected replay object"))?;
    let selection: Option<Selection> = fields
        .remove("select")
        .map(serde_json::from_value)
        .transpose()?;
    let area: Option<[u32; 4]> = fields
        .remove("crop")
        .map(serde_json::from_value)
        .transpose()?;
    if test_only && (selection.is_some() || area.is_some()) {
        return Err(fail("test accepts replay fields only"));
    }
    let replay_spec: Replay = serde_json::from_value(args)?;
    let loaded = store.load()?;
    let (world, report) = replay(loaded.project, &replay_spec)?;
    let ok = report.passed();
    let mut result = json!({"revision":loaded.revision,"passed":ok,"report":report});
    if !test_only && let Some(selection) = selection {
        result["state"] =
            json!(world.snapshot(selection.object.as_deref(), selection.placement.as_deref()));
    }
    let bytes = if let Some(area) = area.filter(|_| ok) {
        let media = Media::load(world.project(), store.root())?;
        result["crop"] = json!(area);
        Some(capture(&world, &media, area)?)
    } else {
        None
    };
    Ok((result, ok, bytes))
}

fn guide(key: &str) -> Result<Value> {
    Ok(match key {
        "edits" => json!([
            {"op":"put","kind":"object","id":"player","data":{"size":[20,20]}},
            {"op":"patch","kind":"object","id":"player","fields":{"gravity":480}},
            {"op":"remove","kind":"object","id":"player"},
            {"op":"rename","kind":"object","id":"player","to":"pilot"},
            {"op":"settings","fields":{"hz":60}},
            {"op":"script_event","id":"drive","event":"step","body":"  move 120 * dt, 0"},
            {"op":"splice","id":"drive","at":[3,1],"text":"  move 180 * dt, 0"},
            {"op":"place","room":"main","object":"player","id":"hero","at":[32,48]},
            {"op":"grid","room":"main","object":"player","prefix":"row","at":[0,0],"step":[32,32],"size":[5,2]},
            {"op":"camera","room":"main","size":[1280,720],"follow":"hero","dead":[80,40],"smooth":12},
            {"op":"slice","id":"terrain","cell":[16,16],"grid":[4,2]},
            {"op":"atlas","map":"layout","sprite":"terrain","keys":"abcd","solid":"ab"},
            {"op":"fill","map":"layout","area":[0,0,8,8],"tile":"#","border":true},
            {"op":"stamp","map":"layout","at":[1,1],"rows":[".#","#."]}
        ]),
        "room" => {
            json!({"bg":[24,30,42,255],"instances":[["hero","player",32,48]],"size":[1280,720],"camera":{"follow":"hero","dead":[80,40],"smooth":12},"maps":["layout"],"lights":[{"at":[64,64],"radius":60,"color":[90,180,240,255]}],"darkness":0.3,"ui":[{"at":[8,8],"text":"Score ","value":"score"}]})
        }
        "object" => {
            json!({"size":[24,24],"color":[255,255,255,255],"shape":"rect","sprite":"ship","script":"drive","solid":false,"one_way":false,"hidden":false,"gravity":0})
        }
        "sprite" => json!({"file":"art/ship.png","frames":[[0,0,16,16]]}),
        "animation" => json!({"sprite":"ship","frames":[0,1],"fps":10}),
        "script" => {
            json!({"example":"create:\n  set vx = 120\nstep:\n  if pressed(\"fire\")\n    spawn shot x, y\n  end","commands":["set name = value","global name = value","move dx, dy","camera x, y","spawn object x, y","sound audio","emit particle","goto room","destroy","if value / else / end","repeat count / end","fn name(parameters) = expression"],"builtins":["x","y","vx","vy","dt","tick","width","height","down(\"action\")","pressed(\"action\")","touch(\"object\")","global(name)","grounded()","blocked(dx,dy)","rand()","abs(x)","floor(x)","ceil(x)","sin(x)","cos(x)","sqrt(x)","min(x,y)","max(x,y)","clamp(x,min,max)","choose(test,yes,no)","view(\"x\")","view(\"y\")","view(\"width\")","view(\"height\")"]})
        }
        "tilemap" => {
            json!({"cell":16,"at":[0,0],"atlas":"terrain","tiles":{"#":[[120,140,160,255],true],"a":[0],"b":[1,true]},"rows":["###","#a#","b##"]})
        }
        "audio" => json!({"file":"sound/ping.wav","gain":0.8}),
        "particle" => {
            json!({"count":12,"life":30,"speed":120,"radius":2,"color":[230,200,110,255],"gravity":80})
        }
        "replay" => {
            json!({"ticks":20,"input":[[1,5,"right"]],"expect":[{"at":20,"field":"entity/hero/x","is":48,"tolerance":0.01}],"fields":["room","particles","camera/x","camera/y","grounded/placement","count/object","global/name","entity/placement/property"]})
        }
        "project" => json!(agge_core::Settings::default()),
        _ => {
            return Err(fail(
                "unknown schema; use project, edits, replay, or a resource kind",
            ));
        }
    })
}
