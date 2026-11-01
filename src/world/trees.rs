//! The trees modelled with Meshy, one for each species and a fir for the
//! forests (prepared by the game's importer: about four hundred
//! triangles each, as tall as the classic ones, their colours in one
//! texture). Options > Graphics chooses between them and the classic trees
//! built in code ([`super::props`]); every tree is planted with both looks
//! ([`Tree`]) and dressed in the one chosen ([`dress_trees`]), at once when
//! the option changes.
//!
//! Each look has two lighter meshes too ([`lighter`]), the same shape with
//! fewer triangles, painted in their vertices, that the trees are drawn with
//! farther from the cameras: the first from tens of metres, the second from
//! a couple of hundred (sooner with the screen split; see
//! [`super::detail::Detail::tree_levels`]).

use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::camera::visibility::NoAutoAabb;
use bevy::prelude::*;

use super::detail::Detail;
use super::lod;
use super::scenery::Species;
use crate::bike::model::imported::{read, texture};
use crate::settings::Settings;

/// How many meshes each look of a tree has: the full one and two lighter.
pub const LEVELS: usize = 3;
/// The mip level of the modelled trees' texture their lighter meshes are
/// painted from (of 2048 texels, 256: as it is seen from afar).
const PAINT_MIP: u32 = 3;

/// The lighter meshes of a tree's `full` one (a list of triangles painted
/// in its vertices): the first with a third of its triangles, at most 200;
/// the second with a tenth, at most 60.
pub fn lighter(full: &Mesh) -> [Mesh; 2] {
    let triangles = full.indices().map_or(0, |indices| indices.len() / 3);
    [
        lod::simplify(full, (triangles / 3).clamp(24, 200)),
        lod::simplify(full, (triangles / 10).clamp(12, 60)),
    ]
}

/// A modelled tree: one for each species, and the fir.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TreeModel {
    Of(Species),
    /// A fir, in the forests with the tall trees.
    Fir,
}

/// Every model, in the order of their indices (and of their cells in the
/// texture, four to a row).
const MODELS: [TreeModel; 15] = [
    TreeModel::Of(Species::UmbrellaPine),
    TreeModel::Of(Species::Cypress),
    TreeModel::Of(Species::Olive),
    TreeModel::Of(Species::Tall),
    TreeModel::Of(Species::Broadleaf),
    TreeModel::Of(Species::MaritimePine),
    TreeModel::Of(Species::Poplar),
    TreeModel::Of(Species::Almond),
    TreeModel::Fir,
    TreeModel::Of(Species::Momiji),
    TreeModel::Of(Species::Ginkgo),
    TreeModel::Of(Species::Sakura),
    TreeModel::Of(Species::BlackPine),
    TreeModel::Of(Species::Sugi),
    TreeModel::Of(Species::Bamboo),
];
/// How many cells of the texture to a row.
#[cfg(any(test, feature = "testing"))]
const COLUMNS: usize = 4;

impl TreeModel {
    /// The model of a tree of `kind` planted with `chance` (0 to 1): half
    /// the tall trees of the forests are firs.
    pub fn of(kind: Species, chance: f32) -> Self {
        if kind == Species::Tall && chance < 0.5 {
            TreeModel::Fir
        } else {
            TreeModel::Of(kind)
        }
    }

    /// Its name in `trees.bin`.
    fn name(self) -> &'static str {
        match self {
            TreeModel::Of(Species::UmbrellaPine) => "umbrella_pine",
            TreeModel::Of(Species::Cypress) => "cypress",
            TreeModel::Of(Species::Olive) => "olive",
            TreeModel::Of(Species::Tall) => "tall",
            TreeModel::Of(Species::Broadleaf) => "broadleaf",
            TreeModel::Of(Species::MaritimePine) => "maritime_pine",
            TreeModel::Of(Species::Poplar) => "poplar",
            TreeModel::Of(Species::Almond) => "almond",
            TreeModel::Fir => "fir",
            TreeModel::Of(Species::Momiji) => "momiji",
            TreeModel::Of(Species::Ginkgo) => "ginkgo",
            TreeModel::Of(Species::Sakura) => "sakura",
            TreeModel::Of(Species::BlackPine) => "black_pine",
            TreeModel::Of(Species::Sugi) => "sugi",
            TreeModel::Of(Species::Bamboo) => "bamboo",
        }
    }

    fn index(self) -> usize {
        MODELS
            .iter()
            .position(|&model| model == self)
            .expect("every model is listed")
    }
}

/// The modelled trees' meshes, full and lighter (by [`TreeModel::index`]),
/// and their bounds; the material of the full ones, in their texture, and
/// of the lighter ones, painted in their vertices.
#[derive(Resource)]
pub struct ModelledTrees {
    meshes: Vec<[Handle<Mesh>; LEVELS]>,
    bounds: Vec<Aabb>,
    material: Handle<StandardMaterial>,
    painted: Handle<StandardMaterial>,
}

impl FromWorld for ModelledTrees {
    fn from_world(world: &mut World) -> Self {
        let mut pack = read(crate::content::file("models/trees/trees.bin"));
        let atlas = texture(crate::content::file("models/trees/color.jpg"), true);
        let models: Vec<(Mesh, [Mesh; 2])> = MODELS
            .iter()
            .map(|model| {
                let index = pack
                    .meshes
                    .iter()
                    .position(|piece| piece.name == model.name())
                    .unwrap_or_else(|| panic!("no {} in trees.bin", model.name()));
                let full = pack.meshes.swap_remove(index).mesh;
                let painted = lod::paint_from_texture(&full, &atlas, PAINT_MIP);
                (full, lighter(&painted))
            })
            .collect();
        let bounds = models
            .iter()
            .map(|(full, _)| full.compute_aabb().expect("a tree has vertices"))
            .collect();
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let meshes = models
            .into_iter()
            .map(|(full, [first, second])| {
                [meshes.add(full), meshes.add(first), meshes.add(second)]
            })
            .collect();
        let atlas = world.resource_mut::<Assets<Image>>().add(atlas);
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(atlas),
            perceptual_roughness: 0.95,
            reflectance: 0.3,
            ..default()
        });
        let painted = materials.add(StandardMaterial {
            perceptual_roughness: 0.95,
            reflectance: 0.3,
            ..default()
        });
        Self {
            meshes,
            bounds,
            material,
            painted,
        }
    }
}

/// A tree planted, with both its looks: the classic one (its meshes, full
/// and lighter, its material and its bounds) and its model. Its bounds are
/// those of its look's full mesh, whichever it is drawn with.
#[derive(Component, Clone)]
#[require(Dressed, NoAutoAabb, Aabb)]
pub struct Tree {
    pub classic: [Handle<Mesh>; LEVELS],
    pub classic_material: Handle<StandardMaterial>,
    pub classic_bounds: Aabb,
    pub model: TreeModel,
}

/// How a tree is drawn now: in which look (modelled or classic; none yet)
/// and with which of its meshes (0, the full one, to [`LEVELS`] - 1).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dressed {
    modelled: Option<bool>,
    level: usize,
}

impl Dressed {
    /// Which of its meshes it is drawn with: 0, the full one, to
    /// [`LEVELS`] - 1.
    pub fn level(&self) -> usize {
        self.level
    }
}

/// Dresses the trees as Options > Graphics says: each one as it is
/// planted, and all of them when the option changes, with their bounds,
/// for the culling. And with the mesh their distance from the nearest
/// camera calls for: lighter far away.
#[allow(clippy::type_complexity)]
pub fn dress_trees(
    settings: Option<Res<Settings>>,
    detail: Option<Res<Detail>>,
    modelled: Option<Res<ModelledTrees>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut trees: Query<(
        &Tree,
        &mut Dressed,
        &GlobalTransform,
        &InheritedVisibility,
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Aabb,
    )>,
) {
    // Without settings (in tests of the world alone), the new ones, as by
    // default, and whole.
    // The classic ones where the content has no modelled ones (the
    // engine's own has none: they are a game's).
    let wanted = modelled.is_some()
        && settings
            .as_deref()
            .is_none_or(|settings| settings.modelled_trees);
    let lighter = settings
        .as_deref()
        .is_some_and(|settings| settings.lighter_far_trees);
    let bounds = detail
        .map_or(Detail::default(), |detail| *detail)
        .tree_levels();
    let eyes: Vec<Vec3> = cameras
        .iter()
        .filter(|(camera, _)| camera.is_active)
        .map(|(_, transform)| transform.translation())
        .collect();
    trees.par_iter_mut().for_each(
        |(tree, mut dressed, transform, visible, mut mesh, mut material, mut aabb)| {
            let restyled = dressed.modelled != Some(wanted);
            // Those out of sight (of the circuits kept for later) keep the
            // mesh they had until they are seen; and with the lighter
            // meshes off, those already whole stay so.
            if !restyled && (!visible.get() || (!lighter && dressed.level == 0)) {
                return;
            }
            let level = if !lighter {
                0
            } else if eyes.is_empty() {
                dressed.level
            } else {
                let distance = eyes
                    .iter()
                    .map(|eye| eye.distance(transform.translation()))
                    .fold(f32::MAX, f32::min);
                lod::level_at(distance, dressed.level, &bounds)
            };
            let (look, paint, look_bounds) =
                if let Some(modelled) = modelled.as_deref().filter(|_| wanted) {
                    let model = tree.model.index();
                    let paint = if level == 0 {
                        &modelled.material
                    } else {
                        &modelled.painted
                    };
                    (
                        &modelled.meshes[model][level],
                        paint,
                        modelled.bounds[model],
                    )
                } else {
                    (
                        &tree.classic[level],
                        &tree.classic_material,
                        tree.classic_bounds,
                    )
                };
            if mesh.0 != *look {
                mesh.0 = look.clone();
            }
            if material.0 != *paint {
                material.0 = paint.clone();
            }
            if restyled {
                *aabb = look_bounds;
            }
            let now = Dressed {
                modelled: Some(wanted),
                level,
            };
            if *dressed != now {
                *dressed = now;
            }
        },
    );
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    pub fn each_modelled_tree_has_lighter_meshes_painted_in_its_colours_on() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StandardMaterial>>();
        let trees = ModelledTrees::from_world(&mut world);
        let meshes = world.resource::<Assets<Mesh>>();
        let triangles = |mesh: &Mesh| mesh.indices().unwrap().len() / 3;
        for model in MODELS {
            let [full, first, second] = trees.meshes[model.index()]
                .each_ref()
                .map(|handle| meshes.get(handle).unwrap());
            // (The second no fewer than two for each clump of leaves.)
            assert!(
                triangles(first) <= triangles(full) / 3 + 1
                    && triangles(second) <= triangles(full) / 3
                    && triangles(second) < triangles(first)
                    && triangles(second) >= 8,
                "{model:?}: {} {} {}",
                triangles(full),
                triangles(first),
                triangles(second)
            );
            // As tall, and in colours of its texture: not black, nor white.
            for light in [first, second] {
                let top = light
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                    .and_then(|p| p.as_float3())
                    .unwrap()
                    .iter()
                    .map(|p| p[1])
                    .fold(f32::MIN, f32::max);
                assert!(
                    top > 0.9 * trees.bounds[model.index()].max().y,
                    "{model:?}: {top}"
                );
                let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) =
                    light.attribute(Mesh::ATTRIBUTE_COLOR)
                else {
                    panic!("{model:?} is painted in its vertices");
                };
                let average =
                    colors.iter().map(|&c| Vec4::from(c)).sum::<Vec4>() / colors.len() as f32;
                assert!(
                    average.max_element() > 0.03 && average.min_element() < 0.9,
                    "{model:?}: {average}"
                );
            }
        }
    }

    pub fn every_model_is_in_the_pack_standing_on_the_ground_on() {
        let pack = read(crate::content::file("models/trees/trees.bin"));
        for model in MODELS {
            let piece = pack
                .meshes
                .iter()
                .find(|piece| piece.name == model.name())
                .unwrap_or_else(|| panic!("{model:?}"));
            let positions = piece
                .mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|p| p.as_float3())
                .unwrap();
            let bottom = positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
            let top = positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
            assert!(bottom.abs() < 1e-3, "{model:?} stands on the ground");
            assert!((4.0..17.0).contains(&top), "{model:?}: {top} m");
            // Light: a few hundred triangles.
            let triangles = piece.mesh.indices().unwrap().len() / 3;
            assert!(triangles < 600, "{model:?}: {triangles}");
            // Its texture coordinates in its own cell of the atlas.
            let uvs = match piece.mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
                Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) => uvs,
                _ => panic!("{model:?} has texture coordinates"),
            };
            let rows = MODELS.len().div_ceil(COLUMNS) as f32;
            let cell = |uv: &[f32; 2]| ((uv[0] * COLUMNS as f32) as usize, (uv[1] * rows) as usize);
            let index = model.index();
            assert!(
                uvs.iter()
                    .all(|uv| cell(uv) == (index % COLUMNS, index / COLUMNS)),
                "{model:?} in its cell"
            );
        }
        // Firs only among the tall trees of the forests.
        assert_eq!(TreeModel::of(Species::Tall, 0.2), TreeModel::Fir);
        assert_eq!(
            TreeModel::of(Species::Tall, 0.8),
            TreeModel::Of(Species::Tall)
        );
        assert_eq!(
            TreeModel::of(Species::Olive, 0.2),
            TreeModel::Of(Species::Olive)
        );
    }
}
