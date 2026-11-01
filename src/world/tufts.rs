//! Grass on the shoulders of the roads where they are grass (see
//! [`Track::set_shoulders`]), never on sand, earth or concrete:
//! - tufts standing up, crossed cards of blades (see
//!   [`super::foliage::tuft`]) from the edge of the asphalt (or of its
//!   curbs) to the walls, the colour of the grass they grow in, so the
//!   ground beside the road has some depth as it rushes past;
//! - along the edge of the asphalt, a strip of grass lying over it with a
//!   ragged edge (see [`super::foliage::grass_edge`]), so the road does not
//!   end on a straight line but where the grass begins.
//!
//! They are gathered into meshes for every stretch of each road, so the
//! stretches out of sight are not drawn, and they are only drawn near the
//! camera, where they can be made out, and cast no shadows: they are many,
//! and see-through, which is dear to draw.

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

use super::detail::fading_out;
use super::foliage::ALPHA_CUTOFF;
use super::quads::MeshBuilder;
use super::road::paved_edge;
use super::terrain::{Ground, Terrain};
use super::textures::WorldTextures;
use crate::geometry::random01;
use crate::track::{CURB_WIDTH, SURFACE_Y, Surface, Track};

/// How many tufts grow on each side of each stretch of road between two
/// points of its centre line, at most.
const PER_STRETCH: u32 = 6;
/// How many points of the centre line each mesh gathers.
const CHUNK: usize = 16;
/// Up to how far from the camera the tufts and the ragged edges are drawn
/// (m, from the middle of each mesh), and over how much further they fade
/// out.
const TUFTS_REACH: f32 = 55.0;
const EDGE_REACH: f32 = 170.0;
const REACH_FADE: f32 = 12.0;

/// A mesh of grass with its material, drawn up to `reach` m from the
/// camera (from the middle of the mesh), fading out over [`REACH_FADE`]
/// more (see [`fading_out`]), and casting no shadow.
pub struct GrassPiece {
    pub mesh: Mesh,
    pub material: StandardMaterial,
    pub reach: f32,
}

impl GrassPiece {
    /// Up to where it is drawn.
    pub fn range(&self) -> VisibilityRange {
        fading_out(self.reach, REACH_FADE, true)
    }
}
/// How far from the edge of the asphalt (or of its curbs) and from the
/// walls the tufts keep (m).
const CLEAR_OF_CURB: f32 = 0.12;
const CLEAR_OF_WALL: f32 = 0.35;
/// How tall and wide a tuft is (m), at least and at most.
const HEIGHT: (f32, f32) = (0.22, 0.42);
const WIDTH: (f32, f32) = (0.45, 0.8);
/// Tufts are only where the ground is about as high as the road beside
/// it (m): not up the earth that rises before a rock face.
const LEVEL: f32 = 0.4;
/// The ragged edge of the grass: how far it reaches over the road and onto
/// the grass from the edge of the asphalt (m), what each repetition of its
/// texture covers along the road (m) and how far above the asphalt it lies
/// (m: under the paint).
const EDGE_OVER_ROAD: f32 = 0.3;
const EDGE_ON_GRASS: f32 = 0.5;
const EDGE_TILE: f32 = 3.2;
const EDGE_LIFT: f32 = 0.008;

/// The tufts of a circuit, a mesh and its material for each stretch of
/// each road that has any, given what each road's shoulders are and where
/// its solid edges are (the lap first, then the roads off it).
pub fn tuft_pieces(
    track: &Track,
    terrain: &Terrain,
    shoulders: &[Vec<[Surface; 2]>],
    edges: &[Vec<[f32; 2]>],
    textures: &WorldTextures,
) -> Vec<GrassPiece> {
    let material = StandardMaterial {
        base_color_texture: Some(textures.tuft.clone()),
        alpha_mode: AlphaMode::Mask(ALPHA_CUTOFF),
        perceptual_roughness: 0.9,
        reflectance: 0.15,
        // Lit as the ground they grow on, from either side.
        cull_mode: None,
        double_sided: false,
        ..default()
    };
    let edge_material = StandardMaterial {
        base_color_texture: Some(textures.grass_edge.clone()),
        ..material.clone()
    };
    let tufts = tuft_meshes(track, terrain, shoulders, edges, textures.grass_color);
    let fringes = edge_meshes(track, terrain, shoulders, textures.grass_color);
    let piece = |material: &StandardMaterial, reach: f32| {
        let material = material.clone();
        move |mesh: Mesh| GrassPiece {
            mesh,
            material: material.clone(),
            reach,
        }
    };
    tufts
        .into_iter()
        .map(piece(&material, TUFTS_REACH))
        .chain(fringes.into_iter().map(piece(&edge_material, EDGE_REACH)))
        .collect()
}

/// The stretches of road number `road` with grass on a side, by chunks of
/// [`CHUNK`] points of its centre line: for each, the stretches from point
/// `index` to the next with grass on `side` (0 left, 1 right, `sign` its
/// lateral sign), not where another road's asphalt is beside it.
fn grassy_stretches(
    track: &Track,
    road: usize,
    sides: &[[Surface; 2]],
) -> Vec<Vec<(usize, usize, f32)>> {
    let line = track.road(road);
    let segments = line.segment_count();
    let next = |index: usize| (index + 1) % line.sample_count();
    let grassy = |index: usize, side: usize| {
        sides.get(index).is_some_and(|s| s[side] == Surface::Grass)
            && sides
                .get(next(index))
                .is_some_and(|s| s[side] == Surface::Grass)
    };
    (0..segments)
        .step_by(CHUNK)
        .map(|chunk| {
            (chunk..(chunk + CHUNK).min(segments))
                .flat_map(|index| [(index, 0, 1.0), (index, 1, -1.0)])
                .filter(|&(index, side, sign)| {
                    // Where two roads meet, one's shoulder is the other's
                    // asphalt.
                    let on_other_road = |lateral: f32| {
                        track.on_other_road(
                            road,
                            line.offset_point(index, sign * lateral),
                            |other, nearest| other.half_width_at(nearest.index) + CURB_WIDTH + 0.5,
                        )
                    };
                    let edge = paved_edge(track, road, index);
                    grassy(index, side)
                        && !on_other_road(edge)
                        && !on_other_road(line.wall_at(index) - CLEAR_OF_WALL)
                })
                .collect()
        })
        .collect()
}

/// The ragged edges of the grass along the asphalt (see [`tuft_pieces`]),
/// the grass of `grass` colour on average.
fn edge_meshes(
    track: &Track,
    terrain: &Terrain,
    shoulders: &[Vec<[Surface; 2]>],
    grass: Color,
) -> Vec<Mesh> {
    let mut meshes = Vec::new();
    for (road, sides) in shoulders.iter().enumerate() {
        let line = track.road(road);
        let step = line.step_length();
        let next = |index: usize| (index + 1) % line.sample_count();
        let lift = SURFACE_Y + EDGE_LIFT + if road == 0 { 0.0 } else { 0.01 };
        for chunk in grassy_stretches(track, road, sides) {
            let mut mesh = MeshBuilder::default();
            for (index, _, sign) in chunk {
                // The curbs of a stretch run its whole length.
                let edge = paved_edge(track, road, index);
                let at = |i: usize, across: f32| {
                    let lateral = sign * (edge + across);
                    track.surface_point_on(road, i, lateral) + Vec3::Y * lift
                };
                // The grass's own colour where it grows (the texture is
                // pale, a little lighter than the lawn's blades on
                // average).
                let middle = at(index, 0.0);
                let tint = terrain.grass_color(middle, Vec3::Y);
                let linear = grass.to_linear();
                let color = Color::LinearRgba(LinearRgba::rgb(
                    linear.red * tint[0] * 0.95,
                    linear.green * tint[1] * 0.95,
                    linear.blue * tint[2] * 0.95,
                ));
                if terrain.ground_drawn(Vec2::new(middle.x, middle.z)) != Ground::Grass {
                    continue;
                }
                let (u0, u1) = (
                    index as f32 * step / EDGE_TILE,
                    (index + 1) as f32 * step / EDGE_TILE,
                );
                mesh.quad(
                    [
                        (at(index, -EDGE_OVER_ROAD), Vec2::new(u0, 0.0)),
                        (at(next(index), -EDGE_OVER_ROAD), Vec2::new(u1, 0.0)),
                        (at(next(index), EDGE_ON_GRASS), Vec2::new(u1, 1.0)),
                        (at(index, EDGE_ON_GRASS), Vec2::new(u0, 1.0)),
                    ],
                    Vec3::Y,
                    color,
                );
            }
            if !mesh.is_empty() {
                meshes.push(mesh.build());
            }
        }
    }
    meshes
}

/// The meshes of the tufts (see [`tuft_pieces`]), the grass of `grass`
/// colour on average.
fn tuft_meshes(
    track: &Track,
    terrain: &Terrain,
    shoulders: &[Vec<[Surface; 2]>],
    edges: &[Vec<[f32; 2]>],
    grass: Color,
) -> Vec<Mesh> {
    let mut meshes = Vec::new();
    for (road, sides) in shoulders.iter().enumerate() {
        let line = track.road(road);
        let next = |index: usize| (index + 1) % line.sample_count();
        for chunk in grassy_stretches(track, road, sides) {
            let mut mesh = MeshBuilder::default();
            for (index, side, sign) in chunk {
                let from = paved_edge(track, road, index) + CLEAR_OF_CURB;
                let edge = edges
                    .get(road)
                    .and_then(|edges| edges.get(index))
                    .map_or(line.wall_at(index), |edge| edge[side]);
                let to = edge.min(line.wall_at(index)) - CLEAR_OF_WALL;
                if to <= from {
                    continue;
                }
                let seed = (road as u32 * 100_003 + index as u32) * 2 + side as u32;
                for n in 0..PER_STRETCH {
                    let random = |k: u32| random01((seed * PER_STRETCH + n) * 8 + k);
                    // Somewhere on this stretch of shoulder.
                    let along = random(0);
                    let lateral = sign * from.lerp(to, random(1));
                    let ground = line
                        .offset_point(index, lateral)
                        .lerp(line.offset_point(next(index), lateral), along);
                    let road_height = track
                        .surface_point_on(road, index, lateral)
                        .y
                        .lerp(track.surface_point_on(road, next(index), lateral).y, along);
                    let height = terrain.height(ground);
                    if (height - road_height).abs() > LEVEL
                        || terrain.ground_drawn(ground) != Ground::Grass
                    {
                        continue;
                    }
                    let root = Vec3::new(ground.x, height - 0.04, ground.y);
                    let tint = terrain.grass_color(root, Vec3::Y);
                    add_tuft(&mut mesh, root, grass, tint, |k| random(2 + k));
                }
            }
            if !mesh.is_empty() {
                let mut mesh = mesh.build();
                normals_up(&mut mesh);
                meshes.push(mesh);
            }
        }
    }
    meshes
}

/// A tuft at `root`, of the grass's colour there (`tint`, see
/// [`Terrain::grass_color`]): two cards crossed at right angles, turned at
/// random, darker at the roots; every normal points up, so it is lit like
/// the ground (see [`tuft_pieces`]).
fn add_tuft(
    mesh: &mut MeshBuilder,
    root: Vec3,
    grass: Color,
    tint: [f32; 4],
    random: impl Fn(u32) -> f32,
) {
    let height = HEIGHT.0.lerp(HEIGHT.1, random(0));
    let width = WIDTH.0.lerp(WIDTH.1, random(1));
    let turn = random(2) * std::f32::consts::PI;
    let shade = 0.85 + 0.3 * random(3);
    let linear = grass.to_linear();
    let color = Color::LinearRgba(LinearRgba::rgb(
        linear.red * tint[0] * shade,
        linear.green * tint[1] * shade,
        linear.blue * tint[2] * shade,
    ));
    for k in 0..2 {
        let across = Vec2::from_angle(turn + k as f32 * std::f32::consts::FRAC_PI_2);
        let half = Vec3::new(across.x, 0.0, across.y) * width * 0.5;
        let up = Vec3::Y * height;
        // Leaning a little, as the wind and the bikes leave it.
        let lean = Vec3::new(random(4) - 0.5, 0.0, random(5) - 0.5) * height * 0.4;
        mesh.quad(
            [
                (root - half, Vec2::new(0.0, 1.0)),
                (root + half, Vec2::new(1.0, 1.0)),
                (root + half + up + lean, Vec2::new(1.0, 0.0)),
                (root - half + up + lean, Vec2::new(0.0, 0.0)),
            ],
            Vec3::new(-across.y, 0.0, across.x),
            color,
        );
    }
}

/// Points every normal of a mesh up.
pub(super) fn normals_up(mesh: &mut Mesh) {
    let count = mesh.count_vertices();
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count]);
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;
    use crate::world::shoulders::track_shoulders;

    pub fn tufts_grow_on_grass_shoulders_only_on(circuits: &'static crate::circuits::Circuits) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let terrain = Terrain::new(&track, &circuit.scenery);
            let shoulders = track_shoulders(&track, &terrain, &circuit.scenery);
            let edges = terrain.track_edges(&track);
            let meshes = tuft_meshes(&track, &terrain, &shoulders, &edges, Color::WHITE);
            let grass = shoulders.iter().flatten().flatten();
            let any_grass = grass.clone().any(|&s| s == Surface::Grass);
            assert_eq!(!meshes.is_empty(), any_grass, "{}", circuit.name);
            for mesh in &meshes {
                let Some(positions) = mesh
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                    .and_then(|p| p.as_float3())
                else {
                    panic!("tufts have positions");
                };
                for &[x, _, z] in positions.iter().step_by(4) {
                    let point = Vec2::new(x, z);
                    let nearest = track.nearest(point);
                    // Off the asphalt and the curbs, on grass.
                    // The curbs start and stop at a point of the centre
                    // line: either side of it, either edge.
                    let edge = (nearest.index.saturating_sub(1)..=nearest.index + 1)
                        .map(|index| paved_edge(&track, nearest.road, index))
                        .fold(f32::MAX, f32::min);
                    assert!(
                        nearest.distance > edge - 0.45,
                        "{}: a tuft on the road at {point}: road {} index {} distance {} edge {}",
                        circuit.name,
                        nearest.road,
                        nearest.index,
                        nearest.distance,
                        edge
                    );
                }
            }
        }
    }

    #[test]
    fn tufts_grow_on_grass_shoulders_only() {
        tufts_grow_on_grass_shoulders_only_on(&crate::testing::DEMO);
    }

    pub fn the_grass_edge_lies_along_the_asphalt_on_grass_shoulders_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let circuit = &circuits[0];
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        let shoulders = track_shoulders(&track, &terrain, &circuit.scenery);
        let meshes = edge_meshes(&track, &terrain, &shoulders, Color::WHITE);
        assert!(!meshes.is_empty());
        for mesh in &meshes {
            let positions = mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|p| p.as_float3())
                .unwrap();
            for &[x, y, z] in positions {
                let point = Vec2::new(x, z);
                let nearest = track.nearest(point);
                let edge = paved_edge(&track, nearest.road, nearest.index);
                // Just either side of where the asphalt ends, just above it
                // (or where the curbs start or stop, the other edge).
                assert!(
                    (nearest.distance - edge).abs() < EDGE_ON_GRASS + CURB_WIDTH + 0.1,
                    "{point}: {} vs {edge}",
                    nearest.distance
                );
                let road = track.road_height(point);
                assert!((y - road - SURFACE_Y).abs() < 0.25, "{y} over {road}");
            }
        }
    }

    #[test]
    fn the_grass_edge_lies_along_the_asphalt_on_grass_shoulders() {
        the_grass_edge_lies_along_the_asphalt_on_grass_shoulders_on(&crate::testing::DEMO);
    }
}
