//! Props made elsewhere (with Meshy), in packs (a game's importer prepares
//! them): each pack's meshes by name and one material in the
//! colours of all of them ([`PropPack`]), loaded as the game starts for the
//! themes that name them ([`PropPacks`]); and how a prop is placed: where
//! it stands and which way it faces ([`spot`]), the ground kept clear so
//! that a landmark is in view from the road ([`in_view`]), and the parts
//! that turn ([`Turning`]). The engine's: the themes of a game place their
//! own props with it.

use std::collections::HashMap;

use bevy::prelude::*;

use super::mirror::MirrorPoint;
use super::props::Footprint;
use super::scenery::{Prop, Spot};
use crate::bike::model::imported::{read, texture};
use crate::geometry::forward;
use crate::track::{Track, WALL_DISTANCE};

/// How fast a wheel turns (rad/s).
const WHEEL_RATE: f32 = 0.6;
/// How far down the road the riders are when a prop that faces them
/// looks at them (m, see [`Prop::facing_riders`]).
pub const RIDERS_BACK: f32 = 110.0;

/// A piece of a pack: its mesh and the corners of the box round it, in its
/// own frame, and points over its faces for the water to mirror it (see
/// [`surface`]).
pub struct Piece {
    pub mesh: Handle<Mesh>,
    pub low: Vec3,
    pub high: Vec3,
    pub surface: Vec<MirrorPoint>,
}

/// How far apart the points over a prop's faces are, at most (m of the
/// prop as made): a few to each texel of the water's map, however big.
const SURFACE_SPACING: f32 = 1.2;

/// Points over the faces of `mesh` (in its own frame), at most
/// [`SURFACE_SPACING`] apart, each with the way its face looks and the
/// colour of `atlas` there (linear): what the water mirrors of it.
pub fn surface(mesh: &Mesh, atlas: &Image) -> Vec<MirrorPoint> {
    let Some(positions) = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|positions| positions.as_float3())
    else {
        return Vec::new();
    };
    let uvs: Vec<[f32; 2]> = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) => uvs.clone(),
        _ => vec![[0.0, 0.0]; positions.len()],
    };
    let indices: Vec<usize> = match mesh.indices() {
        Some(indices) => indices.iter().collect(),
        None => (0..positions.len()).collect(),
    };
    let (width, height) = (atlas.width() as usize, atlas.height() as usize);
    let texels = atlas.data.as_deref().unwrap_or(&[]);
    let color = |uv: Vec2| -> [f32; 3] {
        let x = ((uv.x.fract() + 1.0).fract() * width as f32) as usize;
        let y = ((uv.y.fract() + 1.0).fract() * height as f32) as usize;
        let at = (y.min(height.saturating_sub(1)) * width + x.min(width.saturating_sub(1))) * 4;
        match texels.get(at..at + 3) {
            Some(rgb) => {
                let srgb = Color::srgb_u8(rgb[0], rgb[1], rgb[2]).to_linear();
                [srgb.red, srgb.green, srgb.blue]
            }
            None => [0.5; 3],
        }
    };
    let mut points = Vec::new();
    for &corner in indices.as_chunks::<3>().0 {
        let p = corner.map(|i| Vec3::from(positions[i]));
        let uv = corner.map(|i| Vec2::from(uvs[i]));
        let normal = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        let longest = (p[1] - p[0])
            .length()
            .max((p[2] - p[1]).length())
            .max((p[0] - p[2]).length());
        let steps = ((longest / SURFACE_SPACING).ceil() as usize).clamp(1, 64);
        // A small one, its middle.
        if steps == 1 {
            points.push(MirrorPoint {
                at: (p[0] + p[1] + p[2]) / 3.0,
                normal,
                color: color((uv[0] + uv[1] + uv[2]) / 3.0),
            });
            continue;
        }
        for i in 0..=steps {
            for j in 0..=steps - i {
                let (a, b) = (i as f32 / steps as f32, j as f32 / steps as f32);
                let c = 1.0 - a - b;
                points.push(MirrorPoint {
                    at: p[0] * c + p[1] * a + p[2] * b,
                    normal,
                    color: color(uv[0] * c + uv[1] * a + uv[2] * b),
                });
            }
        }
    }
    points
}

/// A pack of props made with Meshy: their meshes, by name, and their one
/// material, in the colours of all of them.
pub struct PropPack {
    pieces: Vec<(String, Piece)>,
    pub material: Handle<StandardMaterial>,
}

impl PropPack {
    pub fn piece(&self, name: &str) -> &Piece {
        self.find(name)
            .unwrap_or_else(|| panic!("no {name} in the pack"))
    }

    /// Its piece `name`, if it has one.
    pub fn find(&self, name: &str) -> Option<&Piece> {
        self.pieces
            .iter()
            .find(|(piece, _)| piece == name)
            .map(|(_, piece)| piece)
    }

    /// The pack in `bin` (as an importer writes it) and its colours in
    /// `color`.
    pub fn load(world: &mut World, bin: &[u8], color: &[u8]) -> Self {
        let pack = read(bin);
        let atlas = texture(color, true);
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let pieces = pack
            .meshes
            .into_iter()
            .map(|piece| {
                let (low, high) = bounds(&piece.mesh);
                let surface = surface(&piece.mesh, &atlas);
                let mesh = meshes.add(piece.mesh);
                (
                    piece.name,
                    Piece {
                        mesh,
                        low,
                        high,
                        surface,
                    },
                )
            })
            .collect();
        let atlas = world.resource_mut::<Assets<Image>>().add(atlas);
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color_texture: Some(atlas),
                perceptual_roughness: 0.85,
                reflectance: 0.3,
                ..default()
            });
        Self { pieces, material }
    }
}

/// The corners of the box round a mesh.
pub fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|positions| positions.as_float3())
        .expect("a prop has positions")
        .iter()
        .map(|&p| Vec3::from(p))
        .fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(low, high), p| (low.min(p), high.max(p)),
        )
}

/// The packs of props the registered themes name (see
/// [`super::themes::Theme::packs`]), by their folders in `assets/models/`,
/// each loaded once, as the game starts.
#[derive(Resource)]
pub struct PropPacks(HashMap<&'static str, PropPack>);

impl FromWorld for PropPacks {
    fn from_world(world: &mut World) -> Self {
        let packs = super::themes::packs()
            .into_iter()
            .map(|folder| {
                let pack = PropPack::load(
                    world,
                    crate::content::file(&format!("models/{folder}/props.bin")),
                    crate::content::file(&format!("models/{folder}/color.jpg")),
                );
                (folder, pack)
            })
            .collect();
        Self(packs)
    }
}

impl PropPacks {
    /// The pack in `assets/models/FOLDER/`. It stops the game, saying why,
    /// if no theme named it.
    pub fn get(&self, folder: &str) -> &PropPack {
        self.0
            .get(folder)
            .unwrap_or_else(|| panic!("no pack of props in models/{folder}: no theme names it"))
    }
}

/// A part of a prop turning about its axle (+Z as made, its local Z): a
/// water wheel, a mill's wheel.
#[derive(Component)]
pub struct Turning;

/// Turns the wheels.
pub fn turn_wheels(time: Res<Time>, mut wheels: Query<&mut Transform, With<Turning>>) {
    for mut wheel in &mut wheels {
        wheel.rotate_local_z(-WHEEL_RATE * time.delta_secs());
    }
}

/// How far a landmark is seen from (m), and how wide of straight ahead
/// (rad), coming along the road at it.
const VIEW_RANGE: f32 = 220.0;
const VIEW_ANGLE: f32 = 0.6;
/// How wide the ground is kept clear of trees either side of a line from
/// the road to a landmark (m), and every how far along the road one is
/// drawn (m).
pub const VIEW_CLEAR: f32 = 4.5;
const VIEW_EVERY: f32 = 16.0;

/// The ground to keep clear of trees so that a landmark at `position` is in
/// view all the way as the road comes at it: along lines from the road,
/// where it is ahead of the riders, to it, past the road's walls (circles
/// along each, as footprints for the trees to keep out of).
pub fn in_view(track: &Track, position: Vec2) -> Vec<Footprint> {
    let mut clear = Vec::new();
    let step = track.step_length();
    let every = (VIEW_EVERY / step).round().max(1.0) as usize;
    let n = track.sample_count();
    for index in (0..n).step_by(every) {
        let here = track.offset_point(index, 0.0);
        let ahead = (track.offset_point(index + 1, 0.0) - here).normalize_or_zero();
        let to = position - here;
        let distance = to.length();
        if !(WALL_DISTANCE..VIEW_RANGE).contains(&distance) || ahead.angle_to(to).abs() > VIEW_ANGLE
        {
            continue;
        }
        let toward = to / distance;
        let mut along = WALL_DISTANCE + VIEW_CLEAR;
        while along < distance {
            clear.push((here + toward * along, VIEW_CLEAR));
            along += VIEW_CLEAR * 1.5;
        }
    }
    clear
}

/// Where a prop stands and which way it faces.
pub fn spot<M>(track: &Track, prop: &Prop<M>) -> (Vec2, Quat) {
    let (position, facing) = match prop.spot {
        Spot::Road { along, lateral } => {
            let index = track.index_at(along);
            let point = track.offset_point(index, lateral);
            // The road beside it, or, where it faces the riders, the road
            // they are coming up at it.
            let looks = if prop.facing_riders {
                track.index_at(along - RIDERS_BACK)
            } else {
                index
            };
            (point, facing(track.offset_point(looks, 0.0) - point))
        }
        Spot::Over { along } => {
            let (point, heading) = track.pose_at(track.index_at(along));
            (point, facing(forward(heading)))
        }
        Spot::At { position, heading } => (position, facing(forward(heading))),
    };
    (
        position,
        facing * Quat::from_rotation_y(prop.turn) * Quat::from_rotation_z(prop.lean),
    )
}

/// The rotation that turns a prop's front (+Z) towards `direction` (x, z).
pub fn facing(direction: Vec2) -> Quat {
    Quat::from_rotation_y(direction.x.atan2(direction.y))
}
