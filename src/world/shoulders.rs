//! What the ground between the asphalt and the walls of each road is made
//! of (see [`Track::set_shoulders`]): the concrete walkways of the tunnels,
//! the decks of the bridges, the planks and gravel of a mine's excavation,
//! and elsewhere the terrain drawn there: grass, sand or bare earth, or, on
//! grass, the earth worn bare or the leaves fallen on it (see
//! [`super::scenery::FlowerArea::Shoulders`]).

use bevy::prelude::*;

use super::cavern::stone_at;
use super::scenery::{FlowerKind, Scenery, Soil, Stone};
use super::terrain::{Ground, Terrain};
use crate::track::{CURB_WIDTH, Dust, Surface, Track};

/// How far past the curbs the shoulder is looked at (m): a little way into
/// it, where a bike that runs wide goes, and further out. The most common
/// of the three wins; if they all differ, the nearest.
const LOOKS: [f32; 3] = [0.6, 1.6, 2.6];
/// The shoulder is looked at no closer to the walls than this (m).
const WALL_MARGIN: f32 = 0.2;

/// What the shoulders of every road of a circuit are, at each point of its
/// centre line, to the left and to the right: the lap first, then the
/// roads off it (see [`Track::set_shoulders`]).
pub fn track_shoulders(
    track: &Track,
    terrain: &Terrain,
    scenery: &Scenery,
) -> Vec<Vec<[Surface; 2]>> {
    (0..=track.side_roads().len())
        .map(|road| road_shoulders(track, road, terrain, scenery))
        .collect()
}

/// Like [`track_shoulders`], for road number `road`.
fn road_shoulders(
    track: &Track,
    road: usize,
    terrain: &Terrain,
    scenery: &Scenery,
) -> Vec<[Surface; 2]> {
    let line = track.road(road);
    let lap = track.lap_length();
    let halls = scenery.excavation().unwrap_or(&[]);
    (0..line.sample_count())
        .map(|index| {
            let along = line.lap_along_at(index);
            let in_span = |span: &super::scenery::Span| span.contains(along, lap);
            // The lap's tunnels have concrete walkways beside the road and its
            // bridges a deck of concrete or planks out to the walls. A mine's
            // tunnels are its excavation, which the branches through it share.
            let tunnel = scenery.tunnels.iter().any(in_span);
            let excavation = tunnel && scenery.excavation().is_some();
            let paved = road == 0
                && ((tunnel && !excavation)
                    || scenery.bridges.iter().any(|bridge| in_span(&bridge.span)));
            [1.0, -1.0].map(|side| {
                if paved {
                    return Surface::Paved;
                }
                let from = line.half_width_at(index) + CURB_WIDTH;
                let to = line.wall_at(index) - WALL_MARGIN;
                let seen = LOOKS.map(|look| {
                    let point = line.offset_point(index, side * (from + look).min(to));
                    if excavation {
                        stone_surface(stone_at(halls, lap, point, along))
                    } else {
                        match terrain.flowers_drawn(point) {
                            Some(FlowerKind::BareEarth | FlowerKind::FallenLeaves) => {
                                Surface::Dirt(Dust::Earth)
                            }
                            _ => terrain_surface(terrain.ground_drawn(point), terrain.soil()),
                        }
                    }
                });
                if seen[1] == seen[2] { seen[1] } else { seen[0] }
            })
        })
        .collect()
}

/// The surface of the terrain drawn as `ground` where its steep faces are of
/// `soil` (see [`super::terrain::spawn_terrain`]).
fn terrain_surface(ground: Ground, soil: Soil) -> Surface {
    match (soil, ground) {
        // Indoors, the floor: as firm as the road.
        (Soil::Floor, _) => Surface::Paved,
        // In a desert canyon the flat ground is dry, red-brown earth, and
        // by the river damp sand.
        (Soil::Mine, Ground::Grass | Ground::Rock) => Surface::Dirt(Dust::Desert),
        // On the Moon all unpaved ground (classified as Sand or Rock) is grey
        // lunar regolith dust; the Earth portal sector has Ground::Grass.
        (Soil::Lunar, Ground::Sand | Ground::Rock) => Surface::Dirt(Dust::Slate),
        (_, Ground::Sand) => Surface::Dirt(Dust::Sand),
        (_, Ground::Grass) => Surface::Grass,
        (Soil::Rock | Soil::Granite, Ground::Rock) => Surface::Dirt(Dust::Sand),
        (Soil::Checkered, Ground::Rock) => Surface::Dirt(Dust::Earth),
        // Only on the tallest mountains, never by a road.
        (_, Ground::Snow) => Surface::Grass,
    }
}

/// The floor beside the roads of a mine's excavation, or great halls', of each
/// stone (see [`super::cavern`]): planks in the galleries, gravel and earth
/// in the halls, flagstones in great halls and grey grit in natural caves.
fn stone_surface(stone: Stone) -> Surface {
    match stone {
        Stone::Timbered => Surface::Paved,
        Stone::Sandstone => Surface::Dirt(Dust::Desert),
        Stone::Slate => Surface::Dirt(Dust::Slate),
        Stone::Clay => Surface::Dirt(Dust::Earth),
        Stone::Masonry => Surface::Paved,
        Stone::Cave => Surface::Dirt(Dust::Slate),
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    /// The shoulders of a circuit, by its name, with its track and scenery.
    pub fn shoulders_of(
        circuits: &'static crate::circuits::Circuits,
        name: &str,
    ) -> (Track, &'static Scenery, Vec<Vec<[Surface; 2]>>) {
        let circuit = circuits
            .iter()
            .find(|circuit| circuit.name == name)
            .expect("the circuit exists");
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        let shoulders = track_shoulders(&track, &terrain, &circuit.scenery);
        (track, &circuit.scenery, shoulders)
    }

    /// The surfaces beside the lap over a span, both sides.
    fn along_span(
        track: &Track,
        shoulders: &[Vec<[Surface; 2]>],
        span: super::super::scenery::Span,
    ) -> Vec<Surface> {
        let lap = track.lap_length();
        (0..track.sample_count())
            .filter(|&index| span.contains(track.lap_along_at(index), lap))
            .flat_map(|index| shoulders[0][index])
            .collect()
    }

    pub fn every_road_of_every_circuit_has_its_shoulders_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let (track, _, shoulders) = shoulders_of(circuits, circuit.name);
            assert_eq!(
                shoulders.len(),
                1 + track.side_roads().len(),
                "{}",
                circuit.name
            );
            for (road, sides) in shoulders.iter().enumerate() {
                assert_eq!(sides.len(), track.road(road).sample_count());
            }
        }
    }

    #[test]
    fn every_road_of_every_circuit_has_its_shoulders() {
        every_road_of_every_circuit_has_its_shoulders_on(&crate::testing::DEMO);
    }

    pub fn tunnels_and_bridges_are_paved_beside_the_road_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let (track, scenery, shoulders) = shoulders_of(circuits, circuit.name);
            let spans = scenery.bridges.iter().map(|bridge| bridge.span).chain(
                scenery
                    .tunnels
                    .iter()
                    .copied()
                    .filter(|_| scenery.excavation().is_none()),
            );
            for span in spans {
                let surfaces = along_span(&track, &shoulders, span);
                assert!(!surfaces.is_empty());
                assert!(
                    surfaces.iter().all(|&surface| surface == Surface::Paved),
                    "{}: {surfaces:?}",
                    circuit.name
                );
            }
        }
    }

    #[test]
    fn tunnels_and_bridges_are_paved_beside_the_road() {
        tunnels_and_bridges_are_paved_beside_the_road_on(&crate::testing::DEMO);
    }

    pub fn the_bay_is_mostly_grass_and_a_mine_has_none_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (_, _, bay) = shoulders_of(circuits, circuits[0].name);
        let grass = bay[0]
            .iter()
            .flatten()
            .filter(|&&surface| surface == Surface::Grass)
            .count();
        assert!(grass > bay[0].len(), "{grass} of {}", 2 * bay[0].len());

        let with_a_mine = circuits
            .iter()
            .find(|circuit| circuit.scenery.soil == Soil::Mine)
            .expect("there is a mine");
        let (track, scenery, shoulders) = shoulders_of(circuits, with_a_mine.name);
        let all: Vec<Surface> = shoulders.iter().flatten().flatten().copied().collect();
        assert!(!all.contains(&Surface::Grass), "no grass in the desert");
        assert!(all.contains(&Surface::Dirt(Dust::Desert)));
        // Its excavation: planks in the galleries, gravel in the halls.
        let excavation = along_span(&track, &shoulders, scenery.tunnels[0]);
        assert!(excavation.contains(&Surface::Paved));
        assert!(
            excavation
                .iter()
                .any(|surface| matches!(surface, Surface::Dirt(_)))
        );
    }

    pub fn shoulders_in_patches_are_grass_and_bare_earth_on(
        circuits: &'static crate::circuits::Circuits,
        name: &str,
    ) {
        let (_, _, shoulders) = shoulders_of(circuits, name);
        let lap = &shoulders[0];
        let count = |wanted: Surface| lap.iter().flatten().filter(|&&s| s == wanted).count();
        let (grass, earth) = (count(Surface::Grass), count(Surface::Dirt(Dust::Earth)));
        let all = 2 * lap.len();
        // Neither all grass nor all earth: each a good share of the lap.
        assert!(
            grass > all / 5 && earth > all / 5,
            "{grass} grass, {earth} earth of {all}"
        );
        // In patches, not in specks: each side changes from one to the
        // other only now and then.
        for side in 0..2 {
            let changes = lap.windows(2).filter(|w| w[0][side] != w[1][side]).count();
            assert!(
                changes < lap.len() / 10,
                "{changes} changes of {}",
                lap.len()
            );
        }
    }

    #[test]
    fn rock_and_sand_beside_the_road_are_dirt() {
        assert_eq!(
            terrain_surface(Ground::Sand, Soil::Rock),
            Surface::Dirt(Dust::Sand)
        );
        assert_eq!(
            terrain_surface(Ground::Rock, Soil::Checkered),
            Surface::Dirt(Dust::Earth)
        );
        assert_eq!(
            terrain_surface(Ground::Grass, Soil::Checkered),
            Surface::Grass
        );
        assert_eq!(
            terrain_surface(Ground::Grass, Soil::Mine),
            Surface::Dirt(Dust::Desert)
        );
    }
}
