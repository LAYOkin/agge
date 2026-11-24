mod mcp;
mod service;

use agge_core::{Project, Result, asset_path, fail, read_asset, store::Store};
use agge_runtime::Media;
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitCode,
};

const HELP: &str = "AGGE 0.1.0
agge init PROJECT
agge check PROJECT
agge query PROJECT [JSON]
agge edit PROJECT JSON
agge undo|redo PROJECT [REVISION]
agge test|inspect PROJECT JSON
agge capture PROJECT JSON OUTPUT.png
agge knowledge PROJECT JSON
agge mcp PROJECT
agge export PROJECT DIRECTORY [PLAYER_BINARY]

JSON can be inline, @file, or - for stdin. PROJECT is a .agge file.";

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = args.first().map(String::as_str) else {
        println!("{HELP}");
        return Ok(true);
    };
    if matches!(command, "help" | "--help" | "-h") {
        println!("{HELP}");
        return Ok(true);
    }
    if matches!(command, "--version" | "-V") {
        println!("agge {}", env!("CARGO_PKG_VERSION"));
        return Ok(true);
    }
    let file = args
        .get(1)
        .ok_or_else(|| fail("expected project path; see agge --help"))?;
    let store = Store::new(file);
    let argument = || {
        args.get(2)
            .ok_or_else(|| fail("expected JSON, @file, or -"))
            .and_then(|s| read_json(s))
    };
    let output = |value: Value| {
        println!("{value}");
        Ok(true)
    };
    let arity = match command {
        "init" | "check" | "mcp" => 2..=2,
        "query" | "undo" | "redo" => 2..=3,
        "edit" | "inspect" | "knowledge" | "test" => 3..=3,
        "capture" => 4..=4,
        "export" => 3..=4,
        _ => return Err(fail(format!("unknown command {command}"))),
    };
    if !arity.contains(&args.len()) {
        return Err(fail("wrong number of arguments; see agge --help"));
    }
    match command {
        "init" => {
            store.init(&Project::default())?;
            output(json!({"project": file}))
        }
        "mcp" => mcp::serve(&store).map(|()| true),
        "check" => {
            let (value, ok) = service::check(&store)?;
            println!("{value}");
            Ok(ok)
        }
        "query" => output(service::query(
            &store,
            args.get(2)
                .map_or_else(|| Ok(json!({})), |s| read_json(s))?,
        )?),
        "edit" => output(service::edit(&store, argument()?)?),
        "undo" | "redo" => output(json!(
            store.travel(args.get(2).map(String::as_str), command == "redo")?
        )),
        "knowledge" => output(service::knowledge(&store, argument()?)?),
        "test" | "inspect" | "capture" => {
            let (value, ok, image) = service::simulate(&store, argument()?, command == "test")?;
            match (image, command) {
                (Some(bytes), "capture") => {
                    fs::write(&args[3], bytes)?;
                }
                (None, "capture") if ok => return Err(fail("capture requires a crop")),
                (Some(_), _) => return Err(fail("use agge capture to write a crop")),
                _ => (),
            }
            println!("{value}");
            Ok(ok)
        }
        "export" => output(export(
            &store,
            Path::new(&args[2]),
            args.get(3).map(PathBuf::from),
        )?),
        _ => unreachable!(),
    }
}

fn read_json(input: &str) -> Result<Value> {
    const LIMIT: usize = 4 * 1024 * 1024;
    let mut content = String::new();
    let text = if input == "-" {
        io::stdin()
            .take(LIMIT as u64 + 1)
            .read_to_string(&mut content)?;
        &content
    } else if let Some(file) = input.strip_prefix('@') {
        fs::File::open(file)?
            .take(LIMIT as u64 + 1)
            .read_to_string(&mut content)?;
        &content
    } else {
        input
    };
    if text.len() > LIMIT {
        return Err(fail("input exceeds 4 MiB"));
    }
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}

fn export(store: &Store, output: &Path, player: Option<PathBuf>) -> Result<Value> {
    let loaded = store.load()?;
    agge_core::Index::build(&loaded.project).require_valid()?;
    Media::load(&loaded.project, store.root())?;
    if output.exists() {
        return Err(fail("export directory already exists"));
    }
    let player = match player {
        Some(path) => path,
        None => {
            env::current_exe()?.with_file_name(format!("agge-player{}", env::consts::EXE_SUFFIX))
        }
    };
    let binary_name = if player
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        "play.exe"
    } else {
        "play"
    };
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent)?;
    fs::copy(&player, stage.path().join(binary_name))
        .map_err(|e| fail(format!("player {}: {e}", player.display())))?;
    fs::write(stage.path().join("game.agge"), loaded.project.encode())?;
    fs::write(
        stage.path().join("AGGE-LICENSE.txt"),
        include_str!("../../../LICENSE"),
    )?;
    let assets = loaded.project.asset_paths();
    for file in &assets {
        let relative = asset_path(file)?;
        if relative.components().next().is_some_and(|c| {
            ["game.agge", "play", "play.exe", "AGGE-LICENSE.txt"]
                .iter()
                .any(|name| c.as_os_str().eq_ignore_ascii_case(name))
        }) {
            return Err(fail(format!("asset overlaps export entry: {file}")));
        }
        let target = stage.path().join(relative);
        fs::create_dir_all(target.parent().expect("relative asset has staging parent"))?;
        fs::write(target, read_asset(store.root(), file)?)?;
    }
    fs::rename(stage.path(), output)?;
    Ok(
        json!({"directory":output, "binary":binary_name, "assets":assets.len(), "revision":loaded.revision}),
    )
}
