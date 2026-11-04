//! Flat sprites of pixel art standing for 3D things, for the circuits drawn
//! as voxels (see [`super::scenery::Look::Voxel`]): a thing's mesh,
//! "photographed" from the side at a few pixels a metre ([`photograph`]),
//! lit by the sun as it stands, its colours cut to a palette and outlined
//! one pixel darker; and drawn on an upright card that turns about its foot
//! to face the camera ([`FacingCamera`], [`face_the_camera`]), as the
//! sprites of the 8- and 16-bit games stood up from their roads.
//!
//! The photographs are taken on the CPU, as the circuit is prepared: a small
//! rasterizer of the mesh's triangles, by their vertices' colours, with a
//! depth buffer. No camera, no GPU.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

pub struct SpritesPlugin;

impl Plugin for SpritesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            face_the_camera.before(TransformSystems::Propagate),
        );
    }
}

/// The way the sun lights the photographs: from in front, above and the
/// left, as the sprites of old were drawn.
const LIGHT: Vec3 = Vec3::new(-0.45, 0.75, 0.48);
/// How bright the faces turned away from the light are, and those facing it.
const SHADE: (f32, f32) = (0.5, 1.05);
/// How much darker the outline is than what it is round.
const OUTLINE: f32 = 0.55;

/// A thing photographed (see [`photograph`]): its picture, and how wide and
/// high it stands (m), its foot at the bottom middle of the picture.
pub struct Photograph {
    pub image: Image,
    pub size: Vec2,
}

/// `mesh` (positions, normals, its colours in its vertices, linear, and its
/// triangles' indices), seen from in front (from +Z, looking along -Z),
/// `pixel` metres a pixel: each pixel the colour of the nearest triangle
/// over it, lit as that triangle faces the sun ([`LIGHT`]), cut to `shades`
/// of each of red, green and blue if given, and outlined one pixel darker
/// where it borders the empty ground round it, which is see-through. Its
/// foot (the origin) at the bottom middle. `None` if it has no triangles.
pub fn photograph(mesh: &Mesh, pixel: f32, shades: Option<u32>) -> Option<Photograph> {
    let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION)? {
        VertexAttributeValues::Float32x3(values) => values,
        _ => return None,
    };
    let colors: Vec<[f32; 4]> = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(values)) => values.clone(),
        _ => vec![[1.0; 4]; positions.len()],
    };
    let indices: Vec<usize> = match mesh.indices() {
        Some(Indices::U32(indices)) => indices.iter().map(|&i| i as usize).collect(),
        Some(Indices::U16(indices)) => indices.iter().map(|&i| i as usize).collect(),
        None => (0..positions.len()).collect(),
    };
    if indices.len() < 3 {
        return None;
    }
    let points: Vec<Vec3> = positions.iter().map(|&p| Vec3::from_array(p)).collect();
    // As wide either side of its foot as its widest side, and from its foot
    // up to its top.
    let reach = points.iter().map(|p| p.x.abs()).fold(0.0, f32::max);
    let top = points.iter().map(|p| p.y).fold(0.0, f32::max);
    let width = ((2.0 * reach / pixel).ceil() as usize + 2).max(3);
    let height = ((top / pixel).ceil() as usize + 2).max(3);
    let size = Vec2::new(width as f32, height as f32) * pixel;
    // The picture's pixel at a point (x across, y up), its middle at 0.
    let to_pixel = |p: Vec3| {
        Vec2::new(
            p.x / pixel + width as f32 / 2.0,
            height as f32 - 1.0 - p.y / pixel,
        )
    };
    let mut color = vec![None::<Vec3>; width * height];
    let mut depth = vec![f32::NEG_INFINITY; width * height];
    let light = LIGHT.normalize();
    for &[a, b, c] in indices.as_chunks::<3>().0 {
        let (pa, pb, pc) = (points[a], points[b], points[c]);
        // Lit as it faces, whichever way round it was made.
        let mut normal = (pb - pa).cross(pc - pa).normalize_or_zero();
        if normal.z < 0.0 {
            normal = -normal;
        }
        let lit = SHADE.0 + (SHADE.1 - SHADE.0) * normal.dot(light).max(0.0);
        let tint = |i: usize| Vec3::new(colors[i][0], colors[i][1], colors[i][2]);
        let (ta, tb, tc) = (tint(a), tint(b), tint(c));
        let (qa, qb, qc) = (to_pixel(pa), to_pixel(pb), to_pixel(pc));
        let area = (qb - qa).perp_dot(qc - qa);
        if area.abs() < 1e-9 {
            continue;
        }
        let low = qa.min(qb).min(qc).floor().max(Vec2::ZERO);
        let high = qa
            .max(qb)
            .max(qc)
            .ceil()
            .min(Vec2::new(width as f32 - 1.0, height as f32 - 1.0));
        for y in low.y as usize..=high.y as usize {
            for x in low.x as usize..=high.x as usize {
                let at = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let wa = (qb - at).perp_dot(qc - at) / area;
                let wb = (qc - at).perp_dot(qa - at) / area;
                let wc = 1.0 - wa - wb;
                if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                    continue;
                }
                let z = wa * pa.z + wb * pb.z + wc * pc.z;
                let index = y * width + x;
                if z > depth[index] {
                    depth[index] = z;
                    color[index] = Some((ta * wa + tb * wb + tc * wc) * lit);
                }
            }
        }
    }
    // Outlined: what borders the see-through pixels, darker.
    let filled = |x: isize, y: isize| {
        x >= 0
            && y >= 0
            && (x as usize) < width
            && (y as usize) < height
            && color[y as usize * width + x as usize].is_some()
    };
    let mut data = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let Some(rgb) = color[y * width + x] else {
                continue;
            };
            let (xi, yi) = (x as isize, y as isize);
            let edge = !(filled(xi - 1, yi)
                && filled(xi + 1, yi)
                && filled(xi, yi - 1)
                && filled(xi, yi + 1));
            let rgb = if edge { rgb * OUTLINE } else { rgb };
            let srgb = rgb.clamp(Vec3::ZERO, Vec3::ONE).powf(1.0 / 2.2);
            let srgb = match shades {
                Some(shades) => {
                    let steps = (shades.clamp(2, 256) - 1) as f32;
                    (srgb * steps).round() / steps
                }
                None => srgb,
            };
            let out = &mut data[(y * width + x) * 4..][..4];
            out[0] = (srgb.x * 255.0).round() as u8;
            out[1] = (srgb.y * 255.0).round() as u8;
            out[2] = (srgb.z * 255.0).round() as u8;
            out[3] = 255;
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Its pixels sharp, as pixel art's.
    image.sampler = ImageSampler::nearest();
    Some(Photograph { image, size })
}

/// The card a photograph is drawn on: upright, `size` metres wide and high,
/// its foot (the origin) at its bottom middle, facing +Z.
pub fn card(size: Vec2) -> Mesh {
    Mesh::from(Rectangle::new(size.x, size.y)).translated_by(Vec3::Y * size.y / 2.0)
}

/// The material a photograph is drawn with: its pixels as they are (lit in
/// the photograph already), the see-through ones cut out, both sides drawn.
pub fn card_material(image: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(image),
        alpha_mode: AlphaMode::Mask(0.5),
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

/// A card that turns about its foot, its upright, to face the camera.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct FacingCamera;

/// The turn about its upright that makes a card at `at` face a camera at
/// `eye`: its front (+Z) towards it, level.
pub fn facing(at: Vec3, eye: Vec3) -> Quat {
    let toward = Vec2::new(eye.x - at.x, eye.z - at.z);
    if toward.length_squared() < 1e-6 {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_y(toward.x.atan2(toward.y))
}

/// Turns the cards to face the first player's camera (the first camera that
/// draws the world and is on), every frame.
fn face_the_camera(
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut cards: Query<&mut Transform, With<FacingCamera>>,
) {
    let Some(eye) = cameras
        .iter()
        .filter(|(camera, _)| camera.is_active)
        .min_by_key(|(camera, _)| camera.order)
        .map(|(_, transform)| transform.translation())
    else {
        return;
    };
    cards.par_iter_mut().for_each(|mut transform| {
        let turn = facing(transform.translation, eye);
        if transform.rotation != turn {
            transform.rotation = turn;
        }
    });
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    /// A box `width` wide and `high` tall, its foot at the origin, all of
    /// one colour.
    fn a_box(width: f32, high: f32, color: [f32; 4]) -> Mesh {
        let mesh = Mesh::from(Cuboid::new(width, high, width)).translated_by(Vec3::Y * high / 2.0);
        let count = mesh.count_vertices();
        let mut mesh = mesh.with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![color; count]);
        if let Some(Indices::U16(indices)) = mesh.indices() {
            let wide = indices.iter().map(|&i| i as u32).collect();
            mesh.insert_indices(Indices::U32(wide));
        }
        mesh
    }

    #[test]
    fn a_thing_is_photographed_its_foot_at_the_bottom_middle_outlined_and_cut_to_a_palette() {
        let mesh = a_box(2.0, 4.0, [0.2, 0.6, 0.1, 1.0]);
        let photo = photograph(&mesh, 0.25, Some(12)).unwrap();
        let (width, height) = (photo.image.width() as usize, photo.image.height() as usize);
        // 2 m across and 4 m up at 4 pixels a metre, and a pixel either way.
        assert_eq!((width, height), (10, 18));
        assert_eq!(photo.size, Vec2::new(2.5, 4.5));
        let data = photo.image.data.as_ref().unwrap();
        let pixel = |x: usize, y: usize| &data[(y * width + x) * 4..][..4];
        // Its middle filled, its corners above it see-through.
        assert_eq!(pixel(width / 2, height / 2)[3], 255);
        assert_eq!(pixel(0, 0)[3], 0);
        // Outlined: the border darker than the middle.
        let middle = pixel(width / 2, height / 2)[1];
        let border = (0..width)
            .map(|x| pixel(x, height / 2))
            .find(|p| p[3] == 255)
            .unwrap()[1];
        assert!(border < middle, "{border} {middle}");
        // Every channel one of 12 shades.
        let shades: Vec<u8> = (0..12)
            .map(|k| (k as f32 / 11.0 * 255.0).round() as u8)
            .collect();
        for p in data.chunks(4).filter(|p| p[3] == 255) {
            for channel in &p[..3] {
                assert!(shades.contains(channel), "{channel}");
            }
        }
        assert!(
            photograph(
                &Mesh::new(
                    bevy::mesh::PrimitiveTopology::TriangleList,
                    RenderAssetUsages::default()
                )
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new()),
                0.25,
                None
            )
            .is_none()
        );
    }

    #[test]
    fn a_card_faces_the_camera_about_its_upright() {
        let at = Vec3::new(10.0, 2.0, 0.0);
        for eye in [
            Vec3::new(10.0, 5.0, 20.0),
            Vec3::new(-30.0, 1.0, 0.0),
            Vec3::new(14.0, 9.0, -3.0),
        ] {
            let front = facing(at, eye) * Vec3::Z;
            assert!(front.y.abs() < 1e-6);
            let toward = (Vec3::new(eye.x, at.y, eye.z) - at).normalize();
            assert!(front.dot(toward) > 0.999, "{front} {toward}");
        }
        assert_eq!(facing(at, at + Vec3::Y), Quat::IDENTITY);
    }
}
