# Install AGGE

Download your platform's pre-alpha archive and SHA-256 file from a successful [Portable development builds run](https://github.com/LAYOkin/agge/actions/workflows/artifacts.yml). Extract the archive and keep its three executables together. The Windows archive uses Vulkan and needs a compatible graphics driver. The archives are unsigned. Linux builds need the host's ALSA and graphics libraries.

To verify the archive on Linux or macOS, use `sha256sum -c ARCHIVE.sha256` where available, or `shasum -a 256 -c ARCHIVE.sha256`. In PowerShell, compare `(Get-FileHash ARCHIVE -Algorithm SHA256).Hash` with the first value in the companion file.

## Build from source

Install Rust 1.89 or newer. Ubuntu needs `pkg-config libasound2-dev libgl1-mesa-dev libxkbcommon-dev libwayland-dev` for the desktop apps. Windows and macOS use the platform audio and graphics APIs.

From the repository root, build the CLI, Studio, and desktop player together.

```sh
cargo build --workspace --release --locked
```

The executables are `agge`, `agge-studio`, and `agge-player` in `target/release`. On Windows they have an `.exe` extension. To keep a portable copy, put the three binaries in the same directory so `agge export` can find `agge-player`.

If Studio or the player fails to open a window on Windows, a build with Vulkan support is available for systems with a Vulkan-capable driver:

```sh
cargo build -p agge-studio --bins --release --locked --features vulkan
```

This selects Vulkan for both desktop executables. Games exported with that player also use Vulkan.

You can also install the binaries into Cargo's `bin` directory.

```sh
cargo install --locked --path crates/cli
cargo install --locked --path crates/studio --bins
```

Run the Cinder Circuit suite and open the same project in Studio. In PowerShell, quote the replay argument beginning with `@`.

```sh
agge test examples/cinder/game.agge '@examples/cinder/replay.json'
agge-studio examples/cinder/game.agge
```

Use `agge init path/to/game.agge` to start a new project. Configure local agent tools with the [MCP guide](agents.md), or start from the [Studio tutorial](first-game.md).

To preview a particular room without editing the launch room, pass its ID to the desktop player. For example, `agge-player --room clear examples/orbit/game.agge` opens Signal Runner's completion screen.

Launch with `agge-player --save path/to/session.aggesave PROJECT` to enable F6 quicksave and F9 restore. The player validates the save against the canonical project source before restoring it. Game source and assets stay in their original files.

Games with a progress schema load and save player data under `agge/<game-id>/progress.json` in the user's local data directory. This is `LOCALAPPDATA` on Windows, `~/Library/Application Support` on macOS, and `XDG_DATA_HOME` or `~/.local/share` on Linux. Use `agge-player --progress path/to/test.json PROJECT` for a separate slot. The player writes progress only when the game's script requests a save.

## Make a portable build

After an optimized build, run `python tools/package_release.py --label dev` with Python 3.11 or newer. The script puts the archive and checksum in `target/dist`. It checks the bundled projects and replays, exports Cinder Circuit and Glass Yard, and verifies a progress migration on the exported Glass Yard project. CI runs this flow per platform for packaging changes, version tags, and on demand.
