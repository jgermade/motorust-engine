# Credits of the engine's content

What the engine runs on of its own (this folder): photographs and maps in
the public domain or under CC0, a font under the SIL Open Font License and
the gamepads' art under MIT or Apache-2.0. The demo circuit and roster are
the project's own.

## Textures

All textures come from [Poly Haven](https://polyhaven.com) under the
[CC0](https://creativecommons.org/publicdomain/zero/1.0/) licence (public
domain): they can be used, modified and redistributed without permission or
attribution. Thank you anyway.

They have been downsized and retouched (more saturation, less contrast in the
asphalt cracks and the grass recoloured to a brighter green) to bring them
closer to the look of 90s games. Two of them are only the base of textures
painted at startup: blades of grass are painted over the grass photo, and the
stone wall photo gives its fine grain to a procedural wall of rough stones.

| File | Poly Haven original |
|---|---|
| `textures/grass.jpg` | [Leafy Grass](https://polyhaven.com/a/leafy_grass) |
| `textures/rock.jpg` | [Marble Cliff 03](https://polyhaven.com/a/marble_cliff_03) |
| `textures/wall.jpg` | [Stone Wall](https://polyhaven.com/a/stone_wall) |
| `textures/asphalt.jpg` | [Asphalt 02](https://polyhaven.com/a/asphalt_02) |
| `textures/sand.jpg` | [Coast Sand 01](https://polyhaven.com/a/coast_sand_01) |
| `textures/roof.jpg` | [Clay Roof Tiles](https://polyhaven.com/a/clay_roof_tiles) |
| `textures/plaster.jpg` | [Plastered Wall](https://polyhaven.com/a/plastered_wall) |

## The Moon, the Earth and the Milky Way

The maps of the Moon, the Earth and the Milky Way in the space sky are NASA imagery, in the
public domain, downsized to 1024 × 512 (but the Milky Way, see below):

| File | Original |
|---|---|
| `textures/moon.jpg` | [CGI Moon Kit](https://svs.gsfc.nasa.gov/4720) colour map, Lunar Reconnaissance Orbiter (NASA's Scientific Visualization Studio) |
| `textures/earth.jpg` | [Blue Marble: land surface, shallow water and shaded topography](https://visibleearth.nasa.gov/images/57752) (NASA Earth Observatory) |
| `textures/earth_clouds.jpg` | [Blue Marble: clouds](https://visibleearth.nasa.gov/images/57747) (NASA Earth Observatory) |
| `textures/milky_way.jpg` | [Deep Star Maps 2020](https://svs.gsfc.nasa.gov/4851), the Milky Way alone in galactic coordinates, 4096 × 2048 (converted from its EXR), cut to the band 45° either side of the galactic plane (NASA's Scientific Visualization Studio, from ESA's Gaia and other surveys) |

## Font

`fonts/RussoOne-Regular.ttf` is [Russo One](https://fonts.google.com/specimen/Russo+One)
by Jovanny Lemonad, licensed under the SIL Open Font License 1.1
(`fonts/RussoOne-OFL.txt`).

## Gamepads

The gamepads drawn in Options > Control > Mapping (`ui/pads/dualsense.svg`,
`ui/pads/dualshock4.svg`, `ui/pads/dualshock3.svg` and `ui/pads/xbox.svg`) were
designed by the project's author, with Claude, for
[anygamepad](https://github.com/anygamepad/anygamepad.github.io)
(`apps/web/src/art`), licensed under either the MIT or the Apache-2.0 licence.
They are used as they are; the game draws them with resvg (`src/pad_art.rs`).
