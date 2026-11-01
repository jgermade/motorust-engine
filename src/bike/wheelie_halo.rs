//! The halo a bike's rear wheel leaves behind it on the road while it
//! rides a wheelie, on the circuits that have one (see
//! [`crate::world::scenery::Scenery::wheelie_halo`]), as a spin in a
//! platformer: soft discs of light round the rear wheel's hub, one every
//! few centimetres travelled, each left where it was, swelling a little and
//! fading out, so that the wheel draws a glowing trail behind it.
//!
//! The light is added over what is behind it, unlit and casting no shadow;
//! each disc fades through a few shared materials, so they are drawn in
//! batches.

use bevy::{light::NotShadowCaster, prelude::*};

use super::grass_kickup::{puff_image, puff_mesh};
use super::{BikeState, DespawnOnExit, GameState};
use crate::circuits::{Circuits, CurrentCircuit};
use crate::geometry::{forward, smoothstep, to_world};

/// How far behind the middle of the bike the rear wheel's hub is (m), and
/// how high over the road (m).
const HUB_BEHIND: f32 = 0.68;
const HUB_HEIGHT: f32 = 0.31;
/// How far apart along the road the discs are left (m), how wide each is
/// when it is left (m) and when it has faded out (m), and how long it
/// glows (s).
const HALO_SPACING: f32 = 0.3;
const HALO_SIZE: (f32, f32) = (0.9, 1.5);
const HALO_LIFE: f32 = 0.45;
/// How bright a fresh disc is, times the halo's colour (linear, added),
/// and how many steps it fades through.
const HALO_GLOW: f32 = 0.9;
const HALO_LEVELS: usize = 8;
/// How much of a wheelie it takes to leave a halo (see
/// [`super::chassis::Chassis::wheelie_lift`]), and how fast the bike must
/// go (m/s).
const HALO_LIFT: f32 = 0.2;
const HALO_SPEED: f32 = 3.0;

/// The mesh and the materials of the halo's discs on the circuit raced, if
/// it has one: one material for each step it fades through, brightest
/// last.
#[derive(Resource)]
pub struct HaloAssets {
    mesh: Handle<Mesh>,
    materials: Vec<Handle<StandardMaterial>>,
}

/// One disc of a halo: how long it has glowed and how long it will (s).
#[derive(Component, Debug, Clone)]
pub struct HaloDisc {
    pub lifetime: f32,
    pub level: usize,
}

/// How far each bike has gone since it last left a disc (m).
#[derive(Component, Default, Debug, Clone)]
pub struct HaloTrail {
    pub travelled: f32,
}

pub struct WheelieHaloPlugin;

impl Plugin for WheelieHaloPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Race), prepare_halo)
            .add_systems(
                Update,
                (leave_halos, fade_halos).run_if(in_state(GameState::Race)),
            );
    }
}

/// The halo's mesh and materials for the circuit raced, if it has one.
fn prepare_halo(
    mut commands: Commands,
    circuit: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(color) = circuits.get(*circuit).scenery.wheelie_halo else {
        commands.remove_resource::<HaloAssets>();
        return;
    };
    let texture = images.add(puff_image());
    let light = color.to_linear();
    let materials = (0..HALO_LEVELS)
        .map(|level| {
            materials.add(StandardMaterial {
                // Added as light is, by its colour alone: opaque.
                base_color: Color::from(
                    (light * (HALO_GLOW * halo_level_glow(level))).with_alpha(1.0),
                ),
                base_color_texture: Some(texture.clone()),
                alpha_mode: AlphaMode::Add,
                unlit: true,
                cull_mode: None,
                double_sided: true,
                ..default()
            })
        })
        .collect();
    commands.insert_resource(HaloAssets {
        mesh: meshes.add(puff_mesh()),
        materials,
    });
}

/// How bright a disc is at `level` of its fading (0 the faintest), times
/// its brightest.
fn halo_level_glow(level: usize) -> f32 {
    (level as f32 + 1.0) / HALO_LEVELS as f32
}

/// How bright a disc is (0..1) through its life (`progress`, 0..1): at
/// once, and fading out.
fn halo_glow(progress: f32) -> f32 {
    smoothstep(0.0, 0.05, progress) * (1.0 - progress).powi(2)
}

/// How wide a disc is (m) through its life (`progress`, 0..1).
fn halo_size(progress: f32) -> f32 {
    HALO_SIZE.0.lerp(HALO_SIZE.1, progress.sqrt())
}

/// Whether a bike in `state` leaves a halo: on the road, going, its front
/// wheel up.
fn haloing(state: &BikeState) -> bool {
    state.grounded
        && state.ride.is_none()
        && state.speed > HALO_SPEED
        && state.chassis.wheelie_lift() > HALO_LIFT
}

/// Leaves a disc of the halo at the rear wheel of each bike riding a
/// wheelie, one every [`HALO_SPACING`] metres it goes.
fn leave_halos(
    mut commands: Commands,
    time: Res<Time>,
    assets: Option<Res<HaloAssets>>,
    mut bikes: Query<(Entity, &BikeState, Option<&mut HaloTrail>)>,
) {
    let Some(assets) = assets else {
        return;
    };
    let dt = time.delta_secs();
    for (entity, state, trail) in &mut bikes {
        let Some(mut trail) = trail else {
            commands.entity(entity).insert(HaloTrail::default());
            continue;
        };
        if !haloing(state) {
            trail.travelled = HALO_SPACING;
            continue;
        }
        trail.travelled += state.speed * dt;
        let behind = forward(state.heading) * -HUB_BEHIND;
        let hub = to_world(state.position + behind, state.elevation + HUB_HEIGHT);
        while trail.travelled >= HALO_SPACING {
            trail.travelled -= HALO_SPACING;
            // Left back along the way the wheel came, as far as it has gone
            // since.
            let back = to_world(forward(state.heading) * -trail.travelled, 0.0);
            commands.spawn((
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(assets.materials[HALO_LEVELS - 1].clone()),
                Transform::from_translation(hub + back)
                    .with_rotation(Quat::from_rotation_y(state.heading))
                    .with_scale(Vec3::splat(HALO_SIZE.0)),
                Visibility::default(),
                NotShadowCaster,
                HaloDisc {
                    lifetime: 0.0,
                    level: HALO_LEVELS - 1,
                },
                DespawnOnExit(GameState::Race),
            ));
        }
    }
}

/// Swells and fades out each disc of the halos, and takes it away once it
/// has.
fn fade_halos(
    mut commands: Commands,
    time: Res<Time>,
    assets: Option<Res<HaloAssets>>,
    mut discs: Query<(
        Entity,
        &mut HaloDisc,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let Some(assets) = assets else {
        return;
    };
    let dt = time.delta_secs();
    for (entity, mut disc, mut transform, mut material) in &mut discs {
        disc.lifetime += dt;
        let progress = disc.lifetime / HALO_LIFE;
        if progress >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        transform.scale = Vec3::splat(halo_size(progress));
        let level =
            ((halo_glow(progress) * HALO_LEVELS as f32).ceil() as usize).clamp(1, HALO_LEVELS) - 1;
        if level != disc.level {
            disc.level = level;
            material.0 = assets.materials[level].clone();
        }
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    pub fn only_a_circuit_with_a_halo_leaves_one_on(
        circuits: &'static crate::circuits::Circuits,
        name: &str,
    ) {
        for circuit in circuits.iter() {
            let halo = circuit.scenery.wheelie_halo;
            if circuit.name == name {
                let blue = halo
                    .unwrap_or_else(|| panic!("{name} has a halo"))
                    .to_srgba();
                assert!(blue.blue > blue.green && blue.green > blue.red, "{blue:?}");
            } else {
                assert!(halo.is_none(), "{}", circuit.name);
            }
        }
    }

    #[test]
    fn only_a_circuit_with_a_halo_leaves_one() {
        only_a_circuit_with_a_halo_leaves_one_on(&crate::testing::DEMO, "");
    }

    #[test]
    fn a_disc_glows_at_once_and_fades_out_as_it_swells() {
        assert!(halo_glow(0.0) < 1e-6);
        assert!(halo_glow(0.05) > 0.85);
        let mut last = halo_glow(0.05);
        for step in 2..=20 {
            let progress = step as f32 / 20.0;
            let glow = halo_glow(progress);
            assert!(glow <= last + 1e-6, "{progress}: {glow} > {last}");
            last = glow;
            assert!(halo_size(progress) >= halo_size(progress - 0.05));
        }
        assert!(last < 1e-6);
        assert!((halo_size(0.0) - HALO_SIZE.0).abs() < 1e-6);
        assert!((halo_size(1.0) - HALO_SIZE.1).abs() < 1e-6);
        // Its brightest step as bright as a fresh disc.
        assert!((halo_level_glow(HALO_LEVELS - 1) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_bike_riding_a_wheelie_leaves_a_trail_of_discs_behind_its_rear_wheel() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(HaloAssets {
                mesh: Handle::default(),
                materials: vec![Handle::default(); HALO_LEVELS],
            })
            .add_systems(Update, leave_halos);
        let mut state = BikeState {
            speed: 25.0,
            grounded: true,
            ..BikeState::parked_at((Vec2::ZERO, 0.0))
        };
        state.chassis.wheelie = 0.4;
        app.world_mut().spawn(state);
        let dt = 0.02;
        for _ in 0..11 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(dt));
            app.update();
        }
        let discs: Vec<Vec3> = app
            .world_mut()
            .query_filtered::<&Transform, With<HaloDisc>>()
            .iter(app.world())
            .map(|transform| transform.translation)
            .collect();
        // One every few centimetres of the way gone (the first frame only
        // starts the trail), at the rear wheel's hub, behind the bike.
        let gone = 25.0 * dt * 10.0;
        let expected = (gone / HALO_SPACING) as usize;
        assert!(discs.len().abs_diff(expected) <= 1, "{} discs", discs.len());
        let behind = to_world(forward(0.0) * -HUB_BEHIND, HUB_HEIGHT);
        for disc in &discs {
            assert!((disc.y - HUB_HEIGHT).abs() < 1e-4, "{disc}");
            // Back along the way the wheel came in the frame it was left.
            let back = (*disc - behind).dot(to_world(forward(0.0), 0.0));
            assert!((-25.0 * dt - 1e-4..1e-4).contains(&back), "{disc}");
        }
    }

    #[test]
    fn a_halo_is_left_only_riding_a_wheelie() {
        let mut state = BikeState {
            speed: 25.0,
            grounded: true,
            ..BikeState::parked_at((Vec2::ZERO, 0.0))
        };
        assert!(!haloing(&state), "on both wheels");
        state.chassis.wheelie = 0.4;
        assert!(haloing(&state));
        state.grounded = false;
        assert!(!haloing(&state), "in the air");
        state.grounded = true;
        state.speed = 1.0;
        assert!(!haloing(&state), "barely going");
    }
}
