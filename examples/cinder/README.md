# Cinder Circuit

Restore the three relays while avoiding the sentry. The pilot is stopped by the blue force field, while your pulses pass through. Align with each relay and fire, then fly to the exit on the far side of the yard.

| Control | Action |
| --- | --- |
| Enter | Launch or retry |
| Arrows or WASD | Steer |
| Hold Space | Fire pulses as the charge meter refills |
| R | Restart the yard |

```sh
agge-studio examples/cinder/game.agge
agge test examples/cinder/game.agge '@examples/cinder/replay.json'
```

The suite checks the win, loss, cooldown, and force field scenarios with 35 assertions. The sprite sheet, music, and relay sound are included under the repository's MIT license.
