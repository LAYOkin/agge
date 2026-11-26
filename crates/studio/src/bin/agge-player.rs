use agge_core::{Result, fail, valid_id};
use std::{env, path::PathBuf, process::ExitCode};

const HELP: &str = "Usage: agge-player [--room ROOM] [PROJECT]

With no project path, load game.agge beside the player.";

fn main() -> ExitCode {
    if let Err(error) = run() {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let (mut file, mut room) = (None, None);
    let mut options = true;
    while let Some(arg) = args.next() {
        match arg.to_str().filter(|_| options) {
            Some("--help" | "-h") => {
                println!("{HELP}");
                return Ok(());
            }
            Some("--") => options = false,
            Some("--room") if room.is_none() => {
                room = Some(
                    args.next()
                        .and_then(|id| id.into_string().ok())
                        .filter(|id| valid_id(id))
                        .ok_or_else(|| fail("--room requires a lowercase room ID"))?,
                );
            }
            Some("--room") => return Err(fail("--room can only be given once")),
            _ if options && arg.as_encoded_bytes().starts_with(b"-") => {
                return Err(fail(format!(
                    "unknown option {}; see --help",
                    arg.to_string_lossy()
                )));
            }
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => return Err(fail("expected one project path; see --help")),
        }
    }
    let file = match file {
        Some(path) => path,
        None => env::current_exe()?.with_file_name("game.agge"),
    };
    agge_studio::run_player_in_room(file, room.as_deref())
}
