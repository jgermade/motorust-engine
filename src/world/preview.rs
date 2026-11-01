//! A quick look at the start of a circuit, for the tracks menu seen from the
//! road (Options > Game > the view while choosing the track, "Trackside"):
//! the camera flies on from just before the line whenever another track is
//! chosen, so a circuit not yet prepared can be shown there long before the
//! whole of it is. Its terrain is fine and shaped by the roads only round
//! the start's stretch, and the land as it would be with no road elsewhere,
//! in cells growing coarser away from it (see
//! [`super::terrain::Terrain::shaped_within`]); its roads are cut to that
//! stretch, with its tunnels, bridges and arches, and the sea mirrors only
//! the sky. Prepared away from the main thread, a few times quicker than the
//! circuit's own first part (see [`super::Prepared`]), which takes its place
//! when it comes.

use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;

use super::scenery::{FlowerKind, Span, span};
use super::terrain::{GroundMesh, Terrain};
use super::textures::WorldTextures;
use super::{Basis, Prepared, Roadside, Urgency, dam, foliage, mirror, road, sea, structures};
use crate::circuits::Circuit;
use crate::track::Track;
use std::sync::{Arc, Mutex};

/// The stretch of a lap the look takes in (m): from this far before the
/// line to this far past it. The camera flies on at about 22 m/s from 90 m
/// before the line, so it stays in the stretch for some 25 s, longer than
/// the circuit takes to come.
const BEHIND: f32 = 120.0;
const AHEAD: f32 = 450.0;

/// The stretch of a lap of `lap` metres that the look takes in.
pub fn stretch(lap: f32) -> Span {
    span(lap - BEHIND, AHEAD)
}

/// The box round the centre line of `track` along `stretch` (its corners).
fn stretch_box(track: &Track, stretch: Span) -> (Vec2, Vec2) {
    let lap = track.lap_length();
    track
        .centerline()
        .iter()
        .enumerate()
        .filter(|&(index, _)| stretch.contains(index as f32 * track.step_length(), lap))
        .fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(min, max), (_, &point)| (min.min(point), max.max(point)),
        )
}

/// A quick look at the start of circuit `index` (see the module), as the
/// first part of a circuit is: put in the world by [`super::build`], marked
/// as a look.
pub(super) fn prepare_preview(
    circuit: &'static Circuit,
    index: usize,
    textures: &WorldTextures,
    urgency: Urgency,
) -> Prepared {
    let scenery = &circuit.scenery;
    let track = Track::new(&circuit.layout);
    let stretch = stretch(track.lap_length());
    let near = stretch_box(&track, stretch);
    let terrain = Terrain::shaped_within(&track, scenery, urgency.threads(), near);
    let mut ground = terrain.meshes(urgency.threads());
    let shore = sea::ShoreMap::new(&terrain, &track);
    let shore_image = shore.image();
    // The roads near the start, cut where the land is no longer shaped for
    // them.
    let shaped = (
        near.0 - Vec2::splat(super::terrain::FINE_MARGIN),
        near.1 + Vec2::splat(super::terrain::FINE_MARGIN),
    );
    let mut pieces: Vec<_> = road::road_pieces(&track, &terrain, scenery, textures)
        .into_iter()
        .filter_map(|(mesh, material)| Some((cut_to(mesh, shaped)?, material)))
        .collect();
    let (structures, lamps, tops) =
        structures::structure_pieces(&track, &terrain, scenery, textures, Some(stretch));
    pieces.extend(structures);
    ground.extend(tops);
    let sea_mirror = (!scenery.cartoon_water).then(|| {
        mirror::sky_mirror(
            scenery,
            &sea::mirror_reach(&track),
            mirror::deep(scenery.water),
        )
    });
    let meadows = meadows(&ground, textures);
    Prepared {
        basis: Basis {
            index,
            circuit,
            track: Arc::new(track),
            terrain: Arc::new(terrain),
            shore: Arc::new(shore),
            roadside: Roadside {
                edges: Vec::new(),
                shoulders: Vec::new(),
            },
        },
        ground,
        shore_image,
        sea_mirror,
        pieces,
        lakes: Vec::<dam::PreparedLake>::new(),
        lamps,
        meadows,
    }
}

/// One meadow for each kind of flower `ground` is drawn in (the tunnels'
/// tops drawn in the same as the ground round them).
pub(super) fn meadows(ground: &[GroundMesh], textures: &WorldTextures) -> Vec<(FlowerKind, Image)> {
    let mut kinds = Vec::new();
    for kind in ground.iter().filter_map(|ground| ground.flowers) {
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds
        .into_iter()
        .map(|kind| (kind, meadow(textures, kind)))
        .collect()
}

/// The lawn in flower of `kind` (see [`foliage::meadow`]): the same for
/// every circuit, and for a circuit's look at its start and its first part,
/// so each is made once (it takes a tenth of a second or so).
fn meadow(textures: &WorldTextures, kind: FlowerKind) -> Image {
    static MADE: Mutex<Vec<(FlowerKind, Image)>> = Mutex::new(Vec::new());
    let made = |made: &[(FlowerKind, Image)]| {
        made.iter()
            .find(|(made, _)| *made == kind)
            .map(|(_, meadow)| meadow.clone())
    };
    if let Some(meadow) = made(&MADE.lock().expect("the meadows are not poisoned")) {
        return meadow;
    }
    let meadow = foliage::meadow(&textures.lawn, kind);
    MADE.lock()
        .expect("the meadows are not poisoned")
        .push((kind, meadow.clone()));
    meadow
}

/// `mesh` (a list of triangles) with only those whose middle is within the
/// box `(min, max)` (x and z), if any is.
fn cut_to(mut mesh: Mesh, (min, max): (Vec2, Vec2)) -> Option<Mesh> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION).cloned()
    else {
        return Some(mesh);
    };
    let indices: Vec<u32> = match mesh.indices() {
        Some(indices) => indices.iter().map(|index| index as u32).collect(),
        None => (0..positions.len() as u32).collect(),
    };
    let kept: Vec<u32> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|corners| {
            let middle = corners
                .iter()
                .map(|&corner| Vec3::from(positions[corner as usize]))
                .sum::<Vec3>()
                / 3.0;
            let at = middle.xz();
            at.cmpge(min).all() && at.cmple(max).all()
        })
        .flatten()
        .copied()
        .collect();
    if kept.is_empty() {
        return None;
    }
    mesh.insert_indices(Indices::U32(kept));
    Some(mesh)
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::PrimitiveTopology;

    #[test]
    fn the_look_takes_in_the_start_and_its_run_up() {
        let lap = 2000.0;
        let stretch = stretch(lap);
        for along in [0.0, 100.0, 400.0, 1900.0, 1999.0] {
            assert!(stretch.contains(along, lap), "{along}");
        }
        for along in [500.0, 1000.0, 1850.0] {
            assert!(!stretch.contains(along, lap), "{along}");
        }
    }

    #[test]
    fn roads_are_cut_to_the_stretch_they_are_shaped_for() {
        // Two triangles, one in the box and one out of it.
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [50.0, 0.0, 50.0],
                [51.0, 0.0, 50.0],
                [50.0, 0.0, 51.0],
            ],
        )
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 3, 4, 5]));
        let cut = cut_to(mesh.clone(), (Vec2::splat(-10.0), Vec2::splat(10.0))).unwrap();
        assert_eq!(cut.indices().unwrap().len(), 3);
        assert!(cut_to(mesh, (Vec2::splat(100.0), Vec2::splat(110.0))).is_none());
    }

    pub fn a_look_at_the_start_is_quicker_to_work_out_than_the_whole_on(
        circuits: &'static crate::circuits::Circuits,
        name: &str,
    ) {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let textures = WorldTextures::from_world(&mut world);
        // The named circuit, among the longest to prepare: its grid of heights
        // under half the whole circuit's (fine round the start, and across
        // the grid in its rows and columns there), its roads only near the
        // start.
        let index = circuits
            .iter()
            .position(|circuit| circuit.name == name)
            .unwrap_or_else(|| panic!("{name} is a circuit"));
        let look = prepare_preview(&circuits[index], index, &textures, Urgency::Now);
        let whole = Terrain::on_threads(
            &Track::new(&circuits[index].layout),
            &circuits[index].scenery,
            Urgency::Now.threads(),
        );
        let points = |terrain: &Terrain| terrain.grid_points();
        assert!(
            points(&look.basis.terrain) * 2 < points(&whole),
            "{} {}",
            points(&look.basis.terrain),
            points(&whole)
        );
        assert!(!look.pieces.is_empty());
    }
}
