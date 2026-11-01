//! The lawn's repetition broken up, as Options > Graphics > Grass tiling
//! says ([`GrassTiling`]). The lawn's texture covers 11 m (see
//! [`super::terrain::GRASS_TILE`]), and its clumps and patches, coming back
//! every 11 m over a whole hillside, are what gives the ground away. Two
//! ways of hiding it, both cutting the ground into small triangles, each
//! drawn from a piece of the lawn of its own, cross-fading into its
//! neighbours over their edges:
//!
//! - [`GrassTiling::Wide`] (the default): a lawn four times as wide each way
//!   ([`widen`]), put together like that once at start-up from pieces of the
//!   lawn, with its own relief: a repetition every 44 m instead of 11, all
//!   different within it, and drawn as the lawn is, at the cost of the
//!   memory of a bigger texture.
//! - [`GrassTiling::Stochastic`]: the pieces picked as each pixel is drawn
//!   (`lawn.wgsl`, after Heitz and Neyret's "High-Performance By-Example
//!   Noise using a Histogram-Preserving Blending Operator", 2018), never
//!   repeating, at the cost of three readings of the texture (and of its
//!   normal map) a pixel; without parallax.
//!
//! Each piece is blended round the lawn's average colour so the blend keeps
//! its contrast (the three pieces' differences from it, divided by the
//! square root of the sum of their weights squared), with the weights
//! sharpened ([`SHARP`]) so most of the ground is one piece or another, not
//! a haze of three.
//!
//! Every material drawn with the lawn has it (the ground, the banks, the
//! hedges): the wide lawn in place of the lawn in the material itself
//! ([`tile`]), the stochastic one in a material of its own
//! ([`LawnMaterial`]) on a twin of each thing drawn with it, drawn in its
//! place while it is hidden, gone when the tiling changes ([`Lawn`]).

use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    image::ImageAddressMode,
    light::{NotShadowCaster, NotShadowReceiver},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat},
    shader::ShaderRef,
};

use super::relief::{self, Look, ReliefMap, ReliefShown};
use super::textures::{WorldTextures, generate_mipmaps, sampler};
use crate::settings::{GrassTiling, Settings};

/// How many times the lawn's width the wide lawn is, each way.
pub const WIDE: u32 = 4;
/// The side of the squares the wide lawn is cut into (px; each split into
/// two triangles): a quarter of the lawn, 2.75 m.
const PATCH: u32 = 128;
/// How many triangles of the stochastic lawn's grid there are across the
/// lawn: its sides about a third of it, 3.7 m (as Heitz and Neyret's).
const STOCHASTIC_CELLS: f32 = 3.464;
/// How sharply the pieces meet: the power their weights are raised to.
const SHARP: f32 = 4.0;

pub struct LawnPlugin;

impl Plugin for LawnPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "lawn.wgsl");
        // Without the renderer (in the tests), the stochastic lawn is only
        // an asset.
        if app.is_plugin_added::<bevy::pbr::PbrPlugin>() {
            app.add_plugins(MaterialPlugin::<LawnMaterial>::default());
        } else {
            app.init_asset::<LawnMaterial>();
        }
        // The materials, before they are given their relief (of the lawn
        // they have); the things drawn with them, once their meshes have
        // their tangents.
        app.add_systems(
            Last,
            (
                tile_materials
                    .after(relief::show_relief)
                    .before(relief::relieve_materials),
                draw_stochastic.after(relief::give_tangents),
            )
                .after(bevy::asset::AssetEventSystems),
        );
    }
}

/// The lawn stochastic: the lawn's own material (but its texture and its
/// relief), its colour and its normal map read from the lawn three times
/// over.
pub type LawnMaterial = ExtendedMaterial<StandardMaterial, StochasticLawn>;

/// What the stochastic lawn reads (see `lawn.wgsl`).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct StochasticLawn {
    #[uniform(100)]
    pub look: LawnLook,
    #[texture(101)]
    #[sampler(102)]
    pub texture: Handle<Image>,
    /// The lawn's normal map, if the relief has one.
    #[texture(103)]
    #[sampler(104)]
    pub normals: Option<Handle<Image>>,
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct LawnLook {
    /// The lawn's average colour (linear), round which its pieces blend.
    pub mean: Vec4,
    /// 1 with the lawn's normal map, 0 without.
    pub normals: f32,
    /// How many triangles of the grid across the lawn.
    pub cells: f32,
    /// How sharply the pieces meet.
    pub sharp: f32,
}

impl MaterialExtension for StochasticLawn {
    fn fragment_shader() -> ShaderRef {
        "embedded://motorust_engine/world/lawn.wgsl".into()
    }
}

/// On a thing drawn with the lawn, hidden while its twin is drawn with the
/// stochastic lawn in its place (see [`draw_stochastic`]): the twin, and
/// how it was seen, to be seen so again when the tiling changes.
#[derive(Component, Clone, Debug)]
pub struct Lawn {
    pub twin: Entity,
    pub visibility: Visibility,
}

/// The twin of a thing drawn with the lawn, drawn with the stochastic lawn.
#[derive(Component, Clone, Copy, Debug)]
pub struct LawnTwin;

/// The lawn `lawn` (8-bit sRGB, square, its first level read) [`WIDE`]
/// times as wide each way, put together from pieces of it (see the
/// module's), with its mipmaps; repeating both ways as it does.
pub fn widen(lawn: &Image) -> Image {
    let size = lawn.width();
    assert_eq!(size, lawn.height(), "the lawn is square");
    let data = lawn
        .data
        .as_deref()
        .expect("the lawn has its pixels on the CPU");
    let level = &data[..(size * size * 4) as usize];
    let wide = size * WIDE;
    let mean = mean_of(level);
    let mut pixels = vec![0u8; (wide * wide * 4) as usize];
    let corners = Corners::new(size, wide / PATCH);
    let fill = |rows: &mut [u8], from: u32| {
        for (k, row) in rows.chunks_exact_mut((wide * 4) as usize).enumerate() {
            let y = from + k as u32;
            for x in 0..wide {
                let i = (x * 4) as usize;
                row[i..i + 3].copy_from_slice(&wide_pixel(level, size, &corners, mean, x, y));
                row[i + 3] = 255;
            }
        }
    };
    // In bands of rows, each on a thread of its own (but on the web).
    if cfg!(target_arch = "wasm32") {
        fill(&mut pixels, 0);
    } else {
        let band = wide / 4;
        std::thread::scope(|scope| {
            for (n, rows) in pixels.chunks_mut((band * wide * 4) as usize).enumerate() {
                let fill = &fill;
                scope.spawn(move || fill(rows, n as u32 * band));
            }
        });
    }
    let mut image = Image::new(
        Extent3d {
            width: wide,
            height: wide,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = sampler(ImageAddressMode::Repeat);
    generate_mipmaps(&mut image);
    image
}

/// The average of each colour channel of `pixels` (RGBA, 8 bits).
fn mean_of(pixels: &[u8]) -> [f32; 3] {
    let count = (pixels.len() / 4) as f32;
    let mut sum = [0.0f64; 3];
    for pixel in pixels.as_chunks::<4>().0 {
        for (sum, value) in sum.iter_mut().zip(pixel) {
            *sum += *value as f64;
        }
    }
    sum.map(|sum| (sum / count as f64) as f32)
}

/// Where each corner of the wide lawn's squares reads the lawn from (a
/// shift, px), and which way its square is split: one of each, `cells` of
/// them each way, the last ones meeting the first as the wide lawn repeats.
struct Corners {
    cells: u32,
    shifts: Vec<(u32, u32)>,
    flipped: Vec<bool>,
}

impl Corners {
    fn new(size: u32, cells: u32) -> Self {
        let (mut shifts, mut flipped) = (Vec::new(), Vec::new());
        for j in 0..cells {
            for i in 0..cells {
                let [a, b] = pcg2d(i, j);
                shifts.push((a % size, b % size));
                flipped.push(pcg2d(i + cells, j)[0] & 1 == 1);
            }
        }
        Self {
            cells,
            shifts,
            flipped,
        }
    }

    fn index(&self, i: u32, j: u32) -> usize {
        ((j % self.cells) * self.cells + i % self.cells) as usize
    }
}

/// The colour (sRGB, 8 bits) of the wide lawn at (`x`, `y`): the pieces of
/// the lawn (`level`, `size` px each way) read by the corners of the
/// triangle it is in, blended round the lawn's `mean`.
fn wide_pixel(
    level: &[u8],
    size: u32,
    corners: &Corners,
    mean: [f32; 3],
    x: u32,
    y: u32,
) -> [u8; 3] {
    let (i, j) = (x / PATCH, y / PATCH);
    let fx = ((x % PATCH) as f32 + 0.5) / PATCH as f32;
    let fy = ((y % PATCH) as f32 + 0.5) / PATCH as f32;
    let flipped = corners.flipped[corners.index(i, j)];
    let weights = triangle(fx, fy, flipped).map(|((di, dj), weight)| {
        let (sx, sy) = corners.shifts[corners.index(i + di, j + dj)];
        let at = (((y + sy) % size) * size + (x + sx) % size) as usize * 4;
        (&level[at..at + 3], weight)
    });
    let sharpened = sharpen(weights.map(|(_, weight)| weight));
    let spread = sharpened.iter().map(|w| w * w).sum::<f32>().sqrt();
    let mut color = [0u8; 3];
    for (channel, value) in color.iter_mut().enumerate() {
        let offset: f32 = weights
            .iter()
            .zip(sharpened)
            .map(|((piece, _), w)| w * (piece[channel] as f32 - mean[channel]))
            .sum();
        *value = (mean[channel] + offset / spread).round().clamp(0.0, 255.0) as u8;
    }
    color
}

/// The corners (each a step right and down from the square's first, and
/// its weight) of the triangle at (`fx`, `fy`) of its square, split from
/// its first corner to its last or (`flipped`) from its second to its
/// third.
fn triangle(fx: f32, fy: f32, flipped: bool) -> [((u32, u32), f32); 3] {
    if !flipped {
        if fx > fy {
            [((0, 0), 1.0 - fx), ((1, 0), fx - fy), ((1, 1), fy)]
        } else {
            [((0, 0), 1.0 - fy), ((0, 1), fy - fx), ((1, 1), fx)]
        }
    } else {
        // The same, mirrored left to right.
        let gx = 1.0 - fx;
        if gx > fy {
            [((1, 0), 1.0 - gx), ((0, 0), gx - fy), ((0, 1), fy)]
        } else {
            [((1, 0), 1.0 - fy), ((1, 1), fy - gx), ((0, 1), gx)]
        }
    }
}

/// `weights` raised to [`SHARP`], adding up to 1 again.
fn sharpen(weights: [f32; 3]) -> [f32; 3] {
    let raised = weights.map(|w| w.max(0.0).powf(SHARP));
    let sum = raised.iter().sum::<f32>().max(1e-12);
    raised.map(|w| w / sum)
}

/// Two random numbers from two (Jarzynski and Olano's PCG 2D hash, which
/// `lawn.wgsl` has too).
fn pcg2d(x: u32, y: u32) -> [u32; 2] {
    const M: u32 = 1_664_525;
    const C: u32 = 1_013_904_223;
    let (mut x, mut y) = (
        x.wrapping_mul(M).wrapping_add(C),
        y.wrapping_mul(M).wrapping_add(C),
    );
    x = x.wrapping_add(y.wrapping_mul(M));
    y = y.wrapping_add(x.wrapping_mul(M));
    x ^= x >> 16;
    y ^= y >> 16;
    x = x.wrapping_add(y.wrapping_mul(M));
    y = y.wrapping_add(x.wrapping_mul(M));
    x ^= x >> 16;
    y ^= y >> 16;
    [x, y]
}

/// `material` as `tiling` says, if it is drawn with the lawn (`lawn`) or
/// the wide lawn (`wide`): with the wide one, its texture four times as
/// wide, stretched over four times as much; otherwise with the lawn (the
/// stochastic lawn's own material is another, made from this one).
/// Whether that changed it.
pub fn tile(
    material: &mut StandardMaterial,
    lawn: &Handle<Image>,
    wide: &Handle<Image>,
    tiling: GrassTiling,
) -> bool {
    let Some(texture) = material.base_color_texture.as_ref() else {
        return false;
    };
    let widened = texture == wide;
    if !widened && texture != lawn {
        return false;
    }
    let widen = tiling == GrassTiling::Wide;
    if widen == widened {
        return false;
    }
    let (texture, stretch) = if widen {
        (wide, 1.0 / WIDE as f32)
    } else {
        (lawn, WIDE as f32)
    };
    material.base_color_texture = Some(texture.clone());
    material.uv_transform =
        bevy::math::Affine2::from_scale(Vec2::splat(stretch)) * material.uv_transform;
    true
}

/// The lawn's materials as the tiling says: all of them when it changes,
/// otherwise each one as it is made; with the relief of the lawn they have
/// (which the relief's own system gives only to those made this frame).
fn tile_materials(
    settings: Option<Res<Settings>>,
    shown: Res<ReliefShown>,
    textures: Res<WorldTextures>,
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut given: Local<Option<GrassTiling>>,
) {
    let tiling = settings.map_or_else(GrassTiling::default, |settings| settings.grass_tiling);
    let ids: Vec<AssetId<StandardMaterial>> = if *given != Some(tiling) {
        *given = Some(tiling);
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
        let Some(mut material) = materials.get(id).cloned() else {
            continue;
        };
        if !tile(&mut material, &textures.grass, &textures.grass_wide, tiling) {
            continue;
        }
        let texture = material
            .base_color_texture
            .clone()
            .expect("it has the lawn");
        if let Some(map) = textures.relief.of(&texture) {
            relief::relieve(&mut material, map, shown.look(false));
        }
        // Only those it changes are touched, so the others are not sent to
        // the GPU again.
        if let Some(mut slot) = materials.get_mut(id) {
            *slot = material;
        }
    }
}

/// The stochastic lawn's material for things drawn with `material` (the
/// lawn's), with the lawn's `map` if it shows its relief (`relieved`).
fn stochastic(
    material: &StandardMaterial,
    textures: &WorldTextures,
    map: Option<&ReliefMap>,
    relieved: bool,
) -> LawnMaterial {
    // The lawn read by its shader, not by the material's own.
    let base = StandardMaterial {
        base_color_texture: None,
        normal_map_texture: None,
        depth_map: None,
        ..material.clone()
    };
    ExtendedMaterial {
        base,
        extension: StochasticLawn {
            look: look(textures, map.is_some() && relieved),
            texture: textures.grass.clone(),
            normals: map.map(|map| map.normal.clone()),
        },
    }
}

/// What the stochastic lawn's shader reads, with the normal map or without.
fn look(textures: &WorldTextures, normals: bool) -> LawnLook {
    let mean = textures.grass_color.to_linear();
    LawnLook {
        mean: Vec4::new(mean.red, mean.green, mean.blue, 1.0),
        normals: if normals { 1.0 } else { 0.0 },
        cells: STOCHASTIC_CELLS,
        sharp: SHARP,
    }
}

/// What a thing drawn with the lawn is drawn as, to be drawn again the
/// same with the stochastic lawn.
type Drawn<'a> = (
    Entity,
    &'a MeshMaterial3d<StandardMaterial>,
    &'a Mesh3d,
    &'a Transform,
    Option<&'a Visibility>,
    Option<&'a ChildOf>,
    Option<&'a RenderLayers>,
    Has<NotShadowCaster>,
    Has<NotShadowReceiver>,
);

/// The things drawn with the lawn drawn with the stochastic one when the
/// tiling says so (those put in the world this frame, or all of them when
/// it changes to it), and as they were when it changes from it; the
/// stochastic lawn's materials with the relief shown (its normal map, or
/// none: dropped at speed, or none chosen).
///
/// Each is not given the other material itself: Bevy (0.19) draws a thing
/// whose material changes kind for a frame with the new material's
/// bindings in the old one's pipeline, and the GPU stops the game. A twin
/// is drawn in its place instead ([`LawnTwin`]: the same mesh where it is,
/// beside it under the same parent), and it is hidden; when the tiling
/// changes back, the twin goes and it shows again ([`Lawn`]).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw_stochastic(
    mut commands: Commands,
    settings: Option<Res<Settings>>,
    shown: Res<ReliefShown>,
    textures: Res<WorldTextures>,
    standard: Res<Assets<StandardMaterial>>,
    mut lawns: ResMut<Assets<LawnMaterial>>,
    new: Query<
        Drawn,
        (
            Changed<MeshMaterial3d<StandardMaterial>>,
            Without<Lawn>,
            Without<LawnTwin>,
        ),
    >,
    all: Query<Drawn, (Without<Lawn>, Without<LawnTwin>)>,
    twinned: Query<(Entity, &Lawn)>,
    mut given: Local<Option<(GrassTiling, ReliefShown)>>,
    mut made: Local<HashMap<AssetId<StandardMaterial>, Handle<LawnMaterial>>>,
) {
    let tiling = settings.map_or_else(GrassTiling::default, |settings| settings.grass_tiling);
    let changed = *given != Some((tiling, *shown));
    let was = given.map(|(tiling, _)| tiling);
    *given = Some((tiling, *shown));
    // Parallax is not drawn on the stochastic lawn: its normal map only.
    let relieved = matches!(shown.look(false), Look::Normal | Look::Parallax);
    let map = textures.relief.of(&textures.grass);
    if tiling != GrassTiling::Stochastic {
        made.clear();
        if was == Some(GrassTiling::Stochastic) {
            for (entity, lawn) in &twinned {
                if let Ok(mut twin) = commands.get_entity(lawn.twin) {
                    twin.despawn();
                }
                commands
                    .entity(entity)
                    .remove::<Lawn>()
                    .insert(lawn.visibility);
            }
        }
        return;
    }
    if changed {
        // The relief may have changed: the lawn's materials have the
        // normal map or not.
        let normals = map.is_some() && relieved;
        for (_, lawn) in lawns.iter_mut() {
            if (lawn.extension.look.normals > 0.5) != normals {
                lawn.extension.look = look(&textures, normals);
            }
        }
    }
    // Those of materials gone are forgotten.
    made.retain(|id, _| standard.contains(*id));
    let candidates = if was != Some(GrassTiling::Stochastic) {
        all.iter().collect::<Vec<_>>()
    } else {
        new.iter().collect::<Vec<_>>()
    };
    for (entity, material, mesh, transform, visibility, parent, layers, casts_none, takes_none) in
        candidates
    {
        let Some(own) = standard.get(&material.0) else {
            continue;
        };
        let visibility = visibility.copied().unwrap_or_default();
        if own.base_color_texture.as_ref() != Some(&textures.grass) {
            continue;
        }
        let lawn = made
            .entry(material.0.id())
            .or_insert_with(|| lawns.add(stochastic(own, &textures, map, relieved)))
            .clone();
        let mut twin = commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(lawn),
            *transform,
            visibility,
            LawnTwin,
        ));
        if let Some(parent) = parent {
            twin.insert(ChildOf(parent.parent()));
        }
        if let Some(layers) = layers {
            twin.insert(layers.clone());
        }
        if casts_none {
            twin.insert(NotShadowCaster);
        }
        if takes_none {
            twin.insert(NotShadowReceiver);
        }
        let twin = twin.id();
        commands
            .entity(entity)
            .insert((Lawn { twin, visibility }, Visibility::Hidden));
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    /// A lawn of `size` px each way: grey stripes across, a light spot.
    fn lawn(size: u32) -> Image {
        let mut data = Vec::new();
        for y in 0..size {
            for x in 0..size {
                let stripe = if (y / 4) % 2 == 0 { 60 } else { 140 };
                let spot = if x < size / 8 && y < size / 8 { 100 } else { 0 };
                let value = (stripe + spot) as u8;
                data.extend_from_slice(&[value, value / 2, value / 3, 255]);
            }
        }
        Image::new(
            Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        )
    }

    #[test]
    fn the_triangles_weights_add_up_and_meet_at_the_squares_edges() {
        for flipped in [false, true] {
            for (fx, fy) in [(0.1, 0.7), (0.8, 0.3), (0.5, 0.5), (0.0, 0.0), (0.99, 0.01)] {
                let corners = triangle(fx, fy, flipped);
                let sum: f32 = corners.iter().map(|(_, w)| w).sum();
                assert!((sum - 1.0).abs() < 1e-6, "{sum}");
                assert!(corners.iter().all(|(_, w)| *w >= -1e-6));
                // Where it is: its corners, weighted.
                let at = corners.iter().fold(Vec2::ZERO, |at, ((i, j), w)| {
                    at + Vec2::new(*i as f32, *j as f32) * *w
                });
                assert!(at.distance(Vec2::new(fx, fy)) < 1e-5, "{at}");
            }
            // On the left and top edges of its square only those edges'
            // corners count, as they do from the squares beside it.
            for ((i, _), w) in triangle(0.0, 0.4, flipped) {
                assert!(i == 0 || w.abs() < 1e-6);
            }
            for ((_, j), w) in triangle(0.6, 0.0, flipped) {
                assert!(j == 0 || w.abs() < 1e-6);
            }
        }
    }

    #[test]
    fn sharpened_weights_add_up_and_favour_the_heaviest() {
        let sharpened = sharpen([0.5, 0.3, 0.2]);
        assert!((sharpened.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!(sharpened[0] > 0.8 && sharpened[2] < 0.05, "{sharpened:?}");
        assert_eq!(sharpen([1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn the_wide_lawn_repeats_without_seams() {
        let size = 64;
        let level = lawn(size).data.unwrap();
        let wide = size * WIDE;
        let corners = Corners::new(size, wide / PATCH);
        let mean = mean_of(&level);
        let at = |x, y| wide_pixel(&level, size, &corners, mean, x, y);
        for k in [0, 5, 31, 200] {
            for y in [0, 17, 63, 130] {
                assert_eq!(at(wide + k, y), at(k, y));
                assert_eq!(at(y, wide + k), at(y, k));
            }
        }
    }

    #[test]
    fn the_wide_lawn_is_the_lawn_in_pieces_with_its_colours() {
        let size = 256;
        let image = lawn(size);
        let wide = widen(&image);
        assert_eq!((wide.width(), wide.height()), (size * WIDE, size * WIDE));
        assert!(wide.texture_descriptor.mip_level_count > 1);
        let level = image.data.unwrap();
        let data = wide.data.as_deref().unwrap();
        let pixels = (size * WIDE * size * WIDE) as usize;
        let (from, to) = (mean_of(&level), mean_of(&data[..pixels * 4]));
        for (from, to) in from.iter().zip(to) {
            assert!((from - to).abs() < 3.0, "{from} to {to}");
        }
        // Its stripes are still there, as light and as dark: the blend
        // keeps the contrast.
        let red: Vec<u8> = data[..pixels * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0])
            .collect();
        let share =
            |of: &dyn Fn(u8) -> bool| red.iter().filter(|&&r| of(r)).count() as f32 / pixels as f32;
        let (dark, light) = (share(&|r| r < 80), share(&|r| r > 120));
        assert!(dark > 0.3 && light > 0.3, "dark {dark}, light {light}");
        // And it is not the lawn over again: most of it is another piece.
        let wide_side = size * WIDE;
        let moved = (0..pixels)
            .filter(|&n| {
                let (x, y) = (n as u32 % wide_side, n as u32 / wide_side);
                let plain = ((y % size) * size + x % size) as usize * 4;
                red[n].abs_diff(level[plain]) > 10
            })
            .count();
        assert!(moved as f32 > 0.3 * pixels as f32, "{moved} of {pixels}");
    }

    #[test]
    fn the_lawn_s_materials_take_the_wide_lawn_and_give_it_back() {
        let mut images = Assets::<Image>::default();
        let (lawn, wide) = (images.add(Image::default()), images.add(Image::default()));
        let mut material = StandardMaterial {
            base_color_texture: Some(lawn.clone()),
            ..default()
        };
        assert!(!tile(&mut material, &lawn, &wide, GrassTiling::Plain));
        assert!(!tile(&mut material, &lawn, &wide, GrassTiling::Stochastic));
        assert!(tile(&mut material, &lawn, &wide, GrassTiling::Wide));
        assert_eq!(material.base_color_texture, Some(wide.clone()));
        let corner = material.uv_transform.transform_point2(Vec2::ONE);
        assert_eq!(corner, Vec2::splat(1.0 / WIDE as f32));
        assert!(!tile(&mut material, &lawn, &wide, GrassTiling::Wide));
        assert!(tile(&mut material, &lawn, &wide, GrassTiling::Stochastic));
        assert_eq!(material.base_color_texture, Some(lawn.clone()));
        assert_eq!(material.uv_transform, bevy::math::Affine2::IDENTITY);
        // Another texture is left as it is.
        let rock = images.add(Image::default());
        let mut other = StandardMaterial {
            base_color_texture: Some(rock),
            ..default()
        };
        assert!(!tile(&mut other, &lawn, &wide, GrassTiling::Wide));
        assert!(!tile(
            &mut StandardMaterial::default(),
            &lawn,
            &wide,
            GrassTiling::Wide
        ));
    }

    #[test]
    fn the_stochastic_lawn_is_drawn_on_twins_that_go_when_the_tiling_changes() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<LawnMaterial>>()
            .init_resource::<ReliefShown>()
            .insert_resource(Settings {
                grass_tiling: GrassTiling::Stochastic,
                ..default()
            })
            .add_systems(Update, draw_stochastic);
        let textures = WorldTextures::from_world(app.world_mut());
        let grass = textures.grass.clone();
        let rock = textures.rock.clone();
        app.insert_resource(textures);
        let mut material = |texture: Handle<Image>| {
            app.world_mut()
                .resource_mut::<Assets<StandardMaterial>>()
                .add(StandardMaterial {
                    base_color_texture: Some(texture),
                    ..default()
                })
        };
        let (lawn, stone) = (material(grass), material(rock));
        let parent = app.world_mut().spawn(Transform::default()).id();
        let at = Transform::from_xyz(1.0, 2.0, 3.0);
        let ground = app
            .world_mut()
            .spawn((
                Mesh3d(Handle::default()),
                MeshMaterial3d(lawn.clone()),
                at,
                ChildOf(parent),
            ))
            .id();
        let cliff = app
            .world_mut()
            .spawn((Mesh3d(Handle::default()), MeshMaterial3d(stone)))
            .id();
        let twins = |app: &mut App| {
            app.world_mut()
                .query_filtered::<(Entity, &ChildOf, &Transform, &Visibility), With<LawnTwin>>()
                .iter(app.world())
                .map(|(twin, parent, at, seen)| (twin, parent.parent(), *at, *seen))
                .collect::<Vec<_>>()
        };
        app.update();
        // A twin drawn with the stochastic lawn where the ground is, under
        // the same parent; the ground hidden, its material as it was.
        let made = twins(&mut app);
        assert_eq!(made.len(), 1);
        let (twin, under, place, seen) = made[0];
        assert_eq!((under, place, seen), (parent, at, Visibility::Inherited));
        assert!(
            app.world()
                .get::<MeshMaterial3d<LawnMaterial>>(twin)
                .is_some()
        );
        assert_eq!(
            app.world().get::<Visibility>(ground),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(ground)
                .map(|m| m.0.clone()),
            Some(lawn)
        );
        // Not the rock.
        assert!(app.world().get::<Lawn>(cliff).is_none());
        // Once more: no second twin.
        app.update();
        assert_eq!(twins(&mut app).len(), 1);
        // Back to the lawn tiled: the twin gone, the ground seen again.
        app.world_mut().resource_mut::<Settings>().grass_tiling = GrassTiling::Plain;
        app.update();
        assert!(twins(&mut app).is_empty());
        assert!(app.world().get_entity(twin).is_err());
        assert_eq!(
            app.world().get::<Visibility>(ground),
            Some(&Visibility::Inherited)
        );
        assert!(app.world().get::<Lawn>(ground).is_none());
        // And stochastic again.
        app.world_mut().resource_mut::<Settings>().grass_tiling = GrassTiling::Stochastic;
        app.update();
        assert_eq!(twins(&mut app).len(), 1);
    }
}
