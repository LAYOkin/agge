# AGGE

**Artificial General Game Engine** is a local, MIT-licensed 2D game engine written in Rust. The editor, CLI, and MCP server share the same project model and transaction layer. Games run in a separate desktop player that can be bundled with the project and its assets.

An agent can ask for an asset ID, a JSON field, a script range, or the changes since a saved checkpoint. It can then place a grid of objects, stamp a tilemap, or rename a resource and its references in a single transaction. A fixed-tick replay can verify the result with assertions and return a PNG crop if visual inspection is needed.

## Play or edit a game

Install Rust 1.89 or newer. On Linux, install ALSA and the development libraries for your desktop graphics stack. For Ubuntu, the CI setup uses `pkg-config libasound2-dev libgl1-mesa-dev libxkbcommon-dev libwayland-dev`.

```sh
cargo build --workspace --release --locked
cargo run -p agge-studio --bin agge-studio -- examples/orbit/game.agge
cargo run -p agge-studio --bin agge-player -- examples/paddle/game.agge
cargo run -p agge -- test examples/relay/game.agge @examples/relay/replay.json
```

In **Signal Runner**, use the arrows or WASD to collect three power cells and avoid the patrol. In **Rail Paddle**, use left and right to keep the ball in play. In **Relay Courier**, steer around the barriers, collect three relays, and reach the gate in a scrolling room. All three games include reproducible input and gameplay assertions.

## Learn AGGE

- [Editor and first game](docs/first-game.md) covers building a playable room, creating a replay, and packaging a desktop build.
- [Project and scripting reference](docs/reference.md) describes assets, serialization, input, collision, and the language.
- [Agent interface](docs/agents.md) documents MCP setup, revision checks, operations, checkpoints, and selective inspection.
- [Architecture](docs/architecture.md) describes crate boundaries, determinism, and the current limits.
- [Benchmarks](bench/README.md) record measured workflow token counts and describe how to collect model-session usage.

## Current release scope

The editor supports room placement, object properties, sprite-sheet frames, animation playback, tile painting, scripts, audio audition, and particle previews. The runtime supports fixed-step movement and AABB collision, gravity, scrolling cameras, sprites, animation, room lighting, sound, keyboard actions, HUD labels, and particles. The Windows editor, headless workflow, and desktop package were exercised locally. CI is configured for Windows, Linux, and macOS builds.

The engine is at version 0.1. It has no browser or mobile player, rigid-body solver, in-editor pixel painter, or general UI layout system. Token reductions vary by task. The current benchmark measures scripted edit transcripts using a real tokenizer, with engine-level correctness checks. It has yet to establish the 80–90% goal in model sessions.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The engine and bundled examples are distributed under the [MIT License](LICENSE).
