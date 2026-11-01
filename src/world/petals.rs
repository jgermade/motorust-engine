//! Petals, or leaves, drifting down through the air over an area of a
//! circuit ([`Petals`]): red maple leaves, white cherry petals, the golden
//! leaves of the mallorns. Each falls and sways over a spot of its own, a
//! function of time, with no state ([`Petal`]). The engine's.

use std::f32::consts::TAU;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::Assets3d;
use super::detail::fading_out;
use super::quads::MeshBuilder;
use super::scenery::{PetalArea, PetalKind, Petals};
use super::terrain::{SEA_LEVEL, Terrain};
use crate::geometry::random01;
use crate::track::Track;

/// Petals in the air: how many to a square metre of ground, over a road
/// and round a point (thicker, seen from afar); how high they fall from
/// (m); how fast they fall (m/s, the slowest and the quickest); how far
/// they sway (m); how far downwind they drift as they fall, per metre
/// fallen; and how far from a camera they are drawn (m).
const PETALS_ON_ROAD: f32 = 1.0 / 32.0;
const PETALS_ROUND: f32 = 1.0 / 5.0;
const PETAL_FALL_FROM: f32 = 13.0;
const PETAL_FALL: (f32, f32) = (0.7, 1.5);
const PETAL_SWAY: (f32, f32) = (0.4, 1.6);
const PETAL_DRIFT: Vec2 = Vec2::new(0.45, 0.2);
const PETALS_SEEN: f32 = 170.0;

/// A petal in the air: the ground under where it falls, how high it falls
/// from (m), how fast (m/s), where it starts (m into its fall), how far it
/// sways (m) and how fast (rad/s), and how fast it turns about each axis
/// (rad/s).
#[derive(Component, Clone, Copy, Debug)]
pub struct Petal {
    ground: Vec3,
    column: f32,
    fall: f32,
    start: f32,
    sway: f32,
    sway_rate: f32,
    spin: Vec3,
}

impl Petal {
    /// Where it is `time` seconds in: falling from the top of its column
    /// to the ground, over and over, swaying round and drifting downwind
    /// as it falls.
    fn position(&self, time: f32) -> Vec3 {
        let fallen = (self.start + time * self.fall).rem_euclid(self.column);
        let angle = self.start + time * self.sway_rate;
        let sway = Vec2::new(angle.sin(), (0.7 * angle).cos()) * self.sway;
        let drift = PETAL_DRIFT * fallen + sway;
        self.ground + Vec3::new(drift.x, self.column - fallen, drift.y)
    }

    fn rotation(&self, time: f32) -> Quat {
        let turn = self.spin * time + Vec3::splat(self.start);
        Quat::from_euler(EulerRot::YXZ, turn.y, turn.x, turn.z)
    }
}

/// Moves the petals in the air: those of the circuit in sight near enough a
/// camera to be drawn, the others as they come near (where each one is
/// follows from the time alone).
pub fn drift_petals(
    time: Res<Time>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut petals: Query<(&Petal, &InheritedVisibility, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    let eyes: Vec<Vec3> = cameras
        .iter()
        .filter(|(camera, _)| camera.is_active)
        .map(|(_, transform)| transform.translation())
        .collect();
    // Past where they are drawn, and the column they fall down and drift
    // through.
    let reach = PETALS_SEEN + 20.0 + 2.0 * PETAL_FALL_FROM;
    petals
        .par_iter_mut()
        .for_each(|(petal, visible, mut transform)| {
            let near = eyes
                .iter()
                .any(|eye| eye.distance_squared(petal.ground) < reach * reach);
            if visible.get() && near {
                transform.translation = petal.position(now);
                transform.rotation = petal.rotation(now);
            }
        });
}

/// The petals over each area, each falling over a spot of its own, with
/// its own pace and sway (see [`Petal`]).
fn plan_petals(track: &Track, terrain: &Terrain, petals: &Petals, seed: u32) -> Vec<Petal> {
    let lap = track.lap_length();
    let (count, spot): (usize, Box<dyn Fn(f32, f32) -> Vec2>) = match petals.area {
        PetalArea::Road { span, reach } => {
            let length = span.length(lap);
            (
                (length * 2.0 * reach * PETALS_ON_ROAD) as usize,
                Box::new(move |u: f32, v: f32| {
                    let index = track.index_at(span.from + u * length);
                    track.offset_point(index, (2.0 * v - 1.0) * reach)
                }),
            )
        }
        PetalArea::Round { center, radius } => (
            (std::f32::consts::PI * radius * radius * PETALS_ROUND) as usize,
            Box::new(move |u: f32, v: f32| center + Vec2::from_angle(u * TAU) * radius * v.sqrt()),
        ),
    };
    (0..count as u32)
        .map(|k| {
            let random = |n: u32| random01(seed.wrapping_add(k * 8 + n));
            let at = spot(random(0), random(1));
            let ground = terrain.height(at).max(SEA_LEVEL);
            Petal {
                ground: Vec3::new(at.x, ground, at.y),
                column: PETAL_FALL_FROM * (0.7 + 0.3 * random(2)),
                fall: PETAL_FALL.0.lerp(PETAL_FALL.1, random(3)),
                start: random(4) * PETAL_FALL_FROM,
                sway: PETAL_SWAY.0.lerp(PETAL_SWAY.1, random(5)),
                sway_rate: 0.8 + 1.2 * random(6),
                spin: Vec3::new(
                    1.0 + 3.0 * random(7),
                    0.5 + 2.0 * random(2),
                    2.0 * random(5),
                ),
            }
        })
        .collect()
}

/// The petals in the air, one mesh and one material for each kind.
pub fn spawn_petals(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    petals: &[Petals],
    assets: &mut Assets3d,
) {
    for (number, area) in petals.iter().enumerate() {
        let (mesh, color) = match area.kind {
            PetalKind::Maple => (maple_leaf_mesh(), Color::srgb(0.7, 0.22, 0.12)),
            PetalKind::Cherry => (cherry_petal_mesh(), Color::srgb(0.98, 0.93, 0.95)),
            PetalKind::Gold => (cherry_petal_mesh(), Color::srgb(0.93, 0.72, 0.24)),
        };
        let mesh = assets.meshes.add(mesh);
        // Unlit: a petal is too small, and turns too fast, to show its
        // shading, and its back would show dark.
        let material = assets.materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        let seed = 300_000 + 10_000 * number as u32;
        for petal in plan_petals(track, terrain, area, seed) {
            world.spawn((
                petal,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(petal.position(0.0)).with_rotation(petal.rotation(0.0)),
                NotShadowCaster,
                fading_out(PETALS_SEEN, 20.0, false),
            ));
        }
    }
}

/// A maple leaf (m), flat, facing +Z: five pointed lobes round its middle.
fn maple_leaf_mesh() -> Mesh {
    const LOBES: usize = 5;
    const SIZE: f32 = 0.2;
    let mut builder = MeshBuilder::default();
    let white = Color::WHITE;
    for lobe in 0..LOBES {
        // The lobes fan out above the stalk, the middle one longest.
        let middle = (lobe as f32 - 2.0) * 0.55;
        let length = SIZE * (1.0 - 0.2 * (lobe as f32 - 2.0).abs());
        let tip = Vec2::from_angle(std::f32::consts::FRAC_PI_2 + middle) * length;
        let side = |turn: f32| {
            Vec2::from_angle(std::f32::consts::FRAC_PI_2 + middle + turn) * length * 0.45
        };
        for (a, b) in [(side(-0.45), tip), (tip, side(0.45))] {
            builder.triangle(
                [
                    (Vec3::ZERO, Vec2::ZERO),
                    (a.extend(0.0), Vec2::ZERO),
                    (b.extend(0.0), Vec2::ZERO),
                ],
                Vec3::Z,
                white,
            );
        }
    }
    builder.build()
}

/// A cherry petal (m), flat, facing +Z: rounded, notched at its tip.
fn cherry_petal_mesh() -> Mesh {
    const SIZE: f32 = 0.11;
    let outline = [
        Vec2::new(0.0, 0.0),
        Vec2::new(0.45, 0.35),
        Vec2::new(0.55, 0.8),
        Vec2::new(0.25, 1.0),
        Vec2::new(0.0, 0.85),
        Vec2::new(-0.25, 1.0),
        Vec2::new(-0.55, 0.8),
        Vec2::new(-0.45, 0.35),
    ]
    .map(|p| (p * SIZE * 2.0).extend(0.0));
    let mut builder = MeshBuilder::default();
    let middle = Vec3::new(0.0, 0.5 * SIZE * 2.0, 0.0);
    for i in 0..outline.len() {
        builder.triangle(
            [
                (middle, Vec2::ZERO),
                (outline[i], Vec2::ZERO),
                (outline[(i + 1) % outline.len()], Vec2::ZERO),
            ],
            Vec3::Z,
            Color::WHITE,
        );
    }
    builder.build()
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    #[test]
    fn a_petal_falls_from_the_top_of_its_column_to_the_ground_swaying() {
        let petal = Petal {
            ground: Vec3::new(10.0, 4.0, -20.0),
            column: 12.0,
            fall: 1.0,
            start: 3.0,
            sway: 1.0,
            sway_rate: 1.5,
            spin: Vec3::ONE,
        };
        let mut lowest = f32::MAX;
        for step in 0..2_000 {
            let at = petal.position(step as f32 * 0.05);
            assert!(at.y >= 4.0 && at.y <= 16.0, "{at}");
            let away = Vec2::new(at.x - 10.0, at.z + 20.0).length();
            assert!(away <= 1.0 * 2f32.sqrt() + PETAL_DRIFT.length() * 12.0 + 1e-3);
            lowest = lowest.min(at.y);
        }
        // It reaches the ground, and starts again at the top.
        assert!(lowest < 4.1);
        let before = petal.position(8.99);
        let after = petal.position(9.01);
        assert!(after.y > before.y + 11.0, "{before} {after}");
    }

    pub fn petals_drift_over_their_areas_red_and_white_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut kinds = Vec::new();
        for circuit in circuits.iter() {
            let petals = circuit.scenery.petals;
            if petals.is_empty() {
                continue;
            }
            let track = Track::new(&circuit.layout);
            let terrain = Terrain::new(&track, &circuit.scenery);
            let lap = track.lap_length();
            kinds.extend(petals.iter().map(|petals| petals.kind));
            for (number, petals) in petals.iter().enumerate() {
                let planned = plan_petals(&track, &terrain, petals, number as u32 * 7);
                assert!(planned.len() > 100, "{}: {}", circuit.name, planned.len());
                for petal in &planned {
                    let at = Vec2::new(petal.ground.x, petal.ground.z);
                    assert!((petal.ground.y - terrain.height(at).max(SEA_LEVEL)).abs() < 1e-3);
                    match petals.area {
                        PetalArea::Road { span, reach } => {
                            let nearest = track.nearest(at);
                            assert!(nearest.distance <= reach + 0.5);
                            // Over the stretch, give or take its reach
                            // (round a bend, the nearest point of the road
                            // can be a little past its ends).
                            let around =
                                crate::world::scenery::span(span.from - reach, span.to + reach);
                            assert!(around.contains(track.along(&nearest), lap));
                        }
                        PetalArea::Round { center, radius } => {
                            assert!(at.distance(center) <= radius + 1e-3);
                        }
                    }
                }
            }
        }
        // Red maple leaves and white cherry petals, somewhere.
        for kind in [PetalKind::Maple, PetalKind::Cherry] {
            assert!(kinds.contains(&kind), "{kind:?}");
        }
    }
}
