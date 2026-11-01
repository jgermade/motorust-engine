//! Procedural textures of plants, painted at startup like the other
//! procedural textures: the lawn the grass is drawn with, tufts of blades
//! standing up beside the road, the ragged edge of the grass along it, and
//! clumps of flowers.
//!
//! The ones with see-through gaps (tufts, the edge, the flowers) are drawn
//! with alpha masking: their mipmaps keep as much of them solid as the
//! full-size image has (see [`keep_coverage`]), so the grass does not thin
//! out and vanish in the distance.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::scenery::FlowerKind;
use super::textures::generate_mipmaps;
use crate::geometry::{fbm, random01, smoothstep};

/// The alpha below which masked textures are see-through.
pub const ALPHA_CUTOFF: f32 = 0.5;

/// A canvas of linear RGB colours with straight alpha, painted with
/// anti-aliased strokes. If it wraps, what goes past one edge comes back
/// in at the other, so the texture repeats without seams. If not, only
/// what is inside its clip (the corners of a rectangle of pixels: all of
/// it, unless narrowed) is painted.
struct Canvas {
    width: u32,
    height: u32,
    wrap: bool,
    clip: (IVec2, IVec2),
    pixels: Vec<Vec4>,
}

impl Canvas {
    /// A canvas all of `color`, as opaque as `alpha`.
    fn new(width: u32, height: u32, wrap: bool, color: Color, alpha: f32) -> Self {
        let linear = color.to_linear();
        let pixel = Vec4::new(linear.red, linear.green, linear.blue, alpha);
        Self {
            width,
            height,
            wrap,
            clip: (IVec2::ZERO, IVec2::new(width as i32, height as i32)),
            pixels: vec![pixel; (width * height) as usize],
        }
    }

    /// Lays `color` over pixel (x, y) with this much `coverage` (0..1).
    fn blend(&mut self, x: i32, y: i32, color: Vec4, coverage: f32) {
        let (w, h) = (self.width as i32, self.height as i32);
        let (x, y) = if self.wrap {
            (x.rem_euclid(w), y.rem_euclid(h))
        } else if x < self.clip.0.x || y < self.clip.0.y || x >= self.clip.1.x || y >= self.clip.1.y
        {
            return;
        } else {
            (x, y)
        };
        let under = &mut self.pixels[(y * w + x) as usize];
        let a = color.w * coverage.clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let out = a + under.w * (1.0 - a);
        let rgb = if under.w <= 0.0 {
            color.truncate()
        } else {
            (color.truncate() * a + under.truncate() * under.w * (1.0 - a)) / out
        };
        *under = rgb.extend(out);
    }

    /// A stroke from `from` to `to` (px), `width` wide at its start and
    /// `end_width` at its end, of colour `start` turning to `end` along it.
    fn stroke(
        &mut self,
        from: Vec2,
        to: Vec2,
        (width, end_width): (f32, f32),
        start: Vec4,
        end: Vec4,
    ) {
        let reach = width.max(end_width) * 0.5 + 1.0;
        let (min, max) = (from.min(to) - reach, from.max(to) + reach);
        let along = to - from;
        let length2 = along.length_squared().max(1e-6);
        for y in min.y.floor() as i32..=max.y.ceil() as i32 {
            for x in min.x.floor() as i32..=max.x.ceil() as i32 {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let t = ((p - from).dot(along) / length2).clamp(0.0, 1.0);
                let distance = p.distance(from + along * t);
                let radius = width.lerp(end_width, t) * 0.5;
                let coverage = radius - distance + 0.5;
                if coverage > 0.0 {
                    self.blend(x, y, start.lerp(end, t), coverage);
                }
            }
        }
    }

    /// The canvas as an sRGB image with mipmaps; masked ones keep their
    /// coverage down the mipmaps.
    fn image(self, masked: bool) -> Image {
        let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        let data = self
            .pixels
            .iter()
            .flat_map(|p| {
                let srgb = Color::linear_rgb(p.x, p.y, p.z).to_srgba();
                [byte(srgb.red), byte(srgb.green), byte(srgb.blue), byte(p.w)]
            })
            .collect();
        let mut image = Image::new(
            Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        let address = if self.wrap {
            ImageAddressMode::Repeat
        } else {
            ImageAddressMode::ClampToEdge
        };
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: address,
            address_mode_v: address,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            // Masked cards are many and overlap: cheaper filtering for them.
            anisotropy_clamp: if masked { 4 } else { 16 },
            ..default()
        });
        generate_mipmaps(&mut image);
        if masked {
            keep_coverage(&mut image, ALPHA_CUTOFF);
        }
        image
    }
}

/// Raises the alpha of each mipmap of an alpha-masked image so as much of
/// it is solid (at least `cutoff`) as of the full-size image. Averaging
/// alone thins out the leaves and blades as they get smaller, until they
/// vanish.
pub fn keep_coverage(image: &mut Image, cutoff: f32) {
    let levels = image.texture_descriptor.mip_level_count;
    let (width, height) = (image.width() as usize, image.height() as usize);
    let Some(data) = image.data.as_mut() else {
        return;
    };
    let cut = (cutoff * 255.0).round() as u32;
    let coverage = |pixels: &[u8], scale: f32| {
        let solid = pixels
            .chunks(4)
            .filter(|p| ((p[3] as f32 * scale).round() as u32) >= cut)
            .count();
        solid as f32 / (pixels.len() / 4) as f32
    };
    let wanted = coverage(&data[..width * height * 4], 1.0);
    let (mut offset, mut w, mut h) = (width * height * 4, width, height);
    for _ in 1..levels {
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
        let size = w * h * 4;
        let level = &mut data[offset..offset + size];
        // The scale that makes as much of it solid, found by halving.
        let (mut low, mut high) = (0.0f32, 4.0f32);
        for _ in 0..16 {
            let middle = (low + high) / 2.0;
            if coverage(level, middle) < wanted {
                low = middle;
            } else {
                high = middle;
            }
        }
        for pixel in level.chunks_mut(4) {
            pixel[3] = (pixel[3] as f32 * high).round().min(255.0) as u8;
        }
        offset += size;
    }
}

/// A random number from 0 to 1 for stroke `n`, number `k` of its numbers.
fn rand(seed: u32, n: u32, k: u32) -> f32 {
    random01(seed.wrapping_mul(7919) ^ n.wrapping_mul(16) ^ k)
}

/// Lawn: a mat of blades of grass seen from above, of several greens, with
/// a few dry ones, over the shade between them, which is the `photo` (the
/// grass photograph, darkened). It repeats both ways. Its average colour is
/// the photo's, so the grass keeps its tone and only gains detail.
pub fn lawn(photo: &Image) -> Image {
    let (width, height) = (photo.width(), photo.height());
    let Some(source) = photo.data.as_ref() else {
        return photo.clone();
    };
    let photo_at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        Color::srgb_u8(source[i], source[i + 1], source[i + 2]).to_linear()
    };
    let mut canvas = Canvas::new(width, height, true, Color::BLACK, 1.0);
    for y in 0..height {
        for x in 0..width {
            let p = photo_at(x, y);
            canvas.pixels[(y * width + x) as usize] =
                Vec4::new(p.red, p.green, p.blue, 1.0) * Vec4::new(0.55, 0.55, 0.55, 1.0);
        }
    }
    let greens = [
        Color::srgb(0.24, 0.5, 0.1),
        Color::srgb(0.33, 0.6, 0.13),
        Color::srgb(0.42, 0.68, 0.17),
        Color::srgb(0.2, 0.42, 0.08),
        Color::srgb(0.5, 0.66, 0.2),
    ];
    let dry = Color::srgb(0.66, 0.62, 0.34);
    let scale = width as f32 / 512.0;
    let blades = (24_000.0 * scale * scale) as u32;
    for n in 0..blades {
        let r = |k: u32| rand(1, n, k);
        let root = Vec2::new(r(0) * width as f32, r(1) * height as f32);
        // Clumps lean the same way, as grass does where it is trodden or
        // the wind combs it.
        let lean = fbm(root / (60.0 * scale), 2) * std::f32::consts::TAU * 2.0;
        let angle = lean + (r(2) - 0.5) * 2.4;
        let length = (5.0 + 9.0 * r(3)) * scale;
        let tip = root + Vec2::from_angle(angle) * length;
        let color = if r(4) < 0.03 {
            dry
        } else {
            greens[(r(5) * greens.len() as f32) as usize % greens.len()]
        }
        .to_linear();
        let base =
            Vec4::new(color.red, color.green, color.blue, 0.9) * Vec4::new(0.7, 0.7, 0.7, 1.0);
        let lit =
            Vec4::new(color.red, color.green, color.blue, 0.9) * Vec4::new(1.2, 1.2, 1.15, 1.0);
        canvas.stroke(root, tip, ((1.6 + r(6)) * scale, 0.5 * scale), base, lit);
    }
    // The same average colour as the photo.
    let average = |pixels: &mut dyn Iterator<Item = Vec3>| {
        let (sum, count) = pixels.fold((Vec3::ZERO, 0.0), |(s, c), p| (s + p, c + 1.0));
        sum / count
    };
    let target = average(&mut (0..height).flat_map(|y| {
        (0..width).map(move |x| {
            let i = ((y * width + x) * 4) as usize;
            let p = Color::srgb_u8(source[i], source[i + 1], source[i + 2]).to_linear();
            Vec3::new(p.red, p.green, p.blue)
        })
    }));
    let painted = average(&mut canvas.pixels.iter().map(|p| p.truncate()));
    let correction = target / painted.max(Vec3::splat(1e-4));
    for pixel in &mut canvas.pixels {
        *pixel = (pixel.truncate() * correction).extend(1.0);
    }
    canvas.image(false)
}

/// A tuft of grass seen from the side: blades rising from the bottom edge,
/// curving out and over, darker at their roots, on a clear ground. Pale,
/// to be tinted by the colour of the grass they grow in.
pub fn tuft() -> Image {
    const WIDTH: u32 = 256;
    const HEIGHT: u32 = 128;
    let base = Color::srgb(0.8, 0.8, 0.75);
    let mut canvas = Canvas::new(WIDTH, HEIGHT, false, base, 0.0);
    for n in 0..70 {
        let r = |k: u32| rand(2, n, k);
        let x = 20.0 + r(0) * (WIDTH as f32 - 40.0);
        // Taller in the middle of the tuft.
        let middle = 1.0 - ((x / WIDTH as f32) * 2.0 - 1.0).abs();
        let height = HEIGHT as f32 * (0.35 + 0.6 * middle * (0.6 + 0.4 * r(1)));
        let lean = (r(2) - 0.5) * 1.2 + ((x / WIDTH as f32) - 0.5) * 0.9;
        let bend = (r(3) - 0.5) * 0.8;
        let shade = 0.65 + 0.35 * r(4);
        // A few dry, straw-coloured blades among the green.
        let green = if r(5) < 0.06 {
            Vec4::new(1.0, 0.85, 0.5, 1.0)
        } else {
            Vec4::new(1.0, 1.0, 0.95, 1.0)
        };
        let root = green * Vec4::new(0.45, 0.45, 0.45, 1.0);
        let tip = green * Vec4::new(shade, shade, shade, 1.0);
        let width = 3.0 + 3.0 * r(6);
        // A curve of four strokes, tapering to a point.
        let mut at = Vec2::new(x, HEIGHT as f32 + 2.0);
        let mut angle = -std::f32::consts::FRAC_PI_2 + lean * 0.4;
        for k in 0..4 {
            let t0 = k as f32 / 4.0;
            let t1 = (k + 1) as f32 / 4.0;
            angle += (lean + bend * t0) * 0.25;
            let next = at + Vec2::from_angle(angle) * height / 4.0;
            canvas.stroke(
                at,
                next,
                (width * (1.0 - t0 * 0.85), width * (1.0 - t1 * 0.95)),
                root.lerp(tip, t0),
                root.lerp(tip, t1),
            );
            at = next;
        }
    }
    canvas.image(true)
}

/// The ragged edge of the grass along the road, seen from above, to lie
/// over the edge of the asphalt: clear on the road's side (the top rows),
/// grass on the other (the bottom ones), and between them an uneven line
/// of grass with blades and clumps reaching out over the road. Towards the
/// bottom the grass thins out in specks, so it melts into the grass of the
/// ground instead of ending on a line. Pale, to be tinted by the colour of
/// the grass. It repeats along (u) but not across (v).
pub fn grass_edge() -> Image {
    const WIDTH: u32 = 256;
    const HEIGHT: u32 = 64;
    let pale = Color::srgb(0.85, 0.85, 0.82);
    let mut canvas = Canvas::new(WIDTH, HEIGHT, true, pale, 0.0);
    // Where the grass begins down each column: an uneven line that repeats
    // along.
    let phases: Vec<f32> = (0..8)
        .map(|k| rand(7, k, 0) * std::f32::consts::TAU)
        .collect();
    let line = |u: f32| {
        let mut wave = 0.0;
        for (k, phase) in phases.iter().enumerate() {
            let frequency = (k + 1) as f32;
            wave += (u * std::f32::consts::TAU * frequency + phase).sin() / frequency.sqrt();
        }
        0.42 + 0.05 * wave
    };
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (u, v) = (
                (x as f32 + 0.5) / WIDTH as f32,
                (y as f32 + 0.5) / HEIGHT as f32,
            );
            let into = (v - line(u)) * HEIGHT as f32;
            // Thinning out into specks towards the bottom.
            let specks = rand(8, x * HEIGHT + y, 0);
            let thinning = smoothstep(0.75, 0.98, v);
            if into > 0.0 && specks > thinning {
                let shade = 0.78 + 0.22 * rand(9, x * HEIGHT + y, 0);
                let color = Vec4::new(shade, shade, shade * 0.97, 1.0);
                canvas.blend(x as i32, y as i32, color, into.min(1.0));
            }
        }
    }
    // Blades and little clumps reaching out over the road.
    for n in 0..520 {
        let r = |k: u32| rand(10, n, k);
        let x = r(0) * WIDTH as f32;
        let root = Vec2::new(x, (line(x / WIDTH as f32) + 0.04 * r(1)) * HEIGHT as f32);
        let angle = -std::f32::consts::FRAC_PI_2 + (r(2) - 0.5) * 1.8;
        let length = HEIGHT as f32 * (0.06 + 0.22 * r(3) * r(3));
        let tip = root + Vec2::from_angle(angle) * length;
        let shade = 0.72 + 0.28 * r(4);
        let color = Vec4::new(shade, shade, shade * 0.95, 1.0);
        canvas.stroke(
            root,
            tip,
            (2.2 + 1.5 * r(5), 0.6),
            color * Vec4::new(0.85, 0.85, 0.85, 1.0),
            color,
        );
    }
    let mut image = canvas.image(true);
    // Across it does not repeat: the clear road side must not take in the
    // grass of the other side.
    if let ImageSampler::Descriptor(descriptor) = &mut image.sampler {
        descriptor.address_mode_v = ImageAddressMode::ClampToEdge;
    }
    image
}

/// The cells of the flowers' atlas ([`flowers`]): how many pixels each is
/// across, and how many there are across and down.
const FLOWER_CELL: u32 = 256;
const FLOWER_COLUMNS: u32 = 4;
const FLOWER_ROWS: u32 = 3;
/// How far from the edges of its cell each clump keeps (px), so that the
/// smaller mipmaps do not mix in the cells beside it.
const FLOWER_MARGIN: f32 = 6.0;
/// Where the ground is in a cell seen from the side (px down from its
/// top): the stems rise from just above its bottom edge.
const FLOWER_GROUND: f32 = FLOWER_CELL as f32 - 2.0;
/// Every kind of flower, in the order of their cells in the atlas and of
/// their meadows (see [`meadow`]).
pub const FLOWER_KINDS: [FlowerKind; 8] = [
    FlowerKind::SpiderLily,
    FlowerKind::White,
    FlowerKind::Yellow,
    FlowerKind::Iris,
    FlowerKind::Susuki,
    FlowerKind::Phlox,
    FlowerKind::BareEarth,
    FlowerKind::FallenLeaves,
];

/// Whether `kind` hugs the ground, a carpet with no clumps standing up out
/// of it (and no cells in the flowers' atlas): moss phlox, bare earth and
/// fallen leaves.
pub fn hugs_the_ground(kind: FlowerKind) -> bool {
    matches!(
        kind,
        FlowerKind::Phlox | FlowerKind::BareEarth | FlowerKind::FallenLeaves
    )
}

/// The rectangle (its top left and bottom right corners, in texture
/// coordinates) of the cell of the flowers' atlas ([`flowers`]) where a
/// clump of `kind` is painted seen from the side, rising from its bottom
/// edge, or, `from_above`, seen from above; a texel in from its edges.
pub fn flower_cell(kind: FlowerKind, from_above: bool) -> (Vec2, Vec2) {
    let (left, top) = flower_cell_origin(kind, from_above);
    let size = Vec2::new(
        (FLOWER_COLUMNS * FLOWER_CELL) as f32,
        (FLOWER_ROWS * FLOWER_CELL) as f32,
    );
    let min = Vec2::new(left, top) + Vec2::ONE;
    let max = Vec2::new(left, top) + Vec2::splat(FLOWER_CELL as f32 - 1.0);
    (min / size, max / size)
}

/// The top left corner of a cell of the flowers' atlas (px).
fn flower_cell_origin(kind: FlowerKind, from_above: bool) -> (f32, f32) {
    let index = FLOWER_KINDS
        .iter()
        .position(|&k| k == kind)
        .expect("every kind has its cells") as u32
        * 2
        + from_above as u32;
    (
        ((index % FLOWER_COLUMNS) * FLOWER_CELL) as f32,
        ((index / FLOWER_COLUMNS) * FLOWER_CELL) as f32,
    )
}

/// Clumps of flowers, a pair of cells of an atlas for each kind (see
/// [`flower_cell`]): seen from the side, stems and leaves rising from the
/// bottom of the cell to the flowers; seen from above, the flowers open
/// over their leaves. Clear round them, and drawn with alpha masking. In
/// their own colours, not tinted.
pub fn flowers() -> Image {
    let mut canvas = Canvas::new(
        FLOWER_COLUMNS * FLOWER_CELL,
        FLOWER_ROWS * FLOWER_CELL,
        false,
        Color::srgb(0.3, 0.42, 0.2),
        0.0,
    );
    // Each clump is kept inside its cell, clear of its edges.
    let clip = |(x, y): (f32, f32)| {
        let corner = IVec2::new(x as i32, y as i32);
        let margin = IVec2::splat(FLOWER_MARGIN as i32);
        (
            corner + margin,
            corner + IVec2::splat(FLOWER_CELL as i32) - margin,
        )
    };
    // Moss phlox, bare earth and fallen leaves have no clumps: they hug the
    // ground (see [`meadow`]).
    let standing = FLOWER_KINDS
        .into_iter()
        .filter(|&kind| !hugs_the_ground(kind));
    for (seed, kind) in standing.enumerate() {
        let seed = 40 + seed as u32 * 2;
        let side = flower_cell_origin(kind, false);
        canvas.clip = clip(side);
        // Down to the bottom edge: the stems rise from it.
        canvas.clip.1.y += FLOWER_MARGIN as i32;
        paint_side(&mut canvas, kind, Vec2::new(side.0, side.1), seed);
        let above = flower_cell_origin(kind, true);
        canvas.clip = clip(above);
        paint_from_above(&mut canvas, kind, Vec2::new(above.0, above.1), seed + 1);
    }
    canvas.image(true)
}

/// The lawn (see [`lawn`]) in flower, as seen from above: covered in
/// blossoms of `kind`, so thick that a field of them is a carpet of their
/// colour from afar. It repeats both ways, as the lawn does. Moss phlox is
/// painted pale, a carpet of tiny flowers: the terrain colours it, in
/// bands (see [`super::flowers::phlox_color`]).
pub fn meadow(lawn: &Image, kind: FlowerKind) -> Image {
    let (width, height) = (lawn.width(), lawn.height());
    let mut canvas = Canvas::new(width, height, true, Color::BLACK, 1.0);
    if let Some(source) = lawn.data.as_ref() {
        for (pixel, texel) in canvas.pixels.iter_mut().zip(source.chunks(4)) {
            let color = Color::srgb_u8(texel[0], texel[1], texel[2]).to_linear();
            // A little darker under the flowers, between their stems.
            *pixel =
                Vec4::new(color.red, color.green, color.blue, 1.0) * Vec4::new(0.8, 0.8, 0.8, 1.0);
        }
    }
    // Sizes are in pixels for a 512-pixel lawn, which is GRASS_TILE wide.
    let scale = width as f32 / 512.0;
    let seed = 70 + FLOWER_KINDS.iter().position(|&k| k == kind).unwrap_or(0) as u32;
    let spot = |r: &dyn Fn(u32) -> f32| Vec2::new(r(0) * width as f32, r(1) * height as f32);
    let count = |n: f32| (n * scale * scale) as u32;
    let tau = std::f32::consts::TAU;
    match kind {
        FlowerKind::SpiderLily => {
            let red = paint(0.86, 0.07, 0.05);
            let light = paint(0.98, 0.24, 0.15);
            for n in 0..count(1500.0) {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(&r);
                let turn = r(2) * tau;
                for k in 0..6 {
                    let a = turn + k as f32 / 6.0 * tau;
                    let petal = Vec2::from_angle(a) * 6.0 * scale;
                    canvas.stroke(at, at + petal, (3.4 * scale, 1.4 * scale), red, light);
                    let stamen = Vec2::from_angle(a + 0.5) * 10.0 * scale;
                    canvas.stroke(at, at + stamen, (1.3 * scale, 0.8 * scale), red, light);
                }
            }
        }
        FlowerKind::White => {
            let white = paint(0.97, 0.97, 0.94);
            let shade = paint(0.84, 0.86, 0.84);
            let heart = paint(0.98, 0.78, 0.15);
            for n in 0..count(1700.0) {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(&r);
                let petals = shade.lerp(white, r(3));
                for k in 0..8 {
                    let a = r(2) + k as f32 / 8.0 * tau;
                    canvas.stroke(
                        at,
                        at + Vec2::from_angle(a) * 4.5 * scale,
                        (4.0 * scale, 3.4 * scale),
                        petals,
                        white,
                    );
                }
                dot(&mut canvas, at, 3.5 * scale, heart);
            }
        }
        FlowerKind::Yellow => {
            let yellow = paint(1.0, 0.84, 0.08);
            let light = paint(1.0, 0.95, 0.45);
            for n in 0..count(950.0) {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(&r);
                for k in 0..12 {
                    let q = |j: u32| rand(seed, n * 16 + 5000 + k, j);
                    let offset = Vec2::from_angle(q(0) * tau) * 9.0 * scale * q(1).sqrt();
                    dot(
                        &mut canvas,
                        at + offset,
                        (3.5 + 1.5 * q(2)) * scale,
                        yellow.lerp(light, q(3)),
                    );
                }
            }
        }
        FlowerKind::Iris => {
            let purple = paint(0.42, 0.2, 0.72);
            let light = paint(0.62, 0.45, 0.9);
            let signal = paint(1.0, 0.85, 0.2);
            let leaf = paint(0.24, 0.46, 0.14);
            for n in 0..count(900.0) {
                let r = |k: u32| rand(seed, n + 20_000, k);
                let at = spot(&r);
                let to = at + Vec2::from_angle(r(2) * tau) * 18.0 * scale;
                canvas.stroke(at, to, (3.0 * scale, 0.8 * scale), leaf, leaf);
            }
            for n in 0..count(1000.0) {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(&r);
                let turn = r(2) * tau;
                for k in 0..3 {
                    let a = turn + k as f32 / 3.0 * tau;
                    canvas.stroke(
                        at,
                        at + Vec2::from_angle(a) * 7.5 * scale,
                        (6.0 * scale, 3.5 * scale),
                        purple,
                        light,
                    );
                    canvas.stroke(
                        at,
                        at + Vec2::from_angle(a) * 3.0 * scale,
                        (1.5 * scale, 1.2 * scale),
                        signal,
                        signal,
                    );
                    let standard = Vec2::from_angle(a + tau / 6.0) * 4.5 * scale;
                    canvas.stroke(at, at + standard, (4.0 * scale, 2.5 * scale), purple, light);
                }
            }
        }
        FlowerKind::Susuki => {
            let blade_gold = paint(0.78, 0.7, 0.4);
            let straw = paint(0.66, 0.56, 0.32);
            let plume = paint(0.96, 0.9, 0.76);
            let plume_dark = paint(0.84, 0.72, 0.52);
            for n in 0..count(3200.0) {
                let r = |k: u32| rand(seed, n + 20_000, k);
                let at = spot(&r);
                // Blades combed the same way by the wind, in waves.
                let a = fbm(at / (90.0 * scale), 2) * tau + (r(2) - 0.5) * 0.8;
                let to = at + Vec2::from_angle(a) * (10.0 + 8.0 * r(3)) * scale;
                canvas.stroke(at, to, (2.2 * scale, 0.6 * scale), straw, blade_gold);
            }
            for n in 0..count(1400.0) {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(&r);
                let a = fbm(at / (90.0 * scale), 2) * tau;
                for k in 0..7 {
                    let q = |j: u32| rand(seed, n * 8 + 9000 + k, j);
                    let to =
                        at + Vec2::from_angle(a + (q(0) - 0.5) * 1.1) * (11.0 + 7.0 * q(1)) * scale;
                    canvas.stroke(
                        at,
                        to,
                        (2.2 * scale, 0.7 * scale),
                        plume_dark.lerp(plume, q(2)),
                        plume,
                    );
                }
            }
        }
        FlowerKind::Phlox => {
            let petal = paint(1.0, 0.94, 0.97);
            let eye = paint(0.82, 0.55, 0.78);
            for n in 0..count(9400.0) {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(&r);
                let turn = r(2) * tau;
                let shade = Vec4::new(0.85, 0.85, 0.85, 1.0).lerp(Vec4::ONE, r(3));
                for k in 0..5 {
                    let a = turn + k as f32 / 5.0 * tau;
                    dot(
                        &mut canvas,
                        at + Vec2::from_angle(a) * 1.8 * scale,
                        2.6 * scale,
                        petal * shade,
                    );
                }
                dot(&mut canvas, at, 1.4 * scale, eye);
            }
        }
        FlowerKind::BareEarth => {
            // The earth over most of it, in blotches of its browns, dusty
            // and darker; pebbles in it; and tufts of grass left standing.
            let earths = [
                paint(0.36, 0.27, 0.19),
                paint(0.46, 0.35, 0.24),
                paint(0.55, 0.45, 0.33),
                paint(0.5, 0.34, 0.23),
            ];
            for n in 0..count(5200.0) {
                let r = |k: u32| rand(seed, n, k);
                let earth = earths[(r(2) * 4.0) as usize % 4];
                let shade = 0.85 + 0.3 * r(3);
                dot(
                    &mut canvas,
                    spot(&r),
                    (4.0 + 9.0 * r(4)) * scale,
                    earth * shade,
                );
            }
            let stone = paint(0.62, 0.6, 0.56);
            for n in 0..count(900.0) {
                let r = |k: u32| rand(seed, n + 30_000, k);
                dot(
                    &mut canvas,
                    spot(&r),
                    (1.4 + 2.2 * r(2)) * scale,
                    stone * (0.75 + 0.35 * r(3)),
                );
            }
            let blade = paint(0.3, 0.44, 0.17);
            let tip = paint(0.5, 0.6, 0.26);
            for n in 0..count(650.0) {
                let r = |k: u32| rand(seed, n + 60_000, k);
                let at = spot(&r);
                for k in 0..6 {
                    let q = |j: u32| rand(seed, n * 8 + 90_000 + k, j);
                    let to = at + Vec2::from_angle(q(0) * tau) * (4.0 + 7.0 * q(1)) * scale;
                    canvas.stroke(at, to, (1.8 * scale, 0.6 * scale), blade, tip);
                }
            }
        }
        FlowerKind::FallenLeaves => {
            // Leaves lying every way over the grass, in autumn's colours, a
            // darker vein down each, so thick that the grass shows only
            // here and there between them.
            let colours = [
                paint(0.45, 0.3, 0.16),
                paint(0.62, 0.3, 0.12),
                paint(0.76, 0.55, 0.2),
                paint(0.86, 0.68, 0.26),
                paint(0.62, 0.17, 0.1),
                paint(0.32, 0.21, 0.12),
            ];
            for n in 0..count(18_000.0) {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(&r);
                let colour = colours[(r(2) * 6.0) as usize % 6] * (0.8 + 0.35 * r(3));
                let half = Vec2::from_angle(r(4) * tau) * (3.5 + 3.5 * r(5)) * scale;
                let width = (2.6 + 2.0 * r(6)) * scale;
                canvas.stroke(at - half, at + half, (width, width * 0.4), colour, colour);
                let vein = colour * 0.7;
                canvas.stroke(
                    at - half,
                    at + half * 0.8,
                    (0.7 * scale, 0.4 * scale),
                    vein,
                    vein,
                );
            }
        }
    }
    canvas.image(false)
}

/// A colour given in sRGB, as the canvas paints it (linear, opaque).
fn paint(r: f32, g: f32, b: f32) -> Vec4 {
    let linear = Color::srgb(r, g, b).to_linear();
    Vec4::new(linear.red, linear.green, linear.blue, 1.0)
}

/// A stroke through `points` (px), tapering from `width` to `end_width`,
/// of colour `start` turning to `end`.
fn bent_stroke(
    canvas: &mut Canvas,
    points: &[Vec2],
    (width, end_width): (f32, f32),
    start: Vec4,
    end: Vec4,
) {
    let count = (points.len() - 1) as f32;
    for (k, pair) in points.windows(2).enumerate() {
        let (t0, t1) = (k as f32 / count, (k + 1) as f32 / count);
        canvas.stroke(
            pair[0],
            pair[1],
            (width.lerp(end_width, t0), width.lerp(end_width, t1)),
            start.lerp(end, t0),
            start.lerp(end, t1),
        );
    }
}

/// A blade or a stem from `root` up `height` px, leaning `lean` px over at
/// its top and curving out on the way.
fn blade(root: Vec2, height: f32, lean: f32, curve: f32) -> [Vec2; 4] {
    let at = |t: f32| root + Vec2::new(lean * t * t + curve * t * (1.0 - t), -height * t);
    [at(0.0), at(0.35), at(0.7), at(1.0)]
}

/// A disc of `color` (`width` px across) at `at`.
fn dot(canvas: &mut Canvas, at: Vec2, width: f32, color: Vec4) {
    canvas.stroke(at, at, (width, width), color, color);
}

/// A clump of `kind` seen from the side in the cell at `origin` (px).
fn paint_side(canvas: &mut Canvas, kind: FlowerKind, origin: Vec2, seed: u32) {
    let size = FLOWER_CELL as f32;
    let ground = origin.y + FLOWER_GROUND;
    let across = |r: f32| origin.x + FLOWER_MARGIN + 24.0 + r * (size - 2.0 * FLOWER_MARGIN - 48.0);
    // Leaning over, `amount` px at most, those by the edges of the cell
    // into it.
    let lean = |x: f32, r: f32, amount: f32| (r - (x - origin.x) / size) * amount;
    let root_green = paint(0.16, 0.3, 0.08);
    let leaf_green = paint(0.36, 0.58, 0.2);
    match kind {
        FlowerKind::SpiderLily => {
            let red = paint(0.86, 0.07, 0.05);
            let light = paint(0.98, 0.22, 0.14);
            for n in 0..10 {
                let r = |k: u32| rand(seed, n, k);
                let x = across(r(0));
                let height = size * (0.5 + 0.32 * r(1));
                let stem = blade(Vec2::new(x, ground), height, lean(x, r(2), 24.0), 0.0);
                bent_stroke(canvas, &stem, (4.0, 3.0), root_green, leaf_green);
                let head = stem[3];
                // Six petals curling back, and longer stamens curving up and
                // out past them, all round the head.
                for k in 0..6 {
                    let a = (k as f32 / 6.0 + r(3)) * std::f32::consts::TAU;
                    let (out, depth) = (a.cos(), a.sin());
                    let tip = head + Vec2::new(out * 15.0, 3.0 + depth * 4.0);
                    bent_stroke(
                        canvas,
                        &[head, tip, tip + Vec2::new(out * 4.0, -7.0)],
                        (5.0, 1.5),
                        red,
                        light,
                    );
                    let middle = head + Vec2::new(out * 13.0, -12.0 + depth * 2.0);
                    bent_stroke(
                        canvas,
                        &[head, middle, middle + Vec2::new(out * 11.0, -10.0)],
                        (1.8, 1.0),
                        red,
                        light,
                    );
                }
            }
        }
        FlowerKind::White => {
            let white = paint(0.97, 0.97, 0.94);
            let shade = paint(0.8, 0.82, 0.8);
            let heart = paint(0.98, 0.78, 0.15);
            for n in 0..22 {
                let r = |k: u32| rand(seed, n, k);
                let x = across(r(0));
                let leaf = blade(
                    Vec2::new(x, ground),
                    size * (0.12 + 0.18 * r(1)),
                    lean(x, r(2), 50.0),
                    (r(3) - 0.5) * 20.0,
                );
                bent_stroke(canvas, &leaf, (7.0, 1.0), root_green, leaf_green);
            }
            for n in 0..15 {
                let r = |k: u32| rand(seed, n + 100, k);
                let x = across(r(0));
                let stem = blade(
                    Vec2::new(x, ground),
                    size * (0.35 + 0.42 * r(1)),
                    lean(x, r(2), 30.0),
                    (r(3) - 0.5) * 16.0,
                );
                bent_stroke(canvas, &stem, (3.0, 2.0), root_green, leaf_green);
                // The flower turned up a little: a flat oval of petals, and
                // its heart on top.
                let head = stem[3];
                let tilt = (r(4) - 0.5) * 4.0;
                canvas.stroke(
                    head - Vec2::new(10.0, tilt),
                    head + Vec2::new(10.0, tilt),
                    (9.0, 9.0),
                    shade.lerp(white, r(5)),
                    white,
                );
                dot(canvas, head - Vec2::new(0.0, 3.0), 6.0, heart);
            }
        }
        FlowerKind::Yellow => {
            let yellow = paint(1.0, 0.84, 0.08);
            let light = paint(1.0, 0.95, 0.45);
            let bud = paint(0.7, 0.75, 0.2);
            for n in 0..10 {
                let r = |k: u32| rand(seed, n, k);
                let x = across(r(0));
                let leaf = blade(
                    Vec2::new(x, ground),
                    size * (0.18 + 0.15 * r(1)),
                    lean(x, r(2), 60.0),
                    (r(3) - 0.5) * 24.0,
                );
                bent_stroke(canvas, &leaf, (11.0, 2.0), root_green, leaf_green);
            }
            for n in 0..9 {
                let r = |k: u32| rand(seed, n + 100, k);
                let x = across(r(0));
                let height = size * (0.55 + 0.33 * r(1));
                let stem = blade(Vec2::new(x, ground), height, lean(x, r(2), 20.0), 0.0);
                bent_stroke(canvas, &stem, (4.0, 3.0), root_green, leaf_green);
                // A branch either side, and a cluster of blossoms at the
                // end of each.
                let mut ends = vec![stem[3]];
                for (k, side) in [-1.0, 1.0].into_iter().enumerate() {
                    let from = stem[2];
                    let to = from + Vec2::new(side * (14.0 + 10.0 * r(3 + k as u32)), -30.0);
                    canvas.stroke(from, to, (2.5, 2.0), leaf_green, leaf_green);
                    ends.push(to);
                }
                for (e, end) in ends.into_iter().enumerate() {
                    for k in 0..14 {
                        let q = |j: u32| rand(seed, n * 16 + e as u32 * 1000 + 300, k * 4 + j);
                        let at = end + Vec2::new((q(0) - 0.5) * 24.0, -q(1) * 18.0 + 4.0);
                        let color = if q(2) < 0.12 {
                            bud
                        } else {
                            yellow.lerp(light, q(3))
                        };
                        dot(canvas, at, 5.0 + 3.0 * q(3), color);
                    }
                }
            }
        }
        FlowerKind::Iris => {
            let purple = paint(0.42, 0.2, 0.72);
            let light = paint(0.62, 0.45, 0.9);
            let signal = paint(1.0, 0.85, 0.2);
            for n in 0..14 {
                let r = |k: u32| rand(seed, n, k);
                let x = across(r(0));
                let leaf = blade(
                    Vec2::new(x, ground),
                    size * (0.5 + 0.38 * r(1)),
                    lean(x, r(2), 30.0),
                    (r(3) - 0.5) * 10.0,
                );
                bent_stroke(canvas, &leaf, (8.0, 1.0), root_green, leaf_green);
            }
            for n in 0..7 {
                let r = |k: u32| rand(seed, n + 100, k);
                let x = across(r(0));
                let stem = blade(
                    Vec2::new(x, ground),
                    size * (0.58 + 0.3 * r(1)),
                    lean(x, r(2), 16.0),
                    0.0,
                );
                bent_stroke(canvas, &stem, (3.5, 3.0), root_green, leaf_green);
                let head = stem[3];
                // Three falls hanging out and down, three standards up
                // between them, a yellow mark at the root of each fall.
                for side in [-1.0, 1.0, 0.15] {
                    let tip = head + Vec2::new(side * 15.0, 12.0);
                    bent_stroke(
                        canvas,
                        &[head, head + Vec2::new(side * 9.0, 2.0), tip],
                        (10.0, 6.0),
                        purple,
                        light,
                    );
                    canvas.stroke(
                        head,
                        head + Vec2::new(side * 6.0, 3.0),
                        (2.5, 2.0),
                        signal,
                        signal,
                    );
                }
                for side in [-0.5, 0.5, 0.0] {
                    canvas.stroke(
                        head,
                        head + Vec2::new(side * 9.0, -15.0),
                        (7.0, 3.0),
                        purple,
                        light,
                    );
                }
            }
        }
        FlowerKind::Susuki => {
            let blade_green = paint(0.52, 0.56, 0.28);
            let blade_gold = paint(0.78, 0.72, 0.42);
            let straw = paint(0.72, 0.62, 0.4);
            let plume = paint(0.96, 0.9, 0.76);
            let plume_dark = paint(0.82, 0.7, 0.52);
            for n in 0..44 {
                let r = |k: u32| rand(seed, n, k);
                let x = across(r(0));
                let middle = 1.0 - ((x - origin.x) / size * 2.0 - 1.0).abs();
                let leaf = blade(
                    Vec2::new(x, ground),
                    size * (0.3 + 0.4 * middle * (0.6 + 0.4 * r(1))),
                    lean(x, r(2), 120.0),
                    (r(3) - 0.5) * 30.0,
                );
                bent_stroke(canvas, &leaf, (4.0, 0.6), blade_green, blade_gold);
            }
            for n in 0..9 {
                let r = |k: u32| rand(seed, n + 100, k);
                // Rooted to the left: the plumes bow to the right.
                let x =
                    origin.x + FLOWER_MARGIN + 16.0 + r(0) * (size - 2.0 * FLOWER_MARGIN - 110.0);
                let stem = blade(
                    Vec2::new(x, ground),
                    size * (0.72 + 0.22 * r(1)),
                    20.0 + r(2) * 20.0,
                    (r(3) - 0.5) * 10.0,
                );
                bent_stroke(canvas, &stem, (2.5, 2.0), straw, straw);
                // The plume: fine strands from the top of the stem, all
                // bowing the same way, as in the wind.
                let top = stem[3];
                for k in 0..14 {
                    let q = |j: u32| rand(seed, n * 20 + 500 + k, j);
                    let from = top + Vec2::new(0.0, k as f32 * 2.5);
                    let a = 0.35 + 0.9 * q(0);
                    let length = 26.0 + 20.0 * q(1);
                    let to = from + Vec2::new(a.cos(), a.sin() * 0.8) * length;
                    bent_stroke(
                        canvas,
                        &[from, from.lerp(to, 0.5) - Vec2::new(0.0, 3.0), to],
                        (2.6, 0.8),
                        plume_dark.lerp(plume, q(2)),
                        plume,
                    );
                }
            }
        }
        FlowerKind::Phlox | FlowerKind::BareEarth | FlowerKind::FallenLeaves => {}
    }
}

/// A clump of `kind` seen from above in the cell at `origin` (px).
fn paint_from_above(canvas: &mut Canvas, kind: FlowerKind, origin: Vec2, seed: u32) {
    let size = FLOWER_CELL as f32;
    let inside = FLOWER_MARGIN + 28.0;
    let spot =
        |a: f32, b: f32| origin + Vec2::splat(inside) + Vec2::new(a, b) * (size - 2.0 * inside);
    let leaf_dark = paint(0.18, 0.34, 0.1);
    let leaf_green = paint(0.36, 0.58, 0.2);
    let leaves = |canvas: &mut Canvas, count: u32, length: f32, width: f32| {
        for n in 0..count {
            let r = |k: u32| rand(seed, n + 900, k);
            let from = spot(r(0), r(1));
            let to =
                from + Vec2::from_angle(r(2) * std::f32::consts::TAU) * length * (0.6 + 0.4 * r(3));
            canvas.stroke(from, to, (width, 1.0), leaf_dark, leaf_green);
        }
    };
    let radial = |canvas: &mut Canvas,
                  at: Vec2,
                  count: u32,
                  turn: f32,
                  length: f32,
                  widths: (f32, f32),
                  color: Vec4,
                  tip: Vec4| {
        for k in 0..count {
            let a = turn + k as f32 / count as f32 * std::f32::consts::TAU;
            canvas.stroke(at, at + Vec2::from_angle(a) * length, widths, color, tip);
        }
    };
    match kind {
        FlowerKind::SpiderLily => {
            let red = paint(0.86, 0.07, 0.05);
            let light = paint(0.98, 0.22, 0.14);
            for n in 0..13 {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(r(0), r(1));
                let turn = r(2) * std::f32::consts::TAU;
                radial(canvas, at, 6, turn, 15.0, (5.0, 2.0), red, light);
                radial(canvas, at, 6, turn + 0.5, 25.0, (1.6, 1.0), red, light);
            }
        }
        FlowerKind::White => {
            let white = paint(0.97, 0.97, 0.94);
            let heart = paint(0.98, 0.78, 0.15);
            leaves(canvas, 16, 32.0, 8.0);
            for n in 0..22 {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(r(0), r(1));
                radial(canvas, at, 8, r(2), 9.0, (7.0, 6.0), white, white);
                dot(canvas, at, 7.0, heart);
            }
        }
        FlowerKind::Yellow => {
            let yellow = paint(1.0, 0.84, 0.08);
            let light = paint(1.0, 0.95, 0.45);
            leaves(canvas, 12, 36.0, 12.0);
            for n in 0..16 {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(r(0), r(1));
                for k in 0..14 {
                    let q = |j: u32| rand(seed, n * 20 + 200 + k, j);
                    let offset =
                        Vec2::from_angle(q(0) * std::f32::consts::TAU) * 16.0 * q(1).sqrt();
                    dot(
                        canvas,
                        at + offset,
                        6.0 + 2.0 * q(2),
                        yellow.lerp(light, q(3)),
                    );
                }
            }
        }
        FlowerKind::Iris => {
            let purple = paint(0.42, 0.2, 0.72);
            let light = paint(0.62, 0.45, 0.9);
            let signal = paint(1.0, 0.85, 0.2);
            leaves(canvas, 16, 60.0, 6.0);
            for n in 0..9 {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(r(0), r(1));
                let turn = r(2) * std::f32::consts::TAU;
                radial(canvas, at, 3, turn, 19.0, (11.0, 7.0), purple, light);
                radial(canvas, at, 3, turn, 7.0, (2.5, 2.0), signal, signal);
                radial(canvas, at, 3, turn + 1.05, 10.0, (7.0, 4.0), purple, light);
            }
        }
        FlowerKind::Susuki => {
            let blade_green = paint(0.52, 0.56, 0.28);
            let blade_gold = paint(0.78, 0.72, 0.42);
            let plume = paint(0.96, 0.9, 0.76);
            let plume_dark = paint(0.82, 0.7, 0.52);
            for n in 0..26 {
                let r = |k: u32| rand(seed, n + 900, k);
                let from = spot(r(0), r(1));
                let to = from
                    + Vec2::from_angle(r(2) * std::f32::consts::TAU) * 50.0 * (0.6 + 0.4 * r(3));
                canvas.stroke(from, to, (3.0, 0.6), blade_green, blade_gold);
            }
            // The plumes bow the same way, as the side's do.
            for n in 0..11 {
                let r = |k: u32| rand(seed, n, k);
                let at = spot(r(0), r(1));
                for k in 0..14 {
                    let q = |j: u32| rand(seed, n * 20 + 500 + k, j);
                    let a = (q(0) - 0.5) * 1.2;
                    let to = at + Vec2::from_angle(a) * (26.0 + 18.0 * q(1));
                    canvas.stroke(at, to, (2.6, 0.8), plume_dark.lerp(plume, q(2)), plume);
                }
            }
        }
        FlowerKind::Phlox | FlowerKind::BareEarth | FlowerKind::FallenLeaves => {}
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    fn alpha(image: &Image, x: u32, y: u32) -> u8 {
        let data = image.data.as_ref().expect("painted");
        data[((y * image.width() + x) * 4 + 3) as usize]
    }

    /// The share of the pixels of mipmap `level` of `image` that are solid.
    fn coverage_at(image: &Image, level: u32) -> f32 {
        let data = image.data.as_ref().expect("painted");
        let (mut offset, mut w, mut h) = (0, image.width() as usize, image.height() as usize);
        for _ in 0..level {
            offset += w * h * 4;
            (w, h) = ((w / 2).max(1), (h / 2).max(1));
        }
        let pixels = &data[offset..offset + w * h * 4];
        let cut = (ALPHA_CUTOFF * 255.0) as u8;
        pixels.chunks(4).filter(|p| p[3] >= cut).count() as f32 / (w * h) as f32
    }

    #[test]
    fn masked_mipmaps_keep_their_coverage() {
        let image = tuft();
        let full = coverage_at(&image, 0);
        for level in 1..6 {
            let smaller = coverage_at(&image, level);
            assert!(
                (smaller - full).abs() < 0.12,
                "level {level}: {smaller} vs {full}"
            );
        }
    }

    #[test]
    fn the_grass_edge_is_ragged() {
        let image = grass_edge();
        let (width, height) = (image.width(), image.height());
        // Clear along the top (the road), grass lower down.
        assert!((0..width).all(|x| alpha(&image, x, 0) == 0));
        let solid = |y: u32| (0..width).filter(|&x| alpha(&image, x, y) > 128).count();
        assert!(solid(height * 6 / 10) > width as usize * 9 / 10);
        // Where the grass begins goes up and down along it.
        let starts: Vec<u32> = (0..width)
            .filter_map(|x| (0..height).find(|&y| alpha(&image, x, y) > 128))
            .collect();
        let (top, bottom) = (starts.iter().min().unwrap(), starts.iter().max().unwrap());
        assert!(bottom - top > 8, "{top}..{bottom}");
        // It does not repeat across: the road side stays clear when drawn.
        let ImageSampler::Descriptor(sampler) = &image.sampler else {
            panic!("it has its sampler");
        };
        assert_eq!(sampler.address_mode_v, ImageAddressMode::ClampToEdge);
    }

    #[test]
    fn tufts_rise_from_the_bottom_edge() {
        let image = tuft();
        let (width, height) = (image.width(), image.height());
        assert_eq!(alpha(&image, 0, 0), 0);
        assert_eq!(alpha(&image, width - 1, 0), 0);
        let bottom = (0..width)
            .filter(|&x| alpha(&image, x, height - 1) > 128)
            .count();
        let top = (0..width).filter(|&x| alpha(&image, x, 4) > 128).count();
        assert!(bottom > top, "denser at the roots: {bottom} vs {top}");
        assert!(bottom > 0);
    }

    #[test]
    fn the_lawn_keeps_the_photo_s_colour_with_more_detail() {
        let photo = Image::new(
            Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            [90u8, 170, 50, 255].repeat(64 * 64),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        let image = lawn(&photo);
        let data = image.data.as_ref().expect("painted");
        let pixels = &data[..64 * 64 * 4];
        let mean = |c: usize| pixels.chunks(4).map(|p| p[c] as f32).sum::<f32>() / 4096.0;
        assert!((mean(1) - 170.0).abs() < 12.0, "green {}", mean(1));
        let spread = pixels.chunks(4).map(|p| p[1]).max().unwrap() as i32
            - pixels.chunks(4).map(|p| p[1]).min().unwrap() as i32;
        assert!(spread > 40, "blades show: {spread}");
        assert!(pixels.chunks(4).all(|p| p[3] == 255), "opaque");
    }
}
