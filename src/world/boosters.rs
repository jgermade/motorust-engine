//! The toy cars' boosters, where a track has them (see
//! [`crate::track::Track::boosters`]): a narrow pod either side of the road
//! on its wall's line (a booster and the road beside it are close), red
//! with a yellow top, a black wheel in each spinning at the road's edge as a
//! booster's wheels spin against a toy car's sides, an arch of yellow and
//! black chevrons over the road between them, and yellow chevrons painted
//! on the road under it, pointing the way.
//!
//! The pods, the arch and the chevrons of every booster are built into one
//! mesh; the wheels share one mesh and one material, each spinning (see
//! [`super::landmarks::Spin`]).

use std::f32::consts::TAU;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::Assets3d;
use super::landmarks::Spin;
use super::quads::MeshBuilder;
use crate::track::{Booster, SURFACE_Y, Track};

/// A booster's pods: how wide (across the road), how tall, and how far in
/// from the road's walls their inner faces are (m).
const POD_WIDTH: f32 = 1.4;
const POD_HEIGHT: f32 = 4.0;
const POD_IN: f32 = 0.2;
/// How high the underside of the arch is over the road, and how thick it
/// is (m): well over the riders.
const ARCH_UNDER: f32 = 9.0;
const ARCH_THICK: f32 = 2.0;
/// The wheels: how big round (m), how thick, how high their middles over
/// the road, how far out of their pods' inner faces they reach towards the
/// road, and how fast they spin (rad/s).
const WHEEL_RADIUS: f32 = 1.3;
const WHEEL_THICK: f32 = 1.2;
const WHEEL_UP: f32 = 2.0;
const WHEEL_OUT: f32 = 0.6;
const WHEEL_SPIN: f32 = 14.0;

const BOOSTER_RED: Color = Color::srgb(0.88, 0.14, 0.06);
const BOOSTER_YELLOW: Color = Color::srgb(1.0, 0.8, 0.08);
const BOOSTER_BLACK: Color = Color::srgb(0.07, 0.07, 0.08);

/// Where a booster stands: the middle of its stretch of road, the way the
/// road heads there, how far its walls are, and how high its surface.
struct Site {
    middle: Vec2,
    heading: f32,
    wall: f32,
    height: f32,
}

fn site(track: &Track, booster: &Booster) -> Site {
    let road = track.road(booster.road);
    let index = ((booster.from + booster.to) / 2.0 / road.step_length()) as usize;
    let (middle, heading) = road.pose_at(index);
    Site {
        middle,
        heading,
        wall: road.wall_at(index),
        height: road.surface_point(index, 0.0).y,
    }
}

/// The boosters of `track`: their pods, arches and chevrons, and their
/// spinning wheels.
pub fn spawn_boosters(world: &mut ChildSpawnerCommands, track: &Track, assets: &mut Assets3d) {
    if track.boosters().is_empty() {
        return;
    }
    let mut body = MeshBuilder::default();
    let wheel = assets.meshes.add(wheel_mesh());
    let rubber = assets.materials.add(StandardMaterial {
        perceptual_roughness: 0.7,
        ..default()
    });
    for booster in track.boosters() {
        let site = site(track, booster);
        let rotation = Quat::from_rotation_y(site.heading);
        // In the booster's own frame: x across the road (to the right), y
        // up, z back along it (forward is -z, as a heading's).
        let at = |x: f32, y: f32, z: f32| {
            Vec3::new(site.middle.x, site.height, site.middle.y) + rotation * Vec3::new(x, y, z)
        };
        let length = booster.to - booster.from;
        let inner = site.wall - POD_IN;
        let reach = inner + POD_WIDTH;
        for side in [-1.0, 1.0] {
            let middle = side * (inner + POD_WIDTH / 2.0);
            block(
                &mut body,
                &at,
                Vec3::new(middle, POD_HEIGHT / 2.0, 0.0),
                Vec3::new(POD_WIDTH / 2.0, POD_HEIGHT / 2.0, length / 2.0),
                |normal| {
                    if normal.y > 0.5 {
                        BOOSTER_YELLOW
                    } else {
                        BOOSTER_RED
                    }
                },
            );
            // The legs of the arch, up from the pod.
            block(
                &mut body,
                &at,
                Vec3::new(middle, (POD_HEIGHT + ARCH_UNDER) / 2.0, 0.0),
                Vec3::new(0.5, (ARCH_UNDER - POD_HEIGHT) / 2.0, 1.0),
                |_| BOOSTER_RED,
            );
            // Its wheel, just out of the pod's inner face at the road.
            world.spawn((
                Mesh3d(wheel.clone()),
                MeshMaterial3d(rubber.clone()),
                Transform::from_translation(at(
                    side * (inner + WHEEL_RADIUS - WHEEL_OUT),
                    WHEEL_UP - WHEEL_THICK / 2.0,
                    0.0,
                ))
                .with_rotation(rotation),
                Spin(-side * WHEEL_SPIN),
                NotShadowCaster,
            ));
        }
        // The arch over the road, its front in yellow and black chevrons
        // pointing ahead.
        block(
            &mut body,
            &at,
            Vec3::new(0.0, ARCH_UNDER + ARCH_THICK / 2.0, 0.0),
            Vec3::new(reach, ARCH_THICK / 2.0, 1.2),
            |_| BOOSTER_RED,
        );
        for k in 0..6 {
            let x = (k as f32 + 0.5) / 6.0 * 2.0 * reach - reach;
            let color = if k % 2 == 0 {
                BOOSTER_YELLOW
            } else {
                BOOSTER_BLACK
            };
            chevron(
                &mut body,
                &|across: f32, up: f32| at(x + across, ARCH_UNDER + ARCH_THICK / 2.0 + up, 1.25),
                Vec3::Z,
                rotation,
                0.9,
                color,
            );
        }
        // Chevrons on the road under it, pointing the way.
        for k in 0..3 {
            let z = (1.0 - k as f32) * length / 3.0;
            let arrow = |across: f32, back: f32| at(across * 3.0, SURFACE_Y + 0.05, z + back * 3.0);
            road_chevron(&mut body, &arrow, BOOSTER_YELLOW);
        }
    }
    world.spawn((
        Mesh3d(assets.meshes.add(body.build())),
        MeshMaterial3d(assets.materials.add(StandardMaterial {
            perceptual_roughness: 0.4,
            ..default()
        })),
    ));
}

/// A box in a booster's frame (`at` takes it to the world), `half` its size
/// either way round `center`, each face of the colour `paint` gives the way
/// it faces in that frame.
fn block(
    mesh: &mut MeshBuilder,
    at: &impl Fn(f32, f32, f32) -> Vec3,
    center: Vec3,
    half: Vec3,
    paint: impl Fn(Vec3) -> Color,
) {
    let corner = |p: Vec3| {
        let p = center + half * p;
        at(p.x, p.y, p.z)
    };
    let world_normal = |n: Vec3| (at(n.x, n.y, n.z) - at(0.0, 0.0, 0.0)).normalize();
    for normal in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
        let (u, v) = if normal.y.abs() > 0.5 {
            (Vec3::X, Vec3::Z)
        } else {
            (normal.cross(Vec3::Y), Vec3::Y)
        };
        let point = |a: f32, b: f32| (corner(normal + u * a + v * b), Vec2::ZERO);
        mesh.quad(
            [
                point(-1.0, -1.0),
                point(1.0, -1.0),
                point(1.0, 1.0),
                point(-1.0, 1.0),
            ],
            world_normal(normal),
            paint(normal),
        );
    }
}

/// A chevron pointing up on a face (`at` takes a point across it and up it
/// to the world), `size` tall, facing `facing` in the booster's frame
/// turned by `rotation`.
fn chevron(
    mesh: &mut MeshBuilder,
    at: &impl Fn(f32, f32) -> Vec3,
    facing: Vec3,
    rotation: Quat,
    size: f32,
    color: Color,
) {
    let facing = rotation * facing;
    for side in [-1.0, 1.0] {
        mesh.quad(
            [
                (at(0.0, size), Vec2::ZERO),
                (at(side * size, 0.0), Vec2::X),
                (at(side * size, -size * 0.5), Vec2::ONE),
                (at(0.0, size * 0.5), Vec2::Y),
            ],
            facing,
            color,
        );
    }
}

/// A chevron painted on the road, pointing ahead (`at` takes a point across
/// the road and back along it, in units of the chevron, to the world).
fn road_chevron(mesh: &mut MeshBuilder, at: &impl Fn(f32, f32) -> Vec3, color: Color) {
    for side in [-1.0, 1.0] {
        mesh.quad(
            [
                (at(0.0, -1.0), Vec2::ZERO),
                (at(side, 0.0), Vec2::X),
                (at(side, 0.6), Vec2::ONE),
                (at(0.0, -0.4), Vec2::Y),
            ],
            Vec3::Y,
            color,
        );
    }
}

/// A booster's wheel, standing on its underside at the origin: a tyre round
/// its side, its tread in black and grey blocks, and its top in yellow and
/// black segments, so that it is seen to spin.
fn wheel_mesh() -> Mesh {
    const SIDES: usize = 16;
    let mut mesh = MeshBuilder::default();
    let around = |k: usize| {
        let angle = k as f32 / SIDES as f32 * TAU;
        Vec3::new(angle.cos(), 0.0, angle.sin()) * WHEEL_RADIUS
    };
    let up = Vec3::Y * WHEEL_THICK;
    for k in 0..SIDES {
        let (a, b) = (around(k), around(k + 1));
        mesh.quad(
            [
                (a, Vec2::ZERO),
                (b, Vec2::X),
                (b + up, Vec2::ONE),
                (a + up, Vec2::Y),
            ],
            (a + b).normalize(),
            if k % 2 == 0 {
                BOOSTER_BLACK
            } else {
                Color::srgb(0.32, 0.32, 0.34)
            },
        );
        for (height, facing) in [(up, Vec3::Y), (Vec3::ZERO, Vec3::NEG_Y)] {
            let color = if k % 4 < 2 {
                BOOSTER_YELLOW
            } else {
                BOOSTER_BLACK
            };
            mesh.triangle(
                [
                    (height, Vec2::ZERO),
                    (a * 0.8 + height, Vec2::X),
                    (b * 0.8 + height, Vec2::Y),
                ],
                facing,
                color,
            );
        }
    }
    mesh.build()
}
