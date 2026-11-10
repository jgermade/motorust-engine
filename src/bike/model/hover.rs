//! Hover bikes: models that float over the road on pads instead of rolling
//! on wheels (their `model.ron` says so, see [`Hover`]), ridden where a
//! circuit's bikes hover (see `crate::circuits::Circuit::hover`).
//!
//! One has no wheels, no tyres and no fork: its `bike.bin` is its body
//! alone, modelled floating, its pads' undersides a little over the ground.
//! Its measures are a bike's (see [`super::Geometry`]), its axles the
//! middles of its pads and their radii the pads', so that everything that
//! goes by a bike's measures (the rider's seat, grips and pegs, the pitch of
//! the suspension, where it turns nose up) goes by them too.
//!
//! What it adds to a bike's look:
//!
//! - it bobs up and down, gently, most when it stands still ([`bob`]): each
//!   bike of a grid at its own point of the swing;
//! - under each pad a pool of light on the road ([`PadGlow`]), gone while it
//!   is in the air or down;
//! - drifting (see `crate::bike::step`), its nose swings into the bend (the
//!   bike's lean pivot turns it, with its rider).

use std::f32::consts::TAU;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::NotShadowCaster,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::Geometry;
use crate::bike::{BikeState, CrashState};
use crate::geometry::smoothstep;

/// How a model hovers, as its `model.ron` says.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hover {
    /// The colour of the light its pads cast on the road under them.
    #[serde(deserialize_with = "crate::circuits::data::color")]
    pub glow: Color,
}

/// On a bike that hovers.
#[derive(Component, Clone, Copy, Debug)]
pub struct Hovering {
    /// Where in its swing its bob starts (rad): each bike's its own, so a
    /// grid of them does not bob as one.
    pub phase: f32,
}

impl Hovering {
    /// The `number`th bike's: a golden angle on from the one before, so no
    /// two of a grid are in step.
    pub fn numbered(number: u32) -> Self {
        Self {
            phase: (number as f32 * 2.399_963) % TAU,
        }
    }
}

/// How far a hover bike bobs up and down standing still, either way (m),
/// and flat out, where the air holds it steadier.
pub const BOB: f32 = 0.045;
const BOB_AT_SPEED: f32 = 0.012;
/// How long one swing up and down takes (s).
const BOB_PERIOD: f32 = 2.6;
/// The speeds between which the bob settles from the one to the other
/// (m/s).
const BOB_SPEEDS: (f32, f32) = (2.0, 30.0);

/// How far above where it floats at rest a hover bike is `seconds` into
/// its bob at `speed` (m/s), its swing starting at `phase` (m; negative,
/// below): a slow swing up and down with a slower sway under it, so that it
/// never repeats quite alike, never further than [`BOB`] either way.
pub fn bob(seconds: f32, speed: f32, phase: f32) -> f32 {
    let swing = BOB.lerp(
        BOB_AT_SPEED,
        smoothstep(BOB_SPEEDS.0, BOB_SPEEDS.1, speed.abs()),
    );
    let turn = TAU * seconds / BOB_PERIOD + phase;
    swing * (0.8 * turn.sin() + 0.2 * (0.37 * turn + 1.3).sin())
}

/// The pool of light on the road under a pad of `bike`.
#[derive(Component)]
pub struct PadGlow {
    bike: Entity,
}

/// How far over the road the pools of light lie (m): over its paint, and
/// clear of it where the road banks under a bike that stands level.
const GLOW_LIFT: f32 = 0.07;
/// How big a pool is against its pad (its side over the pad's radius).
const GLOW_SIZE: f32 = 3.4;
/// How bright it is.
const GLOW_STRENGTH: f32 = 0.85;
/// The side of its picture (pixels).
const GLOW_PIXELS: u32 = 64;

/// How bright the pool of light is `distance` from its middle (1 at its
/// edge): brightest in the middle, gone at the edge, with no line there.
fn glow_falloff(distance: f32) -> f32 {
    let inside = (1.0 - distance.clamp(0.0, 1.0)).powi(2);
    // A brighter heart, as under a thruster.
    inside * (0.6 + 0.4 * (1.0 - smoothstep(0.0, 0.45, distance)))
}

/// The picture of a pool of light: white, fading out from the middle.
fn glow_image() -> Image {
    let middle = (GLOW_PIXELS as f32 - 1.0) / 2.0;
    let data = (0..GLOW_PIXELS * GLOW_PIXELS)
        .flat_map(|i| {
            let (x, y) = ((i % GLOW_PIXELS) as f32, (i / GLOW_PIXELS) as f32);
            let distance = Vec2::new(x - middle, y - middle).length() / middle;
            let alpha = (glow_falloff(distance) * 255.0).round() as u8;
            [255, 255, 255, alpha]
        })
        .collect();
    let mut image = Image::new(
        Extent3d {
            width: GLOW_PIXELS,
            height: GLOW_PIXELS,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// What the pools of light of every hover bike share: their mesh (a square
/// a metre on a side, lying flat) and their picture.
pub(super) struct GlowParts {
    mesh: Handle<Mesh>,
    image: Handle<Image>,
}

impl GlowParts {
    pub(super) fn new(world: &mut World) -> Self {
        let image = world.resource_mut::<Assets<Image>>().add(glow_image());
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Plane3d::default().mesh().size(1.0, 1.0));
        Self { mesh, image }
    }

    /// The material of the pools of light of a model that hovers so: added
    /// to what is under them, lit by nothing.
    pub(super) fn material(&self, hover: &Hover) -> StandardMaterial {
        StandardMaterial {
            base_color: hover.glow.with_alpha(GLOW_STRENGTH),
            base_color_texture: Some(self.image.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            ..default()
        }
    }
}

/// Where the pools of light of a bike with these measures lie, in its
/// space, and how big each is: under its two pads.
pub fn pads(geometry: &Geometry) -> [(Vec3, f32); 2] {
    [
        (geometry.front_axle, geometry.front_radius),
        (geometry.rear_axle, geometry.rear_radius),
    ]
    .map(|(pad, radius)| (Vec3::new(pad.x, GLOW_LIFT, pad.z), GLOW_SIZE * radius))
}

/// Makes `bike` a hover bike: its bob, and the pools of light under its
/// pads, children of the bike itself, so that they lie on the road while it
/// leans and bobs over them.
pub(super) fn spawn(
    commands: &mut Commands,
    bike: Entity,
    geometry: &Geometry,
    glow: &GlowParts,
    material: &Handle<StandardMaterial>,
) {
    commands
        .entity(bike)
        .insert(Hovering::numbered(bike.index_u32()));
    for (place, size) in pads(geometry) {
        commands.spawn((
            PadGlow { bike },
            Mesh3d(glow.mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(place).with_scale(Vec3::new(size, 1.0, size)),
            NotShadowCaster,
            ChildOf(bike),
        ));
    }
}

/// Whether a hover bike's pads light the road: on it, and not down.
pub fn pads_lit(state: &BikeState, crash: Option<&CrashState>) -> bool {
    state.grounded && !crash.is_some_and(|crash| crash.is_down())
}

/// The pools of light under the pads, there while their bike rides over
/// the road.
pub(super) fn light_pads(
    bikes: Query<(&BikeState, Option<&CrashState>), With<Hovering>>,
    mut glows: Query<(&PadGlow, &mut Visibility)>,
) {
    for (glow, mut visibility) in &mut glows {
        let Ok((state, crash)) = bikes.get(glow.bike) else {
            continue;
        };
        let wanted = if pads_lit(state, crash) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    #[test]
    fn a_hover_bike_bobs_gently_most_standing_still() {
        let swing = |speed: f32, phase: f32| {
            let heights: Vec<f32> = (0..2000)
                .map(|i| bob(i as f32 * 0.01, speed, phase))
                .collect();
            let (low, high) = heights
                .iter()
                .fold((f32::MAX, f32::MIN), |(low, high), &h| {
                    (low.min(h), high.max(h))
                });
            // Never a jump from one frame to the next: it swings.
            for pair in heights.windows(2) {
                assert!((pair[1] - pair[0]).abs() < 0.002);
            }
            (low, high)
        };
        // Standing still: up and down a few centimetres, never past its
        // most either way.
        let (low, high) = swing(0.0, 0.0);
        assert!((0.6 * BOB..=BOB + 1e-6).contains(&high), "{high}");
        assert!((-BOB - 1e-6..-0.6 * BOB).contains(&low), "{low}");
        // Flat out, far less.
        let (fast_low, fast_high) = swing(80.0, 0.0);
        assert!(fast_high < 0.35 * high && fast_low > 0.35 * low);
        // Two bikes of a grid are not in step.
        let (first, second) = (Hovering::numbered(4), Hovering::numbered(5));
        assert!((bob(1.0, 0.0, first.phase) - bob(1.0, 0.0, second.phase)).abs() > 0.005);
    }

    #[test]
    fn the_pools_of_light_lie_under_the_pads_fading_out_to_their_edges() {
        let geometry = Geometry {
            front_axle: Vec3::new(0.0, 0.3, -0.7),
            rear_axle: Vec3::new(0.0, 0.32, 0.68),
            front_radius: 0.3,
            rear_radius: 0.25,
            steering_head: Vec3::new(0.0, 0.84, -0.5),
            grip: Vec3::new(0.3, 0.9, -0.4),
            peg: Vec3::new(0.2, 0.4, 0.3),
            seat: Vec3::new(0.0, 0.8, 0.3),
        };
        let [(front, front_size), (rear, rear_size)] = pads(&geometry);
        assert_eq!((front.x, front.z), (0.0, -0.7));
        assert_eq!((rear.x, rear.z), (0.0, 0.68));
        // On the road, just over it, whatever the pads' height.
        assert_eq!((front.y, rear.y), (GLOW_LIFT, GLOW_LIFT));
        assert!(front_size > rear_size && rear_size > 2.0 * 0.25);
        // Brightest in the middle, gone at the edge.
        assert!(glow_falloff(0.0) > 0.99);
        assert!(glow_falloff(0.5) < glow_falloff(0.2));
        assert_eq!(glow_falloff(1.0), 0.0);
        assert_eq!(glow_falloff(1.4), 0.0);
        let image = glow_image();
        let data = image.data.as_ref().expect("the picture has its pixels");
        // Its corners clear: no square shows on the road.
        assert_eq!(data[3], 0);
        assert_eq!(data[data.len() - 1], 0);
    }

    #[test]
    fn the_pads_light_the_road_only_riding_over_it() {
        let riding = BikeState::parked_at((Vec2::ZERO, 0.0));
        assert!(pads_lit(&riding, None));
        assert!(pads_lit(&riding, Some(&CrashState::default())));
        let flying = BikeState {
            grounded: false,
            ..riding
        };
        assert!(!pads_lit(&flying, None));
    }
}
