//! What the wheels of a bike throw up off the road: grass blades and clumps
//! of soil on grass, and a cloud of dust of the ground's colour on bare
//! earth, sand or gravel (see [`Surface`]). On the concrete of a tunnel or
//! the deck of a bridge nothing flies.
//!
//! Higher bike speeds kick up more of it, backwards and upwards from the
//! rear wheel, providing clear visual feedback for loss of speed and
//! traction.

use std::f32::consts::TAU;

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::Indices,
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
};

use super::{BikeState, DespawnOnExit, GameState};
use crate::geometry::{fbm, forward, smoothstep, to_world};
use crate::track::{Dust, Surface, Track};
use crate::world::generate_mipmaps;

/// Per-bike accumulator for continuous particle emission based on forward speed.
#[derive(Component, Default, Debug, Clone)]
pub struct GrassKickup {
    pub accumulator: f32,
}

/// A puff of dust raised by a wheel: it swells and thins out as it hangs
/// in the air.
#[derive(Component, Debug, Clone)]
pub struct DustPuff {
    pub velocity: Vec3,
    pub spin: f32,
    pub lifetime: f32,
    pub max_lifetime: f32,
    /// How wide it starts and how wide it ends up (m).
    pub size: (f32, f32),
    pub dust: Dust,
    /// Its material now (see [`DustAssets::material`]).
    pub level: usize,
}

/// How many steps of transparency the dust fades through: one shared
/// material each, so the puffs are still drawn in batches.
const DUST_LEVELS: usize = 8;
/// How thick a fresh puff is at most (alpha).
const DUST_OPACITY: f32 = 0.8;
/// How many puffs a wheel raises per second for each m/s of speed.
const DUST_RATE: f32 = 0.85;
/// How much of the bike's speed the air in its wake carries the dust along
/// at first, at least.
const DUST_CARRY: f32 = 0.45;

/// The colour of each kind of dust (sRGB), a little lighter than the
/// ground it comes off, as dust in the air is.
fn dust_color(dust: Dust) -> Color {
    match dust {
        Dust::Sand => Color::srgb(0.92, 0.87, 0.74),
        Dust::Earth => Color::srgb(0.7, 0.57, 0.43),
        Dust::Desert => Color::srgb(0.92, 0.7, 0.55),
        Dust::Slate => Color::srgb(0.55, 0.54, 0.55),
    }
}

const DUSTS: [Dust; 4] = [Dust::Sand, Dust::Earth, Dust::Desert, Dust::Slate];

/// The shared mesh of the puffs and their materials: one per kind of dust
/// and step of transparency.
#[derive(Resource)]
pub struct DustAssets {
    pub mesh: Handle<Mesh>,
    materials: Vec<Vec<Handle<StandardMaterial>>>,
}

impl DustAssets {
    /// The material of `dust` at transparency step `level` (0 the
    /// thickest).
    pub fn material(&self, dust: Dust, level: usize) -> Handle<StandardMaterial> {
        let kind = DUSTS
            .iter()
            .position(|&d| d == dust)
            .expect("every dust has its materials");
        self.materials[kind][level.min(DUST_LEVELS - 1)].clone()
    }
}

impl FromWorld for DustAssets {
    fn from_world(world: &mut World) -> Self {
        let mesh = world.resource_mut::<Assets<Mesh>>().add(puff_mesh());
        let texture = world.resource_mut::<Assets<Image>>().add(puff_image());
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let materials = DUSTS
            .iter()
            .map(|&dust| {
                (0..DUST_LEVELS)
                    .map(|level| {
                        let alpha = DUST_OPACITY * dust_alpha(level);
                        let color = dust_color(dust);
                        materials.add(StandardMaterial {
                            base_color: color.with_alpha(alpha),
                            base_color_texture: Some(texture.clone()),
                            // Dust in the air scatters the light: it glows
                            // a little even in the shade.
                            emissive: color.to_linear() * 0.25,
                            alpha_mode: AlphaMode::Blend,
                            perceptual_roughness: 1.0,
                            reflectance: 0.0,
                            // Every face of a puff is lit as if it faced up,
                            // like the ground it rose from, from either side.
                            cull_mode: None,
                            double_sided: false,
                            ..default()
                        })
                    })
                    .collect()
            })
            .collect();
        Self { mesh, materials }
    }
}

/// How thick (0..1) the dust is at transparency step `level`.
fn dust_alpha(level: usize) -> f32 {
    1.0 - level as f32 / DUST_LEVELS as f32
}

/// The transparency step nearest to `alpha` (0..1).
fn dust_level(alpha: f32) -> usize {
    (((1.0 - alpha) * DUST_LEVELS as f32).round() as usize).min(DUST_LEVELS - 1)
}

/// Physical state of an airborne grass particle.
#[derive(Component, Debug, Clone)]
pub struct GrassParticle {
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub initial_scale: Vec3,
}

/// Shared mesh and material resources for batched GPU grass rendering.
#[derive(Resource)]
pub struct GrassAssets {
    pub meshes: Vec<Handle<Mesh>>,
    pub material: Handle<StandardMaterial>,
}

impl FromWorld for GrassAssets {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let mesh_handles = vec![
            meshes.add(grass_blade_mesh(0)),
            meshes.add(grass_blade_mesh(1)),
            meshes.add(grass_blade_mesh(2)),
            meshes.add(grass_clump_mesh()),
        ];
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let material = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.85,
            reflectance: 0.22,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        Self {
            meshes: mesh_handles,
            material,
        }
    }
}

pub struct GrassKickupPlugin;

impl Plugin for GrassKickupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GrassAssets>()
            .init_resource::<DustAssets>()
            .add_systems(
                Update,
                (kick_up_grass, update_grass_particles, update_dust_puffs)
                    .run_if(in_state(GameState::Race)),
            );
    }
}

/// A puff of dust: four soft discs through its middle, three standing at
/// 60° from each other and one lying flat, so it looks round from anywhere.
/// One metre across; every normal points up (see [`DustAssets`]).
pub(super) fn puff_mesh() -> Mesh {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    let mut quad = |a: Vec3, b: Vec3| {
        let base = positions.len() as u32;
        for (corner, uv) in [
            (-a - b, [0.0, 1.0]),
            (a - b, [1.0, 1.0]),
            (a + b, [1.0, 0.0]),
            (-a + b, [0.0, 0.0]),
        ] {
            positions.push((corner * 0.5).to_array());
            uvs.push(uv);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    for k in 0..3 {
        let angle = k as f32 * TAU / 6.0;
        quad(Vec3::new(angle.cos(), 0.0, angle.sin()), Vec3::Y);
    }
    quad(Vec3::X, Vec3::Z);
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// A soft, billowing disc of white, clear at its edge.
pub(super) fn puff_image() -> Image {
    const SIZE: u32 = 64;
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = (Vec2::new(x as f32, y as f32) + 0.5) / SIZE as f32 * 2.0 - 1.0;
            // Lumpy at the edge and thicker in billows inside.
            let billow = fbm(p * 2.6 + 7.0, 3);
            let radius = 0.72 + 0.22 * (billow - 0.5);
            let edge = 1.0 - smoothstep(radius * 0.35, radius, p.length());
            let alpha = edge * (0.65 + 0.35 * fbm(p * 5.0 - 3.0, 2));
            let shade = (215.0 + 40.0 * billow).min(255.0) as u8;
            data.extend([shade, shade, shade, (alpha * 255.0).round() as u8]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    generate_mipmaps(&mut image);
    image
}

/// Slender grass blade mesh with vertex color gradient from root to tip.
fn grass_blade_mesh(variant: u32) -> Mesh {
    let (w, h) = match variant {
        0 => (0.055, 0.22),
        1 => (0.045, 0.18),
        _ => (0.065, 0.26),
    };
    let base_color = match variant {
        0 => Color::srgb(0.24, 0.68, 0.15),
        1 => Color::srgb(0.38, 0.78, 0.20),
        _ => Color::srgb(0.18, 0.46, 0.12),
    };
    let tip_color = match variant {
        0 => Color::srgb(0.38, 0.84, 0.22),
        1 => Color::srgb(0.55, 0.88, 0.28),
        _ => Color::srgb(0.26, 0.58, 0.18),
    };

    let half_w = w * 0.5;
    let positions = vec![
        [-half_w, 0.0, 0.0],
        [half_w, 0.0, 0.0],
        [half_w * 0.35, h, 0.0],
        [-half_w * 0.35, h, 0.0],
    ];
    let normals = vec![[0.0, 0.0, 1.0]; 4];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let colors = vec![
        base_color.to_linear().to_f32_array(),
        base_color.to_linear().to_f32_array(),
        tip_color.to_linear().to_f32_array(),
        tip_color.to_linear().to_f32_array(),
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

/// Small clump of grass and soil (two crossed quads at 90 degrees with brown roots).
fn grass_clump_mesh() -> Mesh {
    let w = 0.075;
    let h = 0.15;
    let root_color = Color::srgb(0.35, 0.24, 0.14).to_linear().to_f32_array();
    let leaf_color = Color::srgb(0.25, 0.66, 0.16).to_linear().to_f32_array();
    let half_w = w * 0.5;

    let positions = vec![
        // First quad
        [-half_w, 0.0, 0.0],
        [half_w, 0.0, 0.0],
        [half_w * 0.4, h, 0.0],
        [-half_w * 0.4, h, 0.0],
        // Second crossed quad
        [0.0, 0.0, -half_w],
        [0.0, 0.0, half_w],
        [0.0, h, half_w * 0.4],
        [0.0, h, -half_w * 0.4],
    ];
    let normals = vec![
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
    ];
    let uvs = vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
    ];
    let colors = vec![
        root_color, root_color, leaf_color, leaf_color, root_color, root_color, leaf_color,
        leaf_color,
    ];
    let indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

/// Spawns grass particles while a bike travels over grass, and puffs of
/// dust over dirt. Emission frequency and velocity scale with forward
/// speed.
pub fn kick_up_grass(
    time: Res<Time>,
    track: Res<Track>,
    grass_assets: Res<GrassAssets>,
    dust_assets: Res<DustAssets>,
    mut commands: Commands,
    mut bikes: Query<(&BikeState, &mut GrassKickup)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let mut rng_seed = (time.elapsed_secs_wrapped() * 1000.0) as u32;
    let mut random = || {
        rng_seed = rng_seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (rng_seed >> 16) as f32 / 65535.0
    };

    for (state, mut kickup) in &mut bikes {
        if !state.grounded || state.speed < 2.0 || state.ride.is_some() {
            kickup.accumulator = 0.0;
            continue;
        }

        let nearest = track.nearest_at(state.position, state.elevation);
        let surface = track.surface_beside(&nearest);
        if let Surface::Dirt(dust) = surface {
            raise_dust(
                state,
                dust,
                &mut kickup,
                dt,
                &mut random,
                &dust_assets,
                &mut commands,
            );
            continue;
        }
        if surface != Surface::Grass {
            kickup.accumulator = 0.0;
            continue;
        }

        // Spawn rate proportional to forward speed:
        // At 10 m/s ~ 18 blades/sec; at 36 m/s ~ 66 blades/sec.
        let spawn_rate = state.speed * 1.85;
        kickup.accumulator += spawn_rate * dt;

        let fwd_2d = forward(state.heading);
        let fwd_3d = Vec3::new(fwd_2d.x, 0.0, fwd_2d.y);
        let right_3d = Vec3::new(-fwd_2d.y, 0.0, fwd_2d.x);

        while kickup.accumulator >= 1.0 {
            kickup.accumulator -= 1.0;

            // 85% ejected by the rear drive wheel; 15% by the front steering wheel
            let is_rear = random() < 0.85;
            let wheel_offset = if is_rear { -0.68 } else { 0.70 };
            let lateral_jitter = (random() - 0.5) * 0.22;
            let origin_2d = state.position
                + fwd_2d * wheel_offset
                + Vec2::new(-fwd_2d.y, fwd_2d.x) * lateral_jitter;
            let origin_y = state.elevation + 0.06 + random() * 0.05;
            let spawn_pos = to_world(origin_2d, origin_y);

            // Ejection velocity:
            // Rear tire kicks grass backwards (-fwd) and upwards (+Y).
            // Faster speeds produce higher velocities and larger arcs.
            let backward_speed = if is_rear {
                state.speed * (0.45 + 0.35 * random()) + 2.5
            } else {
                state.speed * 0.15 + 1.0
            };
            let upward_speed = 2.0 + state.speed * (0.08 + 0.06 * random()) + random() * 1.5;
            let side_speed = (random() - 0.5) * (2.0 + state.speed * 0.08);

            let velocity =
                -fwd_3d * backward_speed + Vec3::Y * upward_speed + right_3d * side_speed;
            let angular_velocity = Vec3::new(
                (random() - 0.5) * 22.0,
                (random() - 0.5) * 22.0,
                (random() - 0.5) * 22.0,
            );

            let mesh_idx =
                (random() * grass_assets.meshes.len() as f32) as usize % grass_assets.meshes.len();
            let lifetime = 0.35 + random() * 0.25;
            let scale = Vec3::splat(0.8 + random() * 0.5);

            commands.spawn((
                Mesh3d(grass_assets.meshes[mesh_idx].clone()),
                MeshMaterial3d(grass_assets.material.clone()),
                Transform::from_translation(spawn_pos)
                    .with_scale(scale)
                    .with_rotation(Quat::from_rotation_y(random() * TAU)),
                Visibility::default(),
                GrassParticle {
                    velocity,
                    angular_velocity,
                    lifetime: 0.0,
                    max_lifetime: lifetime,
                    initial_scale: scale,
                },
                DespawnOnExit(GameState::Race),
            ));
        }
    }
}

/// Raises puffs of `dust` behind a bike riding over dirt: more and wider
/// the faster it goes, mostly from the rear wheel.
fn raise_dust(
    state: &BikeState,
    dust: Dust,
    kickup: &mut GrassKickup,
    dt: f32,
    random: &mut impl FnMut() -> f32,
    assets: &DustAssets,
    commands: &mut Commands,
) {
    kickup.accumulator += state.speed * DUST_RATE * dt;
    let ahead = forward(state.heading);
    let ahead_3d = Vec3::new(ahead.x, 0.0, ahead.y);
    let right_3d = Vec3::new(-ahead.y, 0.0, ahead.x);
    while kickup.accumulator >= 1.0 {
        kickup.accumulator -= 1.0;
        let rear = random() < 0.8;
        let wheel = if rear { -0.68 } else { 0.7 };
        let origin = state.position + ahead * wheel + ahead.perp() * (random() - 0.5) * 0.4;
        let spawn = to_world(origin, state.elevation + 0.15 + random() * 0.1);
        // Dragged along at first by the air in the bike's wake, then left
        // hanging behind it, billowing up and out.
        let carried = state.speed * (DUST_CARRY + 0.15 * random()) * if rear { 1.0 } else { 0.7 };
        let velocity = ahead_3d * carried
            + Vec3::Y * (0.8 + 1.4 * random())
            + right_3d * (random() - 0.5) * (1.5 + state.speed * 0.08);
        let grown = (1.6 + state.speed * 0.05) * (0.8 + 0.5 * random());
        let start = 0.5 + 0.25 * random();
        commands.spawn((
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material(dust, DUST_LEVELS - 1)),
            Transform::from_translation(spawn)
                .with_rotation(Quat::from_rotation_y(random() * TAU))
                .with_scale(Vec3::splat(start)),
            Visibility::default(),
            NotShadowCaster,
            DustPuff {
                velocity,
                spin: (random() - 0.5) * 1.5,
                lifetime: 0.0,
                max_lifetime: 0.9 + 0.7 * random(),
                size: (start, grown),
                dust,
                level: DUST_LEVELS - 1,
            },
            DespawnOnExit(GameState::Race),
        ));
    }
}

/// How thick a puff is (0..1) through its life (`progress`, 0..1): it
/// thickens at once and thins out slowly.
fn puff_opacity(progress: f32) -> f32 {
    smoothstep(0.0, 0.08, progress) * (1.0 - progress).powf(1.6)
}

/// How wide a puff is through its life (`progress`, 0..1), from `start` to
/// `grown`: it swells fast, then slowly.
fn puff_size(progress: f32, (start, grown): (f32, f32)) -> f32 {
    start + (grown - start) * (1.0 - (1.0 - progress).powi(3))
}

/// Moves the puffs of dust: slowed down by the air, settling a little, and
/// swelling and thinning out until they are gone.
pub fn update_dust_puffs(
    time: Res<Time>,
    assets: Res<DustAssets>,
    mut commands: Commands,
    mut puffs: Query<(
        Entity,
        &mut DustPuff,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (entity, mut puff, mut transform, mut material) in &mut puffs {
        puff.lifetime += dt;
        if puff.lifetime >= puff.max_lifetime {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += puff.velocity * dt;
        transform.rotate_y(puff.spin * dt);
        let drag = (1.0 - 2.4 * dt).max(0.0);
        puff.velocity *= drag;
        puff.velocity.y -= 0.4 * dt;

        let progress = puff.lifetime / puff.max_lifetime;
        transform.scale = Vec3::splat(puff_size(progress, puff.size));
        let level = dust_level(puff_opacity(progress));
        if level != puff.level {
            puff.level = level;
            material.0 = assets.material(puff.dust, level);
        }
    }
}

/// Updates translation, rotation, gravity trajectory, and shrinking of airborne grass particles.
pub fn update_grass_particles(
    time: Res<Time>,
    mut commands: Commands,
    mut particles: Query<(Entity, &mut GrassParticle, &mut Transform)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (entity, mut particle, mut transform) in &mut particles {
        particle.lifetime += dt;
        if particle.lifetime >= particle.max_lifetime {
            commands.entity(entity).despawn();
            continue;
        }

        // Motion, gravity, and air resistance
        transform.translation += particle.velocity * dt;
        transform.rotation *= Quat::from_euler(
            EulerRot::XYZ,
            particle.angular_velocity.x * dt,
            particle.angular_velocity.y * dt,
            particle.angular_velocity.z * dt,
        );

        // Arcade gravity producing parabolic arc
        particle.velocity.y -= 13.5 * dt;
        // Mild aerodynamic drag
        particle.velocity.x *= (1.0 - 1.8 * dt).max(0.0);
        particle.velocity.z *= (1.0 - 1.8 * dt).max(0.0);

        // Scale: remains full size during flight and shrinks quickly near expiry
        let progress = particle.lifetime / particle.max_lifetime;
        let shrink = (1.0 - progress.powi(3)).clamp(0.0, 1.0);
        transform.scale = particle.initial_scale * shrink;
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn grass_blade_mesh_generates_valid_attributes() {
        let mesh = grass_blade_mesh(0);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_POSITION).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
    }

    #[test]
    fn grass_clump_mesh_generates_valid_attributes() {
        let mesh = grass_clump_mesh();
        assert!(mesh.attribute(Mesh::ATTRIBUTE_POSITION).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
    }

    #[test]
    fn dust_puffs_swell_and_fade_away() {
        assert!(puff_opacity(0.1) > puff_opacity(0.5));
        assert!(puff_opacity(0.5) > puff_opacity(0.95));
        assert_eq!(puff_opacity(1.0), 0.0);
        assert!(puff_opacity(0.0) < 0.05, "it does not pop in");
        let size = (0.4, 2.0);
        assert_eq!(puff_size(0.0, size), 0.4);
        assert!(puff_size(0.3, size) > 1.0);
        assert!((puff_size(1.0, size) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn dust_fades_through_its_levels() {
        assert_eq!(dust_level(1.0), 0);
        assert_eq!(dust_level(0.0), DUST_LEVELS - 1);
        for level in 0..DUST_LEVELS {
            assert_eq!(dust_level(dust_alpha(level)), level);
        }
    }

    #[test]
    fn a_puff_is_four_soft_discs_facing_up() {
        let mesh = puff_mesh();
        assert_eq!(mesh.count_vertices(), 16);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("the puff has normals");
        };
        assert!(normals.iter().all(|n| *n == [0.0, 1.0, 0.0]));
        let image = puff_image();
        let data = image.data.as_ref().expect("the puff is painted");
        let alpha = |x: u32, y: u32| data[((y * 64 + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0, "clear at the corners");
        assert!(alpha(32, 32) > 120, "thick in the middle");
    }

    #[test]
    fn grass_kickup_scales_with_speed() {
        let dt = 0.1;
        let slow_speed = 10.0;
        let fast_speed = 30.0;
        let slow_rate = slow_speed * 1.85 * dt;
        let fast_rate = fast_speed * 1.85 * dt;
        assert!(fast_rate > slow_rate * 2.5);
    }
}
