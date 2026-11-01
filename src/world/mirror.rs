//! Water that mirrors what stands round it (`mirror.wgsl`): still and dark,
//! the hills, the roads, the dams and the sky in it, more the lower one
//! looks across it, stirred by a faint ripple near the camera, as a
//! reservoir's. A dam's lake is drawn so (see [`super::dam`]), and so is
//! the sea, unless Options > Graphics > Water says otherwise (or the track
//! is a circuit whose sea is always the cartoon one, see
//! [`super::sea`]).
//!
//! What the water mirrors is mapped from above with the rest of the circuit,
//! away from the main thread ([`prepare_mirror`]): at each point, the colour
//! of what stands there, lit, and how high it stands over the water; and the
//! sky's gradient each way. The shader follows the reflected line of sight
//! over that map until it meets something, or else takes the sky's colour
//! that way. What stands on the ground (buildings, torii, the trees in
//! colour) has maps of its own, given as it is put in the world, and shows
//! in the water faintly, broken up ([`PreparedMirror::stand_up`]).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;

use super::dam::edge;
use super::scenery::{Scenery, Soil, Water};
use super::sky::{Palette, direction};
use super::structures::indices;
use super::terrain::{Ground, Terrain, on_cores};
use super::textures::WorldTextures;
use crate::track::{CURB_WIDTH, Track};

/// The sky it mirrors, mapped round the compass and from the horizon up.
const SKY_MAP: (u32, u32) = (64, 16);
/// The water's own colour, deep down (sRGB): a reservoir's dark green-blue,
/// the sea's too; and the wine red of a flooded pit rich in iron.
const DEEP: Color = Color::srgb(0.06, 0.17, 0.22);
const DEEP_IRON: Color = Color::srgb(0.24, 0.06, 0.04);
/// High country's rivers and lakes: deep green and grey.
const DEEP_RIVER: Color = Color::srgb(0.07, 0.18, 0.2);
/// How bright the sun's glint on the water is.
const GLINT: f32 = 4.0;
/// The ground as it is lit for the mirror: its colour times this much in
/// the shade, and this much more facing the sun.
const MIRROR_SHADE: f32 = 0.55;
const MIRROR_SUN: f32 = 0.75;
/// The colours, before they are lit, of what the textures make the rock,
/// the sand, the checkerboard earth, the asphalt and the concrete look on
/// average (linear).
const ROCK_AVERAGE: [f32; 3] = [0.3, 0.27, 0.23];
const SAND_AVERAGE: [f32; 3] = [0.55, 0.48, 0.36];
const SOIL_AVERAGE: [f32; 3] = [0.3, 0.18, 0.09];
const ASPHALT_AVERAGE: [f32; 3] = [0.08, 0.08, 0.085];
const CONCRETE_AVERAGE: [f32; 3] = [0.62, 0.6, 0.55];
/// How far round the centre line of a dam's crest it may stand over the
/// water (m): past its deck and its brackets.
const CREST_BOX: f32 = 25.0;

/// Water that mirrors what stands round it (see `mirror.wgsl`). Unlit, as
/// what it mirrors is lit already.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct MirrorMaterial {
    /// xy: corner of the map of what stands round the water (m, x and z);
    /// zw: its size (m).
    #[uniform(0)]
    pub area: Vec4,
    /// x: the water's level (m); y: how high over it the map's heights
    /// reach at most (m); z: how far a reflected line of sight is followed
    /// (m); w: 1 if anything stands round it (see [`Self::standing`]).
    #[uniform(1)]
    pub water: Vec4,
    /// What stands round the water: its colour as lit, and how high it
    /// stands over the water (see [`surroundings_map`]).
    #[texture(2)]
    #[sampler(3)]
    pub surroundings: Handle<Image>,
    /// The sky's colour each way (see [`sky_map`]).
    #[texture(4)]
    #[sampler(5)]
    pub sky: Handle<Image>,
    #[uniform(6)]
    pub look: MirrorLook,
    /// What stands on the ground round the water (buildings, torii, the
    /// crowns of the trees in colour), apart from it: its colour as lit and
    /// how high its top is over the water (under 0 where nothing stands);
    /// and how high over the water its solid part begins, and how much of it
    /// the water shows (see [`PreparedMirror::stand_up`]).
    #[texture(7)]
    #[sampler(8)]
    pub standing: Handle<Image>,
    #[texture(9)]
    pub standing_base: Handle<Image>,
}

/// The water's own colour and the sun's glint on it (see `mirror.wgsl`).
#[derive(ShaderType, Clone, Copy, Debug, PartialEq)]
pub struct MirrorLook {
    /// The water's colour, deep down (linear).
    pub deep: Vec4,
    /// xyz: towards the sun; w: how bright its glint is.
    pub sun: Vec4,
}

impl Material for MirrorMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://motorust_engine/world/mirror.wgsl".into()
    }

    // Nothing to draw before the colour or into the shadows: the water is
    // flat and unlit.
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// What is mapped round some water for it to mirror: the box (m, x and z),
/// a texel every `texel` m, the water's level (m) and how far a reflected
/// line of sight is followed over it (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub min: Vec2,
    pub max: Vec2,
    pub texel: f32,
    pub level: f32,
    pub follow: f32,
}

/// Mirroring water worked out away from the main thread, to be drawn with a
/// [`MirrorMaterial`] once its maps are images.
pub struct PreparedMirror {
    area: Vec4,
    water: Vec4,
    look: MirrorLook,
    surroundings: Image,
    sky: Image,
    /// What stands round the water, once it is given (see
    /// [`Self::stand_up`]): the map of its tops and colours, and of its
    /// bases and how much of it the water shows.
    standing: Option<(Image, Image)>,
}

/// What is built round the water is looked at in bands this tall (m), each
/// texel of the map in `STANDING_PARTS` × `STANDING_PARTS` parts: where its
/// points cover `SOLID_PARTS` of them or more in a band and the two next to
/// it (as tall as its points may be apart), it is solid (a floor, a wall, a
/// roof); where they cover fewer, it is thin (stilts, posts), too thin for
/// the map to draw, so the water shows what is behind it, as under a shrine
/// on stilts. A crown of leaves is never thin, and the water shows it
/// `LEAVES_SEEN` as much as what is built: light gets through it.
const STANDING_BAND: f32 = 0.5;
const STANDING_PARTS: usize = 4;
const SOLID_PARTS: u32 = 3;
const LEAVES_SEEN: f32 = 0.5;

/// A point over the surface of something standing round the water (a
/// building, a torii, the crown of a tree), for the water to mirror it (see
/// [`PreparedMirror::stand_up`]): where it is, which way its face looks and
/// its colour there, before it is lit (linear).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MirrorPoint {
    pub at: Vec3,
    pub normal: Vec3,
    pub color: [f32; 3],
}

impl MirrorPoint {
    /// This point of something drawn with `transform`.
    pub fn placed(self, transform: &Transform) -> Self {
        Self {
            at: transform.transform_point(self.at),
            normal: (transform.rotation * self.normal).normalize_or_zero(),
            ..self
        }
    }
}

/// What stands over a texel of the map in a band (see [`STANDING_BAND`]):
/// which of its parts it covers, the colours there, lit, and how much of
/// them the water shows, summed, how many points, and whether any are
/// leaves.
#[derive(Clone, Copy, Default)]
struct Band {
    parts: u16,
    color: Vec3,
    seen: f32,
    count: f32,
    leaves: bool,
}

/// What stands over a texel of the map, as the water mirrors it: its
/// colour, lit; how high over the water it begins and ends (m); and how much
/// of it the water shows.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Standing {
    color: Vec3,
    base: f32,
    top: f32,
    seen: f32,
}

impl Standing {
    /// What the bands over a texel stand as: from the lowest solid one to
    /// the top of the highest, in their colours; nothing if none is solid.
    fn of(bands: &HashMap<i32, Band>) -> Option<Self> {
        let parts = |band: i32| bands.get(&band).map_or(0, |band| band.parts);
        let solid: Vec<(i32, &Band)> = bands
            .iter()
            .filter(|&(&k, band)| {
                band.leaves || (parts(k - 1) | parts(k) | parts(k + 1)).count_ones() >= SOLID_PARTS
            })
            .map(|(&k, band)| (k, band))
            .collect();
        let lowest = solid.iter().map(|&(k, _)| k).min()?;
        let highest = solid.iter().map(|&(k, _)| k).max()?;
        let sum = solid.iter().fold(Band::default(), |sum, (_, band)| Band {
            color: sum.color + band.color,
            seen: sum.seen + band.seen,
            count: sum.count + band.count,
            ..sum
        });
        Some(Self {
            color: sum.color / sum.count,
            base: lowest as f32 * STANDING_BAND,
            top: (highest + 1) as f32 * STANDING_BAND,
            seen: sum.seen / sum.count,
        })
    }
}

impl PreparedMirror {
    /// What is `built` round the water (buildings, torii) and the `leaves`
    /// of the trees in colour mirrored too, apart from the ground, in maps
    /// of their own: each texel they reach, from where they are solid up (see
    /// [`STANDING_BAND`]), in their colour on average, lit by the sun as the
    /// ground is. The texels round it take its colour and its base too, with
    /// nothing standing, so that the maps, read between texels, blend its
    /// edge into nothing, not into black. The map of the ground is worked
    /// out with the circuit; what stands on it is put in the world later,
    /// and given here.
    pub fn stand_up(&mut self, built: &[MirrorPoint], leaves: &[MirrorPoint]) {
        let (width, height) = (
            self.surroundings.width() as usize,
            self.surroundings.height() as usize,
        );
        let (corner, size) = (self.area.xy(), self.area.zw());
        let texel = size / Vec2::new(width as f32, height as f32);
        let sun = self.look.sun.truncate();
        let level = self.water.x;
        // Each texel reached, band by band.
        let mut columns: HashMap<usize, HashMap<i32, Band>> = HashMap::new();
        for (points, seen) in [(built, 1.0), (leaves, LEAVES_SEEN)] {
            for point in points {
                let at = (point.at.xz() - corner) / texel;
                // Under the water (a shrine's footing), it mirrors nothing.
                if at.x < 0.0
                    || at.y < 0.0
                    || at.x >= width as f32
                    || at.y >= height as f32
                    || point.at.y < level
                {
                    continue;
                }
                let index = at.y as usize * width + at.x as usize;
                let part = (at.fract() * STANDING_PARTS as f32).as_uvec2();
                let part = part.y as usize * STANDING_PARTS + part.x as usize;
                let band = ((point.at.y - level) / STANDING_BAND).floor() as i32;
                let light = MIRROR_SHADE + MIRROR_SUN * point.normal.dot(sun).max(0.0);
                let entry = columns.entry(index).or_default().entry(band).or_default();
                entry.parts |= 1 << part;
                entry.color += Vec3::from(point.color) * light;
                entry.seen += seen;
                entry.count += 1.0;
                entry.leaves |= seen < 1.0;
            }
        }
        let standing: HashMap<usize, Standing> = columns
            .iter()
            .filter_map(|(&index, bands)| Some((index, Standing::of(bands)?)))
            .collect();
        // The texels round it, with nothing standing: its colour, base and
        // how much it is seen, on average.
        let mut fringe: HashMap<usize, (Standing, f32)> = HashMap::new();
        for (&index, stands) in &standing {
            let (i, j) = ((index % width) as i32, (index / width) as i32);
            for (di, dj) in [
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let (i, j) = (i + di, j + dj);
                let next = j as usize * width + i as usize;
                if i < 0
                    || j < 0
                    || i >= width as i32
                    || j >= height as i32
                    || standing.contains_key(&next)
                {
                    continue;
                }
                let (sum, n) = fringe.entry(next).or_insert((
                    Standing {
                        color: Vec3::ZERO,
                        base: 0.0,
                        top: -1.0,
                        seen: 0.0,
                    },
                    0.0,
                ));
                sum.color += stands.color;
                sum.base += stands.base;
                sum.seen += stands.seen;
                *n += 1.0;
            }
        }
        // Nothing standing anywhere else, its base down at the water.
        let mut tops = nothing_standing().repeat(width * height);
        let mut bases = vec![0; width * height * 4];
        let fringe = fringe.into_iter().map(|(index, (sum, n))| {
            let average = Standing {
                color: sum.color / n,
                base: sum.base / n,
                seen: sum.seen / n,
                ..sum
            };
            (index, average)
        });
        for (index, stands) in standing
            .iter()
            .map(|(&index, &stands)| (index, stands))
            .chain(fringe)
        {
            let color = stands.color;
            for (k, value) in [color.x, color.y, color.z, stands.top]
                .into_iter()
                .enumerate()
            {
                let at = index * 8 + 2 * k;
                tops[at..at + 2].copy_from_slice(&half_float(value).to_le_bytes());
            }
            for (k, value) in [stands.base, stands.seen].into_iter().enumerate() {
                let at = index * 4 + 2 * k;
                bases[at..at + 2].copy_from_slice(&half_float(value).to_le_bytes());
            }
            if stands.top > 0.0 {
                self.water.y = self.water.y.max(stands.top);
                self.water.w = 1.0;
            }
        }
        let image = |data, format| {
            map_image(
                (width as u32, height as u32),
                data,
                format,
                ImageAddressMode::ClampToEdge,
            )
        };
        self.standing = Some((
            image(tops, TextureFormat::Rgba16Float),
            image(bases, TextureFormat::Rg16Float),
        ));
    }

    /// Its material, its maps added to `images`.
    pub fn into_material(self, images: &mut Assets<Image>) -> MirrorMaterial {
        // Nothing standing: a texel of nothing.
        let (standing, standing_base) = self.standing.unwrap_or_else(|| {
            (
                map_image(
                    (1, 1),
                    nothing_standing(),
                    TextureFormat::Rgba16Float,
                    ImageAddressMode::ClampToEdge,
                ),
                map_image(
                    (1, 1),
                    vec![0; 4],
                    TextureFormat::Rg16Float,
                    ImageAddressMode::ClampToEdge,
                ),
            )
        });
        MirrorMaterial {
            area: self.area,
            water: self.water,
            surroundings: images.add(self.surroundings),
            sky: images.add(self.sky),
            look: self.look,
            standing: images.add(standing),
            standing_base: images.add(standing_base),
        }
    }
}

/// A texel of the map of what stands round the water where nothing does
/// (see [`PreparedMirror::stand_up`]): no colour, its top under the water.
fn nothing_standing() -> Vec<u8> {
    [0.0, 0.0, 0.0, -1.0]
        .into_iter()
        .flat_map(|value| half_float(value).to_le_bytes())
        .collect()
}

/// The water's own colour, deep down (sRGB), for `water`.
pub fn deep(water: Water) -> Color {
    match water {
        Water::Sea => DEEP,
        Water::Iron => DEEP_IRON,
        Water::River => DEEP_RIVER,
    }
}

/// Water at `reach`'s level that mirrors what stands round it on `track`
/// with `scenery` (the map worked out on `threads` threads, leaving out
/// where `unseen` says no line of sight from the water ever gets), deep down
/// `deep` (sRGB).
#[allow(clippy::too_many_arguments)]
pub fn prepare_mirror(
    track: &Track,
    terrain: &Terrain,
    scenery: &Scenery,
    textures: &WorldTextures,
    reach: &Reach,
    unseen: &(dyn Fn(Vec2) -> bool + Sync),
    deep: Color,
    threads: usize,
) -> PreparedMirror {
    let palette = Palette::of(scenery.sky);
    let sun = palette.sun_direction();
    let grass_average = textures.grass_color.to_linear();
    let (corner, size, highest, surroundings) = surroundings_map(
        track,
        terrain,
        scenery,
        reach,
        unseen,
        sun,
        grass_average,
        threads,
    );
    PreparedMirror {
        area: corner.extend(size.x).extend(size.y),
        water: Vec4::new(reach.level, highest, reach.follow, 0.0),
        look: MirrorLook {
            deep: deep.to_linear().to_vec4(),
            sun: sun.extend(GLINT),
        },
        surroundings,
        sky: sky_map(&palette),
        standing: None,
    }
}

/// Water at `reach`'s level that mirrors only the sky as yet, deep down
/// `deep` (sRGB): what stands round it is mapped later, away from the main
/// thread ([`prepare_mirror`]), and put in its material when it is (as a
/// circuit is shown before all of it is ready).
pub fn sky_mirror(scenery: &Scenery, reach: &Reach, deep: Color) -> PreparedMirror {
    let palette = Palette::of(scenery.sky);
    let size = reach.max - reach.min;
    PreparedMirror {
        area: reach.min.extend(size.x).extend(size.y),
        water: Vec4::new(reach.level, 0.0, reach.follow, 0.0),
        look: MirrorLook {
            deep: deep.to_linear().to_vec4(),
            sun: palette.sun_direction().extend(GLINT),
        },
        // One texel of nothing, under the water, over the whole of it.
        surroundings: map_image(
            (1, 1),
            nothing_standing(),
            TextureFormat::Rgba16Float,
            ImageAddressMode::ClampToEdge,
        ),
        sky: sky_map(&palette),
        standing: None,
    }
}

/// What stands round some water within `reach`, mapped from above for it to
/// mirror: the map's corner and size (m), how high over the water its
/// heights reach at most (m), and an image of, at each point, the colour it
/// shows lit by the sun towards `sun` (rgb, linear) and how high it stands
/// over the water (alpha, m; under it, less than 0), in half floats, fine
/// enough for the low, grazing lines of sight across the water. The ground,
/// by what it is (its grass's greens over the grass's `grass_average`
/// colour, rock, sand, snow); the roads' asphalt where they lie on it; and
/// the dams' crests, standing up from the water to the road. What is under
/// the water, and wherever `unseen` says no line of sight from the water
/// ever gets, is down at it, with no colour. Worked out on `threads`
/// threads.
#[allow(clippy::too_many_arguments)]
pub fn surroundings_map(
    track: &Track,
    terrain: &Terrain,
    scenery: &Scenery,
    reach: &Reach,
    unseen: &(dyn Fn(Vec2) -> bool + Sync),
    sun: Vec3,
    grass_average: LinearRgba,
    threads: usize,
) -> (Vec2, Vec2, f32, Image) {
    let (width, height) = (
        ((reach.max.x - reach.min.x) / reach.texel).ceil().max(1.0) as u32,
        ((reach.max.y - reach.min.y) / reach.texel).ceil().max(1.0) as u32,
    );
    let lap = track.lap_length();
    let crests: Vec<usize> = scenery
        .dams
        .iter()
        .flat_map(|dam| indices(track, dam.crest, lap))
        .collect();
    // Where the crests stand: the only places under the water where
    // something stands up out of it.
    let crest_boxes: Vec<(Vec2, Vec2)> = scenery
        .dams
        .iter()
        .map(|dam| {
            indices(track, dam.crest, lap)
                .into_iter()
                .map(|i| track.centerline()[i])
                .fold(
                    (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                    |(min, max), p| (min.min(p - CREST_BOX), max.max(p + CREST_BOX)),
                )
        })
        .collect();
    let by_a_crest = |at: Vec2| {
        crest_boxes
            .iter()
            .any(|(min, max)| at.cmpge(*min).all() && at.cmple(*max).all())
    };
    let rows: Vec<u32> = (0..height).collect();
    let texels = on_cores(&rows, threads, |rows| {
        let mut texels = Vec::with_capacity(rows.len() * width as usize);
        for &j in rows {
            for i in 0..width {
                let at = reach.min + (Vec2::new(i as f32, j as f32) + 0.5) * reach.texel;
                // Under the water nothing is ever met, nor its colour seen:
                // only its height is needed (but by a dam's crest).
                texels.push(if unseen(at) {
                    ([0.0; 3], reach.level)
                } else {
                    let ground = terrain.height(at);
                    if ground < reach.level && !by_a_crest(at) {
                        ([0.0; 3], ground)
                    } else {
                        texel(track, terrain, &crests, at, sun, grass_average)
                    }
                });
            }
        }
        texels
    });
    let mut highest = 0.0f32;
    let mut data = Vec::with_capacity(texels.len() * 8);
    for (color, top) in texels {
        let over = top - reach.level;
        highest = highest.max(over);
        for value in [color[0], color[1], color[2], over] {
            data.extend(half_float(value).to_le_bytes());
        }
    }
    let size = Vec2::new(width as f32, height as f32) * reach.texel;
    (
        reach.min,
        size,
        highest,
        map_image(
            (width, height),
            data,
            TextureFormat::Rgba16Float,
            ImageAddressMode::ClampToEdge,
        ),
    )
}

/// What stands at `at`, lit by the sun towards `sun`: its colour (linear)
/// and the height of its top (m). The crests of the dams are the points of
/// the lap `crests`.
fn texel(
    track: &Track,
    terrain: &Terrain,
    crests: &[usize],
    at: Vec2,
    sun: Vec3,
    grass_average: LinearRgba,
) -> ([f32; 3], f32) {
    let ground = terrain.height(at);
    let normal = terrain.normal(at);
    let p = Vec3::new(at.x, ground, at.y);
    let nearest = track.nearest(at);
    let road = track.height_beside(&nearest);
    let on_crest = nearest.road == 0 && crests.contains(&nearest.index);
    let (top, color) = if on_crest && nearest.distance < edge(track, nearest.index) {
        (road, CONCRETE_AVERAGE)
    } else if nearest.distance < track.half_width_beside(&nearest) + CURB_WIDTH
        && (road - ground).abs() < 1.0
    {
        (ground, ASPHALT_AVERAGE)
    } else {
        let kind = terrain.ground_drawn(at);
        let tint = terrain.ground_color(kind, p, normal);
        // The texture each ground is drawn with (see
        // `terrain::spawn_terrain`): a desert canyon's flat ground is dusty
        // earth, not grass.
        let texture = match (kind, terrain.soil()) {
            (Ground::Grass, Soil::Mine) | (Ground::Sand, _) => SAND_AVERAGE,
            (Ground::Grass, _) => [grass_average.red, grass_average.green, grass_average.blue],
            (Ground::Rock, Soil::Checkered) => SOIL_AVERAGE,
            (Ground::Rock, _) => ROCK_AVERAGE,
            (Ground::Snow, _) => [0.9, 0.92, 0.95],
        };
        (ground, [0, 1, 2].map(|k| tint[k] * texture[k]))
    };
    let light = MIRROR_SHADE + MIRROR_SUN * normal.dot(sun).max(0.0);
    (color.map(|c| c * light), top)
}

/// The bits of `value` as a half float (IEEE 754 binary16), rounded to the
/// nearest; those too small for one, 0, and those too big, infinite.
pub(super) fn half_float(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = ((bits >> 13) & 0x3ff) as u16;
    let round = ((bits >> 12) & 1) as u16;
    match exponent {
        ..=0 => sign,
        31.. => sign | 0x7c00,
        // Rounding up past the mantissa's top carries into the exponent.
        _ => (sign | ((exponent as u16) << 10) | mantissa) + round,
    }
}

/// The value of half-float bits (see [`half_float`]).
#[cfg(any(test, feature = "testing"))]
pub(super) fn from_half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let mantissa = (bits & 0x3ff) as f32 / 1024.0;
    match exponent {
        0 => 0.0,
        31 => sign * f32::INFINITY,
        _ => sign * (1.0 + mantissa) * 2f32.powi(exponent - 15),
    }
}

/// The sky's colour each way, as it is drawn (see [`Palette`]): across,
/// round the compass from the north (-Z) towards the east (+X); down, from
/// overhead to the horizon.
fn sky_map(palette: &Palette) -> Image {
    let (width, height) = SKY_MAP;
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for j in 0..height {
        for i in 0..width {
            let azimuth = (i as f32 + 0.5) / width as f32 * std::f32::consts::TAU;
            let elevation = (1.0 - (j as f32 + 0.5) / height as f32) * std::f32::consts::FRAC_PI_2;
            let color = palette.sky_color(direction(azimuth, elevation)).to_srgba();
            data.extend([
                to_byte(color.red),
                to_byte(color.green),
                to_byte(color.blue),
                255,
            ]);
        }
    }
    map_image(
        (width, height),
        data,
        TextureFormat::Rgba8UnormSrgb,
        ImageAddressMode::Repeat,
    )
}

fn to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// An image of `data` in `format` for a shader to look things up in,
/// smoothly, wrapping across (`across`) and clamped down.
fn map_image(
    (width, height): (u32, u32),
    data: Vec<u8>,
    format: TextureFormat,
    across: ImageAddressMode,
) -> Image {
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: across,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    /// What a map holds at `p` (m): its colour and how high it stands over
    /// the water.
    pub(in crate::world) fn mapped(image: &Image, corner: Vec2, texel: f32, p: Vec2) -> [f32; 4] {
        let width = image.width() as usize;
        let data = image.data.as_ref().expect("the map's texels");
        let (i, j) = (
            ((p.x - corner.x) / texel) as usize,
            ((p.y - corner.y) / texel) as usize,
        );
        let at = (j * width + i) * 8;
        [0, 1, 2, 3]
            .map(|c| from_half(u16::from_le_bytes([data[at + 2 * c], data[at + 2 * c + 1]])))
    }

    #[test]
    fn half_floats_keep_the_heights_round_the_water_to_a_few_centimetres() {
        assert_eq!(half_float(1.0), 0x3c00);
        assert_eq!(half_float(0.5), 0x3800);
        assert_eq!(half_float(-2.0), 0xc000);
        assert_eq!(half_float(1e-6), 0);
        assert_eq!(half_float(1e6), 0x7c00);
        for value in [0.37f32, 3.2, 27.9, -14.6, 85.3] {
            assert!(
                (from_half(half_float(value)) - value).abs() < 0.04,
                "{value}"
            );
        }
    }

    pub fn a_desert_canyon_mirrors_its_red_earth_not_grass_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        use crate::world::sea::{MIRROR_INLAND, ShoreMap, mirror_reach};
        let circuit = circuits
            .iter()
            .find(|circuit| circuit.scenery.soil == Soil::Mine)
            .expect("a desert canyon");
        let track = Track::new(&circuit.layout);
        let terrain = Terrain::new(&track, &circuit.scenery);
        let shore = ShoreMap::new(&terrain, &track);
        let reach = mirror_reach(&track);
        let unseen = |at| shore.distance_at(at) < -MIRROR_INLAND;
        let grass = Color::srgb(0.25, 0.5, 0.12).to_linear();
        let (corner, _, highest, image) = surroundings_map(
            &track,
            &terrain,
            &circuit.scenery,
            &reach,
            &unseen,
            Vec3::Y,
            grass,
            4,
        );
        assert!(highest > 10.0, "{highest}");
        // The land round the water, on average: red earth and rock.
        let (mut sum, mut count) = (Vec3::ZERO, 0);
        let step = reach.texel * 4.0;
        let mut at = corner + step / 2.0;
        while at.y < reach.max.y {
            at.x = corner.x + step / 2.0;
            while at.x < reach.max.x {
                let [r, g, b, over] = mapped(&image, corner, reach.texel, at);
                if over > 1.0 && shore.distance_at(at) > -200.0 {
                    sum += Vec3::new(r, g, b);
                    count += 1;
                }
                at.x += step;
            }
            at.y += step;
        }
        assert!(count > 100, "{count}");
        let average = sum / count as f32;
        assert!(
            average.x > 1.2 * average.y && average.y > average.z,
            "{average:?}"
        );
    }

    /// The values at texel (`i`, `j`) of a map of half floats.
    fn texel_values(image: &Image, (i, j): (usize, usize)) -> Vec<f32> {
        let data = image.data.as_ref().expect("the map's texels");
        let size = data.len() / (image.width() * image.height()) as usize;
        let at = (j * image.width() as usize + i) * size;
        data[at..at + size]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&half| from_half(u16::from_le_bytes(half)))
            .collect()
    }

    #[test]
    fn what_stands_by_the_water_is_mirrored_from_where_it_is_solid_up() {
        // A map 10 texels a side, 2 m each, of ground 1 m over the water.
        let mut data = Vec::new();
        for _ in 0..100 {
            for value in [0.1, 0.2, 0.1, 1.0] {
                data.extend(half_float(value).to_le_bytes());
            }
        }
        let ground = data.clone();
        let mut mirror = PreparedMirror {
            area: Vec4::new(0.0, 0.0, 20.0, 20.0),
            water: Vec4::new(-1.0, 1.0, 500.0, 0.0),
            look: MirrorLook {
                deep: Vec4::ONE,
                sun: Vec4::new(0.0, 1.0, 0.0, 1.0),
            },
            surroundings: map_image(
                (10, 10),
                data,
                TextureFormat::Rgba16Float,
                ImageAddressMode::ClampToEdge,
            ),
            sky: Image::default(),
            standing: None,
        };
        let (red, pink) = ([0.8, 0.1, 0.05], [0.9, 0.6, 0.65]);
        let point = |x: f32, y: f32, z: f32, color| MirrorPoint {
            at: Vec3::new(x, y, z),
            normal: Vec3::Y,
            color,
        };
        let mut built = Vec::new();
        // A wall from 1 to 5.5 m over the water, its points in rows half a
        // metre apart, each over two of the four parts across a texel, one
        // row over the first two and the next over the other two.
        let wall = |built: &mut Vec<MirrorPoint>, (x, z): (f32, f32), heights: [f32; 2]| {
            let mut y = heights[0];
            let mut row = 0;
            while y <= heights[1] {
                for a in [0.3, 0.8] {
                    built.push(point(
                        x + a + if row % 2 == 0 { 0.0 } else { 1.0 },
                        y,
                        z,
                        red,
                    ));
                }
                y += 0.5;
                row += 1;
            }
        };
        // A red shrine over the texel from (2, 2) to (4, 4): its floor 3 m
        // over the water, a wall and its roof 8.5 m over the water, on a
        // stilt in its middle, standing on a footing under the water.
        for (a, b) in [0.25, 0.75, 1.25, 1.75]
            .into_iter()
            .flat_map(|a| [0.25, 0.75, 1.25, 1.75].map(|b| (a, b)))
        {
            built.push(point(2.0 + a, 2.0, 2.0 + b, red));
            built.push(point(2.0 + a, 7.5, 2.0 + b, red));
        }
        wall(&mut built, (2.0, 2.2), [2.0, 7.5]);
        for k in 0..8 {
            built.push(point(3.0, -1.0 + 0.4 * k as f32, 3.0, red));
        }
        // Its footing, a slab under the water.
        for (a, b) in [0.25, 0.75, 1.25, 1.75]
            .into_iter()
            .flat_map(|a| [0.25, 0.75, 1.25, 1.75].map(|b| (a, b)))
        {
            built.push(point(2.0 + a, -1.6, 2.0 + b, red));
        }
        // A wall on its own from (2, 8) to (4, 10).
        wall(&mut built, (2.0, 9.0), [0.0, 4.0]);
        // A stilt on its own from (8, 2) to (10, 4), a post of a boardwalk.
        for k in 0..8 {
            built.push(point(9.0, -1.0 + 0.4 * k as f32, 3.0, red));
        }
        // And the crown of a tree in blossom, 6 to 7.5 m over the water,
        // from (12, 12) to (14, 14): a few points, never thin.
        let leaves = [point(13.0, 5.2, 13.0, pink), point(13.5, 6.2, 12.5, pink)];
        mirror.stand_up(&built, &leaves);

        let (tops, bases) = mirror.standing.as_ref().expect("something stands");
        let shrine = texel_values(tops, (1, 1));
        let base = texel_values(bases, (1, 1));
        assert!((shrine[3] - 9.0).abs() < 0.05, "{shrine:?}");
        assert!(shrine[0] > 4.0 * shrine[1], "{shrine:?}");
        // Seen through under its floor (but for half a metre).
        assert!(base[0] > 2.4 && base[0] < 3.05, "{base:?}");
        assert!((base[1] - 1.0).abs() < 0.01, "{base:?}");
        // The wall on its own stands, solid, from its foot to its top.
        let alone = (texel_values(tops, (1, 4)), texel_values(bases, (1, 4)));
        assert!((alone.0[3] - 5.5).abs() < 0.05, "{alone:?}");
        assert!((alone.1[0] - 1.0).abs() < 0.05, "{alone:?}");
        // The stilt on its own stands as nothing.
        assert!(texel_values(tops, (4, 1))[3] < 0.0);
        // The crown, from its lowest point to its highest, faint.
        let (crown, under) = (texel_values(tops, (6, 6)), texel_values(bases, (6, 6)));
        assert!((crown[3] - 7.5).abs() < 0.05, "{crown:?}");
        assert!((under[0] - 6.0).abs() < 0.05, "{under:?}");
        assert!((under[1] - LEAVES_SEEN).abs() < 0.01, "{under:?}");
        assert!(crown[0] > crown[2] && crown[2] > 0.3, "{crown:?}");
        // Round the shrine, nothing stands, in its colour and from its base,
        // so that it blends into nothing; far from it, nothing at all.
        let (round, round_base) = (texel_values(tops, (2, 1)), texel_values(bases, (2, 1)));
        assert!(round[3] < 0.0 && round[0] > 4.0 * round[1], "{round:?}");
        assert!((round_base[0] - base[0]).abs() < 0.01, "{round_base:?}");
        assert_eq!(texel_values(tops, (8, 2)), vec![0.0, 0.0, 0.0, -1.0]);
        // The ground is mapped as it was; the shader follows the line of
        // sight as high as the shrine and knows something stands.
        assert_eq!(mirror.surroundings.data.as_deref(), Some(&ground[..]));
        assert!(mirror.water.y >= 9.0);
        assert_eq!(mirror.water.w, 1.0);
    }

    #[test]
    fn with_nothing_standing_the_water_mirrors_the_ground_alone() {
        let mut mirror = PreparedMirror {
            area: Vec4::new(0.0, 0.0, 20.0, 20.0),
            water: Vec4::new(-1.0, 1.0, 500.0, 0.0),
            look: MirrorLook {
                deep: Vec4::ONE,
                sun: Vec4::new(0.0, 1.0, 0.0, 1.0),
            },
            surroundings: map_image(
                (10, 10),
                vec![0; 800],
                TextureFormat::Rgba16Float,
                ImageAddressMode::ClampToEdge,
            ),
            sky: Image::default(),
            standing: None,
        };
        // Points off the map, and a stilt alone.
        let stilt: Vec<MirrorPoint> = (0..8)
            .map(|k| MirrorPoint {
                at: Vec3::new(5.0, -1.0 + 0.4 * k as f32, 5.0),
                normal: Vec3::Y,
                color: [1.0; 3],
            })
            .chain([MirrorPoint {
                at: Vec3::new(-5.0, 3.0, 30.0),
                normal: Vec3::Y,
                color: [1.0; 3],
            }])
            .collect();
        mirror.stand_up(&stilt, &[]);
        assert_eq!(mirror.water.w, 0.0);
        assert_eq!(mirror.water.y, 1.0);
    }

    #[test]
    fn the_sea_is_as_dark_as_the_lake_and_a_pit_s_water_red() {
        assert_eq!(deep(Water::Sea), DEEP);
        let iron = deep(Water::Iron).to_srgba();
        assert!(iron.red > 2.0 * iron.green && iron.red > 2.0 * iron.blue);
    }
}
