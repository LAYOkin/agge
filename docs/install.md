# Install AGGE from source

Install Rust 1.89 or newer. Ubuntu needs `pkg-config libasound2-dev libgl1-mesa-dev libxkbcommon-dev libwayland-dev` for the desktop apps. Windows and macOS use the platform audio and graphics APIs.

From the repository root, build the CLI, Studio, and desktop player together.

```sh
cargo build --workspace --release --locked
```

The executables are `agge`, `agge-studio`, and `agge-player` in `target/release`. On Windows they have an `.exe` extension. To keep a portable copy, put the three binaries in the same directory so `agge export` can find `agge-player`.

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
