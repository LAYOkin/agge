# Development status and release criteria

AGGE is pre-alpha. The workspace uses `0.0.0-prealpha` for development builds. Project formats, APIs, and save compatibility may change during development. The source includes Studio, the standalone desktop player, the CLI and MCP server, nine example projects, and a portable archive workflow.

## Criteria for the first public release

The first v0.1 release requires demonstrated readiness for professional and commercial game development. Passing CI and running the examples establish a development baseline. Release evidence must also cover building, shipping, and maintaining complete games.

| Area | Required evidence |
| --- | --- |
| Game production | Validate complete games through authoring, playtesting, export, and a shipped update that preserves player progress. Document the supported workflows and platform requirements. |
| Source | Run locked workspace tests and Clippy on all three CI systems. Record the result for the candidate commit. |
| Portable builds | Stage the three binaries together, check all bundled projects, replay their fixtures, and export and retest Cinder Circuit from the staged installation. Verify archive hashes and executable permissions after extraction. |
| Native QA | From an extracted archive, open Studio and start and export a game. Exercise keyboard and pointer input, resizing and focus changes, sound, save and restore, and an external file reload. Run on representative graphics drivers and on at least one clean system per claimed platform. |
| Installation | Confirm minimum OS and CPU targets, graphics requirements, and package instructions on those systems. Current archives use each CI runner's architecture. Establish signing and notarization expectations before distributing publicly. |
| Agent workflow | Connect the candidate build's MCP server in a fresh project, make a guarded edit, render a crop, and check a replay. Run the published benchmark harness and collect matched model-session usage before making a general token-saving claim. |

The portable workflow uploads development artifacts. Native graphics and audio need desktop QA in addition to the headless replay tests. Versioned release downloads and signing remain release tasks.

## Engine boundaries

Scripts provide AABB movement and platform collision, authored tilemaps, room changes, and lightweight HUDs. Mutable runtime tilemaps, controller input and rebinding in shipped games, a general UI layout and text-input system, arbitrary user shaders, and physics joints are open engine work. Web and mobile builds are outside the desktop target.

Checkpoints restore deterministic runtime state for the same canonical source. Save migrations, user-managed save slots, and serialization of native audio playback position remain open. Live reload supports compatible edits and runtime rebinds. Changes to the current room's authored placements, tick rate, or viewport size require a restart.

Native pixel art includes layered poses, shared palettes, components, symmetry, regional edits, generated tracks, and timed clip playback. In-game examples and exact captures demonstrate these tools. A blind artistic-quality comparison across several asset briefs is still outstanding.
