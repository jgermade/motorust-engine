//! How much detail the world is drawn with. The sun's shadows have a
//! [`Level`], chosen in Options > Graphics or, automatically, lowered while
//! the frame rate falls short of its target and raised again once it keeps
//! up ([`AutoQuality`]): how far they reach, in how many cascades and how
//! fine. They fade out at the end of their reach (see [`super::shadow_fade`]),
//! so a change of level shows as little as a shadow coming near. Nothing
//! else changes with the level, so nothing pops in or out as it does. The
//! edges are smoothed as Options > Graphics says: changing it makes every
//! pipeline again, and what they draw vanishes meanwhile, so only the
//! performance forced changes it by itself (below), and seldom.
//!
//! With two players the world is drawn twice a frame, once for each half of
//! the screen, each with its own shadows, so (unless Options > Graphics says
//! otherwise) it is drawn lighter: the shadows at the lowest level, the
//! grass, flowers and petals nearer and the trees lighter sooner.
//!
//! The performance forced (Options > Graphics > "Force performance") goes
//! further down the same way, quicker, to keep 60 or 120 frames a second
//! (no more than the frame rate cap or the screen let through): the
//! parallax first, if chosen (the dearest of the relief, on the most
//! pixels, the grass, and the least seen in a race: the normal maps stay),
//! then the shadows and the share of the window's resolution the world is
//! drawn at (see [`crate::render_scale`]) by turns, down a ladder of
//! [`Rung`]s ([`ladder`]), and on the way, if chosen, the multisampling,
//! FXAA instead. None but the multisampling makes a pipeline again, so
//! only it stutters: for a moment, as it goes and as it comes back.

use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::VisibilityRange;
use bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT;
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, WgpuFeatures};
use bevy::render::renderer::{RenderAdapter, RenderDevice};

use super::realm::PortalView;
use super::sky::Sunlight;
use crate::menu::GameState;
use crate::pause::Pause;
use crate::settings::{
    ANTIALIASINGS, Antialiasing, Relief, ScreenRate, Settings, ShadowQuality, WEB,
};
use crate::welcome::GameMode;

/// How much detail the shadows have, from the most to the least.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Four cascades out to 250 m, 2048 texels each: as they always were.
    #[default]
    High,
    /// Three out to 150 m.
    Medium,
    /// Two out to 90 m, 1024 texels each.
    Low,
}

impl Level {
    /// The level a quality chosen fixes, if it does.
    pub fn of(quality: ShadowQuality) -> Option<Self> {
        match quality {
            ShadowQuality::Auto => None,
            ShadowQuality::High => Some(Level::High),
            ShadowQuality::Medium => Some(Level::Medium),
            ShadowQuality::Low => Some(Level::Low),
        }
    }
}

/// The detail the world is drawn with now: the shadows' level, whether the
/// screen is split (and drawn lighter for it), the share of the window's
/// resolution the world is drawn at (%, see [`crate::render_scale`]), and
/// whether the parallax may be drawn (see [`super::relief::ReliefShown`])
/// and the edges multisampled, if chosen.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detail {
    pub level: Level,
    pub split: bool,
    pub scale: u32,
    pub parallax: bool,
    /// Whether the edges may be multisampled, if chosen (otherwise FXAA
    /// smooths them, see [`antialias`]).
    pub msaa: bool,
}

impl Default for Detail {
    fn default() -> Self {
        Self {
            level: Level::High,
            split: false,
            scale: 100,
            parallax: true,
            msaa: true,
        }
    }
}

/// A rung of an automatic quality's ladder: the shadows' level, the share
/// of the window's resolution the world is drawn at (%), whether the
/// parallax may be drawn and whether the edges may be multisampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rung {
    pub level: Level,
    pub scale: u32,
    pub parallax: bool,
    pub msaa: bool,
}

/// A rung of the shadows and the resolution, the relief and the edges as
/// chosen.
const fn rung(level: Level, scale: u32) -> Rung {
    Rung {
        level,
        scale,
        parallax: true,
        msaa: true,
    }
}

/// The shadows alone, automatic (Options > Graphics > Shadows): their three
/// levels, the world at the render scale chosen, the relief and the edges
/// as chosen.
const SHADOWS: [Rung; 3] = [
    rung(Level::High, 100),
    rung(Level::Medium, 100),
    rung(Level::Low, 100),
];
/// The performance forced with one view: the shadows and the resolution down
/// by turns, the shadows first, as they show least (see [`ladder`] for the
/// parallax and the multisampling, given up on the way).
const PERFORMANCE: [Rung; 8] = [
    rung(Level::High, 100),
    rung(Level::Medium, 100),
    rung(Level::Medium, 90),
    rung(Level::Low, 90),
    rung(Level::Low, 80),
    rung(Level::Low, 70),
    rung(Level::Low, 60),
    rung(Level::Low, 50),
];
/// With two views, whose shadows are at their lowest already: the
/// resolution alone.
const PERFORMANCE_SPLIT: [Rung; 6] = [
    rung(Level::Low, 100),
    rung(Level::Low, 90),
    rung(Level::Low, 80),
    rung(Level::Low, 70),
    rung(Level::Low, 60),
    rung(Level::Low, 50),
];

/// Which ladder an automatic quality is on: the performance forced or the
/// shadows alone, two views drawn lighter (`split`) or one, the parallax and
/// the multisampling chosen or not, and the most the shadows and the
/// resolution may be (as chosen: the performance forced lowers them from
/// there).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ladder {
    pub performance: bool,
    pub split: bool,
    pub parallax: bool,
    pub msaa: bool,
    pub level: Level,
    pub scale: u32,
}

impl Default for Ladder {
    fn default() -> Self {
        Self {
            performance: false,
            split: false,
            parallax: false,
            msaa: false,
            level: Level::High,
            scale: 100,
        }
    }
}

/// The ladder an automatic quality goes down and up: the shadows' alone, or
/// the performance forced's (for two views or one): the parallax first, if
/// chosen (the dearest of the relief, over the most pixels, the grass, and
/// the least seen racing: the normal maps stay); then the shadows down a
/// level (one view); then, if chosen, the multisampling, smoothed with
/// FXAA instead (a saving that grows with the pixels, and the edges a
/// little softer), before the resolution; then the resolution and the
/// shadows by turns. From the shadows and the resolution chosen at most
/// (`level`, `scale`): no rung above them. A rung for what is not chosen,
/// or that would change nothing, is left out.
pub fn ladder(on: Ladder) -> Vec<Rung> {
    if !on.performance {
        return SHADOWS.to_vec();
    }
    let base: &[Rung] = if on.split {
        &PERFORMANCE_SPLIT
    } else {
        &PERFORMANCE
    };
    let mut rungs = vec![Rung {
        parallax: on.parallax,
        msaa: on.msaa,
        ..base[0]
    }];
    let mut last = rungs[0];
    if last.parallax {
        last.parallax = false;
        rungs.push(last);
    }
    for &next in &base[1..] {
        if next.scale < 100 && last.msaa {
            last.msaa = false;
            rungs.push(last);
        }
        last = Rung {
            parallax: false,
            msaa: last.msaa,
            ..next
        };
        rungs.push(last);
    }
    // No more than chosen; a rung then as the one above it, left out.
    let mut capped: Vec<Rung> = Vec::with_capacity(rungs.len());
    for rung in rungs {
        let rung = Rung {
            level: rung.level.max(on.level),
            scale: rung.scale.min(on.scale),
            ..rung
        };
        if capped.last() != Some(&rung) {
            capped.push(rung);
        }
    }
    capped
}

/// The frame rate the performance forced keeps up, if it is on: 60 or 120,
/// or fewer if the frame rate cap lets fewer through (at the screen's rate,
/// the screen at `screen` Hz, once known): the most it could ever reach.
pub fn performance_target(settings: &Settings, screen: Option<f32>) -> Option<f32> {
    let forced = settings.force_performance.fps()?;
    Some(
        settings
            .fps
            .bound(screen)
            .map_or(forced, |bound| forced.min(bound)),
    )
}

impl Detail {
    /// The share of their full reach the grass, flowers and petals are
    /// drawn to.
    pub fn reach(self) -> f32 {
        if self.split { 0.55 } else { 1.0 }
    }

    /// From how far from the nearest camera (m) the trees are drawn with
    /// their first lighter mesh, and with their second (see
    /// [`super::lod::level_at`]).
    pub fn tree_levels(self) -> [f32; 2] {
        if self.split {
            [50.0, 150.0]
        } else {
            [100.0, 280.0]
        }
    }

    /// The level the shadows are drawn at: with the screen split, the
    /// lowest.
    fn shadows(self) -> Level {
        if self.split { Level::Low } else { self.level }
    }

    /// The sun's shadow cascades (see [`Level`]).
    pub fn cascades(self) -> CascadeShadowConfig {
        let (num_cascades, first_cascade_far_bound, maximum_distance) = match self.shadows() {
            Level::High => (4, 25.0, 250.0),
            Level::Medium => (3, 20.0, 150.0),
            Level::Low => (2, 20.0, 90.0),
        };
        CascadeShadowConfigBuilder {
            num_cascades,
            first_cascade_far_bound,
            maximum_distance,
            ..default()
        }
        .build()
    }

    /// How many texels wide each shadow cascade is.
    pub fn shadow_map(self) -> usize {
        match self.shadows() {
            Level::Low => 1024,
            _ => 2048,
        }
    }
}

/// How an automatic quality looks at the frame rate and steps: how long
/// (s) each look takes; how long after a change (or the start of a race,
/// or the pause) the frames are not looked at; under what share of the
/// target it goes down, and from what share it keeps up; and whether it
/// goes down several rungs at once when far short.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pace {
    window: f32,
    settle: f32,
    falls_short: f32,
    keeps_up: f32,
    jumps: bool,
}

/// The shadows alone: unhurried.
pub const SHADOWS_PACE: Pace = Pace {
    window: 2.0,
    settle: 3.0,
    falls_short: 0.9,
    keeps_up: 0.97,
    jumps: false,
};
/// The performance forced: a look every second, down as soon as the rate
/// falls a little short, as many rungs as it falls short by.
pub const PERFORMANCE_PACE: Pace = Pace {
    window: 1.0,
    settle: 1.0,
    falls_short: 0.95,
    keeps_up: 0.98,
    jumps: true,
};
/// How much faster each rung down is reckoned to draw, to tell how many
/// to go down at once; and the most at once.
const RUNG_GAIN: f32 = 1.1;
const MOST_AT_ONCE: usize = 3;
/// How long the frames are not looked at after the multisampling goes or
/// comes back (s): every pipeline is made again.
const MSAA_SETTLE: f32 = 3.0;
/// How soon after a step up a step down undoes it (s).
const UNDONE: f32 = 20.0;
/// How long the frame rate must keep up before a step up (s), at first and
/// at most: each step up soon undone doubles it.
const HOLD: (f32, f32) = (10.0, 300.0);

/// An automatic quality: how far down its ladder it has come, and how the
/// frame rate has been going.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct AutoQuality {
    /// The rung (0, the most detail; see [`ladder`]).
    pub step: usize,
    /// The ladder it is on (see [`ladder`]).
    ladder: Ladder,
    /// The frames of the look under way (s), and when it began.
    frames: Vec<f32>,
    began: f32,
    /// Since when the frame rate has kept up.
    steady: Option<f32>,
    hold: f32,
    /// When the last step up was, and the last change (or the end of a
    /// rest), whose frames are not looked at.
    stepped_up: Option<f32>,
    changed: f32,
}

impl Default for AutoQuality {
    fn default() -> Self {
        Self {
            step: 0,
            ladder: Ladder::default(),
            frames: Vec::new(),
            began: 0.0,
            steady: None,
            hold: HOLD.0,
            stepped_up: None,
            changed: f32::NEG_INFINITY,
        }
    }
}

impl AutoQuality {
    /// Not looking (in the menus, or paused): what was being looked at
    /// goes, and the first frames after are let settle.
    pub fn rest(&mut self, now: f32) {
        self.frames.clear();
        self.steady = None;
        self.changed = now;
    }

    /// On `ladder` (see [`ladder`]): if it was on another, back to its top,
    /// with everything it had learnt forgotten.
    pub fn on(&mut self, ladder: Ladder, now: f32) {
        if self.ladder != ladder {
            *self = Self {
                ladder,
                ..Self::default()
            };
            self.rest(now);
        }
    }

    /// A frame of `dt` s, ending at `now` (s), aiming at `target` frames a
    /// second, at `pace`, on a ladder of `rungs`: the rung to draw with.
    /// Every look at the rate (see [`Pace`]): short of the target, a rung
    /// down (or as many as it is short by, if the pace jumps); keeping up
    /// for long enough, a rung up.
    pub fn frame(&mut self, now: f32, dt: f32, target: f32, pace: &Pace, rungs: usize) -> usize {
        let last = rungs.saturating_sub(1);
        self.step = self.step.min(last);
        if now - self.changed < pace.settle {
            return self.step;
        }
        if self.frames.is_empty() {
            self.began = now - dt;
        }
        self.frames.push(dt);
        if now - self.began < pace.window {
            return self.step;
        }
        let rate = self.frames.len() as f32 / self.frames.iter().sum::<f32>().max(1e-6);
        let began = self.began;
        self.frames.clear();
        if rate < target * pace.falls_short {
            self.steady = None;
            if self.step < last {
                // A step up soon undone: wait longer before the next one.
                if self.stepped_up.is_some_and(|up| now - up < UNDONE) {
                    self.hold = (self.hold * 2.0).min(HOLD.1);
                }
                let down = if pace.jumps {
                    let short = (target / rate.max(1.0)).ln() / RUNG_GAIN.ln();
                    (short.ceil() as usize).clamp(1, MOST_AT_ONCE)
                } else {
                    1
                };
                self.step = (self.step + down).min(last);
                self.changed = now;
            }
        } else if rate >= target * pace.keeps_up {
            let steady = *self.steady.get_or_insert(began);
            if now - steady >= self.hold && self.step > 0 {
                self.step -= 1;
                self.stepped_up = Some(now);
                self.changed = now;
                self.steady = None;
            }
        } else {
            self.steady = None;
        }
        self.step
    }
}

pub struct DetailPlugin;

impl Plugin for DetailPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Detail>()
            .init_resource::<AutoQuality>()
            .init_resource::<TwoSamples>()
            .add_systems(Startup, find_two_samples)
            .add_systems(
                PostUpdate,
                (
                    adapt_quality,
                    choose_detail,
                    (shadows_for_detail, reach_for_detail, antialias),
                )
                    .chain(),
            );
    }
}

/// How far an entity with a [`VisibilityRange`] is drawn with one view: its
/// range is this one shortened by [`Detail::reach`].
#[derive(Component, Clone, PartialEq)]
struct FullReach(VisibilityRange);

/// Whether the world is drawn lighter for two views now: two share the
/// screen, and Options > Graphics does not say to draw it all anyway.
fn split(settings: Option<&Settings>, game_mode: Option<&GameMode>) -> bool {
    settings.is_none_or(|settings| settings.lighter_split_screen)
        && game_mode.is_some_and(|mode| *mode == GameMode::SplitScreen)
}

/// With the performance forced or the shadows automatic, the frame rate
/// looked at during a race (not paused), on the ladder that goes with them.
#[allow(clippy::too_many_arguments)]
fn adapt_quality(
    time: Res<Time<Real>>,
    settings: Option<Res<Settings>>,
    screen: Option<Res<ScreenRate>>,
    game_mode: Option<Res<GameMode>>,
    state: Option<Res<State<GameState>>>,
    pause: Option<Res<Pause>>,
    mut auto: ResMut<AutoQuality>,
) {
    let now = time.elapsed_secs();
    let settings = settings.as_deref();
    let on = ladder_on(settings, game_mode.as_deref());
    auto.on(on, now);
    let automatic =
        on.performance || settings.is_some_and(|settings| settings.shadows == ShadowQuality::Auto);
    let racing = state.is_some_and(|state| *state.get() == GameState::Race)
        && pause.is_none_or(|pause| !pause.is_open());
    let (Some(settings), true) = (settings, automatic && racing) else {
        auto.rest(now);
        return;
    };
    let screen = screen.and_then(|screen| screen.0);
    let (target, pace) = match performance_target(settings, screen) {
        Some(target) => (target, &PERFORMANCE_PACE),
        None => (settings.fps.target(), &SHADOWS_PACE),
    };
    let rungs = ladder(on);
    let was = rungs[auto.step.min(rungs.len() - 1)];
    let step = auto.frame(now, time.delta_secs(), target, pace, rungs.len());
    // Multisampling on or off, every pipeline is made again: what they
    // draw vanishes a moment, and the frames are not as they will be.
    if rungs[step].msaa != was.msaa {
        auto.changed = now + MSAA_SETTLE - pace.settle;
    }
}

/// The ladder the settings and the players at play call for.
fn ladder_on(settings: Option<&Settings>, game_mode: Option<&GameMode>) -> Ladder {
    Ladder {
        performance: settings.is_some_and(|settings| settings.force_performance.is_on()),
        split: split(settings, game_mode),
        parallax: settings.is_some_and(|settings| settings.relief == Relief::Parallax),
        msaa: settings.is_some_and(|settings| settings.antialiasing.is_multisampled()),
        level: settings
            .and_then(|settings| Level::of(settings.shadows))
            .unwrap_or(Level::High),
        scale: settings.map_or(100, |settings| settings.render_scale.min(100)),
    }
}

/// What the performance forced may lower, as `settings` say: the
/// resolution scale and the shadows (unless at their lowest already), the
/// multisampling (if chosen) and the parallax (if chosen); nothing while it
/// is off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lowered {
    pub scale: bool,
    pub shadows: bool,
    pub antialiasing: bool,
    pub relief: bool,
}

pub fn lowered(settings: &Settings) -> Lowered {
    if !settings.force_performance.is_on() {
        return Lowered::default();
    }
    let lowest = PERFORMANCE.last().map_or(50, |rung| rung.scale);
    Lowered {
        scale: settings.render_scale > lowest,
        shadows: settings.shadows != ShadowQuality::Low,
        antialiasing: settings.antialiasing.is_multisampled(),
        relief: settings.relief == Relief::Parallax,
    }
}

/// The detail for the players at play: with the performance forced, as far
/// down its ladder as it has come; otherwise the shadows' level chosen, or
/// come to automatically, and the render scale chosen. Split when two share
/// the screen, unless Options > Graphics says to draw it all anyway.
pub fn choose_detail(
    game_mode: Option<Res<GameMode>>,
    settings: Option<Res<Settings>>,
    auto: Res<AutoQuality>,
    mut detail: ResMut<Detail>,
) {
    let settings = settings.as_deref();
    let on = ladder_on(settings, game_mode.as_deref());
    let rungs = ladder(on);
    let reached = rungs[auto.step.min(rungs.len() - 1)];
    let (level, scale, parallax, msaa) = match settings {
        Some(_) if on.performance => (reached.level, reached.scale, reached.parallax, reached.msaa),
        Some(settings) => (
            Level::of(settings.shadows).unwrap_or(reached.level),
            settings.render_scale,
            true,
            true,
        ),
        None => (reached.level, 100, true, true),
    };
    detail.set_if_neq(Detail {
        level,
        split: on.split,
        scale,
        parallax,
        msaa,
    });
}

/// The sun's shadows as the detail says.
fn shadows_for_detail(
    detail: Res<Detail>,
    shadow_map: Option<ResMut<DirectionalLightShadowMap>>,
    mut sunlight: Query<&mut CascadeShadowConfig, With<Sunlight>>,
    mut done: Local<Option<Detail>>,
) {
    if *done == Some(*detail) || sunlight.is_empty() {
        return;
    }
    *done = Some(*detail);
    for mut cascades in &mut sunlight {
        *cascades = detail.cascades();
    }
    if let Some(mut shadow_map) = shadow_map
        && shadow_map.size != detail.shadow_map()
    {
        shadow_map.size = detail.shadow_map();
    }
}

/// Drawn up to `reach` m from the cameras and fading out over `fade` m
/// more, as the grass, the flowers and the petals are: dithered away, as
/// Bevy fades the end of a [`VisibilityRange`]. On the web they end at
/// once instead, halfway through the fade (see [`fading_out_on`]).
pub fn fading_out(reach: f32, fade: f32, use_aabb: bool) -> VisibilityRange {
    fading_out_on(WEB, reach, fade, use_aabb)
}

/// [`fading_out`], with no fade on the `web`: on WebGL2, Bevy (0.19) binds
/// the table of ranges the dithering reads (64 of them, 1024 bytes in its
/// shader) as if it held only one (16 bytes), so the pipeline of every mesh
/// that fades out fails to be made and the game quits before it shows.
/// Ending at once, they read none of it.
fn fading_out_on(web: bool, reach: f32, fade: f32, use_aabb: bool) -> VisibilityRange {
    let end_margin = if web {
        reach + fade / 2.0..reach + fade / 2.0
    } else {
        reach..reach + fade
    };
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin,
        use_aabb,
    }
}

/// `range` shortened to `share` of it.
pub fn shortened(range: &VisibilityRange, share: f32) -> VisibilityRange {
    VisibilityRange {
        start_margin: range.start_margin.start * share..range.start_margin.end * share,
        end_margin: range.end_margin.start * share..range.end_margin.end * share,
        use_aabb: range.use_aabb,
    }
}

/// Everything drawn only so far from the cameras (the grass, the flowers,
/// the petals) as far as the detail says: as it comes, and all of it when
/// the detail changes.
fn reach_for_detail(
    mut commands: Commands,
    detail: Res<Detail>,
    mut ranges: Query<(Entity, &mut VisibilityRange, Option<&FullReach>)>,
) {
    let share = detail.reach();
    for (entity, mut range, full) in &mut ranges {
        let full = match full {
            Some(full) if detail.is_changed() => full.0.clone(),
            Some(_) => continue,
            None => {
                commands.entity(entity).insert(FullReach(range.clone()));
                range.clone()
            }
        };
        let wanted = shortened(&full, share);
        if *range != wanted {
            *range = wanted;
        }
    }
}

/// Whether this machine's GPU draws two samples a pixel, for MSAA 2×
/// ([`Antialiasing::Msaa2`]): WebGPU promises only one or four, and some
/// GPUs have no more (Mesa's llvmpipe), so it is read from the GPU as the
/// game starts; not known (on the web, in tests), it is not. Without it, two
/// samples are not offered, and drawn as four if they come chosen.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TwoSamples(pub bool);

impl TwoSamples {
    /// As the resource says, if there is one.
    pub fn of(two: Option<&TwoSamples>) -> bool {
        two.is_some_and(|two| two.0)
    }
}

/// Whether the GPU draws two samples a pixel of what the cameras
/// multisample (the colour they draw, not HDR, in either of the formats
/// Bevy draws it in, and its depth): wgpu allows a count past one and four
/// only for the adapter's own formats' features, when those are asked for
/// (Bevy asks for all the adapter has).
fn find_two_samples(
    mut two: ResMut<TwoSamples>,
    adapter: Option<Res<RenderAdapter>>,
    device: Option<Res<RenderDevice>>,
) {
    let (Some(adapter), Some(device)) = (adapter, device) else {
        return;
    };
    two.0 = device
        .features()
        .contains(WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        && [
            TextureFormat::Rgba8UnormSrgb,
            TextureFormat::Rgba8Unorm,
            CORE_3D_DEPTH_FORMAT,
        ]
        .into_iter()
        .all(|format| {
            adapter
                .get_texture_format_features(format)
                .flags
                .sample_count_supported(2)
        });
    info!(
        "Two samples a pixel (MSAA 2×): {}",
        if two.0 { "yes" } else { "not on this GPU" }
    );
}

/// The ways of smoothing the edges offered in Options > Graphics: those of
/// [`ANTIALIASINGS`], but two samples a pixel where the GPU has not got them
/// (`two_samples`, see [`TwoSamples`]).
pub fn offered(two_samples: bool) -> Vec<Antialiasing> {
    ANTIALIASINGS
        .into_iter()
        .filter(|&antialiasing| antialiasing != Antialiasing::Msaa2 || two_samples)
        .collect()
}

/// The edges smoothed as Options > Graphics says, on every camera as it
/// comes: all of them (the interface's too, while it draws over what the
/// world's drew on the window) multisampled alike, four, two or one samples
/// a pixel, and SMAA or FXAA on the world's if chosen. With the world drawn
/// smaller, into an image (see [`crate::render_scale`]), the interface's
/// camera draws on its own over the window, with one sample a pixel: told
/// from where the world's cameras draw this frame, so that both change
/// together.
#[allow(clippy::type_complexity)]
fn antialias(
    mut commands: Commands,
    settings: Option<Res<Settings>>,
    detail: Option<Res<Detail>>,
    two_samples: Option<Res<TwoSamples>>,
    cameras: Query<
        (
            Entity,
            Option<&Msaa>,
            Has<Fxaa>,
            Has<Smaa>,
            Has<Camera3d>,
            Option<&RenderTarget>,
        ),
        // A portal view takes its player's camera's (see `super::realm`).
        (With<Camera>, Without<PortalView>),
    >,
) {
    let chosen = settings
        .as_deref()
        .map_or(Antialiasing::default(), |settings| settings.antialiasing);
    let now = in_use(
        chosen,
        detail.is_none_or(|detail| detail.msaa),
        TwoSamples::of(two_samples.as_deref()),
    );
    let (msaa, fxaa, smaa) = match now {
        Antialiasing::Msaa => (Msaa::Sample4, false, false),
        Antialiasing::Msaa2 => (Msaa::Sample2, false, false),
        Antialiasing::Smaa => (Msaa::Off, false, true),
        Antialiasing::Fxaa => (Msaa::Off, true, false),
        Antialiasing::Off => (Msaa::Off, false, false),
    };
    let scaled = cameras
        .iter()
        .any(|(.., world, target)| world && matches!(target, Some(RenderTarget::Image(_))));
    for (camera, current, has_fxaa, has_smaa, world, _) in &cameras {
        let msaa = if scaled && !world { Msaa::Off } else { msaa };
        if current != Some(&msaa) {
            commands.entity(camera).insert(msaa);
        }
        if world && fxaa && !has_fxaa {
            commands.entity(camera).insert(Fxaa::default());
        } else if has_fxaa && !(world && fxaa) {
            commands.entity(camera).remove::<Fxaa>();
        }
        if world && smaa && !has_smaa {
            commands.entity(camera).insert(Smaa::default());
        } else if has_smaa && !(world && smaa) {
            commands.entity(camera).remove::<Smaa>();
        }
    }
}

/// The smoothing of the edges in use: the one `chosen`, but FXAA for the
/// multisampling while it may not be used (`msaa`, the performance forced
/// short of its target); four samples for two where the GPU has not got
/// them (`two_samples`, see [`TwoSamples`]); and FXAA for SMAA on the web,
/// where it cannot be chosen (see [`Antialiasing`]).
pub fn in_use(chosen: Antialiasing, msaa: bool, two_samples: bool) -> Antialiasing {
    let chosen = match chosen {
        Antialiasing::Msaa2 if !two_samples => Antialiasing::Msaa,
        Antialiasing::Smaa if WEB => Antialiasing::Fxaa,
        chosen => chosen,
    };
    if chosen.is_multisampled() && !msaa {
        Antialiasing::Fxaa
    } else {
        chosen
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn what_fades_out_ends_at_once_on_the_web() {
        let desktop = fading_out_on(false, 100.0, 20.0, true);
        assert_eq!(desktop.end_margin, 100.0..120.0);
        assert!(!desktop.is_abrupt());
        // On the web, as far on average, but with no dithering.
        let web = fading_out_on(true, 100.0, 20.0, true);
        assert_eq!(web.end_margin, 110.0..110.0);
        assert!(web.is_abrupt());
        assert!(web.use_aabb);
        // And so it stays with two views.
        assert!(shortened(&web, 0.55).is_abrupt());
        assert_eq!(fading_out(100.0, 20.0, false).is_abrupt(), WEB);
    }

    #[test]
    fn each_level_draws_less_and_two_views_lighter() {
        let at = |level, split| Detail {
            level,
            split,
            scale: 100,
            parallax: true,
            msaa: true,
        };
        let levels = [Level::High, Level::Medium, Level::Low];
        for pair in levels.windows(2) {
            let (more, less) = (at(pair[0], false), at(pair[1], false));
            assert!(less.cascades().bounds.last() < more.cascades().bounds.last());
            assert!(less.cascades().bounds.len() < more.cascades().bounds.len());
            // Nothing else changes with the level: nothing pops in or out.
            assert_eq!(less.reach(), more.reach());
            assert_eq!(less.tree_levels(), more.tree_levels());
        }
        // High as they always were: four cascades out to 250 m, 2048 texels.
        let high = at(Level::High, false);
        assert_eq!(high.cascades().bounds.len(), 4);
        assert_eq!(high.cascades().bounds.last(), Some(&250.0));
        assert_eq!(high.shadow_map(), 2048);
        assert_eq!(high.reach(), 1.0);
        // Split, lighter whatever the level.
        let split = at(Level::High, true);
        assert!(split.reach() < high.reach());
        assert_eq!(split.shadow_map(), 1024);
        assert_eq!(split.cascades().bounds.len(), 2);
        assert!(split.tree_levels()[0] < high.tree_levels()[0]);
    }

    /// `seconds` of frames at `fps` from `from` (s), aiming at 60.
    fn run(auto: &mut AutoQuality, from: f32, seconds: f32, fps: f32) -> f32 {
        let dt = 1.0 / fps;
        let mut now = from;
        while now < from + seconds {
            now += dt;
            auto.frame(now, dt, 60.0, &SHADOWS_PACE, SHADOWS.len());
        }
        now
    }

    /// The shadows' level the automatic quality of the shadows has come to.
    fn level(auto: &AutoQuality) -> Level {
        SHADOWS[auto.step].level
    }

    #[test]
    fn the_automatic_quality_goes_down_while_short_and_back_up_when_it_keeps_up() {
        let mut auto = AutoQuality::default();
        auto.rest(0.0);
        assert_eq!(level(&auto), Level::High);
        // At 45 fps: a level down after the settling and a look, then
        // another once that one has settled; and no lower.
        let now = run(&mut auto, 0.0, 5.5, 45.0);
        assert_eq!(level(&auto), Level::Medium);
        let now = run(&mut auto, now, 5.5, 45.0);
        assert_eq!(level(&auto), Level::Low);
        let now = run(&mut auto, now, 10.0, 45.0);
        assert_eq!(level(&auto), Level::Low);
        // Keeping up at 60: not at once, but after a while, back up.
        let now = run(&mut auto, now, 8.0, 60.0);
        assert_eq!(level(&auto), Level::Low);
        let now = run(&mut auto, now, 10.0, 60.0);
        assert_eq!(level(&auto), Level::Medium);
        // That was too much: down again, and the next step up waits twice
        // as long.
        let now = run(&mut auto, now, 5.5, 45.0);
        assert_eq!(level(&auto), Level::Low);
        let now = run(&mut auto, now, 16.0, 60.0);
        assert_eq!(level(&auto), Level::Low, "not after the first hold");
        run(&mut auto, now, 10.0, 60.0);
        assert_eq!(level(&auto), Level::Medium);
    }

    /// `seconds` of frames at `fps` from `from` (s) with the performance
    /// forced, one view, aiming at 60.
    fn perform(auto: &mut AutoQuality, from: f32, seconds: f32, fps: f32) -> f32 {
        let dt = 1.0 / fps;
        let mut now = from;
        while now < from + seconds {
            now += dt;
            auto.frame(now, dt, 60.0, &PERFORMANCE_PACE, PERFORMANCE.len());
        }
        now
    }

    #[test]
    fn the_performance_forced_goes_down_quickly_as_far_as_it_falls_short() {
        let mut auto = AutoQuality::default();
        let one = Ladder {
            performance: true,
            ..default()
        };
        auto.on(one, 0.0);
        // At 45 fps: after a second settling and a second looking, three
        // rungs down at once (as many as it falls short by, three at most).
        let now = perform(&mut auto, 0.0, 2.1, 45.0);
        assert_eq!(auto.step, 3);
        // A little short: one more, two seconds later.
        let now = perform(&mut auto, now, 2.1, 56.0);
        assert_eq!(auto.step, 4);
        // Keeping up at 60 (a screen's rate): it stays, and after a while
        // goes back up a rung.
        let now = perform(&mut auto, now, 9.0, 60.0);
        assert_eq!(auto.step, 4);
        let now = perform(&mut auto, now, 3.0, 60.0);
        assert_eq!(auto.step, 3);
        // Far short for long: down to the last rung, and no further.
        perform(&mut auto, now, 20.0, 20.0);
        assert_eq!(auto.step, PERFORMANCE.len() - 1);
        // Another ladder (two views): back to its top.
        auto.on(Ladder { split: true, ..one }, 100.0);
        assert_eq!(auto.step, 0);
    }

    #[test]
    fn the_performance_ladder_drops_the_parallax_the_shadows_the_multisampling_and_the_resolution()
    {
        let on = |performance, split, parallax, msaa| Ladder {
            performance,
            split,
            parallax,
            msaa,
            ..default()
        };
        let mut ladders = vec![SHADOWS.to_vec()];
        for split in [false, true] {
            for parallax in [false, true] {
                for msaa in [false, true] {
                    ladders.push(ladder(on(true, split, parallax, msaa)));
                }
            }
        }
        for rungs in &ladders {
            assert_eq!(rungs[0].scale, 100);
            for pair in rungs.windows(2) {
                let (above, below) = (pair[0], pair[1]);
                // Each one down a little less: one thing at a time.
                let changes = [
                    below.level > above.level,
                    below.scale < above.scale,
                    above.parallax && !below.parallax,
                    above.msaa && !below.msaa,
                ];
                assert_eq!(
                    changes.iter().filter(|&&c| c).count(),
                    1,
                    "{above:?} {below:?}"
                );
                assert!(below.level >= above.level && below.scale <= above.scale);
                assert!((above.parallax || !below.parallax) && (above.msaa || !below.msaa));
            }
            // Down to half the resolution.
            assert_eq!(rungs.last().unwrap().scale.min(50), 50);
        }
        // Without the parallax or the multisampling chosen, the shadows and
        // the resolution alone; each chosen, a rung more.
        let plain = ladder(on(true, false, false, false));
        assert_eq!(plain.len(), PERFORMANCE.len());
        assert_eq!(
            ladder(on(true, true, false, false)).len(),
            PERFORMANCE_SPLIT.len()
        );
        let all = ladder(on(true, false, true, true));
        assert_eq!(all.len(), PERFORMANCE.len() + 2);
        // At the top, everything as chosen: the parallax first to go, then
        // the shadows a level, then the multisampling, before the
        // resolution.
        let at = |rung: &Rung| (rung.level, rung.scale, rung.parallax, rung.msaa);
        assert_eq!(
            all.iter().take(5).map(at).collect::<Vec<_>>(),
            [
                (Level::High, 100, true, true),
                (Level::High, 100, false, true),
                (Level::Medium, 100, false, true),
                (Level::Medium, 100, false, false),
                (Level::Medium, 90, false, false),
            ]
        );
        // Two views, whose shadows are low already: the parallax, the
        // multisampling, then the resolution alone.
        let split = ladder(on(true, true, true, true));
        assert!(split.iter().all(|rung| rung.level == Level::Low));
        assert_eq!(
            split.iter().take(4).map(at).collect::<Vec<_>>(),
            [
                (Level::Low, 100, true, true),
                (Level::Low, 100, false, true),
                (Level::Low, 100, false, false),
                (Level::Low, 90, false, false),
            ]
        );
        // The shadows alone leave the rest as chosen.
        assert_eq!(ladder(on(false, true, true, true)), SHADOWS.to_vec());
        assert!(
            SHADOWS
                .iter()
                .all(|rung| rung.parallax && rung.msaa && rung.scale == 100)
        );
    }

    #[test]
    fn the_performance_forced_lowers_from_the_shadows_and_the_scale_chosen() {
        let chosen = |level, scale| Ladder {
            performance: true,
            msaa: true,
            level,
            scale,
            ..default()
        };
        // Medium shadows and 85 % chosen: never above them, and no rung
        // that changes nothing.
        let rungs = ladder(chosen(Level::Medium, 85));
        assert!(
            rungs
                .iter()
                .all(|r| r.level >= Level::Medium && r.scale <= 85)
        );
        assert_eq!(
            rungs[0],
            Rung {
                level: Level::Medium,
                scale: 85,
                parallax: false,
                msaa: true
            }
        );
        for pair in rungs.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
        assert_eq!(rungs.last().unwrap().scale, 50);
        // At the lowest already: the multisampling alone left to give up.
        let lowest = ladder(chosen(Level::Low, 50));
        assert_eq!(lowest.len(), 2);
        assert!(lowest[0].msaa && !lowest[1].msaa);
        // What the options say it may lower.
        let mut settings = Settings {
            force_performance: crate::settings::ForcePerformance::Fps60,
            ..default()
        };
        let first = lowered(&settings);
        assert!(first.scale && first.shadows);
        // As the game starts: FXAA and no relief, nothing of them to lower.
        assert!(!first.antialiasing && !first.relief);
        settings.antialiasing = Antialiasing::Msaa;
        settings.relief = Relief::Parallax;
        settings.render_scale = 50;
        settings.shadows = ShadowQuality::Low;
        let all = lowered(&settings);
        assert!(all.antialiasing && all.relief && !all.scale && !all.shadows);
        // Two samples may be given up as four; SMAA, as FXAA, is kept.
        settings.antialiasing = Antialiasing::Msaa2;
        assert!(lowered(&settings).antialiasing);
        settings.antialiasing = Antialiasing::Smaa;
        assert!(!lowered(&settings).antialiasing);
        settings.force_performance = crate::settings::ForcePerformance::Off;
        assert_eq!(lowered(&settings), Lowered::default());
    }

    #[test]
    fn the_multisampling_given_up_is_fxaa_instead() {
        for samples in [Antialiasing::Msaa, Antialiasing::Msaa2] {
            assert_eq!(in_use(samples, false, true), Antialiasing::Fxaa);
        }
        assert_eq!(in_use(Antialiasing::Msaa, true, true), Antialiasing::Msaa);
        for chosen in [Antialiasing::Fxaa, Antialiasing::Off] {
            assert_eq!(in_use(chosen, false, true), chosen);
        }
        // Two samples as chosen where the GPU has them, four where not.
        assert_eq!(in_use(Antialiasing::Msaa2, true, true), Antialiasing::Msaa2);
        assert_eq!(in_use(Antialiasing::Msaa2, true, false), Antialiasing::Msaa);
        // SMAA as chosen on the desktop; on the web, FXAA.
        let smaa = if WEB {
            Antialiasing::Fxaa
        } else {
            Antialiasing::Smaa
        };
        assert_eq!(in_use(Antialiasing::Smaa, false, true), smaa);
        // Two samples offered only where the GPU has them.
        assert!(!offered(false).contains(&Antialiasing::Msaa2));
        assert_eq!(offered(true), ANTIALIASINGS);
        assert_eq!(
            offered(false).len(),
            ANTIALIASINGS.len() - usize::from(!WEB)
        );
    }

    #[test]
    fn the_performance_forced_keeps_60_or_120_as_far_as_the_cap_and_the_screen_let_it() {
        use crate::settings::{ForcePerformance, FpsLimit};
        let mut settings = Settings::default();
        // Off: nothing kept.
        assert_eq!(performance_target(&settings, Some(60.0)), None);
        // 60, or the cap if lower.
        settings.force_performance = ForcePerformance::Fps60;
        settings.fps = FpsLimit::Unlimited;
        assert_eq!(performance_target(&settings, Some(60.0)), Some(60.0));
        settings.fps = FpsLimit::Cap(30);
        assert_eq!(performance_target(&settings, None), Some(30.0));
        settings.fps = FpsLimit::Cap(144);
        assert_eq!(performance_target(&settings, None), Some(60.0));
        // 120, unless the cap or the screen, at its rate, let fewer through
        // (it would never get there, and come down to the last rung).
        settings.force_performance = ForcePerformance::Fps120;
        settings.fps = FpsLimit::Cap(120);
        assert_eq!(performance_target(&settings, Some(60.0)), Some(120.0));
        settings.fps = FpsLimit::Unlimited;
        assert_eq!(performance_target(&settings, Some(60.0)), Some(120.0));
        settings.fps = FpsLimit::Screen;
        assert_eq!(performance_target(&settings, Some(60.0)), Some(60.0));
        assert_eq!(performance_target(&settings, Some(180.0)), Some(120.0));
        assert_eq!(performance_target(&settings, None), Some(120.0));
    }

    #[test]
    fn the_detail_comes_from_the_performance_forced_or_from_the_options() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<GameMode>()
            .init_resource::<DirectionalLightShadowMap>()
            .insert_resource(Settings {
                shadows: ShadowQuality::Medium,
                render_scale: 70,
                ..default()
            })
            .add_plugins(DetailPlugin);
        app.update();
        let detail = |app: &App| *app.world().resource::<Detail>();
        assert_eq!(
            (detail(&app).level, detail(&app).scale),
            (Level::Medium, 70)
        );
        // With the performance forced, as far down its ladder as it has
        // come, from the shadows and the scale chosen (medium, 70 %): with
        // the multisampling chosen, first without it, then the shadows
        // low, then the resolution.
        {
            let mut settings = app.world_mut().resource_mut::<Settings>();
            settings.force_performance = crate::settings::ForcePerformance::Fps120;
            settings.antialiasing = Antialiasing::Msaa;
        }
        app.update();
        let at = |app: &App| {
            let now = detail(app);
            (now.level, now.scale, now.msaa)
        };
        assert_eq!(at(&app), (Level::Medium, 70, true));
        let steps = [
            (1, (Level::Medium, 70, false)),
            (2, (Level::Low, 70, false)),
            (4, (Level::Low, 50, false)),
        ];
        for (step, wanted) in steps {
            app.world_mut().resource_mut::<AutoQuality>().step = step;
            app.update();
            assert_eq!(at(&app), wanted, "{step}");
        }
        assert!(!detail(&app).parallax);
        // With the parallax chosen, it is the first to go (another ladder:
        // from its top again).
        app.world_mut().resource_mut::<Settings>().relief = Relief::Parallax;
        app.update();
        assert_eq!(app.world().resource::<AutoQuality>().step, 0);
        assert!(detail(&app).parallax);
        app.world_mut().resource_mut::<AutoQuality>().step = 1;
        app.update();
        let now = detail(&app);
        assert_eq!(
            (now.level, now.scale, now.parallax, now.msaa),
            (Level::Medium, 70, false, true)
        );
        // Not forced, the relief and the edges as chosen.
        app.world_mut().resource_mut::<Settings>().force_performance =
            crate::settings::ForcePerformance::Off;
        app.update();
        assert!(detail(&app).parallax && detail(&app).msaa);
    }

    #[test]
    fn resting_lets_the_first_frames_after_settle() {
        let mut auto = AutoQuality::default();
        auto.rest(0.0);
        // A long frame or two as the race starts (pipelines being made)
        // count for nothing.
        auto.frame(0.5, 0.5, 60.0, &SHADOWS_PACE, 3);
        auto.frame(1.5, 1.0, 60.0, &SHADOWS_PACE, 3);
        run(&mut auto, 1.5, 1.4, 60.0);
        assert_eq!(level(&auto), Level::High);
        assert!(auto.frames.is_empty(), "still settling");
    }

    #[test]
    fn what_is_drawn_so_far_comes_nearer_with_two_views_and_back() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<GameMode>()
            .init_resource::<DirectionalLightShadowMap>()
            .insert_resource(Settings::default())
            .add_plugins(DetailPlugin);
        let range = VisibilityRange {
            start_margin: 0.0..0.0,
            end_margin: 100.0..120.0,
            use_aabb: true,
        };
        let grass = app.world_mut().spawn(range.clone()).id();
        let sun = app
            .world_mut()
            .spawn((Sunlight, CascadeShadowConfig::default()))
            .id();
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), Msaa::Sample4))
            .id();
        app.update();
        // (Its margins: a range does not print.)
        let margins =
            |range: &VisibilityRange| (range.start_margin.clone(), range.end_margin.clone());
        let reach = |app: &App| margins(app.world().get::<VisibilityRange>(grass).unwrap());
        let cascades = |app: &App| {
            app.world()
                .get::<CascadeShadowConfig>(sun)
                .unwrap()
                .bounds
                .clone()
        };
        let high = Detail::default();
        let split = Detail {
            split: true,
            ..high
        };
        assert_eq!(reach(&app), margins(&range));
        assert_eq!(cascades(&app), high.cascades().bounds);

        *app.world_mut().resource_mut::<GameMode>() = GameMode::SplitScreen;
        app.update();
        assert_eq!(reach(&app), margins(&shortened(&range, split.reach())));
        assert_eq!(cascades(&app), split.cascades().bounds);
        assert_eq!(
            app.world().resource::<DirectionalLightShadowMap>().size,
            split.shadow_map()
        );
        // Something new comes in shortened too.
        let flowers = app.world_mut().spawn(range.clone()).id();
        app.update();
        assert_eq!(
            app.world()
                .get::<VisibilityRange>(flowers)
                .unwrap()
                .end_margin,
            shortened(&range, split.reach()).end_margin
        );

        // With the option off, two players see it all.
        app.world_mut()
            .resource_mut::<Settings>()
            .lighter_split_screen = false;
        app.update();
        assert_eq!(reach(&app), margins(&range));
        assert_eq!(cascades(&app), high.cascades().bounds);
        app.world_mut()
            .resource_mut::<Settings>()
            .lighter_split_screen = true;
        app.update();
        assert_eq!(cascades(&app), split.cascades().bounds);

        *app.world_mut().resource_mut::<GameMode>() = GameMode::SinglePlayer;
        app.update();
        assert_eq!(reach(&app), margins(&range));
        assert_eq!(cascades(&app), high.cascades().bounds);
        assert_eq!(
            app.world().resource::<DirectionalLightShadowMap>().size,
            2048
        );

        // Shadows chosen low: only the shadows change.
        app.world_mut().resource_mut::<Settings>().shadows = ShadowQuality::Low;
        app.update();
        let low = Detail {
            level: Level::Low,
            ..Detail::default()
        };
        assert_eq!(cascades(&app), low.cascades().bounds);
        assert_eq!(reach(&app), margins(&range));

        // FXAA as the game starts; multisampling instead, two samples,
        // SMAA, none; and back.
        let smoothing = |app: &App| {
            (
                *app.world().get::<Msaa>(camera).unwrap(),
                app.world().get::<Fxaa>(camera).is_some(),
                app.world().get::<Smaa>(camera).is_some(),
            )
        };
        assert_eq!(smoothing(&app), (Msaa::Off, true, false));
        let mut choose = |antialiasing| {
            app.world_mut().resource_mut::<Settings>().antialiasing = antialiasing;
            app.update();
            smoothing(&app)
        };
        assert_eq!(choose(Antialiasing::Msaa), (Msaa::Sample4, false, false));
        // Two samples where the GPU has them, else four.
        assert_eq!(choose(Antialiasing::Msaa2), (Msaa::Sample4, false, false));
        app.insert_resource(TwoSamples(true));
        let mut choose = |antialiasing| {
            app.world_mut().resource_mut::<Settings>().antialiasing = antialiasing;
            app.update();
            smoothing(&app)
        };
        assert_eq!(choose(Antialiasing::Msaa2), (Msaa::Sample2, false, false));
        assert_eq!(choose(Antialiasing::Smaa), (Msaa::Off, WEB, !WEB));
        assert_eq!(choose(Antialiasing::Fxaa), (Msaa::Off, true, false));
        assert_eq!(choose(Antialiasing::Off), (Msaa::Off, false, false));
        assert_eq!(choose(Antialiasing::Msaa), (Msaa::Sample4, false, false));
    }
}
