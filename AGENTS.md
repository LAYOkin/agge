# Working on AGGE

AGGE uses Rust. Run `cargo test --workspace --locked` before finishing an engine change. Use the Signal Runner and Rail Paddle replay fixtures for gameplay regression checks.

When editing a game, start with `agge query` or `agge_query`. Request a resource ID, JSON Pointer, or script line range only as needed. Use a named session checkpoint when resuming work. Fetch `{"schema":"edits"}` for batch forms and send the last observed revision as `base`. Prefer `grid`, `fill`, `stamp`, and typed `rename` when they express the edit directly.

Verify effects with `agge test` or `agge_run`. Select runtime state or request a small PNG crop only when the test needs it. Keep project media relative to the `.agge` file. Commit the project, assets, and replay specifications with the game.

See [agent commands](docs/agents.md), [the language and project format](docs/reference.md), and [crate responsibilities](docs/architecture.md).
