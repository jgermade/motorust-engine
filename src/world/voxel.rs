//! The ground as a world of small voxels (a circuit's `look: Voxel`, see
//! [`Look::Voxel`]): drawn, a pixel at a time, as if it were made of cubes
//! `size` metres on a side (see `voxel.wgsl`). Each cube is of one colour,
//! its texture read once at its middle, and lit as the face of a cube it
//! shows: its top where the ground is level, and over a slope the treads
//! and the risers of a staircase of cubes, as many risers as the slope
//! asks, facing down it; a cliff all risers. No triangle is added: the
//! ground's shape is its own (or in blocks too, with `steps`, see
//! [`super::terrain`]), and against the sky its outline stays smooth, which
//! cubes so small hardly show.
//!
//! The ground's pieces are drawn with the standard material as they are
//! made (see [`super::terrain::spawn_terrain`]); once in the world, those of
//! a circuit drawn as voxels are given this material in its place, the
//! same material extended (see [`voxelize_the_ground`]); where the lawn
//! was drawn stochastically in a twin of the piece (see [`super::lawn`]),
//! the twin goes and the piece is seen again, as voxels.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use super::lawn::Lawn;
use super::scenery::Look;
use super::terrain::{GroundPiece, GroundTile};
use super::{PreviewRoot, WorldRoot};
use crate::circuits::Circuits;

pub struct VoxelPlugin;

impl Plugin for VoxelPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "voxel.wgsl");
        // Without the renderer (in the tests), the voxels' material is only
        // an asset.
        if app.is_plugin_added::<bevy::pbr::PbrPlugin>() {
            app.add_plugins(MaterialPlugin::<VoxelMaterial>::default());
        } else {
            app.init_asset::<VoxelMaterial>();
        }
        app.add_systems(PostUpdate, voxelize_the_ground);
    }
}

/// The ground's own material, drawn as voxels.
pub type VoxelMaterial = ExtendedMaterial<StandardMaterial, VoxelGround>;

/// What the voxels read (see `voxel.wgsl`).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct VoxelGround {
    #[uniform(100)]
    pub look: VoxelLook,
}

/// Must match `VoxelLook` in `voxel.wgsl`.
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct VoxelLook {
    /// How big each voxel is (m).
    pub size: f32,
    /// How many metres each repetition of the piece's texture covers.
    pub tile: f32,
    /// How many shades each of red, green and blue a voxel's colour is
    /// cut to, as a palette of pixel art (0: as it is).
    pub shades: f32,
}

impl MaterialExtension for VoxelGround {
    fn fragment_shader() -> ShaderRef {
        "embedded://motorust_engine/world/voxel.wgsl".into()
    }
}

/// On a piece of the ground once it has been looked at: drawn as voxels, or
/// left as it is.
#[derive(Component)]
pub struct Voxelized;

/// What a piece of the ground drawn with `tile` metre repetitions of its
/// texture reads as voxels of a circuit whose look is `look`, if it is drawn
/// as voxels.
pub fn voxel_look(look: Look, tile: f32) -> Option<VoxelLook> {
    look.voxel_size().map(|size| VoxelLook {
        size,
        tile,
        shades: look.voxel_shades().map_or(0.0, |shades| shades as f32),
    })
}

/// Gives the pieces of the ground of a circuit drawn as voxels, as they come
/// into the world, the voxels' material in place of their own, which they
/// are extended from. Each piece is looked at once.
#[allow(clippy::type_complexity)]
fn voxelize_the_ground(
    mut commands: Commands,
    circuits: Option<Res<Circuits>>,
    pieces: Query<
        (
            Entity,
            &MeshMaterial3d<StandardMaterial>,
            &GroundTile,
            &ChildOf,
            Option<&Lawn>,
        ),
        (With<GroundPiece>, Without<Voxelized>),
    >,
    roots: Query<(Option<&WorldRoot>, Option<&PreviewRoot>)>,
    standard: Res<Assets<StandardMaterial>>,
    mut voxels: ResMut<Assets<VoxelMaterial>>,
) {
    let Some(circuits) = circuits else {
        return;
    };
    for (entity, material, tile, parent, lawn) in &pieces {
        let index = roots
            .get(parent.parent())
            .ok()
            .and_then(|(world, preview)| world.map(|r| r.0).or(preview.map(|r| r.0)));
        let look = index
            .filter(|&index| index < circuits.len())
            .and_then(|index| voxel_look(circuits.at(index).scenery.look, tile.0));
        let mut piece = commands.entity(entity);
        piece.insert(Voxelized);
        let (Some(look), Some(base)) = (look, standard.get(&material.0)) else {
            continue;
        };
        let voxel = voxels.add(VoxelMaterial {
            base: base.clone(),
            extension: VoxelGround { look },
        });
        // Not drawn as the stochastic lawn any more: its twin goes.
        if let Some(lawn) = lawn {
            piece.remove::<Lawn>().insert(lawn.visibility);
            commands.entity(lawn.twin).despawn();
            piece = commands.entity(entity);
        }
        piece
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(voxel));
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn only_a_circuit_drawn_as_voxels_has_its_ground_read_as_voxels() {
        assert_eq!(voxel_look(Look::Smooth, 11.0), None);
        let look = Look::Voxel {
            size: 0.25,
            steps: None,
            shades: Some(12),
        };
        assert_eq!(
            voxel_look(look, 11.0),
            Some(VoxelLook {
                size: 0.25,
                tile: 11.0,
                shades: 12.0,
            })
        );
    }

    #[test]
    fn the_ground_of_a_circuit_drawn_as_voxels_is_given_their_material() {
        use crate::circuits::Circuits;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<StandardMaterial>()
            .init_asset::<Shader>()
            .add_plugins(VoxelPlugin);
        let circuits = Circuits::load();
        app.insert_resource(circuits);
        let base = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        // The demo circuit is smooth: its ground keeps its own material.
        let root = app.world_mut().spawn(WorldRoot(0)).id();
        let piece = app
            .world_mut()
            .spawn((
                GroundPiece,
                GroundTile(11.0),
                MeshMaterial3d(base.clone()),
                ChildOf(root),
            ))
            .id();
        app.update();
        assert!(app.world().get::<Voxelized>(piece).is_some());
        assert!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(piece)
                .is_some()
        );
        assert!(
            app.world()
                .get::<MeshMaterial3d<VoxelMaterial>>(piece)
                .is_none()
        );
    }
}
