fn main() -> std::process::ExitCode {
    let file = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .map(|exe| exe.with_file_name("game.agge"))
        });
    let result = file
        .ok_or_else(|| agge_core::fail("expected project path"))
        .and_then(agge_studio::run_player);
    if let Err(error) = result {
        eprintln!("{error}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
