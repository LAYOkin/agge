# Pulse Lock

Tap **Space** or **J** as the cursor crosses the lit gate from left to right. Lock all three in order. Early or late taps reset your locks. If the cursor passes the gate, wait for another pass. Press **Enter** or **X** on the completion screen to play again.

```sh
agge-player examples/pulse_lock/game.agge
agge test examples/pulse_lock/game.agge @examples/pulse_lock/replay.json
```

The project uses the custom `lock` action, `pressed()` for one tap per keypress, a HUD meter driven by a global, and a particle burst for each successful lock. The replay suite covers a clean run, holding the key through the second gate, a missed tap, and restarting.
