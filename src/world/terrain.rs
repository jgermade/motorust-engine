//! Terreno alrededor del circuito: junto a la
//! carretera sigue su altura y su peralte; más allá hay colinas de hierba,
//! peñascos que caen a pico sobre los muros, playas y el mar al oeste y al
//! sur, y montañas tierra adentro, al noreste.
//!
//! Se calcula una vez al arrancar sobre una rejilla: fina cerca del circuito
//! y cada vez más gruesa hacia el horizonte.

use bevy::{
    asset::RenderAssetUsages, camera::visibility::RenderLayers, mesh::Indices, prelude::*,
    render::render_resource::PrimitiveTopology,
};

use super::cavern::excavation_width;
use super::dam::DamGround;
use super::flowers::{FlowerMap, phlox_color};
use super::realm::Realm;
use super::scenery::{Coast, FlowerKind, Greens, Pit, RockTint, Scenery, Side, Soil, Turf, span};
use super::sky::MEDITERRANEAN_SUN;
use super::structures::{TUNNEL_SHELL, Tunnels};
use super::textures::WorldTextures;
use crate::geometry::{fbm, smoothstep, value_noise};
use crate::track::{BASE_ELEVATION, Nearest, Track, WALL_DISTANCE, closed_curve};

pub const SEA_LEVEL: f32 = -1.0;
/// Hasta esta distancia de la línea central, el terreno es la prolongación de
/// la carretera (incluido el arcén hasta los muros).
const FLAT: f32 = WALL_DISTANCE + 1.5;
/// How far from a bridge the ground keeps under its deck (m), and from
/// where it is the ground as anywhere else.
const BRIDGE_REACH: (f32, f32) = (FLAT + 12.0, FLAT + 22.0);
/// Lo que queda el terreno por debajo de la carretera, para que no asome.
const UNDER_ROAD: f32 = 0.03;
const FINE_SPACING: f32 = 3.5;
/// Margen alrededor del circuito con rejilla fina.
pub(super) const FINE_MARGIN: f32 = 260.0;
/// Hasta dónde llega el terreno desde el centro del circuito.
const EXTENT: f32 = 3500.0;
/// Cuánto crece cada celda respecto a la anterior fuera de la zona fina,
/// and how big it grows at most (m): the mountains over the sea keep their
/// shape, a peak its point.
const COARSE_GROWTH: f32 = 1.15;
const COARSEST: f32 = 70.0;
/// Tamaño (m) que cubre cada repetición de las texturas.
pub const GRASS_TILE: f32 = 11.0;
const SAND_TILE: f32 = 8.5;
const ROCK_TILE: f32 = 20.0;
/// Size (m) of each repetition of the checkerboard earth: two by two squares.
const SOIL_TILE: f32 = 7.0;
/// Size (m) of each repetition of a room's floorboards indoors (see
/// [`Soil::Floor`]).
const FLOOR_TILE: f32 = 12.8;
/// Las caras más inclinadas que esto son roca.
pub const CLIFF_NORMAL_Y: f32 = 0.8;
/// Por debajo de esta altura, cerca del mar, hay arena.
const BEACH_HEIGHT: f32 = SEA_LEVEL + 1.6;
/// Above this height (m), on the tallest mountains, there is snow, its line
/// reaching up and down this much along the ravines; but not on faces
/// steeper than this (the normal's height), which stay bare.
const SNOW_LINE: f32 = 560.0;
const SNOW_REACH: f32 = 110.0;
const SNOW_NORMAL_Y: f32 = 0.55;
/// Above this height (m), below the snow, no tree grows: the tallest
/// mountains are bare rock, volcanic scree, reaching up and down this much.
const TREE_LINE: f32 = 400.0;
const TREE_LINE_REACH: f32 = 50.0;
/// Up the hills, the grass gives way to forest from this height (m), and is
/// all forest from this one (see [`grass_tint`]); far from the roads, over
/// this many metres past the fine part of the grid, the land is all forest.
const FOREST_FROM: f32 = 70.0;
const FOREST_FULL: f32 = 170.0;
const WILD_REACH: f32 = 220.0;
/// Where a road passes over a tunnel, the ground stays down at the tunnel
/// this far from its centre line (m): short of the edge of its rock, so the
/// slope up to the road above is under it too.
const BURIED: f32 = TUNNEL_SHELL - 2.0 * FINE_SPACING;
/// How far past the flat ground beside a road on a mountain it climbs the
/// ground reaches the mountainside (m).
const SLOPE_REACH: f32 = 30.0;
/// Along this much of each end of a dam's crest, the ground under the road
/// goes from the road's own to the gorge's and the lake's, which the dam
/// covers.
const CREST_FADE: f32 = 10.0;
/// How far across each face of an open pit runs per metre up (m): steep.
const PIT_FACE: f32 = 0.6;
/// How far above the water the foot of an open pit is (m).
const PIT_FOOT: f32 = 0.4;
/// How far apart the points of a shore drawn as an outline are (m).
const SHORE_SPACING: f32 = 4.0;
/// How far above the road the earth must stand to stop a bike (m), and the
/// steps in which [`Terrain::track_edges`] looks for it (m).
const EARTH_RISE: f32 = 0.3;
const EDGE_STEP: f32 = 0.1;

/// Tipo de suelo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ground {
    Grass,
    Rock,
    Sand,
    /// On the tallest mountains, over the snow line.
    Snow,
}

/// La altura del terreno muestreada en una rejilla rectangular (no uniforme).
#[derive(Resource)]
pub struct Terrain {
    xs: Vec<f32>,
    zs: Vec<f32>,
    /// Alturas por filas: `heights[z * xs.len() + x]`.
    heights: Vec<f32>,
    /// What the ground's look turns on at each point, by rows as `heights`
    /// (see [`look`]).
    sites: Vec<Site>,
    /// What the steep faces are made of.
    soil: Soil,
    /// Where the grass is in flower, if anywhere.
    flowers: Option<FlowerMap>,
    /// Where the ground stays grass however steep it is.
    turf: &'static [Turf],
    /// Where the granite is of another colour.
    rock_tints: &'static [RockTint],
    /// The corners of the fine part of the grid, round the roads: past it
    /// the land is wild (see [`grass_tint`]).
    fine: (Vec2, Vec2),
    /// How much of each point of the grid is mountain across the sea (0 to
    /// 1, by rows as `heights`): wild land, under forest.
    mountain: Vec<f32>,
    /// On the mountains across the sea, how far each point of the grid lies
    /// down a gully (up to 1) or up on a ridge (down to -1), by rows as
    /// `heights` (0 elsewhere): the snow lies on down the gullies, the
    /// forest climbs them, and they are in shade where the ridges catch the
    /// light (see [`weathered`]).
    hollow: Vec<f32>,
    /// How much the ground faces up at each point of the grid (the upward
    /// part of its normal, from the points round it), by rows as `heights`.
    up: Vec<f32>,
    /// How the grass is coloured, and how light (see `Scenery::greens` and
    /// `Scenery::grass_shade`).
    greens: Greens,
    grass_shade: f32,
    /// The dams' lakes, whose water stands over the sea's (see
    /// [`Terrain::water_level`]).
    lakes: Vec<DamGround>,
}

/// A piece of the ground in the world: one of its meshes, each over the
/// whole circuit (see [`Terrain::meshes`]). The benchmark takes it out of
/// the sun's shadows for a turn, to see what drawing all of it into every
/// cascade costs (see [`crate::bench`]).
#[derive(Component)]
pub struct GroundPiece;

/// A mesh of the ground and what it is drawn as: a kind of ground and, on
/// grass in a field of flowers, the flowers (see [`Terrain::meshes`]).
pub struct GroundMesh {
    pub ground: Ground,
    pub flowers: Option<FlowerKind>,
    pub mesh: Mesh,
    /// The layers it is drawn on, on a circuit split between the Earth and
    /// the Moon (see [`super::realm`]); otherwise the default one.
    pub layers: Option<RenderLayers>,
}

impl Terrain {
    /// The terrain of a circuit, worked out on every core.
    #[cfg(any(test, feature = "testing"))]
    pub fn new(track: &Track, scenery: &Scenery) -> Self {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        Self::on_threads(track, scenery, cores)
    }

    /// The terrain of a circuit, worked out on at most `threads` threads.
    pub fn on_threads(track: &Track, scenery: &Scenery, threads: usize) -> Self {
        let (min, max) = track.bounds();
        Self::shaped_within(track, scenery, threads, (min, max))
    }

    /// The terrain of a circuit, worked out on at most `threads` threads,
    /// fine and shaped by its roads only round `near` (a box, its corners;
    /// [`FINE_MARGIN`] round it): elsewhere it is the land as it would be
    /// with no road, in cells growing coarser away from it. Round the whole
    /// circuit, it is as [`Self::on_threads`] makes it; round a stretch of
    /// it, it is a quick look at that stretch (see `super::preview`).
    pub fn shaped_within(
        track: &Track,
        scenery: &Scenery,
        threads: usize,
        (near_min, near_max): (Vec2, Vec2),
    ) -> Self {
        let (min, max) = track.bounds();
        let center = (min + max) / 2.0;
        let xs = axis(near_min.x - FINE_MARGIN, near_max.x + FINE_MARGIN, center.x);
        let zs = axis(near_min.y - FINE_MARGIN, near_max.y + FINE_MARGIN, center.y);
        // Shaped by the roads within it; its grass as the whole circuit's,
        // wild past the box round the whole of it.
        let shaped = (near_min - FINE_MARGIN, near_max + FINE_MARGIN);
        let fine = (min - FINE_MARGIN, max + FINE_MARGIN);
        let shaper = Shaper::new(track, scenery);
        let tunnels = Tunnels::new(track, scenery);

        let points: Vec<Vec2> = zs
            .iter()
            .flat_map(|&z| xs.iter().map(move |&x| Vec2::new(x, z)))
            .collect();
        let heights_of = |part: &[Vec2]| {
            part.iter()
                .map(|&point| {
                    let inside = point.cmpge(shaped.0).all() && point.cmple(shaped.1).all();
                    if !inside {
                        return shaper.height_at(point, None);
                    }
                    // Under the road that passes over a tunnel, the ground
                    // stays down at the tunnel, under its rock (indoors, what
                    // passes over stands on its own, and the floor is flat).
                    if scenery.soil != Soil::Floor
                        && let Some(tunnel) = tunnels.beneath(track, point, BURIED)
                    {
                        return track.height_beside(&tunnel) - UNDER_ROAD;
                    }
                    shaper.height_at(point, Some(&track.nearest(point)))
                })
                .collect::<Vec<_>>()
        };
        // Lo más caro es buscar la carretera más cercana a cada punto: se
        // reparte entre los núcleos del procesador.
        let heights = on_cores(&points, threads, heights_of);
        let fields = scenery.flowers;
        let flowers = (!fields.is_empty()).then(|| FlowerMap::new(track, fields));
        // And what the ground looks like at each point, but for the way it
        // faces: the flowers in it are worked out with the roads near too.
        let grid: Vec<Vec3> = points
            .iter()
            .zip(&heights)
            .map(|(point, &height)| Vec3::new(point.x, height, point.y))
            .collect();
        let mountain: Vec<f32> = points
            .iter()
            .map(|&point| smoothstep(2.0, 40.0, scenery.mountains.height(point)))
            .collect();
        let hollow = hollows(&xs, &zs, &heights, &mountain);
        let up = ups(&xs, &zs, &heights);
        let grid: Vec<(Vec3, f32, f32, f32)> = grid
            .into_iter()
            .zip(hollow.iter().zip(&mountain).zip(&up))
            .map(|(p, ((&hollow, &mountain), &up))| (p, hollow, mountain, up))
            .collect();
        let soil = scenery.soil;
        let turf = scenery.turf;
        let sites = on_cores(&grid, threads, |part: &[(Vec3, f32, f32, f32)]| {
            part.iter()
                .map(|&(p, hollow, mountain, up)| {
                    site(p, soil, flowers.as_ref(), turf, hollow, mountain, up)
                })
                .collect::<Vec<_>>()
        });
        Self {
            xs,
            zs,
            heights,
            sites,
            soil,
            flowers,
            turf,
            rock_tints: scenery.rock_tints,
            fine,
            mountain,
            hollow,
            up,
            greens: scenery.greens,
            grass_shade: scenery.grass_shade,
            lakes: shaper.dams,
        }
    }

    /// How many points its grid has.
    #[cfg(any(test, feature = "testing"))]
    pub fn grid_points(&self) -> usize {
        self.xs.len() * self.zs.len()
    }

    /// The height of the water at `point` or by it: a dam's lake's level in
    /// it and on the rim round it, the sea's elsewhere. What is planted
    /// stands clear over it, not in a lake's water.
    pub fn water_level(&self, point: Vec2) -> f32 {
        self.lakes
            .iter()
            .filter_map(|lake| lake.level_by(point))
            .fold(SEA_LEVEL, f32::max)
    }

    /// Altura del terreno en un punto (interpolada en la rejilla).
    pub fn height(&self, point: Vec2) -> f32 {
        self.on_grid(&self.heights, point)
    }

    /// One of the values kept for each point of the grid, at a point
    /// between them (interpolated).
    fn on_grid(&self, values: &[f32], point: Vec2) -> f32 {
        let (i, tx) = locate(&self.xs, point.x);
        let (j, tz) = locate(&self.zs, point.y);
        let at = |i: usize, j: usize| values[j * self.xs.len() + i];
        let bottom = at(i, j).lerp(at(i + 1, j), tx);
        let top = at(i, j + 1).lerp(at(i + 1, j + 1), tx);
        bottom.lerp(top, tz)
    }

    /// Where the earth rises beside the road, at each point of the centre
    /// line, to the left and to the right (m from it): the first spot, going
    /// out from the edge of the asphalt, where the terrain as drawn (the
    /// triangles of its grid, which can poke inside the walls next to a rock
    /// face) stands [`EARTH_RISE`] above the road, or [`WALL_DISTANCE`] if it
    /// does not before the walls. Looks half way to the next points too, so
    /// a bump between two of them is not missed. The lap first, then each
    /// road off it, short of its own walls. See [`Track::set_edges`].
    pub fn track_edges(&self, track: &Track) -> Vec<Vec<[f32; 2]>> {
        std::iter::once(track)
            .chain(track.side_roads())
            .map(|road| self.road_edges(road))
            .collect()
    }

    /// Like [`Self::track_edges`], for one road.
    fn road_edges(&self, track: &Track) -> Vec<[f32; 2]> {
        let n = track.sample_count();
        let steps = ((WALL_DISTANCE - track.half_width()) / EDGE_STEP).ceil() as usize;
        (0..n)
            .map(|index| {
                let wall = track.wall_at(index);
                [1.0, -1.0].map(|side| {
                    (0..=steps)
                        .map(|k| (track.half_width() + k as f32 * EDGE_STEP).min(wall))
                        .find(|&lateral| {
                            [-1, 0, 1].iter().any(|&offset| {
                                let other = index as isize + offset;
                                let other = if track.is_closed() {
                                    other.rem_euclid(n as isize) as usize
                                } else {
                                    other.clamp(0, n as isize - 1) as usize
                                };
                                let here = track.surface_point(index, side * lateral);
                                let there = track.surface_point(other, side * lateral);
                                // Here and half way to the neighbour.
                                [here, (here + there) / 2.0].iter().any(|road| {
                                    self.height(Vec2::new(road.x, road.z)) - road.y > EARTH_RISE
                                })
                            })
                        })
                        .unwrap_or(wall)
                })
            })
            .collect()
    }

    /// Normal del terreno en un punto.
    pub fn normal(&self, point: Vec2) -> Vec3 {
        const D: f32 = 1.5;
        let dx = self.height(point + Vec2::X * D) - self.height(point - Vec2::X * D);
        let dz = self.height(point + Vec2::Y * D) - self.height(point - Vec2::Y * D);
        Vec3::new(-dx, 2.0 * D, -dz).normalize()
    }

    pub fn ground(&self, point: Vec2) -> Ground {
        let ground = classify(
            Vec3::new(point.x, self.height(point), point.y),
            self.normal(point),
        );
        if self.soil == Soil::Lunar && !is_earth_realm(point) && ground != Ground::Rock {
            Ground::Sand
        } else {
            ground
        }
    }

    /// The triangle of the terrain's meshes (see [`Self::meshes`]) a point
    /// is in, and the indices of its corners in the grid.
    fn triangle_at(&self, point: Vec2) -> ([Vec3; 3], [usize; 3]) {
        let (i, tx) = locate(&self.xs, point.x);
        let (j, tz) = locate(&self.zs, point.y);
        let width = self.xs.len();
        let corners = [(i, j), (i, j + 1), (i + 1, j), (i + 1, j + 1)];
        // The same two triangles per cell as the meshes, split along the
        // diagonal from (i + 1, j) to (i, j + 1).
        let triangle = if tx + tz <= 1.0 { [0, 1, 2] } else { [2, 1, 3] };
        let indices = triangle.map(|k| corners[k].1 * width + corners[k].0);
        let points = triangle.map(|k| {
            let (i, j) = corners[k];
            Vec3::new(self.xs[i], self.heights[j * width + i], self.zs[j])
        });
        (points, indices)
    }

    /// The kind of ground drawn at a point (see [`Self::look_drawn`]).
    pub fn ground_drawn(&self, point: Vec2) -> Ground {
        self.look_drawn(point).0
    }

    /// What the ground is drawn as at a point: the look of the piece of the
    /// terrain's meshes it is in (see [`Self::meshes`]), which is what the
    /// player sees there.
    fn look_drawn(&self, point: Vec2) -> Look {
        let (corners, indices) = self.triangle_at(point);
        let looks = indices.map(|index| look(&self.sites[index], None));
        let flat = corners.map(|corner| Vec2::new(corner.x, corner.z));
        // The piece it is furthest inside of: on a border, either.
        let mut drawn = (f32::MIN, looks[0]);
        for (piece, weights) in self.cut(corners, looks, None).into_iter().flatten() {
            let piece_corners = weights.map(|w| flat[0] * w.x + flat[1] * w.y + flat[2] * w.z);
            let depth = inside(point, piece_corners);
            if depth > drawn.0 {
                drawn = (depth, piece);
            }
        }
        drawn.1
    }

    /// What the look of the ground at `p` (x, height, z) turns on but the
    /// way it faces (see [`look`]).
    fn site(&self, p: Vec3) -> Site {
        let point = Vec2::new(p.x, p.z);
        site(
            p,
            self.soil,
            self.flowers.as_ref(),
            self.turf,
            self.on_grid(&self.hollow, point),
            self.on_grid(&self.mountain, point),
            self.on_grid(&self.up, point),
        )
    }

    /// The pieces a triangle of the grid is drawn in (`corners`, facing
    /// `face`, its corners looking `looks`): whole, if its corners look the
    /// same; otherwise cut along the borders between the looks, found along
    /// its edges, so that they run smoothly across the land instead of
    /// round whole triangles, in steps. Each piece has its look and its
    /// corners, as weights of the triangle's, turning the same way. The
    /// looks are those of the ground drawn as `style` says (see [`look`]).
    fn cut(&self, corners: [Vec3; 3], looks: [Look; 3], style: Option<Realm>) -> Pieces {
        const ONE: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];
        let mut pieces = Pieces::default();
        if looks[0] == looks[1] && looks[1] == looks[2] {
            pieces[0] = Some((looks[0], ONE));
            return pieces;
        }
        // Where the look changes along the edge from corner k to corner l.
        let edge = |k: usize, l: usize| {
            let t = self.border([corners[k], corners[l]], [looks[k], looks[l]], style);
            ONE[k].lerp(ONE[l], t)
        };
        match (0..3).find(|&k| looks[(k + 1) % 3] == looks[(k + 2) % 3]) {
            // One corner looks unlike the other two: a triangle cut off it.
            Some(k) => {
                let (l, m) = ((k + 1) % 3, (k + 2) % 3);
                let (kl, km) = (edge(k, l), edge(k, m));
                pieces[0] = Some((looks[k], [ONE[k], kl, km]));
                pieces[1] = Some((looks[l], [kl, ONE[l], ONE[m]]));
                pieces[2] = Some((looks[l], [kl, ONE[m], km]));
            }
            // All three unlike: each corner's up to the middle.
            None => {
                let middle = Vec3::splat(1.0 / 3.0);
                let sides = [edge(0, 1), edge(1, 2), edge(2, 0)];
                for (k, piece) in pieces.chunks_mut(2).enumerate() {
                    let (after, before) = (sides[k], sides[(k + 2) % 3]);
                    piece[0] = Some((looks[k], [ONE[k], after, middle]));
                    piece[1] = Some((looks[k], [ONE[k], middle, before]));
                }
            }
        }
        pieces
    }

    /// How far along the edge from `a` to `b` (0 to 1) its look changes
    /// from `from`, at `a`, to `to`, at `b`: found by halving it, the same
    /// whichever way the edge is walked, so that the triangles either side
    /// of it agree.
    fn border(&self, [a, b]: [Vec3; 2], [from, to]: [Look; 2], style: Option<Realm>) -> f32 {
        if (b.x, b.z) < (a.x, a.z) {
            return 1.0 - self.border([b, a], [to, from], style);
        }
        let (mut near, mut far) = (0.0, 1.0);
        for _ in 0..BORDER_STEPS {
            let t = (near + far) / 2.0;
            if look(&self.site(a.lerp(b, t)), style) == from {
                near = t;
            } else {
                far = t;
            }
        }
        (near + far) / 2.0
    }

    /// The flowers the fields have at a point, if any, whatever road runs
    /// there (see [`FlowerMap::field_at`]).
    pub fn field_flowers(&self, point: Vec2) -> Option<FlowerKind> {
        self.flowers.as_ref()?.field_at(point)
    }

    /// The colour of grass in flower with `kind` at `p` (x, height, z),
    /// facing `normal`, as a tint over the meadow's texture (linear RGBA):
    /// moss phlox in its drifts; bare earth and fallen leaves in their own
    /// colours, a little lighter or darker here and there, as light as the
    /// grass is; or the grass's greens, gentler, so the flowers stay bright.
    pub fn meadow_color(&self, p: Vec3, normal: Vec3, kind: FlowerKind) -> [f32; 4] {
        if kind == FlowerKind::Phlox {
            return phlox_color(Vec2::new(p.x, p.z));
        }
        if matches!(kind, FlowerKind::BareEarth | FlowerKind::FallenLeaves) {
            let mottle = 0.85 + 0.3 * fbm(Vec2::new(p.x, p.z) / 23.0 + Vec2::splat(13.0), 2);
            let light = mottle * (1.0 + self.grass_shade) / 2.0;
            return [light, light, light, 1.0];
        }
        shaded(
            self.grass_tint(p, normal, 0.0).mix(&Color::WHITE, 0.6),
            (1.0 + self.grass_shade) / 2.0,
        )
    }

    /// The colour of the grass at `p` (x, height, z), facing `normal`, as a
    /// tint over its texture (linear RGBA): its greens (see [`grass_tint`]),
    /// as light as the circuit's grass is.
    pub fn grass_color(&self, p: Vec3, normal: Vec3) -> [f32; 4] {
        shaded(self.grass_tint(p, normal, 0.0), self.grass_shade)
    }

    /// The colour of `ground` at `p`, facing `normal`, as a tint over its
    /// texture (linear RGBA), as the circuit's soil draws it: a desert
    /// canyon's earth and rock, the Moon's regolith (outside the Earth portal
    /// realm), the checkerboard earth's, or the grass's greens (the flowers
    /// and the weathering of the mountains aside).
    pub fn ground_color(&self, ground: Ground, p: Vec3, normal: Vec3) -> [f32; 4] {
        let earth = self.soil == Soil::Lunar && is_earth_realm(Vec2::new(p.x, p.z));
        match (self.soil, ground) {
            (Soil::Floor, _) => floor_vertex_color(p),
            (Soil::Mine, _) => canyon_vertex_color(ground, p, normal),
            (Soil::Lunar, _) if !earth => lunar_vertex_color(ground, p, normal),
            (Soil::Checkered, Ground::Rock) => soil_vertex_color(p, normal),
            (_, Ground::Grass) => self.grass_color(p, normal),
            _ => ground_vertex_color(ground, p, normal),
        }
    }

    /// The tint of the grass at `p`, `wild` as it is far from the roads
    /// (see [`grass_tint`]), by the circuit's greens.
    fn grass_tint(&self, p: Vec3, normal: Vec3, wild: f32) -> Color {
        greens_tint(self.greens, p, normal, wild)
    }

    /// Whether any of the ground is in flower.
    pub fn in_flower(&self) -> bool {
        self.flowers.is_some()
    }

    /// The flowers the ground is drawn in at a point, if it is in flower
    /// (see [`Self::look_drawn`]).
    pub fn flowers_drawn(&self, point: Vec2) -> Option<FlowerKind> {
        self.look_drawn(point).1
    }

    /// The ground's meshes over the cells in rows `cells` of the grid (see
    /// [`Self::meshes`]), its points' normals given: their vertices shared
    /// within it. Drawn as `style` says (see [`Self::meshes_as`]), and only
    /// the triangles of the grid on `side`, if given.
    fn band(
        &self,
        cells: std::ops::Range<usize>,
        normals: &[Vec3],
        style: Option<Realm>,
        side: Option<Realm>,
    ) -> Band {
        let width = self.xs.len();
        let position =
            |i: usize, j: usize| Vec3::new(self.xs[i], self.heights[j * width + i], self.zs[j]);
        // Its points, from its first row to the one after its last.
        let first = cells.start * width;
        let band_points = (cells.len() + 1) * width;
        let mut grass = Builder::shared(band_points);
        let mut sand = Builder::shared(band_points);
        let mut rock = Builder::default();
        let mut snow = Builder::shared(band_points);
        let mut meadows: Vec<(FlowerKind, Builder)> = Vec::new();
        for j in cells {
            for i in 0..width - 1 {
                let corners = [(i, j), (i, j + 1), (i + 1, j), (i + 1, j + 1)];
                // Dos triángulos por celda, en sentido antihorario visto desde arriba.
                for triangle in [[0, 1, 2], [2, 1, 3]] {
                    let indices = triangle.map(|k| corners[k].1 * width + corners[k].0);
                    let points = triangle.map(|k| position(corners[k].0, corners[k].1));
                    let [a, b, c] = points;
                    if side.is_some_and(|side| Realm::of_triangle(points) != side) {
                        continue;
                    }
                    let face = (b - a).cross(c - a).normalize();
                    let looks = indices.map(|index| look(&self.sites[index], style));
                    for (look, weights) in self.cut(points, looks, style).into_iter().flatten() {
                        let (ground_type, flowers) = look;
                        let builder = match look {
                            (Ground::Grass, Some(kind)) => {
                                let meadow = match meadows.iter().position(|(k, _)| *k == kind) {
                                    Some(meadow) => meadow,
                                    None => {
                                        meadows.push((kind, Builder::shared(band_points)));
                                        meadows.len() - 1
                                    }
                                };
                                &mut meadows[meadow].1
                            }
                            (Ground::Grass, None) => &mut grass,
                            (Ground::Sand, _) => &mut sand,
                            (Ground::Rock, _) => &mut rock,
                            (Ground::Snow, _) => &mut snow,
                        };
                        let shared = builder.shared_vertices.is_some();
                        for w in weights {
                            // A point of the grid, or one on a border.
                            let grid = (0..3).find(|&k| w[k] == 1.0).map(|k| indices[k] - first);
                            builder.push(grid, || {
                                let blend = |values: [Vec3; 3]| {
                                    values[0] * w.x + values[1] * w.y + values[2] * w.z
                                };
                                let p = blend(points);
                                let normal = blend(indices.map(|index| normals[index])).normalize();
                                let mountain =
                                    blend(indices.map(|index| Vec3::splat(self.mountain[index]))).x;
                                let hollow =
                                    blend(indices.map(|index| Vec3::splat(self.hollow[index]))).x;
                                let uv = match shared {
                                    // Floorboards lie straight.
                                    true if self.soil == Soil::Floor => Vec2::new(p.x, p.z),
                                    true => {
                                        let warp = Vec2::new(
                                            fbm(Vec2::new(p.x, p.z) * 0.035, 2) - 0.5,
                                            fbm(Vec2::new(p.z + 41.0, p.x - 29.0) * 0.035, 2) - 0.5,
                                        ) * 2.2;
                                        Vec2::new(p.x + warp.x, p.z + warp.y)
                                    }
                                    // Checkerboard earth: straight rows at the same
                                    // heights on every cliff, as on an arcade island.
                                    false if self.soil == Soil::Checkered => {
                                        rock_projection(p, face)
                                    }
                                    false => {
                                        // Continuous 3D domain warp based on world coordinates before projection.
                                        // Warping in 3D breaks world-elevation synchronicity and introduces
                                        // natural undulating strata following the mountain curvature.
                                        let warp_x = fbm(
                                            Vec2::new(p.z * 0.022 + 13.0, p.y * 0.032 - 7.0),
                                            3,
                                        ) - 0.5;
                                        let warp_y = fbm(
                                            Vec2::new(p.x * 0.025 - 29.0, p.z * 0.025 + 41.0),
                                            3,
                                        ) - 0.5;
                                        let warp_z = fbm(
                                            Vec2::new(p.x * 0.022 + 37.0, p.y * 0.032 + 19.0),
                                            3,
                                        ) - 0.5;
                                        let warped_p =
                                            p + Vec3::new(warp_x * 9.0, warp_y * 7.5, warp_z * 9.0);

                                        let proj = rock_projection(warped_p, face);

                                        // Apply 14-degree geological dip angle so rock layers follow a realistic tilt
                                        let dip_angle = 0.244f32;
                                        let cos_dip = dip_angle.cos();
                                        let sin_dip = dip_angle.sin();
                                        let tilted_u = proj.x * cos_dip - proj.y * sin_dip;
                                        let tilted_v = proj.x * sin_dip + proj.y * cos_dip;

                                        let micro_warp = Vec2::new(
                                            fbm(Vec2::new(tilted_u, tilted_v) * 0.05, 2) - 0.5,
                                            fbm(
                                                Vec2::new(tilted_v + 50.0, tilted_u - 30.0) * 0.05,
                                                2,
                                            ) - 0.5,
                                        ) * 2.5;

                                        Vec2::new(tilted_u + micro_warp.x, tilted_v + micro_warp.y)
                                    }
                                };
                                let earth = self.soil == Soil::Lunar
                                    && style.map_or(is_earth_realm(Vec2::new(p.x, p.z)), |style| {
                                        style == Realm::Earth
                                    });
                                let color = match (self.soil, ground_type, flowers) {
                                    (Soil::Floor, ..) => floor_vertex_color(p),
                                    (_, _, Some(kind)) => self.meadow_color(p, normal, kind),
                                    (Soil::Checkered, Ground::Rock, _) => {
                                        soil_vertex_color(p, normal)
                                    }
                                    (Soil::Mine, ..) => canyon_vertex_color(ground_type, p, normal),
                                    (Soil::Granite, Ground::Rock, _) => {
                                        let turf = blend(indices.map(|index| {
                                            Vec3::splat(self.sites[index].turf.map_or(0.0, |_| 1.0))
                                        }))
                                        .x;
                                        let color = granite_vertex_color(p, normal, turf);
                                        tinted(color, p, self.rock_tints)
                                    }
                                    (Soil::Lunar, ..) if !earth => {
                                        lunar_vertex_color(ground_type, p, normal)
                                    }
                                    (_, Ground::Grass, _) => {
                                        let outside = (self.fine.0 - Vec2::new(p.x, p.z))
                                            .max(Vec2::new(p.x, p.z) - self.fine.1)
                                            .max_element();
                                        let wild =
                                            smoothstep(0.0, WILD_REACH, outside).max(mountain);
                                        shaded(self.grass_tint(p, normal, wild), self.grass_shade)
                                    }
                                    _ => ground_vertex_color(ground_type, p, normal),
                                };
                                let color = if mountain > 0.0 {
                                    weathered(color, ground_type, p, hollow, mountain)
                                } else {
                                    color
                                };
                                (p, normal, uv, color)
                            });
                        }
                    }
                }
            }
        }
        Band {
            grass,
            sand,
            rock,
            snow,
            meadows,
        }
    }

    /// What the steep faces are made of.
    pub fn soil(&self) -> Soil {
        self.soil
    }

    /// The ground's meshes, by kind of ground, and the grass in flower by
    /// the flowers it is in, worked out on `threads` threads, in bands of
    /// the grid's rows (see [`Self::band`]) joined in order, the same
    /// however many. Pure data: they can be worked out away from the main
    /// thread.
    pub fn meshes(&self, threads: usize) -> Vec<GroundMesh> {
        self.meshes_as(threads, None, None)
    }

    /// The ground's meshes as [`Self::meshes`] makes them, but, on a circuit
    /// split between the Earth and the Moon (see [`super::realm`]), all of
    /// it drawn as the ground of `style` (Earth's grass or the Moon's
    /// regolith) wherever it lies, and only the part of it on `side`: what
    /// one realm sees of its own ground, or of the land beyond its border
    /// that only the other realm's riders are on. Each is drawn on the
    /// layers that realm's cameras see.
    pub fn meshes_as(
        &self,
        threads: usize,
        style: Option<Realm>,
        side: Option<Realm>,
    ) -> Vec<GroundMesh> {
        let (width, depth) = (self.xs.len(), self.zs.len());
        let position =
            |i: usize, j: usize| Vec3::new(self.xs[i], self.heights[j * width + i], self.zs[j]);
        let rows: Vec<usize> = (0..depth).collect();
        let normals: Vec<Vec3> = on_cores(&rows, threads, |rows| {
            rows.iter()
                .flat_map(|&j| (0..width).map(move |i| (i, j)))
                .map(|(i, j)| {
                    let (left, right) = (i.saturating_sub(1), (i + 1).min(width - 1));
                    let (back, front) = (j.saturating_sub(1), (j + 1).min(depth - 1));
                    let along_x = position(right, j) - position(left, j);
                    let along_z = position(i, front) - position(i, back);
                    along_z.cross(along_x).normalize()
                })
                .collect()
        });
        // The bands dealt out to the threads in turn, so that each gets some
        // of the dense middle of the grid and some of its sparse edges.
        let cells = depth - 1;
        let count = cells.div_ceil(BAND_ROWS);
        let threads = threads.clamp(1, count.max(1));
        let order: Vec<usize> = (0..threads)
            .flat_map(|first| (first..count).step_by(threads))
            .collect();
        let mut bands = on_cores(&order, threads, |part| {
            part.iter()
                .map(|&band| {
                    let rows = band * BAND_ROWS..((band + 1) * BAND_ROWS).min(cells);
                    (band, self.band(rows, &normals, style, side))
                })
                .collect()
        });
        bands.sort_by_key(|(band, _)| *band);
        let mut joined = Band::default();
        for (_, band) in bands {
            joined.append(band);
        }
        let Band {
            grass,
            sand,
            rock,
            snow,
            meadows,
        } = joined;
        let rock_tile = match self.soil {
            Soil::Rock | Soil::Mine | Soil::Granite | Soil::Lunar | Soil::Floor => ROCK_TILE,
            Soil::Checkered => SOIL_TILE,
        };
        let grass_tile = match self.soil {
            Soil::Floor => FLOOR_TILE,
            _ => GRASS_TILE,
        };
        let layers = style.map(|style| match side {
            Some(side) if side != style => style.beyond_layers(),
            _ => style.layers(),
        });
        let ground = |ground: Ground, flowers: Option<FlowerKind>, mesh: Mesh| GroundMesh {
            ground,
            flowers,
            mesh,
            layers: layers.clone(),
        };
        let mut meshes = vec![
            ground(Ground::Grass, None, grass.build(grass_tile)),
            ground(Ground::Sand, None, sand.build(SAND_TILE)),
            ground(Ground::Rock, None, rock.build(rock_tile)),
            ground(Ground::Snow, None, snow.build(GRASS_TILE)),
        ];
        meshes.extend(
            meadows
                .into_iter()
                .map(|(kind, meadow)| ground(Ground::Grass, Some(kind), meadow.build(GRASS_TILE))),
        );
        meshes
    }
}

/// The ground, from its meshes ([`Terrain::meshes`]). The sea is apart
/// ([`super::sea`]).
pub fn spawn_terrain(
    world: &mut ChildSpawnerCommands,
    terrain: &Terrain,
    ground_meshes: Vec<GroundMesh>,
    meadows: &[(FlowerKind, Handle<Image>)],
    textures: &WorldTextures,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    for GroundMesh {
        ground,
        flowers,
        mesh,
        layers,
    } in ground_meshes
    {
        if mesh.count_vertices() == 0 {
            continue;
        }
        let (texture, roughness, reflectance) = match flowers {
            // Grass in flower: the lawn covered in blossoms.
            Some(_) if ground == Ground::Grass => {
                let meadow = meadows
                    .iter()
                    .find(|(kind, _)| Some(*kind) == flowers)
                    .map(|(_, meadow)| meadow)
                    .expect("a meadow for each kind of flower drawn");
                (Some(meadow), 0.9, 0.18)
            }
            _ => ground_look(ground, terrain.soil, textures),
        };
        let mut piece = world.spawn((
            GroundPiece,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: texture.cloned(),
                perceptual_roughness: roughness,
                reflectance,
                ..default()
            })),
        ));
        if let Some(layers) = layers {
            piece.insert(layers);
        }
    }
}

/// What `ground` (not in flower) on soil `soil` is drawn with: its texture,
/// how rough it is and how much it reflects.
pub fn ground_look(
    ground: Ground,
    soil: Soil,
    textures: &WorldTextures,
) -> (Option<&Handle<Image>>, f32, f32) {
    match (ground, soil) {
        // In a desert canyon the flat ground is dry dust.
        (Ground::Grass, Soil::Mine) => (Some(&textures.sand), 0.95, 0.15),
        // Indoors, varnished floorboards.
        (Ground::Grass, Soil::Floor) => (Some(&textures.floor), 0.6, 0.35),
        (Ground::Grass, _) => (Some(&textures.grass), 0.90, 0.18),
        (Ground::Sand, _) => (Some(&textures.sand), 0.88, 0.28),
        (Ground::Rock, Soil::Rock | Soil::Mine | Soil::Lunar | Soil::Floor) => {
            (Some(&textures.rock), 0.78, 0.42)
        }
        (Ground::Rock, Soil::Checkered) => (Some(&textures.soil), 0.92, 0.2),
        (Ground::Rock, Soil::Granite) => (Some(&textures.granite), 0.82, 0.4),
        // Snow, white but for its shading (see `ground_vertex_color`).
        (Ground::Snow, _) => (None, 0.55, 0.35),
    }
}

/// The ground at `point` (x, height, z), where it faces `normal`.
fn classify(point: Vec3, normal: Vec3) -> Ground {
    let (snow_line, tree_line) = lines(point, 0.0);
    ground_of(point.y, snow_line, tree_line, normal, 0.0, None)
}

/// The snow line and the tree line over `point` (m), reaching up and down
/// along the ravines (see [`SNOW_LINE`] and [`TREE_LINE`]); `hollow` down
/// a gully (or, below 0, up a ridge) of a mountain across the sea, where
/// the snow lies on lower and the forest climbs higher.
fn lines(point: Vec3, hollow: f32) -> (f32, f32) {
    let ground = Vec2::new(point.x, point.z);
    (
        SNOW_LINE + (fbm(ground / 90.0, 2) - 0.5) * 2.0 * SNOW_REACH - SNOW_IN_GULLIES * hollow,
        TREE_LINE
            + (fbm(ground / 70.0 + 9.0, 2) - 0.5) * 2.0 * TREE_LINE_REACH
            + TREES_IN_GULLIES * hollow,
    )
}

/// How much lower the snow lies down a gully (m), and how much higher the
/// forest climbs it (and on a ridge, the other way).
const SNOW_IN_GULLIES: f32 = 120.0;
const TREES_IN_GULLIES: f32 = 70.0;
/// On the mountains across the sea, faces steeper than this (the normal's
/// height) are rock even under the lines: only sheer ones, the grid there
/// being too coarse for the steepness of each triangle to tell.
const MOUNTAIN_CLIFF_NORMAL_Y: f32 = 0.45;
/// How a gully or a ridge shows in the grid of a mountain: how far a point
/// lies under (or over) the middle of its neighbours, for the spacing.
const HOLLOW_SCALE: f32 = 0.8;

/// For each point of the grid (by rows), on the mountains across the sea
/// (`mountain`), how far it lies down a gully, up to 1, or up on a ridge,
/// down to -1: how far under the middle of its neighbours (see
/// [`Terrain::hollow`]).
fn hollows(xs: &[f32], zs: &[f32], heights: &[f32], mountain: &[f32]) -> Vec<f32> {
    let (width, depth) = (xs.len(), zs.len());
    (0..depth)
        .flat_map(|j| (0..width).map(move |i| (i, j)))
        .map(|(i, j)| {
            let index = j * width + i;
            if mountain[index] <= 0.0 || i == 0 || j == 0 || i + 1 == width || j + 1 == depth {
                return 0.0;
            }
            let at = |i: usize, j: usize| heights[j * width + i];
            let under =
                (at(i - 1, j) + at(i + 1, j) + at(i, j - 1) + at(i, j + 1)) / 4.0 - at(i, j);
            let spacing = (xs[i + 1] - xs[i - 1] + zs[j + 1] - zs[j - 1]) / 4.0;
            (under / spacing * HOLLOW_SCALE * 4.0).clamp(-1.0, 1.0) * mountain[index]
        })
        .collect()
}

/// What a piece of ground is drawn as: its kind and, on grass in a field
/// of flowers, the flowers.
type Look = (Ground, Option<FlowerKind>);

/// The pieces of a triangle of the grid (see [`Terrain::cut`]): at most
/// six, each with its look and its corners as weights of the triangle's.
type Pieces = [Option<(Look, [Vec3; 3])>; 6];

/// How many times [`Terrain::border`] halves an edge looking for a border
/// along it: to within a 32nd of it.
const BORDER_STEPS: u32 = 5;
/// How much more or less steep than it is the ground is taken to be here
/// and there (of the upward part of its normal), so the rock of the steep
/// faces ends raggedly, not along a contour.
const CLIFF_RAGGED: f32 = 0.05;

/// How much the ground faces up at each point of the grid (`xs` by `zs`,
/// `heights` by rows): the upward part of its normal, from the points
/// either side of it, as the meshes' normals are.
fn ups(xs: &[f32], zs: &[f32], heights: &[f32]) -> Vec<f32> {
    let (width, depth) = (xs.len(), zs.len());
    let position = |i: usize, j: usize| Vec3::new(xs[i], heights[j * width + i], zs[j]);
    (0..depth)
        .flat_map(|j| (0..width).map(move |i| (i, j)))
        .map(|(i, j)| {
            let (left, right) = (i.saturating_sub(1), (i + 1).min(width - 1));
            let (back, front) = (j.saturating_sub(1), (j + 1).min(depth - 1));
            let along_x = position(right, j) - position(left, j);
            let along_z = position(i, front) - position(i, back);
            along_z.cross(along_x).normalize().y
        })
        .collect()
}

/// Whether a world-space point `(x, z)` on a `Soil::Lunar` circuit lies inside
/// the Earth realm (the main western and central sector containing the start
/// line and space center, west of the two interdimensional portals at
/// `(764, -430)` and `(658, -300)`).
pub fn is_earth_realm(point: Vec2) -> bool {
    let t = ((point.y + 430.0) / 130.0).clamp(0.0, 1.45);
    let boundary_x = 764.0 - 106.0 * t;
    point.x < boundary_x
}

/// What the look of the ground at a point turns on but the way it faces:
/// how high it is, the snow line and the tree line over it, and the flowers
/// it is in, if grass.
#[derive(Clone, Copy)]
struct Site {
    height: f32,
    snow_line: f32,
    tree_line: f32,
    flowers: Option<FlowerKind>,
    /// How much of it is mountain across the sea.
    mountain: f32,
    /// Whether it is turf, grass steeper than elsewhere: how steep (see
    /// [`Turf::steepest`]).
    turf: Option<f32>,
    /// How much its ground faces up (the upward part of its normal,
    /// smoothed over the grid), a little more or less here and there, so
    /// the rock of the steep faces does not end along a contour.
    up: f32,
    /// Whether this point lies on the Moon's airless regolith (no grass or snow).
    lunar: bool,
}

/// What the look of the ground at `p` (x, height, z) turns on but the way
/// it faces, where the grass is in `flowers` and is `turf`, `hollow` down a
/// gully of a mountain across the sea and `mountain` its share of it.
fn site(
    p: Vec3,
    soil: Soil,
    flowers: Option<&FlowerMap>,
    turf: &[Turf],
    hollow: f32,
    mountain: f32,
    up: f32,
) -> Site {
    let (snow_line, tree_line) = lines(p, hollow);
    let ground = Vec2::new(p.x, p.z);
    Site {
        height: p.y,
        snow_line,
        tree_line,
        flowers: flowers.and_then(|map| map.at(ground)),
        mountain,
        turf: turf
            .iter()
            .find(|turf| ground.distance(turf.center) < turf.radius)
            .map(|turf| turf.steepest),
        up: up + CLIFF_RAGGED * 2.0 * (fbm(ground / 13.0 + Vec2::splat(5.0), 2) - 0.5),
        lunar: soil == Soil::Lunar && !is_earth_realm(ground),
    }
}

/// `work` done over `items` on at most `threads` threads, a share of them
/// each, its results in order. On the web there are no threads: all on
/// one.
pub(super) fn on_cores<T: Sync, R: Send>(
    items: &[T],
    threads: usize,
    work: impl Fn(&[T]) -> Vec<R> + Sync,
) -> Vec<R> {
    if cfg!(target_arch = "wasm32") || threads <= 1 {
        return work(items);
    }
    let chunk = items.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let workers: Vec<_> = items
            .chunks(chunk)
            .map(|part| scope.spawn(|| work(part)))
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("el cálculo del terreno no falla"))
            .collect()
    })
}

/// What the ground at `site` is drawn as: by its smoothed slope, so the
/// borders of the cliffs run smoothly too, as the others do.
/// Drawn as `style` says on a circuit split between the Earth and the Moon
/// (see [`Terrain::meshes_as`]): its ground all of one realm.
fn look(site: &Site, style: Option<Realm>) -> Look {
    let mut ground = ground_of(
        site.height,
        site.snow_line,
        site.tree_line,
        Vec3::Y * site.up,
        site.mountain,
        site.turf,
    );
    let lunar = style.map_or(site.lunar, |style| style == Realm::Moon);
    if lunar && ground != Ground::Rock {
        ground = Ground::Sand;
    }
    let flowers = if ground == Ground::Grass {
        site.flowers
    } else {
        None
    };
    (ground, flowers)
}

/// How far `point` is inside the triangle `corners` (x, z): the least of
/// its barycentric weights, below 0 outside.
fn inside(point: Vec2, [p, q, r]: [Vec2; 3]) -> f32 {
    let area = (q - p).perp_dot(r - p);
    if area.abs() < 1e-6 {
        return f32::MIN;
    }
    let u = (q - point).perp_dot(r - point) / area;
    let v = (r - point).perp_dot(p - point) / area;
    u.min(v).min(1.0 - u - v)
}

/// The ground `height` m up, under the snow line and the tree line there,
/// where it faces `normal`; on a mountain across the sea (`mountain` over
/// a half), by the lines alone, but for sheer faces: snow however steep,
/// and forest up to the tree line; on `turf`, grass up to as steep as it
/// says (see [`Turf::steepest`]).
fn ground_of(
    height: f32,
    snow_line: f32,
    tree_line: f32,
    normal: Vec3,
    mountain: f32,
    turf: Option<f32>,
) -> Ground {
    let (snow_normal, cliff_normal) = if mountain > 0.5 {
        (0.0, MOUNTAIN_CLIFF_NORMAL_Y)
    } else if let Some(steepest) = turf {
        (SNOW_NORMAL_Y, steepest)
    } else {
        (SNOW_NORMAL_Y, CLIFF_NORMAL_Y)
    };
    if height > snow_line && normal.y > snow_normal {
        Ground::Snow
    } else if normal.y < cliff_normal || height > tree_line {
        Ground::Rock
    } else if height < BEACH_HEIGHT {
        Ground::Sand
    } else {
        Ground::Grass
    }
}

/// Lo que da forma al terreno de una pista.
struct Shaper<'a> {
    track: &'a Track,
    scenery: &'a Scenery,
    lap: f32,
    /// Alturas de la carretera repartidas por el circuito, para un campo
    /// suave que no tenga escalones entre tramos a distinta altura.
    road_field: Vec<(Vec2, f32)>,
    /// En una isla, la costa: un polígono alrededor del circuito, y la caja
    /// que lo contiene.
    shore: Option<(Vec<Vec2>, Vec2, Vec2)>,
    /// What the dams do to the ground round them.
    dams: Vec<DamGround>,
}

impl<'a> Shaper<'a> {
    fn new(track: &'a Track, scenery: &'a Scenery) -> Self {
        let road_field = std::iter::once(track)
            .chain(track.side_roads())
            .flat_map(|road| road.centerline().iter().zip(road.elevations()).step_by(4))
            .map(|(&point, &height)| (point, height))
            .collect();
        let lap = track.lap_length();
        let polygon: Option<Vec<Vec2>> = match scenery.coast {
            Coast::Sea(_) | Coast::Inland(_) => None,
            Coast::AroundTrack(margins) => Some(
                (0..track.sample_count())
                    .step_by(2)
                    .map(|index| {
                        let along = index as f32 * track.step_length();
                        track.offset_point(index, interpolate(margins, lap, along))
                    })
                    .collect(),
            ),
            Coast::Outline(points) | Coast::OutlineWithLakes(points, _) => {
                Some(closed_curve(points, SHORE_SPACING))
            }
        };
        let shore = polygon.map(|polygon| {
            let (min, max) = polygon.iter().fold(
                (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                |(min, max), &p| (min.min(p), max.max(p)),
            );
            (polygon, min, max)
        });
        Self {
            track,
            scenery,
            lap,
            road_field,
            shore,
            dams: scenery.dams.iter().map(DamGround::new).collect(),
        }
    }

    /// Distancia (con signo) a la costa: positiva tierra adentro.
    fn coast(&self, point: Vec2) -> f32 {
        match (&self.scenery.coast, &self.shore) {
            (Coast::Sea(waters) | Coast::Inland(waters), _) => waters.distance(point),
            (coast, Some((polygon, min, max))) => {
                // Lejos de la isla, basta con la distancia a la caja.
                let outside = (*min - point).max(point - *max).max(Vec2::ZERO);
                if outside.length() > 150.0 {
                    return -outside.length();
                }
                let sea = signed_distance(polygon, point);
                match coast {
                    Coast::OutlineWithLakes(_, lakes) => sea.min(lakes.distance(point)),
                    _ => sea,
                }
            }
            _ => unreachable!("una isla siempre tiene costa"),
        }
    }

    /// El terreno sin la carretera: playa, fondo del mar y colinas.
    fn natural(&self, point: Vec2) -> f32 {
        let land = self.coast(point);
        let shore = SEA_LEVEL - 4.0 + 5.6 * smoothstep(-30.0, 25.0, land);
        let hills = self.scenery.hills
            * (fbm(point / 160.0, 4) - 0.4).max(0.0)
            * smoothstep(10.0, 80.0, land);
        shore + hills
    }

    fn rocks(&self, point: Vec2) -> f32 {
        self.scenery
            .massifs
            .iter()
            .filter(|massif| !massif.climbed)
            .map(|massif| {
                let reach = point.distance(massif.center) / massif.radius;
                let shape = 1.0 - smoothstep(0.45, 1.0, reach);
                let rugged = 0.65 + 0.7 * fbm(point / 23.0 + massif.center / 100.0, 3);
                massif.height * shape * rugged
            })
            .fold(0.0, f32::max)
    }

    /// The slopes of the mountains the road climbs (see
    /// [`Massif::climbed`]): smooth, with broad tops, a little rough.
    fn slopes(&self, point: Vec2) -> f32 {
        self.scenery
            .massifs
            .iter()
            .filter(|massif| massif.climbed)
            .map(|massif| {
                let reach = point.distance(massif.center) / massif.radius;
                let shape = 1.0 - smoothstep(0.45, 1.0, reach);
                let rugged = 0.88 + 0.24 * fbm(point / 47.0 + massif.center / 100.0, 3);
                massif.height * shape * rugged
            })
            .fold(0.0, f32::max)
    }

    /// The ground at `point` of the open pit round the water, if there is
    /// one, where it would otherwise be at `ground` (see [`pit_ground`]).
    fn pit(&self, point: Vec2, ground: f32) -> f32 {
        match &self.scenery.pit {
            Some(pit) => pit_ground(pit, self.coast(point), ground),
            None => ground,
        }
    }

    /// Altura de las paredes de roca junto a la carretera en un punto que
    /// está a `along` metros de la vuelta.
    fn cutting(&self, nearest: &Nearest, along: f32, point: Vec2) -> f32 {
        self.scenery
            .cuttings
            .iter()
            .filter(|cutting| (nearest.lateral > 0.0) == (cutting.side == Side::Left))
            .map(|cutting| {
                let inside = cutting.span.weight(along, self.lap, 30.0);
                // Una franja de roca de unos 40 m detrás del muro, con la cima llana.
                let band = 1.0 - smoothstep(FLAT + 30.0, FLAT + 75.0, nearest.distance);
                let rugged = 0.75 + 0.5 * fbm(point / 17.0, 3);
                cutting.height * inside * band * rugged
            })
            .fold(0.0, f32::max)
    }

    /// Altura del terreno en un punto. `nearest` es la carretera más cercana,
    /// o `None` si está tan lejos que no importa.
    fn height_at(&self, point: Vec2, nearest: Option<&Nearest>) -> f32 {
        if self.scenery.soil == Soil::Floor {
            return self.floor(nearest);
        }
        let mountains = self.scenery.mountains.height(point);
        let Some(nearest) = nearest else {
            let wild = self.natural(point) + self.slopes(point) + self.rocks(point) + mountains;
            let wild = self.pit(point, wild);
            return self
                .dams
                .iter()
                .fold(wild, |height, dam| dam.shape(point, height, 1.0));
        };
        let track = self.track;
        let along = track.along(nearest);
        let rock = self.rocks(point).max(self.cutting(nearest, along, point));
        let distance = nearest.distance;
        let road = track.height_beside(nearest) - UNDER_ROAD;
        let around_road = road.lerp(
            road_height_field(&self.road_field, point),
            smoothstep(FLAT, FLAT + 25.0, distance),
        );
        let mut blend = smoothstep(FLAT + 10.0, FLAT + 170.0, distance);
        if self.shore.is_some() || matches!(self.scenery.coast, Coast::Inland(_)) {
            // En una isla la costa está cerca: el terreno baja hasta el mar
            // en cuanto se pasan los muros. Inland, so it does to the rivers
            // and lakes.
            let to_sea = smoothstep(35.0, 0.0, self.coast(point));
            blend = blend.max(to_sea * smoothstep(FLAT, FLAT + 6.0, distance));
        }
        let mut land = around_road.lerp(self.natural(point), blend);
        // La roca sube a pico justo detrás de los muros. En los túneles,
        // algo más lejos: la rejilla del terreno es más gruesa que el hueco
        // entre la pared y la roca, y sus triángulos asomarían dentro.
        // A mine's excavation has no walls of its own past the bikes' (see
        // `cavern`): the rock keeps clear of them all the way to its mouths.
        let excavation = self.scenery.excavation();
        let past = if excavation.is_some() { 20.0 } else { 0.0 };
        let tunnel = self
            .scenery
            .tunnels
            .iter()
            .map(|tunnel| span(tunnel.from - past, tunnel.to + past).weight(along, self.lap, 20.0))
            .fold(0.0, f32::max);
        // A hall wider than the road keeps the rock that much further off.
        let wide =
            excavation.map_or(0.0, |halls| excavation_width(halls, self.lap, along)) * tunnel;
        let rise = smoothstep(FLAT, FLAT + 4.0, distance).lerp(
            smoothstep(FLAT + 4.0 + wide, FLAT + 8.0 + wide, distance),
            tunnel,
        );
        // On a mountain the road climbs, the ground beside it is cut into
        // the mountainside or built up to the road a little way past the
        // walls; round a tunnel through it, as steeply as the rock.
        let slopes = self.slopes(point);
        if slopes > 0.0 {
            let from = FLAT + (4.0 + wide) * tunnel;
            let to = (FLAT + SLOPE_REACH).lerp(FLAT + 8.0 + wide, tunnel);
            let side = road.lerp(self.natural(point) + slopes, smoothstep(from, to, distance));
            land = land.lerp(side, smoothstep(0.0, 10.0, slopes));
        }
        let mut height = land + rock * rise + mountains;
        // Explanadas para los edificios grandes, más allá de los muros.
        for terrace in self.scenery.terraces {
            let reach = 1.0
                - smoothstep(
                    terrace.radius,
                    terrace.radius + terrace.skirt,
                    point.distance(terrace.center),
                );
            height = height.lerp(
                terrace.height + terrace.tilt.dot(point - terrace.center),
                reach * smoothstep(FLAT, FLAT + 8.0, distance),
            );
        }
        // An open pit round the water: its benches and the spoil round its
        // rim, cut away just past the road's verges.
        let pitted = self.pit(point, height);
        height = height.lerp(pitted, smoothstep(FLAT, FLAT + 12.0, distance));
        // Bajo los puentes, el terreno sigue su forma natural (el mar de la
        // ría), siempre por debajo del tablero.
        let bridge = self
            .scenery
            .bridges
            .iter()
            .map(|bridge| bridge.span.weight(along, self.lap, 15.0))
            .fold(0.0, f32::max);
        if bridge > 0.0 {
            let wild = self.natural(point) + self.slopes(point) + self.rocks(point) + mountains;
            let wild = self.pit(point, wild);
            // Near the bridge, that is: further off, the ground as anywhere
            // else, so that cliffs may rise beside a long bridge over a
            // river (as statues either side of a causeway).
            let near = 1.0 - smoothstep(BRIDGE_REACH.0, BRIDGE_REACH.1, distance);
            height = height.lerp(wild.min(road - 2.5), bridge * near);
        }
        // A dam's river, the gorge round it and its lake: past the walls,
        // and under its crest, which the dam covers.
        for dam in &self.dams {
            let free = smoothstep(FLAT, FLAT + 5.0, distance)
                .max(dam.crest.weight(along, self.lap, CREST_FADE));
            height = dam.shape(point, height, free);
        }
        height
    }
}

impl Shaper<'_> {
    /// Indoors (see [`Soil::Floor`]), the floor: flat all over the room, but
    /// for following a road that lies on it a little way past its walls,
    /// just under its asphalt, so that a bump of the road (where it eases
    /// into a ramp) does not sink into it; where the road banks raised, as
    /// it would lie level (its inside stays down, and its outside stands on
    /// supports: see [`crate::track::Banking::raised`]). What runs over it,
    /// on furniture or on pillars, is a bridge, under which it stays flat.
    fn floor(&self, nearest: Option<&Nearest>) -> f32 {
        let floor = BASE_ELEVATION - UNDER_ROAD;
        let Some(nearest) = nearest else {
            return floor;
        };
        let along = self.track.along(nearest);
        let on_a_bridge = self
            .scenery
            .bridges
            .iter()
            .any(|bridge| bridge.span.contains(along, self.lap));
        if on_a_bridge || nearest.road != 0 {
            return floor;
        }
        let road = self.track.level_beside(nearest) - UNDER_ROAD;
        floor.lerp(road, 1.0 - smoothstep(FLAT, FLAT + 6.0, nearest.distance))
    }
}

/// The colour of a room's floorboards at `p` (linear RGBA), over their
/// texture: a little lighter or darker here and there, as a floor worn and
/// lit unevenly.
fn floor_vertex_color(p: Vec3) -> [f32; 4] {
    let v = 0.9 + 0.12 * (fbm(Vec2::new(p.x, p.z) / 60.0, 2) - 0.5);
    [v, v, v, 1.0]
}

/// The ground of an open pit `land` metres from the water (see [`Pit`]),
/// where it would otherwise be at `ground`: under the water as it was; then,
/// from just above the water, bench above bench, each face steep, up to the
/// rim; past it, the spoil heaped round the rim slopes down to the ground
/// round about, or, cut into higher ground, the benches go on up into it.
fn pit_ground(pit: &Pit, land: f32, ground: f32) -> f32 {
    if land < 0.0 {
        return ground;
    }
    let pitch = pit.tread + pit.step * PIT_FACE;
    let width = pit.benches as f32 * pitch;
    let foot = SEA_LEVEL + PIT_FOOT;
    if land < width {
        let bench = (land / pitch).floor();
        let up = smoothstep(0.0, pit.step * PIT_FACE, land - bench * pitch);
        return foot + (bench + up) * pit.step;
    }
    let rim = foot + pit.benches as f32 * pit.step;
    if ground > rim {
        // Cut into higher ground, the benches go on up into it.
        let bench = (land / pitch).floor();
        let up = smoothstep(0.0, pit.step * PIT_FACE, land - bench * pitch);
        return (foot + (bench + up) * pit.step).min(ground);
    }
    rim.lerp(ground, smoothstep(width, width + pit.spoil, land))
}

/// El valor en `along` de una función a trozos dada por pares (metros desde
/// la meta, valor) a lo largo de una vuelta de `lap` metros.
pub fn interpolate(points: &[(f32, f32)], lap: f32, along: f32) -> f32 {
    let along = along.rem_euclid(lap);
    let n = points.len();
    let after = points.iter().position(|&(at, _)| at > along).unwrap_or(n);
    // Antes del primer punto y después del último se enlaza con la vuelta
    // anterior o la siguiente.
    let (a, b) = match after {
        0 => ((points[n - 1].0 - lap, points[n - 1].1), points[0]),
        _ if after == n => (points[n - 1], (points[0].0 + lap, points[0].1)),
        _ => (points[after - 1], points[after]),
    };
    let t = ((along - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    a.1.lerp(b.1, t)
}

/// Distancia con signo de un punto a un polígono cerrado: positiva dentro.
pub(super) fn signed_distance(polygon: &[Vec2], point: Vec2) -> f32 {
    let mut distance = f32::INFINITY;
    let mut inside = false;
    let n = polygon.len();
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        let ab = b - a;
        let t = ((point - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        distance = distance.min(point.distance(a + ab * t));
        if (a.y > point.y) != (b.y > point.y)
            && point.x < a.x + (point.y - a.y) / (b.y - a.y) * (b.x - a.x)
        {
            inside = !inside;
        }
    }
    if inside { distance } else { -distance }
}

/// Media de las alturas de la carretera ponderada por cercanía: suave en
/// todas partes y casi igual a la carretera cerca de ella.
fn road_height_field(road_field: &[(Vec2, f32)], point: Vec2) -> f32 {
    let (mut sum, mut weights) = (0.0, 0.0);
    for &(at, height) in road_field {
        let weight = 1.0 / (at.distance_squared(point) + 100.0).powi(2);
        sum += weight * height;
        weights += weight;
    }
    sum / weights
}

/// Coordenadas de una rejilla en un eje: cada `FINE_SPACING` m entre `from` y
/// `to`, y por fuera celdas cada vez mayores hasta `EXTENT` m de `center`.
fn axis(from: f32, to: f32, center: f32) -> Vec<f32> {
    let mut before = Vec::new();
    let (mut at, mut step) = (from, FINE_SPACING);
    while at > center - EXTENT {
        step = (step * COARSE_GROWTH).min(COARSEST);
        at -= step;
        before.push(at);
    }
    before.reverse();
    let fine = ((to - from) / FINE_SPACING).ceil() as usize;
    let mut coordinates = before;
    coordinates.extend((0..=fine).map(|i| from + i as f32 * FINE_SPACING));
    let (mut at, mut step) = (*coordinates.last().expect("hay coordenadas"), FINE_SPACING);
    while at < center + EXTENT {
        step = (step * COARSE_GROWTH).min(COARSEST);
        at += step;
        coordinates.push(at);
    }
    coordinates
}

/// Celda de la rejilla que contiene `value` y la posición dentro de ella (0..1).
fn locate(axis: &[f32], value: f32) -> (usize, f32) {
    let value = value.clamp(axis[0], axis[axis.len() - 1]);
    let i = axis
        .partition_point(|&a| a <= value)
        .clamp(1, axis.len() - 1)
        - 1;
    (i, (value - axis[i]) / (axis[i + 1] - axis[i]))
}

/// Rock texture coordinates: planar projection from dominant face normal
/// with continuous coordinate orientation.
fn rock_projection(point: Vec3, face: Vec3) -> Vec2 {
    let abs = face.abs();
    if abs.y > 0.75 {
        Vec2::new(point.x, point.z)
    } else if abs.x > abs.z {
        let u = if face.x > 0.0 { point.z } else { -point.z };
        Vec2::new(u, -point.y)
    } else {
        let u = if face.z > 0.0 { -point.x } else { point.x };
        Vec2::new(u, -point.y)
    }
}

/// Grey granite's colour at `p`: the rock's strata and patches, without
/// their ochre and gold; on turf (`turf`, from 0 to 1, see [`Turf`]), green
/// with moss over much of it (see [`mossy`]).
fn granite_vertex_color(p: Vec3, normal: Vec3, turf: f32) -> [f32; 4] {
    // Moss over the grey, as a tint: a deep olive green.
    const MOSS: Vec3 = Vec3::new(0.4, 0.62, 0.18);
    let [r, g, b, a] = ground_vertex_color(Ground::Rock, p, normal);
    let lum = 0.299 * r + 0.587 * g + 0.114 * b;
    let grey = Vec3::new(lum * 0.98, lum, lum * 1.03);
    let [r, g, b] = grey.lerp(lum * MOSS, turf * mossy(p, normal)).to_array();
    [r, g, b, a]
}

/// Granite's `color` at `p`, of the colour of the tints it is in (see
/// [`RockTint`]): as light, of their colour, fading back into it over their
/// outer quarter.
fn tinted(color: [f32; 4], p: Vec3, tints: &[RockTint]) -> [f32; 4] {
    let lightness = |[r, g, b]: [f32; 3]| 0.299 * r + 0.587 * g + 0.114 * b;
    let [r, g, b, a] = color;
    let mut tinted = Vec3::new(r, g, b);
    let here = Vec2::new(p.x, p.z);
    for tint in tints {
        let inside = 1.0 - smoothstep(0.75, 1.0, here.distance(tint.center) / tint.radius);
        if inside > 0.0 {
            let hue = tint.color.to_linear().to_f32_array_no_alpha();
            let colored = Vec3::from(hue) * (lightness(tinted.to_array()) / lightness(hue));
            tinted = tinted.lerp(colored, inside);
        }
    }
    let [r, g, b] = tinted.to_array();
    [r, g, b, a]
}

/// How much moss there is on rock at `p`, facing `normal`, where it grows
/// (see [`granite_vertex_color`]): in patches some metres across, the same
/// all the way down a face, so in streaks down it as where water runs down
/// wet rock, and more over its ledges.
fn mossy(p: Vec3, normal: Vec3) -> f32 {
    let patches = fbm(Vec2::new(p.x, p.z) / MOSS_PATCH + Vec2::splat(31.0), 3);
    smoothstep(0.38, 0.6, patches + MOSS_ON_LEDGES * normal.y.max(0.0))
}

/// How wide the patches of moss on the rock are (m, see [`mossy`]), and
/// how much more there is of it on ledges, facing up, than on sheer faces.
const MOSS_PATCH: f32 = 9.0;
const MOSS_ON_LEDGES: f32 = 0.25;

pub fn ground_vertex_color(ground: Ground, p: Vec3, normal: Vec3) -> [f32; 4] {
    let color = match ground {
        Ground::Grass => lawn_tint(p, normal),
        Ground::Rock => {
            // 1. Macro massif geological formation (scale ~120m):
            // Regional variations across mountain ranges:
            // - Warm Mediterranean ochre/terracotta sandstone
            // - Sun-baked golden coastal limestone
            // - Dark volcanic slate/basalt
            let massif = fbm(Vec2::new(p.x * 0.007 + 100.0, p.z * 0.007 - 100.0), 3);
            let mineral_patch = fbm(Vec2::new(p.x * 0.018 - 45.0, p.z * 0.018 + 75.0), 2);

            let golden_limestone = Color::srgb(1.14, 1.06, 0.92);
            let ochre_sandstone = Color::srgb(1.18, 0.94, 0.80);
            let slate_basalt = Color::srgb(0.76, 0.80, 0.88);
            let pale_granite = Color::srgb(0.96, 0.96, 0.94);

            let base_rock = if massif > 0.55 {
                let t = (massif - 0.55) / 0.45;
                golden_limestone.mix(&ochre_sandstone, t)
            } else if massif < 0.45 {
                let t = (0.45 - massif) / 0.45;
                pale_granite.mix(&slate_basalt, t)
            } else {
                let t = (massif - 0.45) * 10.0;
                pale_granite.mix(&golden_limestone, t)
            };

            // 2. Tilted sedimentary strata layers (scale ~6m - 14m):
            let dip_plane = p.y * 0.28 + (p.x * 0.08 - p.z * 0.06);
            let strata_noise = fbm(Vec2::new(p.x * 0.02, p.z * 0.02), 2) * 1.8;
            let strata_phase = (dip_plane + strata_noise).sin();
            let strata_factor = (strata_phase * 0.5 + 0.5).powf(1.4);

            let strata_dark = Color::srgb(0.70, 0.68, 0.72);
            let strata_light = Color::srgb(1.18, 1.12, 1.04);
            let layered = if strata_factor > 0.5 {
                base_rock.mix(&strata_light, (strata_factor - 0.5) * 0.75)
            } else {
                base_rock.mix(&strata_dark, (0.5 - strata_factor) * 0.65)
            };

            // 3. Directional sun exposure & ambient occlusion in crevices:
            let sun_dir = Vec3::new(0.55, 0.82, -0.35).normalize();
            let sun_dot = normal.dot(sun_dir).clamp(-1.0, 1.0);
            let sun_highlight = (sun_dot * 0.5 + 0.5).powf(1.3);
            let ao = (0.52 + 0.48 * normal.y.max(0.0)).clamp(0.52, 1.0);

            // 4. Subtle moss and lichen on sheltered rock ledges:
            let moss_noise = fbm(Vec2::new(p.x * 0.035 - 15.0, p.z * 0.035 + 30.0), 2);
            let mossy = Color::srgb(0.76, 0.88, 0.68);
            let sheltered = (normal.y > 0.25 && normal.y < 0.82 && moss_noise > 0.56) as i32 as f32;
            let moss_tint = (moss_noise - 0.56).max(0.0) * 1.5 * sheltered;

            // 5. Mineral patch accents:
            let mineral_dark = Color::srgb(0.85, 0.82, 0.80);
            let with_minerals = layered.mix(&mineral_dark, (mineral_patch * 0.5).clamp(0.0, 0.4));

            let lit = with_minerals.mix(&Color::srgb(1.22, 1.16, 1.04), sun_highlight * 0.35);
            let shaded = lit.mix(&Color::srgb(0.56, 0.56, 0.62), (1.0 - ao) * 0.65);
            let mossed = shaded.mix(&mossy, moss_tint.clamp(0.0, 0.45));
            // Past the tree line of the tallest mountains, the bare scree of
            // a volcano: dark grey-brown, a little lighter where the sun is.
            let scree =
                Color::srgb(0.5, 0.44, 0.44).mix(&Color::srgb(0.66, 0.58, 0.55), sun_highlight);
            mossed.mix(&scree, smoothstep(TREE_LINE - 60.0, TREE_LINE, p.y))
        }
        Ground::Sand => {
            // Moisture gradient near sea level and windblown dune variations
            let moisture = ((BEACH_HEIGHT - p.y) / (BEACH_HEIGHT - SEA_LEVEL)).clamp(0.0, 1.0);
            let dune = value_noise(Vec2::new(p.x * 0.05, p.z * 0.05));

            let dry_sand = Color::srgb(1.03, 1.01, 0.95);
            let wet_sand = Color::srgb(0.78, 0.74, 0.68);
            let dune_tint = Color::srgb(1.05, 0.98, 0.90);

            let beach = dry_sand.mix(&dune_tint, (dune - 0.5) * 0.3);
            beach.mix(&wet_sand, (moisture * 1.1).clamp(0.0, 1.0))
        }
        Ground::Snow => snow_color(p, normal),
    };
    color.to_linear().to_f32_array()
}

/// The tint of grass of `greens` at `p`, `wild` as it is far from the roads:
/// the lawn's knows nothing of wild land.
fn greens_tint(greens: Greens, p: Vec3, normal: Vec3, wild: f32) -> Color {
    match greens {
        Greens::Lawn => lawn_tint(p, normal),
        Greens::Countryside => grass_tint(p, normal, wild),
        Greens::Mediterranean => mediterranean_tint(p, normal, wild),
        Greens::Lush => lush_tint(p, normal, wild),
    }
}

/// A lush green land's greens (`Greens::Lush`), as tints over the grass's
/// texture: the countryside's (see [`grass_tint`]), its forests as they
/// are, but its grass deep, rich and green, little red and less blue in
/// it, as the grass of a wet land is in the sun (a green land's at
/// Matamata: about sRGB 80, 125, 35 in the sun), not yellowed nor
/// paled by it; and darker where it grows thick, in drifts some tens of
/// metres across, so that a green hill is not one flat green.
fn lush_tint(p: Vec3, normal: Vec3, wild: f32) -> Color {
    const LUSH: Color = Color::srgb(0.39, 0.68, 0.17);
    const THICK: Color = Color::srgb(0.3, 0.52, 0.12);
    let at = Vec2::new(p.x, p.z);
    let grass = 1.0 - smoothstep(FOREST_FROM, FOREST_FULL, p.y).max(wild);
    let thick = smoothstep(0.42, 0.68, fbm(at / 28.0 + Vec2::splat(53.0), 3));
    grass_tint(p, normal, wild)
        .mix(&LUSH, 0.75 * grass)
        .mix(&THICK, 0.5 * thick * grass)
}

/// A Mediterranean island's greens in summer (`Greens::Mediterranean`), as
/// tints over the grass's texture: broad swathes of dry, straw-coloured
/// grass, olive and grey-green sage, with patches within them and a fine
/// mottling, all on a warped field so no edge runs straight; fresher green
/// down the hollows and by the water, drier up high and on the slopes
/// turned to the sun, dark scrub up the steep slopes before the rock, and
/// the scrub over the wild land far from the roads. The texture is a lush
/// green photo, so each tint is the colour wanted over the texture's own
/// (linear): it already is as dark as it should be, with no `grass_shade`.
pub fn mediterranean_tint(p: Vec3, normal: Vec3, wild: f32) -> Color {
    // Over the grass photo's average (linear 0.153, 0.359, 0.039): dry
    // straw, olive, sage, fresh green, dark scrub and bare earth.
    const STRAW: Color = Color::linear_rgb(2.2, 0.76, 2.2);
    const OLIVE: Color = Color::linear_rgb(1.1, 0.53, 1.3);
    const SAGE: Color = Color::linear_rgb(0.98, 0.56, 2.1);
    const FRESH: Color = Color::linear_rgb(0.8, 0.68, 0.8);
    const SCRUB: Color = Color::linear_rgb(0.3, 0.2, 0.4);
    const EARTH: Color = Color::linear_rgb(1.7, 0.48, 2.0);
    const SWATHES: [Color; 5] = [SAGE, OLIVE, STRAW, OLIVE, FRESH];
    let at = Vec2::new(p.x, p.z);
    // Warped, so the swathes and patches have ragged edges, not round ones.
    let warp = Vec2::new(
        fbm(at / 70.0 + 3.0, 2) - 0.5,
        fbm(at / 70.0 - 11.0, 2) - 0.5,
    ) * 60.0;
    let warped = at + warp;
    let swathe =
        ((fbm(warped / 110.0, 3) - 0.3) / 0.4).clamp(0.0, 1.0) * (SWATHES.len() - 1) as f32;
    let k = (swathe as usize).min(SWATHES.len() - 2);
    let mut tint = SWATHES[k].mix(&SWATHES[k + 1], smoothstep(0.0, 1.0, swathe - k as f32));
    // Patches within them: drier straw, or darker scrub.
    let patch = fbm(warped / 28.0 + 17.0, 3) - 0.5;
    tint = if patch > 0.0 {
        tint.mix(&STRAW, (patch * 2.4).min(0.75))
    } else {
        tint.mix(&SCRUB, (-patch * 2.4).min(0.75))
    };
    // Down the hollows and by the water, fresher; up high and towards the
    // sun, drier.
    let wet = 1.0 - smoothstep(BEACH_HEIGHT, BEACH_HEIGHT + 6.0, p.y);
    let hollow = smoothstep(0.55, 0.8, fbm(at / 55.0 - 23.0, 2)) * 0.5;
    tint = tint.mix(&FRESH, (wet * 0.7).max(hollow));
    let sun = super::sky::direction(MEDITERRANEAN_SUN.0, MEDITERRANEAN_SUN.1);
    let exposed = smoothstep(25.0, 60.0, p.y) * 0.3 + smoothstep(0.85, 1.0, normal.dot(sun)) * 0.25;
    tint = tint.mix(&STRAW, exposed);
    // Up the steep slopes, scrub, and on the steepest banks bare earth.
    let steepness = (1.0 - normal.y).clamp(0.0, 1.0);
    tint = tint.mix(&SCRUB, smoothstep(0.05, 0.16, steepness) * 0.6);
    let detail = value_noise(at * 0.045 + Vec2::new(17.0, -23.0));
    tint = tint.mix(
        &EARTH,
        (smoothstep(0.12, 0.2, steepness) * 0.4 + (detail - 0.5) * 0.15).clamp(0.0, 0.4),
    );
    // A fine mottling of lighter and darker tufts.
    let mottle = 0.86 + 0.28 * value_noise(warped / 6.0);
    let linear = tint.mix(&SCRUB, wild * 0.7).to_linear();
    Color::linear_rgb(
        linear.red * mottle,
        linear.green * mottle,
        linear.blue * mottle,
    )
}

/// The arcade lawn's greens (`Greens::Lawn`), as tints over its texture:
/// natural variation across tens of metres (bright meadows, shaded hollows,
/// golden knolls) and drier embankments on the steeper slopes.
fn lawn_tint(p: Vec3, normal: Vec3) -> Color {
    let macro_n = fbm(Vec2::new(p.x * 0.012, p.z * 0.012), 3);
    let detail_n = value_noise(Vec2::new(p.x * 0.045 + 17.0, p.z * 0.045 - 23.0));
    let steepness = (1.0 - normal.y).clamp(0.0, 1.0);

    // Three harmonious arcade tones:
    // 1. Vibrant lush meadow green
    let lush = Color::srgb(0.92, 1.05, 0.88);
    // 2. Warm golden sunny slope green
    let sunny = Color::srgb(1.05, 1.01, 0.85);
    // 3. Deep green natural shade
    let deep = Color::srgb(0.84, 0.93, 0.82);

    let base_tint = if macro_n > 0.5 {
        lush.mix(&sunny, (macro_n - 0.5) * 2.0)
    } else {
        deep.mix(&lush, macro_n * 2.0)
    };

    // Subtle micro-relief accent and drier embankments on steeper inclines
    let earthy = Color::srgb(1.02, 0.95, 0.82);
    base_tint.mix(
        &earthy,
        (steepness * 1.8 + (detail_n - 0.5) * 0.3).clamp(0.0, 0.55),
    )
}

/// A countryside's greens (`Greens::Countryside`), as tints over the grass's
/// texture: not one green, as a lawn's, but many. Broad swathes, a hundred metres and
/// more across, of fresh, yellow, olive, deep and bluish greens, with
/// patches within them; lusher and deeper by the water, drier and yellower
/// up on the heights and on the slopes turned to the sun, earthier on
/// steep banks; and, up the hills and over the `wild` land far from the
/// roads (0 to 1), the dark, cool green of forests of conifers, as on the
/// lower slopes of a volcano.
pub fn grass_tint(p: Vec3, normal: Vec3, wild: f32) -> Color {
    const DEEP: Color = Color::srgb(0.7, 0.84, 0.7);
    const BLUISH: Color = Color::srgb(0.74, 0.88, 0.86);
    const FRESH: Color = Color::srgb(0.94, 1.0, 0.88);
    const YELLOW: Color = Color::srgb(1.1, 1.02, 0.64);
    const OLIVE: Color = Color::srgb(0.86, 0.84, 0.58);
    const SWATHES: [Color; 5] = [BLUISH, DEEP, FRESH, YELLOW, OLIVE];
    const FOREST: Color = Color::srgb(0.2, 0.31, 0.29);
    const DARK_FOREST: Color = Color::srgb(0.13, 0.21, 0.2);
    const EARTHY: Color = Color::srgb(1.0, 0.9, 0.74);
    let at = Vec2::new(p.x, p.z);
    // The swathes, from one green to the next along the noise.
    let swathe = ((fbm(at / 170.0, 3) - 0.3) / 0.4).clamp(0.0, 1.0) * (SWATHES.len() - 1) as f32;
    let k = (swathe as usize).min(SWATHES.len() - 2);
    let mut tint = SWATHES[k].mix(&SWATHES[k + 1], swathe - k as f32);
    // Patches within them, drier or deeper.
    let patch = fbm(at / 45.0 + 31.0, 2) - 0.5;
    tint = if patch > 0.0 {
        tint.mix(&YELLOW, patch * 0.8)
    } else {
        tint.mix(&DEEP, -patch * 0.8)
    };
    // By the water, lush; up on the heights and towards the sun, dry.
    let wet = 1.0 - smoothstep(BEACH_HEIGHT, BEACH_HEIGHT + 5.0, p.y);
    tint = tint.mix(&DEEP, wet * 0.6);
    let sun = Vec3::new(0.55, 0.82, -0.35).normalize();
    let exposed = smoothstep(20.0, 60.0, p.y) * 0.35 + smoothstep(0.85, 1.0, normal.dot(sun)) * 0.2;
    tint = tint.mix(&YELLOW, exposed);
    // Steep banks, earthier.
    let steepness = (1.0 - normal.y).clamp(0.0, 1.0);
    let detail = value_noise(at * 0.045 + Vec2::new(17.0, -23.0));
    tint = tint.mix(
        &EARTHY,
        (steepness * 1.8 + (detail - 0.5) * 0.3).clamp(0.0, 0.5),
    );
    // Forests up the hills and over the wild land, darkest there: the
    // shadowy conifers at the foot of the volcano.
    let forest = smoothstep(FOREST_FROM, FOREST_FULL, p.y).max(wild);
    tint.mix(&FOREST, forest * 0.9)
        .mix(&DARK_FOREST, wild * 0.8)
}

/// The colour `color` (linear RGBA) of `ground` at `p` on a mountain across
/// the sea (`mountain` how much of it), weathered as a real mountain's:
/// lighter on its ridges and darker down its gullies (`hollow`), which its
/// grid is too coarse to light; under forest, mottled by the trees' crowns,
/// with stands of darker conifers and a few in muted autumn colours; scree
/// in streaks down the slope; snow bluer in the gullies' shade.
fn weathered(color: [f32; 4], ground: Ground, p: Vec3, hollow: f32, mountain: f32) -> [f32; 4] {
    let at = Vec2::new(p.x, p.z);
    let relief = 1.0 - 0.32 * hollow.max(0.0) + 0.2 * (-hollow).max(0.0);
    let [red, green, blue, alpha] = color;
    let [r, g, b] = match ground {
        Ground::Grass => {
            let crowns = 0.8 + 0.4 * fbm(at / 31.0, 2);
            let stands = 0.75 + 0.5 * fbm(at / 210.0 + Vec2::splat(40.0), 3);
            let autumn = 0.45 * smoothstep(0.6, 0.72, fbm(at / 160.0 - Vec2::splat(12.0), 3));
            let k = crowns * stands * relief;
            // Rust and amber, muted, as the maples and larches on a far
            // slope in autumn.
            let rust = [0.09, 0.04, 0.015];
            [
                (red * k).lerp(rust[0] * k, autumn),
                (green * k).lerp(rust[1] * k, autumn),
                (blue * k).lerp(rust[2] * k, autumn),
            ]
        }
        Ground::Rock => {
            let k = (0.78 + 0.44 * fbm(at / 45.0 + Vec2::splat(7.0), 3)) * relief;
            [red * k, green * k, blue * k]
        }
        Ground::Snow => {
            let shade = 0.28 * hollow.max(0.0);
            [
                red * (1.0 - shade),
                green * (1.0 - 0.8 * shade),
                blue * (1.0 - 0.45 * shade),
            ]
        }
        Ground::Sand => [red, green, blue],
    };
    [
        red.lerp(r, mountain),
        green.lerp(g, mountain),
        blue.lerp(b, mountain),
        alpha,
    ]
}

/// A tint `shade` times as light (linear RGBA), its green less vivid the
/// darker it is: a dark grass is a dull one, not a lawn's vivid green.
pub fn shaded(tint: Color, shade: f32) -> [f32; 4] {
    let linear = tint.to_linear();
    let green = shade * (0.45 + 0.55 * shade);
    [
        linear.red * shade,
        linear.green * green,
        linear.blue * shade,
        linear.alpha,
    ]
}

/// Snow: white, a little blue in the shade of the slopes turned away from
/// the sun and in streaks down the ravines.
fn snow_color(p: Vec3, normal: Vec3) -> Color {
    let streaks = fbm(Vec2::new(p.x, p.z) / 40.0, 3);
    let shade = (1.0 - normal.y) * 1.6 + (streaks - 0.5) * 0.5;
    Color::srgb(0.97, 0.98, 1.0).mix(&Color::srgb(0.72, 0.78, 0.92), shade.clamp(0.0, 0.6))
}

/// The colours of a desert canyon, over the textures: dry red-brown earth on
/// the flat, and on the faces bands of red, rust and ochre rock, like the
/// mesas of the American south-west, darker in the crevices.
fn canyon_vertex_color(ground: Ground, p: Vec3, normal: Vec3) -> [f32; 4] {
    let color = match ground {
        Ground::Grass => {
            let patch = fbm(Vec2::new(p.x * 0.015, p.z * 0.015), 3);
            let grit = value_noise(Vec2::new(p.x * 0.06 + 11.0, p.z * 0.06 - 5.0));
            let dust = Color::srgb(1.12, 0.78, 0.58);
            let red_earth = Color::srgb(1.0, 0.62, 0.45);
            let pale = Color::srgb(1.15, 0.9, 0.7);
            let earth = red_earth.mix(&dust, patch);
            earth.mix(
                &pale,
                ((grit - 0.5) * 0.6 + (1.0 - normal.y) * 1.5).clamp(0.0, 0.5),
            )
        }
        Ground::Rock => {
            // Horizontal strata, as in a canyon cut by a river, a little
            // wavy.
            let wave = fbm(Vec2::new(p.x * 0.01, p.z * 0.01), 2) * 6.0;
            let band = ((p.y + wave) * 0.45).sin() * 0.5 + 0.5;
            let thin = ((p.y + wave) * 1.7).sin() * 0.5 + 0.5;
            let red = Color::srgb(1.05, 0.58, 0.42);
            let rust = Color::srgb(0.88, 0.5, 0.36);
            let ochre = Color::srgb(1.2, 0.86, 0.58);
            let layered = if band > 0.5 {
                red.mix(&ochre, (band - 0.5) * 1.6)
            } else {
                red.mix(&rust, (0.5 - band) * 1.6)
            };
            let streaked = layered.mix(&Color::srgb(1.1, 0.95, 0.8), thin.powi(6) * 0.35);
            let ao = (0.55 + 0.45 * normal.y.max(0.0)).clamp(0.55, 1.0);
            streaked.mix(&Color::srgb(0.5, 0.36, 0.34), (1.0 - ao) * 0.6)
        }
        // By the river: damp, darker sand.
        Ground::Sand => {
            let dune = value_noise(Vec2::new(p.x * 0.05, p.z * 0.05));
            Color::srgb(0.95, 0.78, 0.62).mix(&Color::srgb(0.8, 0.66, 0.55), dune * 0.5)
        }
        Ground::Snow => snow_color(p, normal),
    };
    color.to_linear().to_f32_array()
}

/// The colours of the Moon's surface (`Soil::Lunar`), over the sand and rock
/// textures: pale ash-grey regolith, dark slate-blue basalt maria, bright
/// impact ejecta rays, and dark anorthosite crater walls.
fn lunar_vertex_color(ground: Ground, p: Vec3, normal: Vec3) -> [f32; 4] {
    let color = match ground {
        Ground::Grass | Ground::Sand => {
            let mare = fbm(Vec2::new(p.x * 0.012, p.z * 0.012), 3);
            let ejecta = value_noise(Vec2::new(p.x * 0.055 + 19.0, p.z * 0.055 - 13.0));
            let regolith = Color::srgb(0.68, 0.72, 0.78);
            let basalt = Color::srgb(0.46, 0.50, 0.58);
            let highland = Color::srgb(0.84, 0.88, 0.94);
            let base = basalt.mix(&regolith, mare);
            base.mix(
                &highland,
                ((ejecta - 0.5) * 0.55 + (1.0 - normal.y) * 1.4).clamp(0.0, 0.55),
            )
        }
        Ground::Rock => {
            let wave = fbm(Vec2::new(p.x * 0.012, p.z * 0.012), 2) * 5.0;
            let band = ((p.y + wave) * 0.42).sin() * 0.5 + 0.5;
            let anorthosite = Color::srgb(0.76, 0.80, 0.88);
            let dark_basalt = Color::srgb(0.42, 0.46, 0.54);
            let layered = dark_basalt.mix(&anorthosite, band);
            let ao = (0.5 + 0.5 * normal.y.max(0.0)).clamp(0.5, 1.0);
            layered.mix(&Color::srgb(0.24, 0.27, 0.34), (1.0 - ao) * 0.65)
        }
        Ground::Snow => snow_color(p, normal),
    };
    color.to_linear().to_f32_array()
}

/// Checkerboard earth only needs light and shade: its colours are in the
/// texture. Faces towards the sun are lighter and a little warmer, faces
/// away from it darker, with broad, faint patches.
fn soil_vertex_color(p: Vec3, normal: Vec3) -> [f32; 4] {
    let sun = Vec3::new(0.55, 0.82, -0.35).normalize();
    let lit = (normal.dot(sun) * 0.5 + 0.5).clamp(0.0, 1.0);
    let patch = fbm(Vec2::new(p.x, p.z) * 0.02, 2);
    let shade = 0.78 + 0.32 * lit + 0.1 * (patch - 0.5);
    Color::srgb(shade * 1.04, shade, shade * 0.96)
        .to_linear()
        .to_f32_array()
}

/// How many rows of the grid's cells a band of the ground's meshes takes
/// (see [`Terrain::meshes`]).
const BAND_ROWS: usize = 24;

/// The ground's meshes over a band of the grid, by kind of ground (see
/// [`Terrain::band`]).
#[derive(Default)]
struct Band {
    grass: Builder,
    sand: Builder,
    rock: Builder,
    snow: Builder,
    meadows: Vec<(FlowerKind, Builder)>,
}

impl Band {
    /// `next`, the band after it, added to it: each kind of ground's mesh
    /// to its own, a kind of meadow not met before after the others.
    fn append(&mut self, next: Band) {
        self.grass.append(next.grass);
        self.sand.append(next.sand);
        self.rock.append(next.rock);
        self.snow.append(next.snow);
        for (kind, meadow) in next.meadows {
            match self.meadows.iter_mut().find(|(k, _)| *k == kind) {
                Some((_, mine)) => mine.append(meadow),
                None => self.meadows.push((kind, meadow)),
            }
        }
    }
}

/// Acumula los triángulos de un tipo de suelo.
#[derive(Default)]
struct Builder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    /// Para compartir vértices entre triángulos: índice nuevo de cada vértice
    /// de la rejilla. La roca no los comparte, porque cada cara proyecta su
    /// textura desde un lado distinto.
    shared_vertices: Option<Vec<u32>>,
}

impl Builder {
    fn shared(grid_vertices: usize) -> Self {
        Self {
            shared_vertices: Some(vec![u32::MAX; grid_vertices]),
            ..default()
        }
    }

    /// Adds a corner of a triangle: a point of the grid (`grid`), shared
    /// with the triangles before it where this builder shares them, or a
    /// point of its own on a border. `vertex` works out its position,
    /// normal, texture coordinates and colour, if it is new.
    fn push(&mut self, grid: Option<usize>, vertex: impl FnOnce() -> (Vec3, Vec3, Vec2, [f32; 4])) {
        if let (Some(grid), Some(shared)) = (grid, &self.shared_vertices)
            && shared[grid] != u32::MAX
        {
            self.indices.push(shared[grid]);
            return;
        }
        let (position, normal, uv, color) = vertex();
        let index = self.positions.len() as u32;
        self.positions.push(position.to_array());
        self.normals.push(normal.to_array());
        self.uvs.push(uv.to_array());
        self.colors.push(color);
        self.indices.push(index);
        if let (Some(grid), Some(shared)) = (grid, &mut self.shared_vertices) {
            shared[grid] = index;
        }
    }

    /// `other`'s triangles added after its own.
    fn append(&mut self, other: Builder) {
        let offset = self.positions.len() as u32;
        self.positions.extend(other.positions);
        self.normals.extend(other.normals);
        self.uvs.extend(other.uvs);
        self.colors.extend(other.colors);
        self.indices
            .extend(other.indices.into_iter().map(|index| index + offset));
    }

    fn build(self, tile: f32) -> Mesh {
        let uvs: Vec<[f32; 2]> = self.uvs.iter().map(|[u, v]| [u / tile, v / tile]).collect();
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    /// The engine's demo circuit, the bay.
    fn bay() -> (Track, Terrain) {
        let circuit = crate::testing::demo_circuit();
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        (track, terrain)
    }

    pub fn terrain_hugs_the_road_and_the_track_stays_dry_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let terrain = Terrain::new(&track, &circuit.scenery);
            let tunnels = Tunnels::new(&track, &circuit.scenery);
            let lap = track.lap_length();
            for index in (0..track.sample_count()).step_by(7) {
                let along = index as f32 * track.step_length();
                // En los puentes, debajo está la ría.
                if circuit
                    .scenery
                    .bridges
                    .iter()
                    .any(|b| b.span.contains(along, lap))
                {
                    continue;
                }
                for lateral in [-9.0, 0.0, 9.0] {
                    // Where it banks raised, on supports, there is no road
                    // past its walls: its inside wall stands on the floor.
                    let wall = track.wall_at(index);
                    let lateral = if track.lift(index) > 0.0 {
                        f32::clamp(lateral, -wall, wall)
                    } else {
                        lateral
                    };
                    let road = track.surface_point(index, lateral);
                    let ground = terrain.height(Vec2::new(road.x, road.z));
                    // Over a tunnel, the road rests on its rock.
                    if tunnels
                        .beneath(&track, Vec2::new(road.x, road.z), TUNNEL_SHELL)
                        .is_some_and(|tunnel| track.height_beside(&tunnel) < road.y - 5.0)
                    {
                        continue;
                    }
                    let name = circuit.name;
                    // Por debajo del asfalto (si no, asomaría), pero muy cerca.
                    assert!(
                        ground < road.y + 0.02,
                        "el terreno tapa la carretera en {index} de {name}"
                    );
                    // Where it banks raised, its outside stands on risers:
                    // the ground is under its inside, as it would lie level.
                    let level = track.surface_point(index, 0.0).y - track.lift(index);
                    let under = road.y.min(level);
                    assert!(
                        ground > under - 0.4,
                        "hay un hueco bajo la carretera en {index} de {name}"
                    );
                    assert!(
                        road.y > SEA_LEVEL + 0.5,
                        "la carretera se moja en {index} de {name}"
                    );
                }
            }
        }
    }

    #[test]
    fn terrain_hugs_the_road_and_the_track_stays_dry() {
        terrain_hugs_the_road_and_the_track_stays_dry_on(&crate::testing::DEMO);
    }

    pub fn the_track_edge_is_where_the_earth_rises_inside_the_walls_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let terrain = Terrain::new(&track, &circuit.scenery);
            let edges = terrain.track_edges(&track);
            assert_eq!(edges.len(), 1 + track.side_roads().len());
            let edges = &edges[0];
            assert_eq!(edges.len(), track.sample_count());
            let mut inside = 0;
            for (index, sides) in edges.iter().enumerate() {
                let wall = track.wall_at(index);
                for (edge, side) in sides.iter().zip([1.0, -1.0]) {
                    assert!((track.half_width().min(wall)..=wall).contains(edge));
                    inside += usize::from(*edge < wall);
                    // Short of the edge, the earth is not above the road.
                    let road = track.surface_point(index, side * (edge - 0.15));
                    let earth = terrain.height(Vec2::new(road.x, road.z));
                    assert!(earth - road.y <= EARTH_RISE, "{} {index}", circuit.name);
                }
            }
            // Next to the rock faces, the earth comes in before the walls;
            // indoors, on a flat floor, there is none.
            assert!(
                inside > 0 || circuit.scenery.soil == Soil::Floor,
                "{}",
                circuit.name
            );
        }
    }

    #[test]
    fn the_track_edge_is_where_the_earth_rises_inside_the_walls() {
        the_track_edge_is_where_the_earth_rises_inside_the_walls_on(&crate::testing::DEMO);
    }

    #[test]
    fn there_is_sea_to_the_west_and_mountains_to_the_north_east() {
        let (_, terrain) = bay();
        assert!(terrain.height(Vec2::new(-400.0, -200.0)) < SEA_LEVEL);
        assert!(terrain.height(Vec2::new(1000.0, -1000.0)) > 50.0);
    }

    pub fn an_island_has_a_river_under_its_bridge_on(circuits: &'static crate::circuits::Circuits) {
        let circuit = &circuits[1];
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        // Sea beyond the beach of the seafront, and land inside both loops
        // of the figure of eight.
        assert!(terrain.height(Vec2::new(-90.0, -100.0)) < SEA_LEVEL);
        assert!(terrain.height(Vec2::new(-22.0, -100.0)) > SEA_LEVEL + 1.0);
        assert!(terrain.height(Vec2::new(90.0, -150.0)) > SEA_LEVEL + 1.0);
        assert!(terrain.height(Vec2::new(420.0, -500.0)) > SEA_LEVEL + 1.0);
        // The bays between the loops, north and south of the waist.
        assert!(terrain.height(Vec2::new(120.0, -520.0)) < SEA_LEVEL);
        assert!(terrain.height(Vec2::new(330.0, -60.0)) < SEA_LEVEL);
        // The river runs in under the middle of the bridge.
        let bridge = circuit.scenery.bridges[0].span;
        let middle = track.index_at((bridge.from + bridge.to) / 2.0);
        let below = track.offset_point(middle, 0.0);
        assert!(
            terrain.height(below) < SEA_LEVEL,
            "{}",
            terrain.height(below)
        );
    }

    pub fn the_road_dives_to_the_foot_of_a_lighthouse_in_full_view_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let circuit = &circuits[1];
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        let lighthouse = &circuit.scenery.lighthouses[0];
        let elevation = |along: f32| track.elevations()[track.index_at(along)];
        let point = |along: f32| track.centerline()[track.index_at(along)];
        // The road drops some 17 m from the top of the mountain road to the
        // foot of the lighthouse, in under 250 m, at over 11 % at its
        // steepest (and never over 13 %: see `slopes_and_banking_stay_rideable`).
        let top = (1380..1480).step_by(10).map(|a| elevation(a as f32));
        let top = top.fold(f32::MIN, f32::max);
        let near: Vec<f32> = (1300..1800)
            .step_by(5)
            .map(|a| a as f32)
            .filter(|&a| point(a).distance(lighthouse.position) < 70.0)
            .collect();
        let foot = near.iter().map(|&a| elevation(a)).fold(f32::MAX, f32::min);
        assert!(top - foot > 16.0, "{top} m down to {foot} m");
        let steepest = (1440..1640)
            .step_by(10)
            .map(|a| (elevation(a as f32) - elevation(a as f32 + 10.0)) / 10.0)
            .fold(0.0, f32::max);
        assert!(steepest > 0.11, "{steepest}");
        // It stands on a low cape, a little above the road.
        let base = terrain.height(lighthouse.position);
        assert!(base > foot && base < foot + 8.0, "{base} m over {foot} m");
        // All the way down the dive and round its foot, the whole tower is
        // in sight from the rider's eyes, over the ground.
        for along in (1480..1660).step_by(10).map(|a| a as f32) {
            let eye = point(along).extend(elevation(along) + 1.5);
            let foot_of_tower = lighthouse.position.extend(base + 1.0);
            for k in 1..60 {
                let t = k as f32 / 60.0;
                let at = eye.lerp(foot_of_tower, t);
                assert!(
                    terrain.height(at.truncate()) < at.z,
                    "at {along} m, the ground hides the lighthouse"
                );
            }
        }
    }

    pub fn a_road_passes_over_its_own_tunnel_on(circuits: &'static crate::circuits::Circuits) {
        let circuit = &circuits[1];
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        let tunnels = Tunnels::new(&track, &circuit.scenery);
        // Where the stretch coming down from the forest crosses the tunnel.
        let lap = track.lap_length();
        let tunnel = circuit.scenery.tunnels[0];
        let (below, above) = (0..track.sample_count())
            .filter(|&i| tunnel.contains(i as f32 * track.step_length(), lap))
            .flat_map(|i| (0..track.sample_count()).map(move |j| (i, j)))
            .filter(|&(i, j)| {
                !tunnel.contains(j as f32 * track.step_length(), lap)
                    && (i as f32 - j as f32).abs() > 200.0
            })
            .min_by(|&(a, b), &(c, d)| {
                let gap =
                    |i: usize, j: usize| track.centerline()[i].distance(track.centerline()[j]);
                gap(a, b).total_cmp(&gap(c, d))
            })
            .expect("the track crosses the tunnel");
        let point = track.centerline()[below];
        assert!(point.distance(track.centerline()[above]) < 2.0);
        let (low, high) = (track.elevations()[below], track.elevations()[above]);
        assert!(high - low > 14.0, "{low} m under {high} m");
        // On each level, the track is the road at that level.
        assert_eq!(track.nearest_index_at(point, low), below);
        assert_eq!(track.nearest_index_at(point, high), above);
        assert!((track.road_height_at(point, low) - low).abs() < 0.5);
        assert!((track.road_height_at(point, high) - high).abs() < 0.5);
        // The ground stays down at the tunnel, under the rock that carries
        // the road above.
        assert!(tunnels.beneath(&track, point, BURIED).is_some());
        assert!((terrain.height(point) - low).abs() < 0.1);
    }

    pub fn a_road_over_a_mine_climbs_its_mountain_and_passes_over_its_galleries_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let circuit = &circuits[3];
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        let tunnels = Tunnels::new(&track, &circuit.scenery);
        let lap = track.lap_length();
        let step = track.step_length();
        let tunnel = circuit.scenery.tunnels[0];
        // Up and down the mountain the road is cut into its side, not sunk
        // in a canyon: past the flat ground beside it, the mountainside never
        // towers over it on both sides, and often falls away on one. (Where
        // it passes over the excavation, the rock of the excavation carries
        // it, and over the gully, its bridge.)
        let on_bridge = |along: f32| {
            circuit.scenery.bridges.iter().any(|bridge| {
                span(bridge.span.from - 20.0, bridge.span.to + 20.0).contains(along, lap)
            })
        };
        let on_mountain: Vec<usize> = (0..track.sample_count())
            .step_by(25)
            .filter(|&i| {
                let (point, road) = (track.centerline()[i], track.elevations()[i]);
                road > 30.0
                    && !tunnel.contains(i as f32 * step, lap)
                    && !on_bridge(i as f32 * step)
                    && tunnels.beneath(&track, point, BURIED + FLAT).is_none()
            })
            .collect();
        assert!(on_mountain.len() > 20, "{} points", on_mountain.len());
        let mut falls = 0;
        for &index in &on_mountain {
            let road = |lateral: f32| {
                let wall = lateral.clamp(-WALL_DISTANCE, WALL_DISTANCE);
                track.surface_point(index, wall).y
            };
            let beside =
                |lateral: f32| terrain.height(track.offset_point(index, lateral)) - road(lateral);
            for lateral in [FLAT - 1.0, 1.0 - FLAT] {
                assert!(beside(lateral).abs() < 0.3, "{index}: {}", beside(lateral));
            }
            let (left, right) = (beside(FLAT + 45.0), beside(-FLAT - 45.0));
            assert!(
                left.min(right) < 15.0,
                "a canyon at {index}: {left} {right}"
            );
            if left.min(right) < -4.0 {
                falls += 1;
            }
        }
        assert!(falls * 3 > on_mountain.len(), "falls away at {falls}");
        // Over the top it crosses the road through the mountain, high above
        // it.
        let (below, above) = (0..track.sample_count())
            .filter(|&i| tunnel.contains(i as f32 * step, lap))
            .flat_map(|i| (0..track.sample_count()).map(move |j| (i, j)))
            .filter(|&(_, j)| !tunnel.contains(j as f32 * step, lap))
            .min_by(|&(a, b), &(c, d)| {
                let gap =
                    |i: usize, j: usize| track.centerline()[i].distance(track.centerline()[j]);
                gap(a, b).total_cmp(&gap(c, d))
            })
            .expect("the track crosses the excavation");
        let point = track.centerline()[below];
        assert!(point.distance(track.centerline()[above]) < 2.0);
        let (low, high) = (track.elevations()[below], track.elevations()[above]);
        assert!(high - low > 50.0, "{low} m under {high} m");
        assert_eq!(track.nearest_index_at(point, low), below);
        assert_eq!(track.nearest_index_at(point, high), above);
        // The ground stays down in the excavation, under the rock that
        // carries the road above.
        assert!(tunnels.beneath(&track, point, BURIED).is_some());
        assert!((terrain.height(point) - low).abs() < 0.1);
    }

    #[test]
    fn an_open_pit_steps_down_to_the_water_bench_by_bench() {
        let pit = Pit {
            benches: 4,
            step: 3.0,
            tread: 8.0,
            spoil: 40.0,
        };
        let pitch = pit.tread + pit.step * PIT_FACE;
        let foot = SEA_LEVEL + PIT_FOOT;
        // Under the water, the ground is as it was.
        assert_eq!(pit_ground(&pit, -5.0, -9.0), -9.0);
        // Each bench is level, a step above the one below it, and the faces
        // between them are steep.
        for bench in 0..4 {
            let middle = (bench as f32 + 0.5) * pitch + pit.step * PIT_FACE / 2.0;
            let level = foot + (bench + 1) as f32 * pit.step;
            for along in [middle - 2.0, middle, middle + 2.0] {
                assert!((pit_ground(&pit, along, 0.0) - level).abs() < 1e-4);
            }
            let face = bench as f32 * pitch;
            let rise =
                pit_ground(&pit, face + pit.step * PIT_FACE, 0.0) - pit_ground(&pit, face, 0.0);
            assert!((rise - pit.step).abs() < 1e-4);
        }
        // Past the rim, the spoil slopes down to the ground round about, or,
        // cut into higher ground, the benches go on up into it, a step at a
        // time, until they reach it.
        let width = 4.0 * pitch;
        let rim = foot + 4.0 * pit.step;
        assert!((pit_ground(&pit, width + 1.0, 2.0) - rim).abs() < 0.1);
        assert_eq!(pit_ground(&pit, width + pit.spoil + 1.0, 2.0), 2.0);
        let above = width + 0.5 * pitch + pit.step * PIT_FACE / 2.0;
        assert!((pit_ground(&pit, above, 30.0) - (rim + pit.step)).abs() < 1e-4);
        assert!((pit_ground(&pit, above + pitch, 30.0) - (rim + 2.0 * pit.step)).abs() < 1e-4);
        assert_eq!(pit_ground(&pit, width + 10.0 * pitch, 30.0), 30.0);
    }

    pub fn a_mine_s_lake_is_the_bottom_of_an_open_pit_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let circuit = &circuits[3];
        let (Some(pit), Coast::Sea(waters)) = (&circuit.scenery.pit, &circuit.scenery.coast) else {
            panic!("{} has an open pit round its lake", circuit.name);
        };
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        let lake = Vec2::new(-1225.0, -626.0);
        assert!(waters.distance(lake) < 0.0 && terrain.height(lake) < SEA_LEVEL);
        // Out from the middle of the lake, away from the roads, past the
        // water the ground steps up bench by bench to the rim.
        let pitch = pit.tread + pit.step * PIT_FACE;
        let foot = SEA_LEVEL + PIT_FOOT;
        let mut seen = 0;
        for k in 0..24 {
            let angle = k as f32 / 24.0 * std::f32::consts::TAU;
            let out = Vec2::new(angle.cos(), angle.sin());
            let Some(shore) = (0..600)
                .map(|m| m as f32)
                .find(|&m| waters.distance(lake + out * m) >= 0.0)
            else {
                continue;
            };
            let benches = shore..shore + pit.benches as f32 * pitch;
            let clear = (0..=10).all(|i| {
                let point =
                    lake + out * (benches.start + (benches.end - benches.start) * i as f32 / 10.0);
                track.nearest(point).distance > FLAT + 40.0
            });
            if !clear {
                continue;
            }
            seen += 1;
            for bench in 0..pit.benches {
                // Where the ray crosses the middle of each bench: the shore
                // wanders, and the benches with it.
                let middle = (bench as f32 + 0.5) * pitch + pit.step * PIT_FACE / 2.0;
                let along = (0..2400)
                    .map(|q| shore + q as f32 * 0.25)
                    .find(|&m| waters.distance(lake + out * m) >= middle)
                    .expect("the ray crosses every bench");
                let level = foot + (bench + 1) as f32 * pit.step;
                let ground = terrain.height(lake + out * along);
                assert!(
                    (ground - level).abs() < 0.5 * pit.step,
                    "{k} {bench}: {ground} {level}"
                );
            }
        }
        assert!(seen >= 8, "{seen} rays clear of the roads");
    }

    pub fn a_checkered_island_has_checkerboard_cliffs_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let circuit = &circuits[2];
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        assert_eq!(terrain.soil, Soil::Checkered);
        // Sea all round, beyond the beach of the south shore and offshore
        // to the west, and the sea comes in under the middle of the bridge.
        assert!(terrain.height(Vec2::new(200.0, 260.0)) < SEA_LEVEL);
        assert!(terrain.height(Vec2::new(-300.0, -200.0)) < SEA_LEVEL);
        let bridge = circuit.scenery.bridges[0].span;
        let middle = track.index_at((bridge.from + bridge.to) / 2.0);
        assert!(terrain.height(track.offset_point(middle, 0.0)) < SEA_LEVEL);
        // The walls of the tunnel stand well above the road.
        let tunnel = circuit.scenery.tunnels[0];
        let index = track.index_at((tunnel.from + tunnel.to) / 2.0);
        let road = track.surface_point(index, 0.0).y;
        for side in [-1.0, 1.0] {
            let wall = terrain.height(track.offset_point(index, side * (FLAT + 15.0)));
            assert!(wall - road > 10.0, "{} m", wall - road);
        }
    }

    #[test]
    fn rock_walls_rise_beside_the_main_straight() {
        let (track, terrain) = bay();
        // A 200 m de la meta, la roca está a la derecha (-) y no a la izquierda.
        let index = (200.0 / track.step_length()) as usize;
        let road = track.surface_point(index, 0.0).y;
        let right = terrain.height(track.offset_point(index, -(FLAT + 15.0)));
        let left = terrain.height(track.offset_point(index, FLAT + 15.0));
        assert!(right - road > 8.0, "pared de {} m", right - road);
        assert!(left - road < 3.0);
    }

    #[test]
    fn piecewise_values_wrap_around_the_lap() {
        let points = [(100.0, 10.0), (500.0, 20.0)];
        assert_eq!(interpolate(&points, 1000.0, 300.0), 15.0);
        // Después del último, hacia el primero de la vuelta siguiente.
        assert_eq!(interpolate(&points, 1000.0, 800.0), 15.0);
        assert!((interpolate(&points, 1000.0, 50.0) - 10.833).abs() < 0.01);
    }

    #[test]
    fn signed_distance_is_positive_inside() {
        let square = [
            Vec2::ZERO,
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        assert_eq!(signed_distance(&square, Vec2::new(5.0, 3.0)), 3.0);
        assert_eq!(signed_distance(&square, Vec2::new(5.0, -2.0)), -2.0);
    }

    #[test]
    fn the_grass_is_many_greens_darker_in_forests_and_as_dark_as_its_circuit() {
        let up = Vec3::Y;
        let luma = |tint: Color| {
            let l = tint.to_linear();
            0.2126 * l.red + 0.7152 * l.green + 0.0722 * l.blue
        };
        // Not one green: their hues spread across the land.
        let warmth: Vec<f32> = (0..60)
            .map(|k| {
                let p = Vec3::new(k as f32 * 97.0, 10.0, k as f32 * 61.0);
                let l = grass_tint(p, up, 0.0).to_linear();
                l.red / l.green
            })
            .collect();
        let spread = warmth.iter().fold(f32::MIN, |a, &b| a.max(b))
            - warmth.iter().fold(f32::MAX, |a, &b| a.min(b));
        assert!(spread > 0.15, "{spread}");
        // Forests up the hills and over the wild land, much darker than the
        // meadows below; lush by the water.
        for k in 0..20 {
            let at = |height: f32| Vec3::new(k as f32 * 131.0, height, k as f32 * -77.0);
            let meadow = luma(grass_tint(at(20.0), up, 0.0));
            assert!(luma(grass_tint(at(220.0), up, 0.0)) < 0.6 * meadow);
            assert!(luma(grass_tint(at(20.0), up, 1.0)) < 0.6 * meadow);
            // Darkest in the wild, at the foot of the mountains.
            let hill = luma(grass_tint(at(220.0), up, 0.0));
            assert!(luma(grass_tint(at(220.0), up, 1.0)) < 0.75 * hill);
        }
        // A circuit's shade darkens it, its green the most.
        let tint = Color::srgb(0.9, 1.0, 0.8);
        let [r, g, b, _] = shaded(tint, 0.4);
        let linear = tint.to_linear();
        assert!((r - linear.red * 0.4).abs() < 1e-5 && (b - linear.blue * 0.4).abs() < 1e-5);
        assert!(g < linear.green * 0.4);
        assert_eq!(shaded(tint, 1.0), linear.to_f32_array());
    }

    #[test]
    fn mediterranean_greens_are_many_tones_shading_into_each_other() {
        // Over an island, on level ground: dry straw (redder than
        // the photo's green) in places and dark scrub in others...
        let up = Vec3::Y;
        let tint = |x: f32, z: f32| mediterranean_tint(Vec3::new(x, 15.0, z), up, 0.0).to_linear();
        let (mut straw, mut scrub, mut count) = (0, 0, 0);
        for i in 0..60 {
            for j in 0..60 {
                let t = tint(i as f32 * 17.0, j as f32 * -23.0);
                straw += usize::from(t.red > 1.6);
                scrub += usize::from(t.green < 0.4);
                count += 1;
            }
        }
        assert!(straw * 20 > count, "straw {straw} of {count}");
        assert!(scrub * 20 > count, "scrub {scrub} of {count}");
        // ...shading into each other, never in a step from one metre to the
        // next.
        for i in 0..2000 {
            let (x, z) = (i as f32 * 1.0, (i as f32 * 0.37).sin() * 300.0);
            let (a, b) = (tint(x, z), tint(x + 1.0, z));
            let step = (a.red - b.red).abs() + (a.green - b.green).abs() + (a.blue - b.blue).abs();
            assert!(step < 0.6, "a step of {step} at {x}, {z}");
        }
    }

    #[test]
    fn each_track_s_greens_are_its_own() {
        for k in 0..20 {
            let p = Vec3::new(k as f32 * 83.0, 15.0 + k as f32 * 9.0, k as f32 * -47.0);
            let up = Vec3::Y;
            // The lawn stays the arcade lawn, wild land or not.
            assert_eq!(greens_tint(Greens::Lawn, p, up, 1.0), lawn_tint(p, up));
            assert_eq!(greens_tint(Greens::Lawn, p, up, 0.0), lawn_tint(p, up));
            // The countryside's are its own, forest in the wild.
            assert_eq!(
                greens_tint(Greens::Countryside, p, up, 1.0),
                grass_tint(p, up, 1.0)
            );
            // A lush land's, greener than the countryside's but its
            // forests the same.
            let (lush, plain) = (
                greens_tint(Greens::Lush, p, up, 0.0).to_srgba(),
                grass_tint(p, up, 0.0).to_srgba(),
            );
            if p.y < FOREST_FROM {
                assert!(
                    lush.green / lush.red > plain.green / plain.red,
                    "{lush:?} {plain:?}"
                );
            }
            assert_eq!(
                greens_tint(Greens::Lush, p, up, 1.0),
                grass_tint(p, up, 1.0)
            );
        }
    }

    #[test]
    fn the_tallest_mountains_are_bare_over_the_tree_line_and_snowy_over_the_snow_line() {
        let up = Vec3::Y;
        let steep = Vec3::new(0.9, 0.4, 0.0).normalize();
        for k in 0..30 {
            let at = |height: f32| Vec3::new(k as f32 * 53.0, height, k as f32 * 29.0);
            assert_eq!(classify(at(200.0), up), Ground::Grass);
            // Bare above the tree line (or already under snow, where the
            // snow line dips down a ravine).
            assert_ne!(
                classify(at(TREE_LINE + TREE_LINE_REACH + 1.0), up),
                Ground::Grass
            );
            assert_eq!(classify(at(SNOW_LINE + SNOW_REACH + 1.0), up), Ground::Snow);
            assert_eq!(
                classify(at(SNOW_LINE + SNOW_REACH + 1.0), steep),
                Ground::Rock
            );
        }
    }

    #[test]
    fn turf_is_grass_up_to_as_steep_as_it_says() {
        let facing = |up: f32| Vec3::new((1.0 - up * up).sqrt(), up, 0.0);
        let ground = |up: f32, turf: Option<f32>| {
            ground_of(20.0, SNOW_LINE, TREE_LINE, facing(up), 0.0, turf)
        };
        // Elsewhere, rock wherever it is steeper than a cliff.
        assert_eq!(ground(0.9, None), Ground::Grass);
        assert_eq!(ground(0.7, None), Ground::Rock);
        // On turf, grass steeper than that, up to its own steepest.
        assert_eq!(ground(0.7, Some(0.6)), Ground::Grass);
        assert_eq!(ground(0.5, Some(0.6)), Ground::Rock);
        assert_eq!(ground(0.5, Some(0.45)), Ground::Grass);
        assert_eq!(ground(0.3, Some(0.45)), Ground::Rock);
    }

    #[test]
    fn granite_on_turf_is_green_with_moss_in_patches_and_more_on_ledges() {
        let sheer = Vec3::X;
        let ledge = Vec3::new(0.6, 0.8, 0.0);
        let greener = |[r, g, b, _]: [f32; 4]| g / (0.5 * (r + b));
        let mut mossy_faces = 0;
        let mut bare_faces = 0;
        for k in 0..400 {
            let p = Vec3::new(k as f32 * 3.7, 30.0, k as f32 * -2.3);
            assert!(mossy(p, ledge) >= mossy(p, sheer));
            // Grey off turf.
            let grey = granite_vertex_color(p, sheer, 0.0);
            assert!(greener(grey) < 1.05, "{grey:?}");
            let moss = mossy(p, sheer);
            if moss > 0.8 {
                mossy_faces += 1;
                assert!(greener(granite_vertex_color(p, sheer, 1.0)) > 1.4);
            } else if moss < 0.2 {
                bare_faces += 1;
            }
        }
        // In patches: much of it, not all.
        assert!(
            mossy_faces > 100 && bare_faces > 40,
            "{mossy_faces} {bare_faces}"
        );
    }

    #[test]
    fn axis_is_fine_inside_and_coarse_outside() {
        let axis = axis(-100.0, 100.0, 0.0);
        assert!(axis.windows(2).all(|w| w[1] > w[0]));
        assert!(axis.windows(2).all(|w| w[1] - w[0] < COARSEST + 1e-3));
        assert!(axis[0] <= -EXTENT && *axis.last().unwrap() >= EXTENT);
        let (i, _) = locate(&axis, 0.0);
        assert!((axis[i + 1] - axis[i] - FINE_SPACING).abs() < 1e-3);
    }

    pub fn snow_lies_on_down_the_gullies_of_the_mountains_and_forest_climbs_them_on(
        circuits: &'static crate::circuits::Circuits,
        name: &str,
    ) {
        let circuit = circuits
            .iter()
            .find(|circuit| circuit.name == name)
            .unwrap();
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        // About the snow line and the tree line on the volcano, by points of
        // the grid down its gullies and up its ridges.
        let share = |hollow: f32, ground: Ground, from: f32, to: f32| {
            let (mut of, mut count) = (0, 0);
            for (index, site) in terrain.sites.iter().enumerate() {
                let here = terrain.hollow[index];
                let fits = if hollow > 0.0 {
                    here > hollow
                } else {
                    here < hollow
                };
                if terrain.mountain[index] < 0.9 || !fits || !(from..to).contains(&site.height) {
                    continue;
                }
                of += 1;
                count += usize::from(look(site, None).0 == ground);
            }
            assert!(of > 20, "{of} points");
            count as f32 / of as f32
        };
        let (snow, bare) = (
            share(0.3, Ground::Snow, 480.0, 640.0),
            share(-0.3, Ground::Snow, 480.0, 640.0),
        );
        assert!(
            snow > bare + 0.2,
            "snow down the gullies {snow}, on the ridges {bare}"
        );
        let (forest, scree) = (
            share(0.3, Ground::Grass, 360.0, 460.0),
            share(-0.3, Ground::Grass, 360.0, 460.0),
        );
        assert!(
            forest > scree + 0.2,
            "forest up the gullies {forest}, the ridges {scree}"
        );
    }

    pub fn the_borders_between_grounds_run_smoothly_not_in_steps_on(
        circuits: &'static crate::circuits::Circuits,
        name: &str,
    ) {
        // The named circuit: fields of flowers, beaches, and a volcano with
        // a tree line and snow on its top.
        let circuit = circuits
            .iter()
            .find(|circuit| circuit.name == name)
            .unwrap();
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        // What is drawn at a point, against what the ground there is: drawn
        // by whole triangles, it is wrong up to half a cell either side of
        // every border, in steps; cut along the borders, hardly ever.
        let (mut cut, mut whole, mut borders) = (0, 0, 0);
        let (low, high) = (
            Vec2::new(terrain.xs[0], terrain.zs[0]),
            Vec2::new(*terrain.xs.last().unwrap(), *terrain.zs.last().unwrap()),
        );
        let step = 9.7;
        for z in 0..((high.y - low.y) / step) as usize {
            for x in 0..((high.x - low.x) / step) as usize {
                let point = low + Vec2::new(x as f32, z as f32) * step;
                let (corners, indices) = terrain.triangle_at(point);
                let [a, b, c] = corners;
                let face = (b - a).cross(c - a).normalize();
                // On the triangle, as drawn.
                let height = a.y - (face.x * (point.x - a.x) + face.z * (point.y - a.z)) / face.y;
                let truth = look(&terrain.site(Vec3::new(point.x, height, point.y)), None);
                if terrain.look_drawn(point) != truth {
                    cut += 1;
                }
                if look(&terrain.site((a + b + c) / 3.0), None) != truth {
                    whole += 1;
                }
                // Cut, a triangle's pieces cover it, turning its way.
                let looks = indices.map(|index| look(&terrain.sites[index], None));
                if looks[0] != looks[1] || looks[1] != looks[2] {
                    borders += 1;
                    let flat = corners.map(|corner| Vec2::new(corner.x, corner.z));
                    let area = |[p, q, r]: [Vec2; 3]| (q - p).perp_dot(r - p);
                    let pieces: Vec<f32> = terrain
                        .cut(corners, looks, None)
                        .into_iter()
                        .flatten()
                        .map(|(_, weights)| {
                            area(weights.map(|w| flat[0] * w.x + flat[1] * w.y + flat[2] * w.z))
                        })
                        .collect();
                    assert!(pieces.len() == 3 || pieces.len() == 6, "{pieces:?}");
                    let whole_area = area(flat);
                    assert!(pieces.iter().all(|piece| piece * whole_area > 0.0));
                    let sum: f32 = pieces.iter().sum();
                    assert!((sum - whole_area).abs() < 1e-3 * whole_area.abs());
                }
            }
        }
        assert!(
            borders > 1000 && whole > 300,
            "{borders} {whole}: borders to look at"
        );
        assert!(
            cut * 5 < whole,
            "{cut} wrong where whole triangles get {whole}"
        );
    }
}
