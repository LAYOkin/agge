mod mcp;
mod service;

use agge_core::{Project, Result, asset_path, fail, read_asset, store::Store};
use agge_runtime::Media;
use serde_json::{Value, json};
use std::{
    env,
    ffi::OsStr,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

const HELP: &str = "AGGE 0.1.0
agge init PROJECT
agge check PROJECT
agge query PROJECT [JSON]
agge edit PROJECT JSON
agge undo|redo PROJECT [REVISION]
agge test PROJECT JSON [CASE]
agge inspect PROJECT JSON
agge capture PROJECT JSON OUTPUT.png
agge checkpoint PROJECT JSON OUTPUT.aggesave
agge art PROJECT JSON OUTPUT.png
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
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let Some(command) = args.first() else {
        println!("{HELP}");
        return Ok(true);
    };
    let command = argument_text(command)?;
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
        "edit" | "inspect" | "knowledge" => 3..=3,
        "capture" | "art" | "checkpoint" => 4..=4,
        "export" | "test" => 3..=4,
        _ => return Err(fail(format!("unknown command {command}"))),
    };
    if !arity.contains(&args.len()) {
        return Err(fail("wrong number of arguments; see agge --help"));
    }
    match command {
        "init" => {
            store.init(&Project::default())?;
            output(json!({"project": file.to_string_lossy()}))
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
        "edit" => {
            let (value, ok) = service::edit(&store, argument()?)?;
            println!("{value}");
            Ok(ok)
        }
        "undo" | "redo" => {
            let revision = args.get(2).map(|arg| argument_text(arg)).transpose()?;
            output(json!(store.travel(revision, command == "redo")?))
        }
        "knowledge" => output(service::knowledge(&store, argument()?)?),
        "art" => {
            let (value, _, image) =
                service::art(&store, argument()?, &mut service::QueryCache::default())?;
            if let Some(bytes) = image {
                write_capture(store.path(), Path::new(&args[3]), &bytes)?;
            }
            output(value)
        }
        "test" | "inspect" | "capture" | "checkpoint" => {
            let mode = match command {
                "test" => match args.get(3) {
                    Some(case) => service::SimulationMode::TestCase(argument_text(case)?),
                    None => service::SimulationMode::Test,
                },
                "capture" => service::SimulationMode::Capture,
                "checkpoint" => service::SimulationMode::Checkpoint,
                _ => service::SimulationMode::Inspect,
            };
            let (value, ok, image) = service::simulate(&store, argument()?, mode)?;
            match (image, command) {
                (Some(bytes), "checkpoint") => {
                    agge_core::store::write_atomic(Path::new(&args[3]), &bytes, false)?;
                }
                (Some(bytes), "capture") => {
                    write_capture(store.path(), Path::new(&args[3]), &bytes)?;
                }
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

fn write_capture(project: &Path, target: &Path, bytes: &[u8]) -> Result<()> {
    let source = same_file::Handle::from_path(project)?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(target)?;
    let mut output = same_file::Handle::from_file(file)?;
    if source == output {
        return Err(fail("capture output would overwrite the project"));
    }
    let output = output.as_file_mut();
    if output.metadata()?.is_file() {
        output.set_len(0)?;
    }
    output.write_all(bytes)?;
    Ok(())
}

fn argument_text(arg: &OsStr) -> Result<&str> {
    arg.to_str()
        .ok_or_else(|| fail("expected a UTF-8 command, JSON argument or revision"))
}

fn read_json(input: &OsStr) -> Result<Value> {
    const LIMIT: usize = 4 * 1024 * 1024;
    let input = argument_text(input)?;
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
    let (loaded, source) = store.load_with_source()?;
    agge_core::Index::build(&loaded.project).require_valid()?;
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
    fs::write(stage.path().join("game.agge"), source)?;
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
    Media::load(&loaded.project, stage.path())?;
    fs::rename(stage.path(), output)?;
    Ok(
        json!({"directory":output.to_string_lossy(), "binary":binary_name, "assets":assets.len(), "revision":loaded.revision}),
    )
}
