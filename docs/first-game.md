# Build your first room

1. Start `agge-studio` with a path to an existing project, or launch it with no arguments. Enter a location such as `my_game/game.agge` and select **Create project**.
2. Select **+ New**, choose **Script**, and name it `drive`. Enter the following and select **Apply**.

   ```text
   step:
     move (down("right") - down("left")) * 140 * dt, (down("down") - down("up")) * 140 * dt
   ```

3. Create an **Object** named `pilot`. Set its size and tint, and choose `drive` in the Script field. Apply the change.
4. Select the `main` room. Choose `pilot` from the Tool menu, then click the canvas. Switch the tool back to **Select / move** to drag the placement. The inspector exposes the placement ID, coordinates, and object.
5. Apply and press **F5** to play the selected room. Play mode supports pausing, stepping one tick, and inspecting entities and globals. If a script fails, **Open fault source** jumps to the innermost function or failing statement. The script inspector’s **Outline** links to saved events and functions. For a launch-room playtest, select another asset before pressing **Play**.

Put PNG and WAV or OGG assets in folders beside the project. Create a **Sprite** or **Audio** resource with a relative path such as `art/pilot.png`. For sprite sheets, add one [x, y, width, height] rectangle per frame, then make an **Animation** that lists the frame indices in order. Link the object to the animation or directly to the sprite.

If you close Studio with unapplied asset edits or project settings, choose **Keep editing** to return to your changes or **Discard and close** to exit.

The animation inspector supports loop, one-shot, and ping-pong playback. Use the frame slider to inspect individual frames. In the room inspector, **Music** selects a looping audio resource, and **HUD meters** binds bars to globals such as health or charge. **Sort actors by feet** enables top-down draw ordering within each object's draw depth.

Expand **References** in the inspector to navigate between related resources. Incoming script references can jump to the call site, and room references can select a linked placement.

In a room's **Placements** list, select a placement and choose **Duplicate placement** to copy its object and position under a new ID. The copy becomes the selected placement, ready to move or edit.

Click a placement on the canvas, then press the arrow keys to move it by the current **Snap** distance. Choose **Apply** to save the move.

With the tilemap editor, set the grid dimensions and cell size, create or select a palette character, and paint inside the canvas. Enable **Solid** for tiles that should block movement. Add the tilemap under the room's **Tilemap layers**. Use the room's lights, ambient darkness, and HUD label controls to finish the room.

Use the mouse wheel to zoom at the pointer and drag with the middle mouse button to pan in the room and tilemap canvases. **Fit** restores the full view. In the tilemap, right-click any tile to pick it up as the brush, or right-click an empty cell to select the eraser. Hold the primary button to paint a continuous stroke, then choose **Apply** to save it as one undoable change.

Select **Fill** and click a cell to replace its connected region with the current brush. Fill reaches matching cells that share an edge. Switch back to **Paint** for individual tiles and strokes.

Select **Rectangle** and drag between opposite corners to paint a rectangular area with the current brush. Release the button to add the area to the draft.

To delete a palette entry, select it and choose **Remove selected tile**. Erase or replace any cells that use it first.

For regularly spaced sprites, open **Slice grid**, set frame dimensions and the number of columns and rows, and generate the frames. Add an origin or spacing for sheets with margins or gutters. Select a rectangle on the full sheet to edit that frame or check it in **Frame preview**.

To paint textured tiles, select the sheet under **Atlas** in the tilemap inspector. **Map sprite frames** assigns consecutive frames to the keys in the order you enter them. Supply the subset that should block movement under **Solid keys**. You can also select one palette key to change its frame, tint, or collision flag. Signal Runner's station uses a solid wall tile and a non-solid guide light from the same PNG.

### Build a scrolling room

Set the logical viewport in **Project**. In the room inspector, open **Camera and room size**, enable **Custom room size**, and enter dimensions at least as large as that viewport. Pick the player placement in **Follow**. Adjust **Dead zone** and **Smoothing** to control when the camera moves and how quickly it catches up.

The room editor outlines the initial viewport based on the placement and camera settings. Play applies any camera or position changes made by create scripts. Relay Courier is a working example in `examples/relay/game.agge`. Its viewport is 320×192 and its road is 960×192.

## Prove movement with a replay

Set the placement ID to `hero` and its X coordinate to `32`. Save this as `movement.json`.

```json
{"ticks":6,"input":[[1,6,"right"]],"expect":[{"at":6,"field":"entity/hero/x","is":46,"tolerance":0.001}]}
```

```sh
agge check my_game/game.agge
agge test my_game/game.agge @movement.json
```

That test uses the default rate of 60 ticks per second. It checks the same movement script the player runs.

To capture a playtest in Studio, press **Play**, perform the sequence, and select **Copy replay** while the playtest is still open. You can pause before copying to hold the end tick. Paste the JSON into `movement.json` and add an `expect` array for the outcomes you want to preserve. The recording stores action spans and the starting room. It includes manual steps and stops recording at the replay size limits.

Choose **Restart** during a playtest to return to its starting room at tick 0 and begin a new recording. A paused playtest stays paused. Copy the current replay first if you want to keep it.

For examples of collision gameplay, open `examples/switchback/game.agge` for grounded jumps and one-way platforms, or `examples/bankshot/game.agge` for a short arcade game built around `hit_x` and `hit_y`.

Cinder Circuit demonstrates collision filters and scripted animations. Its `replay.json` defines four named scenarios in one suite, including a projectile crossing a force field that blocks the pilot.

## Package the game

Place `agge` and `agge-player` from the same release build in the same directory. Then run the export command with a new destination.

```sh
agge export my_game/game.agge dist
```

AGGE bundles the project, referenced media, and a player named `play.exe` on Windows or `play` on Linux and macOS. The bundled `game.agge` keeps the source revision, so you can run revision-pinned replays against either copy. Launch the player directly from the export directory. To package for another desktop target, build `agge-player` for that target and pass its path as the last argument to `agge export`. Native target libraries and, where applicable, application signing or installers are separate from AGGE's directory exporter.
