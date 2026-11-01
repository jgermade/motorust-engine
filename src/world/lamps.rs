//! The lamps of the tunnels and the lanterns of a mine's excavation light
//! what passes under them. Their light on the floor and the walls is drawn
//! into the tunnels (see [`super::structures`]) so it shows from afar; here
//! a few real point lights go to the lamps nearest the cameras, so the
//! bikes and their riders are lit as they ride under them, and the road
//! and walls round them with it. Each light fades in as its lamp comes
//! near and out as it falls behind, so none pops on or off.

use bevy::prelude::*;

use super::WorldRoot;
use crate::circuits::CurrentCircuit;
use crate::geometry::smoothstep;

/// A lamp: where it is, the colour of its light and how bright it is
/// (lm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lamp {
    pub at: Vec3,
    pub color: Color,
    pub lumens: f32,
}

/// The lamps of a circuit, on its root entity.
#[derive(Component, Clone, Debug, Default)]
pub struct Lamps(pub Vec<Lamp>);

/// One of the point lights that go to the lamps nearest the cameras.
#[derive(Component)]
struct LampLight;

/// How many point lights there are, how far from the camera a lamp gets
/// one (m), fading in over the last stretch of that (m), and how far each
/// one's light reaches (m).
const LIGHTS: usize = 12;
const REACH: f32 = 34.0;
const FADE: f32 = 14.0;
const RANGE: f32 = 13.0;

pub struct LampsPlugin;

impl Plugin for LampsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_lamp_lights)
            .add_systems(PostUpdate, light_the_lamps);
    }
}

fn spawn_lamp_lights(mut commands: Commands) {
    for _ in 0..LIGHTS {
        commands.spawn((
            LampLight,
            PointLight {
                intensity: 0.0,
                range: RANGE,
                radius: 0.3,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}

/// How bright a lamp's light is (0..1) when the nearest camera is
/// `distance` m from it: fully near, fading out towards [`REACH`].
pub fn lamp_light(distance: f32) -> f32 {
    1.0 - smoothstep(REACH - FADE, REACH, distance)
}

/// The lamps of `lamps` that get a light with cameras at `eyes`, nearest
/// first, at most `count`, each with how bright its light is (0..1).
pub fn lit_lamps(lamps: &[Lamp], eyes: &[Vec3], count: usize) -> Vec<(Lamp, f32)> {
    let mut near: Vec<(Lamp, f32)> = lamps
        .iter()
        .filter_map(|lamp| {
            let distance = eyes
                .iter()
                .map(|eye| eye.distance(lamp.at))
                .fold(f32::MAX, f32::min);
            (distance < REACH).then_some((*lamp, distance))
        })
        .collect();
    near.sort_by(|a, b| a.1.total_cmp(&b.1));
    near.truncate(count);
    near.into_iter()
        .map(|(lamp, distance)| (lamp, lamp_light(distance)))
        .collect()
}

/// Puts the point lights on the lamps of the circuit in use nearest the
/// cameras.
fn light_the_lamps(
    current: Res<CurrentCircuit>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    roots: Query<(&WorldRoot, &Lamps)>,
    mut lights: Query<(&mut Transform, &mut PointLight, &mut Visibility), With<LampLight>>,
) {
    let eyes: Vec<Vec3> = cameras.iter().map(GlobalTransform::translation).collect();
    let lamps = roots
        .iter()
        .find(|(root, _)| root.0 == current.0)
        .map_or(&[][..], |(_, lamps)| &lamps.0[..]);
    let mut lit = lit_lamps(lamps, &eyes, LIGHTS).into_iter();
    for (mut transform, mut light, mut visibility) in &mut lights {
        match lit.next() {
            Some((lamp, brightness)) => {
                transform.translation = lamp.at;
                light.color = lamp.color;
                light.intensity = lamp.lumens * brightness;
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    fn lamp(z: f32) -> Lamp {
        Lamp {
            at: Vec3::new(0.0, 6.0, z),
            color: Color::WHITE,
            lumens: 1000.0,
        }
    }

    #[test]
    fn lamps_light_up_as_they_come_near() {
        assert_eq!(lamp_light(0.0), 1.0);
        assert_eq!(lamp_light(REACH - FADE), 1.0);
        assert!(lamp_light(REACH - FADE / 2.0) > 0.3 && lamp_light(REACH - FADE / 2.0) < 0.7);
        assert_eq!(lamp_light(REACH), 0.0);
    }

    #[test]
    fn the_lights_go_to_the_lamps_nearest_the_cameras() {
        let lamps: Vec<Lamp> = (0..60).map(|k| lamp(k as f32 * -4.0)).collect();
        let eye = Vec3::new(0.0, 2.0, -60.0);
        let lit = lit_lamps(&lamps, &[eye], LIGHTS);
        assert_eq!(lit.len(), LIGHTS);
        // Nearest first, all within reach, the nearest fully bright.
        assert!((lit[0].0.at.z + 60.0).abs() < 1e-3);
        assert_eq!(lit[0].1, 1.0);
        for pair in lit.windows(2) {
            assert!(eye.distance(pair[0].0.at) <= eye.distance(pair[1].0.at));
        }
        assert!(lit.iter().all(|(lamp, _)| eye.distance(lamp.at) < REACH));
        // Far from every lamp, none.
        assert!(lit_lamps(&lamps, &[Vec3::new(500.0, 0.0, 0.0)], LIGHTS).is_empty());
        // With two cameras, lamps near either.
        let two = lit_lamps(&lamps, &[eye, Vec3::new(0.0, 2.0, -200.0)], 40);
        assert!(two.iter().any(|(lamp, _)| lamp.at.z > -90.0));
        assert!(two.iter().any(|(lamp, _)| lamp.at.z < -170.0));
    }
}
