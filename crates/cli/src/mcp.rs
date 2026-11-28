use crate::service;
use agge_core::{Result, fail, store::Store};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};

const LATEST: &str = "2025-11-25";
const VERSIONS: [&str; 4] = [LATEST, "2025-06-18", "2025-03-26", "2024-11-05"];
const LIMIT: u64 = 4 * 1024 * 1024;

pub fn serve(store: &Store) -> Result<()> {
    store.load()?;
    let mut input = io::stdin().lock();
    let mut out = io::stdout().lock();
    let mut phase = Phase::New;
    let mut queries = service::QueryCache::default();
    loop {
        let mut line = Vec::new();
        let n = Read::by_ref(&mut input)
            .take(LIMIT + 1)
            .read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        if n as u64 > LIMIT {
            writeln!(
                out,
                "{}",
                error(Value::Null, -32700, "request exceeds 4 MiB")
            )?;
            break;
        }
        let response = match serde_json::from_slice::<Value>(&line) {
            Ok(message) => respond(store, &mut phase, &mut queries, message),
            Err(_) => Some(error(Value::Null, -32700, "invalid JSON")),
        };
        if let Some(response) = response {
            writeln!(out, "{response}")?;
            out.flush()?;
        }
    }
    Ok(())
}

#[derive(PartialEq)]
enum Phase {
    New,
    Initializing,
    Ready,
}

fn respond(
    store: &Store,
    phase: &mut Phase,
    queries: &mut service::QueryCache,
    message: Value,
) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message["method"].as_str();
    if message["jsonrpc"] != "2.0"
        || method.is_none()
        || id
            .as_ref()
            .is_some_and(|v| !v.is_string() && !v.is_i64() && !v.is_u64())
    {
        return Some(error(Value::Null, -32600, "invalid request"));
    }
    let method = method.expect("checked request method");
    let Some(id) = id else {
        if method == "notifications/initialized" && *phase == Phase::Initializing {
            *phase = Phase::Ready;
        }
        return None;
    };
    let params = &message["params"];
    let result = match method {
        "ping" => json!({}),
        "initialize" if *phase == Phase::New => {
            let Some(version) = params["protocolVersion"].as_str() else {
                return Some(error(id, -32602, "expected protocolVersion"));
            };
            *phase = Phase::Initializing;
            json!({
                "protocolVersion": if VERSIONS.contains(&version) { version } else { LATEST },
                "capabilities":{"tools":{}},
                "serverInfo":{"name":"agge","version":env!("CARGO_PKG_VERSION")},
                "instructions":"Query IDs or session changes first. Edit with the returned base revision. Run replay assertions before widening inspection."
            })
        }
        _ if *phase != Phase::Ready => return Some(error(id, -32002, "server is initializing")),
        "tools/list" => json!({"tools":tool_list()}),
        "tools/call" => {
            let Some(name) = params["name"].as_str() else {
                return Some(error(id, -32602, "expected tool name"));
            };
            if !NAMES.contains(&name) {
                return Some(error(id, -32602, "unknown tool"));
            }
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            call(store, name, args, queries)
        }
        _ => return Some(error(id, -32601, "method not found")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

const NAMES: [&str; 7] = [
    "agge_query",
    "agge_edit",
    "agge_check",
    "agge_run",
    "agge_history",
    "agge_knowledge",
    "agge_art",
];

fn tool_list() -> Vec<Value> {
    let text = json!({"type":"string"});
    let number = json!({"type":"integer"});
    let boolean = json!({"type":"boolean"});
    let array = json!({"type":"array"});
    let tool = |name, description, properties, required| {
        json!({"name":name,"description":description,
            "inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
    };
    vec![
        tool(
            NAMES[0],
            "Read fields, pixel areas, diagnostics, or a transitive dependency walk. read batches queries at one revision.",
            json!({"get":text,"asset":text,"path":text,"lines":array,"event":text,"function":text,"pixel":text,"frame":text,"layer":text,"area":array,"placements":text,"object":text,"outline":text,"symbol":text,"refs":text,"walk":{"type":["string","array"]},"direction":{"enum":["in","out","both"]},"depth":number,"diagnostics":text,"prefix":text,"session":text,"advance":boolean,"offset":number,"limit":number,"schema":text,"read":array,"base":text}),
            json!([]),
        ),
        tool(
            NAMES[1],
            "Atomic edits. preview leaves the project untouched. verify requires replays to pass before commit. Query schema edits for forms.",
            json!({"ops":{"type":"array","items":{"type":"object"}},"base":text,"preview":boolean,"verify":array}),
            json!(["ops"]),
        ),
        tool(
            NAMES[2],
            "Validate dependencies, scripts and assets.",
            json!({}),
            json!([]),
        ),
        tool(
            NAMES[3],
            "Replay ticks with input, assertions, watches, scoped state, tile rows or a crop. Or run named replay cases with one summary.",
            json!({"ticks":number,"from":text,"room":text,"seed":{"type":"integer","minimum":0},"input":array,"pointer":array,"progress":{"type":"object"},"progress_out":boolean,"expect":array,"sample":{"type":["array","object"]},"watch":{"type":"object"},"select":{"type":"object"},"tiles":{"type":"object","properties":{"map":text,"area":array},"required":["map","area"],"additionalProperties":false},"crop":array,"cases":{"type":"object"},"detail":boolean,"base":text}),
            json!([]),
        ),
        tool(
            NAMES[4],
            "Undo or redo a transaction.",
            json!({"redo":boolean,"base":text}),
            json!(["redo"]),
        ),
        tool(
            NAMES[5],
            "Read or persist a structured project fact. op is get, put or remove.",
            json!({"op":text,"key":text,"value":{}}),
            json!(["op"]),
        ),
        tool(
            NAMES[6],
            "Inspect pixel poses as PNG. Compare one edited pose against base to get changed pixels and a cropped before/after/mask.",
            json!({"pixel":text,"frames":{"type":["string","array"]},"layer":text,"area":array,"scale":number,"columns":number,"compare":boolean,"base":text,"ops":array}),
            json!(["pixel"]),
        ),
    ]
}

fn call(store: &Store, name: &str, args: Value, queries: &mut service::QueryCache) -> Value {
    let computed = (|| -> Result<service::Simulation> {
        if !args.is_object() {
            return Err(fail("tool arguments must be an object"));
        }
        let value = match name {
            "agge_query" => service::query_with_cache(store, args, queries)?,
            "agge_edit" => {
                let (value, ok) = service::edit(store, args)?;
                return Ok((value, ok, None));
            }
            "agge_check" => {
                if args != json!({}) {
                    return Err(fail("check takes no arguments"));
                }
                let (value, ok) = service::check(store)?;
                return Ok((value, ok, None));
            }
            "agge_run" => {
                if args["crop"].as_array().is_some_and(|crop| {
                    crop.iter()
                        .skip(2)
                        .any(|n| n.as_u64().is_some_and(|n| n > 512))
                }) {
                    return Err(fail("MCP image crop is limited to 512×512"));
                }
                return service::simulate(store, args, service::SimulationMode::Inspect);
            }
            "agge_history" => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Travel {
                    redo: bool,
                    base: Option<String>,
                }
                let travel: Travel = serde_json::from_value(args)?;
                json!(store.travel(travel.base.as_deref(), travel.redo)?)
            }
            "agge_knowledge" => service::knowledge(store, args)?,
            "agge_art" => return service::art(store, args, queries),
            _ => unreachable!(),
        };
        Ok((value, true, None))
    })();
    let (text, ok, image) = match computed {
        Ok((value, ok, image)) => (value.to_string(), ok, image),
        Err(e) => (e.to_string(), false, None),
    };
    let mut content = vec![json!({"type":"text","text":text})];
    if let Some(bytes) = image {
        content.push(json!({"type":"image","mimeType":"image/png","data":STANDARD.encode(bytes)}));
    }
    json!({"content":content,"isError":!ok})
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
