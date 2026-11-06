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
  landmarks, portals between realms, excavations) and how their ground is
  drawn (smooth, or in blocks as voxels: `look: Voxel(block: 1.75)`), read
  again when a file changes in a development build.
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

`cargo run --release` runs the engine on its own content (`src/main.rs`:
its demo circuit and roster). `cargo test` runs the engine's tests on its
own content. Its checks of
circuits and bikes (each `NAME_on` in a module's tests) are there for a
game's tests to run on its own content, with the `testing` feature.

## The map editor

Its interface (`src/editor/ui.rs`): a bar along the top with the circuit,
its lap's figures, the modes and the buttons for saving, undoing, redoing,
the help, leaving, the last and next circuit and the whole circuit in view;
down the left a column of buttons for what the mode in use does; on every
button its key, and beside it what it does. Tab opens a window with all of
the keys, the mouse's too. The keys and the buttons come from one table
(`BINDINGS`), so the legends never say other than what the keys do.


In a desktop development build (a game's `make up`, or `cargo run` here),
the map editor (`src/editor/`) is entered from the tracks menu with E, on
the circuit chosen, and left with Esc; or at start-up with `--edit` (or
`--edit bahia`, a circuit by its file's name or its place in the menu; or
`MOTORUST_EDIT=bahia`). It shows the circuit from above in its own world:
its control points and the lap through them as the game lays it, its walls
banked as they are, red where a bend is too tight or a grade too steep and
orange where its banking is felt as a bump off the middle of the road (the
limits the tests check). Along the bottom a panel shows the lap's height
from end to end, with the marks of its profile, and under it the
stretches of banking (each a bar as high as its factor, orange where it is
raised or grips harder) and the bank itself.

- On the map, pick a control point and drag it; on the panel, a mark (its
  height and where it is along the lap) or a stretch's end.
- N adds a point or mark halfway to the next, or splits a stretch; Delete
  takes out what is picked; B adds a stretch of banking where the cursor
  is on the panel's band; [ and ] change its factor, T raises it on
  supports or not, G and Shift+G its grip.
- Ctrl+Z and Ctrl+Y undo and redo; Ctrl+S saves into the circuit's file,
  each list in place of its own (the banking's added after the profile if
  the file had none), the rest of the file as it was, its comments too;
  the circuit is then read again and built anew.
- The right button or W A S D move over the ground, the wheel zooms, Q / E
  turn and R / F tilt; Tab goes to the next of what is picked; Page Up /
  Page Down to another circuit; Home shows all of it.

M switches to the terrain mode, as a theme park game's terrain editor: a
round brush on the ground under the cursor (its edge and the ring within
which it works all the way, coloured by its tool) that, while the left
button is held, raises (1), lowers (2), smooths (3), flattens to where the
stroke began (4) or restores the land as the scenery makes it (5); [ and ]
or Shift and the wheel size it, , and . or Ctrl and the wheel make it
weaker or stronger; Ctrl+Z and Ctrl+Y undo and redo a stroke. The ground's
meshes are shaped in place as it goes; the ground beside the roads is left
to them (shaped only from just past their walls). Saved, the shaping goes
into the circuit's companion, `circuits/NAME.terrain` beside
`circuits/NAME.ron` (listed in its pack's `pack.txt` the first time), a
grid of heights 2 m apart over the land the scenery makes
(`src/world/sculpt.rs`), and the circuit is built anew with it: its ground's
kinds (a steep hill is rock), its trees and its grass where the ground now
is.

In the terrain mode 6 to 9 paint the ground grass, sand or rock, or as
the land makes it again (a byte a point of the companion's grid, read by
the terrain where it chooses each piece of ground). A moment after each
stroke (and each change in the roadside mode), the circuit is built anew
with what was shaped, painted and placed, not yet saved: its ground's kinds,
its trees and its grass where the ground now is; the world before it is
kept in sight till the new one is whole (`world::Superseded`).

M again goes to the roadside mode: what stands by the road at a distance
along the lap (arches, checkpoints among them; billboards; chevron signs;
brake markers; grandstands), drawn as posts where they stand. 1 to 5
choose what N adds where the cursor is; the left button picks one and
drags it along the road, to the side the cursor is on; X turns it to the
other side, Delete takes it out, Tab goes to the next, Ctrl+Z and Ctrl+Y
undo and redo. Saved, each list is written in place of its own, the items
as they were but for where they stand (their texts, colours and comments
kept), new ones from a template (a checkpoint's arch, a billboard of the
game's name, a chevron pointing round the bend, a brake marker at 100 m, a
grandstand); a list the file has not is added after its kerbs' colours.

The stretches of the scenery (walls, tunnels, bridges, rows of lamps and
palms), the branches and the crests the bike takes off over are still to
come.

## Tools

| Tool | What it does |
|---|---|
| `web/build.sh` | Builds the web version of the demo into a folder ready to be served (`dist/web` by default): the page (`web/index.html`), the `.wasm` and the JavaScript that loads it. Needs the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` at `Cargo.lock`'s version of `wasm-bindgen`. |
| `tools/web_pack.py` | Writes the content packs' files for a web build apart from the `.wasm` (which has none built in, see `src/content.rs`): `content.pack`, every file the packs list, laid one over the other, which the page downloads and hands to the game (`unpack` reads it), and the songs loose in `content/`, which the asset server fetches when one plays. `python3 tools/web_pack.py OUT GAME_ASSETS... ENGINE_ASSETS`. Needs Python 3 alone. |
| `tools/circuit_layout.py` | Works a circuit's lap out from its file (`assets/circuits/NAME.ron`) as `src/track.rs` does (the centre line and the height profile), draws it from above and its profile, and reports its length, its tightest bend, its steepest grade and the crests the bike takes off over, against the limits the tests set. Needs numpy (and matplotlib for the pictures). |

## Builds and releases

- **`.github/workflows/build.yml`**, on every push to `main` and every pull
  request (and by hand): format, clippy and the tests, then the demo built
  for Linux x86_64, Windows x86_64, macOS (Apple Silicon and Intel) and
  WebAssembly, each left as an artifact of the run
  (`motorust-engine-<version>-<target>.tar.gz`, or `.zip` for Windows and
  the web). Only on GitHub's runners.
- **`.github/workflows/release.yml`**, by hand from the Actions tab,
  choosing `major`, `minor` or `patch`: the version goes up from the higher
  of the last `vX.Y.Z` tag and `Cargo.toml`'s, `build.yml` builds it with
  that version, `Cargo.toml` and `Cargo.lock` on `main` take it (a commit
  `vX.Y.Z`), and a GitHub release `vX.Y.Z` is made on that commit with the
  builds and their `SHA256SUMS.txt`.

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
