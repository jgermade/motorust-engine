//! Props made elsewhere (with Meshy), in packs (a game's importer prepares
//! them): each pack's meshes by name and one material in the
//! colours of all of them ([`PropPack`]), loaded for the themes that name
//! them as the first circuit with one of those is built ([`PropPacks`]); and how a prop is placed: where
//! it stands and which way it faces ([`spot`]), the ground kept clear so
//! that a landmark is in view from the road ([`in_view`]), and the parts
//! that turn ([`Turning`]); and each piece lighter far away
//! ([`PropLevels`]). The engine's: the themes of a game place their own
//! props with it.

use std::collections::HashMap;

use bevy::prelude::*;

use super::lod;
use super::mirror::MirrorPoint;
use super::props::Footprint;
use super::scenery::{Prop, Spot};
use crate::bike::model::imported::{read, texture};
use crate::geometry::forward;
use crate::settings::Settings;
use crate::track::{Track, WALL_DISTANCE};

/// How fast a wheel turns (rad/s).
const WHEEL_RATE: f32 = 0.6;
/// How far down the road the riders are when a prop that faces them
/// looks at them (m, see [`Prop::facing_riders`]).
pub const RIDERS_BACK: f32 = 110.0;

/// A piece of a pack: its mesh and the corners of the box round it, in its
/// own frame, and points over its faces for the water to mirror it (see
/// [`surface`]); and its lighter meshes, to draw it far away (see
/// [`Piece::drawn`]).
pub struct Piece {
    pub mesh: Handle<Mesh>,
    pub low: Vec3,
    pub high: Vec3,
    pub surface: Vec<MirrorPoint>,
    lighter: [Handle<Mesh>; LEVELS - 1],
}

impl Piece {
    /// Its mesh, to spawn it with: the full one near the cameras, the
    /// lighter ones far from them ([`PropLevels`]).
    pub fn drawn(&self) -> (Mesh3d, PropLevels) {
        let [first, second] = self.lighter.clone();
        (
            Mesh3d(self.mesh.clone()),
            PropLevels {
                meshes: [self.mesh.clone(), first, second],
                size: (self.high - self.low).max_element(),
                level: 0,
            },
        )
    }
}

/// How many meshes each piece has: the full one and two lighter.
pub const LEVELS: usize = 3;
/// The share of a piece's triangles each lighter mesh keeps, and the fewest
/// it keeps (one with as few or fewer stays as it is).
const LIGHTER: [(usize, usize); LEVELS - 1] = [(4, 48), (12, 24)];
/// How far from the nearest camera (in times its size: its widest, or
/// tallest, as it stands) a prop is drawn with each lighter mesh: from
/// there on it is under about 200 pixels high on a full screen of 1080, and
/// then 90.
const LEVEL_DISTANCES: [f32; LEVELS - 1] = [4.0, 10.0];

/// The lighter meshes of a piece's `full` one (see [`LIGHTER`]): the full
/// one where it would not be lighter.
fn lighter(full: &Mesh) -> [Option<Mesh>; LEVELS - 1] {
    let triangles = full.indices().map_or(0, |indices| indices.len() / 3);
    LIGHTER.map(|(share, fewest)| {
        let target = (triangles / share).max(fewest);
        (target < triangles).then(|| lod::simplify_textured(full, target))
    })
}

/// A prop drawn lighter far away: its meshes, full first, and the size of
/// its piece as made (m).
#[derive(Component, Clone, Debug)]
pub struct PropLevels {
    meshes: [Handle<Mesh>; LEVELS],
    size: f32,
    level: usize,
}

impl PropLevels {
    /// Which of its meshes it is drawn with: 0, the full one, to
    /// [`LEVELS`] - 1.
    pub fn level(&self) -> usize {
        self.level
    }
}

/// The props in view with the mesh their distance from the nearest camera
/// calls for (see [`LEVEL_DISTANCES`]); all whole with Options > Graphics
/// > lighter far props off.
pub fn dress_props(
    settings: Option<Res<Settings>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut props: Query<(
        &mut PropLevels,
        &GlobalTransform,
        &InheritedVisibility,
        &mut Mesh3d,
    )>,
) {
    let lighter = settings
        .as_deref()
        .is_none_or(|settings| settings.lighter_far_props);
    let eyes: Vec<Vec3> = cameras
        .iter()
        .filter(|(camera, _)| camera.is_active)
        .map(|(_, transform)| transform.translation())
        .collect();
    props
        .par_iter_mut()
        .for_each(|(mut levels, transform, visible, mut mesh)| {
            if !visible.get() || (!lighter && levels.level == 0) {
                return;
            }
            let level = if !lighter {
                0
            } else if eyes.is_empty() {
                levels.level
            } else {
                let at = transform.translation();
                let distance = eyes
                    .iter()
                    .map(|eye| eye.distance(at))
                    .fold(f32::MAX, f32::min);
                let size = levels.size * transform.affine().matrix3.x_axis.length();
                lod::level_at(distance, levels.level, &LEVEL_DISTANCES.map(|k| k * size))
            };
            if level != levels.level {
                levels.level = level;
                mesh.0 = levels.meshes[level].clone();
            }
        });
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
/// material, in the colours of all of them; and, if the pack has one
/// (`glow.jpg`, beside its colours, laid out as they are), what of them
/// shines of its own: lit windows, lamps, signs.
pub struct PropPack {
    pieces: Vec<(String, Piece)>,
    pub material: Handle<StandardMaterial>,
    pub glow: Option<Handle<Image>>,
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

    /// The pack in `bin` (as an importer writes it), its colours in
    /// `color` and what shines of it in `glow`, if it has that.
    pub fn load(
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
        bin: &[u8],
        color: &[u8],
        glow: Option<&[u8]>,
    ) -> Self {
        let pack = read(bin);
        let atlas = texture(color, true);
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let lighter = super::terrain::on_cores(&pack.meshes, threads, |part| {
            part.iter().map(|piece| lighter(&piece.mesh)).collect()
        });
        let pieces = pack
            .meshes
            .into_iter()
            .zip(lighter)
            .map(|(piece, lighter)| {
                let (low, high) = bounds(&piece.mesh);
                let surface = surface(&piece.mesh, &atlas);
                let mesh = meshes.add(piece.mesh);
                let lighter = lighter
                    .map(|light| light.map_or_else(|| mesh.clone(), |light| meshes.add(light)));
                (
                    piece.name,
                    Piece {
                        mesh,
                        low,
                        high,
                        surface,
                        lighter,
                    },
                )
            })
            .collect();
        let atlas = images.add(atlas);
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(atlas),
            perceptual_roughness: 0.85,
            reflectance: 0.3,
            ..default()
        });
        let glow = glow.map(|glow| images.add(texture(glow, true)));
        Self {
            pieces,
            material,
            glow,
        }
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
/// each loaded once: those of a circuit's themes as it is first built
/// ([`PropPacks::load_for`]), not all of them as the game starts (they took
/// seconds of it, on the web most of all, for circuits maybe never raced).
#[derive(Resource, Default)]
pub struct PropPacks(HashMap<&'static str, PropPack>);

impl PropPacks {
    /// Loads, if they are not yet, the packs the themes `themes` place.
    pub fn load_for(
        &mut self,
        themes: &super::themes::Themes,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) {
        for (name, _) in themes.iter() {
            for &folder in super::themes::packs_of(name) {
                if self.0.contains_key(folder) {
                    continue;
                }
                let start = bevy::platform::time::Instant::now();
                let pack = PropPack::load(
                    meshes,
                    images,
                    materials,
                    crate::content::file(&format!("models/{folder}/props.bin")),
                    crate::content::file(&format!("models/{folder}/color.jpg")),
                    crate::content::try_file(&format!("models/{folder}/glow.jpg")).ok(),
                );
                info!(
                    "Props of models/{folder}: {} pieces, with their lighter meshes, in {:.0} ms",
                    pack.pieces.len(),
                    start.elapsed().as_secs_f64() * 1000.0
                );
                self.0.insert(folder, pack);
            }
        }
    }

    /// Whether the pack in `assets/models/FOLDER/` is loaded.
    pub fn has(&self, folder: &str) -> bool {
        self.0.contains_key(folder)
    }

    /// The pack in `assets/models/FOLDER/`. It stops the game, saying why,
    /// if no theme of the circuit being built named it.
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
