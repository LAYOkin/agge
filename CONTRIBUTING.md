# Contributing to AGGE

Start with the smallest fixture that reproduces the behavior. For an editor or runtime issue, include a `.agge` project, its relative assets, and a replay when the issue can be expressed with tick-based inputs and assertions. For a CLI or MCP issue, include the command or JSON arguments and the resulting diagnostic.

## Develop locally

Use Rust stable. The [README](README.md) lists platform dependencies and how to launch Studio. Run the same checks as CI before opening a change.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p agge -- test examples/orbit/game.agge @examples/orbit/replay.json
cargo run -p agge -- test examples/paddle/game.agge @examples/paddle/replay.json
```

The [architecture guide](docs/architecture.md) describes crate boundaries. Put format and validation behavior in `agge-core`, simulation in `agge-runtime`, shared CLI and MCP behavior in the `agge` service, and native UI behavior in `agge-studio`. When an edit operation works in both the CLI and MCP, use the same validation path and cover the normal entry points.

## Measure changes

For token-efficiency work, capture both workflows and check that they produce equivalent results before comparing counts. Separate scripted transcripts from provider-reported session usage and include unsuccessful trials. For throughput changes, compare release builds on the same machine and record the fixture, number of repetitions, and timing scope. [Benchmark instructions](bench/README.md)

Commit source projects, assets, and replay specs with examples. AGGE's lock, history, and knowledge files are machine-local sidecars. Consult [AGENTS.md](AGENTS.md) for the project editing workflow.
