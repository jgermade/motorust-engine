//! The portals between the realms of a split circuit (see [`super::realm`]):
//! interdimensional stargates across the road where its realms join, each
//! a heavy ring of titanium and gold chevrons between twin pylons, two
//! counter-rotating rings of plasma ([`PortalSpin`]) and a shimmering
//! curtain across the road ([`PortalGate`], which the realms put each
//! rider's view of the other realm on). The engine's: a circuit lists its
//! portals in its scenery ([`Scenery::portals`](super::scenery::Scenery)).

use std::f32::consts::{PI, TAU};

use bevy::app::Propagate;
use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::Assets3d;
use super::props::Footprint;
use super::quads::MeshBuilder;
use super::realm::{CURTAIN_RADIUS, GATE_HEIGHT, OVERVIEW_LAYER, PortalGate};
use super::scenery::Portal;
use super::terrain::Terrain;
use crate::track::{Track, WALL_DISTANCE};

/// How far out from the centre line of the road a portal's frame stands
/// (m), less its pylons' 3 m.
const ARCH_SPAN: f32 = WALL_DISTANCE + 2.8;

/// Colours of the frames (sRGB).
const HULL_WHITE: Color = Color::srgb(0.88, 0.90, 0.94);
const HULL_SILVER: Color = Color::srgb(0.64, 0.68, 0.75);
const HULL_DARK: Color = Color::srgb(0.26, 0.29, 0.35);
const GOLD_FOIL: Color = Color::srgb(0.88, 0.66, 0.16);

/// An inner plasma ring of an interdimensional stargate portal, rotating
/// continuously around the portal's axis (`Z` in local space).
#[derive(Component)]
pub struct PortalSpin {
    rate: f32,
}

/// Spins the concentric plasma rings inside each interdimensional portal.
pub fn spin_portals(time: Res<Time>, mut rings: Query<(&mut Transform, &PortalSpin)>) {
    let t = time.elapsed_secs();
    for (mut transform, spin) in &mut rings {
        transform.rotation = Quat::from_rotation_z(t * spin.rate);
    }
}

/// Spawns interdimensional stargate portals spanning the road: each has a
/// heavy titanium/gold-chevron stargate ring flanked by twin energy pylons,
/// two counter-rotating glowing plasma rings (`PortalSpin`), and a translucent
/// shimmering vortex membrane across the road aperture.
pub fn spawn_portals(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    portals: &[Portal],
    assets: &mut Assets3d,
) -> Vec<Footprint> {
    if portals.is_empty() {
        return Vec::new();
    }
    let hull = &assets.materials.add(StandardMaterial {
        perceptual_roughness: 0.55,
        metallic: 0.25,
        ..default()
    });
    let glow = &assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.72, 0.92, 1.0),
        emissive: LinearRgba::rgb(2.8, 4.8, 6.8),
        ..default()
    });
    let (frame_hull, frame_glow) = portal_frame_meshes();
    let frame_hull_mesh = assets.meshes.add(frame_hull);
    let frame_glow_mesh = assets.meshes.add(frame_glow);
    let outer_ring_mesh = assets.meshes.add(portal_ring_mesh(11.4, 12.1, 0.55));
    let inner_ring_mesh = assets.meshes.add(portal_ring_mesh(10.3, 10.9, 0.42));
    let vortex_mesh = assets.meshes.add(portal_vortex_mesh(CURTAIN_RADIUS));

    let violet_glow = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.45, 1.0),
        emissive: LinearRgba::rgb(5.5, 1.4, 8.5),
        ..default()
    });
    let vortex_mat = assets.materials.add(StandardMaterial {
        base_color: Color::srgba(0.35, 0.85, 1.0, 0.34),
        emissive: LinearRgba::rgb(1.6, 3.8, 6.2),
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        double_sided: true,
        unlit: true,
        ..default()
    });

    for portal in portals {
        let index = track.index_at(portal.along);
        let (center, heading) = track.pose_at(index);
        let road_y = track.surface_point(index, 0.0).y;
        let left = track.offset_point(index, ARCH_SPAN + 3.0);
        let right = track.offset_point(index, -(ARCH_SPAN + 3.0));
        let base_y = road_y.max(terrain.height(left).min(terrain.height(right)));
        let transform = Transform::from_xyz(center.x, base_y, center.y)
            .with_rotation(Quat::from_rotation_y(heading));

        // Shared by both realms: its frame stands in each, and its curtain
        // shows the other (see [`super::realm`]).
        world
            .spawn((
                transform,
                Visibility::default(),
                PortalGate,
                Propagate(RenderLayers::layer(0)),
            ))
            .with_children(|gate| {
                gate.spawn((
                    Mesh3d(frame_hull_mesh.clone()),
                    MeshMaterial3d(hull.clone()),
                ));
                gate.spawn((
                    Mesh3d(frame_glow_mesh.clone()),
                    MeshMaterial3d(glow.clone()),
                ));
                // Translucent energy curtain, as seen from above in the
                // menus; in a race each rider sees the other realm through
                // their own curtain instead (see [`super::realm`]).
                gate.spawn((
                    Mesh3d(vortex_mesh.clone()),
                    MeshMaterial3d(vortex_mat.clone()),
                    Transform::from_xyz(0.0, GATE_HEIGHT, 0.0),
                    Propagate(RenderLayers::layer(OVERVIEW_LAYER)),
                    NotShadowCaster,
                ));
                // Counter-rotating plasma rings around the portal aperture.
                gate.spawn((
                    PortalSpin { rate: 0.85 },
                    Mesh3d(outer_ring_mesh.clone()),
                    MeshMaterial3d(glow.clone()),
                    Transform::from_xyz(0.0, 4.2, 0.0),
                ));
                gate.spawn((
                    PortalSpin { rate: -1.35 },
                    Mesh3d(inner_ring_mesh.clone()),
                    MeshMaterial3d(violet_glow.clone()),
                    Transform::from_xyz(0.0, 4.2, 0.0),
                ));
            });
    }
    footprints(track, portals)
}

/// The ground the portals' pylons take, either side of the road.
pub fn footprints(track: &Track, portals: &[Portal]) -> Vec<Footprint> {
    portals
        .iter()
        .flat_map(|portal| {
            let index = track.index_at(portal.along);
            [
                (track.offset_point(index, ARCH_SPAN + 3.0), 8.0),
                (track.offset_point(index, -(ARCH_SPAN + 3.0)), 8.0),
            ]
        })
        .collect()
}

fn portal_frame_meshes() -> (Mesh, Mesh) {
    let mut hull = MeshBuilder::default();
    let mut glow = MeshBuilder::default();
    let center_y = 4.2f32;
    let r_in = 12.2f32;
    let r_out = 14.4f32;
    let half_z = 1.45f32;
    let segments = 20usize;

    // 1. Heavy 20-segment stargate outer ring with 9 gold/cyan chevron locks.
    for i in 0..segments {
        let a0 = -0.32 * PI + (i as f32 / segments as f32) * 1.64 * PI;
        let a1 = -0.32 * PI + ((i + 1) as f32 / segments as f32) * 1.64 * PI;
        let p0 = Vec3::new(
            a0.cos() * (r_in + r_out) * 0.5,
            center_y + a0.sin() * (r_in + r_out) * 0.5,
            0.0,
        );
        let p1 = Vec3::new(
            a1.cos() * (r_in + r_out) * 0.5,
            center_y + a1.sin() * (r_in + r_out) * 0.5,
            0.0,
        );
        let color = if i % 2 == 0 { HULL_DARK } else { HULL_SILVER };
        hull.beam(p0, p1, half_z * 2.0, r_out - r_in, color);

        // Glowing chevron lock on every other segment around the upper arch.
        if i % 2 == 1 {
            let am = (a0 + a1) * 0.5;
            let chevron_pos = Vec3::new(
                am.cos() * (r_out + 0.35),
                center_y + am.sin() * (r_out + 0.35),
                0.0,
            );
            add_box(
                &mut hull,
                chevron_pos - Vec3::new(0.85, 0.85, half_z + 0.35),
                chevron_pos + Vec3::new(0.85, 0.85, half_z + 0.35),
                GOLD_FOIL,
            );
            add_box(
                &mut glow,
                chevron_pos - Vec3::new(0.48, 0.48, half_z + 0.48),
                chevron_pos + Vec3::new(0.48, 0.48, half_z + 0.48),
                Color::WHITE,
            );
        }
    }

    // 2. Twin Quantum Energy Pylons flanking the stargate ring.
    let pylon_x = WALL_DISTANCE + 4.2;
    for side in [-1.0, 1.0] {
        let x = side * pylon_x;
        // Deep foundation base and tall titanium tower.
        add_box(
            &mut hull,
            Vec3::new(x - 2.2, -6.0, -2.4),
            Vec3::new(x + 2.2, 2.0, 2.4),
            HULL_DARK,
        );
        add_box(
            &mut hull,
            Vec3::new(x - 1.4, 2.0, -1.6),
            Vec3::new(x + 1.4, 15.5, 1.6),
            HULL_WHITE,
        );
        // Gold thermal induction coils and glowing plasma rings up the pylon.
        for level in 0..4 {
            let y = 3.6 + level as f32 * 2.9;
            add_box(
                &mut hull,
                Vec3::new(x - 1.65, y - 0.35, -1.85),
                Vec3::new(x + 1.65, y + 0.35, 1.85),
                GOLD_FOIL,
            );
            add_box(
                &mut glow,
                Vec3::new(x - 1.72, y - 0.14, -1.92),
                Vec3::new(x + 1.72, y + 0.14, 1.92),
                Color::WHITE,
            );
        }
        // glowing energy crown at top of each pylon.
        add_box(
            &mut glow,
            Vec3::new(x - 0.9, 15.5, -0.9),
            Vec3::new(x + 0.9, 16.8, 0.9),
            Color::WHITE,
        );
    }

    (hull.build(), glow.build())
}

/// A segmented plasma ring centered at the origin in the XY plane, with
/// alternating bright segments that visibly spin around the Z axis.
fn portal_ring_mesh(r_in: f32, r_out: f32, half_z: f32) -> Mesh {
    let mut mesh = MeshBuilder::default();
    let segments = 24usize;
    let r_mid = (r_in + r_out) * 0.5;
    let thickness = r_out - r_in;
    for i in 0..segments {
        // Leave small gaps every 3rd segment so the rotation is unmistakable.
        if i % 3 == 0 {
            continue;
        }
        let a0 = (i as f32 / segments as f32) * TAU;
        let a1 = ((i + 1) as f32 / segments as f32) * TAU;
        let p0 = Vec3::new(a0.cos() * r_mid, a0.sin() * r_mid, 0.0);
        let p1 = Vec3::new(a1.cos() * r_mid, a1.sin() * r_mid, 0.0);
        mesh.beam(p0, p1, half_z * 2.0, thickness, Color::WHITE);
    }
    mesh.build()
}

/// Translucent interdimensional vortex curtain filling the portal aperture:
/// brighter around the outer rim and clearer near the centre so the landscape
/// of the destination world (Earth or Moon) shines through.
fn portal_vortex_mesh(radius: f32) -> Mesh {
    let mut mesh = MeshBuilder::default();
    let slices = 24usize;
    let r_inner = radius * 0.58;
    let inner_color = Color::srgba(0.65, 0.95, 1.0, 0.18);
    let outer_color = Color::srgba(0.20, 0.78, 1.0, 0.65);
    let center = Vec3::ZERO;
    for i in 0..slices {
        let a0 = (i as f32 / slices as f32) * TAU;
        let a1 = ((i + 1) as f32 / slices as f32) * TAU;
        let i0 = Vec3::new(a0.cos() * r_inner, a0.sin() * r_inner, 0.0);
        let i1 = Vec3::new(a1.cos() * r_inner, a1.sin() * r_inner, 0.0);
        let o0 = Vec3::new(a0.cos() * radius, a0.sin() * radius, 0.0);
        let o1 = Vec3::new(a1.cos() * radius, a1.sin() * radius, 0.0);
        // Inner translucent core.
        mesh.triangle(
            [(center, Vec2::ZERO), (i0, Vec2::X), (i1, Vec2::Y)],
            Vec3::Z,
            inner_color,
        );
        // Outer luminous vortex ring.
        mesh.quad(
            [
                (i0, Vec2::ZERO),
                (o0, Vec2::X),
                (o1, Vec2::ONE),
                (i1, Vec2::Y),
            ],
            Vec3::Z,
            outer_color,
        );
    }
    mesh.build()
}

/// Adds a closed axis-aligned box from `low` to `high` to `mesh`.
fn add_box(mesh: &mut MeshBuilder, low: Vec3, high: Vec3, color: Color) {
    let center = (low + high) / 2.0;
    let width = high.x - low.x;
    let height = high.y - low.y;
    mesh.beam(
        Vec3::new(center.x, center.y, low.z),
        Vec3::new(center.x, center.y, high.z),
        width,
        height,
        color,
    );
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn portal_meshes_have_geometry() {
        let (portal_hull, portal_glow) = portal_frame_meshes();
        assert!(portal_hull.count_vertices() > 80);
        assert!(portal_glow.count_vertices() > 20);
        assert!(portal_ring_mesh(11.4, 12.1, 0.55).count_vertices() > 40);
        assert!(portal_vortex_mesh(10.3).count_vertices() > 40);
    }
}
