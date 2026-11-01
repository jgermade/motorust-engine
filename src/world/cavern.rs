//! An excavation, a mine's or great halls' (see [`super::scenery::Hall`]):
//! galleries and halls hewn through the rock, with high, rough roofs, pillars of rock left
//! between the roads where a branch takes another way through (see
//! [`crate::track::Fork`]), and stone of several kinds along it (see
//! [`Stone`]), lit by lanterns.
//!
//! The rock is worked out on a grid over the ground. Inside is the space
//! between the walls of the roads, the ones the bikes run into (see
//! [`Track::edge_beside`]), along the stretch of the lap the excavation
//! takes and the branches that stand in for it. Its walls stand straight up
//! from the edge of that space to above a rider's head and lean out a
//! little higher; from their tops the roof rises in a rough arch to the
//! crown of each hall, highest over the middle of each road, with
//! stalactites hanging from it. Where two roads part, the roofs over them
//! come down to the rock left between them. Over all of it lies the outside
//! of the rock: the top of the mesa it is cut into, and its faces round the
//! two mouths, framed with timber.
//!
//! Great halls are dug alike (see [`super::scenery::Scenery::halls`]),
//! but some of masonry and some natural caves, and wider than the
//! road: it runs through them between low walls of their stone, under
//! roofs far above (see [`Hall::width`]), past rows of great pillars.

use std::collections::HashMap;

use bevy::prelude::*;

use super::quads::MeshBuilder;
use super::scenery::{Hall, Soil, Span, Stone};
use super::structures::TUNNEL_SHELL;
use super::terrain::Terrain;
use super::textures::WorldTextures;
use crate::geometry::{fbm, random01, smoothstep};
use crate::track::{CURB_WIDTH, Nearest, SURFACE_Y, Track, WALL_DISTANCE};

/// Side of the squares of the grid the rock is worked out on (m).
const SPACING: f32 = 1.5;
/// How far below the road the walls go (m).
const FOOT: f32 = 1.5;
/// How high the walls stand straight up (m above the road), about, and how
/// much that varies.
const WALL_TOP: f32 = 4.4;
const WALL_TOP_ROUGH: f32 = 1.2;
/// Above this height (m), the walls lean out, as far as this at their top
/// (m).
const LEAN_FROM: f32 = 3.0;
const LEAN: f32 = 1.1;
/// The crown of a timbered gallery, above the middle of the road (m).
const GALLERY_CROWN: f32 = 10.5;
/// How far the roof rises and falls in domes and hollows (m), in a gallery
/// and in a hall; its smaller lumps are half that.
const GALLERY_ROUGH: f32 = 0.7;
const HALL_ROUGH: f32 = 4.0;
/// Halls blend into each other, and into the galleries, over this far
/// either side of where one ends and the next begins (m).
const HALL_FADE: f32 = 14.0;
/// The rock reaches this far from the centre line of each road (m): past
/// where the ground rises to the mesa round it (see `terrain`).
const SHELL: f32 = TUNNEL_SHELL;
/// The outside of the rock stands at least this far above the roof (m).
const ROOF_ABOVE: f32 = 3.0;
/// Where another stretch of the lap passes over the excavation, the rock
/// carries it level with its road this far from its centre line, as flat as
/// the ground beside a road (m), and comes back to its own shape this much
/// further away (m).
const OVER_FLAT: f32 = WALL_DISTANCE + 1.5;
const OVER_BLEND: f32 = 20.0;
/// How far from the lap through the excavation other stretches of it are
/// looked for to pass over it (m): past the rock over the roads off it.
const OVER_REACH: f32 = 120.0;
/// The floor beside the roads, above the ground (m): under the asphalt.
const FLOOR_LIFT: f32 = 0.02;
/// The lanterns in the halls: on a post by a wall every this many points of
/// each road's centre line, from one side to the other, hanging this high
/// (m), with light that falls off over this far (m).
const LANTERN_EVERY: usize = 7;
const LANTERN_HEIGHT: f32 = 3.0;
/// Some hang higher, on tall poles: this high at most (m).
const HIGH_LANTERN: f32 = 10.0;
const LANTERN_REACH: f32 = 7.5;
/// In the galleries, a timber set every this many points of the lap, and a
/// lantern hanging from either rafter of every other one, this far from the
/// middle (m).
const SET_EVERY: usize = 4;
const SET_LANTERN: f32 = 6.0;
/// Stalactites in the halls: one in about this many squares of this side
/// (m), and how long at most (m). Their tips stay this high above the road
/// (m).
const STALACTITE_CELL: f32 = 6.0;
const STALACTITE_CHANCE: f32 = 0.45;
const STALACTITE_LENGTH: f32 = 5.0;
const STALACTITE_CLEAR: f32 = 9.0;
/// Veins of gold in the slate: seeds in squares of this side (m), how many
/// steps of this length (m) each runs, and how wide (m).
const VEIN_CELL: f32 = 9.0;
const VEIN_STEPS: usize = 18;
const VEIN_STEP: f32 = 1.1;
const VEIN_WIDTH: f32 = 0.22;
/// What each repetition of the textures covers (m): the rock, the layers of
/// sandstone (down the walls) and the planks.
const ROCK_TILE: f32 = 6.0;
const STRATA_TILE: f32 = 16.0;
const PLANK_TILE: f32 = 4.0;
/// The planks laid on the branches, above their asphalt (m), where it is
/// clear of the lap's.
const PLANK_LIFT: f32 = 0.035;
/// Masonry's blocks repeat every this many metres.
const MASONRY_TILE: f32 = 5.0;
/// Where a hall is wider than the road, the low walls either side of it:
/// how high over the road (m) and how thick (m), just past the walls the
/// bikes run into (m).
const PARAPET_HEIGHT: f32 = 1.2;
const PARAPET_THICK: f32 = 0.7;
const PARAPET_OUT: f32 = 0.1;
/// In the halls of masonry at least this much wider than the road (m), a
/// row of square pillars either side, half way out to the walls, every this
/// many metres along, this thick (m).
const PILLARS_FROM: f32 = 8.0;
const PILLAR_EVERY: f32 = 28.0;
const PILLAR_THICK: f32 = 4.0;

/// The meshes of the excavation along `span` of the lap, with their
/// materials, through rock of the kinds `halls` says, and where its
/// lanterns hang.
pub fn cavern_pieces(
    track: &Track,
    terrain: &Terrain,
    span: Span,
    halls: &[Hall],
    soil: Soil,
    textures: &WorldTextures,
) -> (Vec<(Mesh, StandardMaterial)>, Vec<Vec3>) {
    let cave = Cave::new(track, span, halls);
    let mut parts = Parts::default();
    let lanterns = cave.timber_and_lanterns(&mut parts);
    let painter = Painter {
        lanterns: &lanterns,
    };
    cave.carve(&painter, &mut parts);
    cave.parapets_and_pillars(&painter, &mut parts);
    cave.stalactites(&painter, &mut parts);
    cave.veins(&mut parts);
    cave.planked_branches(&painter, &mut parts);
    cave.outside(terrain, &mut parts);
    (parts.pieces(soil, textures), lanterns)
}

/// The meshes of the excavation, each for its own material.
#[derive(Default)]
struct Parts {
    /// The walls and the roof, by stone (see [`Stone::index`]).
    rock: [MeshBuilder; 6],
    /// The floor beside the roads, by stone.
    floor: [MeshBuilder; 6],
    /// The outside of the rock.
    outside: MeshBuilder,
    /// Timber: sets, posts, frames, and the planks that line walls and
    /// branches.
    timber: MeshBuilder,
    planks: MeshBuilder,
    lanterns: MeshBuilder,
    /// Wire and iron.
    iron: MeshBuilder,
    gold: MeshBuilder,
}

impl Parts {
    fn pieces(self, soil: Soil, textures: &WorldTextures) -> Vec<(Mesh, StandardMaterial)> {
        let mut pieces = Vec::new();
        for (stone, (rock, floor)) in Stone::ALL
            .into_iter()
            .zip(self.rock.into_iter().zip(self.floor))
        {
            pieces.push((rock, rock_material(stone, textures)));
            pieces.push((floor, floor_material(stone, textures)));
        }
        let timber = StandardMaterial {
            base_color: Color::srgb(0.72, 0.56, 0.42),
            base_color_texture: Some(textures.planks.clone()),
            perceptual_roughness: 0.9,
            ..default()
        };
        pieces.extend([
            (
                self.outside,
                // The rock of the land round it: a desert's red, or granite.
                match soil {
                    Soil::Granite => StandardMaterial {
                        base_color: Color::srgb(0.8, 0.8, 0.82),
                        base_color_texture: Some(textures.granite.clone()),
                        perceptual_roughness: 0.9,
                        ..default()
                    },
                    _ => StandardMaterial {
                        base_color: Color::srgb(0.95, 0.62, 0.46),
                        base_color_texture: Some(textures.rock.clone()),
                        perceptual_roughness: 0.9,
                        ..default()
                    },
                },
            ),
            (self.timber, timber.clone()),
            (
                self.planks,
                StandardMaterial {
                    base_color: Color::srgb(0.85, 0.7, 0.55),
                    ..timber
                },
            ),
            // Oil lanterns: orange.
            (
                self.lanterns,
                StandardMaterial {
                    base_color: Color::srgb(1.0, 0.75, 0.4),
                    emissive: LinearRgba::rgb(9.0, 4.5, 1.2),
                    ..default()
                },
            ),
            (
                self.iron,
                StandardMaterial {
                    base_color: Color::srgb(0.2, 0.2, 0.2),
                    perceptual_roughness: 0.6,
                    ..default()
                },
            ),
            // Gold that glints in the dark.
            (
                self.gold,
                StandardMaterial {
                    base_color: Color::srgb(1.0, 0.8, 0.3),
                    emissive: LinearRgba::rgb(1.6, 1.0, 0.2),
                    metallic: 0.8,
                    perceptual_roughness: 0.3,
                    ..default()
                },
            ),
        ]);
        pieces
            .into_iter()
            .filter(|(mesh, _)| !mesh.is_empty())
            .map(|(mesh, material)| (mesh.build(), material))
            .collect()
    }
}

impl Stone {
    const ALL: [Stone; 6] = [
        Stone::Timbered,
        Stone::Sandstone,
        Stone::Slate,
        Stone::Clay,
        Stone::Masonry,
        Stone::Cave,
    ];

    fn index(self) -> usize {
        match self {
            Stone::Timbered => 0,
            Stone::Sandstone => 1,
            Stone::Slate => 2,
            Stone::Clay => 3,
            Stone::Masonry => 4,
            Stone::Cave => 5,
        }
    }

    /// Whether it is natural rock: stalactites hang from it.
    fn natural(self) -> bool {
        !matches!(self, Stone::Timbered | Stone::Masonry)
    }

    /// Texture coordinates of point `p` of a face of its rock whose normal
    /// is `normal`: seen from above or below, laid over the ground; else
    /// round it, going down. Sandstone shows its layers.
    fn uv(self, p: Vec3, normal: Vec3) -> Vec2 {
        let across = Vec2::new(-normal.z, normal.x).normalize_or_zero();
        let round = p.x * across.x + p.z * across.y;
        match self {
            Stone::Sandstone => {
                let round = if normal.y.abs() > 0.7 {
                    p.x + p.z
                } else {
                    round
                };
                Vec2::new(round / (2.0 * ROCK_TILE), -p.y / STRATA_TILE)
            }
            // Blocks laid in courses, up the walls and over the vaults.
            Stone::Masonry if normal.y.abs() > 0.7 => Vec2::new(p.x, p.z) / MASONRY_TILE,
            Stone::Masonry => Vec2::new(round, -p.y) / MASONRY_TILE,
            _ if normal.y.abs() > 0.7 => Vec2::new(p.x, p.z) / ROCK_TILE,
            _ => Vec2::new(round, -p.y) / ROCK_TILE,
        }
    }
}

/// The walls and roof of each stone. They glow with no light of their own:
/// only the lanterns light them (see [`Painter`]).
fn rock_material(stone: Stone, textures: &WorldTextures) -> StandardMaterial {
    let (base, texture, roughness) = match stone {
        // Hewn rock, warm in the lanterns' light.
        Stone::Timbered => (Color::srgb(0.95, 0.66, 0.5), &textures.rock, 0.95),
        Stone::Sandstone => (Color::srgb(1.0, 0.9, 0.82), &textures.strata, 0.95),
        Stone::Slate => (Color::srgb(0.5, 0.52, 0.6), &textures.rock, 0.85),
        Stone::Clay => (Color::srgb(0.95, 0.7, 0.4), &textures.rock, 1.0),
        // Dressed grey blocks; and grey granite, as the mountain's.
        Stone::Masonry => (Color::srgb(0.82, 0.82, 0.86), &textures.wall, 0.9),
        Stone::Cave => (Color::srgb(0.9, 0.9, 0.94), &textures.granite, 0.9),
    };
    StandardMaterial {
        base_color: base,
        base_color_texture: Some(texture.clone()),
        perceptual_roughness: roughness,
        reflectance: 0.1,
        ..default()
    }
}

/// The stone of a mine's excavation at `point`, `along` a lap of `lap`
/// metres: that of the hall it is in, where one ends and the next begins
/// wavering, or a timbered gallery outside the halls.
pub fn stone_at(halls: &[Hall], lap: f32, point: Vec2, along: f32) -> Stone {
    let along = along + 30.0 * (fbm(point / 13.0, 2) - 0.5);
    halls
        .iter()
        .find(|hall| hall.span.contains(along, lap))
        .map_or(Stone::Timbered, |hall| hall.stone)
}

/// How much wider than the road an excavation, a mine's or great halls', is `along`
/// a lap of `lap` metres, either side (m): as wide as each of its halls,
/// blending from one to the next as their roofs do (see [`Hall::width`]).
pub fn excavation_width(halls: &[Hall], lap: f32, along: f32) -> f32 {
    let signed = |from: f32, to: f32| (to - from + lap / 2.0).rem_euclid(lap) - lap / 2.0;
    halls
        .iter()
        .map(|hall| {
            let weight = smoothstep(-HALL_FADE, HALL_FADE, signed(hall.span.from, along))
                * smoothstep(-HALL_FADE, HALL_FADE, signed(along, hall.span.to));
            weight * hall.width
        })
        .sum()
}

/// The floor beside the roads of each stone: planks in the galleries,
/// gravel and earth in the halls.
fn floor_material(stone: Stone, textures: &WorldTextures) -> StandardMaterial {
    let (base, texture) = match stone {
        Stone::Timbered => (Color::srgb(0.72, 0.56, 0.42), &textures.planks),
        Stone::Sandstone => (Color::srgb(0.75, 0.42, 0.3), &textures.rock),
        Stone::Slate => (Color::srgb(0.22, 0.22, 0.25), &textures.rock),
        Stone::Clay => (Color::srgb(0.7, 0.5, 0.3), &textures.sand),
        // Flagstones; and grey grit.
        Stone::Masonry => (Color::srgb(0.5, 0.5, 0.54), &textures.wall),
        Stone::Cave => (Color::srgb(0.42, 0.42, 0.45), &textures.granite),
    };
    StandardMaterial {
        base_color: base,
        base_color_texture: Some(texture.clone()),
        perceptual_roughness: 1.0,
        ..default()
    }
}

/// What the excavation is like at a point of the ground.
#[derive(Clone, Copy, Debug)]
struct Spot {
    /// The road nearest it (see [`Nearest::road`]).
    road: usize,
    /// Signed distance from the walls of that road (m): negative between
    /// them.
    walls: f32,
    /// Signed distance from the mouths, along the lap (m): negative inside.
    mouths: f32,
    /// How high the road is beside it.
    floor: f32,
    /// How far along the lap it is (m from the finish line).
    along: f32,
    /// Unit vector on the ground away from the centre line of that road.
    out: Vec2,
    /// How far the walls of the excavation are from that centre line (m):
    /// those of the road, and the hall's width past them.
    half: f32,
}

impl Spot {
    /// Signed distance from the edge of the space inside (m): negative
    /// inside.
    fn edge(&self) -> f32 {
        self.walls.max(self.mouths)
    }

    /// How far in from the walls (m).
    fn depth(&self) -> f32 {
        (-self.walls).max(0.0)
    }
}

/// The excavation through one stretch of the lap.
struct Cave<'a> {
    track: &'a Track,
    span: Span,
    lap: f32,
    halls: &'a [Hall],
    /// The points of the lap's centre line it looks among: from `first`,
    /// `count` of them.
    first: usize,
    count: usize,
    /// The roads off the lap that run through it (see [`Nearest::road`]).
    branches: Vec<usize>,
    /// The other stretches of the lap that come near it, any of which may
    /// pass over it: runs of segments of its centre line, the first and how
    /// many.
    others: Vec<(usize, usize)>,
}

/// Half a section of an arch from the springing (0) to the crown (1), `u`
/// of the way in: steep at first, level at the top.
fn arch(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    (1.0 - (1.0 - u) * (1.0 - u)).sqrt()
}

impl<'a> Cave<'a> {
    fn new(track: &'a Track, span: Span, halls: &'a [Hall]) -> Self {
        let lap = track.lap_length();
        // Some way past the mouths too, so that the walls end at them.
        const MARGIN: f32 = 40.0;
        let first = track.index_at(span.from - MARGIN);
        let count = ((span.length(lap) + 2.0 * MARGIN) / track.step_length()).round() as usize;
        let branches = (1..=track.side_roads().len())
            .filter(|&road| {
                let line = track.road(road);
                (0..line.sample_count()).any(|i| span.contains(line.lap_along_at(i), lap))
            })
            .collect();
        let n = track.sample_count();
        let near = |index: usize| {
            let point = track.centerline()[index];
            (0..=count)
                .step_by(4)
                .any(|k| track.centerline()[(first + k) % n].distance(point) < OVER_REACH)
        };
        let mut others: Vec<(usize, usize)> = Vec::new();
        for k in count + 1..n {
            let index = (first + k) % n;
            if !near(index) {
                continue;
            }
            match others.last_mut() {
                Some((from, length)) if (*from + *length) % n == index => *length += 1,
                _ => others.push((index, 1)),
            }
        }
        Self {
            track,
            span,
            lap,
            halls,
            first,
            count,
            branches,
            others,
        }
    }

    /// Where another stretch of the lap passes over the excavation near
    /// `point`, high over its roof: how far `point` is from its centre line
    /// (m) and the height of its road there.
    fn passing_over(&self, point: Vec2) -> Option<(f32, f32)> {
        let over = self
            .others
            .iter()
            .map(|&(from, count)| self.track.nearest_among(point, from, count))
            .min_by(|a, b| a.distance.total_cmp(&b.distance))?;
        let height = self.track.height_beside(&over);
        (over.distance < OVER_FLAT + OVER_BLEND && height > self.ceiling(point) + ROOF_ABOVE + 2.0)
            .then_some((over.distance, height))
    }

    /// The point of the roads through it nearest to `point`, as
    /// [`Track::nearest`] finds it.
    fn nearest(&self, point: Vec2) -> Nearest {
        let mut nearest = self.track.nearest_among(point, self.first, self.count);
        for &road in &self.branches {
            let line = self.track.road(road);
            let here = Nearest {
                road,
                ..line.nearest_among(point, 0, line.sample_count() - 1)
            };
            if here.distance < nearest.distance {
                nearest = here;
            }
        }
        nearest
    }

    /// The distance from `point` to the edge of the lap's asphalt and curbs
    /// (m): negative on them.
    fn off_lap_asphalt(&self, point: Vec2) -> f32 {
        let nearest = self.track.nearest_among(point, self.first, self.count);
        nearest.distance - self.track.half_width() - CURB_WIDTH
    }

    /// Signed distance (m) from `from` to `to` along the lap, the short way.
    fn signed(&self, from: f32, to: f32) -> f32 {
        let half = self.lap / 2.0;
        (to - from + half).rem_euclid(self.lap) - half
    }

    fn spot(&self, point: Vec2) -> Spot {
        let nearest = self.nearest(point);
        let line = self.track.road(nearest.road);
        let wall = line
            .wall_at(nearest.index)
            .lerp(line.wall_at(nearest.index + 1), nearest.t);
        let along = self.track.along(&nearest);
        let into = self
            .signed(self.span.from, along)
            .min(self.signed(along, self.span.to));
        let half = wall + self.width(along);
        Spot {
            road: nearest.road,
            walls: nearest.distance - half,
            mouths: -into,
            floor: self.track.height_beside(&nearest),
            along,
            out: (point - nearest.point).try_normalize().unwrap_or(Vec2::X),
            half,
        }
    }

    /// How much of `hall` there is `along` the lap (0..1).
    fn weight(&self, hall: &Hall, along: f32) -> f32 {
        smoothstep(-HALL_FADE, HALL_FADE, self.signed(hall.span.from, along))
            * smoothstep(-HALL_FADE, HALL_FADE, self.signed(along, hall.span.to))
    }

    /// How high the crown of the roof is `along` the lap (m above the road),
    /// and how much of a hall, rather than a gallery, it is there (0..1).
    fn crown(&self, along: f32) -> (f32, f32) {
        let (mut crown, mut weight) = (0.0, 0.0);
        for hall in self.halls {
            let w = self.weight(hall, along);
            crown += w * hall.crown;
            weight += w;
        }
        let hall = weight.min(1.0);
        (
            (crown + (1.0 - hall) * GALLERY_CROWN) / weight.max(1.0),
            hall,
        )
    }

    /// How much wider than the road the excavation is `along` the lap, either
    /// side (m; see [`Hall::width`]).
    fn width(&self, along: f32) -> f32 {
        excavation_width(self.halls, self.lap, along)
    }

    /// The stone at `point`, `along` the lap (see [`stone_at`]).
    fn stone(&self, point: Vec2, along: f32) -> Stone {
        stone_at(self.halls, self.lap, point, along)
    }

    /// How high the walls stand straight up at `point` of their foot (m
    /// above the road).
    fn wall_top(point: Vec2) -> f32 {
        WALL_TOP + WALL_TOP_ROUGH * (fbm(point / 5.0 + 3.0, 2) - 0.5)
    }

    /// How far the walls lean out at their top at `point` of their foot
    /// (m).
    fn lean(point: Vec2) -> f32 {
        LEAN * fbm(point / 4.0 + 9.0, 2)
    }

    /// How high the roof is above the road at `point` (m), found to be at
    /// `spot`: from the tops of the walls, in an arch over each road, up to
    /// the crown, rough. Outside, as high as the walls.
    fn roof(&self, point: Vec2, spot: &Spot) -> f32 {
        let wall = Self::wall_top(point);
        let (crown, hall) = self.crown(spot.along);
        let rough = GALLERY_ROUGH.lerp(HALL_ROUGH, hall);
        let crown = crown + rough * (2.0 * fbm(point / 21.0 + 5.0, 2) - 1.0);
        let lumps = 0.5 * rough * (2.0 * fbm(point / 6.0 + 11.0, 3) - 1.0)
            + 0.15 * rough * (2.0 * fbm(point / 1.7 + 2.0, 2) - 1.0);
        let rise = arch(spot.depth() / spot.half.max(1.0));
        wall + (crown - wall) * rise + lumps * rise
    }

    /// The height of the roof over `point` (or of the tops of the walls, if
    /// it is not inside).
    fn ceiling(&self, point: Vec2) -> f32 {
        let spot = self.spot(point);
        spot.floor + self.roof(point, &spot)
    }

    /// The timber sets that prop the galleries and the frames of the mouths,
    /// and the lanterns: on posts by the walls of the halls, hanging from
    /// the timber sets in the galleries. Where the lanterns hang, for their
    /// light.
    fn timber_and_lanterns(&self, parts: &mut Parts) -> Vec<Vec3> {
        let mut lanterns = Vec::new();
        let track = self.track;
        let n = track.sample_count();
        let step = track.step_length();
        let point = |index: usize, lateral: f32, height: f32| {
            let ground = track.offset_point(index, lateral);
            Vec3::new(ground.x, height, ground.y)
        };
        // A timber set the shape of the roof at point `index` of the lap,
        // `thick`, pushed by `push`: a post against each wall, a rafter
        // leaning in from each, and a cap across under the crown.
        let timber_set = |index: usize, thick: f32, push: Vec3, parts: &mut Parts| {
            let post = WALL_DISTANCE - 0.35;
            let ceiling = |lateral: f32| self.ceiling(track.offset_point(index, lateral));
            let mut points = Vec::new();
            for (lateral, below) in [
                (-post, 0.0),
                (-post, 0.4),
                (-3.6, 0.55),
                (3.6, 0.55),
                (post, 0.4),
                (post, 0.0),
            ] {
                let height = if below == 0.0 {
                    track.surface_point(index, lateral).y - 0.3
                } else {
                    ceiling(lateral) - below - thick / 2.0
                };
                points.push(point(index, lateral, height) + push);
            }
            for pair in points.windows(2) {
                parts
                    .timber
                    .beam(pair[0], pair[1], thick, thick, Color::WHITE);
            }
            [points[1], points[2], points[3], points[4]]
        };
        let (first, last) = (track.index_at(self.span.from), track.index_at(self.span.to));
        let ends = [(first, -1.0), (last, 1.0)];
        let count = (last + n - first) % n;
        for k in 0..=count {
            let index = (first + k) % n;
            let along = index as f32 * step;
            let in_a_hall = self
                .halls
                .iter()
                .map(|hall| self.weight(hall, along))
                .sum::<f32>();
            let from_mouths = self
                .signed(self.span.from, along)
                .min(self.signed(along, self.span.to));
            if !index.is_multiple_of(SET_EVERY) || in_a_hall > 0.03 || from_mouths < 6.0 {
                continue;
            }
            let [left, cap_left, cap_right, right] = timber_set(index, 0.42, Vec3::ZERO, parts);
            if !index.is_multiple_of(2 * SET_EVERY) {
                continue;
            }
            // A lantern hanging from either rafter.
            for (from, to) in [(left, cap_left), (right, cap_right)] {
                let reach = (WALL_DISTANCE - 0.35 - SET_LANTERN) / (WALL_DISTANCE - 0.35 - 3.6);
                let rafter = from.lerp(to, reach) - Vec3::Y * 0.25;
                parts
                    .iron
                    .beam(rafter - Vec3::Y * 0.6, rafter, 0.04, 0.04, Color::WHITE);
                parts.lanterns.beam(
                    rafter - Vec3::Y * 1.0,
                    rafter - Vec3::Y * 0.6,
                    0.3,
                    0.3,
                    Color::WHITE,
                );
                lanterns.push(rafter - Vec3::Y * 0.8);
            }
        }
        // A heavy timber set in each mouth, standing a little out of the
        // rock; but a mouth through masonry or natural rock is a gate of its
        // own.
        for (index, outward) in ends {
            let along = index as f32 * step;
            if matches!(
                self.stone(track.centerline()[index], along),
                Stone::Masonry | Stone::Cave
            ) {
                continue;
            }
            let (_, heading) = track.pose_at(index);
            let ahead = crate::geometry::forward(heading);
            let push = Vec3::new(ahead.x, 0.0, ahead.y) * outward * 0.6;
            timber_set(index, 0.8, push, parts);
        }
        // In the halls, a lantern on a post by a wall every so often along
        // each road, one side and then the other, where that wall is the
        // edge of the rock.
        for road in std::iter::once(0).chain(self.branches.iter().copied()) {
            let line = track.road(road);
            for index in (0..line.sample_count()).step_by(LANTERN_EVERY) {
                let along = line.lap_along_at(index);
                let ground = line.offset_point(index, 0.0);
                if !self.span.contains(along, self.lap)
                    || self.stone(ground, along) == Stone::Timbered
                {
                    continue;
                }
                let side = if (index / LANTERN_EVERY).is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                };
                // By the walls of the road: in a hall wider than it, on its
                // low wall.
                let wide = self.width(along);
                let out = if wide > 0.5 { 0.0 } else { -0.45 };
                let foot = line.offset_point(index, side * (line.wall_at(index) + out));
                let spot = self.spot(foot);
                let from_wall = spot.walls + wide;
                if spot.road != road || !(-1.0..0.05).contains(&from_wall) || spot.mouths > -6.0 {
                    continue;
                }
                // Dwarves' lamps stand on posts of iron.
                let iron = matches!(self.stone(ground, along), Stone::Masonry | Stone::Cave);
                let inward = -spot.out;
                let at = |reach: f32, height: f32| {
                    let ground = foot + inward * reach;
                    Vec3::new(ground.x, spot.floor + height, ground.y)
                };
                // Every third one high up on a tall pole, lighting the rock
                // far above the road.
                let high = (index / LANTERN_EVERY) % 3 == 1;
                let top = if high {
                    let roof = self.ceiling(foot + inward * 0.3) - spot.floor;
                    (roof - 1.5).min(HIGH_LANTERN)
                } else {
                    LANTERN_HEIGHT + 0.8
                };
                let lantern = top - 1.0;
                let post = if iron {
                    &mut parts.iron
                } else {
                    &mut parts.timber
                };
                post.beam(at(0.0, -0.2), at(0.0, top), 0.28, 0.28, Color::WHITE);
                post.beam(
                    at(0.0, top - 0.2),
                    at(0.85, top - 0.2),
                    0.18,
                    0.18,
                    Color::WHITE,
                );
                parts.iron.beam(
                    at(0.75, lantern + 0.45),
                    at(0.75, top - 0.25),
                    0.04,
                    0.04,
                    Color::WHITE,
                );
                parts.lanterns.beam(
                    at(0.75, lantern - 0.2),
                    at(0.75, lantern + 0.25),
                    0.32,
                    0.32,
                    Color::WHITE,
                );
                lanterns.push(at(0.75, lantern));
            }
        }
        lanterns
    }

    /// The walls, the roof and the floor, worked out on the grid.
    fn carve(&self, painter: &Painter, parts: &mut Parts) {
        let (min, max) = self.bounds();
        let nx = ((max.x - min.x) / SPACING).ceil() as usize + 1;
        let nz = ((max.y - min.y) / SPACING).ceil() as usize + 1;
        let point = |v: usize| min + Vec2::new((v % nx) as f32, (v / nx) as f32) * SPACING;
        let spots: Vec<Spot> = (0..nx * nz).map(|v| self.spot(point(v))).collect();
        let roofs: Vec<f32> = (0..nx * nz)
            .map(|v| {
                let spot = &spots[v];
                if spot.edge() < 0.0 {
                    spot.floor + self.roof(point(v), spot)
                } else {
                    f32::NAN
                }
            })
            .collect();
        // Where the edge crosses the side of a square from `a` (inside) to
        // `b` (outside).
        let mut crossings: HashMap<(usize, usize), Crossing> = HashMap::new();
        let mut crossing = |a: usize, b: usize| {
            *crossings.entry((a.min(b), a.max(b))).or_insert_with(|| {
                let (ea, eb) = (spots[a].edge(), spots[b].edge());
                let at = point(a).lerp(point(b), ea / (ea - eb));
                let spot = self.spot(at);
                let wall = spot.walls >= spot.mouths;
                let top = if wall {
                    let leaned = at + spot.out * Self::lean(at);
                    Vec3::new(leaned.x, spot.floor + Self::wall_top(at), leaned.y)
                } else {
                    Vec3::new(at.x, spot.floor + self.roof(at, &spot), at.y)
                };
                Crossing {
                    at,
                    spot,
                    wall,
                    top,
                }
            })
        };
        for j in 0..nz - 1 {
            for i in 0..nx - 1 {
                let v = j * nx + i;
                for triangle in [[v, v + 1, v + nx + 1], [v, v + nx + 1, v + nx]] {
                    if triangle.iter().all(|&v| spots[v].edge() >= 0.0) {
                        continue;
                    }
                    // The part of the triangle inside: its corners, and
                    // where the edge crosses its sides.
                    let mut polygon = Vec::with_capacity(4);
                    for k in 0..3 {
                        let (a, b) = (triangle[k], triangle[(k + 1) % 3]);
                        let inside = spots[a].edge() < 0.0;
                        if inside {
                            polygon.push(Corner::Grid(a));
                        }
                        if inside != (spots[b].edge() < 0.0) {
                            let (a, b) = if inside { (a, b) } else { (b, a) };
                            polygon.push(Corner::Edge(crossing(a, b)));
                        }
                    }
                    let center = polygon
                        .iter()
                        .map(|corner| match corner {
                            Corner::Grid(v) => point(*v),
                            Corner::Edge(c) => c.at,
                        })
                        .sum::<Vec2>()
                        / polygon.len() as f32;
                    let along = match polygon[0] {
                        Corner::Grid(v) => spots[v].along,
                        Corner::Edge(c) => c.spot.along,
                    };
                    let stone = self.stone(center, along);
                    // The roof, facing down, and the floor, facing up.
                    let roof = |corner: &Corner| match corner {
                        Corner::Grid(v) => {
                            let p = point(*v);
                            (Vec3::new(p.x, roofs[*v], p.y), spots[*v].floor)
                        }
                        Corner::Edge(c) => (c.top, c.spot.floor),
                    };
                    let floor = |corner: &Corner| match corner {
                        Corner::Grid(v) => {
                            let p = point(*v);
                            let floor = spots[*v].floor;
                            (Vec3::new(p.x, floor + FLOOR_LIFT, p.y), floor)
                        }
                        Corner::Edge(c) => (
                            Vec3::new(c.at.x, c.spot.floor + FLOOR_LIFT, c.at.y),
                            c.spot.floor,
                        ),
                    };
                    for k in 1..polygon.len() - 1 {
                        let corners = [&polygon[0], &polygon[k], &polygon[k + 1]];
                        painter.triangle(
                            &mut parts.rock[stone.index()],
                            corners.map(roof),
                            Vec3::NEG_Y,
                            |p, normal| stone.uv(p, normal),
                        );
                        let planks = stone == Stone::Timbered;
                        painter.triangle(
                            &mut parts.floor[stone.index()],
                            corners.map(floor),
                            Vec3::Y,
                            |p, _| {
                                let tile = if planks { PLANK_TILE } else { ROCK_TILE };
                                Vec2::new(p.x, p.z) / tile
                            },
                        );
                    }
                    // The wall, where the edge crosses the triangle.
                    let edges: Vec<Crossing> = polygon
                        .iter()
                        .filter_map(|corner| match corner {
                            Corner::Edge(c) => Some(*c),
                            Corner::Grid(_) => None,
                        })
                        .collect();
                    if let [a, b] = edges[..]
                        && (a.wall || b.wall)
                    {
                        self.wall(painter, parts, stone, a, b);
                    }
                }
            }
        }
    }

    /// The bit of wall from where the edge crosses one side of a square of
    /// the grid to where it crosses another: straight up from below the
    /// road to above a rider's head, and leaning out from there to the
    /// roof. In clay, lined with planks below.
    fn wall(&self, painter: &Painter, parts: &mut Parts, stone: Stone, a: Crossing, b: Crossing) {
        let inward = -(a.spot.out + b.spot.out);
        let inward = Vec3::new(inward.x, 0.0, inward.y).normalize_or_zero();
        let along = (b.at - a.at).normalize_or_zero();
        let foot = |c: &Crossing, height: f32| Vec3::new(c.at.x, c.spot.floor + height, c.at.y);
        let rows = [
            (
                [foot(&a, -FOOT), foot(&b, -FOOT)],
                [foot(&a, 1.0), foot(&b, 1.0)],
            ),
            (
                [foot(&a, 1.0), foot(&b, 1.0)],
                [foot(&a, LEAN_FROM), foot(&b, LEAN_FROM)],
            ),
            ([foot(&a, LEAN_FROM), foot(&b, LEAN_FROM)], [a.top, b.top]),
        ];
        for (row, (low, high)) in rows.into_iter().enumerate() {
            let lined = stone == Stone::Clay && row < 2;
            let builder = if lined {
                &mut parts.planks
            } else {
                &mut parts.rock[stone.index()]
            };
            let uv = |p: Vec3, normal: Vec3| {
                if lined {
                    Vec2::new(p.x * along.x + p.z * along.y, -p.y) / PLANK_TILE
                } else {
                    stone.uv(p, normal)
                }
            };
            let floors = [a.spot.floor, b.spot.floor];
            for (corners, floors) in [
                ([low[0], low[1], high[1]], [floors[0], floors[1], floors[1]]),
                (
                    [low[0], high[1], high[0]],
                    [floors[0], floors[1], floors[0]],
                ),
            ] {
                let corners = [0, 1, 2].map(|k| (corners[k], floors[k]));
                painter.triangle(builder, corners, inward, uv);
            }
        }
        // A seam of gold along the slate, here and there.
        let middle = (a.at + b.at) / 2.0;
        if stone == Stone::Slate && fbm(middle / 7.0 + 21.0, 2) > 0.6 {
            let height = 1.4 + 2.0 * fbm(middle / 11.0 + 4.0, 2);
            let lift = inward * 0.04;
            let seam = |c: &Crossing, h: f32| foot(c, height + h) + lift;
            parts.gold.quad(
                [
                    (seam(&a, 0.0), Vec2::ZERO),
                    (seam(&b, 0.0), Vec2::X),
                    (seam(&b, 0.16), Vec2::ONE),
                    (seam(&a, 0.16), Vec2::Y),
                ],
                inward,
                Color::WHITE,
            );
        }
    }

    /// Where on the ground the excavation is: the box round its roads and
    /// their walls.
    fn bounds(&self) -> (Vec2, Vec2) {
        let wide = self.halls.iter().map(|hall| hall.width).fold(0.0, f32::max);
        let reach = WALL_DISTANCE + wide + LEAN + 2.0 * SPACING;
        let mut points: Vec<Vec2> = (0..=self.count)
            .map(|k| self.track.centerline()[(self.first + k) % self.track.sample_count()])
            .collect();
        for &road in &self.branches {
            points.extend_from_slice(self.track.road(road).centerline());
        }
        points.iter().fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(min, max), &p| (min.min(p - reach), max.max(p + reach)),
        )
    }

    /// Where a hall is wider than the road, a low wall of its stone along
    /// either side of the road, just past the walls the bikes run into; and
    /// in the widest halls of masonry, a row of great square pillars either
    /// side, from the floor up into the roof.
    fn parapets_and_pillars(&self, painter: &Painter, parts: &mut Parts) {
        let track = self.track;
        let n = track.sample_count();
        let step = track.step_length();
        let (first, last) = (track.index_at(self.span.from), track.index_at(self.span.to));
        let count = (last + n - first) % n;
        let mut since_pillars = PILLAR_EVERY / 2.0;
        for k in 0..count {
            let (i, j) = ((first + k) % n, (first + k + 1) % n);
            let along = i as f32 * step;
            let wide = self.width(along).min(self.width(j as f32 * step));
            if wide < 0.5 {
                continue;
            }
            let stone = self.stone(track.centerline()[i], along);
            for side in [-1.0, 1.0] {
                let inner = |index: usize| side * (track.wall_at(index) + PARAPET_OUT);
                let outer = |index: usize| inner(index) + side * PARAPET_THICK;
                let at = |index: usize, lateral: f32, height: f32| {
                    let ground = track.offset_point(index, lateral);
                    Vec3::new(
                        ground.x,
                        track.surface_point(index, 0.0).y + height,
                        ground.y,
                    )
                };
                let base = [
                    at(i, inner(i), 0.0),
                    at(j, inner(j), 0.0),
                    at(j, outer(j), 0.0),
                    at(i, outer(i), 0.0),
                ];
                self.block(
                    painter,
                    &mut parts.rock[stone.index()],
                    stone,
                    base,
                    -0.5,
                    PARAPET_HEIGHT,
                );
            }
            // The pillars.
            since_pillars += step;
            if stone != Stone::Masonry || wide < PILLARS_FROM || since_pillars < PILLAR_EVERY {
                continue;
            }
            since_pillars = 0.0;
            for side in [-1.0, 1.0] {
                let middle = track.offset_point(i, side * (track.wall_at(i) + wide / 2.0));
                let spot = self.spot(middle);
                let roof = spot.floor + self.roof(middle, &spot);
                let (ahead, out) = {
                    let (_, heading) = track.pose_at(i);
                    let ahead = crate::geometry::forward(heading);
                    (ahead, Vec2::new(-ahead.y, ahead.x))
                };
                let half = PILLAR_THICK / 2.0;
                let corner = |a: f32, o: f32| {
                    let p = middle + ahead * a * half + out * o * half;
                    Vec3::new(p.x, spot.floor, p.y)
                };
                let base = [
                    corner(-1.0, -1.0),
                    corner(1.0, -1.0),
                    corner(1.0, 1.0),
                    corner(-1.0, 1.0),
                ];
                self.block(
                    painter,
                    &mut parts.rock[Stone::Masonry.index()],
                    Stone::Masonry,
                    base,
                    -FOOT,
                    roof - spot.floor + 3.0,
                );
            }
        }
    }

    /// A block of `stone` on the four corners of `base` (round it either
    /// way), from `bottom` to `top` over them (m): its four sides and its
    /// top, lit as the rock.
    fn block(
        &self,
        painter: &Painter,
        builder: &mut MeshBuilder,
        stone: Stone,
        base: [Vec3; 4],
        bottom: f32,
        top: f32,
    ) {
        let middle = base.iter().copied().sum::<Vec3>() / 4.0;
        let floor = middle.y;
        let up = |p: Vec3, h: f32| p + Vec3::Y * h;
        for e in 0..4 {
            let (a, b) = (base[e], base[(e + 1) % 4]);
            let outward = ((a + b) / 2.0 - middle) * Vec3::new(1.0, 0.0, 1.0);
            for corners in [
                [up(a, bottom), up(b, bottom), up(b, top)],
                [up(a, bottom), up(b, top), up(a, top)],
            ] {
                painter.triangle(
                    builder,
                    corners.map(|p| (p, floor)),
                    outward,
                    |p, normal| stone.uv(p, normal),
                );
            }
        }
        for corners in [[base[0], base[1], base[2]], [base[0], base[2], base[3]]] {
            painter.triangle(
                builder,
                corners.map(|p| (up(p, top), floor)),
                Vec3::Y,
                |p, normal| stone.uv(p, normal),
            );
        }
    }

    /// Stalactites hanging from the roofs of the halls, here and there.
    fn stalactites(&self, painter: &Painter, parts: &mut Parts) {
        const SIDES: usize = 6;
        let (min, max) = self.bounds();
        let cells = ((max - min) / STALACTITE_CELL).ceil();
        for cell in 0..(cells.x * cells.y) as u32 {
            let chance = |k: u32| random01(cell * 7 + k + 1013);
            if chance(0) > STALACTITE_CHANCE {
                continue;
            }
            let (cx, cz) = (cell % cells.x as u32, cell / cells.x as u32);
            let at = min
                + (Vec2::new(cx as f32, cz as f32) + Vec2::new(chance(1), chance(2)))
                    * STALACTITE_CELL;
            let spot = self.spot(at);
            if spot.edge() > -2.0 || spot.depth() < 3.5 || !self.stone(at, spot.along).natural() {
                continue;
            }
            let roof = spot.floor + self.roof(at, &spot);
            let length = (1.5 + (STALACTITE_LENGTH - 1.5) * chance(3))
                .min(roof - spot.floor - STALACTITE_CLEAR);
            if length < 1.0 {
                continue;
            }
            let stone = self.stone(at, spot.along);
            let radius = 0.3 + 0.22 * length;
            let tip = Vec3::new(at.x, roof - length, at.y);
            let ring = |k: usize| {
                let angle = (k as f32 + chance(4)) * std::f32::consts::TAU / SIDES as f32;
                let r = radius * (0.8 + 0.4 * random01(cell * 13 + k as u32));
                Vec3::new(at.x + r * angle.cos(), roof + 0.8, at.y + r * angle.sin())
            };
            for k in 0..SIDES {
                let (p, q) = (ring(k), ring((k + 1) % SIDES));
                let outward = (p + q) / 2.0 - Vec3::new(at.x, (p.y + tip.y) / 2.0, at.y);
                painter.triangle(
                    &mut parts.rock[stone.index()],
                    [(p, spot.floor), (q, spot.floor), (tip, spot.floor)],
                    outward,
                    |p, normal| stone.uv(p, normal),
                );
            }
        }
    }

    /// Veins of gold winding over the roofs of slate.
    fn veins(&self, parts: &mut Parts) {
        let (min, max) = self.bounds();
        let cells = ((max - min) / VEIN_CELL).ceil();
        for cell in 0..(cells.x * cells.y) as u32 {
            let chance = |k: u32| random01(cell * 11 + k + 4271);
            if chance(0) > 0.6 {
                continue;
            }
            let (cx, cz) = (cell % cells.x as u32, cell / cells.x as u32);
            let mut at = min
                + (Vec2::new(cx as f32, cz as f32) + Vec2::new(chance(1), chance(2))) * VEIN_CELL;
            let mut heading = chance(3) * std::f32::consts::TAU;
            let mut last: Option<Vec3> = None;
            for k in 0..VEIN_STEPS as u32 {
                let spot = self.spot(at);
                let on_slate = spot.edge() < -1.0
                    && spot.depth() > 1.5
                    && self.stone(at, spot.along) == Stone::Slate;
                let here = Vec3::new(at.x, spot.floor + self.roof(at, &spot) - 0.06, at.y);
                if let (true, Some(last)) = (on_slate, last) {
                    let across = Vec3::new(-(here.z - last.z), 0.0, here.x - last.x)
                        .normalize_or_zero()
                        * VEIN_WIDTH
                        / 2.0;
                    parts.gold.quad(
                        [
                            (last - across, Vec2::ZERO),
                            (here - across, Vec2::X),
                            (here + across, Vec2::ONE),
                            (last + across, Vec2::Y),
                        ],
                        Vec3::NEG_Y,
                        Color::WHITE,
                    );
                }
                last = on_slate.then_some(here);
                heading += 1.2 * (random01(cell * 31 + k + 77) - 0.5);
                at += Vec2::new(heading.cos(), heading.sin()) * VEIN_STEP;
            }
        }
    }

    /// The branches through the excavation are laid with planks, where they
    /// are clear of the lap's asphalt, with a beam along either edge.
    fn planked_branches(&self, painter: &Painter, parts: &mut Parts) {
        for &road in &self.branches {
            let line = self.track.road(road);
            let half = line.half_width();
            let step = line.step_length();
            let at = |index: usize, lateral: f32| {
                self.track.surface_point_on(road, index, lateral)
                    + Vec3::Y * (SURFACE_Y + PLANK_LIFT)
            };
            for index in 0..line.sample_count() - 1 {
                let clear = [index, index + 1].iter().all(|&i| {
                    [-half, half]
                        .iter()
                        .all(|&lateral| self.off_lap_asphalt(line.offset_point(i, lateral)) > 0.0)
                });
                if !clear {
                    continue;
                }
                let floor = self.track.surface_point_on(road, index, 0.0).y;
                let (v0, v1) = (index as f32 * step, (index + 1) as f32 * step);
                let corners = [
                    (at(index, -half), Vec2::new(-half, v0)),
                    (at(index + 1, -half), Vec2::new(-half, v1)),
                    (at(index + 1, half), Vec2::new(half, v1)),
                    (at(index, half), Vec2::new(half, v0)),
                ];
                for [a, b, c] in [[0, 1, 2], [0, 2, 3]] {
                    let [a, b, c] = [corners[a], corners[b], corners[c]];
                    painter.triangle(
                        &mut parts.planks,
                        [(a.0, floor), (b.0, floor), (c.0, floor)],
                        Vec3::Y,
                        |p, _| {
                            let (_, uv) = [a, b, c]
                                .into_iter()
                                .min_by(|x, y| x.0.distance(p).total_cmp(&y.0.distance(p)))
                                .unwrap();
                            uv / PLANK_TILE
                        },
                    );
                }
                for side in [-1.0, 1.0] {
                    let edge = side * (half + 0.12);
                    parts.timber.beam(
                        at(index, edge) + Vec3::Y * 0.05,
                        at(index + 1, edge) + Vec3::Y * 0.05,
                        0.24,
                        0.16,
                        Color::WHITE,
                    );
                }
            }
        }
    }

    /// The outside of the rock: over the excavation, across from the rock
    /// beyond the outermost road on one side to that beyond the outermost
    /// on the other, at the height of the ground round it, and over the
    /// roof wherever that is higher; down its sides; and round the mouths.
    fn outside(&self, terrain: &Terrain, parts: &mut Parts) {
        const STRIPS: usize = 24;
        const SPREAD: usize = 8;
        let track = self.track;
        let n = track.sample_count();
        let (first, last) = (track.index_at(self.span.from), track.index_at(self.span.to));
        let count = (last + n - first) % n;
        let indices: Vec<usize> = (0..=count).map(|k| (first + k) % n).collect();
        // How far to either side of the lap the rock reaches at each point
        // of it (past a hall wider than the road, as much further), for the
        // roads off it too.
        let mut reach: Vec<(f32, f32)> = indices
            .iter()
            .map(|&index| {
                let shell = SHELL + self.width(index as f32 * track.step_length());
                (-shell, shell)
            })
            .collect();
        for &road in &self.branches {
            for &point in track.road(road).centerline() {
                let on = track.nearest_among(point, first, count + 1);
                let k = (on.index + n - first) % n;
                if let Some((low, high)) = reach.get_mut(k) {
                    *low = low.min(on.lateral - SHELL);
                    *high = high.max(on.lateral + SHELL);
                }
            }
        }
        let reach: Vec<(f32, f32)> = (0..reach.len())
            .map(|k| {
                let near = k.saturating_sub(SPREAD)..(k + SPREAD + 1).min(reach.len());
                reach[near]
                    .iter()
                    .fold((0.0f32, 0.0f32), |(low, high), &(l, h)| {
                        (low.min(l), high.max(h))
                    })
            })
            .collect();
        let floor = |index: usize, lateral: f32| {
            track
                .surface_point(index, lateral.clamp(-WALL_DISTANCE, WALL_DISTANCE))
                .y
        };
        let place = |index: usize, lateral: f32, height: f32| {
            let ground = track.offset_point(index, lateral);
            Vec3::new(ground.x, height, ground.y)
        };
        let top = |k: usize, lateral: f32| {
            let index = indices[k];
            let (low, high) = reach[k];
            let beyond = |lateral: f32| terrain.height(track.offset_point(index, lateral));
            let t = (lateral - low) / (high - low);
            let ground = track.offset_point(index, lateral);
            // Where a road passes over, the rock carries it: level with it,
            // just under its verge, and with no hump through it.
            let over = self.passing_over(ground);
            let clear = over.map_or(1.0, |(distance, _)| {
                smoothstep(OVER_FLAT, OVER_FLAT + OVER_BLEND, distance)
            });
            let hump =
                6.0 * (1.0 - (2.0 * t - 1.0).powi(2)) * (0.5 + fbm(ground / 19.0, 3)) * clear;
            let rock = beyond(low - 1.0).lerp(beyond(high + 1.0), t) + hump;
            let roof = [Vec2::ZERO, Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y]
                .iter()
                .map(|&d| self.ceiling(ground + d * 4.0))
                .fold(f32::NEG_INFINITY, f32::max);
            let top = rock.max(roof + ROOF_ABOVE);
            match over {
                Some((_, height)) => (height - 0.15).lerp(top, clear),
                None => top,
            }
        };
        let lateral = |k: usize, s: usize| {
            let (low, high) = reach[k];
            low + (high - low) * s as f32 / STRIPS as f32
        };
        let tops: Vec<Vec<f32>> = (0..indices.len())
            .map(|k| (0..=STRIPS).map(|s| top(k, lateral(k, s))).collect())
            .collect();
        for k in 0..indices.len() - 1 {
            let (i, j) = (indices[k], indices[k + 1]);
            for s in 0..STRIPS {
                let corners = [(i, k, s), (j, k + 1, s), (j, k + 1, s + 1), (i, k, s + 1)].map(
                    |(index, k, s)| {
                        let p = place(index, lateral(k, s), tops[k][s]);
                        (p, Vec2::new(p.x, p.z) / ROCK_TILE)
                    },
                );
                parts.outside.quad(corners, Vec3::Y, Color::WHITE);
            }
            // Its sides, down into the ground.
            for (s, side) in [(0, -1.0), (STRIPS, 1.0)] {
                let (a, b) = (lateral(k, s), lateral(k + 1, s));
                let corners = [
                    place(i, a, floor(i, a) - 6.0),
                    place(j, b, floor(j, b) - 6.0),
                    place(j, b, tops[k + 1][s]),
                    place(i, a, tops[k][s]),
                ]
                .map(|p| (p, Vec2::new(p.x + p.z, -p.y) / ROCK_TILE));
                let outward = track.offset_point(i, side) - track.offset_point(i, 0.0);
                parts
                    .outside
                    .quad(corners, Vec3::new(outward.x, 0.0, outward.y), Color::WHITE);
            }
        }
        // The rock faces round the mouths: from the edge of the opening to
        // the top, and down beside it.
        for (k, outward) in [(0, -1.0), (indices.len() - 1, 1.0)] {
            let index = indices[k];
            let (_, heading) = track.pose_at(index);
            let ahead = crate::geometry::forward(heading);
            let facing = Vec3::new(ahead.x, 0.0, ahead.y) * outward;
            let (low, high) = reach[k];
            let wall = track.wall_at(index) + self.width(index as f32 * track.step_length()) - 0.05;
            let mut laterals = vec![low, -wall - 0.01, wall + 0.01, high];
            laterals.extend((0..=(2.0 * wall) as usize).map(|m| -wall + m as f32));
            laterals.extend([wall]);
            laterals.sort_by(f32::total_cmp);
            laterals.dedup();
            let (row_low, row_high) = reach[k];
            let edge = |lateral: f32| {
                if lateral.abs() < wall + 0.005 {
                    self.ceiling(track.offset_point(index, lateral)) - 0.4
                } else {
                    floor(index, lateral) - 6.0
                }
            };
            let upper = |lateral: f32| {
                let t = (lateral - row_low) / (row_high - row_low) * STRIPS as f32;
                let s = (t.floor() as usize).min(STRIPS - 1);
                tops[k][s].lerp(tops[k][s + 1], t - s as f32)
            };
            for pair in laterals.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if upper(a) <= edge(a) && upper(b) <= edge(b) {
                    continue;
                }
                let corners = [
                    place(index, a, edge(a)),
                    place(index, b, edge(b)),
                    place(index, b, upper(b)),
                    place(index, a, upper(a)),
                ]
                .map(|p| (p + facing * 0.02, Vec2::new(p.x + p.z, -p.y) / ROCK_TILE));
                parts.outside.quad(corners, facing, Color::WHITE);
            }
        }
    }
}

/// Where the edge of the space inside crosses a side of a square of the
/// grid.
#[derive(Clone, Copy, Debug)]
struct Crossing {
    at: Vec2,
    spot: Spot,
    /// Whether it is the edge at a wall, rather than at a mouth.
    wall: bool,
    /// Where the roof meets the top of the wall there.
    top: Vec3,
}

/// A corner of the part of a triangle of the grid inside: one of its own,
/// or where the edge crosses one of its sides.
enum Corner {
    Grid(usize),
    Edge(Crossing),
}

/// Colours the rock as the lanterns light it.
struct Painter<'a> {
    lanterns: &'a [Vec3],
}

impl Painter<'_> {
    /// Adds a triangle with corners at the given points (each over a floor
    /// at the given height) to `builder`, facing `facing`, with texture
    /// coordinates from `uv` (given each point and the normal), coloured by
    /// the light at each corner.
    fn triangle(
        &self,
        builder: &mut MeshBuilder,
        corners: [(Vec3, f32); 3],
        facing: Vec3,
        uv: impl Fn(Vec3, Vec3) -> Vec2,
    ) {
        let [a, b, c] = corners.map(|(p, _)| p);
        let normal = (b - a).cross(c - a).normalize_or_zero();
        let normal = if normal.dot(facing) >= 0.0 {
            normal
        } else {
            -normal
        };
        builder.shaded_triangle(
            corners.map(|(p, floor)| (p, uv(p, normal), self.light(p, normal, floor))),
            facing,
        );
    }

    /// The light at `p`, on a face whose normal is `normal`, over a floor at
    /// `floor`: warm and bright on the faces turned to a lantern near it,
    /// dimmer away from them, and dark high up, with patches.
    fn light(&self, p: Vec3, normal: Vec3, floor: f32) -> Color {
        let glow: f32 = self
            .lanterns
            .iter()
            .map(|lantern| {
                let toward = *lantern - p;
                let d2 = toward.length_squared();
                if d2 > 16.0 * LANTERN_REACH * LANTERN_REACH {
                    return 0.0;
                }
                let facing = (normal.dot(toward) / d2.sqrt().max(0.1)).max(0.0);
                let near = (-d2 / (LANTERN_REACH * LANTERN_REACH)).exp();
                let far = 0.45 * (-d2 / (16.0 * LANTERN_REACH * LANTERN_REACH)).exp();
                (near + far) * (0.25 + 0.75 * facing)
            })
            .sum();
        let up = smoothstep(0.0, 30.0, p.y - floor);
        let patch = 0.7 + 0.6 * fbm(Vec2::new(p.x, p.z) / 7.0 + p.y / 5.0, 2);
        let light = ((0.13 + 1.2 * glow.min(1.3)) * (1.0 - 0.55 * up) * patch).min(1.4);
        Color::srgb(light, light * 0.82, light * 0.66)
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    /// The circuit with a mine's excavation, its track and its scenery.
    pub fn with_a_mine(
        circuits: &'static crate::circuits::Circuits,
    ) -> (Track, &'static crate::world::scenery::Scenery) {
        let circuit = circuits
            .iter()
            .find(|circuit| circuit.scenery.soil == crate::world::scenery::Soil::Mine)
            .expect("a circuit has a mine");
        (Track::new(&circuit.layout), &circuit.scenery)
    }

    /// The circuit with great halls dug through its mountain, its track and
    /// its scenery.
    pub fn with_great_halls(
        circuits: &'static crate::circuits::Circuits,
    ) -> (Track, &'static crate::world::scenery::Scenery) {
        let circuit = circuits
            .iter()
            .find(|circuit| {
                circuit.scenery.soil != crate::world::scenery::Soil::Mine
                    && circuit.scenery.excavation().is_some()
            })
            .expect("a circuit has halls dug as a mine's");
        (Track::new(&circuit.layout), &circuit.scenery)
    }

    pub fn a_great_hall_is_far_wider_and_higher_than_the_road_past_its_pillars_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (track, scenery) = with_great_halls(circuits);
        let halls = scenery.excavation().unwrap();
        let span = scenery.tunnels[0];
        let cave = Cave::new(&track, span, halls);
        let widest = halls
            .iter()
            .max_by(|a, b| a.width.total_cmp(&b.width))
            .unwrap();
        assert_eq!(widest.stone, Stone::Masonry);
        let index = track.index_at((widest.span.from + widest.span.to) / 2.0);
        let wall = track.wall_at(index);
        // Inside out to its own walls, far past the road's.
        for side in [-1.0, 1.0] {
            let inside = track.offset_point(index, side * (wall + widest.width - 1.0));
            assert!(cave.spot(inside).edge() < 0.0);
            let past = track.offset_point(index, side * (wall + widest.width + 1.5));
            assert!(cave.spot(past).edge() > 0.0);
        }
        // Its roof high over the road.
        let point = track.centerline()[index];
        let spot = cave.spot(point);
        assert!(
            cave.roof(point, &spot) > 25.0,
            "{}",
            cave.roof(point, &spot)
        );
        // Built: low walls along the road, pillars, the halls of masonry and
        // the natural caves, lit by lamps on iron posts.
        let mut parts = Parts::default();
        let lanterns = cave.timber_and_lanterns(&mut parts);
        assert!(lanterns.len() > 10, "{} lamps", lanterns.len());
        assert!(parts.timber.is_empty() && !parts.iron.is_empty());
        let painter = Painter {
            lanterns: &lanterns,
        };
        cave.carve(&painter, &mut parts);
        assert!(!parts.rock[Stone::Masonry.index()].is_empty());
        assert!(!parts.rock[Stone::Cave.index()].is_empty());
        let mut built = Parts::default();
        cave.parapets_and_pillars(&painter, &mut built);
        assert!(!built.rock[Stone::Masonry.index()].is_empty());
    }

    pub fn the_halls_are_high_and_the_galleries_lower_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (track, scenery) = with_a_mine(circuits);
        let halls = scenery.halls;
        let span = scenery.tunnels[0];
        let cave = Cave::new(&track, span, halls);
        let lap = track.lap_length();
        for index in 0..track.sample_count() {
            let along = index as f32 * track.step_length();
            if !span.contains(along, lap)
                || cave
                    .signed(span.from, along)
                    .min(cave.signed(along, span.to))
                    < 5.0
            {
                continue;
            }
            let point = track.centerline()[index];
            let spot = cave.spot(point);
            assert!(spot.edge() < 0.0, "inside at {along} m");
            let roof = cave.roof(point, &spot);
            let (crown, hall) = cave.crown(along);
            if hall > 0.99 {
                assert!(roof > 0.6 * crown && roof > 15.0, "{roof} m at {along} m");
            } else if hall < 0.01 {
                assert!((8.0..13.0).contains(&roof), "{roof} m at {along} m");
            }
        }
        // The highest hall is the highest.
        let highest = halls.iter().map(|hall| hall.crown).fold(0.0, f32::max);
        assert!(highest > 30.0);
    }

    pub fn the_walls_stand_where_the_bikes_hit_them_with_the_roof_above_a_rider_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (track, scenery) = with_a_mine(circuits);
        let span = scenery.tunnels[0];
        let cave = Cave::new(&track, span, scenery.halls);
        let lap = track.lap_length();
        for index in 0..track.sample_count() {
            let along = index as f32 * track.step_length();
            if !span.contains(along, lap)
                || cave
                    .signed(span.from, along)
                    .min(cave.signed(along, span.to))
                    < 5.0
            {
                continue;
            }
            for side in [-1.0, 1.0] {
                // Just inside the wall, and just past it, unless another
                // road is there.
                let inside = track.offset_point(index, side * (WALL_DISTANCE - 0.2));
                let spot = cave.spot(inside);
                assert!(spot.edge() < 0.0, "{along} m");
                assert!(cave.roof(inside, &spot) > 3.5);
                let past = track.offset_point(index, side * (WALL_DISTANCE + 0.2));
                let spot = cave.spot(past);
                if spot.road == 0 {
                    assert!(spot.edge() > 0.0, "{along} m");
                }
            }
        }
    }

    pub fn rock_stands_between_the_roads_where_they_part_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (track, scenery) = with_a_mine(circuits);
        let span = scenery.tunnels[0];
        let cave = Cave::new(&track, span, scenery.halls);
        assert_eq!(cave.branches.len(), 4);
        for fork in track.forks() {
            let line = track.road(fork.road);
            let middle = line.centerline()[line.sample_count() / 2];
            let on_lap = track.nearest_among(middle, cave.first, cave.count);
            // Half way between the two roads, rock.
            let between = (middle + on_lap.point) / 2.0;
            assert!(cave.spot(between).edge() > 2.0, "{fork:?}");
            // On either road, the roof high above.
            for point in [middle, on_lap.point] {
                let spot = cave.spot(point);
                assert!(spot.edge() < 0.0);
                assert!(cave.roof(point, &spot) > 15.0);
            }
        }
    }

    pub fn the_rock_over_the_excavation_carries_the_road_over_the_top_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (track, scenery) = with_a_mine(circuits);
        let span = scenery.tunnels[0];
        let cave = Cave::new(&track, span, scenery.halls);
        // The road over the top of the mountain crosses the excavation.
        let (gap, over, under) = cave
            .others
            .iter()
            .flat_map(|&(from, count)| from..from + count)
            .flat_map(|over| (cave.first..cave.first + cave.count).map(move |under| (over, under)))
            .map(|(over, under)| {
                let n = track.sample_count();
                let (over, under) = (over % n, under % n);
                let gap = track.centerline()[over].distance(track.centerline()[under]);
                (gap, over, under)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .expect("other stretches come near");
        assert!(gap < 2.0, "{gap} m");
        let point = track.centerline()[under];
        let (distance, height) = cave.passing_over(point).expect("a road passes over");
        assert!(distance < 2.0);
        assert!((height - track.elevations()[over]).abs() < 0.5);
        // High over the roof, which the rock carries it on.
        assert!(height - cave.ceiling(point) > 40.0);
        // Elsewhere nothing passes over.
        let away = track.centerline()[track.index_at(span.from + 60.0)];
        assert!(cave.passing_over(away).is_none());
    }

    pub fn the_excavation_is_closed_and_lit_on(circuits: &'static crate::circuits::Circuits) {
        let (track, scenery) = with_a_mine(circuits);
        let terrain = Terrain::new(&track, scenery);
        let span = scenery.tunnels[0];
        let cave = Cave::new(&track, span, scenery.halls);
        let mut parts = Parts::default();
        let lanterns = cave.timber_and_lanterns(&mut parts);
        assert!(lanterns.len() > 30, "{} lanterns", lanterns.len());
        let painter = Painter {
            lanterns: &lanterns,
        };
        cave.carve(&painter, &mut parts);
        cave.stalactites(&painter, &mut parts);
        cave.veins(&mut parts);
        cave.planked_branches(&painter, &mut parts);
        cave.outside(&terrain, &mut parts);
        let halls = scenery.halls;
        for (stone, rock) in Stone::ALL.iter().zip(&parts.rock) {
            let used = *stone == Stone::Timbered || halls.iter().any(|hall| hall.stone == *stone);
            assert_eq!(rock.is_empty(), !used, "{stone:?}");
        }
        assert!(!parts.gold.is_empty() && !parts.planks.is_empty() && !parts.outside.is_empty());
        // Brighter by the lanterns than between them, and high up.
        let lantern = lanterns[lanterns.len() / 2];
        let floor = lantern.y - LANTERN_HEIGHT;
        let near = painter
            .light(lantern + Vec3::X, Vec3::NEG_X, floor)
            .to_srgba()
            .red;
        let high = painter
            .light(lantern + Vec3::Y * 25.0, Vec3::NEG_Y, floor)
            .to_srgba()
            .red;
        assert!(near > 1.5 * high, "{near} {high}");
    }
}
