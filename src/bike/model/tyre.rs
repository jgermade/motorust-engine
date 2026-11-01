//! Tyres for the imported bikes. The Meshy wheels bring their tyres as
//! lumpy, low-poly rings (their outline wanders by a couple of centimetres
//! and some have smudges of the bodywork's colours), so they are cut off the
//! wheel ([`strip`]) and replaced by a true surface of revolution ([`mesh`])
//! with the round profile of a MotoGP tyre. They are slicks, as MotoGP
//! races on in the dry: no tread pattern at all, the rubber a little lighter
//! where it is scrubbed, and the compound ([`Compound`]) marked by a band
//! of colour round the sidewall, painted at startup into a texture that
//! repeats round the tyre.

use std::f32::consts::{PI, TAU};

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::Design;
use crate::geometry::{random01, smoothstep, value_noise};
use crate::world::generate_mipmaps;

/// Segments round the tyre and round its cross-section.
const SEGMENTS: usize = 96;
const PROFILE: usize = 32;
/// How many times the texture repeats round the tyre.
const REPEATS: f32 = 18.0;
/// Where the new tyre meets the rim, and where the old one is cut off, as
/// fractions of the wheel's radius: the Meshy tyres are rings from about
/// 0.62 of the radius outwards, their rims end inside that.
const BEAD: f32 = 0.6;
const STRIP: f32 = 0.63;
/// Nor does any triangle reaching out beyond this: the spikes where the
/// Meshy rims meet their tyres.
const STRIP_REACH: f32 = 0.67;
/// The widest part of the old tyre is measured beyond this fraction of the
/// radius, and the new one is a little narrower than it.
const WIDTH_FROM: f32 = 0.7;
const WIDTH_SCALE: f32 = 0.95;
/// The cross-section is made of two half superellipses: an ellipse on the
/// crown, round from one shoulder to the other as a MotoGP tyre is (it
/// leans over 60 degrees on it), and squarer towards the rim, where the
/// tyre is as wide as the rim it covers.
const SQUARENESS: f32 = 2.0;
const BEAD_SQUARENESS: f32 = 4.0;
/// Half the angle of the cross-section covered by the tread, from the crown
/// (rad): nearly all the way round to the sidewalls, which are short.
const TREAD_ANGLE: f32 = 82.0 * PI / 180.0;
/// Where the tread band lies across the texture (v), centred on 0.5.
const TREAD_V: f32 = 0.3;
/// Size of the tyre's texture (px): one repeat round, the whole width
/// across.
const IMAGE: UVec2 = UVec2::new(128, 256);

/// The compound of a slick, marked as MotoGP marks it: a band round the
/// sidewall, white on the soft, none on the medium, yellow on the hard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub enum Compound {
    Soft,
    #[default]
    Medium,
    Hard,
}

impl Compound {
    pub const ALL: [Compound; 3] = [Compound::Soft, Compound::Medium, Compound::Hard];

    /// The tyres a bike wears: an imported one's own, the medium on the
    /// others.
    pub fn of(design: Design) -> Self {
        design.model().map_or(Compound::Medium, |model| model.tyres)
    }

    /// The colour of the band round the sidewalls (sRGB), if it has one.
    fn band(self) -> Option<[f32; 3]> {
        match self {
            Compound::Soft => Some([0.9, 0.9, 0.88]),
            Compound::Medium => None,
            Compound::Hard => Some([0.95, 0.78, 0.1]),
        }
    }
}

/// Cuts the tyre off a Meshy wheel `mesh`, in the bike's space, round
/// `axle` with `radius`: every triangle further from the axle than
/// [`STRIP`] of the radius goes. Also returns half the width of the tyre
/// that should take its place (m).
pub fn strip(mut mesh: Mesh, axle: Vec3, radius: f32) -> (Mesh, f32) {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return (mesh, 0.0);
    };
    let from_axle = |p: &[f32; 3]| Vec2::new(p[1] - axle.y, p[2] - axle.z).length();
    let half_width = positions
        .iter()
        .filter(|p| from_axle(p) > WIDTH_FROM * radius)
        .map(|p| (p[0] - axle.x).abs())
        .fold(0.0, f32::max)
        * WIDTH_SCALE;
    let radii: Vec<f32> = positions.iter().map(from_axle).collect();
    if let Some(Indices::U32(indices)) = mesh.indices() {
        let kept: Vec<u32> = indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|triangle| {
                let reach = triangle.iter().map(|&i| radii[i as usize]);
                let mean = reach.clone().sum::<f32>() / 3.0;
                mean <= STRIP * radius && reach.fold(0.0, f32::max) <= STRIP_REACH * radius
            })
            .flatten()
            .copied()
            .collect();
        mesh.insert_indices(Indices::U32(kept));
    }
    (mesh, half_width)
}

/// Where a point of the cross-section is, `alpha` radians round it from the
/// crown: across (-1 to 1, of the half width) and up (-1 to 1, of the half
/// height), and the outward normal for a section `half` metres big.
fn section(alpha: f32, half: Vec2) -> (Vec2, Vec2) {
    let (sin, cos) = alpha.sin_cos();
    let power = |v: f32, p: f32| v.signum() * v.abs().powf(p);
    let n = if cos >= 0.0 {
        SQUARENESS
    } else {
        BEAD_SQUARENESS
    };
    let point = Vec2::new(power(sin, 2.0 / n), power(cos, 2.0 / n));
    let normal = Vec2::new(
        power(point.x, n - 1.0) / half.x,
        power(point.y, n - 1.0) / half.y,
    );
    // At the very sides of the crown the gradient vanishes in one axis;
    // fall back to the direction of the point.
    let normal = normal.try_normalize().unwrap_or(point.normalize_or_zero());
    (point, normal)
}

/// The texture's `v` for a point `alpha` radians round the cross-section:
/// the tread in the middle band, the sidewalls on either side of it.
fn v_of(alpha: f32) -> f32 {
    let side = 0.5 - TREAD_V;
    if alpha.abs() <= TREAD_ANGLE {
        0.5 + TREAD_V * alpha / TREAD_ANGLE
    } else {
        let beyond = (alpha.abs() - TREAD_ANGLE) / (PI - TREAD_ANGLE);
        0.5 + alpha.signum() * (TREAD_V + side * beyond)
    }
}

/// The inverse of [`v_of`].
fn alpha_of(v: f32) -> f32 {
    let off = v - 0.5;
    if off.abs() <= TREAD_V {
        off / TREAD_V * TREAD_ANGLE
    } else {
        let beyond = (off.abs() - TREAD_V) / (0.5 - TREAD_V);
        off.signum() * (TREAD_ANGLE + beyond * (PI - TREAD_ANGLE))
    }
}

/// A tyre round the X axis at the origin: `radius` to the crown, `half_width`
/// at its widest, sitting on the rim at [`BEAD`] of the radius. Its texture
/// coordinates go [`REPEATS`] times round and once across (see [`v_of`]).
pub fn mesh(radius: f32, half_width: f32) -> Mesh {
    let bead = BEAD * radius;
    let half = Vec2::new(half_width, (radius - bead) / 2.0);
    let middle = (radius + bead) / 2.0;
    let mut positions = Vec::with_capacity((SEGMENTS + 1) * (PROFILE + 1));
    let mut normals = Vec::with_capacity(positions.capacity());
    let mut uvs = Vec::with_capacity(positions.capacity());
    for i in 0..=SEGMENTS {
        let angle = i as f32 / SEGMENTS as f32 * TAU;
        let (sin, cos) = angle.sin_cos();
        for j in 0..=PROFILE {
            let alpha = -PI + TAU * j as f32 / PROFILE as f32;
            let (point, normal) = section(alpha, half);
            let across = point.x * half.x;
            let out = middle + point.y * half.y;
            positions.push([across, out * cos, out * sin]);
            normals.push([normal.x, normal.y * cos, normal.y * sin]);
            uvs.push([i as f32 / SEGMENTS as f32 * REPEATS, v_of(alpha)]);
        }
    }
    let row = (PROFILE + 1) as u32;
    let mut indices = Vec::with_capacity(SEGMENTS * PROFILE * 6);
    for i in 0..SEGMENTS as u32 {
        for j in 0..PROFILE as u32 {
            // Counter-clockwise seen from outside, so that the outside is
            // the face that is drawn.
            let (a, b) = (i * row + j, (i + 1) * row + j);
            indices.extend([a, b, a + 1, b, b + 1, a + 1]);
        }
    }
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices));
    mesh.with_generated_tangents()
        .expect("the tyre has normals and texture coordinates")
}

/// The rubber's shades (sRGB): the tread, a little lighter where it is
/// scrubbed in the middle, and the sidewalls.
const TREAD_RUBBER: f32 = 0.13;
const SCRUBBED: f32 = 0.03;
const SIDEWALL_RUBBER: f32 = 0.09;
/// Where the band runs round the sidewalls (rad from the crown).
const BAND: (f32, f32) = (2.0, 2.12);

/// The colour texture of a slick of `compound`: smooth rubber with its own
/// grain, scrubbed on the tread, and the compound's band on the sidewalls.
pub fn image_of(compound: Compound) -> Image {
    let (width, height) = (IMAGE.x as usize, IMAGE.y as usize);
    let band = compound.band();
    let mut color = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let v = (y as f32 + 0.5) / height as f32;
        let alpha = alpha_of(v).abs();
        let t = (v - 0.5) / TREAD_V;
        for x in 0..width {
            // The rubber's own grain, so it does not look painted; the
            // noise repeats round the tyre with the texture.
            let point = Vec2::new(x as f32, y as f32) * 0.35;
            let round = Vec2::new(width as f32 * 0.35, 0.0);
            let along = x as f32 / width as f32;
            let noise = value_noise(point).lerp(value_noise(point - round), along);
            let grain = (noise - 0.5) * 0.02 + (random01((y * width + x) as u32) - 0.5) * 0.012;
            let rubber = if alpha <= TREAD_ANGLE {
                TREAD_RUBBER + SCRUBBED * (1.0 - smoothstep(0.3, 0.85, t.abs()))
            } else {
                SIDEWALL_RUBBER.lerp(
                    TREAD_RUBBER,
                    1.0 - smoothstep(0.0, 0.2, alpha - TREAD_ANGLE),
                )
            };
            let shade = rubber + grain;
            let rgb = match band {
                Some(band) => {
                    let on = smoothstep(BAND.0 - 0.02, BAND.0 + 0.02, alpha)
                        * (1.0 - smoothstep(BAND.1 - 0.02, BAND.1 + 0.02, alpha));
                    band.map(|c| shade.lerp(c, on))
                }
                None => [shade; 3],
            };
            color.extend(rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
            color.push(255);
        }
    }
    image(color, TextureFormat::Rgba8UnormSrgb)
}

/// An image of the texture's size that repeats round the tyre, with mipmaps.
fn image(data: Vec<u8>, format: TextureFormat) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: IMAGE.x,
            height: IMAGE.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    generate_mipmaps(&mut image);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    image
}

/// The material of a slick of `compound`: rubber with a satin sheen.
pub fn material(compound: Compound, images: &mut Assets<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(images.add(image_of(compound))),
        perceptual_roughness: 0.62,
        reflectance: 0.3,
        ..default()
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    #[test]
    fn every_face_of_the_tyre_is_drawn_from_outside() {
        let tyre = mesh(0.3, 0.07);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            tyre.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        let Some(VertexAttributeValues::Float32x3(normals)) =
            tyre.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("no normals");
        };
        let Some(Indices::U32(indices)) = tyre.indices() else {
            panic!("no indices");
        };
        // The front of each triangle (counter-clockwise, as drawn) faces the
        // way its corners' normals point: out of the tyre.
        for corners in indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|k| Vec3::from(positions[corners[k] as usize]));
            let front = (b - a).cross(c - a);
            if front.length() < 1e-9 {
                continue;
            }
            let normal: Vec3 = corners
                .iter()
                .map(|&k| Vec3::from(normals[k as usize]))
                .sum();
            assert!(front.dot(normal) > 0.0, "a face turned inwards at {a}");
        }
    }

    #[test]
    fn the_tyre_is_round_and_sits_on_its_rim() {
        let (radius, half_width) = (0.3, 0.07);
        let tyre = mesh(radius, half_width);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            tyre.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        // The crown of every segment round it is exactly at the radius.
        let per_segment = PROFILE + 1;
        for segment in positions.chunks(per_segment) {
            let outer = segment
                .iter()
                .map(|p| Vec2::new(p[1], p[2]).length())
                .fold(0.0, f32::max);
            assert!((outer - radius).abs() < 1e-4, "{outer}");
            let inner = segment
                .iter()
                .map(|p| Vec2::new(p[1], p[2]).length())
                .fold(f32::MAX, f32::min);
            assert!((inner - BEAD * radius).abs() < 1e-4, "{inner}");
        }
        let widest = positions.iter().map(|p| p[0].abs()).fold(0.0, f32::max);
        assert!((widest - half_width).abs() < 1e-4);
        assert!(tyre.attribute(Mesh::ATTRIBUTE_TANGENT).is_some());
    }

    #[test]
    fn normals_point_out_of_the_cross_section() {
        let half = Vec2::new(0.07, 0.06);
        for j in 0..PROFILE {
            let alpha = -PI + TAU * (j as f32 + 0.5) / PROFILE as f32;
            let (point, normal) = section(alpha, half);
            assert!(point.dot(normal) > 0.0, "{alpha}");
            assert!((normal.length() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn the_texture_maps_the_tread_to_the_middle_band() {
        for alpha in [-3.0, -1.5, -0.5, 0.0, 0.7, 1.4, 2.9] {
            assert!((alpha_of(v_of(alpha)) - alpha).abs() < 1e-4);
        }
        assert_eq!(v_of(0.0), 0.5);
        assert!((v_of(TREAD_ANGLE) - (0.5 + TREAD_V)).abs() < 1e-6);
        assert!((v_of(-PI)).abs() < 1e-6 && (v_of(PI) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn slicks_are_smooth_with_their_compound_on_the_sidewall() {
        for compound in Compound::ALL {
            let image = image_of(compound);
            let data = image.data.unwrap();
            let pixel = |x: u32, y: u32| {
                let i = ((y * IMAGE.x + x) * 4) as usize;
                [data[i], data[i + 1], data[i + 2]]
            };
            // No grooves: the tread is all rubber, none darker than it.
            let tread = (IMAGE.y / 2 - 40..IMAGE.y / 2 + 40)
                .flat_map(|y| (0..IMAGE.x).map(move |x| (x, y)))
                .map(|(x, y)| pixel(x, y)[0])
                .collect::<Vec<_>>();
            let (low, high) = (tread.iter().min().unwrap(), tread.iter().max().unwrap());
            assert!(high - low < 25, "{compound:?}: {low}..{high}");
            // The band, where the compound has one: white or yellow.
            let v = v_of((BAND.0 + BAND.1) / 2.0);
            let band = pixel(10, (v * IMAGE.y as f32) as u32);
            match compound {
                Compound::Soft => assert!(band.iter().all(|&c| c > 200), "{band:?}"),
                Compound::Hard => assert!(band[0] > 200 && band[2] < 80, "{band:?}"),
                Compound::Medium => assert!(band.iter().all(|&c| c < 60), "{band:?}"),
            }
        }
    }

    #[test]
    fn the_crown_is_round_from_shoulder_to_shoulder() {
        // Half way out to the shoulder the tread has dropped as much as on
        // an ellipse: round, not flat-topped.
        let half = Vec2::new(0.08, 0.06);
        let (top, _) = section(0.0, half);
        let (shoulder, normal) = section(PI / 4.0, half);
        assert!((top.y - 1.0).abs() < 1e-5);
        let ellipse = (1.0 - shoulder.x * shoulder.x).sqrt();
        assert!((shoulder.y - ellipse).abs() < 1e-3, "{shoulder:?}");
        // Leaning over 60 degrees, the bike is still on the tread.
        let lean = normal.x.atan2(normal.y);
        assert!(lean < TREAD_ANGLE);
        let (edge, normal) = section(TREAD_ANGLE, half);
        assert!(
            normal.x.atan2(normal.y) > 60f32.to_radians(),
            "{edge:?} {normal:?}"
        );
    }

    #[test]
    fn the_texture_repeats_seamlessly_round_the_tyre() {
        let image = image_of(Compound::Medium);
        let data = image.data.unwrap();
        let shade = |x: u32, y: u32| data[((y * IMAGE.x + x) * 4) as usize] as i32;
        for y in [40, 100, 128, 160, 220] {
            let (first, last) = (shade(0, y), shade(IMAGE.x - 1, y));
            assert!((first - last).abs() < 12, "{y}: {first} {last}");
        }
    }

    #[test]
    fn stripping_leaves_the_rim_and_takes_the_tyre() {
        // A flat ring of triangles from the axle out to the radius.
        let (axle, radius) = (Vec3::new(0.0, 0.3, -0.7), 0.3);
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for k in 0..20 {
            let r0 = radius * k as f32 / 20.0;
            let r1 = radius * (k + 1) as f32 / 20.0;
            let base = positions.len() as u32;
            for (r, x) in [(r0, -0.05), (r1, -0.05), (r1, 0.05)] {
                positions.push([axle.x + x, axle.y + r, axle.z]);
            }
            indices.extend([base, base + 1, base + 2]);
        }
        let ring = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_indices(Indices::U32(indices));
        let (rim, half_width) = strip(ring, axle, radius);
        let kept = rim.indices().unwrap().len() / 3;
        assert!(kept > 5 && kept < 20, "{kept}");
        assert!((half_width - 0.05 * WIDTH_SCALE).abs() < 1e-5);
    }

    pub fn the_imported_bikes_wear_every_compound_on(roster: &'static crate::bike::Roster) {
        let worn: Vec<Compound> = super::super::designs::every(roster)
            .into_iter()
            .filter(|design| design.model().is_some())
            .map(Compound::of)
            .collect();
        for compound in Compound::ALL {
            assert!(worn.contains(&compound), "{compound:?}");
        }
    }
}
