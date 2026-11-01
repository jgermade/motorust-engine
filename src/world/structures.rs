//! The works along the road: tunnels cut through the rock, bridges over the
//! water (with red arches, or of wood) and billboards.

use bevy::prelude::*;

use super::Assets3d;
use super::cavern::cavern_pieces;
use super::lamps::Lamp;
use super::lettering::sign_image;
use super::quads::MeshBuilder;
use super::scenery::{
    Billboard, BrakeMarker, BridgeStyle, ChevronDirection, ChevronSign, FlowerKind, Scenery, Soil,
    Span,
};
use super::terrain::{CLIFF_NORMAL_Y, GRASS_TILE, Ground, GroundMesh, Terrain};
use super::textures::{PLANK_BOARD, PLANK_GAP, WorldTextures};
use crate::geometry::{fbm, random01, smoothstep};
use crate::track::{Nearest, SURFACE_Y, Track, WALL_DISTANCE};

/// Alto de las paredes del túnel hasta el techo (m), y lo que se abomba la
/// roca de encima.
pub const TUNNEL_HEIGHT: f32 = 6.5;
const TUNNEL_ROCK: f32 = 15.0;
/// Hasta dónde llega la roca del túnel a cada lado de la línea central (m).
pub const TUNNEL_SHELL: f32 = WALL_DISTANCE + 12.0;
/// Points of the centre line at least this many apart along the lap are on
/// different stretches of the track: one can pass over a tunnel of the
/// other.
const OTHER_STRETCH: usize = 60;
/// Where another stretch passes over a tunnel, the rock carries it: just
/// below its asphalt as far as this from its centre line (m), and down to
/// the rest of the rock over this much further.
const DECK_FLAT: f32 = WALL_DISTANCE + 1.5;
const DECK_SLOPE: f32 = 12.0;
const DECK_DROP: f32 = 0.3;
/// Franja de tablero de ajedrez al pie de las paredes del túnel (m).
const TUNNEL_CHECKER: f32 = 0.7;
/// Una luz del techo y una costilla cada tantos puntos del trazado.
const TUNNEL_LIGHT_EVERY: usize = 3;
/// The light of the lamps on what is round them: how far it spreads across
/// the road either side of a lamp of the ceiling (m), and up and down the
/// wall from one on the wall (m); its colour, and how bright each pair of
/// lamps of the ceiling is (lm, see [`super::lamps`]).
const POOL_ACROSS: f32 = 3.2;
const POOL_ON_WALL: f32 = 2.6;
const LAMP_COLOR: Color = Color::srgb(1.0, 0.82, 0.58);
const LAMP_LUMENS: f32 = 900_000.0;
/// How high over the road the light of a pair of lamps of the ceiling
/// shines from (m): well under the ceiling, which would otherwise burn
/// white round it.
const LAMP_HEIGHT: f32 = TUNNEL_HEIGHT - 1.8;
/// How bright the lamps' light on the road and walls is drawn.
const POOL_COLOR: Color = Color::srgb(0.42, 0.3, 0.16);
/// A mine's oil lanterns: orange, and dimmer (lm); their light on the rock
/// is already in its colours (see [`super::cavern`]).
const LANTERN_COLOR: Color = Color::srgb(1.0, 0.62, 0.28);
const LANTERN_LUMENS: f32 = 500_000.0;
const TUNNEL_RIB_EVERY: usize = 4;
/// Alto de los arcos del puente sobre el tablero (m), su grosor y la
/// separación entre tirantes (en puntos del trazado).
const ARCH_RISE: f32 = 15.0;
const ARCH_THICKNESS: f32 = 0.9;
const HANGER_EVERY: usize = 2;
const BRIDGE_RED: Color = Color::srgb(0.72, 0.1, 0.07);
/// Steel truss bridges: how high the trusses stand over the deck (m), every
/// how many points of the lap a panel point is, and a pier, and the colour
/// of the steel, a weathered blue grey.
const TRUSS_HEIGHT: f32 = 7.5;
const TRUSS_PANEL: usize = 3;
const TRUSS_PIER_EVERY: usize = 24;
const TRUSS_STEEL: Color = Color::srgb(0.36, 0.48, 0.55);
/// Grosor del tablero del puente (m).
const DECK_DEPTH: f32 = 1.6;
/// What each repetition of the concrete texture covers on the deck (m).
const DECK_TILE: f32 = 4.0;
/// Wooden bridges: what each repetition of the planks covers (m), how far
/// above the asphalt they lie (over its paint), how tall the fence is (m)
/// and the colours of the wood.
const PLANK_TILE: f32 = 4.0;
const PLANK_LIFT: f32 = 0.02;
/// How thick the boards of a wooden bridge are (m), seen through the gaps
/// between them.
const PLANK_THICKNESS: f32 = 0.07;
const POST_WOOD: Color = Color::srgb(0.34, 0.2, 0.09);
const RAIL_WOOD: Color = Color::srgb(0.52, 0.33, 0.15);
/// An old bridge's wood, weathered grey.
const OLD_POST_WOOD: Color = Color::srgb(0.3, 0.27, 0.23);
const OLD_RAIL_WOOD: Color = Color::srgb(0.42, 0.39, 0.34);

/// The meshes of the tunnels and the bridges, with their materials, and
/// the lamps of the tunnels: worked out away from the main thread, with the
/// rest of the circuit (see [`super::prepare`]).
pub fn structure_pieces(
    track: &Track,
    terrain: &Terrain,
    scenery: &Scenery,
    textures: &WorldTextures,
    within: Option<Span>,
) -> (Vec<(Mesh, StandardMaterial)>, Vec<Lamp>, Vec<GroundMesh>) {
    let mut pieces = Vec::new();
    let mut lamps = Vec::new();
    let mut ground = Vec::new();
    let lap = track.lap_length();
    let wanted = |span: &Span| within.is_none_or(|stretch| span.overlaps(&stretch, lap));
    for tunnel in scenery.tunnels.iter().filter(|tunnel| wanted(tunnel)) {
        // Indoors, a tunnel is only a stretch under something else (a table,
        // another stretch of the road), which draws itself.
        if scenery.soil == Soil::Floor {
            continue;
        }
        // A mine's tunnels are its excavation, and great halls' theirs.
        if let Some(halls) = scenery.excavation() {
            let (cavern, lanterns) =
                cavern_pieces(track, terrain, *tunnel, halls, scenery.soil, textures);
            pieces.extend(cavern);
            lamps.extend(lanterns.into_iter().map(|at| Lamp {
                at,
                color: LANTERN_COLOR,
                lumens: LANTERN_LUMENS,
            }));
            continue;
        }
        let indices = indices(track, *tunnel, lap);
        let mut parts = tunnel_meshes(track, terrain, scenery.soil, &indices);
        lamps.append(&mut parts.lamps);
        // The top in flower, drawn as the ground round it is.
        ground.extend(parts.meadows.drain(..).map(|(kind, meadow)| GroundMesh {
            ground: Ground::Grass,
            flowers: Some(kind),
            mesh: meadow,
            layers: None,
        }));
        let rock = match scenery.soil {
            Soil::Rock | Soil::Mine | Soil::Floor => StandardMaterial {
                base_color: Color::srgb(0.85, 0.75, 0.6),
                base_color_texture: Some(textures.rock.clone()),
                perceptual_roughness: 0.9,
                ..default()
            },
            Soil::Granite => StandardMaterial {
                base_color: Color::srgb(0.78, 0.78, 0.8),
                base_color_texture: Some(textures.granite.clone()),
                perceptual_roughness: 0.9,
                ..default()
            },
            Soil::Lunar => StandardMaterial {
                base_color: Color::srgb(0.58, 0.62, 0.70),
                base_color_texture: Some(textures.rock.clone()),
                perceptual_roughness: 0.88,
                ..default()
            },
            Soil::Checkered => StandardMaterial {
                base_color_texture: Some(textures.soil.clone()),
                perceptual_roughness: 0.9,
                ..default()
            },
        };
        // Warm and lit by the lamps, as in the arcade racers of the 90s;
        // through checkerboard
        // earth, a cave of it; in a lunar station, pressurized metallic hull
        // panels lit by cool cyan-white strips.
        let lining = match scenery.soil {
            Soil::Rock | Soil::Mine | Soil::Granite | Soil::Floor => StandardMaterial {
                base_color: Color::srgb(0.95, 0.78, 0.52),
                base_color_texture: Some(textures.panels.clone()),
                emissive: LinearRgba::rgb(0.12, 0.08, 0.035),
                perceptual_roughness: 0.85,
                ..default()
            },
            Soil::Lunar => StandardMaterial {
                base_color: Color::srgb(0.72, 0.82, 0.96),
                base_color_texture: Some(textures.panels.clone()),
                emissive: LinearRgba::rgb(0.06, 0.12, 0.22),
                perceptual_roughness: 0.55,
                metallic: 0.35,
                ..default()
            },
            Soil::Checkered => StandardMaterial {
                base_color_texture: Some(textures.soil.clone()),
                emissive: LinearRgba::rgb(0.1, 0.06, 0.02),
                perceptual_roughness: 0.9,
                ..default()
            },
        };
        let concrete = StandardMaterial {
            base_color: if scenery.soil == Soil::Lunar {
                Color::srgb(0.66, 0.70, 0.78)
            } else {
                Color::srgb(0.75, 0.73, 0.7)
            },
            base_color_texture: Some(textures.plaster.clone()),
            perceptual_roughness: 0.9,
            ..default()
        };
        let lights = if scenery.soil == Soil::Lunar {
            StandardMaterial {
                base_color: Color::srgb(0.78, 0.94, 1.0),
                emissive: LinearRgba::rgb(3.2, 5.4, 7.5),
                ..default()
            }
        } else {
            StandardMaterial {
                base_color: Color::srgb(1.0, 0.9, 0.6),
                emissive: LinearRgba::rgb(6.0, 4.6, 2.2),
                ..default()
            }
        };
        let checker = StandardMaterial {
            base_color_texture: Some(textures.checker.clone()),
            perceptual_roughness: 0.7,
            ..default()
        };
        let grass = StandardMaterial {
            base_color_texture: Some(textures.grass.clone()),
            perceptual_roughness: 0.9,
            reflectance: 0.18,
            ..default()
        };
        // The lamps' light, added over what it falls on.
        let pools = StandardMaterial {
            base_color: if scenery.soil == Soil::Lunar {
                Color::srgb(0.18, 0.32, 0.46)
            } else {
                POOL_COLOR
            },
            base_color_texture: Some(textures.light_pool.clone()),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            fog_enabled: false,
            ..default()
        };
        // Through granite, the frame is cut from it, as a gate in a mountain.
        let frame = match scenery.soil {
            Soil::Granite => rock.clone(),
            _ => concrete.clone(),
        };
        for (mesh, material) in [
            (parts.rock, rock),
            (parts.grass, grass),
            (parts.lining, lining),
            (parts.portals, frame),
            (parts.sidewalks, concrete),
            (parts.lights, lights),
            (parts.checker, checker),
            (parts.pools, pools),
        ] {
            // Through checkerboard earth, no grass grows on the top.
            if mesh.count_vertices() == 0 {
                continue;
            }
            pieces.push((mesh, material));
        }
    }
    if scenery.dams.iter().any(|dam| wanted(&dam.crest)) {
        pieces.extend(super::dam::dam_pieces(track, terrain, scenery, textures));
    }
    for bridge in scenery.bridges.iter().filter(|bridge| wanted(&bridge.span)) {
        // A dam's crest is the dam (see `dam`), and a toy track's deck and
        // supports are a theme's (`Theme::pieces`).
        if matches!(
            bridge.style,
            BridgeStyle::Dam
                | BridgeStyle::SlotTrack
                | BridgeStyle::OrangeTrack
                | BridgeStyle::Slide
        ) {
            continue;
        }
        let indices = indices(track, bridge.span, lap);
        if bridge.style.wooden() {
            let old = bridge.style == BridgeStyle::OldWooden;
            let (planks, logs) = wooden_bridge_meshes(track, terrain, &indices, bridge.fence, old);
            pieces.push((
                planks,
                StandardMaterial {
                    base_color_texture: Some(textures.planks.clone()),
                    perceptual_roughness: if old { 0.95 } else { 0.85 },
                    // The gaps between the boards seen through.
                    alpha_mode: AlphaMode::Mask(0.5),
                    ..default()
                },
            ));
            pieces.push((
                logs,
                StandardMaterial {
                    perceptual_roughness: 0.9,
                    ..default()
                },
            ));
            continue;
        }
        let (deck, steel) = if bridge.style == BridgeStyle::Truss {
            truss_bridge_meshes(track, terrain, &indices)
        } else {
            bridge_meshes(track, terrain, &indices)
        };
        pieces.push((
            deck,
            StandardMaterial {
                base_color: Color::srgb(0.7, 0.7, 0.68),
                base_color_texture: Some(textures.plaster.clone()),
                perceptual_roughness: 0.9,
                ..default()
            },
        ));
        pieces.push((
            steel,
            StandardMaterial {
                perceptual_roughness: 0.45,
                metallic: 0.2,
                ..default()
            },
        ));
    }
    // What the circuit's themes build (see `super::themes`).
    for (_, theme) in scenery.themes.iter() {
        pieces.extend(theme.pieces(track, scenery, within, textures));
    }
    (pieces, lamps, ground)
}

/// The signs along the road: billboards, curve chevrons and brake markers.
pub fn spawn_signs(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    scenery: &Scenery,
    assets: &mut Assets3d,
) {
    for billboard in scenery.billboards {
        spawn_billboard(world, track, terrain, billboard, assets);
    }
    for chevron in scenery.chevrons {
        spawn_chevron(world, track, terrain, chevron, assets);
    }
    for marker in scenery.brake_markers {
        spawn_brake_marker(world, track, terrain, marker, assets);
    }
}

/// The tunnels of a track, as runs of segments of its centre line: the
/// first point and how many segments.
pub struct Tunnels(Vec<(usize, usize)>);

impl Tunnels {
    pub fn new(track: &Track, scenery: &Scenery) -> Self {
        let lap = track.lap_length();
        Self(
            scenery
                .tunnels
                .iter()
                .map(|&span| {
                    let indices = indices(track, span, lap);
                    (indices[0], indices.len() - 1)
                })
                .collect(),
        )
    }

    /// Where another stretch of the track passes over a tunnel and `point`
    /// is within `reach` of the road through it, under its rock (at most
    /// [`TUNNEL_SHELL`]), near that stretch: the point of the road through
    /// the tunnel nearest to it. The ground stays down there, at the level of
    /// the tunnel, and the rock carries the road above (see
    /// [`tunnel_meshes`]).
    pub fn beneath(&self, track: &Track, point: Vec2, reach: f32) -> Option<Nearest> {
        self.0.iter().find_map(|&(from, count)| {
            let road = track.nearest_among(point, from, count);
            if road.distance >= reach {
                return None;
            }
            let over = passing_over(track, from, count, point, track.height_beside(&road))?;
            (over.distance < TUNNEL_SHELL).then_some(road)
        })
    }
}

/// The point nearest to `point` of the stretch of the track that passes
/// over the tunnel through the `count` segments from `from` on, whose road
/// is at `height` there: the nearest away from the tunnel and its
/// approaches, if it is above the tunnel's rock.
fn passing_over(
    track: &Track,
    from: usize,
    count: usize,
    point: Vec2,
    height: f32,
) -> Option<Nearest> {
    let n = track.sample_count();
    let skipped = (count + 2 * OTHER_STRETCH).min(n - 1);
    let over = track.nearest_among(point, (from + count + OTHER_STRETCH) % n, n - skipped);
    (track.height_beside(&over) > height + TUNNEL_HEIGHT + 2.0).then_some(over)
}

/// Los puntos del trazado de un tramo, en orden de carrera.
pub fn indices(track: &Track, span: Span, lap: f32) -> Vec<usize> {
    let first = track.index_at(span.from);
    let count = (span.length(lap) / track.step_length()).round() as usize;
    (0..=count)
        .map(|i| (first + i) % track.sample_count())
        .collect()
}

/// Las partes de un túnel, cada una con su material.
struct TunnelParts {
    /// La roca por encima y a los lados.
    rock: Mesh,
    /// Grass on the gentle parts of the top, and where the fields round it
    /// are in flower, the same flowers, by kind.
    grass: Mesh,
    meadows: Vec<(FlowerKind, Mesh)>,
    /// Paredes y techo por dentro.
    lining: Mesh,
    /// El marco de hormigón de las bocas.
    portals: Mesh,
    /// Aceras entre los pianos y las paredes.
    sidewalks: Mesh,
    lights: Mesh,
    /// Tablero de ajedrez al pie de las paredes.
    checker: Mesh,
    /// The lamps' light on the road, the walkways and the walls.
    pools: Mesh,
    /// The lamps, for their light on what passes under them (see
    /// [`super::lamps`]).
    lamps: Vec<Lamp>,
}

/// A tunnel, straight inside, with a flat roof, warm lights and its walls
/// chequered below, as in the arcade racers of the 90s. Outside it is a
/// block of rock set into the rock walls round it.
fn tunnel_meshes(track: &Track, terrain: &Terrain, soil: Soil, indices: &[usize]) -> TunnelParts {
    let mut rock = MeshBuilder::default();
    let mut grass = MeshBuilder::default();
    let mut meadows: Vec<(FlowerKind, MeshBuilder)> = Vec::new();
    let mut lining = MeshBuilder::default();
    let mut portals = MeshBuilder::default();
    let mut sidewalks = MeshBuilder::default();
    let mut lights = MeshBuilder::default();
    let mut checker = MeshBuilder::default();
    let mut pools = MeshBuilder::default();
    let mut lamps = Vec::new();
    let step = track.step_length();
    let n = track.sample_count();
    // The points of the centre line `back` before and `ahead` after `i`.
    let around = |i: usize, back: usize, ahead: usize| ((i + n - back) % n, (i + ahead) % n);
    let road = |i: usize| track.surface_point(i, 0.0).y + SURFACE_Y;
    // Un punto a `lateral` de la línea central y `height` sobre la carretera.
    let at = |i: usize, lateral: f32, height: f32| {
        let ground = track.surface_point(i, lateral);
        Vec3::new(ground.x, road(i) + height, ground.z)
    };
    // La roca de encima (altura sobre la carretera): en los bordes, a la
    // altura del terreno de al lado, para que no se note dónde empieza, y
    // por el medio, algo abombada e irregular.
    let edge = |i: usize, side: f32| {
        let beyond = track.offset_point(i, side * (TUNNEL_SHELL + 1.0));
        (terrain.height(beyond) - road(i)).max(TUNNEL_HEIGHT + 3.0)
    };
    // Where another stretch of the road passes over the tunnel, the rock
    // carries it: just below its asphalt, and sloping down to the rest of
    // the rock on either side.
    let (first, count) = (indices[0], indices.len() - 1);
    let deck = |i: usize, lateral: f32| {
        let ground = track.offset_point(i, lateral);
        let over = passing_over(track, first, count, ground, road(i))?;
        let reach = 1.0 - smoothstep(DECK_FLAT, DECK_FLAT + DECK_SLOPE, over.distance);
        (reach > 0.0).then(|| (track.height_beside(&over) - DECK_DROP - road(i), reach))
    };
    let rock_top = |i: usize, lateral: f32| {
        let t = (lateral / TUNNEL_SHELL + 1.0) / 2.0;
        let ground = track.surface_point(i, lateral);
        let rugged = fbm(Vec2::new(ground.x, ground.z) / 19.0, 3);
        let hump = TUNNEL_ROCK * 0.2 * (1.0 - (2.0 * t - 1.0).powi(2)) * (0.5 + rugged);
        let top = edge(i, -1.0).lerp(edge(i, 1.0), t) + hump;
        deck(i, lateral).map_or(top, |(deck, reach)| {
            // Over the ground that rises towards the road above, too, but
            // never over that road.
            let earth = terrain.height(Vec2::new(ground.x, ground.z)) - road(i);
            top.lerp(deck, reach).max((earth + 0.05).min(deck))
        })
    };
    // The walls where they stand, closer in where the road narrows (as at
    // a gate the road goes through into the tunnel), and the walkways
    // between them and the asphalt.
    let inner = |k: usize| track.wall_at(k);
    let sidewalk_from = |k: usize| track.half_width_at(k);
    for pair in indices.windows(2) {
        let (i, j) = (pair[0], pair[1]);
        let (vi, vj) = ((i as f32 * step) / 4.0, (i as f32 * step + step) / 4.0);
        let inward = |lateral: f32| {
            let toward = track.offset_point(i, -lateral.signum()) - track.offset_point(i, 0.0);
            Vec3::new(toward.x, 0.0, toward.y)
        };
        for side in [1.0, -1.0] {
            let wall = |k: usize| side * inner(k);
            // Pared: el tablero abajo y el revestimiento hasta el techo.
            checker.quad(
                [
                    (at(i, wall(i), -1.5), Vec2::new(vi * 4.0, 2.0)),
                    (at(j, wall(j), -1.5), Vec2::new(vj * 4.0, 2.0)),
                    (at(j, wall(j), TUNNEL_CHECKER), Vec2::new(vj * 4.0, 0.0)),
                    (at(i, wall(i), TUNNEL_CHECKER), Vec2::new(vi * 4.0, 0.0)),
                ],
                inward(side),
                Color::WHITE,
            );
            lining.quad(
                [
                    (at(i, wall(i), TUNNEL_CHECKER), Vec2::new(vi, 1.5)),
                    (at(j, wall(j), TUNNEL_CHECKER), Vec2::new(vj, 1.5)),
                    (at(j, wall(j), TUNNEL_HEIGHT), Vec2::new(vj, 0.0)),
                    (at(i, wall(i), TUNNEL_HEIGHT), Vec2::new(vi, 0.0)),
                ],
                inward(side),
                Color::WHITE,
            );
            // Aceras.
            let walk = |k: usize| side * sidewalk_from(k);
            // Siguen el peralte, como el arcén que tapan.
            let ground = |k: usize, lateral: f32| {
                track.surface_point(k, lateral) + Vec3::Y * (SURFACE_Y + 0.03)
            };
            sidewalks.quad(
                [
                    (ground(i, walk(i)), Vec2::new(vi, 0.0)),
                    (ground(j, walk(j)), Vec2::new(vj, 0.0)),
                    (ground(j, wall(j)), Vec2::new(vj, 1.0)),
                    (ground(i, wall(i)), Vec2::new(vi, 1.0)),
                ],
                Vec3::Y,
                Color::WHITE,
            );
            // Their kerb, down to under the asphalt: nothing shows through
            // the step between them.
            let kerb = |k: usize| ground(k, walk(k)) - Vec3::Y * 0.12;
            sidewalks.quad(
                [
                    (kerb(i), Vec2::new(vi, 0.05)),
                    (kerb(j), Vec2::new(vj, 0.05)),
                    (ground(j, walk(j)), Vec2::new(vj, 0.0)),
                    (ground(i, walk(i)), Vec2::new(vi, 0.0)),
                ],
                inward(side),
                Color::srgb(0.85, 0.85, 0.85),
            );
            // Lámparas en las paredes, a media altura.
            if i % TUNNEL_LIGHT_EVERY == 0 {
                let lamp = |k: usize| wall(k) - side * 0.05;
                lights.quad(
                    [
                        (at(i, lamp(i), 3.6), Vec2::ZERO),
                        (at(j, lamp(j), 3.6), Vec2::X),
                        (at(j, lamp(j), 4.4), Vec2::ONE),
                        (at(i, lamp(i), 4.4), Vec2::Y),
                    ],
                    inward(side),
                    Color::WHITE,
                );
                // Its light on the wall round it, brightest at the lamp,
                // and on the walkway under it.
                let (before, after) = around(i, 1, 2);
                let glow = |k: usize| wall(k) - side * 0.02;
                let (low, high) = (4.0 - POOL_ON_WALL, (4.0 + POOL_ON_WALL).min(TUNNEL_HEIGHT));
                let v = |height: f32| 0.5 - (height - 4.0) / (2.0 * POOL_ON_WALL);
                pools.quad(
                    [
                        (at(before, glow(before), low), Vec2::new(0.0, v(low))),
                        (at(after, glow(after), low), Vec2::new(1.0, v(low))),
                        (at(after, glow(after), high), Vec2::new(1.0, v(high))),
                        (at(before, glow(before), high), Vec2::new(0.0, v(high))),
                    ],
                    inward(side),
                    Color::WHITE,
                );
                let floor = |k: usize, lateral: f32| {
                    track.surface_point(k, lateral) + Vec3::Y * (SURFACE_Y + 0.045)
                };
                pools.quad(
                    [
                        (floor(before, walk(before)), Vec2::new(0.0, 0.0)),
                        (floor(after, walk(after)), Vec2::new(1.0, 0.0)),
                        (floor(after, glow(after)), Vec2::new(1.0, 0.5)),
                        (floor(before, glow(before)), Vec2::new(0.0, 0.5)),
                    ],
                    Vec3::Y,
                    Color::srgb(0.6, 0.6, 0.6),
                );
            }
            // Los lados de la roca, por fuera.
            let shell = side * TUNNEL_SHELL;
            rock.quad(
                [
                    (at(i, shell, -6.0), Vec2::new(vi, 2.0)),
                    (at(j, shell, -6.0), Vec2::new(vj, 2.0)),
                    (at(j, shell, rock_top(j, shell)), Vec2::new(vj, 0.0)),
                    (at(i, shell, rock_top(i, shell)), Vec2::new(vi, 0.0)),
                ],
                -inward(side),
                Color::WHITE,
            );
        }
        // El techo, por dentro, con dos filas de luces.
        lining.quad(
            [
                (at(i, -inner(i), TUNNEL_HEIGHT), Vec2::new(vi, 0.0)),
                (at(j, -inner(j), TUNNEL_HEIGHT), Vec2::new(vj, 0.0)),
                (
                    at(j, inner(j), TUNNEL_HEIGHT),
                    Vec2::new(vj, inner(j) / 2.0),
                ),
                (
                    at(i, inner(i), TUNNEL_HEIGHT),
                    Vec2::new(vi, inner(i) / 2.0),
                ),
            ],
            Vec3::NEG_Y,
            Color::WHITE,
        );
        // Costillas de lado a lado bajo el techo, que le dan relieve.
        if i % TUNNEL_RIB_EVERY == 0 {
            let rib = TUNNEL_HEIGHT - 0.3;
            lining.beam(
                at(i, -inner(i), rib),
                at(i, inner(i), rib),
                0.6,
                0.6,
                Color::srgb(0.8, 0.8, 0.8),
            );
        }
        if i % TUNNEL_LIGHT_EVERY == 0 {
            for lateral in [-3.5, 3.5] {
                lights.quad(
                    [
                        (at(i, lateral - 0.3, TUNNEL_HEIGHT - 0.05), Vec2::ZERO),
                        (at(j, lateral - 0.3, TUNNEL_HEIGHT - 0.05), Vec2::X),
                        (at(j, lateral + 0.3, TUNNEL_HEIGHT - 0.05), Vec2::ONE),
                        (at(i, lateral + 0.3, TUNNEL_HEIGHT - 0.05), Vec2::Y),
                    ],
                    Vec3::NEG_Y,
                    Color::WHITE,
                );
                // Its light on the road under it, following its banking.
                let (before, after) = around(i, 1, 2);
                let floor = |k: usize, lateral: f32| {
                    track.surface_point(k, lateral) + Vec3::Y * (SURFACE_Y + 0.02)
                };
                let (a, b) = (lateral - POOL_ACROSS, lateral + POOL_ACROSS);
                pools.quad(
                    [
                        (floor(before, a), Vec2::new(0.0, 0.0)),
                        (floor(after, a), Vec2::new(1.0, 0.0)),
                        (floor(after, b), Vec2::new(1.0, 1.0)),
                        (floor(before, b), Vec2::new(0.0, 1.0)),
                    ],
                    Vec3::Y,
                    Color::WHITE,
                );
            }
            // Both lamps, as one light over the middle of the road.
            lamps.push(Lamp {
                at: (at(i, 0.0, LAMP_HEIGHT) + at(j, 0.0, LAMP_HEIGHT)) / 2.0,
                color: LAMP_COLOR,
                lumens: LAMP_LUMENS,
            });
        }
        // La roca de encima, en franjas de lado a lado.
        const STRIPS: usize = 12;
        // Whether its top is gentle enough for grass, judged the whole way
        // across at once, its rugged hump left out: grass and rock lie in
        // bands across it, as on the slopes round it, not in a chequer.
        let level = |k: usize| (edge(k, -1.0) + edge(k, 1.0)) / 2.0;
        let along = (level(j) - level(i)) / step;
        let across = (edge(i, 1.0) - edge(i, -1.0)) / (2.0 * TUNNEL_SHELL);
        let gentle = 1.0 / (1.0 + along * along + across * across).sqrt() > CLIFF_NORMAL_Y;
        for k in 0..STRIPS {
            let (a, b) = (
                -TUNNEL_SHELL + 2.0 * TUNNEL_SHELL * k as f32 / STRIPS as f32,
                -TUNNEL_SHELL + 2.0 * TUNNEL_SHELL * (k + 1) as f32 / STRIPS as f32,
            );
            let corners = [
                (at(i, a, rock_top(i, a)), Vec2::new(vi, a / 4.0)),
                (at(j, a, rock_top(j, a)), Vec2::new(vj, a / 4.0)),
                (at(j, b, rock_top(j, b)), Vec2::new(vj, b / 4.0)),
                (at(i, b, rock_top(i, b)), Vec2::new(vi, b / 4.0)),
            ];
            // Through natural rock, grass grows where the top is gentle, as
            // on the hills around (where a road passes over, by the slope
            // down to it).
            let [p, q, r, _] = corners.map(|(p, _)| p);
            let face = (q - p).cross(r - p).normalize_or_zero();
            let face = if face.y < 0.0 { -face } else { face };
            let grassy = match deck(i, (a + b) / 2.0) {
                Some(_) => face.y > CLIFF_NORMAL_Y,
                None => gentle,
            };
            if matches!(soil, Soil::Rock | Soil::Granite) && grassy {
                let center = corners.iter().map(|(p, _)| *p).sum::<Vec3>() / 4.0;
                let corners = corners.map(|(p, _)| (p, Vec2::new(p.x, p.z) / GRASS_TILE));
                // In a field of flowers, its flowers: the top is the hill
                // the tunnel goes through, not a strip of lawn over it.
                match terrain.field_flowers(Vec2::new(center.x, center.z)) {
                    Some(kind) => {
                        let meadow = match meadows.iter().position(|(k, _)| *k == kind) {
                            Some(meadow) => meadow,
                            None => {
                                meadows.push((kind, MeshBuilder::default()));
                                meadows.len() - 1
                            }
                        };
                        // Coloured corner by corner, as the ground is: no
                        // squares of colour.
                        let [a, b, c, d] = corners.map(|(p, uv)| {
                            let tint = terrain.meadow_color(p, face, kind);
                            (p, uv, Color::LinearRgba(LinearRgba::from_f32_array(tint)))
                        });
                        meadows[meadow].1.shaded_triangle([a, b, c], Vec3::Y);
                        meadows[meadow].1.shaded_triangle([a, c, d], Vec3::Y);
                    }
                    None => {
                        let tint = terrain.grass_color(center, face);
                        grass.quad(
                            corners,
                            Vec3::Y,
                            Color::LinearRgba(LinearRgba::from_f32_array(tint)),
                        );
                    }
                }
            } else {
                rock.quad(corners, Vec3::Y, Color::WHITE);
            }
        }
    }
    // Las bocas: la roca, de lado a lado, con la abertura del túnel en un
    // marco de hormigón que sobresale un poco.
    const FRAME: f32 = 1.2;
    for (end, outward) in [(indices[0], -1.0), (indices[indices.len() - 1], 1.0)] {
        let (_, heading) = track.pose_at(end);
        let ahead = crate::geometry::forward(heading);
        let facing = Vec3::new(ahead.x, 0.0, ahead.y) * outward;
        let top = |lateral: f32| rock_top(end, lateral);
        let inner = inner(end);
        let framed = inner + FRAME;
        let lintel = TUNNEL_HEIGHT + FRAME;
        // La roca: a los lados del marco y por encima.
        for (from, to, bottom) in [
            (-TUNNEL_SHELL, -framed, -6.0),
            (framed, TUNNEL_SHELL, -6.0),
            (-framed, framed, lintel),
        ] {
            rock.quad(
                [
                    (at(end, from, bottom), Vec2::new(from / 4.0, 2.0)),
                    (at(end, to, bottom), Vec2::new(to / 4.0, 2.0)),
                    (at(end, to, top(to)), Vec2::new(to / 4.0, 0.0)),
                    (at(end, from, top(from)), Vec2::new(from / 4.0, 0.0)),
                ],
                facing,
                Color::WHITE,
            );
        }
        // El marco: dos pilares y el dintel, por delante de la roca.
        let push = facing * 0.4;
        for (from, to, bottom, height) in [
            (-framed, -inner, -1.5, lintel),
            (inner, framed, -1.5, lintel),
            (-framed, framed, TUNNEL_HEIGHT, lintel),
        ] {
            portals.quad(
                [
                    (at(end, from, bottom) + push, Vec2::new(from / 4.0, 2.0)),
                    (at(end, to, bottom) + push, Vec2::new(to / 4.0, 2.0)),
                    (at(end, to, height) + push, Vec2::new(to / 4.0, 0.0)),
                    (at(end, from, height) + push, Vec2::new(from / 4.0, 0.0)),
                ],
                facing,
                Color::WHITE,
            );
        }
    }
    TunnelParts {
        rock: rock.build(),
        grass: grass.build(),
        meadows: meadows
            .into_iter()
            .map(|(kind, meadow)| (kind, meadow.build()))
            .collect(),
        lining: lining.build(),
        portals: portals.build(),
        sidewalks: sidewalks.build(),
        lights: lights.build(),
        checker: checker.build(),
        pools: pools.build(),
        lamps,
    }
}

/// El tablero de un puente (de hormigón, por debajo de la carretera) y sus
/// dos arcos rojos, uno a cada lado, con los tirantes que cuelgan de ellos.
/// The deck's top runs out from under the asphalt to past the walls: off
/// the bridge the ground does that, but under a bridge it drops away.
fn bridge_meshes(track: &Track, terrain: &Terrain, indices: &[usize]) -> (Mesh, Mesh) {
    let deck = concrete_deck(
        track,
        terrain,
        indices,
        &[0, indices.len() / 2, indices.len() - 1],
    );
    let mut arches = MeshBuilder::default();
    let edge = WALL_DISTANCE + 0.8;
    let road = |i: usize, lateral: f32| track.surface_point(i, lateral);
    for (k, pair) in indices.windows(2).enumerate() {
        let i = pair[0];
        for side in [-1.0, 1.0] {
            // El arco, a lo largo de todo el puente.
            let arch = |k: usize| {
                let t = k as f32 / (indices.len() - 1) as f32;
                road(indices[k], side * (edge + 0.4))
                    + Vec3::Y * ARCH_RISE * (std::f32::consts::PI * t).sin()
            };
            arches.beam(
                arch(k),
                arch(k + 1),
                ARCH_THICKNESS,
                ARCH_THICKNESS * 1.4,
                BRIDGE_RED,
            );
            if k % HANGER_EVERY == 0 && k > 0 {
                arches.beam(
                    road(i, side * (edge + 0.4)),
                    arch(k),
                    0.18,
                    0.18,
                    BRIDGE_RED,
                );
            }
        }
    }
    (deck.build(), arches.build())
}

/// A concrete deck along the points `indices` of the lap, out to its walls,
/// on pairs of pillars at the points `piers` (of `indices`) down to the
/// ground below.
fn concrete_deck(
    track: &Track,
    terrain: &Terrain,
    indices: &[usize],
    piers: &[usize],
) -> MeshBuilder {
    let mut deck = MeshBuilder::default();
    let edge = WALL_DISTANCE + 0.8;
    // Tucked a little under the asphalt, so no crack shows between them.
    let inner = track.half_width() - 0.3;
    let road = |i: usize, lateral: f32| track.surface_point(i, lateral);
    for pair in indices.windows(2) {
        let (i, j) = (pair[0], pair[1]);
        let down = Vec3::Y * DECK_DEPTH;
        // The top, beside the asphalt, on each side.
        let (across, along) = (
            (edge - inner) / DECK_TILE,
            road(i, 0.0).distance(road(j, 0.0)) / DECK_TILE,
        );
        for side in [-1.0, 1.0] {
            deck.quad(
                [
                    (road(i, side * inner), Vec2::ZERO),
                    (road(j, side * inner), Vec2::new(0.0, along)),
                    (road(j, side * edge), Vec2::new(across, along)),
                    (road(i, side * edge), Vec2::new(across, 0.0)),
                ],
                Vec3::Y,
                Color::WHITE,
            );
        }
        // Por debajo y a los lados del tablero.
        deck.quad(
            [
                (road(i, -edge) - down, Vec2::ZERO),
                (road(j, -edge) - down, Vec2::X),
                (road(j, edge) - down, Vec2::ONE),
                (road(i, edge) - down, Vec2::Y),
            ],
            Vec3::NEG_Y,
            Color::WHITE,
        );
        for side in [-1.0, 1.0] {
            let out = road(i, side * (edge + 1.0)) - road(i, side * edge);
            deck.quad(
                [
                    (road(i, side * edge) - down, Vec2::ZERO),
                    (road(j, side * edge) - down, Vec2::X),
                    (road(j, side * edge), Vec2::ONE),
                    (road(i, side * edge), Vec2::Y),
                ],
                out,
                Color::WHITE,
            );
        }
    }
    // Pilares hasta el suelo.
    for &k in piers {
        let i = indices[k];
        for side in [-1.0, 1.0] {
            let top = road(i, side * (edge - 1.5)) - Vec3::Y * DECK_DEPTH;
            let ground = terrain.height(Vec2::new(top.x, top.z)).min(top.y) - 2.0;
            deck.beam(Vec3::new(top.x, ground, top.z), top, 2.2, 2.2, Color::WHITE);
        }
    }
    deck
}

/// A steel truss bridge: a concrete deck (the first mesh) on piers, and two
/// through trusses (the second), one along each edge of the deck, with a top
/// chord over sloping end posts, a post at every panel point and diagonals
/// sloping down towards the middle, braced to each other overhead.
fn truss_bridge_meshes(track: &Track, terrain: &Terrain, indices: &[usize]) -> (Mesh, Mesh) {
    let last = indices.len() - 1;
    let piers: Vec<usize> = (0..=last)
        .step_by(TRUSS_PIER_EVERY)
        .chain(std::iter::once(last))
        .collect();
    let deck = concrete_deck(track, terrain, indices, &piers);
    let mut steel = MeshBuilder::default();
    let edge = WALL_DISTANCE + 0.8;
    let lateral = edge + 0.3;
    // The panel points, every few points of the lap, the first and the last
    // where the bridge begins and ends.
    let panels = (last / TRUSS_PANEL).max(2);
    let point = |k: usize| indices[(k * last / panels).min(last)];
    let bottom =
        |k: usize, side: f32| track.surface_point(point(k), side * lateral) + Vec3::Y * 0.2;
    let top = |k: usize, side: f32| bottom(k, side) + Vec3::Y * TRUSS_HEIGHT;
    for side in [-1.0, 1.0] {
        for k in 0..panels {
            steel.beam(bottom(k, side), bottom(k + 1, side), 0.5, 0.7, TRUSS_STEEL);
            // The top chord, between the ends, which slope down to the deck.
            if k == 0 {
                steel.beam(bottom(0, side), top(1, side), 0.5, 0.6, TRUSS_STEEL);
            } else if k == panels - 1 {
                steel.beam(top(k, side), bottom(panels, side), 0.5, 0.6, TRUSS_STEEL);
            } else {
                steel.beam(top(k, side), top(k + 1, side), 0.5, 0.6, TRUSS_STEEL);
            }
            if k > 0 {
                steel.beam(bottom(k, side), top(k, side), 0.32, 0.32, TRUSS_STEEL);
            }
            // Diagonals sloping down towards the middle of the span.
            if k > 0 && k < panels - 1 {
                let (from, to) = if 2 * k < panels {
                    (top(k, side), bottom(k + 1, side))
                } else {
                    (bottom(k, side), top(k + 1, side))
                };
                steel.beam(from, to, 0.28, 0.28, TRUSS_STEEL);
            }
        }
    }
    // Braced to each other overhead: a strut across at each panel point and
    // a cross between them.
    for k in 1..panels {
        steel.beam(top(k, -1.0), top(k, 1.0), 0.35, 0.45, TRUSS_STEEL);
        if k + 1 < panels {
            steel.beam(top(k, -1.0), top(k + 1, 1.0), 0.18, 0.18, TRUSS_STEEL);
            steel.beam(top(k, 1.0), top(k + 1, -1.0), 0.18, 0.18, TRUSS_STEEL);
        }
    }
    (deck.build(), steel.build())
}

/// A wooden bridge: planks laid across from fence to fence over the road and
/// on the sides and underside of the deck (the first mesh), and the logs
/// (the second): a fence of posts and two rails on each side, and piers of
/// two logs and a crossbeam down to the water. An `old` one is weathered
/// (see [`BridgeStyle::OldWooden`]): its boards in strips of silver-grey,
/// darker and mossy, its wood grey, its posts leaning and its rails sagging,
/// here and there a lower rail gone.
fn wooden_bridge_meshes(
    track: &Track,
    terrain: &Terrain,
    indices: &[usize],
    fence_height: f32,
    old: bool,
) -> (Mesh, Mesh) {
    let mut planks = MeshBuilder::default();
    let mut logs = MeshBuilder::default();
    // Out to its fence, which stands where the walls are: closer than the
    // edge of the asphalt where the bridge is narrower than the road.
    let edge = |i: usize| track.wall_at(i) + 0.8;
    let step = track.step_length();
    let road = |i: usize, lateral: f32| track.surface_point(i, lateral);
    let deck = |i: usize, lateral: f32| road(i, lateral) + Vec3::Y * (SURFACE_Y + PLANK_LIFT);
    let down = Vec3::Y * (DECK_DEPTH + SURFACE_Y + PLANK_LIFT);
    for (k, pair) in indices.windows(2).enumerate() {
        let (i, j) = (pair[0], pair[1]);
        let across = 2.0 * edge(i) / PLANK_TILE;
        let (v0, v1) = (
            k as f32 * step / PLANK_TILE,
            (k + 1) as f32 * step / PLANK_TILE,
        );
        // Old boards, strip by strip: weathered silver-grey, some darker
        // and damp, some green with moss.
        let board = if old {
            weathered(i as u32)
        } else {
            Color::WHITE
        };
        // The boards' ends either side of each gap between them, as thick
        // as a board, seen through the gap (see `textures::planks`): the
        // end of the board before it, facing on along the road, and the
        // start of the one after, facing back.
        let forward = (deck(j, 0.0) - deck(i, 0.0)).normalize_or_zero();
        let first = (v0 / PLANK_BOARD).floor() as i32;
        for next in first..=(v1 / PLANK_BOARD).ceil() as i32 {
            let gap = next as f32 * PLANK_BOARD;
            for (at, facing, of) in [(gap, forward, next - 1), (gap + PLANK_GAP, -forward, next)] {
                if !(v0..v1).contains(&at) {
                    continue;
                }
                let t = (at - v0) / (v1 - v0);
                let edge = edge(i) + (edge(j) - edge(i)) * t;
                let point = |lateral: f32| deck(i, lateral).lerp(deck(j, lateral), t);
                // Its wood from the middle of its board.
                let v = (of as f32 + 0.5) * PLANK_BOARD;
                let thick = Vec3::Y * PLANK_THICKNESS;
                planks.quad(
                    [
                        (point(-edge), Vec2::new(0.0, v)),
                        (point(edge), Vec2::new(2.0 * edge / PLANK_TILE, v)),
                        (
                            point(edge) - thick,
                            Vec2::new(2.0 * edge / PLANK_TILE, v + PLANK_THICKNESS / PLANK_TILE),
                        ),
                        (
                            point(-edge) - thick,
                            Vec2::new(0.0, v + PLANK_THICKNESS / PLANK_TILE),
                        ),
                    ],
                    facing,
                    board,
                );
            }
        }
        // The boards run across the road: along them is U.
        for (lift, facing) in [(Vec3::ZERO, Vec3::Y), (down, Vec3::NEG_Y)] {
            planks.quad(
                [
                    (deck(i, -edge(i)) - lift, Vec2::new(0.0, v0)),
                    (deck(j, -edge(j)) - lift, Vec2::new(0.0, v1)),
                    (deck(j, edge(j)) - lift, Vec2::new(across, v1)),
                    (deck(i, edge(i)) - lift, Vec2::new(across, v0)),
                ],
                facing,
                board,
            );
        }
        for side in [-1.0, 1.0] {
            let out = road(i, side * (edge(i) + 1.0)) - road(i, side * edge(i));
            let thick = DECK_DEPTH / PLANK_TILE;
            planks.quad(
                [
                    (deck(i, side * edge(i)), Vec2::new(0.0, v0)),
                    (deck(j, side * edge(j)), Vec2::new(0.0, v1)),
                    (deck(j, side * edge(j)) - down, Vec2::new(thick, v1)),
                    (deck(i, side * edge(i)) - down, Vec2::new(thick, v0)),
                ],
                out,
                board,
            );
            // The fence: a post every other point, rails between them; old,
            // the posts lean, the rails sag and here and there the lower one
            // is gone.
            let (post_wood, rail_wood) = if old {
                (OLD_POST_WOOD, OLD_RAIL_WOOD)
            } else {
                (POST_WOOD, RAIL_WOOD)
            };
            let seed =
                |at: usize, n: u32| random01(at as u32 * 13 + n + if side > 0.0 { 7 } else { 0 });
            let sag = |at: usize| if old { 0.12 * (seed(at, 1) - 0.5) } else { 0.0 };
            let fence = |i: usize, height: f32| {
                deck(i, side * (track.wall_at(i) + 0.2)) + Vec3::Y * (height + sag(i))
            };
            for (at, wanted) in [(i, k % 2 == 0), (j, k + 2 == indices.len())] {
                if !wanted {
                    continue;
                }
                let lean = if old {
                    Vec3::new(seed(at, 2) - 0.5, 0.0, seed(at, 3) - 0.5) * 0.3
                } else {
                    Vec3::ZERO
                };
                logs.beam(
                    fence(at, -0.3),
                    fence(at, fence_height) + lean,
                    0.3,
                    0.3,
                    post_wood,
                );
            }
            for (rail, height) in [fence_height - 0.12, fence_height * 0.5]
                .into_iter()
                .enumerate()
            {
                if old && rail == 1 && seed(i, 4) < 0.12 {
                    continue;
                }
                logs.beam(fence(i, height), fence(j, height), 0.17, 0.17, rail_wood);
            }
        }
    }
    // Piers at the ends, at the quarters and in the middle.
    let last = indices.len() - 1;
    for k in [0, last / 4, last / 2, 3 * last / 4, last] {
        let i = indices[k];
        let mut tops = [Vec3::ZERO; 2];
        for (top, side) in tops.iter_mut().zip([-1.0, 1.0]) {
            *top = road(i, side * (edge(i) - 1.2)) - Vec3::Y * DECK_DEPTH;
            let ground = terrain.height(Vec2::new(top.x, top.z)).min(top.y) - 2.0;
            logs.beam(Vec3::new(top.x, ground, top.z), *top, 1.1, 1.1, POST_WOOD);
        }
        logs.beam(
            tops[0] - Vec3::Y * 0.6,
            tops[1] - Vec3::Y * 0.6,
            0.7,
            0.7,
            RAIL_WOOD,
        );
    }
    (planks.build(), logs.build())
}

/// The tint of an old bridge's boards at strip `k` (over the planks'
/// texture, which is a warm brown: the tints take the red out of it, to a
/// grey): mostly weathered silver-grey, some strips darker and damp, some
/// green with moss.
fn weathered(k: u32) -> Color {
    let pick = random01(k * 31 + 5);
    let shade = 0.88 + 0.24 * random01(k * 17 + 3);
    let [r, g, b] = if pick < 0.2 {
        [0.4, 0.47, 0.6]
    } else if pick < 0.32 {
        [0.48, 0.66, 0.55]
    } else {
        [0.62, 0.74, 0.96]
    };
    Color::srgb(r * shade, g * shade, b * shade)
}

/// Un cartel con un rótulo, sobre dos postes detrás del muro, mirando a los
/// que llegan.
/// The size of the billboards, chevron signs and brake markers (m).
const BILLBOARD: Vec2 = Vec2::new(12.0, 3.0);
const CHEVRON: Vec2 = Vec2::new(4.8, 1.6);
const BRAKE_MARKER: Vec2 = Vec2::new(2.2, 1.8);

/// What a billboard says, on white with a border of its colour.
fn billboard_image(billboard: &Billboard) -> Image {
    sign_image(
        billboard.text,
        billboard.color,
        billboard.accent,
        Color::WHITE,
        Some(billboard.color),
        BILLBOARD.x / BILLBOARD.y,
    )
}

/// The arrows of a curve sign: black on yellow.
fn chevron_image(chevron: &ChevronSign) -> Image {
    let text = match chevron.direction {
        ChevronDirection::Left => "<<<",
        ChevronDirection::Right => ">>>",
    };
    let yellow = Color::srgb(0.98, 0.84, 0.05);
    let black = Color::srgb(0.08, 0.08, 0.1);
    sign_image(
        text,
        black,
        None,
        yellow,
        Some(black),
        CHEVRON.x / CHEVRON.y,
    )
}

/// The distance on a brake marker: black on white.
fn brake_marker_image(marker: &BrakeMarker) -> Image {
    let text = if marker.distance >= 100 { "100" } else { "50" };
    let white = Color::srgb(0.96, 0.96, 0.96);
    let black = Color::srgb(0.08, 0.08, 0.1);
    sign_image(
        text,
        black,
        None,
        white,
        Some(black),
        BRAKE_MARKER.x / BRAKE_MARKER.y,
    )
}

/// Draws the signs of `scenery` ahead (see [`sign_image`]), its arches'
/// over a road whose walls stand `wall` metres from its centre line: on the
/// threads that prepare the circuits, so the main thread only copies them.
pub fn draw_signs(scenery: &Scenery, wall: f32) {
    let span = super::road::gantry_span(wall);
    for gantry in scenery.gantries {
        super::road::gantry_image(gantry, span);
    }
    for billboard in scenery.billboards {
        billboard_image(billboard);
    }
    for chevron in scenery.chevrons {
        chevron_image(chevron);
    }
    for marker in scenery.brake_markers {
        brake_marker_image(marker);
    }
}

fn spawn_billboard(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    billboard: &Billboard,
    assets: &mut Assets3d,
) {
    const WIDTH: f32 = BILLBOARD.x;
    const HEIGHT: f32 = BILLBOARD.y;
    const LIFT: f32 = 1.4;
    let index = track.index_at(billboard.along);
    let lateral = billboard.side.sign() * (WALL_DISTANCE + 3.0);
    let point = track.offset_point(index, lateral);
    let base = terrain.height(point);
    let (_, heading) = track.pose_at(index);
    // Girado hacia la carretera para que se lea al llegar.
    let turn = billboard.side.sign() * 0.5;
    let image = billboard_image(billboard);
    let mut panel = MeshBuilder::default();
    panel.quad(
        [
            (Vec3::new(-WIDTH / 2.0, LIFT, 0.0), Vec2::new(0.0, 1.0)),
            (Vec3::new(WIDTH / 2.0, LIFT, 0.0), Vec2::new(1.0, 1.0)),
            (
                Vec3::new(WIDTH / 2.0, LIFT + HEIGHT, 0.0),
                Vec2::new(1.0, 0.0),
            ),
            (Vec3::new(-WIDTH / 2.0, LIFT + HEIGHT, 0.0), Vec2::ZERO),
        ],
        Vec3::Z,
        Color::WHITE,
    );
    let mut back = MeshBuilder::default();
    back.beam(
        Vec3::new(-WIDTH / 2.0, LIFT + HEIGHT / 2.0, -0.08),
        Vec3::new(WIDTH / 2.0, LIFT + HEIGHT / 2.0, -0.08),
        0.12,
        HEIGHT,
        Color::srgb(0.85, 0.85, 0.85),
    );
    for x in [-WIDTH / 3.0, WIDTH / 3.0] {
        back.beam(
            Vec3::new(x, -1.0, -0.2),
            Vec3::new(x, LIFT + HEIGHT * 0.9, -0.2),
            0.2,
            0.2,
            Color::srgb(0.4, 0.4, 0.42),
        );
    }
    world.spawn((
        Transform::from_xyz(point.x, base, point.y)
            .with_rotation(Quat::from_rotation_y(heading + turn)),
        Visibility::default(),
        children![
            (
                Mesh3d(assets.meshes.add(panel.build())),
                MeshMaterial3d(assets.materials.add(StandardMaterial {
                    base_color_texture: Some(assets.images.add(image)),
                    perceptual_roughness: 0.6,
                    ..default()
                })),
            ),
            (
                Mesh3d(assets.meshes.add(back.build())),
                MeshMaterial3d(assets.materials.add(StandardMaterial {
                    perceptual_roughness: 0.7,
                    ..default()
                })),
            ),
        ],
    ));
}

/// Cartel de curva con flechas reflectantes (<<< o >>>) de advertencia en curvas cerradas.
fn spawn_chevron(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    chevron: &ChevronSign,
    assets: &mut Assets3d,
) {
    const WIDTH: f32 = CHEVRON.x;
    const HEIGHT: f32 = CHEVRON.y;
    const LIFT: f32 = 1.0;
    let index = track.index_at(chevron.along);
    let lateral = chevron.side.sign() * (WALL_DISTANCE + 2.4);
    let point = track.offset_point(index, lateral);
    let base = terrain.height(point);
    let (_, heading) = track.pose_at(index);
    let turn = chevron.side.sign() * 0.4;
    let image = chevron_image(chevron);
    let mut panel = MeshBuilder::default();
    panel.quad(
        [
            (Vec3::new(-WIDTH / 2.0, LIFT, 0.0), Vec2::new(0.0, 1.0)),
            (Vec3::new(WIDTH / 2.0, LIFT, 0.0), Vec2::new(1.0, 1.0)),
            (
                Vec3::new(WIDTH / 2.0, LIFT + HEIGHT, 0.0),
                Vec2::new(1.0, 0.0),
            ),
            (Vec3::new(-WIDTH / 2.0, LIFT + HEIGHT, 0.0), Vec2::ZERO),
        ],
        Vec3::Z,
        Color::WHITE,
    );
    let mut back = MeshBuilder::default();
    for x in [-WIDTH / 3.0, WIDTH / 3.0] {
        back.beam(
            Vec3::new(x, -0.6, -0.15),
            Vec3::new(x, LIFT + HEIGHT * 0.8, -0.15),
            0.15,
            0.15,
            Color::srgb(0.5, 0.52, 0.55),
        );
    }
    world.spawn((
        Transform::from_xyz(point.x, base, point.y)
            .with_rotation(Quat::from_rotation_y(heading + turn)),
        Visibility::default(),
        children![
            (
                Mesh3d(assets.meshes.add(panel.build())),
                MeshMaterial3d(assets.materials.add(StandardMaterial {
                    base_color_texture: Some(assets.images.add(image)),
                    perceptual_roughness: 0.5,
                    ..default()
                })),
            ),
            (
                Mesh3d(assets.meshes.add(back.build())),
                MeshMaterial3d(assets.materials.add(StandardMaterial {
                    perceptual_roughness: 0.6,
                    ..default()
                })),
            ),
        ],
    ));
}

/// Cartel indicador de distancia a curva (100 m, 50 m) antes de frenadas fuertes.
fn spawn_brake_marker(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    marker: &BrakeMarker,
    assets: &mut Assets3d,
) {
    const WIDTH: f32 = BRAKE_MARKER.x;
    const HEIGHT: f32 = BRAKE_MARKER.y;
    const LIFT: f32 = 0.9;
    let index = track.index_at(marker.along);
    let lateral = marker.side.sign() * (WALL_DISTANCE + 2.0);
    let point = track.offset_point(index, lateral);
    let base = terrain.height(point);
    let (_, heading) = track.pose_at(index);
    let turn = marker.side.sign() * 0.35;
    let image = brake_marker_image(marker);
    let mut panel = MeshBuilder::default();
    panel.quad(
        [
            (Vec3::new(-WIDTH / 2.0, LIFT, 0.0), Vec2::new(0.0, 1.0)),
            (Vec3::new(WIDTH / 2.0, LIFT, 0.0), Vec2::new(1.0, 1.0)),
            (
                Vec3::new(WIDTH / 2.0, LIFT + HEIGHT, 0.0),
                Vec2::new(1.0, 0.0),
            ),
            (Vec3::new(-WIDTH / 2.0, LIFT + HEIGHT, 0.0), Vec2::ZERO),
        ],
        Vec3::Z,
        Color::WHITE,
    );
    let mut back = MeshBuilder::default();
    for x in [-WIDTH / 4.0, WIDTH / 4.0] {
        back.beam(
            Vec3::new(x, -0.6, -0.12),
            Vec3::new(x, LIFT + HEIGHT * 0.8, -0.12),
            0.12,
            0.12,
            Color::srgb(0.45, 0.48, 0.5),
        );
    }
    world.spawn((
        Transform::from_xyz(point.x, base, point.y)
            .with_rotation(Quat::from_rotation_y(heading + turn)),
        Visibility::default(),
        children![
            (
                Mesh3d(assets.meshes.add(panel.build())),
                MeshMaterial3d(assets.materials.add(StandardMaterial {
                    base_color_texture: Some(assets.images.add(image)),
                    perceptual_roughness: 0.5,
                    ..default()
                })),
            ),
            (
                Mesh3d(assets.meshes.add(back.build())),
                MeshMaterial3d(assets.materials.add(StandardMaterial {
                    perceptual_roughness: 0.6,
                    ..default()
                })),
            ),
        ],
    ));
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    /// The height of the highest upward-facing triangle of `mesh` over
    /// `point` (x and z), if any.
    fn top_at(mesh: &Mesh, point: Vec2) -> Option<f32> {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return None;
        };
        let indices: Vec<usize> = mesh.indices()?.iter().collect();
        indices
            .chunks(3)
            .filter_map(|triangle| {
                let [a, b, c] = [0, 1, 2].map(|n| Vec3::from(positions[triangle[n]]));
                let normal = (b - a).cross(c - a).normalize_or_zero();
                if normal.y < 0.5 {
                    return None;
                }
                // Barycentric coordinates of the point in the triangle, seen
                // from above.
                let flat = |v: Vec3| Vec2::new(v.x, v.z);
                let (a2, b2, c2) = (flat(a), flat(b), flat(c));
                let area = (b2 - a2).perp_dot(c2 - a2);
                let u = (b2 - point).perp_dot(c2 - point) / area;
                let v = (c2 - point).perp_dot(a2 - point) / area;
                let w = 1.0 - u - v;
                (u >= 0.0 && v >= 0.0 && w >= 0.0).then_some(a.y * u + b.y * v + c.y * w)
            })
            .reduce(f32::max)
    }

    pub fn a_tunnels_top_is_in_flower_where_the_fields_round_it_are_on(
        circuits: &'static crate::circuits::Circuits,
        name: &str,
    ) {
        let circuit = circuits
            .iter()
            .find(|circuit| circuit.name == name)
            .unwrap();
        let scenery = &circuit.scenery;
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, scenery);
        let lap = track.lap_length();
        for tunnel in scenery.tunnels {
            let indices = indices(&track, *tunnel, lap);
            let parts = tunnel_meshes(&track, &terrain, scenery.soil, &indices);
            // Its gentle top is grass in flower, as the hill round it,
            // not a strip of lawn over it.
            let flowered: usize = parts
                .meadows
                .iter()
                .map(|(_, meadow)| meadow.count_vertices())
                .sum();
            assert!(flowered > 0);
            for (kind, meadow) in &parts.meadows {
                let Some(VertexAttributeValues::Float32x3(positions)) =
                    meadow.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("positions");
                };
                // Each of its quads (two triangles, four corners) in the
                // flowers of the field it is in.
                for quad in positions.chunks(6).step_by(7) {
                    let mut corners: Vec<Vec2> = Vec::new();
                    for p in quad {
                        let p = Vec2::new(p[0], p[2]);
                        if !corners.contains(&p) {
                            corners.push(p);
                        }
                    }
                    let center = corners.iter().sum::<Vec2>() / corners.len() as f32;
                    assert_eq!(terrain.field_flowers(center), Some(*kind));
                }
            }
        }
    }

    pub fn concrete_bridges_have_a_deck_out_to_the_walls_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut checked = 0;
        for circuit in circuits.iter() {
            let scenery = &circuit.scenery;
            if !scenery.bridges.iter().any(|bridge| !bridge.style.wooden()) {
                continue;
            }
            let track = Track::new(&circuit.layout);
            let terrain = Terrain::new(&track, scenery);
            let lap = track.lap_length();
            for bridge in scenery.bridges {
                let indices = indices(&track, bridge.span, lap);
                let (deck, _) = match bridge.style {
                    BridgeStyle::Wooden
                    | BridgeStyle::OldWooden
                    | BridgeStyle::SlotTrack
                    | BridgeStyle::OrangeTrack
                    | BridgeStyle::Slide => continue,
                    BridgeStyle::RedArches => bridge_meshes(&track, &terrain, &indices),
                    BridgeStyle::Truss => truss_bridge_meshes(&track, &terrain, &indices),
                    // A dam's crest is the dam's own concrete.
                    BridgeStyle::Dam => {
                        let dam = scenery
                            .dams
                            .iter()
                            .find(|dam| dam.crest == bridge.span)
                            .expect("a dam's crest is a dam's");
                        (
                            super::super::dam::dam_mesh(&track, &terrain, dam),
                            Mesh::from(Cuboid::default()),
                        )
                    }
                };
                let middle = indices[indices.len() / 2];
                // From the asphalt's edge out to the walls, which close in
                // where the road over it is narrow.
                let beside = track.half_width_at(middle);
                let by_the_wall = track.wall_at(middle) - 0.3;
                for lateral in [-by_the_wall, -beside, beside, by_the_wall] {
                    let road = track.surface_point(middle, lateral);
                    let top = top_at(&deck, Vec2::new(road.x, road.z)).unwrap_or_else(|| {
                        panic!("{}: a hole {lateral} m from the centre", circuit.name)
                    });
                    assert!((top - road.y).abs() < 0.1, "{top} {}", road.y);
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "some circuit has a concrete bridge");
    }

    pub fn a_wooden_bridge_s_boards_show_their_ends_at_each_gap_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let circuit = &circuits[2];
        let bridge = circuit
            .scenery
            .bridges
            .iter()
            .find(|bridge| bridge.style == BridgeStyle::Wooden)
            .expect("a circuit has a wooden bridge");
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        let indices = indices(&track, bridge.span, track.lap_length());
        let (planks, _) = wooden_bridge_meshes(&track, &terrain, &indices, bridge.fence, false);
        let Some(VertexAttributeValues::Float32x3(normals)) =
            planks.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("the planks have normals");
        };
        // Two ends at each gap, a board apart, facing along the road, each
        // a quad (four corners).
        let length = (indices.len() - 1) as f32 * track.step_length();
        let gaps = length / (PLANK_BOARD * PLANK_TILE);
        let middle = indices[indices.len() / 2];
        let line = track.centerline();
        let along = (line[middle + 1] - line[middle]).normalize();
        let ends = normals
            .iter()
            .filter(|n| Vec3::from(**n).dot(Vec3::new(along.x, 0.0, along.y)).abs() > 0.9)
            .count()
            / 4;
        assert!(
            (ends as f32 - 2.0 * gaps).abs() <= 4.0,
            "{ends} ends for {gaps} gaps"
        );
    }
}
