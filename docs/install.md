# Install AGGE from source

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
