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
| `agge_query` | `agge query PROJECT JSON` | `prefix`, `get`, `asset`, `outline`, `refs`, `session`, `read`, or `schema` |
| `agge_edit` | `agge edit PROJECT JSON` | `{base?,ops,preview?,verify?}` |
| `agge_check` | `agge check PROJECT` | Empty object |
| `agge_run` | `agge test`, `inspect`, or `capture` | `{ticks,room?,seed?,input?,expect?,sample?,select?,crop?,base?}` |
| `agge_history` | `agge undo` or `agge redo` | `{redo,base?}` |
| `agge_knowledge` | `agge knowledge PROJECT JSON` | `{op,key?,value?}` |

A CLI JSON argument can be inline, `@file.json`, or `-` for stdin. Exit codes are 0 for success, 1 for a failed validation or replay assertion, and 2 for a usage or operation error. MCP returns compact JSON in a text item and includes PNG data as a separate image item when requested.

Example queries include `{"prefix":"room/","limit":8}`, `{"refs":"object/pilot"}`, `{"get":"room/main","path":"/instances/0"}`, and `{"get":"script/drive","lines":[1,12]}`. Reference queries show both directions and the field or script line. List, outline, and reference queries accept `offset` and `limit` and return `next` when there are more results.

Use `{"get":"room/main/instances/hero"}` to retrieve a placement by ID without reading its room. Group related lookups with `{"read":[{"get":"room/main/instances/hero"},{"refs":"object/pilot"}]}`. The response contains one `revision` and ordered `data` entries. A failed lookup occupies its position as `{ "error": "..." }`. Set `base` on the outer request to reject a stale snapshot. A batch accepts 16 queries and up to 2 MiB of output. Schema, session, and asset queries run separately.

Use `{"asset":"art/ship.png"}` for dimensions and byte size before slicing, or pass a relative WAV or OGG path for sample rate, channels, and frame count when the container provides it. The query can inspect a file before you add its resource. It returns metadata without binary contents.

For scripts, `{"outline":"script/drive"}` returns event ranges as `[first_line,line_count]` and lists the instance variables, globals, and input actions used. Add `"symbol":"var/speed"` or another listed symbol to locate its reads and writes as `[line,access]` pairs. Use `{"get":"script/drive","event":"step"}` for the exact event text. Line numbers start at 1 inside the script.

Function definitions appear in the outline as `fn/name`. Use `{"get":"script/motion","function":"axis"}` for one declaration or `{"outline":"script/motion","symbol":"fn/axis"}` for its signature and local uses. Use `{"refs":"script/motion/fn/axis"}` to find call sites in other scripts. Rename the script with the typed `rename` operation to update qualified calls.

Use `{"schema":"edits"}` for the edit catalog or `{"schema":"edit/grid"}` for one form. Query any resource kind or `project` or `replay` for its shape. `put` replaces a whole resource. `patch` changes top-level JSON fields, with `null` removing a field. `rename` updates typed references across rooms, objects, animations, project settings, and compiled script references. `fill` and `stamp` use tile coordinates, while `place` and `grid` use pixels.

`{"op":"placement","room":"main","id":"hero","to":"pilot","at":[48,32]}` renames and moves one placement and preserves a camera reference to it. Use `object` to retarget the placement or `remove:true` alone to delete it and clear its camera reference. Update any external replay selectors that use a renamed placement ID.

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

Use `{"op":"flood","map":"layout","at":[2,3],"tile":"#"}` to replace the edge-connected region of matching cells at column 2, row 3. Coordinates are zero-based. Use a defined palette tile or `"."` to erase the region.

After slicing a sheet, use `{"op":"clip","id":"pulse","sprite":"ship","first":4,"count":6,"fps":16,"mode":"once"}` to create or replace an animation. `first` defaults to 0 and `mode` to `loop`. Use a normal animation resource with an explicit frame list for custom frame sequences.

Set a room's bounds and camera in one operation, for example
`{"op":"camera","room":"road","size":[960,192],"follow":"runner","dead":[64,0],"smooth":12}`.
The operation replaces the camera configuration. Omit `size` to keep the existing world dimensions. Use a room patch with `"size":null` to restore viewport-sized bounds. The follow target is a room placement ID. `{"refs":"room/road/instances/runner"}` traces the camera's dependency on it.

Use the query revision as `base` in edits and history operations when working alongside a person or another process. A batch validates against the final project and commits as a single history entry. Results list changed and removed IDs. A failed validation or stale revision leaves the source intact.

Add `preview:true` to validate and return a proposed receipt without writing project source or history. Add `verify:[...]` to run up to four assertion-bearing replays on the candidate before committing, with at most 6,000 total ticks. The first failed replay stops verification and returns its report. With either option, `committed` records whether a change was written. If it is false, `revision` identifies the source read by the operation, and `changed` and `removed` describe the candidate. A failed verification uses exit code 1 or MCP `isError`.

Use `{"session":"builder","advance":true}` to save the current resource manifest for a session. On a later connection, `{"session":"builder"}` returns changed and removed resource IDs relative to that checkpoint. Set `advance` again after processing the difference. Checkpoints follow source resources plus the bytes of referenced PNG and audio files. A media-only edit changes an `asset/path` entry while the source revision stays the same. Files that disappear or become unreadable also register a change while referenced. Query `{"refs":"asset/art/ship.png"}` to find the resources that use a file.

Persist small structured project facts with `{"op":"put","key":"controls","value":{"fire":"Space"}}`. Read with `get` or remove with `remove`. Knowledge and checkpoints live beside the project in its local `.knowledge` sidecar.

## Verify behavior

Input spans are inclusive, start at tick 1, and use `[first,last,action]`. Assertions and samples may read tick 0 after create events. Fields are `room`, `particles`, `camera/x`, `camera/y`, `grounded/placement_id`, `count/object_id`, `global/name`, and `entity/placement_id/property`. `grounded` returns `1` or `0`. Entity properties include position, velocity, object ID, and instance variables.

Use `animation/placement` for the current clip ID and `frame/placement` for its sheet frame. `music` returns the current room's configured audio ID if present.

Use `timer/placement/name` for remaining ticks and `expired/placement/name` to check when a timer is due. `expired` returns `1` only on the due tick and `0` otherwise. Replays read after the tick finishes, so a timer that a script canceled or rescheduled during that tick returns `0`.

Use `sound/audio_id` to count how many times a sound effect was queued on a tick. For example, `{"at":12,"field":"sound/ping","is":1}` checks for one cue. A known clip that was silent returns `0`. An unknown clip ID returns no value. Tick 0 includes sounds from startup create events, and the check works without playing audio.

```json
{
  "ticks": 12,
  "input": [[1, 8, "right"]],
  "expect": [{"at": 12, "field": "global/score", "is": 1}],
  "select": {"placement": "hero"}
}
```

Send `select` only when you need runtime state. Selected state includes the camera origin when it differs from `[0,0]`. Use `crop:[x,y,width,height]` in viewport coordinates when an image is needed. The renderer applies the current camera to world layers and lighting. HUD coordinates remain fixed on screen. MCP caps a crop at 512 pixels per edge. The CLI capture command takes an output path and caps a crop at 2048 pixels per edge. A fixed tick count, a fixed seed, and explicit inputs make replay the preferred entry point for automated tests.

For smaller responses, use `sample:[[0,"entity/hero/x"],[12,"global/score"]]`. To read the same fields at several ticks, use `sample:{"at":[0,6,12],"fields":["entity/hero/x","entity/hero/y"]}`. Reports return just the values in request order, with fields grouped by tick in the grid form. A missing field value or a tick skipped after a fault returns `null`. Samples are observations. Add assertions to make a result fail when a value is wrong.

Set `base` to the revision returned by a query to pin a single replay or a suite to that project source. If the project has changed, AGGE rejects the request and reports the current revision.

Set `seed` on a replay to override the project's random seed from tick 0. Use an unsigned 64-bit integer. Omit it to use the project seed. In a suite, set it inside each case whose random sequence you want to control.

Pass `{"cases":{"startup":{"ticks":0,"expect":[{"at":0,"field":"room","is":"main"}]}}}` to `agge test` or `agge_run` for a suite. Each case starts a fresh world from the same compiled revision. Set `detail:true` to include successful reports and samples. By default the response summarizes passing cases and includes reports only for failures. Each report keeps the first eight failed assertions and counts the rest. Suites accept up to 32 cases and 120,000 total ticks. Selective state and PNG crops are available on a single replay.



### Inspect runtime faults

A replay fault identifies the tick, entity, event, and message. Script faults add the object, placement when available, and `source` with the script ID and statement line. If the fault occurred in a function, the last `stack` frame identifies the innermost declaration. Reports keep up to 16 innermost frames and include `omitted_frames` when truncated. Startup faults are reported at tick 0 and selectable runtime state includes the failing entity. A fault stops the simulation, so that state can include changes from a partially executed tick.
