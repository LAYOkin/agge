# Agents and automation

AGGE runs locally over stdio with JSON-RPC. Register the release `agge` executable with arguments `mcp` and the absolute path to a project. The project must already exist. Each server is bound to that project, including when its client runs from another directory.

A typical MCP client configuration uses this command and argument pair. Use the executable path and project path for your machine.

```json
{
  "mcpServers": {
    "agge": {
      "command": "/absolute/path/to/agge",
      "args": ["mcp", "/absolute/path/to/game.agge"]
    }
  }
}
```

## Small reads and edits

| MCP tool | Equivalent CLI | Inputs |
| --- | --- | --- |
| `agge_query` | `agge query PROJECT JSON` | `prefix`, `get`, `outline`, `refs`, `session`, or `schema` |
| `agge_edit` | `agge edit PROJECT JSON` | `{base?,ops}` |
| `agge_check` | `agge check PROJECT` | Empty object |
| `agge_run` | `agge test`, `inspect`, or `capture` | `{ticks,room?,input?,expect?,select?,crop?}` |
| `agge_history` | `agge undo` or `agge redo` | `{redo,base?}` |
| `agge_knowledge` | `agge knowledge PROJECT JSON` | `{op,key?,value?}` |

A CLI JSON argument can be inline, `@file.json`, or `-` for stdin. Exit codes are 0 for success, 1 for a failed validation or replay assertion, and 2 for a usage or operation error. MCP returns compact JSON in a text item and includes PNG data as a separate image item when requested.

Example queries include `{"prefix":"room/","limit":8}`, `{"refs":"object/pilot"}`, `{"get":"room/main","path":"/instances/0"}`, and `{"get":"script/drive","lines":[1,12]}`. Reference queries show both directions and the field or script line. List, outline, and reference queries accept `offset` and `limit` and return `next` when there are more results.

For scripts, `{"outline":"script/drive"}` returns event ranges as `[first_line,line_count]` and lists the instance variables, globals, and input actions used. Add `"symbol":"var/speed"` or another listed symbol to locate its reads and writes as `[line,access]` pairs. Use `{"get":"script/drive","event":"step"}` for the exact event text. Line numbers start at 1 inside the script.

Function definitions appear in the outline as `fn/name`. Use `{"get":"script/motion","function":"axis"}` for one declaration or `{"outline":"script/motion","symbol":"fn/axis"}` for its signature and local uses. Use `{"refs":"script/motion/fn/axis"}` to find call sites in other scripts. Rename the script with the typed `rename` operation to update qualified calls.

Use `{"schema":"edits"}` for valid forms of `put`, `patch`, `remove`, `rename`, `settings`, `script_event`, `splice`, `place`, `grid`, `camera`, `slice`, `atlas`, `fill`, and `stamp`. Query any resource kind or `project` or `replay` for its shape. `put` replaces a whole resource. `patch` changes top-level JSON fields, with `null` removing a field. `rename` updates typed references across rooms, objects, animations, project settings, and compiled script references. `fill` and `stamp` use tile coordinates, while `place` and `grid` use pixels.

Use `{"op":"script_event","id":"drive","event":"step","body":"  move 120 * dt, 0"}` to replace an event body or add an absent event to an existing script. An empty string keeps the header and clears the executable body. Explicit `null` removes the event. The body must omit event headers. The edit preserves the existing header and trailing comments outside the executable block.

Use `{"op":"splice","id":"drive","at":[6,1],"text":"  move 180 * dt, 0"}` for a line-range edit. The first number is the 1-based script line and the second is how many lines to remove. A count of zero inserts before that line. An empty `text` deletes the range. Batch operations see the line numbering after earlier operations in the batch.

A regular sprite sheet and tile palette can be configured in one batch.

```json
{"ops":[
  {"op":"slice","id":"terrain","cell":[16,16],"grid":[4,2]},
  {"op":"atlas","map":"layout","sprite":"terrain","keys":"abcdefgh","solid":"efgh"},
  {"op":"stamp","map":"layout","at":[0,0],"rows":["abcdefgh"]}
]}
```

`slice` replaces the frame list in row-major order. Optional `at` and `gap` values are in PNG pixels. `atlas` maps each key to a consecutive frame starting at `first` (default 0), with `solid` listing which keys block movement. The operation updates those entries and preserves the other keys. Include every existing textured key when changing an assigned atlas. Check media validation after slicing to verify the frame rectangles fit the actual PNG.

Set a room's bounds and camera in one operation, for example
`{"op":"camera","room":"road","size":[960,192],"follow":"runner","dead":[64,0],"smooth":12}`.
The operation replaces the camera configuration. Omit `size` to keep the existing world dimensions. Use a room patch with `"size":null` to restore viewport-sized bounds. The follow target is a room placement ID. `{"refs":"room/road/instances/runner"}` traces the camera's dependency on it.

Use the query revision as `base` in edits and history operations when working alongside a person or another process. A batch validates against the final project and commits as a single history entry. Results list changed and removed IDs. A failed validation or stale revision leaves the source intact.

Use `{"session":"builder","advance":true}` to save the current resource manifest for a session. On a later connection, `{"session":"builder"}` returns changed and removed resource IDs relative to that checkpoint. Set `advance` again after processing the difference. Checkpoints follow source resources plus the bytes of referenced PNG and audio files. A media-only edit changes an `asset/path` entry while the source revision stays the same. Files that disappear or become unreadable also register a change while referenced. Query `{"refs":"asset/art/ship.png"}` to find the resources that use a file.

Persist small structured project facts with `{"op":"put","key":"controls","value":{"fire":"Space"}}`. Read with `get` or remove with `remove`. Knowledge and checkpoints live beside the project in its local `.knowledge` sidecar.

## Verify behavior

Input spans are inclusive, start at tick 1, and use `[first,last,action]`. Assertions may sample tick 0 after create events. Fields are `room`, `particles`, `camera/x`, `camera/y`, `count/object_id`, `global/name`, and `entity/placement_id/property`. Properties include position, velocity, object ID, and the script's instance variables.

```json
{
  "ticks": 12,
  "input": [[1, 8, "right"]],
  "expect": [{"at": 12, "field": "global/score", "is": 1}],
  "select": {"placement": "hero"}
}
```

Send `select` only when you need runtime state. Selected state includes the camera origin when it differs from `[0,0]`. Use `crop:[x,y,width,height]` in viewport coordinates when an image is needed. The renderer applies the current camera to world layers and lighting. HUD coordinates remain fixed on screen. MCP caps a crop at 512 pixels per edge. The CLI capture command takes an output path and caps a crop at 2048 pixels per edge. A fixed tick count, a fixed seed, and explicit inputs make replay the preferred entry point for automated tests.



### Inspect runtime faults

A replay fault identifies the tick, entity, event, and message. Script faults add the object, placement when available, and `source` with the script ID and statement line. If the fault occurred in a function, the last `stack` frame identifies the innermost declaration. Reports keep up to 16 innermost frames and include `omitted_frames` when truncated. Startup faults are reported at tick 0 and selectable runtime state includes the failing entity. A fault stops the simulation, so that state can include changes from a partially executed tick.
