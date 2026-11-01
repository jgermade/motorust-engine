//! La carretera y lo que la bordea: asfalto, líneas, pianos en las curvas,
//! muros (de piedra, de hormigón, barandillas o setos, según el tramo) y los
//! arcos sobre la carretera. Todo sigue la altura y el peralte del trazado.

use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

use super::Assets3d;
use super::lettering::sign_image;
use super::quads::MeshBuilder;
use super::scenery::{BridgeStyle, Gantry, Scenery, Side, Soil, Span, WallStyle};
use super::terrain::{Terrain, shaded};
use super::textures::{STONE_WALL, WorldTextures};
use crate::geometry::{fbm, smoothstep};
use crate::track::{CURB_WIDTH, SURFACE_Y, Track};

/// Tamaño (m) que cubre cada repetición de la textura del asfalto.
const ASPHALT_TILE: f32 = 7.0;
const LINE_WIDTH: f32 = 0.25;
/// Distancia del borde del asfalto a la línea blanca lateral.
const EDGE_LINE_INSET: f32 = 0.45;
/// La línea central: tramos pintados y huecos, en puntos del trazado.
const DASH: (usize, usize) = (2, 3);
/// Las pinturas van un poco por encima del asfalto para no parpadear.
const PAINT_LIFT: f32 = 0.012;
/// Hay pianos donde la curva es más cerrada que este radio (m).
const CURB_RADIUS: f32 = 130.0;

/// How high a wall stands over the road (m): a plinth by the road lifts a
/// small prop over it (a theme's, see [`super::themes::Theme::opens_wall`]).
pub const WALL_HEIGHT: f32 = 1.1;
/// The leaves of a hedge where the grass is as painted.
const HEDGE: Color = Color::srgb(0.55, 0.85, 0.4);
/// Las barreras de hormigón son algo más bajas, y los setos, más altos.
const CONCRETE_HEIGHT: f32 = 1.0;
/// A dam's low parapet over its lake.
pub const PARAPET_HEIGHT: f32 = 0.45;
const HEDGE_HEIGHT: f32 = 1.3;
/// La barandilla: alto (un poste en cada punto del trazado) y color.
const RAILING_HEIGHT: f32 = 1.05;
const RAILING_GREEN: Color = Color::srgb(0.08, 0.42, 0.28);
/// The lacquered vermilion of a shrine's fence.
const SHRINE_VERMILION: Color = Color::srgb(0.8, 0.17, 0.07);
/// The weathered wood of a fence along country lanes.
const FENCE_WOOD: Color = Color::srgb(0.42, 0.33, 0.24);
/// Indoors, the borders of the toy tracks: a slot-car track's low red and
/// white one, the toy cars' orange edge and a slide's tall side (m over the
/// road).
const SLOT_BORDER_HEIGHT: f32 = 0.7;
const ORANGE_EDGE_HEIGHT: f32 = 1.0;
const CHUTE_SIDE_HEIGHT: f32 = 2.45;
const HOT_WHEELS_ORANGE: Color = Color::srgb(1.0, 0.42, 0.04);
const SLIDE_RED: Color = Color::srgb(0.88, 0.12, 0.08);
/// The plastic of a toy track between its lanes and its borders: a slot-car
/// track's charcoal, the toy cars' orange and a slide's yellow.
const SLOT_MARGIN: Color = Color::srgb(0.1, 0.1, 0.11);
const SLIDE_YELLOW: Color = Color::srgb(1.0, 0.8, 0.08);
const WALL_THICKNESS: f32 = 0.6;
/// Lo que se hunde el muro en el suelo, para que no flote en las cuestas.
const WALL_FOOTING: f32 = 0.5;
const WALL_TILE: f32 = 2.4;
/// Donde el terreno detrás del muro sube más que esto, la roca hace de muro.
const CLIFF_RISE: f32 = 3.0;

/// How high the arches over the road stand (m, to the top of their beam),
/// and how deep their beam is (m).
pub const GANTRY_HEIGHT: f32 = 7.5;
pub const GANTRY_BEAM: f32 = 1.6;
/// Tamaño de cada casilla del tablero del arco y de la línea de meta (m).
const CHECKER_TILE: f32 = 1.0;

/// The meshes of the roads and what lines them (asphalt, paint, kerbs,
/// walls, the finish line, the grid and skid marks), with their materials:
/// worked out away from the main thread, with the rest of the circuit (see
/// [`super::prepare`]).
pub fn road_pieces(
    track: &Track,
    terrain: &Terrain,
    scenery: &Scenery,
    textures: &WorldTextures,
) -> Vec<(Mesh, StandardMaterial)> {
    let mut pieces = Vec::new();
    let mut spawn = |mesh: Mesh, material: StandardMaterial| pieces.push((mesh, material));

    // The lap, and the roads off it.
    for number in 0..=track.side_roads().len() {
        let road = Road { track, number };
        spawn_surface(&mut spawn, road, terrain, scenery, textures);
    }

    // Línea de meta: una franja de tablero de ajedrez de lado a lado.
    let checkered = || StandardMaterial {
        base_color_texture: Some(textures.checker.clone()),
        perceptual_roughness: 0.7,
        ..default()
    };
    spawn(finish_line(track), checkered());

    // Parrilla de salida con cajones pintados para las motos.
    spawn(
        starting_grid(track),
        StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.6,
            ..default()
        },
    );

    // Huellas de derrape y frenada en las curvas para dar dinamismo al asfalto
    // (not on a toy's clean plastic, indoors).
    if scenery.soil != Soil::Floor
        && let Some(skids) = skid_marks(track)
    {
        spawn(
            skids,
            StandardMaterial {
                base_color: Color::srgb(0.12, 0.12, 0.14),
                perceptual_roughness: 0.95,
                ..default()
            },
        );
    }
    pieces
}

/// The arches over the road (those `within` a stretch of it, if given).
pub fn spawn_gantries(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    scenery: &Scenery,
    within: Option<Span>,
    assets: &mut Assets3d,
) {
    let lap = track.lap_length();
    for gantry in scenery
        .gantries
        .iter()
        .filter(|gantry| within.is_none_or(|stretch| stretch.contains(gantry.along, lap)))
    {
        spawn_gantry(world, track, gantry, assets);
    }
}

/// A road to build: the lap (number 0), or a road off it (see
/// [`crate::track::Nearest::road`]).
#[derive(Clone, Copy)]
struct Road<'a> {
    track: &'a Track,
    number: usize,
}

impl<'a> Road<'a> {
    /// Its own centre line.
    fn line(&self) -> &'a Track {
        self.track.road(self.number)
    }

    /// Point of its surface `lateral` metres to the left of point `index`
    /// of its centre line: where a road off the lap shares the lap's road,
    /// on the lap's surface.
    fn surface_point(&self, index: usize, lateral: f32) -> Vec3 {
        self.track.surface_point_on(self.number, index, lateral)
    }

    /// Where its paint, curbs and asphalt go above the asphalt of the lap:
    /// a road off the lap is drawn over it where they meet.
    fn lift(&self) -> f32 {
        if self.number == 0 {
            0.0
        } else {
            SIDE_ROAD_LIFT
        }
    }

    /// Whether `point` is on the asphalt (or the curbs) of another road.
    fn on_other_asphalt(&self, point: Vec2) -> bool {
        self.track.on_other_road(self.number, point, |road, _| {
            road.half_width() + CURB_WIDTH - 0.1
        })
    }

    /// Whether `point` is between the walls of another road.
    fn between_other_walls(&self, point: Vec2) -> bool {
        self.track
            .on_other_road(self.number, point, |road, nearest| {
                road.wall_at(nearest.index) - 0.3
            })
    }

    /// Whether it is the road into a loop or out of one.
    fn leads_to_a_loop(&self) -> bool {
        self.track
            .loops()
            .iter()
            .any(|looped| looped.approach == self.number || looped.exit == self.number)
    }

    /// Indoors, which toy track it is at point `index` of its centre line:
    /// the roads into and out of a loop are the toy cars' orange track, as the
    /// loop is; the others are as the lap beside them says (see
    /// [`toy_track`]).
    fn toy_track(&self, scenery: &Scenery, index: usize) -> BridgeStyle {
        if self.leads_to_a_loop() {
            BridgeStyle::OrangeTrack
        } else {
            toy_track(
                scenery,
                self.track.lap_length(),
                self.line().lap_along_at(index),
            )
        }
    }

    /// Whether point `index` of the centre line is in the straight into or
    /// out of a loop.
    fn at_a_loop(&self, index: usize) -> bool {
        let line = self.line();
        self.track.loops().iter().any(|looped| {
            let from_loop = if looped.approach == self.number {
                line.length() - index as f32 * line.step_length()
            } else if looped.exit == self.number {
                index as f32 * line.step_length()
            } else {
                f32::INFINITY
            };
            from_loop < LOOP_STRAIGHT
        })
    }
}

/// Whether point `index` of road number `number` (see
/// [`crate::track::Nearest::road`]) has curbs beside its asphalt: in the
/// corners, and into and out of the loops.
fn has_curbs(road: Road, index: usize) -> bool {
    road.line().smoothed_curvature(index, 4) > 1.0 / CURB_RADIUS || road.at_a_loop(index)
}

/// How far from the centre line of road number `number` the asphalt, or
/// its curbs where it has them, end at point `index` (m): where the ground
/// beside it begins.
pub fn paved_edge(track: &Track, number: usize, index: usize) -> f32 {
    let road = Road { track, number };
    let half = road.line().half_width_at(index);
    if has_curbs(road, index) {
        half + CURB_WIDTH
    } else {
        half
    }
}

/// How far above the asphalt of the lap a road off it goes where they meet
/// (m).
const SIDE_ROAD_LIFT: f32 = 0.01;
/// The road into a loop and out of it has curbs, like the running surface
/// of the loop, over this much of it (m).
const LOOP_STRAIGHT: f32 = 30.0;

/// The asphalt, the paint, the curbs and the walls of one road.
fn spawn_surface(
    spawn: &mut impl FnMut(Mesh, StandardMaterial),
    road: Road,
    terrain: &Terrain,
    scenery: &Scenery,
    textures: &WorldTextures,
) {
    let line = road.line();
    let half = line.half_width();
    // On a wooden bridge the planks are the road (see `structures`): no
    // asphalt or paint under them, which would show past a fence narrower
    // than the road.
    let lap = road.track.lap_length();
    let planked = |index: usize| {
        road.number == 0
            && scenery.bridges.iter().any(|bridge| {
                bridge.style.wooden()
                    && bridge.span.contains(index as f32 * line.step_length(), lap)
            })
    };
    // The asphalt, except where it lies under the lap's.
    let covered = |corners: [Vec2; 4]| corners.iter().all(|&p| road.on_other_asphalt(p));
    let indoors = scenery.soil == Soil::Floor;
    if indoors {
        spawn_toy_tracks(spawn, road, scenery, textures);
    } else {
        spawn(
            ribbon(road, -half, half, 0.0, ASPHALT_TILE, |index, corners| {
                ((road.number == 0 || !covered(corners)) && !planked(index)).then_some(Color::WHITE)
            }),
            StandardMaterial {
                base_color_texture: Some(textures.asphalt.clone()),
                perceptual_roughness: 0.88,
                reflectance: 0.5,
                ..default()
            },
        );
    }

    // Pinturas: líneas blancas a los lados, discontinua en el centro y pianos
    // de dos colores solo en las curvas (and into and out of the loops).
    // Not where they would cross another road's asphalt, so where two roads
    // meet, their asphalt is one.
    let painted = || StandardMaterial {
        perceptual_roughness: 0.7,
        ..default()
    };
    let clear = |corners: [Vec2; 4]| {
        let middle = corners.iter().sum::<Vec2>() / 4.0;
        !road.on_other_asphalt(middle)
    };
    let white =
        |index, corners| (clear(corners) && !planked(index) && !indoors).then_some(Color::WHITE);
    let edge = half - EDGE_LINE_INSET;
    spawn(
        ribbon(road, edge - LINE_WIDTH, edge, PAINT_LIFT, 1.0, white),
        painted(),
    );
    spawn(
        ribbon(road, -edge, -edge + LINE_WIDTH, PAINT_LIFT, 1.0, white),
        painted(),
    );
    let dashes = |index: usize, corners| {
        (index % (DASH.0 + DASH.1) < DASH.0 && clear(corners) && !planked(index) && !indoors)
            .then_some(Color::WHITE)
    };
    let center = LINE_WIDTH / 2.0;
    spawn(
        ribbon(road, -center, center, PAINT_LIFT, 1.0, dashes),
        painted(),
    );
    // Indoors, kerbs only on a slot-car track, as its red and white ones.
    let curbs = |index: usize, corners| {
        let toy = indoors.then(|| road.toy_track(scenery, index));
        (has_curbs(road, index)
            && clear(corners)
            && !planked(index)
            && toy.is_none_or(|style| style == BridgeStyle::SlotTrack))
        .then_some(scenery.curbs[index % 2])
    };
    spawn(
        ribbon(road, half, half + CURB_WIDTH, 0.0, 1.0, curbs),
        painted(),
    );
    spawn(
        ribbon(road, -half - CURB_WIDTH, -half, 0.0, 1.0, curbs),
        painted(),
    );

    // Los muros, cada uno con su aspecto.
    let lap = road.track.lap_length();
    // Indoors the roads into and out of a loop have the toy cars' raised
    // edges, as the loop.
    let orange = indoors && road.leads_to_a_loop();
    let style = |index: usize, side: Side| {
        if orange {
            Some(WallStyle::OrangeEdge)
        } else {
            wall_style(scenery, lap, line.lap_along_at(index), side)
        }
    };
    for (wanted, material) in [
        (
            WallStyle::Stone,
            StandardMaterial {
                base_color_texture: Some(textures.wall.clone()),
                perceptual_roughness: 0.95,
                ..default()
            },
        ),
        (
            WallStyle::Concrete,
            StandardMaterial {
                base_color: Color::srgb(0.92, 0.92, 0.9),
                base_color_texture: Some(textures.plaster.clone()),
                perceptual_roughness: 0.9,
                ..default()
            },
        ),
        (
            WallStyle::Parapet,
            StandardMaterial {
                base_color: Color::srgb(0.92, 0.92, 0.9),
                base_color_texture: Some(textures.plaster.clone()),
                perceptual_roughness: 0.9,
                ..default()
            },
        ),
        (
            WallStyle::Hedge,
            StandardMaterial {
                base_color: hedge_color(scenery.grass_shade),
                base_color_texture: Some(textures.grass.clone()),
                perceptual_roughness: 1.0,
                ..default()
            },
        ),
        (
            WallStyle::Checkered,
            StandardMaterial {
                base_color_texture: Some(textures.soil.clone()),
                perceptual_roughness: 0.9,
                ..default()
            },
        ),
        (
            WallStyle::Timber,
            StandardMaterial {
                base_color: Color::srgb(0.8, 0.68, 0.56),
                base_color_texture: Some(textures.planks.clone()),
                perceptual_roughness: 0.9,
                ..default()
            },
        ),
        (
            WallStyle::SlotBorder,
            StandardMaterial {
                base_color_texture: Some(textures.stripes.clone()),
                perceptual_roughness: 0.4,
                ..default()
            },
        ),
        (
            WallStyle::OrangeEdge,
            StandardMaterial {
                base_color: HOT_WHEELS_ORANGE,
                perceptual_roughness: 0.35,
                ..default()
            },
        ),
        (
            WallStyle::ChuteSide,
            StandardMaterial {
                base_color: SLIDE_RED,
                perceptual_roughness: 0.3,
                ..default()
            },
        ),
    ] {
        let mesh = walls(road, terrain, wanted, |index, side| {
            style(index, side) == Some(wanted)
        });
        if let Some(mesh) = mesh {
            spawn(mesh, material);
        }
    }
    // The see-through ones: the seafront's painted railing, a shrine's
    // lacquered fence, a wooden fence along a lane.
    for (wanted, color, roughness) in [
        (WallStyle::Railing, RAILING_GREEN, 0.5),
        (WallStyle::ShrineFence, SHRINE_VERMILION, 0.7),
        (WallStyle::Fence, FENCE_WOOD, 0.9),
    ] {
        let mesh = railing(road, color, |index, side| {
            style(index, side) == Some(wanted)
        });
        if let Some(mesh) = mesh {
            spawn(
                mesh,
                StandardMaterial {
                    perceptual_roughness: roughness,
                    ..default()
                },
            );
        }
    }
}

/// Which toy track the road is at a point of the lap indoors: the toy cars'
/// orange track or a slide where a bridge of theirs says so, and otherwise
/// a slot-car track, on the floor or on what stands on it.
pub(super) fn toy_track(scenery: &Scenery, lap: f32, along: f32) -> BridgeStyle {
    scenery
        .bridges
        .iter()
        .find(|bridge| bridge.span.contains(along, lap))
        .map(|bridge| bridge.style)
        .filter(|style| matches!(style, BridgeStyle::OrangeTrack | BridgeStyle::Slide))
        .unwrap_or(BridgeStyle::SlotTrack)
}

/// In how many strips across a toy track's lanes are laid (see
/// [`ribbon_in_strips`]): its slots run straight through its banked esses.
const LANE_STRIPS: usize = 6;

/// Indoors, the road as a toy track (see [`toy_track`]): its lanes, and its
/// plastic on out under its borders.
fn spawn_toy_tracks(
    spawn: &mut impl FnMut(Mesh, StandardMaterial),
    road: Road,
    scenery: &Scenery,
    textures: &WorldTextures,
) {
    let line = road.line();
    let half = line.half_width();
    let style = |index: usize| road.toy_track(scenery, index);
    for (wanted, texture, margin, roughness) in [
        (
            BridgeStyle::SlotTrack,
            &textures.slot_track,
            SLOT_MARGIN,
            0.22,
        ),
        (
            BridgeStyle::OrangeTrack,
            &textures.orange_track,
            HOT_WHEELS_ORANGE,
            0.38,
        ),
        (BridgeStyle::Slide, &textures.chute, SLIDE_YELLOW, 0.22),
    ] {
        let mine = |index: usize| (style(index) == wanted).then_some(Color::WHITE);
        let lanes = ribbon_in_strips(
            road,
            -half,
            half,
            0.0,
            ASPHALT_TILE,
            LANE_STRIPS,
            |index, _| mine(index),
        );
        if lanes.count_vertices() == 0 {
            continue;
        }
        spawn(
            lanes,
            StandardMaterial {
                base_color_texture: Some(texture.clone()),
                perceptual_roughness: roughness,
                reflectance: 0.5,
                ..default()
            },
        );
        // Under the kerbs and out to the borders, a little below the lanes.
        let plastic = || StandardMaterial {
            base_color: margin,
            perceptual_roughness: roughness,
            ..default()
        };
        let border = line.wall() + 0.3;
        for (from, to) in [(half, border), (-border, -half)] {
            spawn(
                ribbon(road, from, to, -0.01, 1.0, |index, _| mine(index)),
                plastic(),
            );
        }
    }
}

/// Cómo se cierra la carretera a un lado en un punto de la vuelta, o `None`
/// si no hay muro (en los túneles, que tienen sus paredes).
pub fn wall_style(scenery: &Scenery, lap: f32, along: f32, side: Side) -> Option<WallStyle> {
    // Indoors, a tunnel is only a stretch under something else: the road
    // has its borders there as anywhere.
    if scenery.soil != Soil::Floor && scenery.tunnels.iter().any(|span| span.contains(along, lap)) {
        return None;
    }
    if let Some(bridge) = scenery
        .bridges
        .iter()
        .find(|bridge| bridge.span.contains(along, lap))
    {
        // Wooden bridges have their own fence (see `structures`). Along a
        // dam's crest, a low parapet on the lake's side, so the water shows
        // over it, a little way down.
        return match bridge.style {
            BridgeStyle::Dam
                if scenery
                    .dams
                    .iter()
                    .any(|dam| dam.crest == bridge.span && dam.lake_side == side) =>
            {
                Some(WallStyle::Parapet)
            }
            BridgeStyle::RedArches | BridgeStyle::Truss | BridgeStyle::Dam => {
                Some(WallStyle::Concrete)
            }
            BridgeStyle::Wooden | BridgeStyle::OldWooden => None,
            // A toy track's own borders.
            BridgeStyle::SlotTrack => Some(WallStyle::SlotBorder),
            BridgeStyle::OrangeTrack => Some(WallStyle::OrangeEdge),
            BridgeStyle::Slide => Some(WallStyle::ChuteSide),
        };
    }
    // Where a small prop stands by the road, the wall is the shrine's low
    // fence, so that it is seen whole (see `Theme::opens_wall`).
    if scenery
        .themes
        .iter()
        .any(|(_, theme)| theme.opens_wall(lap, along, side))
    {
        return Some(WallStyle::ShrineFence);
    }
    let run = scenery
        .walls
        .iter()
        .find(|run| run.side == side && run.span.contains(along, lap));
    Some(run.map_or(WallStyle::Stone, |run| run.style))
}

/// Cinta que sigue la carretera entre dos desplazamientos laterales (`from <
/// to`, positivo = izquierda), `lift` metros por encima del asfalto. Cada
/// tramo lleva el color que diga `paint` (from its number and its corners on
/// the ground), o no se pinta si es `None`.
fn ribbon(
    road: Road,
    from: f32,
    to: f32,
    lift: f32,
    tile: f32,
    paint: impl Fn(usize, [Vec2; 4]) -> Option<Color>,
) -> Mesh {
    ribbon_in_strips(road, from, to, lift, tile, 1, paint)
}

/// A ribbon as [`ribbon`], in `strips` strips side by side across it:
/// where the road's banking changes quickly, each of its pieces is twisted
/// (one side rising more than the other), and two triangles across it all
/// fold along their diagonal, bending what is painted along it this way and
/// that, piece after piece; narrower, each is twisted the less.
fn ribbon_in_strips(
    road: Road,
    from: f32,
    to: f32,
    lift: f32,
    tile: f32,
    strips: usize,
    paint: impl Fn(usize, [Vec2; 4]) -> Option<Color>,
) -> Mesh {
    debug_assert!(from < to, "la cinta va de derecha a izquierda");
    let line = road.line();
    let step = line.step_length();
    let mut mesh = MeshBuilder::default();
    for index in 0..line.segment_count() {
        let corners = [
            line.offset_point(index, from),
            line.offset_point(index + 1, from),
            line.offset_point(index + 1, to),
            line.offset_point(index, to),
        ];
        let Some(color) = paint(index, corners) else {
            continue;
        };
        let at = |i: usize, lateral: f32| {
            road.surface_point(i, lateral) + Vec3::Y * (SURFACE_Y + lift + road.lift())
        };
        let (v0, v1) = (index as f32 * step / tile, (index + 1) as f32 * step / tile);
        for strip in 0..strips {
            let across = |k: usize| from + (to - from) * k as f32 / strips as f32;
            let (a, b) = (across(strip), across(strip + 1));
            let (u0, u1) = ((a - from) / tile, (b - from) / tile);
            mesh.quad(
                [
                    (at(index, a), Vec2::new(u0, v0)),
                    (at(index + 1, a), Vec2::new(u0, v1)),
                    (at(index + 1, b), Vec2::new(u1, v1)),
                    (at(index, b), Vec2::new(u1, v0)),
                ],
                Vec3::Y,
                color,
            );
        }
    }
    mesh.build()
}

/// The cross-section of a wall: points (m out from the line of its face on
/// the road's side, m up from the road) from its foot on the road's side,
/// up and over its top, down to its foot on the far side. Its corners are
/// rounded off and it is shaded smoothly round them: no wall has sharp
/// edges.
struct Profile {
    points: Vec<Vec2>,
    /// Whether it is lumpy all over (a hedge).
    lumpy: bool,
    /// The point at the top of the face on the road's side: the texture is
    /// measured down and over from there.
    top: usize,
    /// What each repetition of the texture covers (m along the wall, m
    /// across its profile).
    tile: Vec2,
}

/// How each kind of wall is shaped:
/// - stone: its face leaning back a little as it rises, under a coping of
///   flat stones that juts out on both sides;
/// - concrete: a New Jersey barrier, its foot sloping out to the road;
/// - hedge: rounded, wider at the top than at the root;
/// - a dam's parapet: a low, thick block of concrete;
/// - checkerboard earth and timber: blocks, straight up.
fn profile(style: WallStyle) -> Profile {
    let foot = -WALL_FOOTING;
    // The corners, how much each is rounded off (m), and which is the top
    // of the face on the road's side.
    let (corners, rounding, top, tile): (&[(f32, f32)], f32, usize, Vec2) = match style {
        WallStyle::Stone => (
            &[
                (0.0, foot),
                (0.05, 0.92),
                (-0.04, 0.92),
                (-0.04, 1.03),
                (0.0, WALL_HEIGHT),
                (WALL_THICKNESS, WALL_HEIGHT),
                (WALL_THICKNESS + 0.04, 1.03),
                (WALL_THICKNESS + 0.04, 0.92),
                (WALL_THICKNESS - 0.05, 0.92),
                (WALL_THICKNESS, foot),
            ],
            0.04,
            4,
            STONE_WALL,
        ),
        WallStyle::Concrete => (
            &[
                (0.0, foot),
                (0.0, 0.08),
                (0.17, 0.32),
                (0.225, CONCRETE_HEIGHT - 0.03),
                (0.255, CONCRETE_HEIGHT),
                (0.345, CONCRETE_HEIGHT),
                (0.375, CONCRETE_HEIGHT - 0.03),
                (0.43, 0.32),
                (WALL_THICKNESS, 0.08),
                (WALL_THICKNESS, foot),
            ],
            0.06,
            4,
            Vec2::splat(WALL_TILE),
        ),
        WallStyle::Hedge => (
            &[
                (0.04, foot),
                (0.0, 0.25),
                (-0.06, 0.7),
                (-0.06, 1.0),
                (0.04, 1.2),
                (0.3, HEDGE_HEIGHT),
                (0.56, 1.2),
                (0.66, 1.0),
                (0.66, 0.7),
                (0.6, 0.25),
                (0.56, foot),
            ],
            0.12,
            3,
            Vec2::splat(WALL_TILE),
        ),
        WallStyle::Parapet => (
            &[
                (0.0, foot),
                (0.0, PARAPET_HEIGHT - 0.05),
                (0.05, PARAPET_HEIGHT),
                (WALL_THICKNESS - 0.05, PARAPET_HEIGHT),
                (WALL_THICKNESS, PARAPET_HEIGHT - 0.05),
                (WALL_THICKNESS, foot),
            ],
            0.04,
            2,
            Vec2::splat(WALL_TILE),
        ),
        // A slot-car track's low border, rounded over its top.
        WallStyle::SlotBorder => (
            &[
                (0.0, foot),
                (0.0, SLOT_BORDER_HEIGHT - 0.2),
                (0.15, SLOT_BORDER_HEIGHT),
                (0.5, SLOT_BORDER_HEIGHT),
                (0.6, SLOT_BORDER_HEIGHT - 0.2),
                (0.6, foot),
            ],
            0.08,
            1,
            Vec2::splat(4.0),
        ),
        // The toy cars' edge: thin, straight up, its top lip turned out.
        WallStyle::OrangeEdge => (
            &[
                (0.0, foot),
                (0.0, ORANGE_EDGE_HEIGHT - 0.1),
                (0.06, ORANGE_EDGE_HEIGHT),
                (0.32, ORANGE_EDGE_HEIGHT),
                (0.32, ORANGE_EDGE_HEIGHT - 0.12),
                (0.2, ORANGE_EDGE_HEIGHT - 0.2),
                (0.2, foot),
            ],
            0.04,
            1,
            Vec2::splat(WALL_TILE),
        ),
        // A slide's side: tall, curving up from the chute, rolled over at
        // its top.
        WallStyle::ChuteSide => (
            &[
                (-0.5, 0.0),
                (0.0, 0.5),
                (0.0, CHUTE_SIDE_HEIGHT - 0.25),
                (0.15, CHUTE_SIDE_HEIGHT),
                (0.45, CHUTE_SIDE_HEIGHT),
                (0.5, CHUTE_SIDE_HEIGHT - 0.25),
                (0.5, foot),
            ],
            0.25,
            1,
            Vec2::splat(WALL_TILE),
        ),
        WallStyle::Checkered
        | WallStyle::Timber
        | WallStyle::Railing
        | WallStyle::ShrineFence
        | WallStyle::Fence
        | WallStyle::Blocks
        | WallStyle::Bricks => (
            &[
                (0.0, foot),
                (0.0, WALL_HEIGHT),
                (WALL_THICKNESS, WALL_HEIGHT),
                (WALL_THICKNESS, foot),
            ],
            0.06,
            1,
            Vec2::splat(WALL_TILE),
        ),
    };
    let corners: Vec<Vec2> = corners.iter().map(|&p| Vec2::from(p)).collect();
    let points = round_corners(&corners, rounding);
    let top = (0..points.len())
        .min_by(|&a, &b| {
            let distance = |k: usize| points[k].distance(corners[top]);
            distance(a).total_cmp(&distance(b))
        })
        .expect("a profile has points");
    Profile {
        points,
        lumpy: style == WallStyle::Hedge,
        top,
        tile,
    }
}

/// The leaves of a hedge on a track whose grass is `grass_shade` as light
/// (`Scenery::grass_shade`): as dark and as dull as the grass round it, not
/// a bright green strip over a dark verge.
fn hedge_color(grass_shade: f32) -> Color {
    let [red, green, blue, alpha] = shaded(HEDGE, grass_shade);
    Color::linear_rgba(red, green, blue, alpha)
}

/// The points of a profile with its corners (all but its two ends) rounded
/// off: each replaced by three points along a curve, `radius` (m) back
/// along each side, or a little under half the side if it is shorter.
fn round_corners(corners: &[Vec2], radius: f32) -> Vec<Vec2> {
    let last = corners.len() - 1;
    let mut points = vec![corners[0]];
    for k in 1..last {
        let (before, corner, after) = (corners[k - 1], corners[k], corners[k + 1]);
        let back = |to: Vec2| {
            let side = to - corner;
            corner + side.normalize_or_zero() * radius.min(side.length() * 0.45)
        };
        let (a, b) = (back(before), back(after));
        // The middle of a quadratic curve from a to b about the corner.
        points.extend([a, a * 0.25 + corner * 0.5 + b * 0.25, b]);
    }
    points.push(corners[last]);
    points
}

/// The concrete barriers come in sections this many points of the centre
/// line long, with a dark joint this wide (m) between them.
const BARRIER_SECTION: usize = 2;
const BARRIER_JOINT: f32 = 0.025;

/// How light or dark a wall of `style` is at `p`, `up` from the road: the
/// grime at its foot, slow changes of tone along it, and, on stone, its
/// lighter coping and moss low down; on a hedge, the shade inside it.
fn wall_tint(style: WallStyle, p: Vec3, up: f32) -> Color {
    if matches!(
        style,
        WallStyle::Checkered | WallStyle::SlotBorder | WallStyle::OrangeEdge | WallStyle::ChuteSide
    ) {
        return Color::WHITE;
    }
    let ground = Vec2::new(p.x, p.z);
    let grime = 0.68 + 0.32 * smoothstep(-0.1, 0.4, up);
    let along = 1.0 + 0.2 * (fbm(ground * 0.035, 2) - 0.5);
    let shade = grime * along;
    match style {
        WallStyle::Stone => {
            let coping = if up > 0.91 { 1.1 } else { 1.0 };
            let moss = smoothstep(0.55, 0.75, fbm(ground * 0.12 + Vec2::splat(up * 1.5), 3))
                * (1.0 - smoothstep(0.1, 0.8, up));
            let stone = Color::srgb(shade * coping, shade * coping, shade * coping);
            stone.mix(
                &Color::srgb(0.5 * shade, 0.62 * shade, 0.36 * shade),
                0.55 * moss,
            )
        }
        WallStyle::Hedge => {
            let inside = 0.6 + 0.4 * smoothstep(0.0, 1.1, up);
            let leafy = 0.85 + 0.3 * fbm(ground * 0.4 + Vec2::splat(up * 3.0), 3);
            let v = along * inside * leafy;
            Color::srgb(v, v, v)
        }
        _ => Color::srgb(shade, shade, shade),
    }
}

/// Muros de `style` a los dos lados, donde diga `wanted`, swept along the
/// road with their cross-section (see [`profile`]). Donde detrás sube la
/// roca, no hace falta muro: la pared del peñasco cierra la carretera. Nor
/// where they would stand on another road, where two roads meet. `None` si
/// no hay ninguno.
fn walls(
    road: Road,
    terrain: &Terrain,
    style: WallStyle,
    wanted: impl Fn(usize, Side) -> bool,
) -> Option<Mesh> {
    let line = road.line();
    let step = line.step_length();
    let segments = line.segment_count();
    let profile = profile(style);
    // How far round the profile each point is (m), from the top of the
    // face on the road's side: the texture's v.
    let mut around = vec![0.0; profile.points.len()];
    for k in 1..profile.points.len() {
        around[k] = around[k - 1] + profile.points[k].distance(profile.points[k - 1]);
    }
    let around: Vec<f32> = around.iter().map(|a| a - around[profile.top]).collect();
    // The normal of each edge of the profile (out, up), and at each point
    // the average of the two edges that meet there, for a rounded profile.
    let edge_normal = |k: usize| {
        let e = profile.points[k + 1] - profile.points[k];
        Vec2::new(-e.y, e.x).normalize_or_zero()
    };
    let point_normal = |k: usize| {
        let before = if k > 0 {
            edge_normal(k - 1)
        } else {
            edge_normal(0)
        };
        let after = if k + 1 < profile.points.len() {
            edge_normal(k)
        } else {
            before
        };
        (before + after).normalize_or_zero()
    };
    let mut mesh = WallMesh::default();
    for (side, wall_side) in [(1.0, Side::Left), (-1.0, Side::Right)] {
        let inner = |i: usize| side * line.wall_at(i);
        let cliff = |index: usize| {
            let behind = line.offset_point(index, inner(index) + side * 5.0);
            terrain.height(behind) - road.surface_point(index, inner(index)).y > CLIFF_RISE
        };
        let visible: Vec<bool> = (0..segments)
            .map(|i| {
                wanted(i, wall_side)
                    && !cliff(i)
                    && !cliff(i + 1)
                    && !at_a_junction(road, i, wall_side)
            })
            .collect();
        // Whether the wall goes on before or after a stretch.
        let before = |i: usize| match i {
            0 if line.is_closed() => visible[segments - 1],
            0 => false,
            _ => visible[i - 1],
        };
        let after = |i: usize| match i + 1 {
            next if next == segments && line.is_closed() => visible[0],
            next if next == segments => false,
            next => visible[next],
        };
        // A point of the profile at point `i` of the centre line: where it
        // is, and its outward and upward directions there.
        let frame = |i: usize| {
            let base = road.surface_point(i, inner(i)).y + SURFACE_Y;
            let toward = line.offset_point(i, side) - line.offset_point(i, 0.0);
            let out = Vec3::new(toward.x, 0.0, toward.y).normalize_or_zero();
            (base, out)
        };
        let at = |i: usize, p: Vec2| {
            let (base, _) = frame(i);
            let ground = line.offset_point(i, inner(i) + side * p.x);
            let mut position = Vec3::new(ground.x, base + p.y, ground.y);
            if profile.lumpy {
                // A hedge is lumpy: pushed in and out along its profile.
                let (_, out) = frame(i);
                let lump = fbm(
                    Vec2::new(position.x, position.z) * 0.9 + Vec2::splat(p.y * 2.0),
                    2,
                ) - 0.5;
                position += out * lump * 0.12;
            }
            position
        };
        let direction = |i: usize, n: Vec2| {
            let (_, out) = frame(i);
            (out * n.x + Vec3::Y * n.y).normalize_or_zero()
        };
        let points = profile.points.len();
        let middle = profile.points.iter().sum::<Vec2>() / points as f32;
        // Each run of wall without a break is one sheet, its corners shared
        // round the profile and along the road.
        let mut index = 0;
        while index < segments {
            if !visible[index] {
                index += 1;
                continue;
            }
            let start = index;
            while index < segments && visible[index] {
                index += 1;
            }
            let end = index;
            let first = mesh.positions.len() as u32;
            for i in start..=end {
                let u = i as f32 * step / profile.tile.x;
                for (k, &p) in profile.points.iter().enumerate() {
                    let position = at(i, p);
                    // Shaded smoothly round the profile.
                    let normal = direction(i, point_normal(k));
                    let uv = Vec2::new(u, around[k] / profile.tile.y);
                    mesh.push(position, normal, uv, wall_tint(style, position, p.y));
                }
            }
            let corner = |i: usize, k: usize| first + ((i - start) * points + k) as u32;
            for i in start..end {
                for k in 0..points - 1 {
                    mesh.sheet_quad([
                        corner(i, k),
                        corner(i + 1, k),
                        corner(i + 1, k + 1),
                        corner(i, k + 1),
                    ]);
                }
            }
            // Concrete barriers come in sections with dark joints between
            // them, just in front of the barrier.
            if style == WallStyle::Concrete {
                let dark = Color::srgb(0.2, 0.2, 0.2);
                let t = BARRIER_JOINT / step;
                for i in (start..end).filter(|i| i % BARRIER_SECTION == 0) {
                    for k in 0..points - 1 {
                        let corners = [(0.0, k), (t, k), (t, k + 1), (0.0, k + 1)].map(|(t, k)| {
                            let p = profile.points[k];
                            let normal = direction(i, point_normal(k));
                            let position = at(i, p).lerp(at(i + 1, p), t) + normal * 0.004;
                            (position, normal, Vec2::ZERO, dark)
                        });
                        mesh.quad(corners);
                    }
                }
            }
            // Ends where the wall starts or stops beside the rock: the
            // profile, filled in from its middle.
            for (at_end, open, facing) in
                [(start, !before(start), -1.0), (end, !after(end - 1), 1.0)]
            {
                if !open {
                    continue;
                }
                let ahead = {
                    let segment = at_end.min(end - 1);
                    let (a, b) = (
                        line.offset_point(segment, 0.0),
                        line.offset_point(segment + 1, 0.0),
                    );
                    Vec3::new(b.x - a.x, 0.0, b.y - a.y).normalize_or_zero() * facing
                };
                let center = at(at_end, middle);
                let tint = wall_tint(style, center, middle.y);
                let uv = |p: Vec2| (p - middle) / profile.tile.y;
                for k in 0..points {
                    let (a, b) = (profile.points[k], profile.points[(k + 1) % points]);
                    mesh.triangle(
                        [
                            (center, uv(middle)),
                            (at(at_end, a), uv(a)),
                            (at(at_end, b), uv(b)),
                        ],
                        ahead,
                        tint,
                    );
                }
            }
        }
    }
    (!mesh.is_empty()).then(|| mesh.build())
}

/// A mesh of quads, each corner with its own normal, UV and colour.
#[derive(Default)]
struct WallMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl WallMesh {
    fn push(&mut self, position: Vec3, normal: Vec3, uv: Vec2, color: Color) -> u32 {
        self.positions.push(position.to_array());
        self.normals.push(normal.to_array());
        self.uvs.push(uv.to_array());
        self.colors.push(color.to_linear().to_f32_array());
        (self.positions.len() - 1) as u32
    }

    /// A quad between four vertices already pushed, facing the way their
    /// normals point on average.
    fn sheet_quad(&mut self, [a, b, c, d]: [u32; 4]) {
        let position = |i: u32| Vec3::from(self.positions[i as usize]);
        let facing: Vec3 = [a, b, c, d]
            .iter()
            .map(|&i| Vec3::from(self.normals[i as usize]))
            .sum();
        let (pa, pb, pc) = (position(a), position(b), position(c));
        let face = (pb - pa).cross(pc - pa);
        let face = if face.length_squared() > 1e-12 {
            face
        } else {
            (position(c) - pa).cross(position(d) - pa)
        };
        if face.dot(facing) >= 0.0 {
            self.indices.extend([a, b, c, a, c, d]);
        } else {
            self.indices.extend([a, c, b, a, d, c]);
        }
    }

    /// A quad, facing the way its corners' normals point on average.
    fn quad(&mut self, corners: [(Vec3, Vec3, Vec2, Color); 4]) {
        let [a, b, c, d] = corners.map(|(p, n, uv, color)| self.push(p, n, uv, color));
        let facing: Vec3 = corners.iter().map(|c| c.1).sum();
        let [pa, pb, pc] = [corners[0].0, corners[1].0, corners[2].0];
        if (pb - pa).cross(pc - pa).dot(facing) >= 0.0 {
            self.indices.extend([a, b, c, a, c, d]);
        } else {
            self.indices.extend([a, c, b, a, d, c]);
        }
    }

    /// A flat triangle facing `facing`.
    fn triangle(&mut self, corners: [(Vec3, Vec2); 3], facing: Vec3, color: Color) {
        let [pa, pb, pc] = corners.map(|(p, _)| p);
        let [a, b, c] = corners.map(|(p, uv)| self.push(p, facing, uv, color));
        if (pb - pa).cross(pc - pa).dot(facing) >= 0.0 {
            self.indices.extend([a, b, c]);
        } else {
            self.indices.extend([a, c, b]);
        }
    }

    fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// A railing where `wanted` says, where the walls would stand: a post at
/// every point of the road and two rails, all of `color` (the seafront's
/// green, a shrine's vermilion).
/// Whether another road runs over the line of the wall between `index` and
/// the next point on `side` (a junction): no wall is drawn there, or it
/// would stand across that road (a railing goes by its posts, see
/// [`railing`]).
fn at_a_junction(road: Road, index: usize, side: Side) -> bool {
    let line = road.line();
    let inner = |i: usize| side.sign() * line.wall_at(i);
    let middle = (line.offset_point(index, inner(index))
        + line.offset_point(index + 1, inner(index + 1)))
        / 2.0;
    road.between_other_walls(middle)
}

fn railing(road: Road, color: Color, wanted: impl Fn(usize, Side) -> bool) -> Option<Mesh> {
    let line = road.line();
    let segments = line.segment_count();
    let mut mesh = MeshBuilder::default();
    for side in [Side::Left, Side::Right] {
        let at = |i: usize, height: f32| {
            let lateral = side.sign() * (line.wall_at(i) + 0.2);
            road.surface_point(i, lateral) + Vec3::Y * (SURFACE_Y + height)
        };
        // As a wall is: no post where another road runs over its line,
        // nor beside one that does (so the fence stops short of the
        // junction, not on its edge), and no rail to a post that is not
        // there.
        let over = |index: usize| {
            let index = index % line.sample_count();
            let post = line.offset_point(index, side.sign() * (line.wall_at(index) + 0.2));
            road.between_other_walls(post)
        };
        let shows = |index: usize| {
            wanted(index, side)
                && !over(index)
                && !over(index + 1)
                && !over(index + line.sample_count() - 1)
        };
        for index in 0..segments {
            if !shows(index) {
                continue;
            }
            let next = (index + 1) % line.sample_count();
            mesh.beam(
                at(index, -0.3),
                at(index, RAILING_HEIGHT),
                0.12,
                0.12,
                color,
            );
            if shows(next) {
                for height in [RAILING_HEIGHT, RAILING_HEIGHT * 0.55] {
                    mesh.beam(at(index, height), at(next, height), 0.08, 0.08, color);
                }
            }
        }
    }
    (!mesh.is_empty()).then(|| mesh.build())
}

fn finish_line(track: &Track) -> Mesh {
    let half = track.half_width();
    let (_, heading) = track.pose_at(0);
    let ahead = crate::geometry::forward(heading);
    // Por encima de la línea central discontinua, que pasa por debajo.
    let lift = SURFACE_Y + 2.0 * PAINT_LIFT;
    let at = |lateral: f32, along: f32| {
        track.surface_point(0, lateral) + Vec3::new(ahead.x, 0.0, ahead.y) * along + Vec3::Y * lift
    };
    let squares = 2.0 * half / CHECKER_TILE / 2.0;
    let mut mesh = MeshBuilder::default();
    mesh.quad(
        [
            (at(-half, -CHECKER_TILE), Vec2::new(0.0, 0.0)),
            (at(-half, CHECKER_TILE), Vec2::new(0.0, 1.0)),
            (at(half, CHECKER_TILE), Vec2::new(squares, 1.0)),
            (at(half, -CHECKER_TILE), Vec2::new(squares, 0.0)),
        ],
        Vec3::Y,
        Color::WHITE,
    );
    mesh.build()
}

/// How far either side of the centre line the posts of an arch over a road
/// whose walls stand `wall` metres from it stand (m): just past them.
pub fn gantry_span(wall: f32) -> f32 {
    wall + 1.2
}

/// Arco sobre la carretera, con un tablero de ajedrez o un rótulo por
/// delante y por detrás.
/// The sign on a gantry with text, `span` metres either side of the centre
/// line (see [`gantry_span`]): white letters on its colour.
pub fn gantry_image(gantry: &Gantry, span: f32) -> Option<Image> {
    gantry.text.map(|text| {
        sign_image(
            text,
            Color::WHITE,
            None,
            gantry.color,
            None,
            2.0 * span / GANTRY_BEAM,
        )
    })
}

fn spawn_gantry(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    gantry: &Gantry,
    assets: &mut Assets3d,
) {
    let index = track.index_at(gantry.along);
    let (point, heading) = track.pose_at(index);
    let ground = track.surface_point(index, 0.0).y;
    let span = gantry_span(track.wall());
    let paint = assets.materials.add(StandardMaterial {
        base_color: gantry.color,
        perceptual_roughness: 0.5,
        ..default()
    });
    let pillar = assets
        .meshes
        .add(Cuboid::new(0.8, GANTRY_HEIGHT + 2.0, 0.8));
    let beam = assets
        .meshes
        .add(Cuboid::new(2.0 * span + 0.8, GANTRY_BEAM, 0.7));

    // El tablero o el rótulo, en las dos caras del travesaño: el de delante se
    // lee al llegar, y el de detrás, al mirar atrás.
    let (banner_material, repeat) = match gantry.text {
        None => (
            StandardMaterial {
                base_color_texture: Some(assets.textures.checker.clone()),
                perceptual_roughness: 0.7,
                ..default()
            },
            Vec2::new(
                2.0 * span / CHECKER_TILE / 2.0,
                GANTRY_BEAM / CHECKER_TILE / 2.0,
            ),
        ),
        Some(_) => {
            let image = gantry_image(gantry, span).expect("a gantry with text has a sign");
            (
                StandardMaterial {
                    base_color_texture: Some(assets.images.add(image)),
                    perceptual_roughness: 0.6,
                    ..default()
                },
                Vec2::ONE,
            )
        }
    };
    let mut banner = MeshBuilder::default();
    for (z, facing) in [(-0.36, Vec3::NEG_Z), (0.36, Vec3::Z)] {
        let y = GANTRY_HEIGHT - GANTRY_BEAM / 2.0;
        // Visto desde detrás, la derecha es -X: el rótulo va al revés.
        let (left, right) = if z > 0.0 {
            (-span, span)
        } else {
            (span, -span)
        };
        banner.quad(
            [
                (
                    Vec3::new(left, y - GANTRY_BEAM / 2.0, z),
                    Vec2::new(0.0, repeat.y),
                ),
                (
                    Vec3::new(right, y - GANTRY_BEAM / 2.0, z),
                    Vec2::new(repeat.x, repeat.y),
                ),
                (
                    Vec3::new(right, y + GANTRY_BEAM / 2.0, z),
                    Vec2::new(repeat.x, 0.0),
                ),
                (Vec3::new(left, y + GANTRY_BEAM / 2.0, z), Vec2::ZERO),
            ],
            facing,
            Color::WHITE,
        );
    }

    let flag_pole = assets.meshes.add(Cylinder::new(0.04, 2.2));
    let pole_mat = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.8, 0.82),
        perceptual_roughness: 0.3,
        ..default()
    });
    let mut flag_mesh = MeshBuilder::default();
    flag_mesh.triangle(
        [
            (Vec3::new(0.0, 1.9, 0.0), Vec2::ZERO),
            (Vec3::new(0.0, 0.9, 0.0), Vec2::Y),
            (Vec3::new(1.3, 1.4, 0.15), Vec2::ONE),
        ],
        Vec3::Z,
        gantry.color,
    );
    let flag_cloth = assets.meshes.add(flag_mesh.build());
    let flag_mat = assets.materials.add(StandardMaterial {
        base_color: gantry.color,
        cull_mode: None,
        double_sided: true,
        perceptual_roughness: 0.6,
        ..default()
    });

    world.spawn((
        Transform::from_xyz(point.x, ground, point.y).with_rotation(Quat::from_rotation_y(heading)),
        Visibility::default(),
        children![
            (
                Mesh3d(pillar.clone()),
                MeshMaterial3d(paint.clone()),
                Transform::from_xyz(-span, GANTRY_HEIGHT / 2.0 - 1.0, 0.0),
            ),
            (
                Mesh3d(pillar),
                MeshMaterial3d(paint.clone()),
                Transform::from_xyz(span, GANTRY_HEIGHT / 2.0 - 1.0, 0.0),
            ),
            (
                Mesh3d(beam),
                MeshMaterial3d(paint),
                Transform::from_xyz(0.0, GANTRY_HEIGHT - GANTRY_BEAM / 2.0, 0.0),
            ),
            (
                Mesh3d(assets.meshes.add(banner.build())),
                MeshMaterial3d(assets.materials.add(banner_material)),
            ),
            (
                Mesh3d(flag_pole.clone()),
                MeshMaterial3d(pole_mat.clone()),
                Transform::from_xyz(-span, GANTRY_HEIGHT + 1.1, 0.0),
            ),
            (
                Mesh3d(flag_cloth.clone()),
                MeshMaterial3d(flag_mat.clone()),
                Transform::from_xyz(-span, GANTRY_HEIGHT, 0.0),
            ),
            (
                Mesh3d(flag_pole),
                MeshMaterial3d(pole_mat),
                Transform::from_xyz(span, GANTRY_HEIGHT + 1.1, 0.0),
            ),
            (
                Mesh3d(flag_cloth),
                MeshMaterial3d(flag_mat),
                Transform::from_xyz(span, GANTRY_HEIGHT, 0.0),
            ),
        ],
    ));
}

/// A grid box's front line, along the road: from just ahead of the front
/// tyre of the bike standing in it (m ahead of where the bike stands)...
const GRID_LINE_BACK: f32 = crate::bike::model::NOSE + 0.06;
/// ...to this far ahead.
const GRID_LINE_FRONT: f32 = GRID_LINE_BACK + 0.18;
/// How far behind where the bike stands the box's sides end: a little past
/// its rear tyre (m).
const GRID_BOX_BACK: f32 = crate::bike::model::TAIL + 0.1;
/// Half the width of a grid box, to the middle of its sides (m).
const GRID_BOX_HALF_WIDTH: f32 = 0.85;

/// The starting grid's marks: a box for each bike lined up before the
/// finish line, drawn round the bike standing in it, its front line just
/// ahead of the front tyre and its sides back to just behind the rear one.
fn starting_grid(track: &Track) -> Mesh {
    let mut mesh = MeshBuilder::default();
    let lift = SURFACE_Y + 1.5 * PAINT_LIFT;
    for slot in 0..8 {
        let (point, heading) = track.grid_slot(slot);
        let fwd = crate::geometry::forward(heading);
        let left = Vec2::new(-fwd.y, fwd.x);
        let at = |x_off: f32, z_off: f32| {
            let p = point + left * x_off + fwd * z_off;
            Vec3::new(p.x, track.road_height(p) + lift, p.y)
        };
        // Línea frontal transversal de salida
        let w = GRID_BOX_HALF_WIDTH;
        let (back, front) = (GRID_LINE_BACK, GRID_LINE_FRONT);
        mesh.quad(
            [
                (at(-w, back), Vec2::ZERO),
                (at(w, back), Vec2::X),
                (at(w, front), Vec2::ONE),
                (at(-w, front), Vec2::Y),
            ],
            Vec3::Y,
            Color::WHITE,
        );
        // Marcas laterales del cajón
        for side in [-w, w] {
            let sw = 0.06;
            mesh.quad(
                [
                    (at(side - sw, -GRID_BOX_BACK), Vec2::ZERO),
                    (at(side + sw, -GRID_BOX_BACK), Vec2::X),
                    (at(side + sw, back), Vec2::ONE),
                    (at(side - sw, back), Vec2::Y),
                ],
                Vec3::Y,
                Color::WHITE,
            );
        }
    }
    mesh.build()
}

/// Huellas de neumático en las curvas para dar sensación de carrera.
fn skid_marks(track: &Track) -> Option<Mesh> {
    let mut mesh = MeshBuilder::default();
    let n = track.sample_count();
    let step = track.step_length();
    let lift = SURFACE_Y + 0.5 * PAINT_LIFT;
    for i in 0..n {
        let curv = track.smoothed_curvature(i, 4);
        if curv <= 1.0 / CURB_RADIUS {
            continue;
        }
        let next = (i + 1) % n;
        let at = |idx: usize, lat: f32| track.surface_point(idx, lat) + Vec3::Y * lift;
        for base_lat in [-2.4, 2.4] {
            let (lat0, lat1) = (base_lat - 0.16, base_lat + 0.16);
            let u0 = i as f32 * step / 3.0;
            let u1 = (i + 1) as f32 * step / 3.0;
            mesh.quad(
                [
                    (at(i, lat0), Vec2::new(u0, 0.0)),
                    (at(next, lat0), Vec2::new(u1, 0.0)),
                    (at(next, lat1), Vec2::new(u1, 1.0)),
                    (at(i, lat1), Vec2::new(u0, 1.0)),
                ],
                Vec3::Y,
                Color::srgb(0.12, 0.12, 0.14),
            );
        }
    }
    (!mesh.is_empty()).then(|| mesh.build())
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;
    use crate::track::WALL_DISTANCE;

    #[test]
    fn a_hedge_is_as_dull_as_the_grass_round_it() {
        let painted = hedge_color(1.0).to_srgba();
        let hedge = HEDGE.to_srgba();
        for (a, b) in [
            (painted.red, hedge.red),
            (painted.green, hedge.green),
            (painted.blue, hedge.blue),
        ] {
            assert!((a - b).abs() < 1e-4, "{painted:?} is not {hedge:?}");
        }
        // A duller grass: darker and less vivid.
        let dull = hedge_color(0.36);
        let (dull_srgb, painted_hsl) = (dull.to_srgba(), Hsla::from(hedge_color(1.0)));
        assert!(dull_srgb.red < painted.red && dull_srgb.green < painted.green);
        assert!(Hsla::from(dull).saturation < painted_hsl.saturation - 0.1);
        assert!(Hsla::from(dull).lightness < painted_hsl.lightness - 0.15);
    }

    #[test]
    fn a_bike_on_the_grid_stands_inside_its_box() {
        use crate::bike::model::{NOSE, TAIL};
        let track = Track::default();
        let mesh = starting_grid(&track);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|p| p.as_float3())
            .unwrap();
        for slot in 0..8 {
            let (point, heading) = track.grid_slot(slot);
            let fwd = crate::geometry::forward(heading);
            let left = Vec2::new(-fwd.y, fwd.x);
            // The box's corners round this slot, along the road and across.
            let corners: Vec<_> = positions
                .iter()
                .map(|p| Vec2::new(p[0], p[2]) - point)
                .map(|d| (d.dot(fwd), d.dot(left)))
                .filter(|&(along, across)| along.abs() < 3.0 && across.abs() < 1.5)
                .collect();
            assert!(!corners.is_empty(), "slot {slot} has its box");
            // Only the front line crosses the middle: all of it ahead of the
            // front tyre, though not far ahead.
            let middle = corners.iter().filter(|(_, across)| across.abs() < 0.5);
            for &(along, _) in middle {
                assert!(along > NOSE && along < NOSE + 0.4, "slot {slot}: {along} m");
            }
            // The sides reach back past the rear tyre.
            let back = corners.iter().map(|&(along, _)| along).fold(0.0, f32::min);
            assert!(back < -TAIL && back > -TAIL - 0.4, "slot {slot}: {back} m");
        }
    }

    #[test]
    fn curbs_only_in_the_corners() {
        let track = Track::default();
        let curved = (0..track.sample_count())
            .filter(|&i| track.smoothed_curvature(i, 4) > 1.0 / CURB_RADIUS)
            .count();
        assert!(curved > 0 && curved < track.sample_count() / 2);
    }

    #[test]
    fn walls_are_shaped_by_their_kind() {
        for style in [
            WallStyle::Stone,
            WallStyle::Concrete,
            WallStyle::Hedge,
            WallStyle::Checkered,
            WallStyle::Timber,
        ] {
            let profile = profile(style);
            let points = &profile.points;
            // From the foot on the road's side over the top to the far foot,
            // standing on the line of the wall, as thick as a wall.
            assert_eq!(points[0].y, -WALL_FOOTING, "{style:?}");
            assert_eq!(points[points.len() - 1].y, -WALL_FOOTING, "{style:?}");
            assert!(points[0].x.abs() < 0.05, "{style:?}");
            let top = points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
            assert!((0.9..1.4).contains(&top), "{style:?}: {top}");
            let far = points.iter().map(|p| p.x).fold(f32::MIN, f32::max);
            assert!((0.5..0.75).contains(&far), "{style:?}: {far}");
            assert!(points[profile.top].y > 0.9, "{style:?}");
            // No sharp corners: no two edges in a row turn by more than 60°.
            for k in 1..points.len() - 1 {
                let (a, b) = (points[k] - points[k - 1], points[k + 1] - points[k]);
                assert!(a.angle_to(b).abs() < 60f32.to_radians(), "{style:?} at {k}");
            }
        }
        // A concrete barrier slopes out at its foot: narrower at the top.
        let barrier = profile(WallStyle::Concrete);
        let inner = |from: f32, to: f32| {
            barrier
                .points
                .iter()
                .filter(|p| (from..to).contains(&p.y) && p.x < WALL_THICKNESS / 2.0)
                .map(|p| p.x)
                .fold(f32::MAX, f32::min)
        };
        assert!(inner(0.8, 1.1) > inner(-0.1, 0.1) + 0.15);
        // A stone wall's coping juts out over its face.
        let stone = profile(WallStyle::Stone);
        assert!(stone.points.iter().any(|p| p.x < 0.0));
        // A dam's parapet is low, so the lake shows over it, and as thick
        // as a wall.
        let parapet = profile(WallStyle::Parapet);
        let top = parapet.points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!((0.3..0.6).contains(&top), "{top}");
        let far = parapet.points.iter().map(|p| p.x).fold(f32::MIN, f32::max);
        assert_eq!(far, WALL_THICKNESS);
    }

    pub fn walls_face_out_and_follow_the_road_on(circuits: &'static crate::circuits::Circuits) {
        let circuit = &circuits[0];
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        let road = Road {
            track: &track,
            number: 0,
        };
        for style in [WallStyle::Stone, WallStyle::Concrete, WallStyle::Hedge] {
            let mesh = walls(road, &terrain, style, |index, _| index < 40)
                .expect("there are walls at the start");
            let positions = mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|p| p.as_float3())
                .unwrap();
            let normals = mesh
                .attribute(Mesh::ATTRIBUTE_NORMAL)
                .and_then(|p| p.as_float3())
                .unwrap();
            let Some(Indices::U32(indices)) = mesh.indices() else {
                panic!("indexed");
            };
            // Every triangle winds the way its normals point.
            for triangle in indices.chunks(3) {
                let [a, b, c] = [0, 1, 2].map(|k| Vec3::from(positions[triangle[k] as usize]));
                let n: Vec3 = (0..3)
                    .map(|k| Vec3::from(normals[triangle[k] as usize]))
                    .sum();
                let face = (b - a).cross(c - a);
                if face.length() > 1e-6 && n.length() > 1e-3 {
                    assert!(face.dot(n) > 0.0, "{style:?}");
                }
            }
            // Beside the road, near its walls.
            for &[x, _, z] in positions {
                let distance = track.nearest(Vec2::new(x, z)).distance;
                assert!(
                    (WALL_DISTANCE - 0.2..WALL_DISTANCE + WALL_THICKNESS + 0.4).contains(&distance),
                    "{style:?}: {distance}"
                );
            }
        }
    }

    #[test]
    fn walls_face_out_and_follow_the_road() {
        walls_face_out_and_follow_the_road_on(&crate::testing::DEMO);
    }

    pub fn no_railing_stands_across_another_road_on(
        circuits: &'static crate::circuits::Circuits,
        name: &str,
    ) {
        // The named circuit, where the lap splits and crosses itself and
        // the wall opens into a fence for the props by the road.
        let circuit = circuits
            .iter()
            .find(|circuit| circuit.name == name)
            .unwrap_or_else(|| panic!("{name} is a circuit"));
        let track = Track::new(&circuit.layout);
        let lap = track.lap_length();
        let road = Road {
            track: &track,
            number: 0,
        };
        let line = road.line();
        let style = |index: usize, side: Side| {
            wall_style(&circuit.scenery, lap, line.lap_along_at(index), side)
        };
        let mesh = railing(road, SHRINE_VERMILION, |index, side| {
            style(index, side) == Some(WallStyle::ShrineFence)
        })
        .expect("the fence is somewhere");
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|p| p.as_float3())
            .unwrap();
        for &[x, _, z] in positions {
            let point = Vec2::new(x, z);
            assert!(
                !road.between_other_walls(point),
                "a railing at {point:?} stands on another road"
            );
        }
    }

    pub fn wooden_bridges_have_their_fence_instead_of_walls_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let scenery = &circuit.scenery;
            let lap = Track::new(&circuit.layout).lap_length();
            for bridge in scenery.bridges {
                let span = bridge.span;
                let middle = span.from + span.length(lap) / 2.0;
                let wall = wall_style(scenery, lap, middle, Side::Left);
                match bridge.style {
                    BridgeStyle::RedArches | BridgeStyle::Truss => {
                        assert_eq!(wall, Some(WallStyle::Concrete))
                    }
                    // A dam's crest: a low parapet over the lake, a barrier
                    // over the drop on the other side.
                    BridgeStyle::Dam => {
                        let dam = scenery
                            .dams
                            .iter()
                            .find(|dam| dam.crest == span)
                            .expect("a dam's crest is a dam's");
                        for side in [Side::Left, Side::Right] {
                            assert_eq!(
                                wall_style(scenery, lap, middle, side),
                                Some(if side == dam.lake_side {
                                    WallStyle::Parapet
                                } else {
                                    WallStyle::Concrete
                                }),
                                "{}",
                                circuit.name
                            );
                        }
                    }
                    BridgeStyle::Wooden | BridgeStyle::OldWooden => {
                        assert_eq!(wall, None, "{}", circuit.name)
                    }
                    // A toy track's own borders.
                    BridgeStyle::SlotTrack => assert_eq!(wall, Some(WallStyle::SlotBorder)),
                    BridgeStyle::OrangeTrack => assert_eq!(wall, Some(WallStyle::OrangeEdge)),
                    BridgeStyle::Slide => assert_eq!(wall, Some(WallStyle::ChuteSide)),
                }
                // Off the bridge, the walls are back.
                let off = wall_style(scenery, lap, span.to + 20.0, Side::Left);
                assert!(off.is_some(), "{}", circuit.name);
            }
        }
    }

    #[test]
    fn wooden_bridges_have_their_fence_instead_of_walls() {
        wooden_bridges_have_their_fence_instead_of_walls_on(&crate::testing::DEMO);
    }
}
