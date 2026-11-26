# Native pixel art

## Start with the smallest useful read

1. Query `{"schema":"pixel"}` for drawing forms and `{"schema":"palette"}` for a style profile. Query `{"prefix":"pixel/"}` or `{"pixel":"diver"}` for a revision, canvas size, pose names, and layers. `get: "pixel/diver"` accepts `lines: [first,count]`.
2. Create a `palette` and `pixel` with normal `put` operations. Store the drawing as the `data` string for `pixel`. Use `style` to share colors, shading ramps, and light direction across related assets.
3. Use plain shapes for a silhouette, then place exact pixel clusters with `rows`, `line`, or `px`. Build repeated parts once. Keep silhouettes, material clusters, moving pieces, and effects on layers you can edit independently.
4. Inspect a region as glyphs or PNG. Send the last observed revision as `base` in a patch. Attach the result to a sprite, animation, or tilemap and run a replay.

Add `"compare":true` to an `agge_art` preview with `ops` and one pose. It reports the changed RGBA pixel count and returns a PNG with before, after, and a magenta change mask. Omit `area` to crop to the changed pixels automatically. Pass a layer to see changes hidden under the composite. A visible no-op returns `changed:0` without a PNG. The source resource must already exist and retain its canvas size.

## Style and source

```text
palette sea {"swatches":{"k":"182536","a":"2b4c61","b":"4e9d9d","c":"98d8b7","w":"ffedbd"},"ramps":["abc"],"ink":"k"}
pixel signal
canvas 16 16
style sea
layer body
  oval b 3 3 10 10
  shade 2 2 12 12
  outline 1 1 14 14
layer glass
  rows 5 5 cw.2c/.wwc
frame idle
frame blink idle
layer glass
  clear 5 5 5 2
  line c 6 6 9 6
tween bob idle 4 0 -2 glass
---
sprite signal {"pixel":"signal","poses":["idle","blink","bob_2","bob_3"]}
animation pulse {"sprite":"signal","frames":[0,1,2,3],"fps":8,"mode":"ping_pong"}
```

Use a canvas of 1–256 pixels per edge, with up to 16 layers in a pose and 64 exported poses. Coordinates start at the top left. Colors are RRGGBB or RRGGBBAA. A swatch key is one ASCII letter or one of `@+$%&?=!*^~`. Alpha is retained in the resulting sprite. At each pixel the top visible layer selects the swatch. Transparent pixels expose layers below. A ramp lists distinct swatches from dark to light. `light:[-1,-1]` points towards the top left.

The base pose contains all layers before the first `frame`, `tween`, or `motion`. `base OTHER` before the layers inherits another pixel resource's base layers at the same canvas size. An alternate style can recolor them by providing the same glyphs. A frame defaults to inheriting the base pose, or it can name an earlier pose. Editing an inherited layer retains its stack position. New layers go on top.

A `tween PREFIX FROM COUNT DX DY [LAYER]` generates `PREFIX_0` through `PREFIX_(COUNT-1)` with integer offsets between the endpoints. It can move a single layer or the entire pose. For multiple moving parts, use `motion hover idle head=0,0/0,-1/0,0 tool=0,0/1,0/2,0`. Each track needs the same number of offsets, and each offset is relative to the source pose. Repeat offsets for holds. A later frame can inherit any generated pose. Export order follows source order unless the sprite specifies `poses`.

## Drawing forms

| Form | Effect on the current layer |
| --- | --- |
| `px C X Y`, `line C X Y X2 Y2` | One pixel or a rasterized line |
| `box C X Y W H`, `rect C X Y W H`, `oval C X Y W H` | Filled rectangle, hollow border, or filled ellipse |
| `poly C X,Y X,Y ...` | Filled contour with up to 32 vertices |
| `rows X Y 3a..b/.b2c` | Slash-separated rows. An optional count repeats the next glyph. `.` skips, and `-` erases. |
| `dither A B X Y W H`, `recolor A B X Y W H` | Alternate glyphs or replace a particular glyph in a region |
| `flood C X Y`, `clear X Y W H` | Flood a matching edge-connected region or clear a rectangle |
| `copy X Y W H DX DY [TRANSFORM]` | Copy a region to a destination. Transparent source pixels skip. |
| `mirror x X Y W H` or `mirror y X Y W H` | Reflect a region into adjacent pixels on the right or below |
| `use PART[:POSE] X Y [TRANSFORM]` | Stamp a component in the destination style. Omit the pose for the first exported pose. |
| `shift DX DY` | Move the current layer and clip against the canvas |
| `shade X Y W H` | Move boundary pixels one stop on their swatch ramp using the style light direction |
| `outline X Y W H [C]`, `outline8 X Y W H [C]` | Add a one-pixel edge on empty pixels using four or eight neighbors. Omit ink to use the profile. |

Transforms are `mx`, `my`, `r90`, `r180`, and `r270`. Positive rotations are clockwise. Geometry and stamp destinations clip against the canvas. Copy sources must fit. `#` starts a comment. The project validator reports bad glyphs, unknown parts and poses, cyclic references, and commands that exceed the bounded rendering budget.

## Iterate on an exact region

```json
{"base":"REVISION","ops":[{"op":"pixel_patch","id":"diver","frame":"idle","layer":"head","draw":"px w 23 9"}]}
```

Omit `frame` to patch the base. A patch appends local commands. Set `replace:true` to replace just that frame's authored block for the layer. Edit the generator with `{"op":"splice","kind":"pixel","id":"diver","at":[12,1],"text":"tween recoil idle 4 0 -3 tool"}` or inherit a generated pose in a new named frame. Patches participate in normal undo, redo, typed rename, and `verify` transactions. `refs:"pixel/diver"` shows sprites, inherited variants, and components that refer to it.

```json
{"pixel":"diver","frame":"idle","layer":"head","area":[19,7,7,5]}
```

Pass this request to `agge query` or `agge_query` for exact glyph rows. Omitting the layer composites the pose. The region is `[x,y,width,height]`, with a maximum of 4,096 returned glyphs.

```json
{"pixel":"diver","frames":["idle","weld_1","weld_2"],"area":[17,3,22,22],"scale":4,"columns":3}
```

Pass this request to `agge art PROJECT JSON OUTPUT.png` or `agge_art` for a contact sheet. `frames` can be a name, an ordered array, or `"*"` for all exported poses. A single pose is the default. Use `base` to pin the source. Optionally include up to 16 `ops` to render a candidate revision without saving it. Output stays within 512 pixels per edge.

## Studio

Create and apply a **Palette**, then create **Pixel artwork**. Choose a pose, a layer, and a brush. Click or drag to paint, right-click to sample a swatch, or choose **Erase** to clear pixels. Enable **Isolate layer** to see edits underneath other layers. Use **Source** for drawing commands, inherited parts, and layers. **Play poses** loops the artwork at preview speed. The existing Animation inspector uses the clip's authored frame rate.

For an end-to-end example, open [Brassline Foundry](../examples/pixel_foundry). Its artwork, atlas, and animations ship in the project file.
