//! Texturas del escenario: fotos CC0 de Poly Haven (ver `assets/CREDITS.md`)
//! retocadas con colores más vivos, como en los juegos de los 90. Van
//! incrustadas en el ejecutable y sus mipmaps se calculan al arrancar, así no
//! parpadean a lo lejos.

use std::sync::Arc;

use super::relief::{Bumps, ReliefMaps};
use crate::geometry::{fbm, random01, smoothstep, value_noise};
use bevy::{
    asset::RenderAssetUsages,
    image::{
        CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler,
        ImageSamplerDescriptor, ImageType,
    },
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// Cloned (handles only) for the threads that prepare the circuits.
#[derive(Resource, Clone)]
pub struct WorldTextures {
    pub grass: Handle<Image>,
    /// The lawn four times as wide each way, put together from pieces of
    /// it, for the grass tiled wide (see [`super::lawn`]).
    pub grass_wide: Handle<Image>,
    pub rock: Handle<Image>,
    /// The rock without its warm tint: grey granite (see [`Soil::Granite`]).
    ///
    /// [`Soil::Granite`]: super::scenery::Soil::Granite
    pub granite: Handle<Image>,
    pub sand: Handle<Image>,
    pub asphalt: Handle<Image>,
    pub wall: Handle<Image>,
    pub roof: Handle<Image>,
    pub plaster: Handle<Image>,
    /// Tablero de ajedrez blanco y negro de 2 × 2 casillas.
    pub checker: Handle<Image>,
    /// Paneles de hormigón con juntas, para los túneles.
    pub panels: Handle<Image>,
    /// Gradas de público con ropa de colores vivos como en los arcades de los 90.
    pub crowd: Handle<Image>,
    /// Two by two squares of brown checkerboard earth, as on an arcade island.
    pub soil: Handle<Image>,
    /// Streaks of falling water, repeating downwards.
    pub falls: Handle<Image>,
    /// Wooden planks laid across, for the wooden bridges.
    pub planks: Handle<Image>,
    /// Layers of sandstone, cream, salmon, rust and ochre, one over another
    /// down the texture.
    pub strata: Handle<Image>,
    /// A tuft of grass blades seen from the side (see
    /// [`super::foliage::tuft`]).
    pub tuft: Handle<Image>,
    /// The ragged edge of the grass along the road (see
    /// [`super::foliage::grass_edge`]).
    pub grass_edge: Handle<Image>,
    /// A pool of lamplight: white in the middle, fading out to nothing at
    /// its edges (see [`light_pool`]).
    pub light_pool: Handle<Image>,
    /// A room's varnished floorboards, indoors (see [`floorboards`]).
    pub floor: Handle<Image>,
    /// A slot-car track's dark plastic, two lanes of it a repetition (see
    /// [`slot_track`]).
    pub slot_track: Handle<Image>,
    /// The toy cars' orange track, two lanes of it a repetition (see
    /// [`orange_track`]).
    pub orange_track: Handle<Image>,
    /// A slide's glossy yellow plastic (see [`chute`]).
    pub chute: Handle<Image>,
    /// A shop's ceiling of white tiles in a grid (see [`ceiling_tiles`]).
    pub ceiling: Handle<Image>,
    /// The fronts of boxed toys, eight of them side by side (see
    /// [`toy_boxes`]).
    pub toy_boxes: Handle<Image>,
    /// A green baseplate of plastic bricks, studded (see [`baseplate`]).
    pub baseplate: Handle<Image>,
    /// A mat of foam tiles in bright colours, fitting together like a
    /// jigsaw (see [`foam_mat`]).
    pub foam: Handle<Image>,
    /// The faces of toy blocks, sixteen letters each of its colour (see
    /// [`block_faces`]).
    pub blocks: Handle<Image>,
    /// Red and white stripes, a slot-car track's borders (see [`stripes`]).
    pub stripes: Handle<Image>,
    /// The average colour of the grass texture: the tufts of grass are
    /// painted it (see [`super::tufts`]).
    pub grass_color: Color,
    /// The lawn as painted, kept to paint it in flower for the circuits with
    /// fields of flowers, as they are prepared (see
    /// [`super::foliage::meadow`]).
    pub lawn: Arc<Image>,
    /// The relief of the textures that have one (see [`super::relief`]).
    pub relief: ReliefMaps,
}

impl FromWorld for WorldTextures {
    fn from_world(world: &mut World) -> Self {
        let mut images = world.resource_mut::<Assets<Image>>();
        // The grass photo, under a mat of blades painted over it.
        let lawn = super::foliage::lawn(&embedded(
            crate::content::file("textures/grass.jpg"),
            ImageAddressMode::Repeat,
            TextureEnhance::Grass,
        ));
        let grass_color = average_color(&lawn);
        let grass = images.add(lawn.clone());
        let lawn = Arc::new(lawn);
        // The wide lawn, the longest to make, on a thread of its own while
        // the rest are (but on the web, where there is only one).
        let widening = (!cfg!(target_arch = "wasm32")).then(|| {
            let lawn = lawn.clone();
            std::thread::spawn(move || super::lawn::widen(&lawn))
        });
        // The stones of the walls, with the photo's grain.
        let wall = images.add(stone_wall(&embedded(
            crate::content::file("textures/wall.jpg"),
            ImageAddressMode::Repeat,
            TextureEnhance::Wall,
        )));
        let mut tiled = |bytes: &[u8], enhance: TextureEnhance| {
            images.add(embedded(bytes, ImageAddressMode::Repeat, enhance))
        };
        let rock = tiled(
            crate::content::file("textures/rock.jpg"),
            TextureEnhance::Rock,
        );
        let granite = tiled(
            crate::content::file("textures/rock.jpg"),
            TextureEnhance::Granite,
        );
        let sand = tiled(
            crate::content::file("textures/sand.jpg"),
            TextureEnhance::Sand,
        );
        let asphalt = tiled(
            crate::content::file("textures/asphalt.jpg"),
            TextureEnhance::Asphalt,
        );
        let roof = tiled(
            crate::content::file("textures/roof.jpg"),
            TextureEnhance::None,
        );
        let plaster = tiled(
            crate::content::file("textures/plaster.jpg"),
            TextureEnhance::None,
        );
        let checker = images.add(checker());
        let panels = images.add(panels());
        let crowd = images.add(crowd());
        let soil = images.add(soil());
        let falls = images.add(falls());
        let planks = images.add(planks());
        let strata = images.add(strata());
        let tuft = images.add(super::foliage::tuft());
        let grass_edge = images.add(super::foliage::grass_edge());
        let light_pool = images.add(light_pool());
        let floor = images.add(floorboards());
        let slot_track = images.add(slot_track());
        let orange_track = images.add(orange_track());
        let chute = images.add(chute());
        let ceiling = images.add(ceiling_tiles());
        let toy_boxes = images.add(toy_boxes());
        let baseplate = images.add(baseplate());
        let foam = images.add(foam_mat());
        let blocks = images.add(block_faces());
        let stripes = images.add(stripes());
        let grass_wide = images.add(match widening {
            Some(thread) => thread.join().expect("the wide lawn is made"),
            None => super::lawn::widen(&lawn),
        });
        let relief = ReliefMaps::new(
            &mut images,
            &[
                (&grass, GRASS_BUMPS),
                (&grass_wide, GRASS_BUMPS),
                (&soil, SOIL_BUMPS),
                (&rock, ROCK_BUMPS),
                (&granite, ROCK_BUMPS),
                (&sand, SAND_BUMPS),
                (&asphalt, ASPHALT_BUMPS),
                (&wall, WALL_BUMPS),
                (&roof, ROOF_BUMPS),
                (&plaster, PLASTER_BUMPS),
                (&panels, PANEL_BUMPS),
                (&planks, PLANK_BUMPS),
                (&strata, STRATA_BUMPS),
            ],
        );
        Self {
            grass,
            grass_wide,
            rock,
            granite,
            sand,
            asphalt,
            wall,
            roof,
            plaster,
            checker,
            panels,
            crowd,
            soil,
            falls,
            planks,
            strata,
            tuft,
            grass_edge,
            light_pool,
            floor,
            slot_track,
            orange_track,
            chute,
            ceiling,
            toy_boxes,
            baseplate,
            foam,
            blocks,
            stripes,
            grass_color,
            lawn,
            relief,
        }
    }
}

/// How the bumps of each texture with relief are cut out of it (see
/// [`super::relief`]): about as wide as the bumps (px), how high they stand
/// (px of the texture), and whether parallax shifts them too, on the
/// surfaces seen close. The rocks' are cracks and ledges over cliffs 20 m a
/// repetition (25 px a metre); the asphalt's and the sand's, grains and
/// ripples of a few millimetres, a little more than true; the stone walls'
/// (160 px a metre), stones some 3 cm proud of their gaps; the roofs', the
/// rows of clay tiles; the plaster's, its trowelling; the tunnels' panels,
/// their joints; the planks', the gaps between them; the strata's, their
/// seams; the grass's, its blades, a few centimetres, and the checkerboard
/// earth's, the bevels of its squares. Parallax on the surfaces seen close,
/// and on the grass, seen close at the roadside (the most of it there is on
/// the screen, the most it costs).
const GRASS_BUMPS: Bumps = Bumps {
    broad: 8,
    height: 2.5,
    parallax: true,
};
const SOIL_BUMPS: Bumps = Bumps {
    broad: 16,
    height: 3.0,
    parallax: true,
};
const ROCK_BUMPS: Bumps = Bumps {
    broad: 24,
    height: 3.0,
    parallax: false,
};
const SAND_BUMPS: Bumps = Bumps {
    broad: 12,
    height: 1.2,
    parallax: false,
};
const ASPHALT_BUMPS: Bumps = Bumps {
    broad: 8,
    height: 1.0,
    parallax: false,
};
const WALL_BUMPS: Bumps = Bumps {
    broad: 20,
    height: 5.0,
    parallax: true,
};
const ROOF_BUMPS: Bumps = Bumps {
    broad: 24,
    height: 4.0,
    parallax: true,
};
const PLASTER_BUMPS: Bumps = Bumps {
    broad: 8,
    height: 1.0,
    parallax: false,
};
const PANEL_BUMPS: Bumps = Bumps {
    broad: 16,
    height: 2.0,
    parallax: true,
};
const PLANK_BUMPS: Bumps = Bumps {
    broad: 8,
    height: 1.5,
    parallax: true,
};
const STRATA_BUMPS: Bumps = Bumps {
    broad: 12,
    height: 2.0,
    parallax: false,
};

/// The average colour of the full-size level of an sRGB image, averaged in
/// linear space.
fn average_color(image: &Image) -> Color {
    let pixels = (image.width() * image.height()) as usize;
    let Some(data) = image.data.as_ref() else {
        return Color::WHITE;
    };
    let sum = data[..pixels * 4].chunks(4).fold(Vec3::ZERO, |sum, p| {
        let linear = Color::srgb_u8(p[0], p[1], p[2]).to_linear();
        sum + Vec3::new(linear.red, linear.green, linear.blue)
    });
    let mean = sum / pixels as f32;
    Color::linear_rgb(mean.x, mean.y, mean.z)
}

pub(super) fn sampler(vertical: ImageAddressMode) -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: vertical,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        // Nitidez en el suelo visto en ángulo rasante, como el asfalto a lo lejos.
        anisotropy_clamp: 16,
        ..default()
    })
}

/// How much of the rock photo's colour granite keeps (see
/// [`TextureEnhance::Granite`]).
const GRANITE_COLOR: f32 = 0.15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureEnhance {
    Grass,
    Rock,
    /// The rock, grey (see [`WorldTextures::granite`]).
    Granite,
    Sand,
    Asphalt,
    Wall,
    None,
}

fn embedded(bytes: &[u8], vertical: ImageAddressMode, enhance: TextureEnhance) -> Image {
    let mut image = Image::from_buffer(
        bytes,
        ImageType::Extension("jpg"),
        CompressedImageFormats::NONE,
        true,
        sampler(vertical),
        RenderAssetUsages::RENDER_WORLD,
    )
    .expect("las texturas incrustadas son JPEG válidos");
    enhance_texture(&mut image, enhance);
    generate_mipmaps(&mut image);
    image
}

/// Realce de contraste y micro-detalle para texturas retro estilo arcade:
/// asfalto con más grano de árido, hierba más frondosa y viva, y roca con estratos más marcados.
fn enhance_texture(image: &mut Image, kind: TextureEnhance) {
    if matches!(kind, TextureEnhance::None) {
        return;
    }
    let width = image.width() as usize;
    let height = image.height() as usize;
    let Some(data) = image.data.as_mut() else {
        return;
    };

    let orig = data.clone();
    let get_pixel = |x: usize, y: usize| -> [f32; 3] {
        let idx = (y * width + x) * 4;
        [
            orig[idx] as f32 / 255.0,
            orig[idx + 1] as f32 / 255.0,
            orig[idx + 2] as f32 / 255.0,
        ]
    };

    let mut sum_lum = 0.0f32;
    for idx in (0..width * height * 4).step_by(4) {
        let r0 = orig[idx] as f32 / 255.0;
        let g0 = orig[idx + 1] as f32 / 255.0;
        let b0 = orig[idx + 2] as f32 / 255.0;
        sum_lum += 0.299 * r0 + 0.587 * g0 + 0.114 * b0;
    }
    let avg_lum = sum_lum / (width * height) as f32;

    for y in 0..height {
        for x in 0..width {
            let center = get_pixel(x, y);
            // Desenfoque 3x3 continuo con repetición en los bordes para mantener la textura sin costuras
            let mut blur = [0.0f32; 3];
            for dy in [-1isize, 0, 1] {
                for dx in [-1isize, 0, 1] {
                    let sx = (x as isize + dx).rem_euclid(width as isize) as usize;
                    let sy = (y as isize + dy).rem_euclid(height as isize) as usize;
                    let p = get_pixel(sx, sy);
                    for c in 0..3 {
                        blur[c] += p[c] / 9.0;
                    }
                }
            }

            let mut r = center[0];
            let mut g = center[1];
            let mut b = center[2];

            match kind {
                TextureEnhance::Asphalt => {
                    // Micro-grano de árido sutil y natural, sin ruido de destellos
                    let high_pass = [
                        center[0] - blur[0],
                        center[1] - blur[1],
                        center[2] - blur[2],
                    ];
                    r = (r + 0.40 * high_pass[0]).clamp(0.0, 1.0);
                    g = (g + 0.40 * high_pass[1]).clamp(0.0, 1.0);
                    b = (b + 0.40 * high_pass[2]).clamp(0.0, 1.0);

                    // Curva de luminosidad calibrada para asfalto de circuito:
                    // tono pizarra neutro, legible con sol y sombra, sin quedar negro empastado
                    let lum = 0.299 * r + 0.587 * g + 0.114 * b;
                    let target_lum = (lum * 1.38 + 0.025).clamp(0.0, 1.0);
                    let ratio = if lum > 1e-4 { target_lum / lum } else { 1.0 };
                    r = (r * ratio).clamp(0.0, 1.0);
                    g = (g * ratio).clamp(0.0, 1.0);
                    b = (b * ratio).clamp(0.0, 1.0);
                }
                TextureEnhance::Grass => {
                    let high_pass = [
                        center[0] - blur[0],
                        center[1] - blur[1],
                        center[2] - blur[2],
                    ];
                    // Gentle macro-gradient detrending to eliminate repetitive tiling blotches
                    let blur_lum = 0.299 * blur[0] + 0.587 * blur[1] + 0.114 * blur[2];
                    let detrend = (avg_lum - blur_lum) * 0.40;
                    r = (r + 0.58 * high_pass[0] + detrend).clamp(0.0, 1.0);
                    g = (g + 0.58 * high_pass[1] + detrend).clamp(0.0, 1.0);
                    b = (b + 0.58 * high_pass[2] + detrend).clamp(0.0, 1.0);

                    // 90s arcade meadow green hue, vibrant and balanced
                    g = (g * 1.15).min(1.0);
                    r = (r * 0.92 + 0.03).clamp(0.0, 1.0);
                    b = (b * 0.85).clamp(0.0, 1.0);
                }
                TextureEnhance::Rock | TextureEnhance::Granite => {
                    // Dual-phase seamless blend to break the prominent continuous horizontal crack in rock.jpg
                    let half_w = width / 2;
                    let half_h = height / 2;
                    let ox = (x + half_w) % width;
                    let oy = (y + half_h) % height;
                    let phase2 = get_pixel(ox, oy);

                    // Organic periodic blending weight that tiles seamlessly across 512x512
                    let fx = x as f32 / width as f32 * std::f32::consts::TAU;
                    let fy = y as f32 / height as f32 * std::f32::consts::TAU;
                    let harmonic = (fx * 2.0).sin() * (fy * 3.0).cos() * 0.26
                        + (fx * 4.0 + fy * 2.0).sin() * 0.14;
                    let blend_w = (0.5 + harmonic).clamp(0.18, 0.82);

                    let base_r = center[0] * (1.0 - blend_w) + phase2[0] * blend_w;
                    let base_g = center[1] * (1.0 - blend_w) + phase2[1] * blend_w;
                    let base_b = center[2] * (1.0 - blend_w) + phase2[2] * blend_w;

                    // Wide-kernel macro detrending (17x17) to eliminate large repeating corner blotches
                    let mut macro_sum = [0.0f32; 3];
                    let step = (width / 16).max(8) as isize;
                    for mdy in [-step, 0, step] {
                        for mdx in [-step, 0, step] {
                            let msx = (x as isize + mdx).rem_euclid(width as isize) as usize;
                            let msy = (y as isize + mdy).rem_euclid(height as isize) as usize;
                            let mp = get_pixel(msx, msy);
                            for c in 0..3 {
                                macro_sum[c] += mp[c] / 9.0;
                            }
                        }
                    }
                    let macro_lum =
                        0.299 * macro_sum[0] + 0.587 * macro_sum[1] + 0.114 * macro_sum[2];
                    let detrend = (avg_lum - macro_lum) * 0.45;

                    // Crisp high-pass micro-relief on stone fissures without overpowering black bars
                    let high_pass = [
                        center[0] - blur[0],
                        center[1] - blur[1],
                        center[2] - blur[2],
                    ];
                    r = (base_r + 0.72 * high_pass[0] + detrend).clamp(0.0, 1.0);
                    g = (base_g + 0.72 * high_pass[1] + detrend).clamp(0.0, 1.0);
                    b = (base_b + 0.72 * high_pass[2] + detrend).clamp(0.0, 1.0);

                    // Luminous, warm Mediterranean limestone/granite tone curve
                    let lum = 0.299 * r + 0.587 * g + 0.114 * b;
                    let target_lum = ((lum - 0.48) * 1.16 + 0.48).clamp(0.0, 1.0);
                    let ratio = if lum > 1e-4 { target_lum / lum } else { 1.0 };
                    r = (r * ratio * 1.06 + 0.015).clamp(0.0, 1.0);
                    g = (g * ratio * 1.02 + 0.01).clamp(0.0, 1.0);
                    b = (b * ratio * 0.96).clamp(0.0, 1.0);
                    if kind == TextureEnhance::Granite {
                        // Grey, keeping a trace of the photo's colour, and a
                        // touch cool, as granite in the shade of a gorge.
                        let lum = 0.299 * r + 0.587 * g + 0.114 * b;
                        r = lum + (r - lum) * GRANITE_COLOR;
                        g = lum + (g - lum) * GRANITE_COLOR;
                        b = ((lum + (b - lum) * GRANITE_COLOR) * 1.04).min(1.0);
                    }
                }
                TextureEnhance::Sand => {
                    let high_pass = [
                        center[0] - blur[0],
                        center[1] - blur[1],
                        center[2] - blur[2],
                    ];
                    let blur_lum = 0.299 * blur[0] + 0.587 * blur[1] + 0.114 * blur[2];
                    let detrend = (avg_lum - blur_lum) * 0.35;
                    r = (r + 0.55 * high_pass[0] + detrend).clamp(0.0, 1.0);
                    g = (g + 0.55 * high_pass[1] + detrend).clamp(0.0, 1.0);
                    b = (b + 0.55 * high_pass[2] + detrend).clamp(0.0, 1.0);
                    // Warm, luminous sand
                    r = (r * 1.06).min(1.0);
                    g = (g * 1.02).min(1.0);
                    b = (b * 0.93).clamp(0.0, 1.0);
                }
                TextureEnhance::Wall => {
                    let high_pass = [
                        center[0] - blur[0],
                        center[1] - blur[1],
                        center[2] - blur[2],
                    ];
                    r = (r + 0.85 * high_pass[0]).clamp(0.0, 1.0);
                    g = (g + 0.85 * high_pass[1]).clamp(0.0, 1.0);
                    b = (b + 0.85 * high_pass[2]).clamp(0.0, 1.0);
                    let lum = 0.299 * r + 0.587 * g + 0.114 * b;
                    let contrasted_lum = ((lum - 0.5) * 1.25 + 0.5).clamp(0.0, 1.0);
                    let ratio = if lum > 1e-4 {
                        contrasted_lum / lum
                    } else {
                        1.0
                    };
                    r = (r * ratio).clamp(0.0, 1.0);
                    g = (g * ratio).clamp(0.0, 1.0);
                    b = (b * ratio).clamp(0.0, 1.0);
                }
                TextureEnhance::None => {}
            }

            let idx = (y * width + x) * 4;
            data[idx] = (r * 255.0).round() as u8;
            data[idx + 1] = (g * 255.0).round() as u8;
            data[idx + 2] = (b * 255.0).round() as u8;
        }
    }
}

/// A pool of lamplight, to be added over what it falls on: white in the
/// middle, fading smoothly to black at its edges, round.
fn light_pool() -> Image {
    const SIZE: u32 = 64;
    let data = (0..SIZE * SIZE)
        .flat_map(|i| {
            let p =
                (Vec2::new((i % SIZE) as f32, (i / SIZE) as f32) + 0.5) / SIZE as f32 * 2.0 - 1.0;
            let light =
                (-3.2 * p.length_squared()).exp() * (1.0 - smoothstep(0.75, 1.0, p.length()));
            let value = (light * 255.0).round() as u8;
            [value, value, value, value]
        })
        .collect();
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
    image.sampler = sampler(ImageAddressMode::ClampToEdge);
    if let ImageSampler::Descriptor(descriptor) = &mut image.sampler {
        descriptor.address_mode_u = ImageAddressMode::ClampToEdge;
    }
    generate_mipmaps(&mut image);
    image
}

/// What each repetition of the stone wall covers (m along the wall and up
/// it): far wider than it is tall, so its pattern does not show.
pub const STONE_WALL: Vec2 = Vec2::new(9.6, 2.4);

/// A wall of rough stones laid in uneven courses, as the dry-stone walls
/// along Mediterranean roads: each stone of its own shape, size and tone
/// (limestone, grey, ochre, brown), rounded at its edges and lit from
/// above, with dark gaps between them and the fine grain of the stone wall
/// `photo` on its faces. It covers [`STONE_WALL`] and repeats both ways.
fn stone_wall(photo: &Image) -> Image {
    const WIDTH: u32 = 1536;
    const HEIGHT: u32 = 384;
    // The stones: a staggered grid of cells, each with its stone somewhere
    // in it.
    const COLUMNS: i32 = 24;
    const ROWS: i32 = 10;
    let cell = Vec2::new(WIDTH as f32 / COLUMNS as f32, HEIGHT as f32 / ROWS as f32);
    let seed_of = |column: i32, row: i32| {
        let (c, r) = (column.rem_euclid(COLUMNS), row.rem_euclid(ROWS));
        let n = (r * COLUMNS + c) as u32;
        let stagger = 0.5 * (row.rem_euclid(2)) as f32;
        let jitter = Vec2::new(
            0.15 + 0.7 * random01(n * 4),
            0.3 + 0.4 * random01(n * 4 + 1),
        );
        (
            (Vec2::new(column as f32 + stagger, row as f32) + jitter) * cell,
            n,
        )
    };
    // The grain of the photo: its fine detail only, not its stones.
    let (pw, ph) = (photo.width() as i32, photo.height() as i32);
    let source = photo.data.as_ref().expect("the photo is in memory");
    let lum = |x: i32, y: i32| {
        let i = ((y.rem_euclid(ph) * pw + x.rem_euclid(pw)) * 4) as usize;
        (0.299 * source[i] as f32 + 0.587 * source[i + 1] as f32 + 0.114 * source[i + 2] as f32)
            / 255.0
    };
    let grain = |x: i32, y: i32| {
        let mut blur = 0.0;
        for (dx, dy) in [(-2, 0), (2, 0), (0, -2), (0, 2)] {
            blur += lum(x + dx, y + dy) / 4.0;
        }
        lum(x, y) - blur
    };
    let tones = [
        (0.35, Color::srgb(0.8, 0.72, 0.58)),
        (0.2, Color::srgb(0.86, 0.83, 0.76)),
        (0.2, Color::srgb(0.66, 0.65, 0.62)),
        (0.15, Color::srgb(0.82, 0.66, 0.47)),
        (0.1, Color::srgb(0.6, 0.52, 0.42)),
    ];
    let tone = |n: u32| {
        let mut pick = random01(n * 4 + 2);
        for (share, color) in tones {
            if pick < share {
                return color.to_linear();
            }
            pick -= share;
        }
        tones[0].1.to_linear()
    };
    let mortar = Color::srgb(0.46, 0.42, 0.36).to_linear();
    let light = Vec2::new(-0.6, -0.8);
    // Distances are measured squashed along, so the stones come out longer
    // than they are tall, laid flat in their courses.
    let squash = Vec2::new(0.62, 1.0);
    tiling(WIDTH, HEIGHT, |x, y| {
        let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
        let row = (p.y / cell.y).floor() as i32;
        // The nearest two stones.
        let mut nearest = [(f32::MAX, Vec2::ZERO, 0u32); 2];
        for r in row - 1..=row + 1 {
            let stagger = 0.5 * (r.rem_euclid(2)) as f32;
            let column = (p.x / cell.x - stagger).floor() as i32;
            for c in column - 1..=column + 1 {
                let (at, n) = seed_of(c, r);
                let d = (p * squash).distance_squared(at * squash);
                if d < nearest[0].0 {
                    nearest[1] = nearest[0];
                    nearest[0] = (d, at, n);
                } else if d < nearest[1].0 {
                    nearest[1] = (d, at, n);
                }
            }
        }
        let [(d1, s1, n), (d2, s2, _)] = nearest;
        // How far inside its stone (px): to the line half way between them.
        let edge = (d2 - d1) / (2.0 * (s1 * squash).distance(s2 * squash).max(1e-3));
        let gap = 2.2 + 1.6 * fbm(p / 23.0, 2);
        let stone = smoothstep(gap, gap + 1.5, edge);
        // Rounded edges: the face slopes down towards the gap, lit on the
        // side of the light.
        let toward = ((s2 - s1) * squash).normalize_or_zero();
        let rounding = 1.0 - smoothstep(gap, gap + 10.0, edge);
        let bevel = 1.0 + 0.45 * rounding * toward.dot(light);
        let lumps = 0.88 + 0.24 * fbm(p / 9.0 + Vec2::splat(n as f32), 3);
        let brightness = (0.86 + 0.24 * random01(n * 4 + 3)) * bevel * lumps;
        // Dark lichen here and there.
        let lichen = smoothstep(0.66, 0.8, fbm(p / 6.0 + 31.0, 3)) * 0.35;
        let detail = 1.0 + 1.6 * grain(x as i32, y as i32);
        let face = tone(n) * (brightness * detail * (1.0 - lichen));
        let deep = 0.55 + 0.45 * (edge / gap).clamp(0.0, 1.0);
        let gap_color = mortar * (deep * (0.85 + 0.3 * fbm(p / 5.0, 2)));
        Color::LinearRgba(gap_color.mix(&face, stone))
    })
}

/// Dos por dos paneles de hormigón, cada uno algo distinto, con juntas
/// oscuras y manchas: se repite sin que se note la costura.
fn panels() -> Image {
    const SIZE: u32 = 256;
    const JOINT: u32 = 5;
    let data = (0..SIZE * SIZE)
        .flat_map(|i| {
            let (x, y) = (i % SIZE, i / SIZE);
            let half = SIZE / 2;
            let (px, py) = (x % half, y % half);
            let joint = px < JOINT || py < JOINT;
            // Cada panel, de un tono; con manchas finas y gruesas.
            let panel = (x / half + 2 * (y / half)) as f32;
            let point = Vec2::new(x as f32, y as f32);
            let stain = fbm(point / 18.0 + Vec2::splat(panel * 7.3), 3);
            let grain = value_noise(point / 2.5);
            let tone =
                0.78 + 0.05 * (panel * 1.7).sin() + 0.12 * (stain - 0.5) + 0.06 * (grain - 0.5);
            let value = if joint { 0.35 } else { tone };
            let v = (value.clamp(0.0, 1.0) * 255.0) as u8;
            [v, v, v, 255]
        })
        .collect();
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
    image.sampler = sampler(ImageAddressMode::Repeat);
    generate_mipmaps(&mut image);
    image
}

fn checker() -> Image {
    const SIZE: u32 = 64;
    let data = (0..SIZE * SIZE)
        .flat_map(|i| {
            let (x, y) = (i % SIZE, i / SIZE);
            let value = if (x < SIZE / 2) == (y < SIZE / 2) {
                245
            } else {
                15
            };
            [value, value, value, 255]
        })
        .collect();
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
    image.sampler = sampler(ImageAddressMode::Repeat);
    generate_mipmaps(&mut image);
    image
}

/// How many px a metre the floorboards are drawn at, how wide each board
/// is (m) and how long (m): a texture of eight boards, 12.8 m square.
const FLOOR_PX: f32 = 40.0;
const BOARD_WIDTH: f32 = 1.6;
const BOARD_LENGTH: f32 = 6.4;

/// A room's floorboards, honey-coloured oak, varnished: boards along x, each
/// [`BOARD_WIDTH`] wide, their ends staggered, each its own tone, with the
/// grain of the wood along them and a thin dark gap between them. It
/// repeats every 12.8 m both ways.
fn floorboards() -> Image {
    const SIZE: u32 = 512;
    let oak = Color::srgb(0.66, 0.45, 0.26).to_linear();
    let gap = Color::srgb(0.16, 0.1, 0.06);
    tiling(SIZE, SIZE, |x, y| {
        let (u, v) = (x as f32 / FLOOR_PX, y as f32 / FLOOR_PX);
        let board = (v / BOARD_WIDTH).floor() as u32;
        // Each row's ends a different way along, so they never line up.
        let shift = random01(board * 7 + 3) * BOARD_LENGTH;
        let along = (u + shift).rem_euclid(BOARD_LENGTH);
        let piece = ((u + shift) / BOARD_LENGTH).floor() as u32;
        let across = v.rem_euclid(BOARD_WIDTH);
        if across < 0.06 || along < 0.05 {
            return gap;
        }
        // Each board of its own tone, darker or lighter, a little redder or
        // yellower.
        let tone = 0.68 + 0.5 * random01(board * 31 + piece * 7 + 11);
        let warm = random01(board * 13 + piece * 3 + 5) - 0.5;
        let grain = value_noise(Vec2::new(u * 0.35 + piece as f32 * 9.0, v * 9.0));
        let figure = fbm(
            Vec2::new(u * 0.8, v * 3.0) + Vec2::splat(board as f32 * 5.1),
            3,
        );
        let light = tone * (0.8 + 0.3 * grain) * (0.85 + 0.3 * figure);
        let tint = LinearRgba::rgb(1.0 + 0.12 * warm, 1.0, 1.0 - 0.15 * warm);
        Color::LinearRgba(LinearRgba::rgb(
            oak.red * tint.red * light,
            oak.green * tint.green * light,
            oak.blue * tint.blue * light,
        ))
    })
}

/// A slot-car track's surface: dark charcoal plastic, a little speckled,
/// two lanes a repetition across (3.5 m each on the road's 7 m texture),
/// each with its slot down the middle, a narrow black groove between two
/// strips of copper braid, and a thin pale line at each lane's edge; a
/// joint between the pieces of track at each repetition along.
fn slot_track() -> Image {
    const SIZE: u32 = 256;
    let plastic = Color::srgb(0.07, 0.07, 0.08);
    tiling(SIZE, SIZE, |x, y| {
        let (u, v) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
        // Across a lane, from its edge (0 to 1), and from its slot (m).
        let lane = (u * 2.0).fract();
        let from_slot = (lane - 0.5).abs() * 3.5;
        if from_slot < 0.05 {
            return Color::srgb(0.01, 0.01, 0.01);
        }
        if (0.1..0.2).contains(&from_slot) {
            let shine = 0.85 + 0.15 * value_noise(Vec2::new(u * 400.0, v * 30.0));
            return Color::srgb(0.58 * shine, 0.58 * shine, 0.6 * shine);
        }
        let edge = lane.min(1.0 - lane) * 3.5;
        if edge < 0.04 {
            return Color::srgb(0.7, 0.7, 0.68);
        }
        if !(0.006..=0.994).contains(&v) {
            return Color::srgb(0.02, 0.02, 0.02);
        }
        let speckle = 0.9 + 0.2 * value_noise(Vec2::new(x as f32, y as f32) * 0.7);
        let worn = 0.95 + 0.1 * fbm(Vec2::new(u * 9.0, v * 3.0), 2);
        plastic.mix(
            &Color::srgb(0.16, 0.16, 0.17),
            (speckle * worn - 0.9).max(0.0),
        )
    })
}

/// The toy cars' track: bright orange plastic, two lanes a repetition
/// across, each with a row of chevrons down its middle pointing the way
/// the road runs (towards +v) and a low ridge between the lanes; a darker
/// joint where each piece of track meets the next.
fn orange_track() -> Image {
    const SIZE: u32 = 256;
    let orange = Color::srgb(1.0, 0.42, 0.04);
    let dark = Color::srgb(0.86, 0.28, 0.02);
    tiling(SIZE, SIZE, |x, y| {
        let (u, v) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
        let lane = (u * 2.0).fract();
        let edge = lane.min(1.0 - lane);
        if edge < 0.02 {
            return Color::srgb(1.0, 0.56, 0.18);
        }
        if edge < 0.035 {
            return dark;
        }
        if v < 0.01 {
            return Color::srgb(0.7, 0.22, 0.02);
        }
        // Four chevrons a repetition, each a V pointing along +v.
        let across = (lane - 0.5).abs();
        let along = (v * 4.0).fract();
        let chevron = along - 0.3 - across * 0.9;
        if across < 0.22 && (0.0..0.12).contains(&chevron) {
            return dark;
        }
        let sheen = 0.96 + 0.06 * value_noise(Vec2::new(u * 40.0, v * 12.0));
        orange.mix(&Color::WHITE, (sheen - 1.0).max(0.0) * 2.0)
    })
}

/// A slide's chute: glossy yellow plastic, a faint sheen along it and a
/// seam across it at each repetition.
fn chute() -> Image {
    const SIZE: u32 = 128;
    let yellow = Color::srgb(1.0, 0.8, 0.08);
    tiling(SIZE, SIZE, |x, y| {
        let (u, v) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
        if v < 0.015 {
            return Color::srgb(0.85, 0.6, 0.04);
        }
        let sheen = 0.5 + 0.5 * (u * std::f32::consts::TAU * 2.0).cos();
        yellow.mix(&Color::srgb(1.0, 0.95, 0.6), 0.25 * sheen * sheen)
    })
}

/// A shop's ceiling: square white tiles in a grid of thin grey rails, four
/// tiles a repetition, each finely speckled, as acoustic tiles are.
fn ceiling_tiles() -> Image {
    const SIZE: u32 = 256;
    tiling(SIZE, SIZE, |x, y| {
        let (px, py) = (x % (SIZE / 2), y % (SIZE / 2));
        if px < 3 || py < 3 {
            return Color::srgb(0.62, 0.62, 0.6);
        }
        let speckle = value_noise(Vec2::new(x as f32, y as f32) * 0.9);
        let v = 0.9 + 0.08 * speckle;
        Color::srgb(v, v, v * 0.98)
    })
}

/// How many box fronts [`toy_boxes`] draws side by side.
pub const TOY_BOXES: u32 = 16;
/// Which of [`toy_boxes`]' fronts is which: the first eight, windows
/// showing the toy inside; then two dolls', a jigsaw's, a board game's, a
/// carton's, a white box's (to be tinted), a space toy's and one in polka
/// dots.
pub const DOLL_BOXES: [u32; 2] = [8, 9];
pub const JIGSAW_BOX: u32 = 10;
pub const GAME_BOX: u32 = 11;
pub const CARTON: u32 = 12;
pub const WHITE_BOX: u32 = 13;
pub const SPACE_BOX: u32 = 14;
pub const DOTTED_BOX: u32 = 15;

/// The fronts of boxed toys, [`TOY_BOXES`] side by side, as the boxes on a
/// toy shop's shelves look from across the room, each over a plain stretch
/// of its colour along its foot (what its sides and top are painted with):
///
/// - the first eight, each in its own bright colour: a white band
///   with a coloured logo at the top, a window in the middle showing the
///   toy inside (a round head over a body, or a car, or bricks), a stripe
///   of another colour down one side and a darker border;
/// - two dolls' ([`DOLL_BOXES`]), pink and lilac: an arched window, a doll
///   in it with golden hair and a dress;
/// - a jigsaw's ([`JIGSAW_BOX`]): hills, a house and the sun under the sky,
///   cut into pieces, over a blue foot;
/// - a board game's ([`GAME_BOX`]): its board, four coloured corners round
///   a white cross, on dark blue;
/// - a carton ([`CARTON`]), brown cardboard taped shut, a ride-on car
///   printed on it in black and arrows for the way up;
/// - a white box ([`WHITE_BOX`]), grey where the others are coloured, a
///   robot in its window: tinted, it is any colour;
/// - a space toy's ([`SPACE_BOX`]): a rocket among stars on dark blue;
/// - polka dots ([`DOTTED_BOX`]), white on blue.
fn toy_boxes() -> Image {
    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 96;
    let paints = [
        Color::srgb(0.95, 0.3, 0.6),
        Color::srgb(0.15, 0.4, 0.9),
        Color::srgb(0.9, 0.12, 0.1),
        Color::srgb(1.0, 0.8, 0.1),
        Color::srgb(0.2, 0.7, 0.3),
        Color::srgb(0.55, 0.25, 0.85),
        Color::srgb(1.0, 0.5, 0.05),
        Color::srgb(0.1, 0.7, 0.75),
    ];
    let paint_of = |index: u32| match index {
        8 => Color::srgb(0.98, 0.45, 0.72),
        9 => Color::srgb(0.72, 0.52, 0.95),
        JIGSAW_BOX => Color::srgb(0.15, 0.35, 0.78),
        GAME_BOX => Color::srgb(0.1, 0.18, 0.5),
        CARTON => Color::srgb(0.76, 0.58, 0.38),
        WHITE_BOX => Color::srgb(0.95, 0.95, 0.95),
        SPACE_BOX => Color::srgb(0.07, 0.09, 0.28),
        DOTTED_BOX => Color::srgb(0.12, 0.6, 0.88),
        _ => paints[index as usize % paints.len()],
    };
    tiling(WIDTH * TOY_BOXES, HEIGHT, |x, y| {
        let index = x / WIDTH;
        let paint = paint_of(index);
        let accent = paints[(index as usize + 3) % paints.len()];
        let (u, v) = ((x % WIDTH) as f32 / WIDTH as f32, y as f32 / HEIGHT as f32);
        let shade = 0.92 + 0.08 * value_noise(Vec2::new(x as f32, y as f32) * 0.3);
        let plain = paint.mix(&Color::BLACK, 1.0 - shade);
        let border = u.min(1.0 - u).min(v.min(1.0 - v) * 1.5);
        if border < 0.04 && index != CARTON {
            return paint.mix(&Color::BLACK, 0.45);
        }
        // The logo band at the top, with its blob of colour.
        let logo = Vec2::new((u - 0.5) * 2.2, (v - 0.1) * 6.0).length() < 0.55;
        // The foot, plain.
        if v > 0.9 {
            return plain;
        }
        match index {
            _ if DOLL_BOXES.contains(&index) => {
                if v < 0.18 {
                    return if logo { accent } else { Color::WHITE };
                }
                // The arched window, the doll in it.
                let (wu, wv) = ((u - 0.15) / 0.7, (v - 0.22) / 0.64);
                let arch = Vec2::new(wu - 0.5, (wv - 0.3) * 0.9).length() < 0.5;
                if !(0.0..1.0).contains(&wu) || !(0.0..1.0).contains(&wv) || (wv < 0.3 && !arch) {
                    return plain;
                }
                let face = Vec2::new(wu - 0.5, (wv - 0.24) * 0.75).length();
                if face < 0.13 {
                    return Color::srgb(0.98, 0.83, 0.72);
                }
                if (face < 0.19 && wv < 0.33)
                    || ((wu - 0.5).abs() < 0.19 && (0.24..0.5).contains(&wv))
                {
                    return Color::srgb(0.98, 0.82, 0.3);
                }
                if (0.36..0.92).contains(&wv) && (wu - 0.5).abs() < 0.05 + 0.32 * (wv - 0.36) {
                    return paint.mix(&Color::BLACK, 0.3);
                }
                Color::srgb(1.0, 0.92, 0.96)
            }
            JIGSAW_BOX => {
                if v < 0.16 {
                    return if logo {
                        Color::srgb(0.95, 0.2, 0.15)
                    } else {
                        Color::WHITE
                    };
                }
                let (pu, pv) = ((u - 0.06) / 0.88, (v - 0.18) / 0.68);
                if !(0.0..1.0).contains(&pu) || !(0.0..1.0).contains(&pv) {
                    return Color::WHITE;
                }
                // Cut into pieces.
                if (pu * 4.0).fract() < 0.05 || (pv * 4.0).fract() < 0.05 {
                    return Color::srgb(0.3, 0.32, 0.3);
                }
                let hill = 0.55 + 0.08 * (pu * 7.0).sin();
                if pv > hill + 0.15 {
                    Color::srgb(0.25, 0.55, 0.22)
                } else if pv > hill {
                    if (0.2..0.38).contains(&pu) && pv < hill + 0.12 {
                        Color::srgb(0.85, 0.2, 0.15)
                    } else {
                        Color::srgb(0.4, 0.75, 0.3)
                    }
                } else if Vec2::new(pu - 0.75, (pv - 0.25) * 0.75).length() < 0.1 {
                    Color::srgb(1.0, 0.85, 0.2)
                } else {
                    Color::srgb(0.55, 0.78, 0.98)
                }
            }
            GAME_BOX => {
                if v < 0.2 {
                    return if logo {
                        Color::srgb(0.95, 0.2, 0.15)
                    } else {
                        Color::srgb(1.0, 0.82, 0.2)
                    };
                }
                let (bu, bv) = ((u - 0.12) / 0.76, (v - 0.28) / 0.52);
                if !(0.0..1.0).contains(&bu) || !(0.0..1.0).contains(&bv) {
                    return plain;
                }
                if (bu - 0.5).abs() < 0.1 || (bv - 0.5).abs() < 0.1 {
                    return Color::WHITE;
                }
                match (bu < 0.5, bv < 0.5) {
                    (true, true) => Color::srgb(0.9, 0.15, 0.12),
                    (false, true) => Color::srgb(0.15, 0.45, 0.92),
                    (true, false) => Color::srgb(1.0, 0.8, 0.1),
                    (false, false) => Color::srgb(0.2, 0.7, 0.3),
                }
            }
            CARTON => {
                let ink = Color::srgb(0.22, 0.15, 0.09);
                // The tape down its middle, at the top.
                if v < 0.14 && (u - 0.5).abs() < 0.08 {
                    return Color::srgb(0.85, 0.72, 0.52);
                }
                // The arrows for the way up, in the top corners.
                for corner in [0.17, 0.83] {
                    let (au, av) = (u - corner, v - 0.12);
                    if ((-0.08..-0.02).contains(&av) && au.abs() < av + 0.08)
                        || (au.abs() < 0.015 && (-0.02..0.05).contains(&av))
                    {
                        return ink;
                    }
                }
                // A ride-on car: its body, and its wheels as rings.
                let body = (u - 0.5).abs() < 0.36 && (0.42..0.62).contains(&v)
                    || (u - 0.45).abs() < 0.2 && (0.32..0.44).contains(&v);
                let wheel = |c: f32| Vec2::new(u - c, (v - 0.68) * 0.67).length();
                let rim = [0.27, 0.73]
                    .iter()
                    .any(|&c| (0.05..0.08).contains(&wheel(c)));
                let outline = body
                    && !((u - 0.5).abs() < 0.33 && (0.445..0.6).contains(&v))
                    && !((u - 0.45).abs() < 0.17 && (0.345..0.44).contains(&v));
                if outline || rim {
                    return ink;
                }
                plain
            }
            WHITE_BOX => {
                let grey = Color::srgb(0.62, 0.62, 0.64);
                if v < 0.2 {
                    return if logo { grey } else { Color::WHITE };
                }
                if u < 0.14 {
                    return Color::srgb(0.82, 0.82, 0.84);
                }
                let (wu, wv) = ((u - 0.2) / 0.72, (v - 0.27) / 0.6);
                if !(0.0..1.0).contains(&wu) || !(0.0..1.0).contains(&wv) {
                    return plain;
                }
                // A robot: a square head on a square body, arms and legs.
                let robot = ((wu - 0.5).abs() < 0.13 && (0.08..0.28).contains(&wv))
                    || ((wu - 0.5).abs() < 0.22 && (0.3..0.65).contains(&wv))
                    || ((wu - 0.5).abs() > 0.26
                        && (wu - 0.5).abs() < 0.34
                        && (0.32..0.6).contains(&wv))
                    || ((wu - 0.5).abs() > 0.05
                        && (wu - 0.5).abs() < 0.17
                        && (0.65..0.95).contains(&wv));
                if robot {
                    Color::srgb(0.45, 0.45, 0.48)
                } else {
                    Color::srgb(0.88, 0.9, 0.93)
                }
            }
            SPACE_BOX => {
                if v < 0.18 {
                    return if logo {
                        Color::srgb(1.0, 0.82, 0.15)
                    } else {
                        plain
                    };
                }
                let (ru, rv) = (u - 0.5, v - 0.52);
                // The rocket: a white body, a red nose and fins, a flame.
                if ru.abs() < 0.1 && (-0.2..0.16).contains(&rv) {
                    return Color::WHITE;
                }
                if (-0.3..-0.2).contains(&rv) && ru.abs() < (rv + 0.3) {
                    return Color::srgb(0.9, 0.15, 0.1);
                }
                if (0.06..0.2).contains(&rv) && ru.abs() < 0.1 + (rv - 0.06) * 0.9 {
                    return Color::srgb(0.9, 0.15, 0.1);
                }
                if (0.16..0.3).contains(&rv) && ru.abs() < 0.06 - (rv - 0.16) * 0.35 {
                    return Color::srgb(1.0, 0.6, 0.1);
                }
                if random01(x * 7919 + y * 104_729) > 0.975 {
                    return Color::WHITE;
                }
                plain
            }
            DOTTED_BOX => {
                if v < 0.2 {
                    return if logo {
                        Color::srgb(0.95, 0.2, 0.15)
                    } else {
                        Color::WHITE
                    };
                }
                // In rows, every other a half step along; the cells about
                // square, 64 / 5 by 96 / 8 pixels.
                let row = (v * 8.0).floor();
                let cell = Vec2::new((u * 5.0 + row * 0.5).fract() - 0.5, (v * 8.0).fract() - 0.5);
                if cell.length() < 0.25 {
                    Color::WHITE
                } else {
                    plain
                }
            }
            _ => {
                if v < 0.2 {
                    return if logo { accent } else { Color::WHITE };
                }
                if u < 0.14 {
                    return accent;
                }
                // The window, with the toy in it against a pale backing.
                let (wu, wv) = ((u - 0.2) / 0.72, (v - 0.27) / 0.62);
                if (0.0..1.0).contains(&wu) && (0.0..1.0).contains(&wv) {
                    let toy = match index % 3 {
                        // A figure: a round head over a body.
                        0 => {
                            Vec2::new(wu - 0.5, (wv - 0.28) * 1.2).length() < 0.17
                                || ((wu - 0.5).abs() < 0.2 && (0.45..0.92).contains(&wv))
                        }
                        // A car: a body on two wheels.
                        1 => {
                            ((wu - 0.5).abs() < 0.4 && (0.45..0.68).contains(&wv))
                                || ((wu - 0.5).abs() < 0.22 && (0.3..0.5).contains(&wv))
                                || Vec2::new((wu - 0.25) * 1.0, wv - 0.72).length() < 0.1
                                || Vec2::new((wu - 0.75) * 1.0, wv - 0.72).length() < 0.1
                        }
                        // Bricks, stacked.
                        _ => wv > 0.25 && (wu * 3.0).fract() > 0.08 && (wv * 4.0).fract() > 0.1,
                    };
                    return if toy {
                        accent.mix(&Color::BLACK, 0.15)
                    } else {
                        Color::srgb(0.92, 0.94, 0.98)
                    };
                }
                plain
            }
        }
    })
}

/// A green baseplate of plastic bricks: a grid of round studs, four by
/// four a repetition, each lit on top, with a darker ring of shadow round
/// its foot.
fn baseplate() -> Image {
    const SIZE: u32 = 128;
    const STUDS: u32 = 4;
    let green = Color::srgb(0.16, 0.56, 0.22);
    tiling(SIZE, SIZE, |x, y| {
        let cell = SIZE / STUDS;
        let p = Vec2::new((x % cell) as f32, (y % cell) as f32) / cell as f32 - Vec2::splat(0.5);
        let r = p.length();
        if r < 0.3 {
            // Lit from the top left.
            let lit = 1.0 + 0.5 * (-p.x - p.y).clamp(-0.3, 0.3);
            return Color::LinearRgba(green.to_linear() * lit);
        }
        if r < 0.36 {
            return green.mix(&Color::BLACK, 0.3);
        }
        green
    })
}

/// A mat of foam tiles, two by two a repetition, red, blue, yellow and
/// green, fitting together with the round tabs of a jigsaw, a seam between
/// them.
fn foam_mat() -> Image {
    const SIZE: u32 = 256;
    let colors = [
        Color::srgb(0.9, 0.2, 0.18),
        Color::srgb(0.18, 0.42, 0.88),
        Color::srgb(1.0, 0.8, 0.12),
        Color::srgb(0.2, 0.68, 0.3),
    ];
    let half = SIZE as f32 / 2.0;
    tiling(SIZE, SIZE, |x, y| {
        let p = Vec2::new(x as f32, y as f32);
        // Which tile: a round tab of the tile to the left and of the one
        // above sticks into each, in the middle of its edge.
        let tile = (p / half).floor();
        let local = p - tile * half;
        let tab = half * 0.12;
        let tabs = [Vec2::new(0.0, half * 0.5), Vec2::new(half * 0.5, 0.0)];
        let owner = match tabs.iter().position(|centre| local.distance(*centre) < tab) {
            Some(0) => tile - Vec2::X,
            Some(_) => tile - Vec2::Y,
            None => tile,
        };
        let index = (owner.x.rem_euclid(2.0) + 2.0 * owner.y.rem_euclid(2.0)) as usize;
        let color = colors[index % colors.len()];
        // The seam: round a tab (its own or its neighbours'), its outline;
        // elsewhere the tile's edge.
        let near = [
            tabs[0],
            tabs[1],
            Vec2::new(half, half * 0.5),
            Vec2::new(half * 0.5, half),
        ]
        .iter()
        .map(|centre| local.distance(*centre))
        .fold(f32::MAX, f32::min);
        let seam = if near < tab + 2.0 {
            (near - tab).abs()
        } else {
            local.x.min(local.y).min(half - local.x).min(half - local.y)
        };
        if seam < 1.5 {
            return color.mix(&Color::BLACK, 0.35);
        }
        let grain = 0.95 + 0.05 * value_noise(p * 0.5);
        color.mix(&Color::BLACK, 1.0 - grain)
    })
}

/// A red stripe and a white one, side by side across the texture, with a
/// thin grey line between them: a slot-car track's borders.
fn stripes() -> Image {
    const WIDTH: u32 = 64;
    tiling(WIDTH, 4, |x, _| {
        let at = x % (WIDTH / 2);
        if at < 1 {
            Color::srgb(0.5, 0.5, 0.5)
        } else if x < WIDTH / 2 {
            Color::srgb(0.88, 0.08, 0.06)
        } else {
            Color::srgb(0.96, 0.96, 0.94)
        }
    })
}

/// The faces of toy blocks: sixteen in a row, each a letter of its own
/// in a contrasting colour on a painted face with a lighter bevelled
/// border, as wooden alphabet blocks.
fn block_faces() -> Image {
    const FACE: u32 = 32;
    const LETTERS: &str = "ABCDEFGHIJKLMNOP";
    let paints = [
        Color::srgb(0.9, 0.15, 0.12),
        Color::srgb(0.15, 0.4, 0.9),
        Color::srgb(1.0, 0.78, 0.1),
        Color::srgb(0.2, 0.68, 0.28),
        Color::srgb(0.95, 0.45, 0.08),
        Color::srgb(0.6, 0.25, 0.8),
    ];
    let glyphs: Vec<[u8; 7]> = LETTERS.chars().map(super::lettering::glyph).collect();
    tiling(FACE * 16, FACE, |x, y| {
        let index = (x / FACE) as usize;
        let paint = paints[index % paints.len()];
        let ink = paints[(index + 2) % paints.len()];
        let (px, py) = (x % FACE, y);
        let border = px.min(FACE - 1 - px).min(py.min(FACE - 1 - py));
        if border < 2 {
            return paint.mix(&Color::WHITE, 0.35);
        }
        // The letter, 5 × 7 pixels of the font at 4 px each, centred.
        let (gx, gy) = (px as i32 - 6, py as i32 - 2);
        if (0..20).contains(&gx) && (0..28).contains(&gy) {
            let row = glyphs[index][(gy / 4) as usize];
            if row & (16 >> (gx / 4)) != 0 {
                return ink.mix(&Color::WHITE, 0.15);
            }
        }
        paint
    })
}

/// An sRGB texture of `width` × `height` pixels that repeats both ways, with
/// its mipmaps, from the colour of each pixel.
fn tiling(width: u32, height: u32, pixel: impl Fn(u32, u32) -> Color) -> Image {
    let mut image = tiling_level(width, height, pixel);
    generate_mipmaps(&mut image);
    image
}

/// [`tiling`]'s first level alone, opaque, without its mipmaps.
fn tiling_level(width: u32, height: u32, pixel: impl Fn(u32, u32) -> Color) -> Image {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    let data = (0..width * height)
        .flat_map(|i| {
            let color = pixel(i % width, i / width).to_srgba();
            [byte(color.red), byte(color.green), byte(color.blue), 255]
        })
        .collect();
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = sampler(ImageAddressMode::Repeat);
    image
}

/// Checkerboard earth: two by two squares, chocolate and caramel, each with a
/// light bevel on its upper and left edges, a dark one on the others, and
/// some grain.
fn soil() -> Image {
    const SIZE: u32 = 256;
    const HALF: u32 = SIZE / 2;
    const BEVEL: u32 = 7;
    let dark = Color::srgb(0.42, 0.21, 0.07);
    let light = Color::srgb(0.80, 0.46, 0.16);
    tiling(SIZE, SIZE, |x, y| {
        let (px, py) = (x % HALF, y % HALF);
        let base = if (x < HALF) == (y < HALF) {
            light
        } else {
            dark
        };
        let point = Vec2::new(x as f32, y as f32);
        let grain = 0.08 * (fbm(point / 9.0, 3) - 0.5) + 0.05 * (value_noise(point / 2.0) - 0.5);
        let shade = if px < BEVEL || py < BEVEL {
            1.18
        } else if px >= HALF - BEVEL || py >= HALF - BEVEL {
            0.72
        } else {
            1.0
        };
        Color::LinearRgba(base.to_linear() * (shade + grain))
    })
}

/// Falling water: blue with white streaks of foam running down, which
/// repeat seamlessly in both directions.
fn falls() -> Image {
    const WIDTH: u32 = 128;
    const HEIGHT: u32 = 256;
    let deep = Color::srgb(0.12, 0.42, 0.78).to_linear();
    let foam = Color::srgb(0.92, 0.97, 1.0).to_linear();
    tiling(WIDTH, HEIGHT, |x, y| {
        let u = x as f32 / WIDTH as f32 * std::f32::consts::TAU;
        let v = y as f32 / HEIGHT as f32 * std::f32::consts::TAU;
        // Streaks: thin across, long down, wavering a little.
        let waver = 0.4 * (v * 2.0 + u * 3.0).sin();
        let streaks = 0.5
            + 0.25 * (u * 9.0 + waver).sin()
            + 0.15 * (u * 23.0 + 1.3 + (v * 3.0).sin()).sin()
            + 0.1 * (u * 5.0 - v * 2.0).sin();
        let t = ((streaks - 0.45) * 2.4).clamp(0.0, 1.0);
        Color::LinearRgba(deep.mix(&foam, t * t))
    })
}

/// How much of the planks' texture down it each board takes (with the gap
/// before it), and the gap (see [`planks`]).
pub const PLANK_BOARD: f32 = 1.0 / 8.0;
pub const PLANK_GAP: f32 = 2.0 / 128.0;

/// Wooden planks laid across: eight boards, each of its own tone, with
/// grain along them and gaps between them, dark and see-through (alpha 0):
/// a material that cuts its texture out (`AlphaMode::Mask`), as the wooden
/// bridges' decks, shows what is under them through the gaps; the rest
/// draw them dark.
fn planks() -> Image {
    const SIZE: u32 = 128;
    const BOARDS: u32 = (1.0 / PLANK_BOARD) as u32;
    let wood = Color::srgb(0.58, 0.36, 0.18).to_linear();
    let gap = |y: u32| (y as f32 / SIZE as f32).rem_euclid(PLANK_BOARD) < PLANK_GAP;
    let mut image = tiling_level(SIZE, SIZE, |x, y| {
        let board = y * BOARDS / SIZE;
        if gap(y) {
            return Color::srgb(0.16, 0.09, 0.04);
        }
        let tone = 0.82 + 0.3 * random01(board + 17);
        let point = Vec2::new(x as f32 / 14.0, y as f32 * 1.6 + board as f32 * 31.0);
        let grain = 0.85 + 0.3 * value_noise(point);
        Color::LinearRgba(wood * (tone * grain))
    });
    let data = image.data.as_mut().expect("its pixels");
    for (i, pixel) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        if gap(i as u32 / SIZE) {
            pixel[3] = 0;
        }
    }
    generate_mipmaps(&mut image);
    image
}

/// Layers of sandstone down the texture: bands of cream, salmon, rust and
/// ochre, each of its own thickness, wavering a little across, with thin
/// dark seams between some of them and grain. It repeats both ways.
fn strata() -> Image {
    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 256;
    const BANDS: u32 = 11;
    let colors = [
        Color::srgb(0.95, 0.82, 0.64),
        Color::srgb(0.9, 0.58, 0.42),
        Color::srgb(0.72, 0.33, 0.2),
        Color::srgb(0.88, 0.62, 0.34),
        Color::srgb(0.8, 0.45, 0.3),
    ]
    .map(|color| color.to_linear());
    // Where each band starts down the texture (0..1), at irregular steps.
    let mut starts = [0.0; BANDS as usize];
    let mut total = 0.0;
    for (band, start) in starts.iter_mut().enumerate() {
        *start = total;
        total += 0.4 + random01(band as u32 + 91);
    }
    tiling(WIDTH, HEIGHT, |x, y| {
        let across = x as f32 / WIDTH as f32 * std::f32::consts::TAU;
        let waver = 0.012 * (across + 0.7).sin() + 0.006 * (3.0 * across).sin();
        let down = (y as f32 / HEIGHT as f32 + waver).rem_euclid(1.0) * total;
        let band = starts.iter().rposition(|&start| start <= down).unwrap_or(0);
        let next = starts.get(band + 1).copied().unwrap_or(total);
        let seam = random01(band as u32 + 7) < 0.45 && next - down < 0.035;
        let color = colors[(band * 3 + 1) % colors.len()];
        let grain = 0.86 + 0.24 * random01(x * 7919 + y * 104_729);
        let shade = if seam { 0.45 } else { grain };
        Color::LinearRgba(color * shade)
    })
}

/// Público pixelado para las gradas: filas de espectadores con ropa de colores vivos
/// como en los juegos de carreras arcade de los 90.
fn crowd() -> Image {
    const WIDTH: u32 = 128;
    const HEIGHT: u32 = 64;
    const SHIRTS: [Color; 6] = [
        Color::srgb(0.92, 0.15, 0.12),
        Color::srgb(0.98, 0.85, 0.15),
        Color::srgb(0.12, 0.45, 0.88),
        Color::srgb(0.95, 0.95, 0.95),
        Color::srgb(0.95, 0.50, 0.10),
        Color::srgb(0.15, 0.72, 0.28),
    ];
    let data = (0..WIDTH * HEIGHT)
        .flat_map(|i| {
            let (x, y) = (i % WIDTH, i / HEIGHT);
            let row = y / 16;
            let row_y = y % 16;
            let col = x / 8;
            let col_x = x % 8;
            let person_seed = (row * 37 + col * 13) as f32;
            let shirt_idx =
                ((person_seed * 1.7).sin().abs() * SHIRTS.len() as f32) as usize % SHIRTS.len();
            let shirt = SHIRTS[shirt_idx];
            let skin = Color::srgb(0.95, 0.78, 0.65);
            let hair = if (person_seed * 2.3).sin() > 0.0 {
                Color::srgb(0.25, 0.18, 0.12)
            } else {
                Color::srgb(0.55, 0.40, 0.20)
            };
            let bench = Color::srgb(0.55, 0.55, 0.58);
            let c = if row_y < 3 {
                bench
            } else if row_y < 7 && (2..=5).contains(&col_x) {
                if row_y < 5 { hair } else { skin }
            } else if (7..14).contains(&row_y) && (1..=6).contains(&col_x) {
                shirt
            } else {
                bench
            };
            let srgba = c.to_srgba();
            [
                (srgba.red * 255.0).round() as u8,
                (srgba.green * 255.0).round() as u8,
                (srgba.blue * 255.0).round() as u8,
                255,
            ]
        })
        .collect();
    let mut image = Image::new(
        Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = sampler(ImageAddressMode::Repeat);
    generate_mipmaps(&mut image);
    image
}

/// Añade a una imagen RGBA de 8 bits todos sus niveles de mipmap, cada uno la
/// mitad de grande que el anterior, promediando en espacio lineal: las de
/// color (sRGB) se pasan a lineal para promediar; las de datos (normales,
/// rugosidad) ya lo están. Also to those of data of two channels or one, 8
/// bits each (the normal maps of [`super::relief`]).
pub fn generate_mipmaps(image: &mut Image) {
    let (srgb, channels) = match image.texture_descriptor.format {
        TextureFormat::Rgba8UnormSrgb => (true, 4),
        TextureFormat::Rgba8Unorm => (false, 4),
        TextureFormat::Rg8Unorm => (false, 2),
        TextureFormat::R8Unorm => (false, 1),
        other => panic!("solo se generan mipmaps de imágenes de 8 bits, no {other:?}"),
    };
    let to_linear: Vec<f32> = (0..=255u8)
        .map(|v| {
            if srgb {
                Color::srgb_u8(v, 0, 0).to_linear().red
            } else {
                v as f32 / 255.0
            }
        })
        .collect();
    let to_srgb = |linear: f32| {
        let value = if srgb {
            Color::linear_rgb(linear, 0.0, 0.0).to_srgba().red
        } else {
            linear
        };
        (value * 255.0).round() as u8
    };

    let (mut width, mut height) = (image.width(), image.height());
    let mut data = image.data.take().expect("la imagen tiene datos en la CPU");
    let mut level = data.clone();
    let mut levels = 1;
    while width > 1 || height > 1 {
        let (next_width, next_height) = ((width / 2).max(1), (height / 2).max(1));
        let mut next = Vec::with_capacity((next_width * next_height * channels) as usize);
        for y in 0..next_height {
            for x in 0..next_width {
                for channel in 0..channels {
                    let mut sum = 0.0;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((2 * x + dx).min(width - 1), (2 * y + dy).min(height - 1));
                        let value = level[((sy * width + sx) * channels + channel) as usize];
                        sum += if channel < 3 {
                            to_linear[value as usize]
                        } else {
                            value as f32 / 255.0
                        };
                    }
                    let average = sum / 4.0;
                    next.push(if channel < 3 {
                        to_srgb(average)
                    } else {
                        (average * 255.0).round() as u8
                    });
                }
            }
        }
        data.extend_from_slice(&next);
        level = next;
        (width, height) = (next_width, next_height);
        levels += 1;
    }
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn mipmaps_go_down_to_one_pixel() {
        let mut image = checker();
        let levels = image.texture_descriptor.mip_level_count;
        assert_eq!(levels, 7, "64, 32, 16, 8, 4, 2 y 1 píxeles");
        let expected: u32 = (0..levels).map(|l| (64 >> l) * (64 >> l) * 4).sum();
        assert_eq!(image.data.as_ref().unwrap().len() as u32, expected);
        // El último nivel es la media: gris, no negro ni blanco.
        let last = &image.data.take().unwrap()[expected as usize - 4..];
        assert!(last[0] > 100 && last[0] < 220);
    }

    /// The colour of a pixel of the first mip level.
    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let width = image.texture_descriptor.size.width;
        let at = ((y * width + x) * 4) as usize;
        let data = image.data.as_ref().expect("the texture has its pixels");
        [data[at], data[at + 1], data[at + 2], data[at + 3]]
    }

    #[test]
    fn the_planks_are_solid_with_see_through_gaps_between_them() {
        let image = planks();
        // Eight boards down the texture, each 16 px: its first two rows the
        // gap before it.
        for y in 0..128 {
            let alpha = pixel(&image, 40, y)[3];
            if y % 16 < 2 {
                assert_eq!(alpha, 0, "a gap at {y}");
            } else {
                assert_eq!(alpha, 255, "a board at {y}");
            }
        }
    }

    #[test]
    fn checkerboard_earth_alternates_two_browns() {
        let soil = soil();
        let (light, dark) = (pixel(&soil, 64, 64), pixel(&soil, 192, 64));
        // Brown: more red than green, more green than blue.
        for brown in [light, dark] {
            assert!(brown[0] > brown[1] && brown[1] > brown[2], "{brown:?}");
        }
        assert!(light[0] > dark[0] + 40, "{light:?} {dark:?}");
        // Diagonally, the same square again.
        assert_eq!(pixel(&soil, 192, 192)[0] / 16, light[0] / 16);
    }

    #[test]
    fn falling_water_repeats_without_seams() {
        let falls = falls();
        let (width, height) = (
            falls.texture_descriptor.size.width,
            falls.texture_descriptor.size.height,
        );
        let step = |a: [u8; 4], b: [u8; 4]| (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap();
        // Across each seam, no bigger a step than between neighbours inside.
        for x in (0..width).step_by(7) {
            let inside = (1..height)
                .map(|y| step(pixel(&falls, x, y - 1), pixel(&falls, x, y)))
                .max()
                .unwrap();
            assert!(step(pixel(&falls, x, height - 1), pixel(&falls, x, 0)) <= inside);
        }
        for y in (0..height).step_by(7) {
            let inside = (1..width)
                .map(|x| step(pixel(&falls, x - 1, y), pixel(&falls, x, y)))
                .max()
                .unwrap();
            assert!(step(pixel(&falls, width - 1, y), pixel(&falls, 0, y)) <= inside);
        }
    }

    #[test]
    fn embedded_textures_decode() {
        let image = embedded(
            crate::content::file("textures/asphalt.jpg"),
            ImageAddressMode::Repeat,
            TextureEnhance::Asphalt,
        );
        assert!(image.texture_descriptor.mip_level_count > 1);
    }
}
