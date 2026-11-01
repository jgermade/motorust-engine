//! A circuit split between the Earth and the Moon: two worlds
//! laid side by side on one map, joined only by the interdimensional portals
//! the road runs through. Each realm is drawn on render layers of its own,
//! and a rider's camera sees the realm it is in, never the other one over
//! the border beside the road: there, it sees the land as its own realm's
//! (the terrain is drawn twice, each time all of one realm, see
//! [`super::terrain::Terrain::meshes_as`]). The other realm shows only
//! through a portal's curtain ([`PortalCurtain`]), drawn by a second camera
//! ([`PortalView`]) at the same place, seeing only that realm.
//!
//! From above, in the menus, the circuit is seen whole, each realm as it
//! is ([`OVERVIEW_LAYER`]).

use bevy::app::{HierarchyPropagatePlugin, Propagate, PropagateSet};
use bevy::camera::primitives::Frustum;
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::{ImageRenderTarget, RenderTarget, SubCameraView, Viewport};
use bevy::image::ImageSampler;
use bevy::light::NotShadowCaster;
use bevy::math::primitives::ViewFrustum;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::pbr::PbrPlugin;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::transform::TransformSystems;

use super::quads::MeshBuilder;
use super::scenery::{Scenery, Soil};
use super::sky::Sunlight;
use super::terrain::is_earth_realm;
use crate::bike::BikeState;
use crate::camera::CameraPlayer;
use crate::circuits::{Circuits, CurrentCircuit};
use crate::menu::GameState;
use crate::track::Track;

/// The render layers of a split circuit: what stands on the Moon, what
/// stands on the Earth, everything as it is (seen from above in the
/// menus), and each player's view of the portals' curtains. What both
/// realms share (the sky, the portals' frames) is on the default layer, 0.
pub const MOON_LAYER: usize = 1;
pub const EARTH_LAYER: usize = 2;
pub const OVERVIEW_LAYER: usize = 3;
const CURTAIN_LAYERS: [usize; 2] = [4, 5];
/// Every layer: the sun lights and shadows them all, but in a race.
const ALL_LAYERS: [usize; 6] = [0, MOON_LAYER, EARTH_LAYER, OVERVIEW_LAYER, 4, 5];

/// How high over the road the middle of a portal's ring is (m), and the
/// radius of its curtain (m), as `super::portals` builds them.
pub const GATE_HEIGHT: f32 = 4.2;
pub const CURTAIN_RADIUS: f32 = 10.3;
/// Near a portal the realm is the side of its curtain one is on: within
/// this far of it along the road (m) and this far round its middle (m).
const GATE_DEPTH: f32 = 30.0;
const GATE_REACH: f32 = 16.0;
/// A portal is looked through, drawing the other realm, when in sight and
/// nearer than this (m); further, it is too small to tell.
const PORTAL_SIGHT: f32 = 500.0;

/// One of the two worlds of a split circuit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Realm {
    Earth,
    Moon,
}

impl Realm {
    /// The realm a point (x, z) of the map lies in, away from the portals.
    pub fn at(point: Vec2) -> Self {
        if is_earth_realm(point) {
            Realm::Earth
        } else {
            Realm::Moon
        }
    }

    /// The realm a triangle lies in: that of its middle.
    pub fn of_triangle([a, b, c]: [Vec3; 3]) -> Self {
        let middle = (a + b + c) / 3.0;
        Self::at(Vec2::new(middle.x, middle.z))
    }

    pub fn other(self) -> Self {
        match self {
            Realm::Earth => Realm::Moon,
            Realm::Moon => Realm::Earth,
        }
    }

    fn layer(self) -> usize {
        match self {
            Realm::Earth => EARTH_LAYER,
            Realm::Moon => MOON_LAYER,
        }
    }

    /// The layers of what stands in this realm: seen from it, and from
    /// above in the menus.
    pub fn layers(self) -> RenderLayers {
        RenderLayers::from_layers(&[self.layer(), OVERVIEW_LAYER])
    }

    /// The layers of the land beyond its border drawn as this realm's: seen
    /// only from it.
    pub fn beyond_layers(self) -> RenderLayers {
        RenderLayers::layer(self.layer())
    }
}

/// Whether a circuit is split between the Earth and the Moon: the Moon's
/// soil, with portals to the Earth.
pub fn splits(scenery: &Scenery) -> bool {
    scenery.soil == Soil::Lunar && !scenery.portals.is_empty()
}

/// The triangles of `mesh` (a list of triangles) whose middle lies in
/// `realm`; `None` if there are none. The vertices are all kept.
pub fn part_in(mesh: &Mesh, realm: Realm) -> Option<Mesh> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Some(mesh.clone());
    };
    let indices: Vec<u32> = match mesh.indices() {
        Some(indices) => indices.iter().map(|index| index as u32).collect(),
        None => (0..positions.len() as u32).collect(),
    };
    let kept: Vec<u32> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|triangle| {
            Realm::of_triangle([0, 1, 2].map(|k| Vec3::from(positions[triangle[k] as usize])))
                == realm
        })
        .flatten()
        .copied()
        .collect();
    if kept.is_empty() {
        return None;
    }
    let mut part = mesh.clone();
    if kept.len() != indices.len() {
        part.insert_indices(Indices::U32(kept));
    }
    Some(part)
}

/// What stands on a split circuit and is put in the world under this:
/// each of its children is drawn in the realm it stands in, unless it says
/// where it is drawn itself (a [`Propagate`] of its layers).
#[derive(Component)]
pub struct RealmScenery;

/// An interdimensional portal across the road, its ring in its XY plane
/// (see [`super::portals`]): its curtain is put on it here.
#[derive(Component)]
pub struct PortalGate;

/// A portal of the circuit in use: the middle of its ring, which way its
/// curtain faces, and on which side of it the Earth is (1 in front, -1
/// behind).
#[derive(Clone, Copy, Debug)]
struct Gate {
    middle: Vec3,
    facing: Vec3,
    earth_side: f32,
}

/// The portals of the circuit in use, if it is split.
#[derive(Resource, Default)]
struct Gates(Vec<Gate>);

impl Gates {
    /// The realm a point is in: near a portal, the side of its curtain it is
    /// on, so that one goes from one realm to the other just as one passes
    /// through it; elsewhere, that of the map.
    fn realm_at(&self, point: Vec3) -> Realm {
        for gate in &self.0 {
            let offset = point - gate.middle;
            let depth = offset.dot(gate.facing);
            if depth.abs() < GATE_DEPTH && (offset - gate.facing * depth).length() < GATE_REACH {
                return if depth * gate.earth_side >= 0.0 {
                    Realm::Earth
                } else {
                    Realm::Moon
                };
            }
        }
        Realm::at(Vec2::new(point.x, point.z))
    }
}

/// The portals along `track`, as `super::portals` puts them, on a split
/// circuit.
fn gates(track: &Track, scenery: &Scenery) -> Vec<Gate> {
    if !splits(scenery) {
        return Vec::new();
    }
    scenery
        .portals
        .iter()
        .map(|portal| {
            let index = track.index_at(portal.along);
            let (center, heading) = track.pose_at(index);
            let road_y = track.surface_point(index, 0.0).y;
            let facing = Quat::from_rotation_y(heading) * Vec3::Z;
            let ahead = center + Vec2::new(facing.x, facing.z) * GATE_DEPTH;
            Gate {
                middle: Vec3::new(center.x, road_y + GATE_HEIGHT, center.y),
                facing,
                earth_side: if is_earth_realm(ahead) { 1.0 } else { -1.0 },
            }
        })
        .collect()
}

/// What a player's portal view is drawn into, and the curtains showing it.
#[derive(Resource)]
struct PortalViews {
    images: [Handle<Image>; 2],
    curtains: [Handle<PortalCurtain>; 2],
    curtain_mesh: Handle<Mesh>,
}

impl FromWorld for PortalViews {
    fn from_world(world: &mut World) -> Self {
        let images = {
            let mut images = world.resource_mut::<Assets<Image>>();
            [(); 2].map(|_| images.add(target_image(UVec2::ONE)))
        };
        let curtains = {
            let mut curtains = world.resource_mut::<Assets<PortalCurtain>>();
            [0, 1].map(|player| {
                curtains.add(PortalCurtain {
                    look: Vec4::ZERO,
                    beyond: images[player].clone(),
                })
            })
        };
        let curtain_mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(curtain_mesh(CURTAIN_RADIUS));
        Self {
            images,
            curtains,
            curtain_mesh,
        }
    }
}

/// An image `size` pixels big for a camera to draw into.
fn target_image(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.data = None;
    image.sampler = ImageSampler::linear();
    // Made again at another size from scratch: what was drawn into it before
    // is not kept (nor can it be: it cannot be copied from).
    image.copy_on_resize = false;
    image
}

/// The curtain of a portal: a disc `radius` m round, seen from both sides,
/// its texture coordinates across it from 0 to 1.
fn curtain_mesh(radius: f32) -> Mesh {
    const SLICES: usize = 48;
    let mut mesh = MeshBuilder::default();
    let uv = |p: Vec3| Vec2::new(p.x, -p.y) / (2.0 * radius) + Vec2::splat(0.5);
    for i in 0..SLICES {
        let a0 = i as f32 / SLICES as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / SLICES as f32 * std::f32::consts::TAU;
        let p0 = Vec3::new(a0.cos(), a0.sin(), 0.0) * radius;
        let p1 = Vec3::new(a1.cos(), a1.sin(), 0.0) * radius;
        for facing in [Vec3::Z, Vec3::NEG_Z] {
            mesh.triangle(
                [(Vec3::ZERO, uv(Vec3::ZERO)), (p0, uv(p0)), (p1, uv(p1))],
                facing,
                Color::WHITE,
            );
        }
    }
    mesh.build()
}

/// A portal's curtain as a rider sees it (see `portal.wgsl`): the other
/// realm, as its [`PortalView`] draws it at the same place, swirling at
/// the rim. Unlit, as what it shows is lit already.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct PortalCurtain {
    /// x: 1 while the other realm is drawn into `beyond`, 0 otherwise.
    #[uniform(0)]
    look: Vec4,
    #[texture(1)]
    #[sampler(2)]
    beyond: Handle<Image>,
}

impl Material for PortalCurtain {
    fn fragment_shader() -> ShaderRef {
        "embedded://motorust_engine/world/portal.wgsl".into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// The camera drawing, for a player's camera, the realm it is not in, from
/// the same place, for the portals' curtains to show.
#[derive(Component)]
pub struct PortalView {
    player: usize,
    /// How it projects the part of the screen it draws, if any.
    clip_from_view: Option<Mat4>,
}

pub struct RealmPlugin;

impl Plugin for RealmPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "portal.wgsl");
        // Without the renderer (in the tests), the curtain is only an asset.
        if app.is_plugin_added::<PbrPlugin>() {
            app.add_plugins(MaterialPlugin::<PortalCurtain>::default());
        } else {
            app.init_asset::<PortalCurtain>();
        }
        app.add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate))
            .configure_sets(
                PostUpdate,
                PropagateSet::<RenderLayers>::default().before(VisibilitySystems::CheckVisibility),
            )
            .init_resource::<Gates>()
            .init_resource::<PortalViews>()
            .add_systems(
                PostUpdate,
                (
                    find_gates,
                    (
                        place_scenery,
                        dress_gates,
                        keep_bikes_in_their_realm,
                        see_from_realms,
                    ),
                    look_through_portals,
                )
                    .chain()
                    .before(PropagateSet::<RenderLayers>::default())
                    .before(TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                narrow_portal_frusta
                    .after(VisibilitySystems::UpdateFrusta)
                    .before(VisibilitySystems::CheckVisibility),
            );
    }
}

/// The portals of the circuit in use, when it changes.
fn find_gates(
    current: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    track: Res<Track>,
    mut gates: ResMut<Gates>,
) {
    if current.is_changed() || track.is_changed() {
        gates.0 = self::gates(&track, &circuits.get(*current).scenery);
    }
}

/// Each new child of a [`RealmScenery`] drawn in the realm it stands in.
fn place_scenery(
    mut commands: Commands,
    groups: Query<&Children, (With<RealmScenery>, Changed<Children>)>,
    placed: Query<&Transform, (Without<Propagate<RenderLayers>>, Without<RenderLayers>)>,
) {
    for children in &groups {
        for &child in children {
            if let Ok(transform) = placed.get(child) {
                let realm = Realm::at(transform.translation.xz());
                commands.entity(child).insert(Propagate(realm.layers()));
            }
        }
    }
}

/// Each player's curtain put on each new portal, seen only by that
/// player's camera.
fn dress_gates(
    mut commands: Commands,
    views: Res<PortalViews>,
    gates: Query<Entity, Added<PortalGate>>,
) {
    for gate in &gates {
        commands.entity(gate).with_children(|gate| {
            for (player, curtain) in views.curtains.iter().enumerate() {
                gate.spawn((
                    Mesh3d(views.curtain_mesh.clone()),
                    MeshMaterial3d(curtain.clone()),
                    Transform::from_xyz(0.0, GATE_HEIGHT, 0.0),
                    Propagate(RenderLayers::layer(CURTAIN_LAYERS[player])),
                    NotShadowCaster,
                ));
            }
        });
    }
}

/// Each bike drawn in the realm it is in, on a split circuit.
#[allow(clippy::type_complexity)]
fn keep_bikes_in_their_realm(
    mut commands: Commands,
    gates: Res<Gates>,
    bikes: Query<(Entity, &Transform, Option<&Propagate<RenderLayers>>), With<BikeState>>,
) {
    for (bike, transform, drawn) in &bikes {
        if gates.0.is_empty() {
            if drawn.is_some() {
                commands.entity(bike).remove::<Propagate<RenderLayers>>();
            }
            continue;
        }
        let layers = gates.realm_at(transform.translation).layers();
        if drawn.is_none_or(|drawn| drawn.0 != layers) {
            commands.entity(bike).insert(Propagate(layers));
        }
    }
}

/// The layers of a player's camera in a race on a split circuit: the shared
/// ones, its realm and its curtains.
fn race_layers(realm: Realm, player: usize) -> RenderLayers {
    RenderLayers::from_layers(&[0, realm.layer(), CURTAIN_LAYERS[player.min(1)]])
}

/// What each player's camera sees: in a race on a split circuit, the realm
/// it is in; otherwise, the circuit as it is. And the sun, in such a race,
/// casts the shadows of the first player's realm only: those of the other
/// would fall where nothing stands.
#[allow(clippy::type_complexity)]
fn see_from_realms(
    mut commands: Commands,
    gates: Res<Gates>,
    state: Option<Res<State<GameState>>>,
    cameras: Query<(Entity, &CameraPlayer, &Transform, Option<&RenderLayers>), With<Camera3d>>,
    suns: Query<(Entity, Option<&RenderLayers>), With<Sunlight>>,
) {
    let racing = state.is_some_and(|state| *state.get() == GameState::Race) && !gates.0.is_empty();
    let mut first = None;
    for (camera, player, transform, seen) in &cameras {
        let layers = if racing {
            let realm = gates.realm_at(transform.translation);
            if player.0 == 0 {
                first = Some(realm);
            }
            race_layers(realm, player.0)
        } else {
            RenderLayers::from_layers(&[0, OVERVIEW_LAYER])
        };
        if seen != Some(&layers) {
            commands.entity(camera).insert(layers);
        }
    }
    let lit = match first {
        Some(realm) => RenderLayers::from_layers(&[0, realm.layer()]),
        None => RenderLayers::from_layers(&ALL_LAYERS),
    };
    for (sun, layers) in &suns {
        if layers != Some(&lit) {
            commands.entity(sun).insert(lit.clone());
        }
    }
}

/// Where on a player's screen (`size` pixels) the curtains of the portals
/// in sight are, seen through `clip_from_world`: the box round them, a few
/// pixels more; the whole screen if one is round the camera, and nothing if
/// none is on it.
fn curtain_rect(clip_from_world: Mat4, gates: &[Gate], eye: Vec3, size: UVec2) -> Option<URect> {
    const SLICES: usize = 16;
    const MARGIN: f32 = 4.0;
    let screen = size.as_vec2();
    let mut seen = Rect::EMPTY;
    for gate in gates {
        if gate.middle.distance(eye) > PORTAL_SIGHT {
            continue;
        }
        // Passing through it: drawn whole.
        let offset = eye - gate.middle;
        let depth = offset.dot(gate.facing);
        if depth.abs() < 2.0 && (offset - gate.facing * depth).length() < CURTAIN_RADIUS {
            return Some(URect::from_corners(UVec2::ZERO, size));
        }
        let across = gate.facing.cross(Vec3::Y).normalize_or(Vec3::X);
        let rim: Vec<Vec4> = (0..SLICES)
            .map(|k| {
                let angle = k as f32 / SLICES as f32 * std::f32::consts::TAU;
                let at =
                    gate.middle + (across * angle.cos() + Vec3::Y * angle.sin()) * CURTAIN_RADIUS;
                clip_from_world * at.extend(1.0)
            })
            .collect();
        let ahead = rim.iter().filter(|clip| clip.w > 0.1).count();
        if ahead == 0 {
            // Behind the camera.
            continue;
        }
        if ahead < rim.len() {
            // Round the camera: drawn whole.
            return Some(URect::from_corners(UVec2::ZERO, size));
        }
        for clip in rim {
            let ndc = clip.xy() / clip.w;
            let at = Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) * screen;
            seen = seen.union_point(at);
        }
    }
    let seen = seen
        .inflate(MARGIN)
        .intersect(Rect::from_corners(Vec2::ZERO, screen));
    if seen.is_empty() || seen.width() < 1.0 || seen.height() < 1.0 {
        return None;
    }
    Some(URect::from_corners(
        seen.min.floor().as_uvec2(),
        seen.max.ceil().as_uvec2().min(size),
    ))
}

/// Each player's portal view, in a race on a split circuit: made, put where
/// the player's camera is and drawing the other realm, only where a
/// portal's curtain is on the screen, while one is in sight; and the
/// player's curtains told whether it does.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn look_through_portals(
    mut commands: Commands,
    gates: Res<Gates>,
    state: Option<Res<State<GameState>>>,
    views: Res<PortalViews>,
    mut images: ResMut<Assets<Image>>,
    mut curtains: ResMut<Assets<PortalCurtain>>,
    players: Query<
        (
            &CameraPlayer,
            &Camera,
            &Transform,
            &Projection,
            Option<&DistanceFog>,
        ),
        Without<PortalView>,
    >,
    mut portal_views: Query<
        (
            Entity,
            &mut PortalView,
            &mut Camera,
            &mut Transform,
            &mut Projection,
            &mut RenderLayers,
            Option<&DistanceFog>,
            Option<&Msaa>,
        ),
        Without<CameraPlayer>,
    >,
) {
    let racing = state.is_some_and(|state| *state.get() == GameState::Race) && !gates.0.is_empty();
    let mut drawn = [false; 2];
    for (
        entity,
        mut view,
        mut camera,
        mut transform,
        mut projection,
        mut layers,
        own_fog,
        own_msaa,
    ) in &mut portal_views
    {
        let player = players
            .iter()
            .find(|(player, camera, ..)| player.0 == view.player && camera.is_active);
        let Some((_, seen_from, at, seen_as, fog)) = player.filter(|_| racing) else {
            commands.entity(entity).despawn();
            continue;
        };
        let slot = view.player.min(1);
        *transform = *at;
        *projection = seen_as.clone();
        let beyond =
            RenderLayers::from_layers(&[0, gates.realm_at(at.translation).other().layer()]);
        if *layers != beyond {
            *layers = beyond;
        }
        // The fog changes with the circuit, its colour with it.
        if let Some(fog) = fog
            && own_fog.is_none_or(|own| own.color != fog.color)
        {
            commands.entity(entity).insert(fog.clone());
        }
        // Not multisampled: the curtain's swirl hides its edges, and it is
        // cheaper.
        if own_msaa != Some(&Msaa::Off) {
            commands.entity(entity).insert(Msaa::Off);
        }
        // The image the size of what the player's camera draws, so that a
        // curtain finds what it shows where it is itself on the screen.
        let size = seen_from.physical_viewport_size().unwrap_or(UVec2::ONE);
        let image = &views.images[slot];
        if images.get(image).is_some_and(|drawn| drawn.size() != size) {
            if let Some(mut drawn) = images.get_mut(image) {
                *drawn = target_image(size);
            }
            // The curtain made again with the new image.
            if let Some(mut curtain) = curtains.get_mut(&views.curtains[slot]) {
                curtain.beyond = image.clone();
            }
        }
        // Drawn only where the curtains are on the screen.
        let clip_from_world = seen_as.get_clip_from_view() * at.to_matrix().inverse();
        let rect = curtain_rect(clip_from_world, &gates.0, at.translation, size);
        camera.is_active = rect.is_some();
        view.clip_from_view = None;
        if let Some(rect) = rect {
            let sub = SubCameraView {
                full_size: size,
                offset: rect.min.as_vec2(),
                size: rect.size(),
            };
            view.clip_from_view = Some(seen_as.get_clip_from_view_for_sub(&sub));
            camera.viewport = Some(Viewport {
                physical_position: rect.min,
                physical_size: rect.size(),
                depth: 0.0..1.0,
            });
            camera.sub_camera_view = Some(sub);
        }
        drawn[slot] = rect.is_some();
    }
    // A portal view for each player's camera that has none.
    if racing {
        for (player, ..) in &players {
            if player.0 < 2
                && !portal_views
                    .iter()
                    .any(|(_, view, ..)| view.player == player.0)
            {
                commands.spawn((
                    PortalView {
                        player: player.0,
                        clip_from_view: None,
                    },
                    Camera3d::default(),
                    Camera {
                        // Before the players' cameras, which show it.
                        order: -4 + player.0 as isize,
                        is_active: false,
                        ..default()
                    },
                    RenderTarget::Image(ImageRenderTarget {
                        handle: views.images[player.0].clone(),
                        scale_factor: 1.0,
                    }),
                    RenderLayers::layer(0),
                ));
            }
        }
    }
    for (player, handle) in views.curtains.iter().enumerate() {
        let look = Vec4::new(if drawn[player] { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0);
        if curtains
            .get(handle)
            .is_some_and(|curtain| curtain.look != look)
            && let Some(mut curtain) = curtains.get_mut(handle)
        {
            curtain.look = look;
        }
    }
}

/// A portal view culls what it draws to the part of the screen it draws
/// (Bevy culls to the whole of a sub view): far less of the other realm is
/// drawn than its camera would see.
fn narrow_portal_frusta(
    mut views: Query<(&PortalView, &GlobalTransform, &Projection, &mut Frustum)>,
) {
    for (view, transform, projection, mut frustum) in &mut views {
        if let Some(clip_from_view) = view.clip_from_view {
            frustum.0 = ViewFrustum::from_clip_from_world_custom_far(
                &(clip_from_view * transform.affine().inverse()),
                &transform.translation(),
                &transform.back(),
                projection.far(),
            );
        }
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    pub fn split_circuit(
        circuits: &'static crate::circuits::Circuits,
    ) -> &'static crate::circuits::Circuit {
        circuits
            .iter()
            .find(|circuit| splits(&circuit.scenery))
            .expect("a circuit split between the Earth and the Moon")
    }

    pub fn only_a_lunar_circuit_with_portals_is_split_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        assert_eq!(
            circuits
                .iter()
                .filter(|circuit| splits(&circuit.scenery))
                .count(),
            1
        );
    }

    pub fn one_goes_from_one_realm_to_the_other_only_through_a_portal_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let circuit = split_circuit(circuits);
        let track = Track::new(&circuit.layout);
        let gates = Gates(gates(&track, &circuit.scenery));
        assert_eq!(gates.0.len(), 2);
        // Along the lap, riding at the height of a rider's eyes, the realm
        // changes twice, each time passing through a portal.
        let lap = track.lap_length();
        let mut changes = Vec::new();
        let mut along = 0.0;
        let mut last = None;
        while along < lap {
            let index = track.index_at(along);
            let eye = track.surface_point(index, 0.0) + Vec3::Y * 1.5;
            let realm = gates.realm_at(eye);
            if last.is_some_and(|last| last != realm) {
                changes.push(along);
            }
            last = Some(realm);
            along += 1.0;
        }
        assert_eq!(changes.len(), 2, "realm changes at {changes:?}");
        let portals = circuit.scenery.portals;
        for (change, portal) in changes.iter().zip(portals) {
            assert!(
                (change - portal.along).abs() <= track.step_length() + 1.0,
                "changes at {change} m, the portal is at {} m",
                portal.along
            );
        }
        // The lap starts on the Earth; it goes to the Moon at the first.
        let start = track.surface_point(0, 0.0);
        assert_eq!(gates.realm_at(start), Realm::Earth);
    }

    #[test]
    fn a_mesh_is_parted_by_realm() {
        let mut builder = MeshBuilder::default();
        for x in [-2000.0, 3000.0] {
            builder.triangle(
                [
                    (Vec3::new(x, 0.0, 0.0), Vec2::ZERO),
                    (Vec3::new(x + 1.0, 0.0, 0.0), Vec2::X),
                    (Vec3::new(x, 0.0, 1.0), Vec2::Y),
                ],
                Vec3::Y,
                Color::WHITE,
            );
        }
        let mesh = builder.build();
        let earth = part_in(&mesh, Realm::Earth).expect("a triangle on Earth");
        let moon = part_in(&mesh, Realm::Moon).expect("a triangle on the Moon");
        assert_eq!(earth.indices().unwrap().len(), 3);
        assert_eq!(moon.indices().unwrap().len(), 3);
    }

    #[test]
    fn each_realm_is_seen_with_its_own_land_and_from_above_as_it_is() {
        for realm in [Realm::Earth, Realm::Moon] {
            let seen = race_layers(realm, 0);
            assert!(seen.intersects(&realm.layers()));
            assert!(seen.intersects(&realm.beyond_layers()));
            assert!(!seen.intersects(&realm.other().layers()));
            assert!(!seen.intersects(&realm.other().beyond_layers()));
            let above = RenderLayers::from_layers(&[0, OVERVIEW_LAYER]);
            assert!(above.intersects(&realm.layers()));
            assert!(!above.intersects(&realm.beyond_layers()));
        }
        // Each player sees only their own curtains.
        assert!(!race_layers(Realm::Earth, 0).intersects(&RenderLayers::layer(CURTAIN_LAYERS[1])));
    }

    #[test]
    fn a_portal_view_draws_only_round_the_curtains_in_sight() {
        let gate = Gate {
            middle: Vec3::new(0.0, GATE_HEIGHT, -100.0),
            facing: Vec3::Z,
            earth_side: 1.0,
        };
        let size = UVec2::new(1600, 900);
        let projection = Projection::Perspective(PerspectiveProjection {
            fov: 1.2,
            aspect_ratio: 1600.0 / 900.0,
            ..default()
        });
        let eye = Transform::from_xyz(0.0, 1.5, 0.0);
        let clip = |at: &Transform| projection.get_clip_from_view() * at.to_matrix().inverse();
        // Ahead: a box round it, well inside the screen.
        let rect = curtain_rect(clip(&eye), &[gate], eye.translation, size).expect("in sight");
        assert!(rect.width() < 400 && rect.height() < 400, "{rect:?}");
        assert!(rect.min.x < 800 && rect.max.x > 800, "{rect:?}");
        // Behind: nothing drawn.
        let back = eye.looking_to(Vec3::Z, Vec3::Y);
        assert!(curtain_rect(clip(&back), &[gate], back.translation, size).is_none());
        // Passing through it: the whole screen.
        let through = Transform::from_xyz(0.0, 1.5, -100.0);
        assert_eq!(
            curtain_rect(clip(&through), &[gate], through.translation, size),
            Some(URect::from_corners(UVec2::ZERO, size))
        );
        // Too far to tell: nothing drawn.
        let far = Transform::from_xyz(0.0, 1.5, PORTAL_SIGHT);
        assert!(curtain_rect(clip(&far), &[gate], far.translation, size).is_none());
    }
}
