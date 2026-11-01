//! Arcade landmarks, where the scenery of a track asks for them
//! ([`Scenery`]): giant loops on blocks of checkerboard earth, carved totem
//! poles, sunflowers turned towards the road, lines of spinning golden rings
//! floating over it and waterfalls pouring down the rock walls.
//!
//! Each kind shares one mesh and one material wherever it can, so the GPU
//! draws them in batches. Only the rings spin and the water flows; both are
//! cheap updates of transforms and of a single material.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::{math::Affine2, prelude::*};

use super::Assets3d;
use super::props::{Footprint, row_points};
use super::quads::MeshBuilder;
use super::scenery::{Loop, Rings, Row, Totem, Waterfall};
use super::terrain::{Ground, SEA_LEVEL, Terrain};
use crate::geometry::{forward, random01};
use crate::track::{CURB_WIDTH, LOOP_HALF_WIDTH, LOOP_WALL, SURFACE_Y, Track, WALL_DISTANCE};

/// Width of the ribbon of a loop (m), how thick its earth is, and how far
/// apart its two ends are side by side, so that it does not run into itself.
const LOOP_WIDTH: f32 = 8.0;
const LOOP_THICKNESS: f32 = 2.4;
const LOOP_SHIFT: f32 = LOOP_WIDTH + 1.0;
const _: () = assert!(LOOP_SHIFT > LOOP_WIDTH);
/// Pieces of the circle of a loop, and of the straight run in and out.
const LOOP_STEPS: usize = 72;
const RUN_STEPS: usize = 6;
/// Size (m) of each repetition of the checkerboard on the loops and their
/// blocks: two by two squares, smaller than on the cliffs.
const LOOP_TILE: f32 = 3.2;
/// The band of grass along the edges of the running surface and on top of
/// the blocks (m).
const GRASS_LIP: f32 = 0.45;
/// Margin of the block of earth around the loop (m).
const BLOCK_MARGIN: f32 = 1.5;

/// Where the totem poles stand, from the centre line (m).
const TOTEM_LATERAL: f32 = WALL_DISTANCE + 3.2;
/// Each block of a totem pole: side and height (m).
const TOTEM_BLOCK: f32 = 1.5;
const TOTEM_BLOCK_HEIGHT: f32 = 1.6;

const SUNFLOWER_HEIGHT: f32 = 3.0;
const PETALS: usize = 14;

/// Rings: how far apart along the road (m), their radius and thickness (m),
/// and how fast they spin (rad/s).
const RING_SPACING: f32 = 6.0;
const RING_RADIUS: f32 = 1.0;
const RING_THICKNESS: f32 = 0.13;
const RING_SPIN: f32 = 3.5;

/// A waterfall runs from the stream on top of the cutting down to the pool
/// behind the wall (m from the centre line), in this many strips, a little
/// in front of the rock face so that it does not sink into it.
const FALLS_TOP: f32 = WALL_DISTANCE + 22.0;
const FALLS_BOTTOM: f32 = WALL_DISTANCE + 0.9;
/// Its foot, where the water lands, is where the rock rises: looked for
/// out from just past the wall, in steps of so many metres, as the last
/// point where the ground lies at most so high over the road (m), and no
/// further out than this past `FALLS_BOTTOM` (m).
const FALLS_FOOT_FROM: f32 = WALL_DISTANCE + 0.3;
const FALLS_FOOT_STEP: f32 = 0.25;
const FALLS_FOOT_RISE: f32 = 1.5;
const FALLS_FOOT_PAST: f32 = 2.0;
const FALLS_STATIONS: usize = 44;
const FALLS_IN_FRONT: f32 = 0.8;
/// Size (m) of each repetition of the water texture, and how many of them
/// flow down each second.
const FALLS_TILE: f32 = 6.0;
const FALLS_SPEED: f32 = 0.8;

/// Something that spins about its vertical axis, this many radians a second.
#[derive(Component)]
pub struct Spin(pub f32);

/// The water of the waterfalls, whose texture flows down.
#[derive(Component)]
pub struct Flowing;

pub fn spin(time: Res<Time>, mut spinning: Query<(&mut Transform, &Spin)>) {
    for (mut transform, spin) in &mut spinning {
        transform.rotate_y(spin.0 * time.delta_secs());
    }
}

/// Slides the texture of the waterfalls down: all of a track's waterfalls
/// share one material, so it is one small change per track.
pub fn flow(
    time: Res<Time>,
    falls: Query<&MeshMaterial3d<StandardMaterial>, With<Flowing>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let offset = (time.elapsed_secs() * FALLS_SPEED).fract();
    for handle in &falls {
        if let Some(mut material) = materials.get_mut(&handle.0) {
            material.uv_transform = Affine2::from_translation(Vec2::new(0.0, -offset));
        }
    }
}

/// The turn about the vertical axis that makes local +Z look towards
/// `direction` (on the ground plane).
fn facing(direction: Vec2) -> Quat {
    Quat::from_rotation_y(direction.x.atan2(direction.y))
}

/// The giant loops, each on its block of earth. Returns the ground they take.
pub fn spawn_loops(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    loops: &[Loop],
    assets: &mut Assets3d,
) -> Vec<Footprint> {
    if loops.is_empty() {
        return Vec::new();
    }
    let soil = assets.materials.add(StandardMaterial {
        base_color_texture: Some(assets.textures.soil.clone()),
        perceptual_roughness: 0.9,
        ..default()
    });
    let grass = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.7, 1.0, 0.55),
        base_color_texture: Some(assets.textures.grass.clone()),
        perceptual_roughness: 0.95,
        ..default()
    });
    loops
        .iter()
        .map(|spec| {
            let index = track.index_at(spec.along);
            let (_, heading) = track.pose_at(index);
            let ahead = forward(heading);
            let across = ahead.perp();
            let center = track.offset_point(index, spec.side.sign() * spec.lateral);
            let half = block_half_size(spec.radius);
            // The block stands just above the highest ground under it and
            // goes down below the lowest.
            let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
            for i in -4..=4 {
                for j in -2..=2 {
                    let point =
                        center + ahead * half.x * i as f32 / 4.0 + across * half.y * j as f32 / 2.0;
                    let height = terrain.height(point);
                    low = low.min(height);
                    high = high.max(height);
                }
            }
            let top = high + 0.3;
            let (earth, lawn) = loop_meshes(spec.radius, top - low + 2.0);
            let transform = Transform::from_xyz(center.x, top, center.y)
                .with_rotation(Quat::from_rotation_y((-ahead.y).atan2(ahead.x)));
            world.spawn((
                Mesh3d(assets.meshes.add(earth)),
                MeshMaterial3d(soil.clone()),
                transform,
            ));
            world.spawn((
                Mesh3d(assets.meshes.add(lawn)),
                MeshMaterial3d(grass.clone()),
                transform,
            ));
            (center, half.length())
        })
        .collect()
}

/// The loops the bikes ride (see [`crate::track::Loop`]): a running
/// surface of asphalt with curbs and white lines, low walls of checkerboard
/// earth along its edges, carrying on those of the roads in and out, all on
/// a ring of checkerboard earth. Returns the ground they take.
pub fn spawn_ridden_loops(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    curbs: [Color; 2],
    toy: bool,
    assets: &mut Assets3d,
) -> Vec<Footprint> {
    if track.loops().is_empty() {
        return Vec::new();
    }
    // A toy's (indoors): the toy cars' orange track, its edges and the ring
    // under it of orange plastic, on a blue stand.
    let asphalt = assets.materials.add(StandardMaterial {
        base_color_texture: Some(if toy {
            assets.textures.orange_track.clone()
        } else {
            assets.textures.asphalt.clone()
        }),
        perceptual_roughness: if toy { 0.38 } else { 0.88 },
        reflectance: 0.5,
        ..default()
    });
    let painted = assets.materials.add(StandardMaterial {
        perceptual_roughness: 0.7,
        ..default()
    });
    let soil = assets.materials.add(if toy {
        StandardMaterial {
            base_color: TOY_ORANGE,
            perceptual_roughness: 0.4,
            ..default()
        }
    } else {
        StandardMaterial {
            base_color_texture: Some(assets.textures.soil.clone()),
            perceptual_roughness: 0.9,
            ..default()
        }
    });
    let curbs = if toy { [TOY_ORANGE; 2] } else { curbs };
    track
        .loops()
        .iter()
        .map(|looped| {
            let [road, paint, earth] = ridden_loop_meshes(looped, curbs, !toy);
            for (mesh, material) in [(road, &asphalt), (paint, &painted), (earth, &soil)] {
                world.spawn((
                    Mesh3d(assets.meshes.add(mesh)),
                    MeshMaterial3d(material.clone()),
                ));
            }
            let middle = looped.mouth + looped.left() * looped.shift / 2.0;
            let reach =
                (looped.radius + RIDDEN_THICKNESS).hypot(looped.shift.abs() / 2.0 + RIDDEN_LIP_OUT);
            (middle, reach)
        })
        .collect()
}

/// How thick the ring of earth under the running surface of a loop the
/// bikes ride is (m), and its walls: from the middle of the running surface
/// to their inner and outer faces, and their height (m), as the walls of the
/// road into the loop.
const RIDDEN_THICKNESS: f32 = 2.4;
const RIDDEN_LIP_IN: f32 = LOOP_WALL;
const RIDDEN_LIP_OUT: f32 = LOOP_WALL + 0.6;
const RIDDEN_LIP_HEIGHT: f32 = 1.1;
/// Pieces of a loop the bikes ride, all the way round.
const RIDDEN_STEPS: usize = 120;
/// The road's paint: how far in from the edge of the asphalt the white line
/// is, how wide it is and how far above the asphalt (m).
const RIDDEN_LINE_INSET: f32 = 0.45;
const RIDDEN_LINE: f32 = 0.25;
const RIDDEN_PAINT_LIFT: f32 = 0.012;
const RIDDEN_ASPHALT_TILE: f32 = 7.0;

/// The orange of a toy loop's edges and ring.
const TOY_ORANGE: Color = Color::srgb(0.92, 0.36, 0.04);

/// The meshes of a loop the bikes ride, in the world: the asphalt of the
/// running surface, its paint (curbs in `curbs`, and white lines if
/// `lines`), and the earth (the ring and the walls along its edges).
fn ridden_loop_meshes(looped: &crate::track::Loop, curbs: [Color; 2], lines: bool) -> [Mesh; 3] {
    let (mut road, mut paint, mut earth) = (
        MeshBuilder::default(),
        MeshBuilder::default(),
        MeshBuilder::default(),
    );
    let half = LOOP_HALF_WIDTH - CURB_WIDTH;
    let step = TAU / RIDDEN_STEPS as f32;
    // A point `lateral` metres left of the middle of the running surface,
    // `height` metres off it (inwards), `angle` radians round.
    let at = |angle: f32, lateral: f32, height: f32| {
        looped.surface_point(angle, lateral) + looped.up(angle) * (height + SURFACE_Y)
    };
    for k in 0..RIDDEN_STEPS {
        let (a, b) = (k as f32 * step, (k + 1) as f32 * step);
        let (va, vb) = (a * looped.arc(), b * looped.arc());
        let up = looped.up((a + b) / 2.0);
        let across = Vec3::new(looped.left().x, 0.0, looped.left().y);
        // A strip from `from` to `to` (left of the middle) at `height`,
        // facing `facing`.
        let strip = |mesh: &mut MeshBuilder,
                     from: f32,
                     to: f32,
                     height: f32,
                     tile: f32,
                     facing: Vec3,
                     color: Color| {
            mesh.quad(
                [
                    (at(a, from, height), Vec2::new(0.0, va / tile)),
                    (at(b, from, height), Vec2::new(0.0, vb / tile)),
                    (at(b, to, height), Vec2::new((to - from) / tile, vb / tile)),
                    (at(a, to, height), Vec2::new((to - from) / tile, va / tile)),
                ],
                facing,
                color,
            );
        };
        // A wall at `lateral` from `low` to `high` off the running surface,
        // facing `facing`.
        let wall = |mesh: &mut MeshBuilder, lateral: f32, low: f32, high: f32, facing: Vec3| {
            mesh.quad(
                [
                    (
                        at(a, lateral, low),
                        Vec2::new(va / LOOP_TILE, low / LOOP_TILE),
                    ),
                    (
                        at(b, lateral, low),
                        Vec2::new(vb / LOOP_TILE, low / LOOP_TILE),
                    ),
                    (
                        at(b, lateral, high),
                        Vec2::new(vb / LOOP_TILE, high / LOOP_TILE),
                    ),
                    (
                        at(a, lateral, high),
                        Vec2::new(va / LOOP_TILE, high / LOOP_TILE),
                    ),
                ],
                facing,
                Color::WHITE,
            );
        };
        strip(
            &mut road,
            -half,
            half,
            0.0,
            RIDDEN_ASPHALT_TILE,
            up,
            Color::WHITE,
        );
        let curb = curbs[k % 2];
        for side in [1.0, -1.0] {
            let (inner, outer) = (side * half, side * LOOP_HALF_WIDTH);
            strip(
                &mut paint,
                inner.min(outer),
                inner.max(outer),
                0.0,
                1.0,
                up,
                curb,
            );
            let line = side * (half - RIDDEN_LINE_INSET);
            let (from, to) = (line, line - side * RIDDEN_LINE);
            if lines {
                strip(
                    &mut paint,
                    from.min(to),
                    from.max(to),
                    RIDDEN_PAINT_LIFT,
                    1.0,
                    up,
                    Color::WHITE,
                );
            }
            // The edge of earth beyond the curbs, the wall and its top, and
            // the side of the ring down to its outer face.
            let (edge, lip, out) = (
                side * LOOP_HALF_WIDTH,
                side * RIDDEN_LIP_IN,
                side * RIDDEN_LIP_OUT,
            );
            strip(
                &mut earth,
                edge.min(lip),
                edge.max(lip),
                0.0,
                LOOP_TILE,
                up,
                Color::WHITE,
            );
            wall(&mut earth, lip, 0.0, RIDDEN_LIP_HEIGHT, -across * side);
            strip(
                &mut earth,
                lip.min(out),
                lip.max(out),
                RIDDEN_LIP_HEIGHT,
                LOOP_TILE,
                up,
                Color::WHITE,
            );
            wall(
                &mut earth,
                out,
                -RIDDEN_THICKNESS,
                RIDDEN_LIP_HEIGHT,
                across * side,
            );
        }
        strip(
            &mut earth,
            -RIDDEN_LIP_OUT,
            RIDDEN_LIP_OUT,
            -RIDDEN_THICKNESS,
            LOOP_TILE,
            -up,
            Color::WHITE,
        );
    }
    [road.build(), paint.build(), earth.build()]
}

/// Half the size of the block of earth under a loop: along the road and
/// across it (m).
fn block_half_size(radius: f32) -> Vec2 {
    Vec2::new(
        radius + LOOP_THICKNESS + BLOCK_MARGIN,
        (LOOP_SHIFT + LOOP_WIDTH) / 2.0 + BLOCK_MARGIN,
    )
}

/// The meshes of a loop of `radius` on a block of earth `depth` metres
/// deep, in its own frame: X along the road, Y up and the top of the block
/// at Y = 0. The first is the earth, with checkerboard sides; the second,
/// the grass of the running surface, of its edges and of the top of the
/// block. The ribbon comes in along +X, goes round anticlockwise seen from
/// +Z and leaves along +X, shifted sideways (Z) so that it does not run into
/// itself, as in the loops of the platformers of the 90s.
fn loop_meshes(radius: f32, depth: f32) -> (Mesh, Mesh) {
    let mut earth = MeshBuilder::default();
    let mut lawn = MeshBuilder::default();
    let run = radius + LOOP_THICKNESS;
    let center = Vec2::new(0.0, radius + LOOP_THICKNESS);
    let shift = |t: f32| (t - 0.5) * LOOP_SHIFT;
    // The running surface: a point in XY, the normal towards where one runs
    // (inwards), and the shift in Z.
    let mut path: Vec<(Vec2, Vec2, f32)> = Vec::new();
    for k in 0..RUN_STEPS {
        let x = -run + run * k as f32 / RUN_STEPS as f32;
        path.push((Vec2::new(x, LOOP_THICKNESS), Vec2::Y, shift(0.0)));
    }
    for k in 0..=LOOP_STEPS {
        let t = k as f32 / LOOP_STEPS as f32;
        let out = Vec2::from_angle(t * TAU - FRAC_PI_2);
        path.push((center + out * radius, -out, shift(t)));
    }
    for k in 1..=RUN_STEPS {
        let x = run * k as f32 / RUN_STEPS as f32;
        path.push((Vec2::new(x, LOOP_THICKNESS), Vec2::Y, shift(1.0)));
    }

    let at = |p: Vec2, z: f32| Vec3::new(p.x, p.y, z);
    let planar = |p: Vec3| Vec2::new(p.x, -p.y) / LOOP_TILE;
    let mut along = 0.0;
    for pair in path.windows(2) {
        let [(a, na, za), (b, nb, zb)] = [pair[0], pair[1]];
        let length = a.distance(b) / LOOP_TILE;
        let normal = (na + nb).normalize();
        let inward = Vec3::new(normal.x, normal.y, 0.0);
        let (u0, u1) = (along, along + length);
        along = u1;
        let across = LOOP_WIDTH / LOOP_TILE;
        // Running surface and the earth underneath.
        for (offset, facing, builder) in [
            (0.0, inward, &mut lawn),
            (LOOP_THICKNESS, -inward, &mut earth),
        ] {
            let (a, b) = (a - na * offset, b - nb * offset);
            builder.quad(
                [
                    (at(a, za - LOOP_WIDTH / 2.0), Vec2::new(u0, 0.0)),
                    (at(b, zb - LOOP_WIDTH / 2.0), Vec2::new(u1, 0.0)),
                    (at(b, zb + LOOP_WIDTH / 2.0), Vec2::new(u1, across)),
                    (at(a, za + LOOP_WIDTH / 2.0), Vec2::new(u0, across)),
                ],
                facing,
                Color::WHITE,
            );
        }
        // The sides: a lip of grass along the running surface and the
        // checkerboard earth below it.
        for side in [-1.0, 1.0] {
            let (za, zb) = (za + side * LOOP_WIDTH / 2.0, zb + side * LOOP_WIDTH / 2.0);
            for (from, to, builder) in [
                (0.0, GRASS_LIP, &mut lawn),
                (GRASS_LIP, LOOP_THICKNESS, &mut earth),
            ] {
                let corners = [
                    at(a - na * from, za),
                    at(b - nb * from, zb),
                    at(b - nb * to, zb),
                    at(a - na * to, za),
                ];
                builder.quad(
                    corners.map(|p| (p, planar(p))),
                    Vec3::Z * side,
                    Color::WHITE,
                );
            }
        }
    }
    // The block underneath, with its lawn on top.
    let half = block_half_size(radius);
    block(
        &mut earth,
        &mut lawn,
        Vec3::new(-half.x, -depth, -half.y),
        Vec3::new(half.x, 0.0, half.y),
    );
    (earth.build(), lawn.build())
}

/// A block of earth from `min` to `max`: lawn on top and a lip of it down
/// the sides, and checkerboard earth below.
fn block(earth: &mut MeshBuilder, lawn: &mut MeshBuilder, min: Vec3, max: Vec3) {
    let corner = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    lawn.quad(
        [
            corner(min.x, max.y, min.z),
            corner(max.x, max.y, min.z),
            corner(max.x, max.y, max.z),
            corner(min.x, max.y, max.z),
        ]
        .map(|p| (p, Vec2::new(p.x, p.z) / LOOP_TILE)),
        Vec3::Y,
        Color::WHITE,
    );
    let lip = max.y - GRASS_LIP;
    // Each side: two of its corners on the ground plane, and where it faces.
    for (a, b, facing) in [
        (
            Vec2::new(min.x, min.z),
            Vec2::new(max.x, min.z),
            Vec3::NEG_Z,
        ),
        (Vec2::new(max.x, min.z), Vec2::new(max.x, max.z), Vec3::X),
        (Vec2::new(max.x, max.z), Vec2::new(min.x, max.z), Vec3::Z),
        (
            Vec2::new(min.x, max.z),
            Vec2::new(min.x, min.z),
            Vec3::NEG_X,
        ),
    ] {
        // Along the side, so that the squares are not stretched.
        let uv = |p: Vec3| Vec2::new(p.x + p.z, -p.y) / LOOP_TILE;
        for (from, to, builder) in [(max.y, lip, &mut *lawn), (lip, min.y, &mut *earth)] {
            builder.quad(
                [
                    corner(a.x, from, a.y),
                    corner(b.x, from, b.y),
                    corner(b.x, to, b.y),
                    corner(a.x, to, a.y),
                ]
                .map(|p| (p, uv(p))),
                facing,
                Color::WHITE,
            );
        }
    }
}

/// The totem poles, facing the road. Returns the ground they take.
pub fn spawn_totems(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    totems: &[Totem],
    assets: &mut Assets3d,
) -> Vec<Footprint> {
    if totems.is_empty() {
        return Vec::new();
    }
    let mesh = assets.meshes.add(totem_mesh());
    let material = assets.materials.add(StandardMaterial {
        perceptual_roughness: 0.75,
        ..default()
    });
    totems
        .iter()
        .map(|totem| {
            let index = track.index_at(totem.along);
            let point = track.offset_point(index, totem.side.sign() * TOTEM_LATERAL);
            let road = track.offset_point(index, 0.0);
            world.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(point.x, terrain.height(point) - 0.2, point.y)
                    .with_rotation(facing(road - point)),
            ));
            (point, 2.5)
        })
        .collect()
}

/// A totem pole of four carved blocks, each of its own colour, with eyes
/// and a beak on the front (+Z), and wings on top.
fn totem_mesh() -> Mesh {
    const PAINTS: [Color; 4] = [
        Color::srgb(0.42, 0.24, 0.66),
        Color::srgb(0.96, 0.55, 0.1),
        Color::srgb(0.16, 0.46, 0.86),
        Color::srgb(0.86, 0.14, 0.1),
    ];
    let white = Color::srgb(0.97, 0.97, 0.95);
    let black = Color::srgb(0.05, 0.05, 0.07);
    let beak = Color::srgb(1.0, 0.8, 0.12);
    let mut mesh = MeshBuilder::default();
    let half = TOTEM_BLOCK / 2.0;
    let front = half + 0.01;
    // A square on the front face, centred at (x, y), lifted `lift` off it.
    let square = |mesh: &mut MeshBuilder, x: f32, y: f32, size: f32, lift: f32, color: Color| {
        let s = size / 2.0;
        mesh.quad(
            [
                (Vec3::new(x - s, y - s, front + lift), Vec2::ZERO),
                (Vec3::new(x + s, y - s, front + lift), Vec2::X),
                (Vec3::new(x + s, y + s, front + lift), Vec2::ONE),
                (Vec3::new(x - s, y + s, front + lift), Vec2::Y),
            ],
            Vec3::Z,
            color,
        );
    };
    for (k, &paint) in PAINTS.iter().enumerate() {
        let bottom = k as f32 * TOTEM_BLOCK_HEIGHT;
        let top = bottom + TOTEM_BLOCK_HEIGHT;
        mesh.beam(
            Vec3::new(0.0, bottom, 0.0),
            Vec3::new(0.0, top, 0.0),
            TOTEM_BLOCK,
            TOTEM_BLOCK,
            paint,
        );
        // A dark band between the blocks.
        mesh.beam(
            Vec3::new(0.0, top - 0.08, 0.0),
            Vec3::new(0.0, top + 0.08, 0.0),
            TOTEM_BLOCK + 0.06,
            TOTEM_BLOCK + 0.06,
            black,
        );
        // The face: two eyes with pupils, a beak and a mouth.
        let eyes = bottom + TOTEM_BLOCK_HEIGHT * 0.66;
        for x in [-0.38, 0.38] {
            square(&mut mesh, x, eyes, 0.42, 0.0, white);
            square(&mut mesh, x + 0.04, eyes - 0.03, 0.2, 0.01, black);
        }
        square(&mut mesh, 0.0, bottom + 0.32, 0.8, 0.0, black);
        mesh.beam(
            Vec3::new(0.0, eyes - 0.32, half - 0.1),
            Vec3::new(0.0, eyes - 0.32, half + 0.45),
            0.34,
            0.3,
            beak,
        );
    }
    // Wings on the top block, spread out and up.
    let top = PAINTS.len() as f32 * TOTEM_BLOCK_HEIGHT;
    for side in [-1.0, 1.0] {
        for (rise, color) in [(0.0, PAINTS[1]), (0.55, beak)] {
            mesh.beam(
                Vec3::new(side * (half - 0.1), top - 1.0 + rise, 0.0),
                Vec3::new(side * (half + 1.5), top - 0.1 + rise * 1.6, 0.0),
                0.7,
                0.16,
                color,
            );
        }
    }
    mesh.build()
}

/// Sunflowers in rows, each turned towards the road. Returns the ground they
/// take.
pub fn spawn_sunflowers(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    rows: &[Row],
    assets: &mut Assets3d,
) -> Vec<Footprint> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mesh = assets.meshes.add(sunflower_mesh());
    let material = assets.materials.add(StandardMaterial {
        perceptual_roughness: 0.8,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let mut taken = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        for (k, (index, point)) in row_points(track, row).into_iter().enumerate() {
            let random = |n: u32| random01((r as u32 * 1000 + k as u32) * 8 + n + 120_000);
            if terrain.height(point) < SEA_LEVEL + 1.0 || terrain.ground(point) != Ground::Grass {
                continue;
            }
            let road = track.offset_point(index, 0.0);
            let turn = (random(0) - 0.5) * 0.6;
            world.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(point.x, terrain.height(point) - 0.1, point.y)
                    .with_rotation(facing(road - point) * Quat::from_rotation_y(turn))
                    .with_scale(Vec3::splat(0.85 + 0.3 * random(1))),
            ));
            taken.push((point, 1.2));
        }
    }
    taken
}

/// A sunflower: a green stem with two leaves and, on top, a brown disc
/// ringed with yellow petals, facing +Z.
fn sunflower_mesh() -> Mesh {
    let stem = Color::srgb(0.2, 0.52, 0.12);
    let leaf = Color::srgb(0.26, 0.62, 0.16);
    let mut mesh = MeshBuilder::default();
    mesh.beam(
        Vec3::ZERO,
        Vec3::new(0.0, SUNFLOWER_HEIGHT, 0.0),
        0.14,
        0.14,
        stem,
    );
    for (side, y) in [(1.0, 1.0), (-1.0, 1.6)] {
        mesh.quad(
            [
                (Vec3::new(0.0, y, 0.0), Vec2::ZERO),
                (Vec3::new(side * 0.55, y + 0.05, 0.15), Vec2::X),
                (Vec3::new(side * 1.0, y + 0.45, 0.1), Vec2::ONE),
                (Vec3::new(side * 0.4, y + 0.35, 0.0), Vec2::Y),
            ],
            Vec3::Z,
            leaf,
        );
    }
    let head = Vec3::new(0.0, SUNFLOWER_HEIGHT, 0.12);
    let ring = |radius: f32, angle: f32, z: f32| {
        head + Vec3::new(angle.cos() * radius, angle.sin() * radius, z)
    };
    // The back of the head, green, and the disc in front.
    for k in 0..PETALS {
        let (a0, a1) = (
            k as f32 / PETALS as f32 * TAU,
            (k + 1) as f32 / PETALS as f32 * TAU,
        );
        mesh.triangle(
            [
                (head - Vec3::Z * 0.06, Vec2::ZERO),
                (ring(0.5, a0, -0.06), Vec2::X),
                (ring(0.5, a1, -0.06), Vec2::Y),
            ],
            Vec3::NEG_Z,
            stem,
        );
        for (radius, z, color) in [
            (0.5, 0.02, Color::srgb(0.24, 0.13, 0.04)),
            (0.36, 0.05, Color::srgb(0.4, 0.22, 0.06)),
        ] {
            mesh.triangle(
                [
                    (head + Vec3::Z * z, Vec2::ZERO),
                    (ring(radius, a0, z), Vec2::X),
                    (ring(radius, a1, z), Vec2::Y),
                ],
                Vec3::Z,
                color,
            );
        }
        // A petal: a diamond from the edge of the disc outwards.
        let middle = (a0 + a1) / 2.0;
        let petal = if k % 2 == 0 {
            Color::srgb(1.0, 0.82, 0.08)
        } else {
            Color::srgb(1.0, 0.7, 0.04)
        };
        mesh.quad(
            [
                (ring(0.4, middle, 0.0), Vec2::ZERO),
                (ring(0.62, a0 + 0.05, 0.0), Vec2::X),
                (ring(1.08, middle, -0.05), Vec2::ONE),
                (ring(0.62, a1 - 0.05, 0.0), Vec2::Y),
            ],
            Vec3::Z,
            petal,
        );
    }
    mesh.build()
}

/// Lines of golden rings over the road, standing across it and spinning.
pub fn spawn_rings(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    rings: &[Rings],
    assets: &mut Assets3d,
) {
    if rings.is_empty() {
        return;
    }
    // The torus lies flat; stood up, it faces along the road.
    let mesh = assets.meshes.add(
        Mesh::from(
            Torus::new(RING_RADIUS - RING_THICKNESS, RING_RADIUS + RING_THICKNESS)
                .mesh()
                .minor_resolution(10)
                .major_resolution(32),
        )
        .rotated_by(Quat::from_rotation_x(FRAC_PI_2)),
    );
    let gold = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.78, 0.12),
        emissive: LinearRgba::rgb(0.5, 0.32, 0.02),
        metallic: 0.6,
        perceptual_roughness: 0.25,
        ..default()
    });
    for line in rings {
        for k in 0..line.count {
            let index = track.index_at(line.along + k as f32 * RING_SPACING);
            let (_, heading) = track.pose_at(index);
            let position = track.surface_point(index, line.lateral)
                + Vec3::Y * (SURFACE_Y + line.height + RING_RADIUS);
            world.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(gold.clone()),
                Transform::from_translation(position).with_rotation(Quat::from_rotation_y(heading)),
                Spin(RING_SPIN),
            ));
        }
    }
}

/// The waterfalls: one mesh for all of them, with flowing water, and puffs
/// of foam where they land.
pub fn spawn_waterfalls(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    waterfalls: &[Waterfall],
    assets: &mut Assets3d,
) {
    if waterfalls.is_empty() {
        return;
    }
    let mut water = MeshBuilder::default();
    let mut foam = Vec::new();
    for waterfall in waterfalls {
        foam.extend(waterfall_sheet(&mut water, track, terrain, waterfall));
    }
    world.spawn((
        Mesh3d(assets.meshes.add(water.build())),
        MeshMaterial3d(assets.materials.add(StandardMaterial {
            base_color_texture: Some(assets.textures.falls.clone()),
            emissive: LinearRgba::rgb(0.08, 0.12, 0.18),
            perceptual_roughness: 0.2,
            reflectance: 0.5,
            double_sided: true,
            cull_mode: None,
            ..default()
        })),
        Flowing,
    ));
    let puff = assets
        .meshes
        .add(Sphere::new(1.0).mesh().ico(2).expect("valid sphere"));
    let white = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.98, 1.0),
        emissive: LinearRgba::rgb(0.15, 0.17, 0.2),
        perceptual_roughness: 1.0,
        ..default()
    });
    for (k, at) in foam.into_iter().enumerate() {
        let size = 0.9 + 0.7 * random01(k as u32 + 130_000);
        world.spawn((
            Mesh3d(puff.clone()),
            MeshMaterial3d(white.clone()),
            Transform::from_translation(at).with_scale(Vec3::new(
                size * 1.3,
                size * 0.6,
                size * 1.3,
            )),
        ));
    }
}

/// Adds the sheet of water of a waterfall to `mesh`: draped a little in
/// front of the ground, from the stream on top of the cutting, over its
/// edge and down its face, to the pool at its foot (see [`falls_foot`]).
/// Returns where the foam goes.
fn waterfall_sheet(
    mesh: &mut MeshBuilder,
    track: &Track,
    terrain: &Terrain,
    waterfall: &Waterfall,
) -> Vec<Vec3> {
    let step = track.step_length();
    let columns = (waterfall.width / step).round().max(1.0) as usize;
    let first = track.index_at(waterfall.along - waterfall.width / 2.0);
    let side = waterfall.side.sign();
    let point = |index: usize, lateral: f32| {
        let at = track.offset_point(index, side * lateral);
        let behind = track.offset_point(index, side * (lateral + FALLS_IN_FRONT));
        let ground = terrain.height(behind).max(terrain.height(at));
        Vec3::new(at.x, ground + 0.08, at.y)
    };
    let feet: Vec<f32> = (0..=columns)
        .map(|k| falls_foot(track, terrain, first + k, side))
        .collect();
    let stations = |foot: f32| -> Vec<f32> {
        (0..=FALLS_STATIONS)
            .map(|k| FALLS_TOP.lerp(foot, k as f32 / FALLS_STATIONS as f32))
            .collect()
    };
    for k in 0..columns {
        let (i, j) = (first + k, first + k + 1);
        let (u0, u1) = (
            k as f32 * step / FALLS_TILE,
            (k + 1) as f32 * step / FALLS_TILE,
        );
        let toward_road = {
            let toward = track.offset_point(i, 0.0) - track.offset_point(i, side);
            Vec3::new(toward.x, 1.0, toward.y)
        };
        let mut v = 0.0;
        let (near, far) = (stations(feet[k]), stations(feet[k + 1]));
        for (a, b) in near.windows(2).zip(far.windows(2)) {
            let (a0, a1) = (point(i, a[0]), point(i, a[1]));
            let (b0, b1) = (point(j, b[0]), point(j, b[1]));
            let down = a0.distance(a1) / FALLS_TILE;
            mesh.quad(
                [
                    (a0, Vec2::new(u0, v)),
                    (b0, Vec2::new(u1, v)),
                    (b1, Vec2::new(u1, v + down)),
                    (a1, Vec2::new(u0, v + down)),
                ],
                toward_road,
                Color::WHITE,
            );
            v += down;
        }
    }
    (0..=columns)
        .map(|k| {
            let at = track.offset_point(first + k, side * feet[k]);
            Vec3::new(at.x, terrain.height(at) + 0.1, at.y)
        })
        .collect()
}

/// How far from the middle of the road the foot of a waterfall's rock face
/// is at sample `index`, on `side` (its sign): where the ground starts to
/// rise. The terrain is a grid coarser than the gap between the wall and
/// the rock, so how high its ground stands just past the wall varies along
/// a cliff; the water ends, and its foam lands, at the ground's own foot.
fn falls_foot(track: &Track, terrain: &Terrain, index: usize, side: f32) -> f32 {
    let mut foot = FALLS_FOOT_FROM;
    let mut lateral = FALLS_FOOT_FROM;
    while lateral <= FALLS_BOTTOM + FALLS_FOOT_PAST {
        let at = track.offset_point(index, side * lateral);
        if terrain.height(at) > track.road_height(at) + FALLS_FOOT_RISE {
            break;
        }
        foot = lateral;
        lateral += FALLS_FOOT_STEP;
    }
    foot
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    fn positions(mesh: &Mesh) -> Vec<Vec3> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(p)) => p.iter().map(|&p| p.into()).collect(),
            _ => panic!("the mesh has positions"),
        }
    }

    #[test]
    fn a_loop_stands_on_its_block_and_does_not_run_into_itself() {
        let radius = 14.0;
        let (earth, lawn) = loop_meshes(radius, 6.0);
        let all: Vec<Vec3> = positions(&earth)
            .into_iter()
            .chain(positions(&lawn))
            .collect();
        let top = all.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        let bottom = all.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        // As tall as the loop, on the block 6 m deep.
        assert!((top - 2.0 * (radius + LOOP_THICKNESS)).abs() < 0.1, "{top}");
        assert!((bottom + 6.0).abs() < 1e-3, "{bottom}");
        // Where the ribbon comes in and where it leaves, side by side.
        let half = block_half_size(radius);
        assert!(
            all.iter()
                .all(|p| p.x.abs() <= half.x + 1e-3 && p.z.abs() <= half.y + 1e-3)
        );
    }

    pub fn landmarks_of_every_circuit_stand_clear_of_the_road_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let scenery = &circuit.scenery;
            let name = circuit.name;
            // Nothing of a loop's block comes within the walls.
            for spec in scenery.loops {
                let index = track.index_at(spec.along);
                let (_, heading) = track.pose_at(index);
                let (ahead, across) = (forward(heading), forward(heading).perp());
                let center = track.offset_point(index, spec.side.sign() * spec.lateral);
                let half = block_half_size(spec.radius);
                for i in -4..=4 {
                    for j in -2..=2 {
                        let corner = center
                            + ahead * half.x * i as f32 / 4.0
                            + across * half.y * j as f32 / 2.0;
                        let distance = track.distance_to_centerline(corner);
                        assert!(
                            distance > WALL_DISTANCE + 2.0,
                            "the loop at {} of {name} comes {distance} m from the road",
                            spec.along
                        );
                    }
                }
            }
            // The rings float over the asphalt, within reach of a bike.
            for line in scenery.rings {
                assert!(
                    line.lateral.abs() < track.half_width() - RING_RADIUS,
                    "{name}"
                );
                assert!((0.3..3.0).contains(&line.height), "{name}");
            }
        }
    }

    #[test]
    fn landmarks_of_every_circuit_stand_clear_of_the_road() {
        landmarks_of_every_circuit_stand_clear_of_the_road_on(&crate::testing::DEMO);
    }

    pub fn waterfalls_pour_down_a_rock_wall_on(circuits: &'static crate::circuits::Circuits) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let terrain = Terrain::new(&track, &circuit.scenery);
            for waterfall in circuit.scenery.waterfalls {
                let mut mesh = MeshBuilder::default();
                let foam = waterfall_sheet(&mut mesh, &track, &terrain, waterfall);
                let points = positions(&mesh.build());
                let index = track.index_at(waterfall.along);
                let road = track.surface_point(index, 0.0).y;
                let top = points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
                assert!(
                    top - road > 6.0,
                    "the waterfall at {} of {} falls {} m",
                    waterfall.along,
                    circuit.name,
                    top - road
                );
                // And lands at the foot of the wall, by the road.
                for p in foam {
                    let road = track.road_height(Vec2::new(p.x, p.z));
                    assert!(
                        p.y - road < 4.0 && top - p.y > 6.0,
                        "{}: the waterfall at {}'s foam {} m up",
                        circuit.name,
                        waterfall.along,
                        p.y - road
                    );
                }
            }
        }
    }

    #[test]
    fn waterfalls_pour_down_a_rock_wall() {
        waterfalls_pour_down_a_rock_wall_on(&crate::testing::DEMO);
    }
}
