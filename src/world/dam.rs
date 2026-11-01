//! A dam across a gorge (see [`Dam`]), as El Atazar's: an arch of concrete
//! bowed towards the lake it holds, its face tall, smooth and nearly sheer,
//! streaked down by the damp, under a crest that juts out over it on
//! brackets. The road runs up the gorge beside the river, looking up at the
//! face, climbs away from it, comes back down onto the crest and runs along
//! it, narrow, the lake a couple of metres under it over a low parapet on
//! one side and the drop to the river on the other, to a paved square with
//! a monument at its far end.
//!
//! What it does to the ground (the river's bed and banks, the gorge's far
//! wall and the lake's bed and rim, see [`DamGround`]) is worked out with
//! the rest of the terrain; the concrete of the dam and the square with its
//! monument are meshes of their own ([`dam_pieces`]), and so is the lake's
//! water, which mirrors what stands round it ([`prepare_lakes`], see
//! [`super::mirror`]).

use bevy::prelude::*;

use super::mirror::{PreparedMirror, Reach, deep, prepare_mirror};
use super::quads::MeshBuilder;
use super::scenery::{Dam, Scenery, Span, span};
use super::structures::indices;
use super::terrain::{SEA_LEVEL, Terrain, signed_distance};
use super::textures::WorldTextures;
use crate::geometry::{fbm, random01, smoothstep};
use crate::track::{Track, closed_curve};

/// The river: how wide its water is either side of its course, and how deep
/// its bed lies (m over the sea, so the sea's water fills it).
pub const RIVER_HALF_WIDTH: f32 = 8.0;
const RIVER_BED: f32 = SEA_LEVEL - 2.5;
/// How steeply the bank on the road's side drops to the water (m down for
/// each metre across).
const BANK_SLOPE: f32 = 1.3;
/// The gorge's far wall, across the river from the road: how far from the
/// water's edge it rises to its full height (m), how far its top runs on
/// level, and where it has faded back into the land round about.
const FAR_WALL_RISE: f32 = 18.0;
const FAR_WALL_TOP: f32 = 28.0;
const FAR_WALL_FADE: f32 = 80.0;
/// How ragged the far wall's foot runs along the river (m either way).
const FAR_WALL_RAGGED: f32 = 4.0;
/// At the foot of the dam the valley opens out, so its face shows across
/// it: the river widens into a pool this much wider either side, with a
/// floor of gravel this much wider again, this high over the sea, before
/// the walls rise; and closes in again between these distances down the
/// river from the dam (m).
const POOL: f32 = 10.0;
const VALLEY: f32 = 26.0;
const VALLEY_FLOOR: f32 = 1.5;
const OPEN_NEAR: f32 = 35.0;
const OPEN_FAR: f32 = 120.0;
/// On the road's bank the floor opens out further down the river, so from
/// the road up the gorge the face shows over it all the way down.
const OPEN_FAR_ROAD: f32 = 200.0;
/// The lake's bed: how far under the water it lies at the shore, how fast
/// it shelves away from it (m down for each metre out) and how deep it gets
/// at most.
const LAKE_EDGE_DEPTH: f32 = 0.6;
const LAKE_SHELVING: f32 = 0.45;
const LAKE_DEPTH: f32 = 18.0;
/// Round the lake, the ground holds the water in: for this many metres from
/// the shore it stands at least this much over the water, rising this much
/// for each metre further out.
const LAKE_RIM: f32 = 45.0;
const LAKE_RIM_RISE: f32 = 0.8;
const LAKE_RIM_SLOPE: f32 = 0.12;
/// The water reaches this far past the shore, under the ground, so no gap
/// shows at the water's edge.
const LAKE_WATER_PAST: f32 = 3.0;
/// The shore as drawn: a point every this many metres.
const SHORE_SPACING: f32 = 4.0;

/// The dam's crest runs on past the road's span this far at each end, into
/// the hillsides.
const ABUTMENT: f32 = 20.0;
/// How far the deck's edge stands past the walls, as on the other bridges;
/// how far the crest juts out past it over the downstream face, and how
/// deep its edge is.
const EDGE_PAST_WALL: f32 = 0.8;
const CORBEL: f32 = 1.4;
const CORBEL_DEPTH: f32 = 2.2;
/// A bracket under the jutting crest every this many points of the lap.
const BRACKET_EVERY: usize = 2;
/// The downstream face, nearly sheer under the crest and leaning out more
/// and more towards its foot, as an arch dam's (m out for each metre down,
/// and for each square metre down); and how far it goes into the ground at
/// its foot.
const FACE_LEAN: f32 = 0.05;
const FACE_FLARE: f32 = 0.0032;
const FOOTING: f32 = 1.5;
const FACE_ROWS: usize = 20;
/// The concrete, pale and a little warm, a texture repeat every this many
/// metres; its streaks, down from the crest, some darker with damp and some
/// rusty.
const CONCRETE: Color = Color::srgb(0.86, 0.84, 0.8);
const CONCRETE_TILE: f32 = 4.0;
const STREAK_DARK: f32 = 0.42;
const RUST: Color = Color::srgb(0.95, 0.82, 0.7);
/// The square at the far end of the crest, and its monument: two slender
/// slabs of concrete standing side by side.
const SQUARE_RADIUS: f32 = 15.0;
const MONUMENT_HEIGHT: f32 = 15.0;
/// What the lake mirrors is mapped this far round its shore (m), a point
/// every this many metres: the hills round it, the roads and the dam; and a
/// line of sight across it is followed this far (see `mirror.wgsl`).
const MIRROR_REACH: f32 = 170.0;
const MIRROR_TEXEL: f32 = 2.0;
const MIRROR_FOLLOW: f32 = 520.0;

/// Where a point is from the river below a dam: how far from its course,
/// on which side of it (positive on the left going downstream), how high
/// the gorge's far wall stands there, and how far down the river from the
/// dam.
struct RiverPoint {
    distance: f32,
    side: f32,
    wall: f32,
    downstream: f32,
}

/// What a dam does to the ground round it, worked out once for each dam.
pub struct DamGround {
    /// Where the road runs along the crest: the dam covers the ground under
    /// it, which is shaped as if the road were not there.
    pub crest: Span,
    /// The lake's shore, a smooth closed curve, and the box round it and
    /// its rim.
    shore: Vec<Vec2>,
    lake_box: (Vec2, Vec2),
    level: f32,
    /// The river's course with the far wall's height at each point, and the
    /// box round it and its walls.
    river: Vec<(Vec2, f32)>,
    river_box: (Vec2, Vec2),
    /// Which side of the river (going downstream) the road runs along:
    /// positive on the left.
    road_bank: f32,
}

impl DamGround {
    pub fn new(dam: &Dam) -> Self {
        let shore = closed_curve(dam.lake, SHORE_SPACING);
        let lake_box = bounds(shore.iter().copied(), LAKE_RIM);
        let river = dam.river.to_vec();
        let river_box = bounds(
            river.iter().map(|&(point, _)| point),
            RIVER_HALF_WIDTH + POOL + VALLEY + FAR_WALL_FADE + FAR_WALL_RAGGED,
        );
        Self {
            crest: dam.crest,
            shore,
            lake_box,
            level: dam.level,
            river,
            river_box,
            road_bank: dam.road_bank.sign(),
        }
    }

    /// The lake's level where `point` is in its water or by it, on the rim
    /// round it (see [`LAKE_RIM`]); none elsewhere.
    pub fn level_by(&self, point: Vec2) -> Option<f32> {
        (within(self.lake_box, point) && signed_distance(&self.shore, point) > -LAKE_RIM)
            .then_some(self.level)
    }

    /// The ground at `point` with the dam's river and lake, where it would
    /// otherwise stand at `height`; `free` is how much of the change is
    /// made (0 on a road, which keeps its own ground).
    pub fn shape(&self, point: Vec2, height: f32, free: f32) -> f32 {
        if free <= 0.0 {
            return height;
        }
        // The rim first, then the gorge, cut through it under the dam, and
        // last the lake's bed.
        let height = height.lerp(self.rim(point, height), free);
        let height = height.lerp(self.gorge(point, height), free);
        height.lerp(self.lake_bed(point, height), free)
    }

    /// The river's bed, the bank dropping to it from the road and the far
    /// wall rising across it; at the foot of the dam, a pool, and a floor of
    /// gravel either side of it before the walls, so the dam's face shows
    /// across the whole valley.
    fn gorge(&self, point: Vec2, height: f32) -> f32 {
        if !within(self.river_box, point) {
            return height;
        }
        let Some(at) = self.river_at(point) else {
            return height;
        };
        let opening = 1.0 - smoothstep(OPEN_NEAR, OPEN_FAR, at.downstream);
        let water = RIVER_HALF_WIDTH + POOL * opening;
        let road_side = at.side * self.road_bank > 0.0;
        let floor_opening = if road_side {
            1.0 - smoothstep(OPEN_NEAR, OPEN_FAR_ROAD, at.downstream)
        } else {
            opening
        };
        let foot = water + VALLEY * floor_opening;
        // Out from the water, the floor of the valley, a little over it.
        let floor_at = |distance: f32| {
            (RIVER_BED + (distance - water).max(0.0) * BANK_SLOPE).min(VALLEY_FLOOR)
        };
        let floor = floor_at(at.distance.min(foot));
        if road_side {
            // The road's bank only drops to the water.
            return height.min(floor + (at.distance - foot).max(0.0) * BANK_SLOPE);
        }
        let ragged = FAR_WALL_RAGGED * 2.0 * (fbm(point / 23.0 + Vec2::splat(11.0), 2) - 0.5);
        let across = at.distance - foot + RIVER_HALF_WIDTH + ragged;
        let rugged = 0.82 + 0.36 * fbm(point / 19.0, 3);
        let rise = smoothstep(RIVER_HALF_WIDTH, RIVER_HALF_WIDTH + FAR_WALL_RISE, across);
        let fade = 1.0
            - smoothstep(
                RIVER_HALF_WIDTH + FAR_WALL_TOP,
                RIVER_HALF_WIDTH + FAR_WALL_FADE,
                across,
            );
        let face = floor.max(RIVER_BED + (at.wall * rugged - RIVER_BED) * rise * fade);
        // Up its face the wall is all there is; past its top the ground
        // stands at least as high as it.
        let on_face = 1.0
            - smoothstep(
                RIVER_HALF_WIDTH + 0.7 * FAR_WALL_RISE,
                RIVER_HALF_WIDTH + FAR_WALL_RISE,
                across,
            );
        height.max(face).lerp(face, on_face)
    }

    /// Where `point` is from the river's course (see [`RiverPoint`]); none
    /// upstream of its first point, under the crest.
    fn river_at(&self, point: Vec2) -> Option<RiverPoint> {
        let mut best: Option<RiverPoint> = None;
        let mut downstream = 0.0;
        for (k, pair) in self.river.windows(2).enumerate() {
            let ((a, wall_a), (b, wall_b)) = (pair[0], pair[1]);
            let ab = b - a;
            let length = ab.length();
            let t = (point - a).dot(ab) / (length * length);
            if k == 0 && t < 0.0 {
                downstream += length;
                continue;
            }
            let t = t.clamp(0.0, 1.0);
            let on = a + ab * t;
            let distance = point.distance(on);
            if best
                .as_ref()
                .is_none_or(|nearest| distance < nearest.distance)
            {
                let left = Vec2::new(ab.y, -ab.x);
                best = Some(RiverPoint {
                    distance,
                    side: (point - on).dot(left).signum(),
                    wall: wall_a.lerp(wall_b, t),
                    downstream: downstream + length * t,
                });
            }
            downstream += length;
        }
        best
    }

    /// The lake's bed, under the water.
    fn lake_bed(&self, point: Vec2, height: f32) -> f32 {
        if !within(self.lake_box, point) {
            return height;
        }
        let inside = signed_distance(&self.shore, point);
        if inside <= 0.0 {
            return height;
        }
        let deep = (inside * LAKE_SHELVING).min(LAKE_DEPTH);
        height.min(self.level - LAKE_EDGE_DEPTH - deep)
    }

    /// The ground round the lake, holding its water in (but where the dam
    /// does, across the gorge, which is cut through it).
    fn rim(&self, point: Vec2, height: f32) -> f32 {
        if !within(self.lake_box, point) {
            return height;
        }
        let out = -signed_distance(&self.shore, point);
        if !(0.0..=LAKE_RIM).contains(&out) {
            return height;
        }
        let rim = self.level + LAKE_RIM_RISE + LAKE_RIM_SLOPE * out;
        let hold = 1.0 - smoothstep(0.6 * LAKE_RIM, LAKE_RIM, out);
        height.lerp(height.max(rim), hold)
    }
}

/// The box round `points`, `margin` metres bigger all round.
fn bounds(points: impl Iterator<Item = Vec2>, margin: f32) -> (Vec2, Vec2) {
    let (min, max) = points.fold(
        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
        |(min, max), p| (min.min(p), max.max(p)),
    );
    (min - margin, max + margin)
}

fn within((min, max): (Vec2, Vec2), point: Vec2) -> bool {
    point.cmpge(min).all() && point.cmple(max).all()
}

/// The dam of each of `scenery`'s dams and its square with the monument,
/// with their materials (the lake's water is drawn on its own: see
/// [`prepare_lakes`]).
pub fn dam_pieces(
    track: &Track,
    terrain: &Terrain,
    scenery: &Scenery,
    textures: &WorldTextures,
) -> Vec<(Mesh, StandardMaterial)> {
    let concrete = || StandardMaterial {
        base_color: CONCRETE,
        base_color_texture: Some(textures.plaster.clone()),
        perceptual_roughness: 0.92,
        ..default()
    };
    let mut pieces = Vec::new();
    for dam in scenery.dams {
        pieces.push((dam_mesh(track, terrain, dam), concrete()));
        if let Some(at) = dam.monument {
            pieces.push((square_mesh(terrain, at), concrete()));
        }
    }
    pieces
}

/// A dam's lake, worked out with the rest of the world (off the main
/// thread): its water, and what it mirrors (see [`super::mirror`]).
pub struct PreparedLake {
    pub mesh: Mesh,
    pub mirror: PreparedMirror,
}

/// What a dam's lake mirrors is mapped round its shore, as far as
/// [`MIRROR_REACH`], a texel every [`MIRROR_TEXEL`].
pub(super) fn lake_reach(dam: &Dam) -> Reach {
    let shore = closed_curve(dam.lake, SHORE_SPACING);
    let (min, max) = bounds(shore.iter().copied(), MIRROR_REACH);
    Reach {
        min,
        max,
        texel: MIRROR_TEXEL,
        level: dam.level,
        follow: MIRROR_FOLLOW,
    }
}

/// The lakes of `scenery`'s dams, with the maps their water mirrors (worked
/// out on `threads` threads).
pub fn prepare_lakes(
    track: &Track,
    terrain: &Terrain,
    scenery: &Scenery,
    textures: &WorldTextures,
    threads: usize,
) -> Vec<PreparedLake> {
    scenery
        .dams
        .iter()
        .map(|dam| PreparedLake {
            mesh: lake_mesh(dam),
            mirror: prepare_mirror(
                track,
                terrain,
                scenery,
                textures,
                &lake_reach(dam),
                &|_| false,
                deep(scenery.water),
                threads,
            ),
        })
        .collect()
}

/// How far out the downstream face stands past the deck's edge, `h` metres
/// under the crest.
fn face_out(h: f32) -> f32 {
    FACE_LEAN * h + FACE_FLARE * h * h
}

/// The deck's edge at point `index` of the lap (m from the centre line):
/// just past the walls, which close in where the road over the crest is
/// narrow.
pub(super) fn edge(track: &Track, index: usize) -> f32 {
    track.wall_at(index) + EDGE_PAST_WALL
}

/// How far down the dam's downstream face goes at point `index` of the lap
/// (m under the crest): until it meets the ground.
fn face_depth(track: &Track, terrain: &Terrain, index: usize, downstream: f32) -> f32 {
    let edge = edge(track, index);
    let top = track.surface_point(index, downstream * edge).y;
    (0..150)
        .map(|h| h as f32)
        .find(|&h| {
            let at = track.offset_point(index, downstream * (edge + face_out(h)));
            terrain.height(at) >= top - h
        })
        .unwrap_or(150.0)
}

/// The colour of the concrete down the face at point `index` of the lap,
/// `t` of the way down (0 at the crest, 1 at the foot): each strip a shade
/// of its own, some streaked darker with the damp running down from the
/// crest, a few rusty, the foot darker with the damp.
fn streaked(index: usize, t: f32) -> Color {
    let seed = index as u32;
    let tone = 0.86 + 0.14 * random01(seed + 41_000);
    let damp = smoothstep(0.35, 1.0, random01(seed / 2 + 42_000));
    let streak = damp * STREAK_DARK * (1.0 - t).powf(0.6);
    let foot = 0.12 * smoothstep(0.8, 1.0, t);
    let shade = tone * (1.0 - streak - foot);
    let rust = smoothstep(0.82, 0.95, random01(seed / 3 + 43_000)) * (1.0 - t) * 0.8;
    Color::srgb(shade, shade, shade).mix(&(RUST.to_linear() * shade).into(), rust)
}

/// The concrete of a dam: its crest out to the parapets either side of the
/// asphalt, jutting out over the downstream face on brackets; its upstream
/// face, straight down into the lake; and its downstream face, nearly sheer
/// under the crest and leaning out towards its foot in the river, streaked
/// by the damp. Following the crest, which is an arch, the face is curved
/// as an arch dam's.
pub(super) fn dam_mesh(track: &Track, terrain: &Terrain, dam: &Dam) -> Mesh {
    let lap = track.lap_length();
    let indices = indices(
        track,
        span(dam.crest.from - ABUTMENT, dam.crest.to + ABUTMENT),
        lap,
    );
    let upstream = dam.lake_side.sign();
    let downstream = -upstream;
    // From under the asphalt's edge to the deck's, which close in with the
    // walls where the road over the crest is narrow.
    let inner = |i: usize| track.half_width_at(i) - 0.3;
    let edge = |i: usize| edge(track, i);
    let step = track.step_length();
    let mut mesh = MeshBuilder::default();
    let depths: Vec<f32> = indices
        .iter()
        .map(|&i| face_depth(track, terrain, i, downstream))
        .collect();
    let road = |i: usize, lateral: f32| track.surface_point(i, lateral);
    let out = |i: usize, side: f32| {
        let across = track.offset_point(i, side) - track.offset_point(i, 0.0);
        Vec3::new(across.x, 0.0, across.y)
    };
    // A point of the downstream side, `lateral` metres out past the deck's
    // edge and `h` under the crest.
    let below = |i: usize, lateral: f32, h: f32| {
        let at = track.offset_point(i, downstream * (edge(i) + lateral));
        Vec3::new(at.x, road(i, downstream * edge(i)).y - h, at.y)
    };
    for (k, pair) in indices.windows(2).enumerate() {
        let (i, j) = (pair[0], pair[1]);
        let (u0, u1) = (
            k as f32 * step / CONCRETE_TILE,
            (k + 1) as f32 * step / CONCRETE_TILE,
        );
        let top_color = Color::srgb(0.95, 0.94, 0.92);
        // The crest beside the asphalt, out to the parapets, and on the
        // downstream side out over the face.
        for (side, past) in [(upstream, 0.0), (downstream, CORBEL)] {
            let across = (edge(i) + past - inner(i)) / CONCRETE_TILE;
            mesh.quad(
                [
                    (road(i, side * inner(i)), Vec2::new(0.0, u0)),
                    (road(j, side * inner(j)), Vec2::new(0.0, u1)),
                    (road(j, side * (edge(j) + past)), Vec2::new(across, u1)),
                    (road(i, side * (edge(i) + past)), Vec2::new(across, u0)),
                ],
                Vec3::Y,
                top_color,
            );
        }
        // The upstream face, straight down into the lake.
        let face_bottom = |i: usize| {
            let at = track.offset_point(i, upstream * edge(i));
            terrain.height(at).min(dam.level - 2.0) - FOOTING
        };
        let (top_i, top_j) = (road(i, upstream * edge(i)), road(j, upstream * edge(j)));
        let (bottom_i, bottom_j) = (top_i.with_y(face_bottom(i)), top_j.with_y(face_bottom(j)));
        mesh.quad(
            [
                (top_i, Vec2::new(u0, 0.0)),
                (top_j, Vec2::new(u1, 0.0)),
                (
                    bottom_j,
                    Vec2::new(u1, (top_j.y - bottom_j.y) / CONCRETE_TILE),
                ),
                (
                    bottom_i,
                    Vec2::new(u0, (top_i.y - bottom_i.y) / CONCRETE_TILE),
                ),
            ],
            out(i, upstream),
            top_color,
        );
        // The edge of the jutting crest, and its underside back to the face.
        let lip = |i: usize, h: f32| below(i, CORBEL, h);
        mesh.quad(
            [
                (lip(i, 0.0), Vec2::new(u0, 0.0)),
                (lip(j, 0.0), Vec2::new(u1, 0.0)),
                (lip(j, CORBEL_DEPTH), Vec2::new(u1, 0.5)),
                (lip(i, CORBEL_DEPTH), Vec2::new(u0, 0.5)),
            ],
            out(i, downstream),
            top_color,
        );
        let under = |i: usize| below(i, face_out(CORBEL_DEPTH), CORBEL_DEPTH);
        mesh.quad(
            [
                (lip(i, CORBEL_DEPTH), Vec2::new(u0, 0.0)),
                (lip(j, CORBEL_DEPTH), Vec2::new(u1, 0.0)),
                (under(j), Vec2::new(u1, 0.3)),
                (under(i), Vec2::new(u0, 0.3)),
            ],
            Vec3::NEG_Y,
            streaked(i, 0.05),
        );
        // A bracket under it, from the face up to its edge.
        if k % BRACKET_EVERY == 0 {
            let h = CORBEL_DEPTH + 1.8;
            mesh.beam(
                below(i, face_out(h), h),
                below(i, CORBEL - 0.3, CORBEL_DEPTH),
                0.45,
                0.45,
                streaked(i, 0.1),
            );
        }
        // The downstream face, from under the crest down to its foot in
        // the ground.
        let (depth_i, depth_j) = (depths[k], depths[k + 1]);
        if depth_i <= CORBEL_DEPTH && depth_j <= CORBEL_DEPTH {
            continue;
        }
        let face = |i: usize, depth: f32, t: f32| {
            let h = CORBEL_DEPTH.lerp(depth.max(CORBEL_DEPTH) + FOOTING, t);
            below(i, face_out(h), h)
        };
        let v = |p: Vec3, i: usize| (road(i, downstream * edge(i)).y - p.y) / CONCRETE_TILE;
        for row in 0..FACE_ROWS {
            let (t0, t1) = (
                row as f32 / FACE_ROWS as f32,
                (row + 1) as f32 / FACE_ROWS as f32,
            );
            let corners = [
                face(i, depth_i, t0),
                face(j, depth_j, t0),
                face(j, depth_j, t1),
                face(i, depth_i, t1),
            ];
            mesh.quad(
                [
                    (corners[0], Vec2::new(u0, v(corners[0], i))),
                    (corners[1], Vec2::new(u1, v(corners[1], j))),
                    (corners[2], Vec2::new(u1, v(corners[2], j))),
                    (corners[3], Vec2::new(u0, v(corners[3], i))),
                ],
                out(i, downstream) + Vec3::Y * 0.2,
                streaked(i, (t0 + t1) / 2.0),
            );
        }
    }
    mesh.build()
}

/// The lake's water: level, a fan from the middle of its shore out to a
/// little past it, under the ground.
fn lake_mesh(dam: &Dam) -> Mesh {
    let shore = closed_curve(dam.lake, SHORE_SPACING);
    let middle = shore.iter().copied().sum::<Vec2>() / shore.len() as f32;
    let mut mesh = MeshBuilder::default();
    let at = |p: Vec2| {
        let p = p + (p - middle).normalize_or_zero() * LAKE_WATER_PAST;
        (Vec3::new(p.x, dam.level, p.y), p / 8.0)
    };
    let centre = (Vec3::new(middle.x, dam.level, middle.y), middle / 8.0);
    for k in 0..shore.len() {
        let (a, b) = (shore[k], shore[(k + 1) % shore.len()]);
        mesh.triangle([centre, at(a), at(b)], Vec3::Y, Color::WHITE);
    }
    mesh.build()
}

/// The paved square at `at`, on the level ground there, and its monument in
/// the middle: two slender slabs side by side, one taller, on a low plinth.
fn square_mesh(terrain: &Terrain, at: Vec2) -> Mesh {
    const SIDES: usize = 24;
    let ground = terrain.height(at) + 0.05;
    let mut mesh = MeshBuilder::default();
    let centre = (Vec3::new(at.x, ground, at.y), Vec2::ZERO);
    let corner = |k: usize| {
        let angle = k as f32 / SIDES as f32 * std::f32::consts::TAU;
        let p = at + Vec2::from_angle(angle) * SQUARE_RADIUS;
        let y = ground.max(terrain.height(p) + 0.05);
        (Vec3::new(p.x, y, p.y), (p - at) / CONCRETE_TILE)
    };
    for k in 0..SIDES {
        mesh.triangle(
            [centre, corner(k), corner(k + 1)],
            Vec3::Y,
            Color::srgb(0.9, 0.89, 0.86),
        );
    }
    let plinth = Vec3::new(at.x, ground, at.y);
    mesh.beam(plinth, plinth + Vec3::Y * 0.6, 6.0, 4.0, Color::WHITE);
    for (offset, height, width) in [
        (-0.9, MONUMENT_HEIGHT, 1.2),
        (0.9, 0.82 * MONUMENT_HEIGHT, 1.0),
    ] {
        let foot = plinth + Vec3::new(offset, 0.6, 0.0);
        mesh.beam(foot, foot + Vec3::Y * height, width, 0.7, Color::WHITE);
    }
    mesh.build()
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;
    use crate::track::WALL_DISTANCE;
    use crate::world::mirror::surroundings_map;
    use crate::world::mirror::tests::mapped;
    use crate::world::road::PARAPET_HEIGHT;
    use crate::world::scenery::BridgeStyle;

    /// The circuit with a dam: its track, its terrain and its dam.
    pub fn with_a_dam(
        circuits: &'static crate::circuits::Circuits,
    ) -> (Track, Terrain, &'static Dam) {
        let circuit = &circuits[1];
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        (track, terrain, &circuit.scenery.dams[0])
    }

    pub fn every_dam_s_crest_is_a_bridge_of_its_own_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut dams = 0;
        for circuit in circuits.iter() {
            let scenery = &circuit.scenery;
            for dam in scenery.dams {
                assert!(
                    scenery
                        .bridges
                        .iter()
                        .any(|bridge| bridge.style == BridgeStyle::Dam && bridge.span == dam.crest),
                    "{}",
                    circuit.name
                );
                dams += 1;
            }
            for bridge in scenery.bridges {
                if bridge.style == BridgeStyle::Dam {
                    assert!(
                        scenery.dams.iter().any(|dam| dam.crest == bridge.span),
                        "{}",
                        circuit.name
                    );
                }
            }
        }
        assert!(dams > 0, "some circuit has a dam");
    }

    pub fn a_lake_s_water_is_a_fan_that_never_folds_over_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            for dam in circuit.scenery.dams {
                let shore = closed_curve(dam.lake, SHORE_SPACING);
                let middle = shore.iter().copied().sum::<Vec2>() / shore.len() as f32;
                let turns: Vec<f32> = (0..shore.len())
                    .map(|k| (shore[k] - middle).perp_dot(shore[(k + 1) % shore.len()] - middle))
                    .collect();
                // Every slice of it turns the same way round its middle.
                assert!(
                    turns.iter().all(|&turn| turn > 0.0) || turns.iter().all(|&turn| turn < 0.0),
                    "{}",
                    circuit.name
                );
            }
        }
    }

    #[test]
    fn a_lake_s_water_is_a_fan_that_never_folds_over() {
        a_lake_s_water_is_a_fan_that_never_folds_over_on(&crate::testing::DEMO);
    }

    pub fn a_dam_s_crest_bows_towards_its_lake_on(circuits: &'static crate::circuits::Circuits) {
        // As an arch dam: over the crest the road bends away from the lake,
        // all the way along it, so the arch's middle stands out towards the
        // water it holds.
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            for dam in circuit.scenery.dams {
                let crest = indices(&track, dam.crest, track.lap_length());
                let towards = |i: usize| track.curvature(i) * dam.lake_side.sign();
                let turn = crest.iter().map(|&i| towards(i)).sum::<f32>() * track.step_length();
                assert!(turn < -0.8, "{}: {turn} rad round the lake", circuit.name);
                for window in crest[3..crest.len() - 3].windows(7) {
                    let bend = window.iter().map(|&i| towards(i)).sum::<f32>();
                    assert!(bend < 0.0, "{}: round the lake", circuit.name);
                }
            }
        }
    }

    #[test]
    fn a_dam_s_crest_bows_towards_its_lake() {
        a_dam_s_crest_bows_towards_its_lake_on(&crate::testing::DEMO);
    }

    pub fn the_road_comes_down_onto_a_dam_straight_at_it_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // The crest is not the top of the climb: the hairpin before it is
        // well away and higher up, and the road comes down the hillside
        // from it straight at the dam.
        let (track, _, dam) = with_a_dam(circuits);
        let height = |along: f32| track.elevations()[track.index_at(along)];
        let crest = height(dam.crest.from);
        let top = (1..=300)
            .map(|back| height(dam.crest.from - back as f32))
            .fold(f32::MIN, f32::max);
        assert!(top > crest + 6.0, "{top} m before a crest at {crest} m");
        for back in (2..=150).step_by(2) {
            let along = dam.crest.from - back as f32;
            assert!(
                height(along) > height(along + 2.0) - 0.05,
                "up at {along} m"
            );
        }
        for back in (20..=220).step_by(2) {
            let index = track.index_at(dam.crest.from - back as f32);
            let radius = 1.0 / track.smoothed_curvature(index, 6).max(1e-6);
            assert!(
                radius > 400.0,
                "a bend of {radius} m, {back} m before the dam"
            );
        }
    }

    pub fn a_dam_s_lake_shows_coming_down_to_the_dam_and_over_its_parapet_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (track, terrain, dam) = with_a_dam(circuits);
        let shore = closed_curve(dam.lake, SHORE_SPACING);
        let crest = indices(&track, dam.crest, track.lap_length());
        let side = dam.lake_side.sign();
        // What stands under a line of sight: along the crest, its deck out
        // to its edges and the low parapet on the lake's side; elsewhere
        // the ground (the railing on the way down is seen through).
        let under = |p: Vec2| {
            let nearest = track.nearest(p);
            if !crest.contains(&nearest.index) || nearest.distance > edge(&track, nearest.index) {
                return terrain.height(p);
            }
            let out = nearest.lateral * side;
            let wall = track.wall_at(nearest.index);
            let parapet = (wall..wall + 0.6).contains(&out);
            track.height_beside(&nearest) + if parapet { PARAPET_HEIGHT } else { 0.0 }
        };
        // Coming down to the dam and along the first half of the crest, the
        // water is in sight ahead on the lake's side, well within the view.
        let (reach, angle) = (90.0, 40f32.to_radians());
        for back in (-70..=80).step_by(10) {
            let along = dam.crest.from - back as f32;
            let i = track.index_at(along);
            let ahead = (track.centerline()[i + 1] - track.centerline()[i]).normalize();
            let aside = (track.offset_point(i, side) - track.centerline()[i]).normalize();
            let target =
                track.centerline()[i] + (ahead * angle.cos() + aside * angle.sin()) * reach;
            assert!(
                signed_distance(&shore, target) > 4.0,
                "from {along} m, {target} is not out on the lake"
            );
            let eye = track.surface_point(i, 0.0) + Vec3::Y * 1.5;
            let water = Vec3::new(target.x, dam.level, target.y);
            for k in 1..90 {
                let at = eye.lerp(water, k as f32 / 90.0);
                assert!(
                    under(Vec2::new(at.x, at.z)) < at.y,
                    "from {along} m, the water is hidden at {at}"
                );
            }
        }
    }

    pub fn a_dam_s_lake_mirrors_the_hills_round_it_and_the_dam_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (track, terrain, dam) = with_a_dam(circuits);
        let grass = Color::srgb(0.4, 0.55, 0.2).to_linear();
        let scenery = &circuits[1].scenery;
        let (corner, size, highest, image) = surroundings_map(
            &track,
            &terrain,
            scenery,
            &lake_reach(dam),
            &|_| false,
            Vec3::Y,
            grass,
            4,
        );
        let width = image.width() as usize;
        assert_eq!(size.x, width as f32 * MIRROR_TEXEL);
        let at = |p: Vec2| mapped(&image, corner, MIRROR_TEXEL, p);
        // Out on the lake: the water, or under it.
        let shore = closed_curve(dam.lake, SHORE_SPACING);
        let middle = shore.iter().copied().sum::<Vec2>() / shore.len() as f32;
        assert!(at(middle)[3] < 0.0);
        // The dam's crest stands up over the water to the road, pale.
        let crest = indices(&track, dam.crest, track.lap_length());
        let i = crest[crest.len() / 2];
        let top = at(track.centerline()[i]);
        let over = track.elevations()[i] - dam.level;
        assert!(
            (top[3] - over).abs() < 0.3,
            "{} m over, not {over} m",
            top[3]
        );
        assert!(top[0] > 0.3 && top[1] > 0.3 && top[2] > 0.3);
        // The hills round it stand up over the water, in colour, much of the
        // way round (the gorge below the dam, past it, lies low).
        let hills = shore
            .iter()
            .filter(|&&p| {
                let out = at(p + (p - middle).normalize() * 70.0);
                out[3] > 3.0 && out[0] + out[1] + out[2] > 0.05
            })
            .count();
        assert!(hills > shore.len() / 3, "{hills} of {}", shore.len());
        // The highest of it all, for a line of sight to give up over.
        assert!(highest >= top[3] && highest < 400.0, "{highest}");
    }

    pub fn a_dam_holds_its_lake_a_few_metres_under_the_crest_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (track, terrain, dam) = with_a_dam(circuits);
        let lap = track.lap_length();
        let crest: Vec<usize> = indices(&track, dam.crest, lap);
        for &i in &crest {
            let under = track.elevations()[i] - dam.level;
            assert!(
                (2.0..5.0).contains(&under),
                "the water {under} m under the crest"
            );
        }
        // Under the water, the lake's bed; round its shore the ground holds
        // the water in, but along the dam, which holds it there.
        let shore = closed_curve(dam.lake, SHORE_SPACING);
        let middle = shore.iter().copied().sum::<Vec2>() / shore.len() as f32;
        assert!(terrain.height(middle) < dam.level - 5.0);
        let by_the_dam = |p: Vec2| {
            crest
                .iter()
                .any(|&i| track.centerline()[i].distance(p) < WALL_DISTANCE + 8.0)
        };
        let mut held = 0;
        for &p in &shore {
            let (inside, outside) = (
                p + (middle - p).normalize() * 4.0,
                p + (p - middle).normalize() * 4.0,
            );
            if by_the_dam(p) {
                continue;
            }
            assert!(terrain.height(inside) < dam.level, "{inside}");
            assert!(terrain.height(outside) > dam.level, "{outside}");
            held += 1;
        }
        assert!(held > shore.len() / 2);
    }

    pub fn the_sun_shines_on_a_dam_s_face_all_across_it_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (track, _, dam) = with_a_dam(circuits);
        let sun = crate::world::sky::Palette::of(circuits[1].scenery.sky).sun_direction();
        let sun = Vec2::new(sun.x, sun.z).normalize();
        let downstream = -dam.lake_side.sign();
        for i in indices(&track, dam.crest, track.lap_length()) {
            // Which way the face looks, out over the gorge.
            let facing =
                (track.offset_point(i, downstream) - track.offset_point(i, 0.0)).normalize();
            assert!(facing.dot(sun) > 0.3, "{} at {i}", facing.dot(sun));
        }
    }

    pub fn a_dam_s_river_runs_from_its_foot_down_the_gorge_to_the_sea_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (_, terrain, dam) = with_a_dam(circuits);
        // From under the dam to the mouth, the sea's water.
        for pair in dam.river.windows(2) {
            let ((a, _), (b, _)) = (pair[0], pair[1]);
            for k in 0..=10 {
                let at = a.lerp(b, k as f32 / 10.0);
                assert!(terrain.height(at) < SEA_LEVEL, "{at}");
            }
        }
        // Across it from the road, the gorge's far wall stands high over
        // the water near the dam.
        // (Down the river, past where the valley opens out at the dam.)
        let (a, wall) = dam.river[3];
        let b = dam.river[4].0;
        // Towards the road's bank: its left going downstream, if the road
        // is on the left.
        let downstream = (b - a).normalize();
        let across = Vec2::new(downstream.y, -downstream.x) * dam.road_bank.sign();
        let far = a - across * (RIVER_HALF_WIDTH + FAR_WALL_RISE + 10.0);
        assert!(terrain.height(far) > 0.6 * wall, "{}", terrain.height(far));
    }

    pub fn a_dam_s_face_drops_to_the_river_in_sight_of_the_road_up_the_gorge_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (track, terrain, dam) = with_a_dam(circuits);
        let lap = track.lap_length();
        let crest = indices(&track, dam.crest, lap);
        let middle = crest[crest.len() / 2];
        let downstream = -dam.lake_side.sign();
        // In the middle the face goes all the way down to the river...
        let depth = face_depth(&track, &terrain, middle, downstream);
        let crest_height = track.elevations()[middle];
        assert!(
            depth > crest_height - 2.0,
            "{depth} m down from {crest_height} m"
        );
        // ...and into the hillsides at the ends of the crest.
        let ends = indices(
            &track,
            span(dam.crest.from - ABUTMENT, dam.crest.to + ABUTMENT),
            lap,
        );
        for end in [ends[0], ends[ends.len() - 1]] {
            assert!(face_depth(&track, &terrain, end, downstream) < 0.4 * depth);
        }
        // From the road up the gorge, before it turns off below the dam,
        // the face is in sight half way down it.
        let h = depth / 2.0;
        let on_face = track.offset_point(middle, downstream * (edge(&track, middle) + face_out(h)));
        let on_face = Vec3::new(on_face.x, crest_height - h, on_face.y);
        for along in [2950.0, 2980.0, 3010.0] {
            let i = track.index_at(along);
            let eye = track.surface_point(i, 0.0) + Vec3::Y * 1.5;
            for k in 1..50 {
                let at = eye.lerp(on_face, k as f32 / 50.0);
                assert!(
                    terrain.height(Vec2::new(at.x, at.z)) < at.y,
                    "from {along} m the ground hides the dam at {at}"
                );
            }
        }
    }
}
