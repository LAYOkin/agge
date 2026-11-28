# Project and scripting reference

Native artwork uses `palette` resources and `pixel` source blocks. A sprite can reference a PNG sheet or a pixel source. See the [pixel-art reference](pixel-art.md) for the format and drawing commands.

## Project format

An `agge/1` file is UTF-8 text. The first line contains the format version and a JSON object with settings. Each resource has a kind and lowercase ID. JSON resources occupy one line. Script and pixel resources occupy a block terminated by a line containing only three hyphens.

```text
agge/1 {"title":"Demo","start":"main","size":[320,180],"hz":60,"seed":1}
room main {"instances":[["hero","pilot",32,32]]}
object pilot {"size":[16,16],"script":"drive"}
script drive
step:
  move down("right") * 120 * dt, 0
---
```

IDs match `[a-z][a-z0-9_]{0,47}`. Resource IDs are scoped by kind. Room placements have IDs unique within their room. The **Project** dialog edits the title, launch room, dimensions, tick rate, and seed. Custom keyboard actions use a header entry such as `"bindings":{"jump":["Space","J"]}`. Bindings override the defaults for the named action. The default actions are `left`, `right`, `up`, `down`, `fire`, `confirm`, and `cancel`. They map to arrows or WASD, Space or Z, Enter or X, and Escape or Backspace.

In Studio, open **Project → Keyboard bindings**, enter an action ID, and choose **Add override**. Use **Add key** to assign one or more keys, then **Apply settings**. An override with no keys disables that action in native play. Remove a built-in action's override to restore its default keys.

| Kind | Data |
| --- | --- |
| `room` | `bg` RGBA, placement tuples `[id,object,x,y]`, ordered `backdrops` and `maps`, `lights`, `darkness`, `ui` labels, `meters`, optional `music` and `music_gain`, and `y_sort` |
| `object` | `size`, RGBA `color`, `shape`, `sprite` or `animation`, `flip`, `script`, `solid`, `one_way`, `hitbox`, `hidden`, `gravity`, `depth`, collision `layer` and `mask` |
| `sprite` | Project-relative PNG `file` with optional frame rectangles, or a `pixel` resource ID with optional named `poses` |
| `animation` | Sprite ID, zero-based `frames`, `fps`, and optional `mode` (`loop`, `once`, or `ping_pong`) |
| `tilemap` | Pixel `cell` size, optional pixel offset `at`, optional `atlas` sprite, palette `tiles`, and equal-width ASCII `rows`. A dot marks an empty tile |
| `audio` | Project-relative WAV or OGG Vorbis `file` and optional `gain` |
| `particle` | Burst `count`, `life` in ticks, pixel `speed` per second, `radius`, RGBA `color`, optional `gravity`, and optional `fade` for the last N ticks (0 holds opacity) |
| `palette` | Indexed `swatches`, optional light direction, shading ramps, and outline ink |
| `pixel` | Canvas, palette style, layered drawing commands, and named poses |

A label has pixel coordinates `at`, `text`, and optional `size`, `color`, and `value`. `value` names a global number appended to the label. Coordinates start at the top left. Positive Y points down. An object's position and collider use its top-left corner, even for round visuals.

A HUD meter uses `at`, `size:[width,height]`, `value` (a global), `max`, and optional `color` and `track`. It fills left to right with the value clamped between zero and the maximum. A room's `music` names an audio clip and `music_gain` sets its relative volume (default 1). The native player loops it, keeps playing it across rooms that share the same clip ID, and stops it in a room without music.

Draw order uses object `depth`, with smaller numbers drawn first. Room `y_sort:true` also sorts by the bottom of each object's bounds within the same depth, so actors farther down the screen draw in front. Equal positions retain instance order. Draw order leaves physics and scripts in their existing tick order.

### Room size and camera

The project `size` is the logical viewport. A room can set a separate `size` to create a larger world. An explicit room size must contain the viewport and can be up to 524,288 pixels on each axis. With no room size, the world matches the viewport.

```text
room road {"size":[960,192],"camera":{"follow":"runner","dead":[64,0],"smooth":12},"instances":[["runner","courier",34,84]]}
```

The camera's `at` is an initial top-left world coordinate, defaulting to `[0,0]`. `follow` names a placement in the room. The camera tracks the center of its bounds after movement. `dead` is the width and height of an optional centered region the target can move within before the camera follows. `smooth` defaults to 0 for immediate tracking. Positive values move a fraction of the remaining distance each tick, capped at `min(smooth / hz, 1)`. The initial follow adjustment snaps after create events. If the target is destroyed, the camera keeps its last position. A room change resets the camera to the new room's configuration. The viewport stays within the room, and HUD positions stay relative to the viewport.

### Backdrops

Backdrops draw in array order, before tilemaps. Each backdrop names a `sprite`, with optional zero-based `frame`, `at:[x,y]`, draw `size:[width,height]` (default 64×64), `scroll:[x,y]`, `repeat:[x,y]` booleans, and RGBA `tint`. By default it uses the world origin and a scroll factor of 1 on both axes. Set an axis to 0 for a viewport-fixed layer or a fraction for parallax. Repeat fills the visible area along the selected axes.

### Tile palettes

Flat tiles use `[rgba,solid?]`. Atlas tiles use `[frame,solid?,tint?]`, with a zero-based frame in the tilemap's shared sprite. A plain sprite with no frame rectangles has one frame. Atlas tiles default to white tint and non-solid collision. The collision flag fills the whole cell independently of the PNG's transparency. A palette can mix both kinds.

In a script, `tile terrain 4, 2, "."` clears a cell on the current room's `terrain` map. Pass any palette glyph to replace it. The write updates collision and rendering immediately, including later movement in the same event. Coordinates are zero-based integers. A write outside the active map faults at that statement. `tile_is("terrain", 4, 2, "#")` checks the glyph and `tile_solid("terrain", 4, 2)` checks its collision flag. Probes outside the map return 0.

Edits persist when revisiting a room and are included in runtime checkpoints. Rooms using the same map asset have independent edits. Reset a cell by writing its authored glyph. Up to 65,536 changed cells may be active across a world.

```text
sprite terrain {"file":"art/terrain.png","frames":[[0,0,16,16],[16,0,16,16]]}
tilemap floor {"cell":16,"atlas":"terrain","tiles":{"#":[0,true],"+":[1],"w":[[36,70,100,180]]},"rows":["#.+w"]}
```

## Script

A script may define one block each for `create:`, `step:`, `hit_x:`, and `hit_y:`. Create runs once per instance. Step runs once per simulation tick. The hit handlers run when velocity-driven movement is clipped along their axis. Indentation is for readability. A `#` outside a quoted string starts a comment.

```text
create:
  set charges = 3
step:
  if pressed("fire") && charges > 0
    set charges = charges - 1
    spawn spark x + 8, y
    emit flash
    sound ping
  else
    move down("right") * 120 * dt, 0
  end
```

| Statement | Effect |
| --- | --- |
| `set name = expr` | Set an instance variable or one of `x`, `y`, `vx`, and `vy` |
| `global name = expr` | Set a global shared across instances and rooms |
| `move dx, dy` | Sweep a collider by the specified pixel offsets |
| `tile map x, y, "glyph"` | Change one tile cell in the active room |
| `camera x, y` | Set the viewport's top-left world coordinate for the end of this tick |
| `spawn object x, y` | Queue creation of an instance at a pixel position |
| `emit particle` or `emit particle, x, y` | Emit a burst at the instance origin or the specified world position |
| `sound audio` | Queue a clip for native playback |
| `save` | Request a portable progress save at the end of a successful tick |
| `animate clip` or `animate clip, restart` | Switch the instance's clip, restarting when `restart` is nonzero |
| `flip x, y` | Mirror the instance sprite on each axis whose expression is nonzero |
| `tint r, g, b, a` or `tint` | Set instance draw color with channels rounded and clamped to 0–255, or restore the authored color |
| `drop ticks` | Ignore one-way platforms for up to 600 ticks, including this tick. Zero cancels |
| `after name, ticks` | Start or replace a named instance timer. Zero cancels it |
| `goto room` | Queue a room change |
| `destroy` | End the event and remove the current instance |
| `if expr` / `else` / `end` | Run a block when the expression evaluates to a nonzero number |
| `repeat count` / `end` | Repeat a block 0 to 4096 times within the tick instruction budget |

Expressions use finite single-precision numbers, `true` and `false`, parentheses, and instance variables. Operators in precedence order from weakest to strongest are `||`, `&&`, `== !=`, `< <= > >=`, `+ -`, `* / %`, then unary `- !`. Boolean operators short-circuit. Undefined instance variables fault at runtime and unset globals read as zero.

Read-only variables are `dt`, `tick`, `width`, and `height`. Width and height are the room dimensions. Use `view("width")` and `view("height")` for the viewport dimensions. `view("x")` and `view("y")` read the last applied camera position. A `camera` request clamps to the room and overrides follow for that tick. If multiple scripts request a camera position, the last request wins. Requests from the previous room are cleared when a room change takes effect. Functions are `down("action")`, `pressed("action")`, `touch("object")`, `global(name)`, `rand()`, and the numeric functions `abs(x)`, `floor(x)`, `ceil(x)`, `sin(x)`, `cos(x)`, `sqrt(x)`, `min(x,y)`, `max(x,y)`, and `clamp(x,min,max)`. `touch` tests overlap with other instances of the named object. `rand` returns a seeded value in [0, 1).

`released("action")` returns `1` on the first simulation tick after the action stops being held and `0` otherwise. Use it to commit an action after a hold, such as launching a charged shot.

Enable desktop pointer input with `"pointer":true` in the project header or in Studio's Project dialog. `pointer("active")` is 1 inside the game viewport. `pointer("x")` and `pointer("y")` read logical pixel coordinates. Use `world_x` and `world_y` to include the current camera offset. Coordinates return -1 outside the viewport. Button actions are `mouse_left`, `mouse_right`, and `mouse_middle`. Read them with the same `pressed`, `down`, and `released` functions used for keyboard actions.

### Player progress

Declare persistent numeric globals in the project header. The game's stable progress ID chooses the native save directory. The schema version identifies the shape of the saved data.

```text
agge/1 {"progress":{"id":"quarry","version":2,"globals":{"stars":0,"stage":1},"migrations":{"1":{"rename":{"coins":"stars"}}}}}
```

The player loads progress before the launch room's create events. Use those globals to select an unlocked level or restore settings. A script's `save` captures the declared values at the end of a successful tick, including changes from later events. Keep transient state in undeclared globals or instance variables. Exact runtime checkpoints capture the entire live world for the same source revision.

Increase the schema version when changing persistent keys. Each migration key names the version it upgrades by one. `rename` maps previous globals to the next names and `remove` lists retired keys. Renames apply simultaneously. New keys use their declared defaults. The player reports invalid files, incomplete migration chains, and newer versions without replacing the saved data.

### Reuse script expressions

Declare an expression function at the top level of any script. An event can call functions in its own script by name or call another script's functions with `script_id.function(...)`. The module can contain just functions. Declarations can appear before or after events.

```text
# script motion
fn axis(negative, positive, speed) = (positive - negative) * speed * dt
```

```text
# script drive
step:
  move motion.axis(down("left"), down("right"), 120), 0
```

Each function accepts up to eight numeric parameters. Functions read engine variables, global values, input actions, and collision queries. Pass an instance variable through a parameter to use it in a helper. Arguments are evaluated left to right and the helper evaluates in its declaring script. `choose(test, yes, no)` evaluates only the selected branch, so it can guard a division or a recursive call. The runtime applies its expression nesting limit and tick instruction budget inside functions.

At each tick, entities run in increasing ID order. After an entity's step, gravity updates vertical velocity and the runtime sweeps its velocity through solid tiles and instances on each axis. An axis blocked by a solid has its velocity reset. Assigning X or Y directly teleports the collider. Step spawns run after the current entities step. The first requested room transition then takes effect at the end of the tick. Globals and runtime IDs persist across rooms. Particles are cleared on entry to a new room.

`grounded()` returns `1` when the instance is stationary or descending vertically and a quarter-pixel downward sweep is blocked. `blocked(dx,dy)` tests a displacement against the same collision rules without changing position. Set `one_way:true` on objects that should block only downward movement crossing their top. An object can use `solid` or `one_way`. Overlap tests through `touch` apply to both.

`hitbox:[x,y,width,height]` overrides collision and overlap geometry relative to the object's draw origin. The default covers its full `size`. Authored `flip:[x,y]` and the script override affect sprite pixels only. `layer` and `mask` are unsigned 16-bit sets. A moving object stops against a solid or one-way object when its mask intersects the obstacle's layer. Tiles use layer value `1`. By default objects are on layer `1` and check all layers. A mask of `0` ignores every blocker. Overlap queries through `touch` are independent of movement masks.

`touch("object", dx, dy)` tests the actor's bounds at an offset in pixels without moving it. It checks overlap at that location and does not sweep the path. Omit both offsets to test the current position. Only other live instances of the named object in the current room can match.

### Animation and cooldowns

`animate` preserves elapsed time when the requested clip is already active. Pass `true` to restart it. `once` plays each frame for its normal duration and holds the last one. `anim_done()` returns `1` after the full duration of a one-shot clip. `ping_pong` alternates direction without duplicating the end frames. Clip timing advances on simulation ticks.

Use sparse `holds` to pause selected clip slots. In `{"sprite":"ship","frames":[0,1,2],"fps":12,"holds":{"1":4}}`, the middle slot uses four frame units at 12 fps. Other slots use one unit. Keys are zero-based positions in `frames`. Holds affect the reverse pass in ping-pong mode and extend one-shot completion.

`after shot, 16` sets a timer for 16 ticks after the current tick. `remaining(shot)` returns `0` when idle or due. `expired(shot)` returns `1` only on the due tick, unless the script has already changed or canceled it. An instance can hold 16 timers. Timers follow the instance's lifetime and use whole-tick durations up to 60,000.

```text
step:
  if down("fire") && remaining(shot) == 0
    spawn pulse x + 16, y + 8
    animate attack, true
    after shot, 16
  end
```

The runtime resolves both velocity sweeps before running `hit_x` and then `hit_y`. `impact()` returns the signed pre-clamp velocity for the current hit event and `0` in other events. Event code can change velocity for the next tick. Gravity can cause a vertical hit on successive ticks when an instance rests on a floor. Explicit `move` uses collision sweeps without emitting hit events. Bank Shot uses this pattern to rebound and keep its full horizontal speed.

```text
hit_x:
  set vx = -impact()
```


