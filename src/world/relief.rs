//! The relief of the scenery's textures: normal maps, and depth maps for
//! parallax on the surfaces seen close, worked out at start-up from each
//! texture's own colours ([`ReliefMaps::add`]) and given to every material
//! drawn with one of those textures as Options > Graphics says
//! ([`Relief`]): to those there already when it changes, and to each new
//! one as it is made, whichever part of the world makes it. The meshes
//! drawn with them are given the tangents a normal map needs ([`tangents`])
//! as they are made, whatever the relief, so that it can come on at any
//! time; without it, they are drawn as they always were.
//!
//! The bumps are where a texture is lighter than round it: lighter stones
//! and grains of gravel stand out, dark gaps, joints and cracks sink in.
//! Its colours' slow changes (shading and tone over patches wider than the
//! bumps) taken away, the rest is its height ([`Heights::of`]). As they are
//! read off the texture as it is drawn, retouched (see
//! [`super::textures`]), they always fit it.
//!
//! The road has it only if Options > Graphics says so too
//! (`Settings::road_relief`); and, unless it says not to
//! (`Settings::flat_at_speed`), the rest loses it while the players go fast
//! ([`ReliefShown`]), where it is not seen, and gets it back as they slow
//! down. The performance forced takes the parallax away first, if it falls
//! short (see [`super::detail`]).
//!
//! A normal map changes the pipeline its materials are drawn with, so a
//! change of relief makes some again, and what they draw vanishes for a
//! moment meanwhile; a depth map is only read or not (a flag of the
//! material), in the same pipeline. So at speed the normal maps are swapped
//! for a flat one, not taken away: nothing vanishes in the middle of a race.

use bevy::{
    asset::RenderAssetUsages,
    image::ImageAddressMode,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    pbr::ParallaxMappingMethod,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::detail::Detail;
use super::textures::{WorldTextures, generate_mipmaps, sampler};
use crate::bike::{BikeState, LocalPlayer, Player};
use crate::settings::{Relief, Settings};

pub struct ReliefPlugin;

impl Plugin for ReliefPlugin {
    fn build(&self, app: &mut App) {
        // In `Last`, after the materials made this frame are announced and
        // before they and the meshes go to the GPU, so they are drawn with
        // their relief from their first frame.
        app.init_resource::<SharedRelief>()
            .init_resource::<ReliefShown>()
            .add_systems(
                Last,
                (show_relief, relieve_materials, give_tangents)
                    .chain()
                    .after(bevy::asset::AssetEventSystems),
            );
    }
}

/// How many layers parallax splits the depth into, at most (seen at a
/// grazing angle; head on, one): the readings of the depth map for each
/// pixel of the surfaces with it. Bevy's own is 16.
pub const PARALLAX_LAYERS: f32 = 8.0;

/// How a texture's bumps are cut out of its colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bumps {
    /// Over how many pixels each way the colours' slow changes are measured
    /// (and taken away): about the size of the widest bumps.
    pub broad: u32,
    /// How high the highest bumps stand over the deepest hollows, in the
    /// texture's own pixels: how steep their sides are, and how deep the
    /// parallax goes.
    pub height: f32,
    /// Whether parallax shifts them too: on the surfaces seen close.
    pub parallax: bool,
}

/// A texture's relief.
#[derive(Clone, Debug)]
pub struct ReliefMap {
    pub normal: Handle<Image>,
    /// Its depth map, and how deep it goes (a share of the texture's width,
    /// as Bevy takes it), for parallax.
    pub depth: Option<(Handle<Image>, f32)>,
    /// A flat normal map (one pixel), the same for all, for the relief
    /// dropped at speed.
    pub flat: Handle<Image>,
}

/// The relief of each texture with one, by the texture.
#[derive(Clone, Debug, Default)]
pub struct ReliefMaps(Vec<(AssetId<Image>, ReliefMap)>);

/// The speeds (m/s) over which, all the players going faster, the relief
/// is dropped (80 km/h), and under which, one of them going slower, it is
/// given back (60 km/h): apart, so that it does not come and go at one
/// speed.
pub const FAST: f32 = 80.0 / 3.6;
pub const SLOW: f32 = 60.0 / 3.6;

/// What a material with relief shows of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// None: no normal map.
    Off,
    /// A flat normal map: none to be seen, in the pipeline of those with
    /// one.
    Flat,
    Normal,
    Parallax,
}

/// The relief the materials show: as Options > Graphics says, and as fast
/// as the players go.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReliefShown {
    /// The relief chosen.
    pub relief: Relief,
    /// Whether the road has it too.
    pub road: bool,
    /// Whether the rest has it flattened: dropped at speed, the players
    /// going fast.
    pub flat: bool,
}

impl ReliefShown {
    /// What a material shows: the road's (`road`) or another's.
    pub fn look(&self, road: bool) -> Look {
        match self.relief {
            Relief::Off => Look::Off,
            _ if road && !self.road => Look::Off,
            _ if self.flat && !road => Look::Flat,
            Relief::Normal => Look::Normal,
            Relief::Parallax => Look::Parallax,
        }
    }
}

/// Whether the players go fast, with the relief dropped (`flat`) or not as
/// they did: all of them over [`FAST`] (over [`SLOW`] to keep it dropped).
/// Without players (in the menus), not.
pub fn fast(speeds: impl IntoIterator<Item = f32>, flat: bool) -> bool {
    let limit = if flat { SLOW } else { FAST };
    let mut any = false;
    for speed in speeds {
        if speed.abs() <= limit {
            return false;
        }
        any = true;
    }
    any
}

impl ReliefMaps {
    /// The relief of each of `textures` (in `images`, with their pixels) as
    /// its bumps say, worked out each on a thread of its own (but on the
    /// web, where there is only one).
    pub fn new(images: &mut Assets<Image>, textures: &[(&Handle<Image>, Bumps)]) -> Self {
        let work = |(texture, bumps): &(&Handle<Image>, Bumps)| {
            let image = images.get(*texture).expect("the texture is there");
            maps(image, *bumps)
        };
        let made: Vec<(Image, Option<(Image, f32)>)> = if cfg!(target_arch = "wasm32") {
            textures.iter().map(work).collect()
        } else {
            std::thread::scope(|scope| {
                let threads: Vec<_> = textures
                    .iter()
                    .map(|texture| scope.spawn(move || work(texture)))
                    .collect();
                threads
                    .into_iter()
                    .map(|thread| thread.join().expect("the relief is worked out"))
                    .collect()
            })
        };
        let flat = images.add(flat_normal_map());
        Self(
            textures
                .iter()
                .zip(made)
                .map(|((texture, _), (normal, depth))| {
                    let normal = images.add(normal);
                    let depth = depth.map(|(depth, scale)| (images.add(depth), scale));
                    let flat = flat.clone();
                    (
                        texture.id(),
                        ReliefMap {
                            normal,
                            depth,
                            flat,
                        },
                    )
                })
                .collect(),
        )
    }

    /// The relief of `texture`, if it has one.
    pub fn of(&self, texture: &Handle<Image>) -> Option<&ReliefMap> {
        self.of_id(texture.id())
    }

    /// The relief of the texture with `id`, if it has one.
    fn of_id(&self, id: AssetId<Image>) -> Option<&ReliefMap> {
        self.0.iter().find(|(of, _)| *of == id).map(|(_, map)| map)
    }
}

/// Textures made as the circuits are put in the world that have the relief
/// of another (the lawn in flower, the lawn's), by the texture: its own and
/// the other's.
#[derive(Resource, Default)]
pub struct SharedRelief(Vec<(AssetId<Image>, AssetId<Image>)>);

impl SharedRelief {
    /// `texture` with the relief of `of`.
    pub fn share(&mut self, texture: &Handle<Image>, of: &Handle<Image>) {
        self.0.push((texture.id(), of.id()));
    }

    /// The relief `material` is drawn with, if any: its texture's, or the
    /// one its texture shares.
    fn of<'a>(&self, maps: &'a ReliefMaps, material: &StandardMaterial) -> Option<&'a ReliefMap> {
        let texture = material.base_color_texture.as_ref()?.id();
        maps.of_id(texture).or_else(|| {
            let (_, of) = self.0.iter().find(|(shared, _)| *shared == texture)?;
            maps.of_id(*of)
        })
    }
}

/// The normal map of `image` and, for parallax, its depth map and how deep
/// it goes, as `bumps` says.
fn maps(image: &Image, bumps: Bumps) -> (Image, Option<(Image, f32)>) {
    let heights = Heights::of(image, bumps.broad);
    let normal = normal_map(&heights, bumps.height);
    let depth = bumps
        .parallax
        .then(|| (depth_map(&heights), bumps.height / heights.width as f32));
    (normal, depth)
}

/// A texture's heights, row by row: 0 in its deepest hollows, 1 on its
/// highest bumps, ½ on the level.
#[derive(Clone, Debug, PartialEq)]
pub struct Heights {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f32>,
}

impl Heights {
    /// The heights of an 8-bit RGBA `image`, its first level: its
    /// lightness, a little blurred (a photo's grain is not relief), less
    /// its average over `broad` pixels each way, spread so most of it lies
    /// well within 0 to 1.
    pub fn of(image: &Image, broad: u32) -> Self {
        let (width, height) = (image.width() as usize, image.height() as usize);
        let data = image
            .data
            .as_deref()
            .expect("the texture has its pixels on the CPU");
        let light: Vec<f32> = data[..width * height * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0)
            .collect();
        let fine = blur(&light, width, height, 1);
        let coarse = blur(&light, width, height, broad as usize);
        let detail: Vec<f32> = fine.iter().zip(&coarse).map(|(f, c)| f - c).collect();
        let spread = (detail.iter().map(|d| d * d).sum::<f32>() / detail.len() as f32)
            .sqrt()
            .max(1e-4);
        let values = detail
            .iter()
            .map(|d| 0.5 + 0.5 * (d / (2.0 * spread)).tanh())
            .collect();
        Self {
            width,
            height,
            values,
        }
    }

    /// The height at pixel (`x`, `y`), the texture repeating both ways.
    fn at(&self, x: isize, y: isize) -> f32 {
        let x = x.rem_euclid(self.width as isize) as usize;
        let y = y.rem_euclid(self.height as isize) as usize;
        self.values[y * self.width + x]
    }

    /// The normal at pixel (`x`, `y`) for bumps `height` pixels high, in the
    /// texture's own frame (as the normal maps take it): x along it (u), y
    /// up it (against v, which runs down the image), z out of it.
    pub fn normal_at(&self, x: usize, y: usize, height: f32) -> Vec3 {
        let (x, y) = (x as isize, y as isize);
        let across = (self.at(x + 1, y) - self.at(x - 1, y)) / 2.0 * height;
        let down = (self.at(x, y + 1) - self.at(x, y - 1)) / 2.0 * height;
        Vec3::new(-across, down, 1.0).normalize()
    }
}

/// `values` (`width` × `height`, row by row) blurred over a box `2 radius +
/// 1` pixels each way (at most the whole texture), repeating at its edges
/// as the texture does.
fn blur(values: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let rows = box_pass(values, width, height, radius.min((width - 1) / 2), true);
    box_pass(&rows, width, height, radius.min((height - 1) / 2), false)
}

/// One way of [`blur`]: along the rows (`across`) or down the columns.
fn box_pass(values: &[f32], width: usize, height: usize, radius: usize, across: bool) -> Vec<f32> {
    let (length, lines) = if across {
        (width, height)
    } else {
        (height, width)
    };
    let index = |line: usize, i: usize| {
        if across {
            line * width + i
        } else {
            i * width + line
        }
    };
    let span = (2 * radius + 1) as f32;
    let mut blurred = vec![0.0; values.len()];
    // Each line with `radius` of it before and after, as it repeats.
    let mut line_of = vec![0.0; length + 2 * radius + 1];
    for line in 0..lines {
        for (k, value) in line_of.iter_mut().enumerate() {
            let i = (k + length * (radius / length + 1) - radius) % length;
            *value = values[index(line, i)];
        }
        let mut sum: f32 = line_of[..2 * radius + 1].iter().sum();
        for i in 0..length {
            blurred[index(line, i)] = sum / span;
            sum += line_of[i + 2 * radius + 1] - line_of[i];
        }
    }
    blurred
}

/// The normal map of `heights` for bumps `height` pixels high: two
/// channels (x and y, Bevy works out z), linear, with its mipmaps, which
/// flatten the bumps away into the distance.
fn normal_map(heights: &Heights, height: f32) -> Image {
    let byte = |v: f32| ((v * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
    let data = (0..heights.height)
        .flat_map(|y| {
            (0..heights.width).flat_map(move |x| {
                let normal = heights.normal_at(x, y, height);
                [byte(normal.x), byte(normal.y)]
            })
        })
        .collect();
    let mut image = data_image(heights, data, TextureFormat::Rg8Unorm);
    generate_mipmaps(&mut image);
    image
}

/// The depth map of `heights`, as Bevy's parallax takes it: 0 on the
/// highest bumps, 1 in the deepest hollows. Without mipmaps: Bevy reads
/// only its first level.
fn depth_map(heights: &Heights) -> Image {
    let data = heights
        .values
        .iter()
        .map(|h| ((1.0 - h).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    data_image(heights, data, TextureFormat::R8Unorm)
}

/// A normal map of one pixel, level.
fn flat_normal_map() -> Image {
    let level = Heights {
        width: 1,
        height: 1,
        values: vec![0.5],
    };
    data_image(&level, vec![128, 128], TextureFormat::Rg8Unorm)
}

/// An image of data (not colour) the size of `heights`, repeating both ways.
fn data_image(heights: &Heights, data: Vec<u8>, format: TextureFormat) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: heights.width as u32,
            height: heights.height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = sampler(ImageAddressMode::Repeat);
    image
}

/// `material` with the relief `look` says, `map` being its texture's:
/// whether that changed it.
pub fn relieve(material: &mut StandardMaterial, map: &ReliefMap, look: Look) -> bool {
    let normal = match look {
        Look::Off => None,
        Look::Flat => Some(map.flat.clone()),
        Look::Normal | Look::Parallax => Some(map.normal.clone()),
    };
    let depth = map.depth.clone().filter(|_| look == Look::Parallax);
    if material.normal_map_texture == normal
        && material.depth_map.as_ref() == depth.as_ref().map(|(map, _)| map)
    {
        return false;
    }
    material.normal_map_texture = normal;
    match depth {
        Some((map, scale)) => {
            material.depth_map = Some(map);
            material.parallax_depth_scale = scale;
            material.max_parallax_layer_count = PARALLAX_LAYERS;
            material.parallax_mapping_method = ParallaxMappingMethod::Occlusion;
        }
        None => material.depth_map = None,
    }
    true
}

/// The relief to show, as the settings say, the players go and the
/// performance forced lets it (its parallax, see [`Detail::parallax`]).
#[allow(clippy::type_complexity)]
pub(super) fn show_relief(
    settings: Option<Res<Settings>>,
    detail: Option<Res<Detail>>,
    players: Query<&BikeState, Or<(With<Player>, With<LocalPlayer>)>>,
    mut shown: ResMut<ReliefShown>,
) {
    let (relief, road, at_speed) = settings.map_or((Relief::default(), false, true), |settings| {
        (
            settings.relief,
            settings.road_relief,
            settings.flat_at_speed,
        )
    });
    let relief = capped(relief, detail.is_none_or(|detail| detail.parallax));
    let flat = at_speed && fast(players.iter().map(|bike| bike.speed), shown.flat);
    let now = ReliefShown { relief, road, flat };
    if *shown != now {
        *shown = now;
    }
}

/// The relief `chosen`, without its parallax unless it may be drawn
/// (`parallax`).
pub fn capped(chosen: Relief, parallax: bool) -> Relief {
    if chosen == Relief::Parallax && !parallax {
        Relief::Normal
    } else {
        chosen
    }
}

/// Whether `material` is the road's: drawn with the asphalt.
pub fn is_road(material: &StandardMaterial, textures: &WorldTextures) -> bool {
    material.base_color_texture.as_ref() == Some(&textures.asphalt)
}

/// The relief shown given to the materials drawn with a texture that has
/// one: all of them when it changes, otherwise each one as it is made.
pub(super) fn relieve_materials(
    shown: Res<ReliefShown>,
    textures: Res<WorldTextures>,
    shared: Res<SharedRelief>,
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut given: Local<Option<ReliefShown>>,
) {
    let ids: Vec<AssetId<StandardMaterial>> = if *given != Some(*shown) {
        *given = Some(*shown);
        events.clear();
        materials.ids().collect()
    } else {
        events
            .read()
            .filter_map(|event| match event {
                AssetEvent::Added { id } => Some(*id),
                _ => None,
            })
            .collect()
    };
    for id in ids {
        let Some(map) = materials
            .get(id)
            .and_then(|material| shared.of(&textures.relief, material))
        else {
            continue;
        };
        let map = map.clone();
        // Only those it changes are touched, so the others are not sent
        // to the GPU again.
        let mut material = materials.get(id).cloned().expect("it is there");
        let look = shown.look(is_road(&material, &textures));
        if relieve(&mut material, &map, look)
            && let Some(mut slot) = materials.get_mut(id)
        {
            *slot = material;
        }
    }
}

/// The meshes just put in the world drawn with a texture that has relief
/// given tangents, if they have none, whatever the relief chosen: it can
/// come on later, and by then most of them have gone to the GPU, their
/// vertices no longer kept here (a mesh only for the GPU,
/// `RenderAssetUsages::RENDER_WORLD`, is sent once). So they are given them
/// before they go, in the frame they are made, without telling it (which
/// would send them again, with nothing left to send). One that has gone
/// already is left without: drawn as it was.
#[allow(clippy::type_complexity)]
pub(super) fn give_tangents(
    textures: Res<WorldTextures>,
    shared: Res<SharedRelief>,
    new: Query<
        (&Mesh3d, &MeshMaterial3d<StandardMaterial>),
        Or<(Added<Mesh3d>, Changed<MeshMaterial3d<StandardMaterial>>)>,
    >,
    materials: Res<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (mesh, material) in &new {
        let relieved = materials
            .get(&material.0)
            .is_some_and(|material| shared.of(&textures.relief, material).is_some());
        if !relieved {
            continue;
        }
        let Some(drawn) = meshes.get(&mesh.0) else {
            continue;
        };
        // Gone to the GPU, or with tangents already.
        if !matches!(
            drawn.try_attribute_option(Mesh::ATTRIBUTE_TANGENT),
            Ok(None)
        ) {
            continue;
        }
        let Some(tangents) = tangents(drawn) else {
            continue;
        };
        // Kept here as well, it can be sent again; otherwise it has not
        // gone yet, and goes with them.
        let kept = drawn.asset_usage.contains(RenderAssetUsages::MAIN_WORLD);
        if kept {
            if let Some(mut drawn) = meshes.get_mut(&mesh.0) {
                drawn.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
            }
        } else if let Some(drawn) = meshes.get_mut_untracked(&mesh.0) {
            drawn.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
        }
    }
}

/// `mesh` given its [`tangents`], if it has none and can have them: as a
/// circuit's meshes are prepared, away from the main thread, those drawn
/// with a texture that has relief (so the main thread has none of them to
/// work out, see [`give_tangents`]).
pub fn add_tangents(mesh: &mut Mesh) {
    if mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_none()
        && let Some(tangents) = tangents(mesh)
    {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    }
}

/// The tangents of a mesh of triangles with normals and texture
/// coordinates, as Bevy's normal maps and parallax take them: along u
/// (`xyz`), and the side (`w`) up the texture (against v) lies on, of the
/// normal crossed with it. Each triangle's own, weighted by its area, are
/// summed at its corners. Quicker than Bevy's own (MikkTSpace), and as good
/// for normal maps worked out from the textures (not baked against one
/// mesh's tangents), which takes a while with the terrain's hundreds of
/// thousands of corners. `None` if the mesh is not one of those.
pub fn tangents(mesh: &Mesh) -> Option<Vec<[f32; 4]>> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        return None;
    };
    let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {
        return None;
    };
    let corners: Vec<usize> = match mesh.indices() {
        Some(Indices::U32(indices)) => indices.iter().map(|&i| i as usize).collect(),
        Some(Indices::U16(indices)) => indices.iter().map(|&i| i as usize).collect(),
        None => (0..positions.len()).collect(),
    };
    // Along u and along v, at each corner.
    let mut sums = vec![(Vec3::ZERO, Vec3::ZERO); positions.len()];
    for triangle in corners.as_chunks::<3>().0 {
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
        let (pa, pb, pc) = (
            Vec3::from(positions[a]),
            Vec3::from(positions[b]),
            Vec3::from(positions[c]),
        );
        let (ta, tb, tc) = (Vec2::from(uvs[a]), Vec2::from(uvs[b]), Vec2::from(uvs[c]));
        let (e1, e2) = (pb - pa, pc - pa);
        let (d1, d2) = (tb - ta, tc - ta);
        let det = d1.x * d2.y - d2.x * d1.y;
        if det.abs() < 1e-12 {
            continue;
        }
        let along_u = ((e1 * d2.y - e2 * d1.y) / det).normalize_or_zero();
        let along_v = ((e2 * d1.x - e1 * d2.x) / det).normalize_or_zero();
        let area = e1.cross(e2).length();
        for corner in [a, b, c] {
            sums[corner].0 += along_u * area;
            sums[corner].1 += along_v * area;
        }
    }
    Some(
        sums.iter()
            .zip(normals)
            .map(|(&(along_u, along_v), &normal)| {
                let normal = Vec3::from(normal);
                let mut tangent = (along_u - normal * normal.dot(along_u)).normalize_or_zero();
                if tangent == Vec3::ZERO {
                    tangent = normal.any_orthonormal_vector();
                }
                // Bevy's bitangent, w × (normal × tangent), runs up the
                // texture, against v.
                let w = if normal.cross(tangent).dot(along_v) > 0.0 {
                    -1.0
                } else {
                    1.0
                };
                [tangent.x, tangent.y, tangent.z, w]
            })
            .collect(),
    )
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    /// A texture of `width` × `height` grey pixels, each as `light` says.
    fn grey(width: u32, height: u32, light: impl Fn(u32, u32) -> u8) -> Image {
        let data = (0..width * height)
            .flat_map(|i| {
                let v = light(i % width, i / width);
                [v, v, v, 255]
            })
            .collect();
        Image::new(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        )
    }

    #[test]
    fn an_even_texture_is_level() {
        let heights = Heights::of(&grey(32, 16, |_, _| 128), 8);
        assert!(heights.values.iter().all(|h| (h - 0.5).abs() < 1e-4));
        for (x, y) in [(0, 0), (31, 15), (7, 9)] {
            assert!(heights.normal_at(x, y, 4.0).distance(Vec3::Z) < 1e-4);
        }
    }

    #[test]
    fn light_stands_out_and_dark_sinks_in() {
        // A light square in the middle of a darker texture.
        let image = grey(64, 64, |x, y| {
            if (24..40).contains(&x) && (24..40).contains(&y) {
                200
            } else {
                80
            }
        });
        let heights = Heights::of(&image, 16);
        assert!(heights.at(32, 32) > 0.75, "{}", heights.at(32, 32));
        // Round it, lower than its surroundings; further off, level.
        assert!(heights.at(20, 32) < 0.5, "{}", heights.at(20, 32));
        assert!(
            (heights.at(4, 4) - 0.5).abs() < 1e-4,
            "{}",
            heights.at(4, 4)
        );
        assert!(heights.values.iter().all(|h| (0.0..=1.0).contains(h)));
        // Its sides face out: left on its left, up (against v) on its top.
        let left = heights.normal_at(23, 32, 4.0);
        assert!(left.x < -0.3 && left.y.abs() < 1e-3, "{left}");
        let top = heights.normal_at(32, 23, 4.0);
        assert!(top.y > 0.3 && top.x.abs() < 1e-3, "{top}");
        // The higher the bumps, the steeper.
        assert!(heights.normal_at(23, 32, 8.0).x < left.x);
    }

    #[test]
    fn heights_repeat_as_the_texture_does() {
        // Bright along the left edge: its bump wraps round to the right
        // edge too, without a seam.
        let image = grey(32, 8, |x, _| if x < 2 { 220 } else { 90 });
        let heights = Heights::of(&image, 6);
        assert!((heights.at(31, 3) - heights.at(2, 3)).abs() < 1e-4);
        assert!((heights.at(-1, 3) - heights.at(31, 3)).abs() < 1e-6);
        assert!(
            heights.normal_at(31, 3, 4.0).x < 0.0,
            "the bump's left side"
        );
        assert!(
            heights.normal_at(2, 3, 4.0).x > 0.0,
            "the bump's right side"
        );
    }

    #[test]
    fn blurring_keeps_the_average_and_spreads_a_spike() {
        let mut values = vec![0.0; 25];
        values[12] = 25.0;
        let blurred = blur(&values, 5, 5, 1);
        assert!((blurred.iter().sum::<f32>() - 25.0).abs() < 1e-3);
        assert!((blurred[12] - 25.0 / 9.0).abs() < 1e-4);
        assert!(
            (blurred[6] - 25.0 / 9.0).abs() < 1e-4,
            "a corner of its box"
        );
        assert_eq!(blurred[0], 0.0);
    }

    #[test]
    fn the_maps_are_data_with_their_mipmaps_or_without() {
        let heights = Heights::of(&grey(16, 8, |x, y| ((x * 13 + y * 7) % 256) as u8), 4);
        let normal = normal_map(&heights, 2.0);
        assert_eq!(normal.texture_descriptor.format, TextureFormat::Rg8Unorm);
        assert_eq!(
            normal.texture_descriptor.mip_level_count, 5,
            "16 × 8 to 1 × 1"
        );
        let depth = depth_map(&heights);
        assert_eq!(depth.texture_descriptor.format, TextureFormat::R8Unorm);
        assert_eq!(depth.texture_descriptor.mip_level_count, 1);
        assert_eq!(depth.data.as_ref().unwrap().len(), 16 * 8);
        // Level: a normal map's x and y at ½, a depth map's at ½.
        let flat = Heights::of(&grey(4, 4, |_, _| 50), 2);
        assert_eq!(&normal_map(&flat, 2.0).data.unwrap()[..2], &[128, 128]);
        assert_eq!(depth_map(&flat).data.unwrap()[0], 128);
    }

    fn map(images: &mut Assets<Image>, parallax: bool) -> ReliefMap {
        let mut image = || images.add(Image::default());
        ReliefMap {
            normal: image(),
            depth: parallax.then(|| (image(), 0.01)),
            flat: image(),
        }
    }

    #[test]
    fn materials_take_the_relief_chosen() {
        let mut images = Assets::<Image>::default();
        let deep = map(&mut images, true);
        let mut material = StandardMaterial::default();
        assert!(!relieve(&mut material, &deep, Look::Off));
        assert!(relieve(&mut material, &deep, Look::Normal));
        assert_eq!(material.normal_map_texture, Some(deep.normal.clone()));
        assert!(material.depth_map.is_none());
        assert!(relieve(&mut material, &deep, Look::Parallax));
        assert!(material.depth_map.is_some());
        assert_eq!(material.parallax_depth_scale, 0.01);
        assert_eq!(material.max_parallax_layer_count, PARALLAX_LAYERS);
        assert!(!relieve(&mut material, &deep, Look::Parallax));
        // Flat: a normal map still (the same pipeline), level, no depth.
        assert!(relieve(&mut material, &deep, Look::Flat));
        assert_eq!(material.normal_map_texture, Some(deep.flat.clone()));
        assert!(material.depth_map.is_none());
        assert!(relieve(&mut material, &deep, Look::Off));
        assert!(material.normal_map_texture.is_none() && material.depth_map.is_none());
        // Without a depth map, parallax is normal maps alone.
        let shallow = map(&mut images, false);
        let mut flat = StandardMaterial::default();
        assert!(relieve(&mut flat, &shallow, Look::Parallax));
        assert!(flat.normal_map_texture.is_some() && flat.depth_map.is_none());
        assert!(!relieve(&mut flat, &shallow, Look::Normal));
    }

    #[test]
    fn the_road_has_relief_only_if_asked_and_keeps_it_at_speed() {
        let shown = |relief, road, flat| ReliefShown { relief, road, flat };
        // As the game starts: the road without, the rest with normal maps.
        let first = shown(Relief::Normal, false, false);
        assert_eq!(
            (first.look(true), first.look(false)),
            (Look::Off, Look::Normal)
        );
        let road = shown(Relief::Parallax, true, false);
        assert_eq!(
            (road.look(true), road.look(false)),
            (Look::Parallax, Look::Parallax)
        );
        // At speed the rest is flattened, the road keeps it if it has it.
        let fast = shown(Relief::Parallax, true, true);
        assert_eq!(
            (fast.look(true), fast.look(false)),
            (Look::Parallax, Look::Flat)
        );
        let fast = shown(Relief::Normal, false, true);
        assert_eq!((fast.look(true), fast.look(false)), (Look::Off, Look::Flat));
        // None at all, whatever else.
        let off = shown(Relief::Off, true, true);
        assert_eq!((off.look(true), off.look(false)), (Look::Off, Look::Off));
    }

    #[test]
    fn the_relief_goes_with_all_the_players_fast_and_comes_back_with_one_slow() {
        let kmh = |speeds: &[f32]| speeds.iter().map(|s| s / 3.6).collect::<Vec<_>>();
        assert!(!fast(kmh(&[]), false), "in the menus");
        assert!(!fast(kmh(&[70.0]), false));
        assert!(fast(kmh(&[90.0]), false));
        assert!(!fast(kmh(&[90.0, 40.0]), false), "one of two slow");
        assert!(fast(kmh(&[90.0, 120.0]), false));
        // Between the two speeds it stays as it was.
        assert!(fast(kmh(&[70.0]), true));
        assert!(!fast(kmh(&[70.0]), false));
        assert!(!fast(kmh(&[50.0]), true));
        // Backwards counts as fast as forwards.
        assert!(fast(kmh(&[-90.0]), false));
    }

    #[test]
    fn the_performance_forced_takes_only_the_parallax_away() {
        assert_eq!(capped(Relief::Parallax, false), Relief::Normal);
        assert_eq!(capped(Relief::Parallax, true), Relief::Parallax);
        for relief in [Relief::Off, Relief::Normal] {
            assert_eq!(capped(relief, false), relief);
        }
    }

    #[test]
    fn the_flat_normal_map_is_level() {
        let flat = flat_normal_map();
        assert_eq!(flat.texture_descriptor.format, TextureFormat::Rg8Unorm);
        assert_eq!(flat.data.as_deref(), Some(&[128, 128][..]));
    }

    /// Two triangles on the ground, u along x and v along z.
    fn ground() -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [0.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [2.0, 0.0, 2.0],
                [0.0, 0.0, 2.0],
            ],
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4])
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        )
        .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
    }

    #[test]
    fn tangents_run_along_u_with_the_bitangent_up_the_texture() {
        let tangents = tangents(&ground()).expect("a mesh of triangles");
        for [x, y, z, w] in tangents {
            let tangent = Vec3::new(x, y, z);
            assert!(tangent.distance(Vec3::X) < 1e-5, "{tangent}");
            // Up the texture is against v: towards -z.
            let bitangent = w * Vec3::Y.cross(tangent);
            assert!(bitangent.distance(Vec3::NEG_Z) < 1e-5, "{bitangent}");
        }
    }

    #[test]
    fn tangents_agree_with_bevys_own() {
        let meshes = [
            ground(),
            Sphere::new(1.0).mesh().uv(16, 12),
            Cuboid::new(1.0, 2.0, 3.0).mesh().build(),
            Cylinder::new(0.5, 2.0).mesh().build(),
        ];
        for mesh in meshes {
            let ours = tangents(&mesh).expect("a mesh of triangles");
            let bevys = mesh.clone().with_generated_tangents().unwrap();
            let Some(VertexAttributeValues::Float32x4(bevys)) =
                bevys.attribute(Mesh::ATTRIBUTE_TANGENT)
            else {
                panic!("Bevy's tangents");
            };
            for (ours, bevys) in ours.iter().zip(bevys) {
                let (a, b) = (Vec3::from_slice(ours), Vec3::from_slice(bevys));
                if b.length() < 0.5 {
                    // Where Bevy's has none (a pole of the sphere).
                    continue;
                }
                assert!(a.dot(b.normalize()) > 0.9, "{a} against {b}");
                assert_eq!(ours[3], bevys[3], "the same side");
            }
        }
    }

    #[test]
    fn only_meshes_of_triangles_with_texture_coordinates_have_tangents() {
        let mut lines = ground();
        lines.remove_attribute(Mesh::ATTRIBUTE_UV_0);
        assert!(tangents(&lines).is_none());
        let strip = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::RENDER_WORLD);
        assert!(tangents(&strip).is_none());
    }
}
