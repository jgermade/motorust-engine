# Motorust Engine

The engine of Motorust, an arcade motorbike racing game made with [Bevy](https://bevy.org) 0.19: everything
that makes a motorbike racing game, knowing nothing of the content it runs.

- **The bikes**: arcade physics in a fixed step (120 Hz) with interpolated
  rendering, crashes, wheelies, loops, rivals and their racing lines, riders
  posed every frame; four bikes built in code with painted liveries, and
  models a game brings (`model.ron` in a folder of its pack).
- **The circuits**: written in RON (`assets/circuits/`), their road (control
  points, heights, banking, branches, forks, loops) and their scenery
  (terrain shaped by mountains, coasts, lakes and rivers; walls, tunnels,
  bridges, dams, the sea, the sky, trees, grass, flowers, petals, houses,
  landmarks, portals between realms, excavations), read again when a file
  changes in a development build.
- **The race**: modes (a race, a championship, against the clock,
  elimination, free ride), the HUD and minimap, the menus, options and
  settings, local split screen, gamepads, sound synthesised in real time and
  music a game brings.
- **What a game adds**: its content pack laid over the engine's
  (`content::register_pack`), its identity (`game.ron`), its themes, scenery
  of its own read from the circuits' files by name (`world::themes`), its
  terrain functions, and its plugins.

## Its content

`assets/` is the engine's own content pack (`assets/pack.txt`): a demo
circuit (Bahía GP, with no song), a demo roster (the bikes built in code),
its identity, the textures (Poly Haven, CC0) and the maps of the Moon, the
Earth and the Milky Way (NASA, public domain), the UI font (Russo One, SIL
OFL 1.1) and the gamepads' art (MIT or Apache-2.0). See
[assets/CREDITS.md](assets/CREDITS.md).

## Using it

A game depends on the crate (`motorust_engine`), registers its pack, themes
and terrain functions, and builds the engine's app:

```rust
fn main() {
    motorust_engine::content::register_pack(MY_PACK);
    motorust_engine::world::themes::register::<MyTheme>("my_theme");
    let mut app = motorust_engine::app();
    app.add_plugins(MyGamePlugin);
    app.run();
}
```

`cargo test` runs the engine's tests on its own content. Its checks of
circuits and bikes (each `NAME_on` in a module's tests) are there for a
game's tests to run on its own content, with the `testing` feature.

## Tools

| Tool | What it does |
|---|---|
| `tools/circuit_layout.py` | Works a circuit's lap out from its file (`assets/circuits/NAME.ron`) as `src/track.rs` does (the centre line and the height profile), draws it from above and its profile, and reports its length, its tightest bend, its steepest grade and the crests the bike takes off over, against the limits the tests set. Needs numpy (and matplotlib for the pictures). |

## Licence

The engine's code is dual-licensed under either of

- the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), or
- the MIT licence ([LICENSE-MIT](LICENSE-MIT)),

at your option. Its content keeps the licences its makers gave it (see
[assets/CREDITS.md](assets/CREDITS.md)).

Unless you explicitly state otherwise, any contribution you intentionally
submit for inclusion in the work, as defined in the Apache-2.0 licence,
shall be dual-licensed as above, without any additional terms or
conditions.
