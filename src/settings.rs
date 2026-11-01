//! Ajustes que se guardan entre partidas: resolución, límite de fotogramas
//! por segundo y controles. Se leen al arrancar y se guardan cada vez que
//! cambian, en `ajustes.ron` dentro de la carpeta de ajustes del sistema (en
//! macOS, `~/Library/Application Support/motorust`); en la web, en el
//! almacenamiento del navegador.

use std::num::NonZero;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::window::{
    Monitor, MonitorSelection, OnMonitor, PresentMode, PrimaryMonitor, PrimaryWindow, VideoMode,
    VideoModeSelection, WindowMode,
};
use serde::{Deserialize, Serialize};

use crate::locale::Language;

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ToggleMute>()
            .init_resource::<ControlsTest>()
            .init_resource::<ScreenRate>()
            .add_systems(
                PreUpdate,
                gamepad_shortcuts.after(bevy::input::InputSystems),
            )
            .add_systems(
                Update,
                (toggle_fullscreen, apply_settings, follow_screen_rate).chain(),
            );
        // En la web, el navegador decide cuándo se dibuja cada fotograma.
        if !WEB {
            // The cap is kept where the frame is shown, in the render world.
            app.init_resource::<FrameCap>()
                .add_plugins(ExtractResourcePlugin::<FrameCap>::default())
                .add_systems(Update, cap_frames);
            if let Some(render) = app.get_sub_app_mut(RenderApp) {
                render.add_systems(Render, limit_frames.in_set(RenderSystems::PostCleanup));
            }
            #[cfg(all(not(target_arch = "wasm32"), not(windows)))]
            app.add_systems(Update, set_window_icon);
        }
    }
}

/// Si el juego corre en el navegador.
pub const WEB: bool = cfg!(target_arch = "wasm32");

/// Todo lo que se puede ajustar.
#[derive(Resource, Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub fullscreen: bool,
    /// How fullscreen covers the screen, where it can be chosen: see
    /// [`Settings::fullscreen_kind`].
    pub fullscreen_kind: FullscreenKind,
    pub resolution: Resolution,
    pub fps: FpsLimit,
    pub language: Language,
    pub controls: Controls,
    /// Volume of the sound effects and of the music (0 to 100 %), see
    /// [`crate::sound::fx_gain`] and [`crate::sound::music_gain`]. They are
    /// saved under new names: the scale changed, and the levels saved with
    /// the old one would now sound different, so they start again from
    /// [`DEFAULT_VOLUME`].
    #[serde(rename = "fx_level")]
    pub fx_volume: u32,
    #[serde(rename = "music_level")]
    pub music_volume: u32,
    pub music_enabled: bool,
    /// How races start: at the lights, or when a player opens the throttle.
    pub start: RaceStart,
    /// How many red lights the start has (see [`crate::race::lights`]).
    pub start_lights: u32,
    /// How long they stay on once all are lit: a moment never quite the
    /// same, or as long as between one and the next.
    pub lights_hold: LightsHold,
    /// At the lights without holding the bikes, how many false starts each
    /// player is let off in a race: the next one disqualifies.
    pub false_starts: u32,
    /// Whether a race's results go to the middle of the screen once
    /// everybody is in (see [`crate::race::results`]); otherwise they stay
    /// at the top right.
    pub centre_results: bool,
    /// Whether the race's whole time so far is shown under the lap's in
    /// the race (see [`crate::race::hud`]).
    pub show_total_time: bool,
    /// How fast the rivals ride, and how much time the checkpoints give.
    pub difficulty: Difficulty,
    /// How the track is seen while it is chosen: from the road, as in the
    /// menus before it, or from above.
    pub track_view: TrackView,
    /// Whether each frame is drawn while the next one is simulated
    /// (Bevy's pipelined rendering): more frames per second when the CPU
    /// holds them back, a frame more between the controls and the screen.
    /// Applied when the game starts.
    pub pipelined_rendering: bool,
    /// Whether the trees are the ones modelled with Meshy, or the classic
    /// ones built in code (see [`crate::world`]).
    pub modelled_trees: bool,
    /// Whether the trees far from the cameras are drawn with lighter meshes
    /// (see [`crate::world`]); off, every tree is drawn whole.
    pub lighter_far_trees: bool,
    /// Whether, with the screen split, the world is drawn lighter: nearer
    /// and coarser shadows, the grass, flowers and petals less far, and the
    /// lighter trees sooner.
    pub lighter_split_screen: bool,
    /// Whether the frame rate is written down every few seconds, to a file
    /// beside the settings (see [`crate::perf_log`]).
    pub performance_log: bool,
    /// How far and how fine the sun's shadows are: chosen, or lowered and
    /// raised as the frame rate asks (see [`crate::world::detail`]).
    pub shadows: ShadowQuality,
    /// How the edges are smoothed (see [`crate::world::detail`]): fixed, as
    /// changing it makes every pipeline again.
    pub antialiasing: Antialiasing,
    /// The relief of the scenery's textures: none, normal maps, or normal
    /// maps and parallax (see [`crate::world::relief`]).
    pub relief: Relief,
    /// Whether the road has relief too (see [`crate::world::relief`]).
    pub road_relief: bool,
    /// Whether the relief is dropped while the players go fast, but the
    /// road's (see [`crate::world::relief::ReliefShown`]).
    pub flat_at_speed: bool,
    /// How the grass's repetition is broken up: not at all, with a wide
    /// texture, or stochastically (see [`crate::world::lawn`]).
    pub grass_tiling: GrassTiling,
    /// How much of the window's resolution the world is drawn at (%), scaled
    /// up to it (see [`crate::render_scale`]); the interface is always drawn
    /// at the window's own.
    pub render_scale: u32,
    /// Whether the shadows and the render scale are lowered and raised as
    /// the frame rate asks, to keep 60 or 120 frames a second (see
    /// [`crate::world::detail`]), instead of as chosen.
    pub force_performance: ForcePerformance,
    /// What the performance HUD shows at the top right: the frame rate, the
    /// pixels the world is drawn at, the smoothing of the edges and the
    /// textures (their relief, the lawn's tiling) in use, and, on the race's
    /// map, where the player is (see [`crate::hud`] and [`crate::minimap`]).
    pub show_fps: bool,
    pub show_resolution: bool,
    pub show_antialiasing: bool,
    pub show_textures: bool,
    pub show_coordinates: bool,
    /// Whether the sea is drawn as a cartoon (flat cartoon blues with waves
    /// rolling in), as it always is on some circuits; otherwise it
    /// is still and dark and mirrors what stands round it, as a dam's lake
    /// (see [`crate::world`]).
    pub cartoon_water: bool,
    /// How many frames may wait to go to the GPU (`desired_maximum_frame_latency`):
    /// fewer, less lag; more, steadier frames. Applied when the game starts.
    pub frame_latency: u32,
    /// How the keyboard and the d-pad steer (Options > Control > Advanced):
    /// as always, or by the curves of Options > Control > Keyboard
    /// behaviour, leaning the bike over and turning it with the lean (see
    /// [`crate::bike::curves`]).
    pub keyboard_mode: crate::bike::curves::KeyboardMode,
    /// Whether the bikes change gears, automatically: none while changing
    /// up, each gear strongest at the revs it pulls best (see
    /// [`crate::bike::gearbox`]), and the gear on the speedometer.
    pub gears: bool,
    /// Whether those curves are drawn under the performance HUD (Options >
    /// Graphics > Performance HUD), with where the first player's bike is
    /// on them (see [`crate::curves_hud`]).
    pub show_keyboard_curves: bool,
    /// The curves' shapes and numbers (saved under a name of their own: the
    /// first ones, saved as a single set of curves, are not read).
    pub keyboard_shapes: crate::bike::curves::KeyboardShapes,
    /// The same for the stick (Options > Control > Advanced and Stick
    /// behaviour), with its response.
    pub stick_mode: crate::bike::curves::KeyboardMode,
    pub stick_shapes: crate::bike::curves::KeyboardShapes,
    /// The best of each track (see [`crate::race::Records`]).
    pub records: crate::race::Records,
    /// Settings tried out and not to be kept (the benchmark's, see
    /// [`crate::bench`]): never saved, and never saved over the player's.
    #[serde(skip)]
    pub transient: bool,
}

/// How hard the races are.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Difficulty {
    Easy,
    /// The rivals as they have always been.
    #[default]
    Medium,
    Hard,
    /// The rivals beyond what they dare on hard: braking later and riding
    /// the corners harder (see [`crate::bike::ai`]).
    VeryHard,
}

/// The difficulties that can be chosen, in order.
pub const DIFFICULTIES: [Difficulty; 4] = [
    Difficulty::Easy,
    Difficulty::Medium,
    Difficulty::Hard,
    Difficulty::VeryHard,
];

impl Difficulty {
    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (Difficulty::Easy, Language::Spanish) => "Fácil",
            (Difficulty::Easy, Language::English) => "Easy",
            (Difficulty::Medium, Language::Spanish) => "Media",
            (Difficulty::Medium, Language::English) => "Medium",
            (Difficulty::Hard, Language::Spanish) => "Difícil",
            (Difficulty::Hard, Language::English) => "Hard",
            (Difficulty::VeryHard, Language::Spanish) => "Muy difícil",
            (Difficulty::VeryHard, Language::English) => "Very hard",
        }
    }

    /// How much of its bike a rival dares to use, for one that uses
    /// `skill` on medium: less on easy, more on hard (never more than all);
    /// on very hard, more than all for most of them: past 1, a rival brakes
    /// later and rides the corners harder, its top speed the bike's (see
    /// [`crate::bike::ai::decide`]).
    pub fn rival_skill(self, skill: f32) -> f32 {
        match self {
            Difficulty::Easy => skill - 0.07,
            Difficulty::Medium => skill,
            Difficulty::Hard => (skill + 0.045).min(1.0),
            Difficulty::VeryHard => skill + VERY_HARD,
        }
    }
}

/// How much more a rival dares on very hard than on medium.
const VERY_HARD: f32 = 0.12;

/// The frame latencies that can be chosen, and Bevy's own.
pub const FRAME_LATENCIES: [u32; 3] = [1, 2, 3];
pub const DEFAULT_FRAME_LATENCY: u32 = 2;

/// The graphics settings the game started with: those applied only when it
/// starts ([`Settings::pipelined_rendering`], [`Settings::frame_latency`]).
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub struct StartedWith {
    pub pipelined_rendering: bool,
    pub frame_latency: u32,
}

impl From<&Settings> for StartedWith {
    fn from(settings: &Settings) -> Self {
        Self {
            pipelined_rendering: settings.pipelined_rendering,
            frame_latency: settings.frame_latency,
        }
    }
}

/// How the track is shown while it is chosen (see
/// [`crate::track_select`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum TrackView {
    /// From the road, as in the menus before it: the camera goes on along
    /// the track it was already flying over, so nothing jumps between the
    /// start menu, the modes and the track.
    #[default]
    Trackside,
    /// From high above, turning slowly round the whole track: the lap is
    /// seen whole, as a map of it.
    Aerial,
}

/// The views that can be chosen, in order.
pub const TRACK_VIEWS: [TrackView; 2] = [TrackView::Trackside, TrackView::Aerial];

impl TrackView {
    /// Whether the track is seen from the road while it is chosen (the
    /// other view is from above).
    pub fn at_the_road(self) -> bool {
        matches!(self, TrackView::Trackside)
    }

    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (TrackView::Trackside, Language::Spanish) => "A pie de pista",
            (TrackView::Trackside, Language::English) => "Trackside",
            (TrackView::Aerial, Language::Spanish) => "Aérea",
            (TrackView::Aerial, Language::English) => "Aerial",
        }
    }
}

/// How a race starts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum RaceStart {
    /// At the lights: five red lights come on one by one and the race
    /// starts when they go out (see [`crate::race::lights`]).
    #[default]
    Lights,
    /// At the lights, but with nothing holding the bikes on the grid: a
    /// player who moves off before they go out is disqualified.
    LightsUnheld,
    /// When a player first opens the throttle.
    Throttle,
}

/// How many red lights the start can have, and how many it has at first,
/// as in a Grand Prix.
pub const START_LIGHTS: [u32; 5] = [1, 2, 3, 4, 5];
pub const DEFAULT_START_LIGHTS: u32 = 5;

/// How long the red lights stay on once all are lit, before they go out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum LightsHold {
    /// A moment never quite the same, as in a Grand Prix: the start cannot
    /// be timed.
    #[default]
    Random,
    /// As long as between one light and the next: the start keeps their
    /// beat.
    Fixed,
}

/// The holds that can be chosen, in order.
pub const LIGHTS_HOLDS: [LightsHold; 2] = [LightsHold::Random, LightsHold::Fixed];

impl LightsHold {
    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (LightsHold::Random, Language::Spanish) => "Aleatoria",
            (LightsHold::Random, Language::English) => "Random",
            (LightsHold::Fixed, Language::Spanish) => "Fija, al compás",
            (LightsHold::Fixed, Language::English) => "Fixed, on the beat",
        }
    }
}

/// How many false starts a player can be let off, and how many at first.
pub const FALSE_STARTS: [u32; 4] = [0, 1, 2, 3];
pub const DEFAULT_FALSE_STARTS: u32 = 2;

/// The starts that can be chosen, in order.
pub const RACE_STARTS: [RaceStart; 3] = [
    RaceStart::Lights,
    RaceStart::LightsUnheld,
    RaceStart::Throttle,
];

impl RaceStart {
    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (RaceStart::Lights, Language::Spanish) => "Semáforo",
            (RaceStart::Lights, Language::English) => "Lights",
            (RaceStart::LightsUnheld, Language::Spanish) => "Semáforo real",
            (RaceStart::LightsUnheld, Language::English) => "Real lights",
            (RaceStart::Throttle, Language::Spanish) => "Acelerando",
            (RaceStart::Throttle, Language::English) => "Throttle",
        }
    }

    /// Whether the race starts at the lights.
    pub fn at_lights(self) -> bool {
        self != RaceStart::Throttle
    }

    /// Whether the bikes are held on the grid until the lights go out.
    pub fn holds(self) -> bool {
        self == RaceStart::Lights
    }
}

/// Volume the effects and the music start at (%).
pub const DEFAULT_VOLUME: u32 = 80;

impl Default for Settings {
    fn default() -> Self {
        Self {
            fullscreen: false,
            fullscreen_kind: FullscreenKind::default(),
            resolution: Resolution::default(),
            fps: FpsLimit::default(),
            language: Language::default(),
            controls: Controls::default(),
            fx_volume: DEFAULT_VOLUME,
            music_volume: DEFAULT_VOLUME,
            music_enabled: true,
            start: RaceStart::default(),
            start_lights: DEFAULT_START_LIGHTS,
            lights_hold: LightsHold::default(),
            false_starts: DEFAULT_FALSE_STARTS,
            centre_results: false,
            show_total_time: false,
            difficulty: Difficulty::default(),
            track_view: TrackView::default(),
            pipelined_rendering: true,
            modelled_trees: true,
            lighter_far_trees: false,
            lighter_split_screen: true,
            performance_log: true,
            shadows: ShadowQuality::default(),
            antialiasing: Antialiasing::default(),
            relief: Relief::default(),
            road_relief: false,
            flat_at_speed: true,
            grass_tiling: GrassTiling::default(),
            render_scale: DEFAULT_RENDER_SCALE,
            force_performance: ForcePerformance::Off,
            show_fps: true,
            show_resolution: false,
            show_antialiasing: false,
            show_textures: false,
            show_coordinates: false,
            cartoon_water: false,
            frame_latency: DEFAULT_FRAME_LATENCY,
            keyboard_mode: crate::bike::curves::KeyboardMode::Classic,
            gears: false,
            show_keyboard_curves: false,
            keyboard_shapes: crate::bike::curves::KeyboardShapes::default(),
            stick_mode: crate::bike::curves::KeyboardMode::Classic,
            stick_shapes: crate::bike::curves::KeyboardShapes::classic(true),
            records: crate::race::Records::default(),
            transient: false,
        }
    }
}

/// Tamaño de la ventana (dinámica o fija en puntos —en una pantalla Retina, el
/// doble de píxeles—).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Resolution {
    #[default]
    Dynamic,
    Window(u32, u32),
    #[doc(hidden)]
    Fullscreen,
}

/// Las resoluciones que se pueden elegir, en orden. En la web, el juego
/// ocupa la página.
pub const RESOLUTIONS: &[Resolution] = if WEB {
    &[Resolution::Dynamic]
} else {
    &[
        Resolution::Dynamic,
        Resolution::Window(1280, 720),
        Resolution::Window(1600, 900),
        Resolution::Window(1920, 1080),
        Resolution::Window(2560, 1440),
    ]
};

impl Resolution {
    pub fn name(self, window: Option<&Window>, lang: Language) -> String {
        if WEB {
            return match window {
                Some(w) if w.physical_width() > 0 && w.physical_height() > 0 => {
                    format!(
                        "{} ({} × {})",
                        lang.dynamic(),
                        w.physical_width(),
                        w.physical_height()
                    )
                }
                _ => lang.dynamic().into(),
            };
        }
        match self {
            Resolution::Dynamic => match window {
                Some(w) if w.physical_width() > 0 && w.physical_height() > 0 => {
                    format!(
                        "{} ({} × {})",
                        lang.dynamic(),
                        w.physical_width(),
                        w.physical_height()
                    )
                }
                _ => lang.dynamic().into(),
            },
            Resolution::Window(width, height) => format!("{width} × {height}"),
            Resolution::Fullscreen => lang.fullscreen().into(),
        }
    }
}

/// Whether the player chooses the kind of fullscreen (on macOS: a Space of
/// its own over the desktop as it is, or the screen switched to a display
/// mode of its own). Elsewhere a fixed resolution switches the screen and a
/// dynamic one does not, as always.
pub const CHOOSE_FULLSCREEN: bool = cfg!(target_os = "macos");

/// How fullscreen covers the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum FullscreenKind {
    /// Without borders over the desktop as it is, at its resolution (on
    /// macOS, in a Space of its own): `WindowMode::BorderlessFullscreen`.
    #[default]
    Desktop,
    /// The screen switched to a display mode of its own, at the resolution
    /// chosen: `WindowMode::Fullscreen`.
    Exclusive,
}

/// The kinds of fullscreen that can be chosen, in order.
pub const FULLSCREEN_KINDS: [FullscreenKind; 2] =
    [FullscreenKind::Desktop, FullscreenKind::Exclusive];

impl FullscreenKind {
    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (FullscreenKind::Exclusive, Language::Spanish) => "Pantalla completa dedicada",
            (FullscreenKind::Exclusive, Language::English) => "Dedicated fullscreen",
            (FullscreenKind::Desktop, Language::Spanish) => "Espacio dedicado (escritorio)",
            (FullscreenKind::Desktop, Language::English) => "Own Space (desktop)",
        }
    }
}

/// Cuántos fotogramas por segundo como mucho: los de la pantalla, un número
/// fijo o tantos como se pueda.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum FpsLimit {
    #[default]
    Screen,
    Cap(u32),
    Unlimited,
}

/// The shadows' quality: automatic (lowered while the frame rate falls
/// short, raised again once it keeps up), or one of the levels, from the
/// most detail to the least (see [`crate::world::detail::Level`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ShadowQuality {
    #[default]
    Auto,
    High,
    Medium,
    Low,
}

/// The shadows' qualities that can be chosen, in order.
pub const SHADOW_QUALITIES: [ShadowQuality; 4] = [
    ShadowQuality::Auto,
    ShadowQuality::High,
    ShadowQuality::Medium,
    ShadowQuality::Low,
];

/// How the edges are smoothed: four or two samples a pixel, SMAA or FXAA
/// over the frame (FXAA the cheaper; as the game starts), or not at all.
/// Two samples and SMAA only on the desktop: WebGL2 and WebGPU promise
/// multisampling at four samples alone, and Bevy's SMAA has not been tried
/// on the web (see [`crate::world::detail::in_use`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Antialiasing {
    Msaa,
    Msaa2,
    Smaa,
    #[default]
    Fxaa,
    Off,
}

impl Antialiasing {
    /// Whether it takes several samples a pixel: changing to or from it
    /// makes every pipeline again.
    pub fn is_multisampled(self) -> bool {
        matches!(self, Antialiasing::Msaa | Antialiasing::Msaa2)
    }
}

/// The ways of smoothing the edges that can be chosen, in order.
#[cfg(not(target_arch = "wasm32"))]
pub const ANTIALIASINGS: [Antialiasing; 5] = [
    Antialiasing::Msaa,
    Antialiasing::Msaa2,
    Antialiasing::Smaa,
    Antialiasing::Fxaa,
    Antialiasing::Off,
];
#[cfg(target_arch = "wasm32")]
pub const ANTIALIASINGS: [Antialiasing; 3] =
    [Antialiasing::Msaa, Antialiasing::Fxaa, Antialiasing::Off];

/// The relief of the scenery's textures (see [`crate::world::relief`]):
/// none, as they always were; normal maps, which light their bumps as the
/// sun falls on them; or normal maps and, on the grass and the surfaces
/// seen close (the walls, the planks, the tunnels' panels, the roofs),
/// parallax, which shifts their bumps with the point of view as well, so
/// they do not look flat going slowly by, at the cost of several more
/// readings of a texture for each of their pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Relief {
    /// As the game starts.
    #[default]
    Off,
    Normal,
    Parallax,
}

/// The reliefs that can be chosen, in order.
pub const RELIEFS: [Relief; 3] = [Relief::Off, Relief::Normal, Relief::Parallax];

/// How the grass's repetition is broken up (see [`crate::world::lawn`]):
/// not at all, its lawn repeating every 11 m as it always did; with a
/// texture four times as wide each way worked out at startup from pieces
/// of it, drawn as the lawn is (more memory, nothing more to draw); or
/// stochastically as it is drawn, pieces of it at random over a grid of
/// triangles (three readings of the texture for each pixel of the grass,
/// and no parallax on it).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum GrassTiling {
    Plain,
    #[default]
    Wide,
    Stochastic,
}

/// The grass tilings that can be chosen, in order.
pub const GRASS_TILINGS: [GrassTiling; 3] = [
    GrassTiling::Plain,
    GrassTiling::Wide,
    GrassTiling::Stochastic,
];

impl GrassTiling {
    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (GrassTiling::Plain, Language::Spanish) => "Ninguna",
            (GrassTiling::Plain, Language::English) => "None",
            (GrassTiling::Wide, Language::Spanish) => "Textura amplia",
            (GrassTiling::Wide, Language::English) => "Wide texture",
            (GrassTiling::Stochastic, Language::Spanish) => "Estocástica",
            (GrassTiling::Stochastic, Language::English) => "Stochastic",
        }
    }
}

impl Relief {
    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (Relief::Off, Language::Spanish) => "No",
            (Relief::Off, Language::English) => "Off",
            (Relief::Normal, Language::Spanish) => "Normales",
            (Relief::Normal, Language::English) => "Normal maps",
            (Relief::Parallax, Language::Spanish) => "Normales y parallax",
            (Relief::Parallax, Language::English) => "Normals and parallax",
        }
    }
}

/// The render scales that can be chosen (% of the window's resolution, see
/// [`Settings::render_scale`]), in order, and the one at first: all of it.
pub const RENDER_SCALES: [u32; 4] = [100, 85, 70, 50];
pub const DEFAULT_RENDER_SCALE: u32 = 100;

impl FpsLimit {
    /// The frame rate the automatic quality aims at: the cap chosen, or 60
    /// (with the screen's rate or unlimited).
    pub fn target(self) -> f32 {
        match self {
            FpsLimit::Cap(fps) => fps as f32,
            FpsLimit::Screen | FpsLimit::Unlimited => 60.0,
        }
    }
}

/// Los límites que se pueden elegir, en orden.
pub const FPS_LIMITS: [FpsLimit; 5] = [
    FpsLimit::Screen,
    FpsLimit::Cap(30),
    FpsLimit::Cap(60),
    FpsLimit::Cap(120),
    FpsLimit::Unlimited,
];

impl FpsLimit {
    pub fn name(self, lang: Language) -> String {
        match self {
            FpsLimit::Screen => lang.fps_screen().into(),
            FpsLimit::Cap(fps) => fps.to_string(),
            FpsLimit::Unlimited => lang.fps_unlimited().into(),
        }
    }

    /// How each frame goes to the screen. At the screen's rate, on its beat
    /// (FifoRelaxed, else Fifo): the image never tears. With a fixed cap,
    /// which [`limit_frames`] keeps, not on the screen's beat: on it, a cap
    /// over the screen's rate could never be reached (120 stayed at 60 on a
    /// BC-250 whose gamescope session ran the screen at 60 Hz), so each
    /// frame is handed over to be shown at the next refresh, the newest one
    /// then (Mailbox), still without tearing. Where there is no Mailbox
    /// (Metal, on macOS, has only Fifo and Immediate) the cap stays on the
    /// screen's beat rather than tear ([`CAP_ON_MAILBOX`]). Unlimited, each
    /// frame shown at once (Immediate first): the least lag, tearing.
    fn present_mode(self) -> PresentMode {
        match self {
            FpsLimit::Screen => PresentMode::AutoVsync,
            FpsLimit::Cap(_) if CAP_ON_MAILBOX => PresentMode::Mailbox,
            FpsLimit::Cap(_) => PresentMode::AutoVsync,
            FpsLimit::Unlimited => PresentMode::AutoNoVsync,
        }
    }

    /// The most frames a second the cap lets through, where the screen at
    /// `screen` Hz (if known) shows them: the cap; at the screen's rate,
    /// its rate; unlimited, no bound.
    pub fn bound(self, screen: Option<f32>) -> Option<f32> {
        match self {
            FpsLimit::Cap(fps) => Some(fps as f32),
            FpsLimit::Screen => screen,
            FpsLimit::Unlimited => None,
        }
    }
}

/// Whether a fixed cap hands its frames over to be shown at the next
/// refresh (Mailbox) rather than on the screen's beat: everywhere but on
/// macOS, where Metal has no Mailbox (Bevy would fall back to Immediate,
/// which tears).
const CAP_ON_MAILBOX: bool = !cfg!(target_os = "macos");

/// Whether the shadows and the render scale follow the frame rate, and the
/// frame rate they keep: off (as chosen), 60 or 120 frames a second.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ForcePerformance {
    #[default]
    Off,
    Fps60,
    Fps120,
}

/// The choices, in order.
pub const FORCE_PERFORMANCES: [ForcePerformance; 3] = [
    ForcePerformance::Off,
    ForcePerformance::Fps60,
    ForcePerformance::Fps120,
];

impl ForcePerformance {
    /// The frame rate kept, if on.
    pub fn fps(self) -> Option<f32> {
        match self {
            ForcePerformance::Off => None,
            ForcePerformance::Fps60 => Some(60.0),
            ForcePerformance::Fps120 => Some(120.0),
        }
    }

    pub fn is_on(self) -> bool {
        self != ForcePerformance::Off
    }

    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (ForcePerformance::Off, Language::Spanish) => "No",
            (ForcePerformance::Off, Language::English) => "Off",
            (ForcePerformance::Fps60, _) => "60 fps",
            (ForcePerformance::Fps120, _) => "120 fps",
        }
    }
}

/// The refresh rate of the screen the window is on (Hz), once known.
#[derive(Resource, Clone, Copy, Default, PartialEq, Debug)]
pub struct ScreenRate(pub Option<f32>);

/// Keeps [`ScreenRate`] as the monitor the window is on (or the main one)
/// says.
fn follow_screen_rate(
    window: Query<Option<&OnMonitor>, With<PrimaryWindow>>,
    monitors: Query<(&Monitor, Has<PrimaryMonitor>)>,
    mut rate: ResMut<ScreenRate>,
) {
    let on = window.iter().next().flatten();
    let monitor = on
        .and_then(|on| monitors.get(on.0).ok())
        .or_else(|| monitors.iter().find(|(_, primary)| *primary))
        .map(|(monitor, _)| monitor);
    let hz = monitor
        .and_then(|monitor| monitor.refresh_rate_millihertz)
        .filter(|&millihertz| millihertz > 0)
        .map(|millihertz| millihertz as f32 / 1000.0);
    rate.set_if_neq(ScreenRate(hz));
}

/// La opción de `options` que va `step` puestos después de `current`, dando la
/// vuelta. Si `current` no está entre ellas, se empieza por la primera.
pub fn cycle<T: Copy + PartialEq>(options: &[T], current: T, step: i32) -> T {
    let index = options.iter().position(|&o| o == current);
    let next = match index {
        Some(index) => (index as i32 + step).rem_euclid(options.len() as i32) as usize,
        None => 0,
    };
    options[next]
}

/// Lo que hace el jugador en la carrera y se puede cambiar de tecla o botón.
/// El stick izquierdo (girar y, hacia atrás, caballito), el derecho (mirar
/// alrededor), M (sonido) y lo que mueve los menús no se cambian.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Throttle,
    Brake,
    Left,
    Right,
    Wheelie,
    Restart,
    Pause,
}

pub const ACTIONS: [Action; 7] = [
    Action::Throttle,
    Action::Brake,
    Action::Left,
    Action::Right,
    Action::Wheelie,
    Action::Restart,
    Action::Pause,
];

impl Action {
    pub fn name(self, lang: Language) -> &'static str {
        match self {
            Action::Throttle => lang.action_throttle(),
            Action::Brake => lang.action_brake(),
            Action::Left => lang.action_left(),
            Action::Right => lang.action_right(),
            Action::Wheelie => lang.action_wheelie(),
            Action::Restart => lang.restart(),
            Action::Pause => lang.action_pause(),
        }
    }
}

/// Las teclas y los botones del mando de una acción: each on its own, or
/// several pressed together.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Binding {
    pub keys: Vec<KeyCode>,
    pub buttons: Vec<GamepadButton>,
    pub key_combos: Vec<Vec<KeyCode>>,
    pub button_combos: Vec<Vec<GamepadButton>>,
}

impl Binding {
    fn new(keys: &[KeyCode], buttons: &[GamepadButton]) -> Self {
        Self {
            keys: keys.to_vec(),
            buttons: buttons.to_vec(),
            ..default()
        }
    }
}

/// A kind of input an action is bound to: the keys of the keyboard or the
/// buttons of a gamepad, each kind remapped on its own.
pub trait Bound: Copy + PartialEq + std::fmt::Debug + 'static {
    fn singles(binding: &Binding) -> &Vec<Self>;
    fn singles_mut(binding: &mut Binding) -> &mut Vec<Self>;
    fn combos(binding: &Binding) -> &Vec<Vec<Self>>;
    fn combos_mut(binding: &mut Binding) -> &mut Vec<Vec<Self>>;
}

impl Bound for KeyCode {
    fn singles(binding: &Binding) -> &Vec<Self> {
        &binding.keys
    }
    fn singles_mut(binding: &mut Binding) -> &mut Vec<Self> {
        &mut binding.keys
    }
    fn combos(binding: &Binding) -> &Vec<Vec<Self>> {
        &binding.key_combos
    }
    fn combos_mut(binding: &mut Binding) -> &mut Vec<Vec<Self>> {
        &mut binding.key_combos
    }
}

impl Bound for GamepadButton {
    fn singles(binding: &Binding) -> &Vec<Self> {
        &binding.buttons
    }
    fn singles_mut(binding: &mut Binding) -> &mut Vec<Self> {
        &mut binding.buttons
    }
    fn combos(binding: &Binding) -> &Vec<Vec<Self>> {
        &binding.button_combos
    }
    fn combos_mut(binding: &mut Binding) -> &mut Vec<Vec<Self>> {
        &mut binding.button_combos
    }
}

/// Whether two combinations are the same keys or buttons, in any order.
fn same_combo<T: PartialEq>(a: &[T], b: &[T]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.contains(x))
}

/// Las teclas y botones de cada acción.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Controls {
    pub throttle: Binding,
    pub brake: Binding,
    pub left: Binding,
    pub right: Binding,
    pub wheelie: Binding,
    pub restart: Binding,
    pub pause: Binding,
}

impl Default for Controls {
    fn default() -> Self {
        use GamepadButton as B;
        use KeyCode as K;
        Self {
            throttle: Binding::new(&[K::KeyW, K::ArrowUp], &[B::RightTrigger2, B::South]),
            brake: Binding::new(&[K::KeyS, K::ArrowDown], &[B::LeftTrigger2, B::West]),
            left: Binding::new(&[K::KeyA, K::ArrowLeft], &[B::DPadLeft]),
            right: Binding::new(&[K::KeyD, K::ArrowRight], &[B::DPadRight]),
            wheelie: Binding::new(&[K::Space, K::ShiftLeft], &[B::East]),
            restart: Binding::new(&[K::KeyR], &[B::Select]),
            pause: Binding::new(&[K::Escape], &[B::Start]),
        }
    }
}

impl Controls {
    pub fn get(&self, action: Action) -> &Binding {
        match action {
            Action::Throttle => &self.throttle,
            Action::Brake => &self.brake,
            Action::Left => &self.left,
            Action::Right => &self.right,
            Action::Wheelie => &self.wheelie,
            Action::Restart => &self.restart,
            Action::Pause => &self.pause,
        }
    }

    fn get_mut(&mut self, action: Action) -> &mut Binding {
        match action {
            Action::Throttle => &mut self.throttle,
            Action::Brake => &mut self.brake,
            Action::Left => &mut self.left,
            Action::Right => &mut self.right,
            Action::Wheelie => &mut self.wheelie,
            Action::Restart => &mut self.restart,
            Action::Pause => &mut self.pause,
        }
    }

    /// The wheelie used to have no gamepad button (it was the left stick
    /// pulled back): settings saved then get B, unless B already does
    /// something else.
    fn give_the_wheelie_a_button(&mut self) {
        let east = GamepadButton::East;
        let taken = ACTIONS
            .iter()
            .any(|&action| self.get(action).buttons.contains(&east));
        if self.wheelie.buttons.is_empty() && !taken {
            self.wheelie.buttons.push(east);
        }
    }

    /// The action `input` does, on its own or with others: the first one
    /// it is bound to.
    pub fn action_of<T: Bound>(&self, input: T) -> Option<Action> {
        ACTIONS
            .into_iter()
            .find(|&action| T::singles(self.get(action)).contains(&input))
            .or_else(|| {
                ACTIONS.into_iter().find(|&action| {
                    T::combos(self.get(action))
                        .iter()
                        .any(|combo| combo.contains(&input))
                })
            })
    }

    /// What `from` did (its action, on its own or the combination it is
    /// in) is done with `to` instead: one key or button, or several pressed
    /// together. `to` stops doing anything else, and the action keeps its
    /// other keys or buttons. The action, if `from` had one.
    pub fn remap<T: Bound>(&mut self, from: T, to: &[T]) -> Option<Action> {
        let action = self.action_of(from)?;
        if to.is_empty() {
            return Some(action);
        }
        // Where `from` was, to put `to` there.
        let binding = self.get_mut(action);
        let single = T::singles(binding).iter().position(|&x| x == from);
        match single {
            Some(index) => {
                T::singles_mut(binding).remove(index);
            }
            None => T::combos_mut(binding).retain(|combo| !combo.contains(&from)),
        }
        for other in ACTIONS {
            let binding = self.get_mut(other);
            match to {
                [one] => T::singles_mut(binding).retain(|x| x != one),
                _ => T::combos_mut(binding).retain(|combo| !same_combo(combo, to)),
            }
        }
        let binding = self.get_mut(action);
        match to {
            [one] => {
                let singles = T::singles_mut(binding);
                let index = single.unwrap_or(singles.len()).min(singles.len());
                singles.insert(index, *one);
            }
            _ => T::combos_mut(binding).push(to.to_vec()),
        }
        Some(action)
    }

    /// Every action back to its keys (or buttons) at first; the other kind
    /// is left as it is.
    pub fn reset<T: Bound>(&mut self) {
        let defaults = Controls::default();
        for action in ACTIONS {
            let default = defaults.get(action);
            let binding = self.get_mut(action);
            *T::singles_mut(binding) = T::singles(default).clone();
            *T::combos_mut(binding) = T::combos(default).clone();
        }
    }

    /// 1 si se pulsa alguna tecla de la acción (or all the keys of one of
    /// its combinations), y si no, 0.
    pub fn key_value(&self, action: Action, keyboard: &ButtonInput<KeyCode>) -> f32 {
        let binding = self.get(action);
        let combo = binding
            .key_combos
            .iter()
            .any(|combo| !combo.is_empty() && keyboard.all_pressed(combo.iter().copied()));
        if combo || keyboard.any_pressed(binding.keys.iter().copied()) {
            1.0
        } else {
            0.0
        }
    }

    /// Cuánto se aprieta en este mando algún botón de la acción (0..1): los
    /// gatillos, según lo que se aprieten; los demás, a tope o nada. A
    /// combination counts once all its buttons are pressed, as much as the
    /// least pressed of them.
    pub fn button_value(&self, action: Action, gamepad: &Gamepad) -> f32 {
        let binding = self.get(action);
        let combos = binding.button_combos.iter().map(|combo| {
            if !combo.is_empty() && combo.iter().all(|&button| gamepad.pressed(button)) {
                combo
                    .iter()
                    .map(|&button| gamepad.get(button).unwrap_or(0.0))
                    .fold(1.0, f32::min)
            } else {
                0.0
            }
        });
        binding
            .buttons
            .iter()
            .map(|&button| gamepad.get(button).unwrap_or(0.0))
            .chain(combos)
            .fold(0.0, f32::max)
    }

    /// Si se acaba de pulsar una tecla o un botón de la acción (or completed
    /// one of its combinations).
    pub fn just_pressed<'a>(
        &self,
        action: Action,
        keyboard: &ButtonInput<KeyCode>,
        gamepads: impl IntoIterator<Item = &'a Gamepad>,
    ) -> bool {
        let binding = self.get(action);
        let key_combo = binding.key_combos.iter().any(|combo| {
            !combo.is_empty()
                && keyboard.all_pressed(combo.iter().copied())
                && keyboard.any_just_pressed(combo.iter().copied())
        });
        key_combo
            || keyboard.any_just_pressed(binding.keys.iter().copied())
            || gamepads.into_iter().any(|gamepad| {
                binding
                    .buttons
                    .iter()
                    .any(|&button| gamepad.just_pressed(button))
                    || binding.button_combos.iter().any(|combo| {
                        !combo.is_empty()
                            && combo.iter().all(|&button| gamepad.pressed(button))
                            && combo.iter().any(|&button| gamepad.just_pressed(button))
                    })
            })
    }

    /// The keys of the action, a combination joined with "+".
    pub fn describe_keys(&self, action: Action, lang: Language) -> String {
        let binding = self.get(action);
        names(&binding.keys, &binding.key_combos, |&key| {
            key_name(key, lang)
        })
    }

    /// The gamepad buttons of the action, a combination joined with "+".
    pub fn describe_buttons(&self, action: Action, lang: Language) -> String {
        let binding = self.get(action);
        // The wheelie always has RT and A together too.
        if action == Action::Wheelie
            && binding.buttons.is_empty()
            && binding.button_combos.is_empty()
        {
            return "RT + A".to_string();
        }
        names(&binding.buttons, &binding.button_combos, |&button| {
            button_name(button, lang)
        })
    }
}

/// The names of some keys or buttons, and of combinations of them, in a list.
fn names<T>(singles: &[T], combos: &[Vec<T>], name: impl Fn(&T) -> String) -> String {
    let list: Vec<String> = singles
        .iter()
        .map(&name)
        .chain(
            combos
                .iter()
                .map(|combo| combo.iter().map(&name).collect::<Vec<_>>().join("+")),
        )
        .collect();
    if list.is_empty() {
        "—".to_string()
    } else {
        list.join(", ")
    }
}

/// Cómo se llama una tecla en pantalla (la fuente no tiene flechas). Los
/// signos son los de la tecla en un teclado estadounidense: el juego conoce
/// el sitio de cada tecla, no lo que pone en ella.
pub fn key_name(key: KeyCode, lang: Language) -> String {
    use KeyCode as K;
    let english = lang == Language::English;
    let name = match key {
        K::ArrowUp => {
            if english {
                "Up"
            } else {
                "Arriba"
            }
        }
        K::ArrowDown => {
            if english {
                "Down"
            } else {
                "Abajo"
            }
        }
        K::ArrowLeft => {
            if english {
                "Left"
            } else {
                "Izquierda"
            }
        }
        K::ArrowRight => {
            if english {
                "Right"
            } else {
                "Derecha"
            }
        }
        K::Space => {
            if english {
                "Space"
            } else {
                "Espacio"
            }
        }
        K::Enter => "Enter",
        K::NumpadEnter => "Enter (num.)",
        K::Escape => "Esc",
        K::Tab => "Tab",
        K::Backspace => {
            if english {
                "Backspace"
            } else {
                "Borrar"
            }
        }
        K::CapsLock => {
            if english {
                "Caps Lock"
            } else {
                "Bloq Mayús"
            }
        }
        K::ShiftLeft => {
            if english {
                "Left Shift"
            } else {
                "Mayús izq."
            }
        }
        K::ShiftRight => {
            if english {
                "Right Shift"
            } else {
                "Mayús der."
            }
        }
        K::ControlLeft => {
            if english {
                "Left Ctrl"
            } else {
                "Ctrl izq."
            }
        }
        K::ControlRight => {
            if english {
                "Right Ctrl"
            } else {
                "Ctrl der."
            }
        }
        K::AltLeft => {
            if english {
                "Left Alt"
            } else {
                "Alt izq."
            }
        }
        K::AltRight => {
            if english {
                "Right Alt"
            } else {
                "Alt der."
            }
        }
        K::SuperLeft if cfg!(target_os = "macos") => {
            if english {
                "Left Cmd"
            } else {
                "Cmd izq."
            }
        }
        K::SuperRight if cfg!(target_os = "macos") => {
            if english {
                "Right Cmd"
            } else {
                "Cmd der."
            }
        }
        K::Comma => ",",
        K::Period => ".",
        K::Minus => "-",
        K::Equal => "=",
        K::Slash => "/",
        K::Backslash => "\\",
        K::Semicolon => ";",
        K::Quote => "'",
        K::BracketLeft => "[",
        K::BracketRight => "]",
        K::Backquote => "`",
        _ => {
            // KeyW, Digit1, Numpad1, F1...: sin el prefijo.
            let name = format!("{key:?}");
            return match name.strip_prefix("Key").or(name.strip_prefix("Digit")) {
                Some(short) => short.to_string(),
                None => match name.strip_prefix("Numpad") {
                    Some(short) => format!("{short} (num.)"),
                    None => name,
                },
            };
        }
    };
    name.into()
}

/// Cómo se llama un botón del mando en pantalla: como en los de Xbox.
pub fn button_name(button: GamepadButton, lang: Language) -> String {
    use GamepadButton as B;
    let english = lang == Language::English;
    let name = match button {
        B::South => "A",
        B::East => "B",
        B::West => "X",
        B::North => "Y",
        B::LeftTrigger => "LB",
        B::RightTrigger => "RB",
        B::LeftTrigger2 => "LT",
        B::RightTrigger2 => "RT",
        B::Select => "Select",
        B::Start => "Start",
        B::Mode => "Home",
        B::LeftThumb => "L3",
        B::RightThumb => "R3",
        B::DPadUp => {
            if english {
                "D-pad up"
            } else {
                "Cruceta arriba"
            }
        }
        B::DPadDown => {
            if english {
                "D-pad down"
            } else {
                "Cruceta abajo"
            }
        }
        B::DPadLeft => {
            if english {
                "D-pad left"
            } else {
                "Cruceta izq."
            }
        }
        B::DPadRight => {
            if english {
                "D-pad right"
            } else {
                "Cruceta der."
            }
        }
        other => return format!("{other:?}"),
    };
    name.into()
}

impl Settings {
    /// La ventana del juego con estos ajustes.
    pub fn window(&self) -> Window {
        let mut window = Window {
            title: crate::game_info::info().title.into(),
            // En la web se dibuja en el canvas de la página y lo llena (the
            // canvas with the game's folder's name as its id).
            canvas: WEB.then(|| format!("#{}", crate::game_info::info().folder)),
            fit_canvas_to_parent: true,
            // As chosen in Options (Bevy's 2 at first; 1 is less lag).
            desired_maximum_frame_latency: NonZero::new(
                if FRAME_LATENCIES.contains(&self.frame_latency) {
                    self.frame_latency
                } else {
                    DEFAULT_FRAME_LATENCY
                },
            ),
            ..default()
        };
        self.configure(&mut window, None);
        window
    }

    /// The kind of fullscreen used: the one chosen where it can be chosen
    /// ([`CHOOSE_FULLSCREEN`]); elsewhere the screen is switched for a fixed
    /// resolution and not for a dynamic one; never in the browser.
    pub fn fullscreen_kind(&self) -> FullscreenKind {
        self.kind_of_fullscreen(CHOOSE_FULLSCREEN)
    }

    fn kind_of_fullscreen(&self, chosen: bool) -> FullscreenKind {
        if WEB {
            FullscreenKind::Desktop
        } else if chosen {
            self.fullscreen_kind
        } else if matches!(self.resolution, Resolution::Window(..)) {
            FullscreenKind::Exclusive
        } else {
            FullscreenKind::Desktop
        }
    }

    /// Whether the screen itself is switched to a display mode of its own.
    /// Then the monitor's display modes are needed, known only once the
    /// window is open.
    fn exclusive(&self) -> bool {
        self.fullscreen && self.fullscreen_kind() == FullscreenKind::Exclusive
    }

    /// The pixels a fixed resolution draws the world at, at most, in a
    /// fullscreen over the desktop, where the screen is not switched to it
    /// and the window covers it as it is (see
    /// [`crate::render_scale::NativeShare`]): there it changed nothing
    /// before. `None` otherwise: in a window it is the window's size, in a
    /// dedicated fullscreen the screen's display mode.
    pub fn desktop_resolution(&self) -> Option<UVec2> {
        self.desktop_resolution_as(self.fullscreen_kind())
    }

    fn desktop_resolution_as(&self, kind: FullscreenKind) -> Option<UVec2> {
        match self.resolution {
            Resolution::Window(width, height)
                if !WEB && self.fullscreen && kind == FullscreenKind::Desktop =>
            {
                Some(UVec2::new(width, height))
            }
            _ => None,
        }
    }

    /// Sets `window` as these settings ask. In a dedicated fullscreen, the
    /// screen is switched to the display mode of `monitor` that comes
    /// closest to the resolution chosen (in pixels), or with a dynamic one
    /// to its own size; otherwise, or with no monitor known yet, the window
    /// covers the screen as it is.
    fn configure(&self, window: &mut Window, monitor: Option<&Monitor>) {
        self.configure_as(window, monitor, self.exclusive());
    }

    /// As [`Self::configure`], switching the screen or not as `exclusive`
    /// says.
    fn configure_as(&self, window: &mut Window, monitor: Option<&Monitor>, exclusive: bool) {
        if self.fullscreen {
            let mode = match monitor {
                Some(monitor) if exclusive => match self.resolution {
                    Resolution::Window(width, height) => {
                        fitting_mode(&monitor.video_modes, width, height)
                    }
                    Resolution::Dynamic | Resolution::Fullscreen => fitting_mode(
                        &monitor.video_modes,
                        monitor.physical_width,
                        monitor.physical_height,
                    ),
                },
                _ => None,
            };
            window.mode = match mode {
                Some(mode) => WindowMode::Fullscreen(
                    MonitorSelection::Current,
                    VideoModeSelection::Specific(mode),
                ),
                None => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            };
        } else {
            window.mode = WindowMode::Windowed;
            match self.resolution {
                Resolution::Dynamic | Resolution::Fullscreen => {}
                Resolution::Window(width, height) => {
                    if !WEB {
                        window.resolution.set(width as f32, height as f32);
                    }
                }
            }
        }
        window.present_mode = self.fps.present_mode();
    }

    /// Los ajustes guardados, o los de siempre si no hay o no se entienden.
    /// Se lee antes de abrir la ventana, cuando aún no hay registro, así que
    /// los problemas se escriben directamente en la terminal.
    pub fn load() -> Self {
        let Some(text) = storage::read() else {
            return Self::default();
        };
        let settings = ron::from_str::<Self>(&text).unwrap_or_else(|error| {
            eprintln!(
                "No se entienden los ajustes de {}: {error}. Se usan los de siempre",
                storage::place()
            );
            Self::default()
        });
        settings.brought_up_to_date(&text)
    }

    /// These settings, read from `text` as saved by an earlier version,
    /// with what has changed since put as it would be now.
    fn brought_up_to_date(self, text: &str) -> Self {
        let mut settings = self;
        if settings.resolution == Resolution::Fullscreen {
            settings.fullscreen = true;
            settings.resolution = Resolution::Dynamic;
        }
        // Saved before the kind of fullscreen could be chosen: in fullscreen
        // a fixed resolution switched the screen.
        if !text.contains("fullscreen_kind")
            && matches!(settings.resolution, Resolution::Window(..))
        {
            settings.fullscreen_kind = FullscreenKind::Exclusive;
        }
        // Saved when the performance mode was on or off, at 60 fps.
        if text.contains("performance_mode: true") && !text.contains("force_performance") {
            settings.force_performance = ForcePerformance::Fps60;
        }
        settings.controls.give_the_wheelie_a_button();
        // The curves were turned on with a box before the keyboard's
        // behaviour could be chosen.
        if text.contains("keyboard_curves_on: true") && !text.contains("keyboard_mode") {
            settings.keyboard_mode = crate::bike::curves::KeyboardMode::Bezier;
        }
        settings
    }

    fn save(&self) {
        if self.transient {
            return;
        }
        let saved = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|error| error.to_string())
            .and_then(|text| storage::write(&text));
        if let Err(error) = saved {
            warn!(
                "No se pueden guardar los ajustes en {}: {error}",
                storage::place()
            );
        }
    }
}

/// Los ajustes, en un archivo de la carpeta de ajustes del sistema.
#[cfg(not(target_arch = "wasm32"))]
mod storage {
    use std::path::PathBuf;

    /// Lo guardado, si hay algo.
    pub fn read() -> Option<String> {
        let path = path()?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                eprintln!(
                    "No se pueden leer los ajustes de {}: {error}",
                    path.display()
                );
                None
            }
        }
    }

    /// Guarda el texto. Si el sistema no tiene carpeta de ajustes, no se
    /// guarda en ningún sitio.
    pub fn write(text: &str) -> Result<(), String> {
        let Some(path) = path() else {
            return Ok(());
        };
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, text).map_err(|error| error.to_string())
    }

    /// Dónde se guardan, para los mensajes.
    pub fn place() -> String {
        path().map_or_else(|| "ningún sitio".into(), |path| path.display().to_string())
    }

    /// Dónde se guardan los ajustes en cada sistema.
    fn path() -> Option<PathBuf> {
        let var = |name| std::env::var_os(name).map(PathBuf::from);
        let folder = if cfg!(target_os = "macos") {
            var("HOME")?.join("Library/Application Support")
        } else if cfg!(windows) {
            var("APPDATA")?
        } else {
            match var("XDG_CONFIG_HOME").filter(|path| path.is_absolute()) {
                Some(folder) => folder,
                None => var("HOME")?.join(".config"),
            }
        };
        Some(
            folder
                .join(crate::game_info::info().folder)
                .join("ajustes.ron"),
        )
    }
}

/// Los ajustes, en el almacenamiento del navegador (`localStorage`), que
/// guarda lo de cada página aunque se cierre.
#[cfg(target_arch = "wasm32")]
mod storage {
    const KEY: &str = "motorust.ajustes";

    fn local() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    /// Lo guardado, si hay algo.
    pub fn read() -> Option<String> {
        local()?.get_item(KEY).ok()?
    }

    /// Guarda el texto.
    pub fn write(text: &str) -> Result<(), String> {
        local()
            .and_then(|storage| storage.set_item(KEY, text).ok())
            .ok_or_else(|| "el navegador no deja guardar".into())
    }

    /// Dónde se guardan, para los mensajes.
    pub fn place() -> String {
        "el navegador".into()
    }
}

/// Si se acaban de pulsar las teclas de la pantalla completa: Cmd+Enter en
/// macOS y Alt+Enter en Windows y Linux. En la web valen las dos, que no se
/// sabe en qué sistema corre el navegador. Ese Enter no cuenta para los
/// menús.
pub fn fullscreen_keys(keyboard: &ButtonInput<KeyCode>) -> bool {
    let command = keyboard.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]);
    let alt = keyboard.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let held = if cfg!(target_os = "macos") {
        command
    } else if WEB {
        command || alt
    } else {
        alt
    };
    held && keyboard.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter])
}

/// The shortcuts of a gamepad: Select held with R3 (the right stick
/// pressed in) switches fullscreen, with Y (the top face button) the sound
/// off and on.
pub const SHORTCUT_HELD: GamepadButton = GamepadButton::Select;
pub const FULLSCREEN_BUTTON: GamepadButton = GamepadButton::RightThumb;
pub const MUTE_BUTTON: GamepadButton = GamepadButton::North;

/// While the controls are tried out (Options > Control > Mapping, Test),
/// no key or button works anything: the shortcuts (Select with R3 or with
/// Y, Alt or Cmd with Enter, M) are tried out like any others, as the
/// menu's (the test ends with Esc, or B held down, see
/// [`crate::mapping`]).
#[derive(Resource, Clone, Copy, Default, Debug)]
pub struct ControlsTest {
    /// Whether they are being tried out.
    pub on: bool,
}

/// Asks for the sound to be turned off, or back on (M, or Select and Y).
#[derive(Message, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ToggleMute;

/// What a gamepad shortcut asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shortcut {
    Fullscreen,
    Mute,
}

/// How a gamepad's Select is being held: since it went down, and whether
/// a shortcut was made with it.
#[derive(Clone, Copy, Default, Debug)]
pub struct ShortcutHold {
    held: bool,
    used: bool,
    /// A press of Select alone was handed on: it is let go of next.
    handed_on: bool,
}

/// Reads the shortcuts of a gamepad's buttons, taking their presses out
/// of them so that nothing else acts on them: Select (which restarts the
/// race) goes down unseen and, if no shortcut is made while it is held,
/// counts as pressed when it is let go of; R3 and Y held with Select are
/// the shortcut's.
pub fn read_shortcut(
    buttons: &mut ButtonInput<GamepadButton>,
    hold: &mut ShortcutHold,
) -> Option<Shortcut> {
    if hold.handed_on {
        buttons.release(SHORTCUT_HELD);
        buttons.clear_just_released(SHORTCUT_HELD);
        hold.handed_on = false;
    }
    if buttons.clear_just_pressed(SHORTCUT_HELD) {
        *hold = ShortcutHold {
            held: true,
            ..default()
        };
    }
    let mut shortcut = None;
    if hold.held && buttons.pressed(SHORTCUT_HELD) {
        if buttons.clear_just_pressed(FULLSCREEN_BUTTON) {
            shortcut = Some(Shortcut::Fullscreen);
        } else if buttons.clear_just_pressed(MUTE_BUTTON) {
            shortcut = Some(Shortcut::Mute);
        }
        hold.used |= shortcut.is_some();
    }
    if hold.held && buttons.just_released(SHORTCUT_HELD) {
        hold.held = false;
        if !hold.used {
            // Select alone: pressed now, for a frame.
            buttons.press(SHORTCUT_HELD);
            hold.handed_on = true;
        }
    }
    shortcut
}

/// The gamepads' shortcuts: fullscreen and sound (but not while the
/// controls are tried out).
fn gamepad_shortcuts(
    mut gamepads: Query<(Entity, &mut Gamepad)>,
    mut holds: Local<bevy::platform::collections::HashMap<Entity, ShortcutHold>>,
    mut settings: ResMut<Settings>,
    test: Res<ControlsTest>,
    mut mute: MessageWriter<ToggleMute>,
) {
    for (entity, mut gamepad) in &mut gamepads {
        let hold = holds.entry(entity).or_default();
        match read_shortcut(gamepad.bypass_change_detection().digital_mut(), hold) {
            Some(_) if test.on => {}
            Some(Shortcut::Fullscreen) => settings.fullscreen = !settings.fullscreen,
            Some(Shortcut::Mute) => {
                mute.write(ToggleMute);
            }
            None => {}
        }
    }
}

/// With the fullscreen keys ([`fullscreen_keys`]), switches fullscreen,
/// kept as if chosen in Options. Not while the controls are tried out.
fn toggle_fullscreen(
    keyboard: Res<ButtonInput<KeyCode>>,
    test: Res<ControlsTest>,
    mut settings: ResMut<Settings>,
) {
    if test.on || !fullscreen_keys(&keyboard) {
        return;
    }
    settings.fullscreen = !settings.fullscreen;
}

/// The display mode of a monitor that comes closest to `width` × `height`
/// pixels: the nearest size (the same one if there is), then the fastest
/// refresh rate and the deepest colour.
pub fn fitting_mode(modes: &[VideoMode], width: u32, height: u32) -> Option<VideoMode> {
    modes.iter().copied().min_by_key(|mode| {
        let size = mode.physical_size;
        (
            size.x.abs_diff(width) + size.y.abs_diff(height),
            std::cmp::Reverse(mode.refresh_rate_millihertz),
            std::cmp::Reverse(mode.bit_depth),
        )
    })
}

/// Al cambiar los ajustes se guardan y, si cambian la pantalla completa, la
/// resolución o el límite de FPS, la ventana se pone como piden.
#[allow(clippy::type_complexity)]
fn apply_settings(
    settings: Res<Settings>,
    mut window: Query<(&mut Window, Option<&OnMonitor>), With<PrimaryWindow>>,
    monitors: Query<(&Monitor, Has<PrimaryMonitor>)>,
    mut applied: Local<Option<(bool, FullscreenKind, Resolution, FpsLimit)>>,
) {
    let wanted = (
        settings.fullscreen,
        settings.fullscreen_kind(),
        settings.resolution,
        settings.fps,
    );
    // La ventana ya se abrió con los ajustes guardados; salvo que la
    // pantalla deba pasar a la resolución elegida, que espera a conocer los
    // modos del monitor.
    let applied = applied.get_or_insert(if settings.exclusive() {
        (
            false,
            FullscreenKind::Desktop,
            Resolution::Dynamic,
            settings.fps,
        )
    } else {
        wanted
    });
    if *applied != wanted
        && let Ok((mut window, on)) = window.single_mut()
    {
        // The monitor the window is on, or else the main one.
        let monitor = on
            .and_then(|on| monitors.get(on.0).ok())
            .or_else(|| monitors.iter().find(|(_, primary)| *primary))
            .map(|(monitor, _)| monitor);
        if monitor.is_some() || !settings.exclusive() {
            settings.configure(&mut window, monitor);
            *applied = wanted;
        }
    }
    if settings.is_changed() && !settings.is_added() {
        settings.save();
    }
}

/// The frame rate cap chosen, if any ([`FpsLimit::Cap`]), copied into the
/// render world, where [`limit_frames`] keeps to it.
#[derive(Resource, Clone, Copy, Default, PartialEq, Eq, Debug, ExtractResource)]
pub struct FrameCap(pub Option<u32>);

fn cap_frames(settings: Res<Settings>, mut cap: ResMut<FrameCap>) {
    let wanted = FrameCap(match settings.fps {
        FpsLimit::Cap(fps) => Some(fps),
        FpsLimit::Screen | FpsLimit::Unlimited => None,
    });
    if *cap != wanted {
        *cap = wanted;
    }
}

/// With a fixed cap, waits whatever is left of each frame so as not to go
/// over it. It counts from the previous deadline, not from when the wait
/// ended, so the pace does not drift.
///
/// It waits right after the frame has been shown, in the render world:
/// then the next frame starts at once, reading the controls as they are
/// after the wait, and is shown as soon as it is drawn. Waiting before the
/// frame is drawn (at the end of the main world's update) would show
/// controls read before the wait, that much later.
fn limit_frames(cap: Res<FrameCap>, mut deadline: Local<Option<Instant>>) {
    let FrameCap(Some(fps)) = *cap else {
        *deadline = None;
        return;
    };
    let frame = Duration::from_secs_f64(1.0 / f64::from(fps.max(1)));
    let now = Instant::now();
    *deadline = Some(match *deadline {
        Some(time) if time > now => {
            std::thread::sleep(time - now);
            time + frame
        }
        // El primero, o uno que ya llega tarde: se empieza a contar de nuevo.
        _ => now + frame,
    });
}

/// En Linux y macOS, pone el icono de la ventana en cuanto el manejador de
/// ventanas de winit esté disponible. En Windows se incrusta en el ejecutable
/// con winres y el sistema operativo lo aplica directamente.
#[cfg(all(not(target_arch = "wasm32"), not(windows)))]
fn set_window_icon(
    primary_window: Query<Entity, With<PrimaryWindow>>,
    windows: Option<NonSend<bevy::winit::WinitWindows>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(windows) = windows else {
        return;
    };
    let Ok(entity) = primary_window.single() else {
        return;
    };
    let Some(window) = windows.get_window(entity) else {
        return;
    };
    let Some(icon) = crate::game_info::info().icon else {
        *done = true;
        return;
    };
    if let Ok(image) = image::load_from_memory(crate::content::file(icon)) {
        let rgba = image.into_rgba8();
        let (width, height) = rgba.dimensions();
        if let Ok(icon) = winit::window::Icon::from_rgba(rgba.into_raw(), width, height) {
            window.set_window_icon(Some(icon));
        }
    }
    *done = true;
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_saving_and_loading() {
        let mut settings = Settings {
            fullscreen: true,
            resolution: Resolution::Dynamic,
            fps: FpsLimit::Cap(60),
            language: Language::Spanish,
            ..default()
        };
        settings.controls.remap(KeyCode::Space, &[KeyCode::KeyJ]);
        settings
            .controls
            .remap(GamepadButton::South, &[GamepadButton::RightTrigger]);
        settings
            .controls
            .remap(KeyCode::ShiftLeft, &[KeyCode::ControlLeft, KeyCode::KeyW]);
        // The records and the start are kept too.
        settings.records.lap("Bahía GP", 71.25);
        settings.records.checkpoint("Bahía GP", 3);
        settings.start = RaceStart::Throttle;
        settings.difficulty = Difficulty::Hard;
        settings.pipelined_rendering = false;
        settings.frame_latency = 1;
        let text = ron::ser::to_string_pretty(&settings, default()).unwrap();
        let loaded: Settings = ron::from_str(&text).unwrap();
        assert_eq!(loaded, settings);
        // Un archivo a medias (de una versión anterior, por ejemplo) completa
        // lo que falta con lo de siempre.
        let partial: Settings = ron::from_str("(fps: Unlimited)").unwrap();
        assert_eq!(partial.fps, FpsLimit::Unlimited);
        assert_eq!(partial.controls, Controls::default());
        assert_eq!(partial.start, RaceStart::Lights);
        assert_eq!(partial.start_lights, DEFAULT_START_LIGHTS);
        assert_eq!(partial.lights_hold, LightsHold::Random);
        assert_eq!(partial.false_starts, DEFAULT_FALSE_STARTS);
        assert!(!partial.centre_results);
        assert!(partial.modelled_trees);
        // As are the lighter far trees (off), less detail for two players
        // and the performance log (on).
        assert!(!partial.lighter_far_trees);
        assert!(partial.lighter_split_screen && partial.performance_log);
        // At the lights, held or not; or at the throttle.
        assert!(RaceStart::Lights.at_lights() && RaceStart::Lights.holds());
        assert!(RaceStart::LightsUnheld.at_lights() && !RaceStart::LightsUnheld.holds());
        assert!(!RaceStart::Throttle.at_lights() && !RaceStart::Throttle.holds());
        // Medium, pipelined rendering and Bevy's frame latency at first.
        assert_eq!(partial.difficulty, Difficulty::Medium);
        assert!(partial.pipelined_rendering);
        assert_eq!(partial.frame_latency, 2);
        assert_eq!(
            partial.window().desired_maximum_frame_latency,
            NonZero::new(2)
        );
        // One that still keeps a championship under way (the arcade no
        // longer does) loads without it.
        let older: Settings = ron::from_str(
            "(records: (story: Some((round: 2, points: (10, 8, 6, 5, 4, 3, 2, 1))), \
             best_laps: [(\"Bahía GP\", 71.25)]))",
        )
        .unwrap();
        assert_eq!(older.records.best_lap("Bahía GP"), Some(71.25));
    }

    #[test]
    fn the_road_starts_without_relief_and_the_rest_drops_it_at_speed() {
        let first = Settings::default();
        assert!(!first.road_relief && first.flat_at_speed);
        // Saved before they were options: as at first.
        let older: Settings = ron::from_str("(relief: Parallax)").unwrap();
        assert!(!older.road_relief && older.flat_at_speed);
        let text = ron::ser::to_string(&Settings {
            road_relief: true,
            flat_at_speed: false,
            ..default()
        })
        .unwrap();
        let read: Settings = ron::from_str(&text).unwrap();
        assert!(read.road_relief && !read.flat_at_speed);
    }

    #[test]
    fn the_grass_starts_with_the_wide_texture_and_is_kept() {
        assert_eq!(Settings::default().grass_tiling, GrassTiling::Wide);
        let older: Settings = ron::from_str("(fps: Unlimited)").unwrap();
        assert_eq!(older.grass_tiling, GrassTiling::Wide);
        for tiling in GRASS_TILINGS {
            let text = ron::ser::to_string(&Settings {
                grass_tiling: tiling,
                ..default()
            })
            .unwrap();
            assert_eq!(
                ron::from_str::<Settings>(&text).unwrap().grass_tiling,
                tiling
            );
        }
        assert_eq!(GrassTiling::Plain.name(Language::Spanish), "Ninguna");
    }

    #[test]
    fn every_smoothing_is_kept_and_only_the_samples_are_multisampled() {
        for antialiasing in ANTIALIASINGS {
            let text = ron::ser::to_string(&Settings {
                antialiasing,
                ..default()
            })
            .unwrap();
            assert_eq!(
                ron::from_str::<Settings>(&text).unwrap().antialiasing,
                antialiasing
            );
        }
        // Two samples and SMAA on the desktop, between four samples and FXAA.
        assert_eq!(ANTIALIASINGS.len(), if WEB { 3 } else { 5 });
        assert_eq!(ANTIALIASINGS.first(), Some(&Antialiasing::Msaa));
        assert_eq!(ANTIALIASINGS.last(), Some(&Antialiasing::Off));
        let multisampled: Vec<_> = [
            Antialiasing::Msaa,
            Antialiasing::Msaa2,
            Antialiasing::Smaa,
            Antialiasing::Fxaa,
            Antialiasing::Off,
        ]
        .into_iter()
        .filter(|antialiasing| antialiasing.is_multisampled())
        .collect();
        assert_eq!(multisampled, [Antialiasing::Msaa, Antialiasing::Msaa2]);
    }

    #[test]
    fn the_game_starts_with_fxaa_and_no_relief_and_keeps_what_is_chosen() {
        // As the game starts: no relief, FXAA, the shadows automatic.
        let first = Settings::default();
        assert_eq!(
            (first.relief, first.antialiasing, first.shadows),
            (Relief::Off, Antialiasing::Fxaa, ShadowQuality::Auto)
        );
        // Saved without them: as the game starts.
        let older: Settings = ron::from_str("(fps: Unlimited)").unwrap();
        assert_eq!(
            (older.relief, older.antialiasing),
            (Relief::Off, Antialiasing::Fxaa)
        );
        // Saved with them: as saved.
        let saved: Settings = ron::from_str("(relief: Normal, antialiasing: Msaa)").unwrap();
        assert_eq!(
            (saved.relief, saved.antialiasing),
            (Relief::Normal, Antialiasing::Msaa)
        );
        for relief in RELIEFS {
            let text = ron::ser::to_string(&Settings {
                relief,
                ..default()
            })
            .unwrap();
            assert_eq!(ron::from_str::<Settings>(&text).unwrap().relief, relief);
        }
        assert_eq!(
            Relief::Parallax.name(Language::Spanish),
            "Normales y parallax"
        );
        assert_eq!(Relief::Off.name(Language::English), "Off");
    }

    #[test]
    fn the_keyboard_s_curves_are_saved_and_older_settings_start_without_them() {
        let mut settings = Settings {
            keyboard_mode: crate::bike::curves::KeyboardMode::Bezier,
            show_keyboard_curves: true,
            ..Settings::default()
        };
        let shapes = &mut settings.keyboard_shapes;
        shapes.lean_in_seconds = 0.25;
        shapes.turn.shape = crate::bike::curves::CurveShape::Custom;
        shapes.turn.custom = crate::bezier::Bezier::new(0.1, 0.5, 0.6, 0.9);
        shapes.set_learn(crate::bike::curves::LearnLine::Race);
        let text = ron::ser::to_string(&settings).unwrap();
        let loaded: Settings = ron::from_str(&text).unwrap();
        assert_eq!(loaded.keyboard_shapes, settings.keyboard_shapes);
        assert_eq!(
            loaded.keyboard_mode,
            crate::bike::curves::KeyboardMode::Bezier
        );
        assert!(loaded.show_keyboard_curves);
        // Saved before them: off, and the classic ones.
        let older: Settings = ron::from_str("(fps: Unlimited)").unwrap();
        assert_eq!(
            older.keyboard_mode,
            crate::bike::curves::KeyboardMode::Classic
        );
        assert!(!older.show_keyboard_curves);
        // Turned on with the box they had at first: Bézier.
        let boxed = Settings::default()
            .brought_up_to_date("(keyboard_curves_on: true, show_keyboard_curves: true)");
        assert_eq!(
            boxed.keyboard_mode,
            crate::bike::curves::KeyboardMode::Bezier
        );
        assert_eq!(
            older.keyboard_shapes,
            crate::bike::curves::KeyboardShapes::default()
        );
        // The first curves, saved as one set, are not read; the rest is.
        let first: Settings = ron::from_str(
            "(fps: Unlimited, keyboard_curves: (lean_in: (curve: (x1: 0.1, y1: 0.2, x2: 0.3, \
             y2: 0.4), seconds: 0.3), top_speed_lean: 0.9))",
        )
        .unwrap();
        assert_eq!(first.fps, FpsLimit::Unlimited);
        assert_eq!(first.keyboard_shapes, older.keyboard_shapes);
    }

    #[test]
    fn the_performance_forced_the_performance_hud_and_the_water_start_as_they_were() {
        let fresh: Settings = ron::from_str("()").unwrap();
        // Nothing forced; the frame rate shown, the rest of the HUD not;
        // the sea mirroring, not the cartoon one.
        assert_eq!(fresh.force_performance, ForcePerformance::Off);
        assert!(fresh.show_fps && !fresh.show_resolution && !fresh.show_coordinates);
        assert!(!fresh.show_antialiasing && !fresh.show_textures);
        assert!(!fresh.cartoon_water);
        // The performance mode on, as saved before: forced to 60 fps; off,
        // nothing forced; and once saved again, as chosen.
        let on = "(performance_mode: true, fps: Unlimited)";
        let loaded = ron::from_str::<Settings>(on)
            .unwrap()
            .brought_up_to_date(on);
        assert_eq!(loaded.force_performance, ForcePerformance::Fps60);
        assert_eq!(loaded.fps, FpsLimit::Unlimited);
        let off = "(performance_mode: false)";
        let loaded = ron::from_str::<Settings>(off)
            .unwrap()
            .brought_up_to_date(off);
        assert_eq!(loaded.force_performance, ForcePerformance::Off);
        let now = "(performance_mode: true, force_performance: Fps120)";
        let loaded = ron::from_str::<Settings>(now)
            .unwrap()
            .brought_up_to_date(now);
        assert_eq!(loaded.force_performance, ForcePerformance::Fps120);
        // Off, 60 and 120, in turn.
        assert_eq!(
            FORCE_PERFORMANCES.map(ForcePerformance::fps),
            [None, Some(60.0), Some(120.0)]
        );
        assert_eq!(
            cycle(&FORCE_PERFORMANCES, ForcePerformance::Fps120, 1),
            ForcePerformance::Off
        );
    }

    #[test]
    fn a_fixed_cap_does_not_wait_for_the_screen_where_it_need_not_tear() {
        // The screen's rate on its beat; unlimited, at once.
        assert_eq!(FpsLimit::Screen.present_mode(), PresentMode::AutoVsync);
        assert_eq!(FpsLimit::Unlimited.present_mode(), PresentMode::AutoNoVsync);
        // A cap of 120 on a screen at 60 Hz must not be held to 60: the
        // newest frame shown at each refresh, without tearing, but where
        // there is no such thing (macOS).
        let cap = FpsLimit::Cap(120).present_mode();
        if cfg!(target_os = "macos") {
            assert_eq!(cap, PresentMode::AutoVsync);
        } else {
            assert_eq!(cap, PresentMode::Mailbox);
        }
        // How many frames each lets through on a screen at 60 Hz.
        assert_eq!(FpsLimit::Cap(120).bound(Some(60.0)), Some(120.0));
        assert_eq!(FpsLimit::Screen.bound(Some(60.0)), Some(60.0));
        assert_eq!(FpsLimit::Screen.bound(None), None);
        assert_eq!(FpsLimit::Unlimited.bound(Some(60.0)), None);
    }

    #[test]
    fn volumes_saved_with_the_old_scale_start_again_from_the_default() {
        // The old scale: effects at 20 % and music at 90 % is the balance
        // that the new scale calls 80 %.
        let old: Settings = ron::from_str("(fx_volume: 20, music_volume: 90)").unwrap();
        assert_eq!(old.fx_volume, DEFAULT_VOLUME);
        assert_eq!(old.music_volume, DEFAULT_VOLUME);
        let new: Settings = ron::from_str("(fx_level: 50, music_level: 100)").unwrap();
        assert_eq!((new.fx_volume, new.music_volume), (50, 100));
    }

    #[test]
    fn a_key_does_only_one_thing() {
        let mut controls = Controls::default();
        // W pasa a frenar en lugar de S: deja de acelerar, que se queda con
        // la flecha, y frenar sigue también con la suya.
        assert_eq!(
            controls.remap(KeyCode::KeyS, &[KeyCode::KeyW]),
            Some(Action::Brake)
        );
        assert_eq!(controls.brake.keys, [KeyCode::KeyW, KeyCode::ArrowDown]);
        assert_eq!(controls.throttle.keys, [KeyCode::ArrowUp]);
        // Los botones van aparte: frenar sigue con los suyos.
        assert_eq!(
            controls.brake.buttons,
            [GamepadButton::LeftTrigger2, GamepadButton::West]
        );
        // B, the wheelie's, becomes A, which stops accelerating.
        controls.remap(GamepadButton::East, &[GamepadButton::South]);
        assert_eq!(controls.wheelie.buttons, [GamepadButton::South]);
        assert_eq!(controls.throttle.buttons, [GamepadButton::RightTrigger2]);
        // A key that does nothing has nothing to remap.
        assert_eq!(controls.remap(KeyCode::KeyK, &[KeyCode::KeyL]), None);
    }

    #[test]
    fn an_action_can_take_keys_pressed_together() {
        let mut controls = Controls::default();
        let together = [KeyCode::ShiftLeft, KeyCode::KeyR];
        assert_eq!(
            controls.remap(KeyCode::KeyR, &together),
            Some(Action::Restart)
        );
        assert!(controls.restart.keys.is_empty());
        assert_eq!(controls.restart.key_combos, [together.to_vec()]);
        // Shift on its own still does the wheelie.
        assert!(controls.wheelie.keys.contains(&KeyCode::ShiftLeft));
        let mut keyboard = ButtonInput::<KeyCode>::default();
        keyboard.press(KeyCode::KeyR);
        assert_eq!(controls.key_value(Action::Restart, &keyboard), 0.0);
        assert!(!controls.just_pressed(Action::Restart, &keyboard, []));
        keyboard.clear();
        keyboard.press(KeyCode::ShiftLeft);
        assert!(controls.just_pressed(Action::Restart, &keyboard, []));
        assert_eq!(controls.key_value(Action::Restart, &keyboard), 1.0);
        assert_eq!(
            controls.describe_keys(Action::Restart, Language::English),
            "Left Shift+R"
        );
        // A key of a combination remaps it all.
        assert_eq!(controls.action_of(KeyCode::KeyR), Some(Action::Restart));
        controls.remap(KeyCode::KeyR, &[KeyCode::F5]);
        assert_eq!(controls.restart.keys, [KeyCode::F5]);
        assert!(controls.restart.key_combos.is_empty());
    }

    #[test]
    fn the_keyboard_and_the_gamepad_are_reset_apart() {
        let mut controls = Controls::default();
        controls.remap(KeyCode::KeyW, &[KeyCode::KeyI]);
        controls.remap(GamepadButton::South, &[GamepadButton::North]);
        controls.reset::<KeyCode>();
        assert_eq!(controls.throttle.keys, Controls::default().throttle.keys);
        assert!(controls.throttle.buttons.contains(&GamepadButton::North));
        controls.reset::<GamepadButton>();
        assert_eq!(controls, Controls::default());
    }

    #[test]
    fn options_cycle_around() {
        assert_eq!(
            cycle(RESOLUTIONS, RESOLUTIONS[RESOLUTIONS.len() - 1], 1),
            RESOLUTIONS[0]
        );
        assert_eq!(
            cycle(&FPS_LIMITS, FpsLimit::Screen, -1),
            FpsLimit::Unlimited
        );
        // Una que no está en la lista (escrita a mano en el archivo) vuelve a
        // la primera.
        assert_eq!(
            cycle(RESOLUTIONS, Resolution::Window(800, 600), 1),
            RESOLUTIONS[0]
        );
    }

    #[test]
    fn dynamic_resolution_has_name() {
        assert_eq!(
            Resolution::Dynamic.name(None, Language::Spanish),
            "Dinámica"
        );
        assert_eq!(Resolution::Dynamic.name(None, Language::English), "Dynamic");
        assert_eq!(RESOLUTIONS[0], Resolution::Dynamic);
        assert_eq!(Resolution::default(), Resolution::Dynamic);
    }

    #[test]
    fn fullscreen_takes_the_display_mode_nearest_the_resolution() {
        let mode = |width: u32, height: u32, hz: u32| VideoMode {
            physical_size: UVec2::new(width, height),
            bit_depth: 32,
            refresh_rate_millihertz: hz * 1000,
        };
        let modes = [
            mode(2560, 1600, 60),
            mode(1920, 1200, 60),
            mode(1280, 720, 60),
            mode(1280, 720, 120),
            mode(1024, 768, 60),
        ];
        // The same size, at its fastest rate.
        let found = fitting_mode(&modes, 1280, 720).unwrap();
        assert_eq!(found, mode(1280, 720, 120));
        // A size the screen lacks: the nearest one.
        assert_eq!(
            fitting_mode(&modes, 1920, 1080).unwrap().physical_size,
            UVec2::new(1920, 1200)
        );
        assert_eq!(fitting_mode(&[], 1920, 1080), None);
        // Where the kind of fullscreen is not chosen, only a fixed
        // resolution switches the screen; where it is, the kind chosen.
        let fixed = Settings {
            fullscreen: true,
            fullscreen_kind: FullscreenKind::Exclusive,
            resolution: Resolution::Window(1280, 720),
            ..default()
        };
        assert!(fixed.exclusive());
        let dynamic = Settings {
            resolution: Resolution::Dynamic,
            ..fixed.clone()
        };
        assert_eq!(dynamic.kind_of_fullscreen(false), FullscreenKind::Desktop);
        assert_eq!(dynamic.kind_of_fullscreen(true), FullscreenKind::Exclusive);
        let desktop = Settings {
            fullscreen_kind: FullscreenKind::Desktop,
            ..fixed.clone()
        };
        assert_eq!(desktop.kind_of_fullscreen(false), FullscreenKind::Exclusive);
        assert_eq!(desktop.kind_of_fullscreen(true), FullscreenKind::Desktop);
        // Over the desktop, a fixed resolution draws the world at that
        // many pixels at most; switching the screen, or in a window, not.
        assert_eq!(
            desktop.desktop_resolution_as(FullscreenKind::Desktop),
            Some(UVec2::new(1280, 720))
        );
        assert_eq!(
            desktop.desktop_resolution_as(FullscreenKind::Exclusive),
            None
        );
        let windowed = Settings {
            fullscreen: false,
            ..desktop.clone()
        };
        assert_eq!(
            windowed.desktop_resolution_as(FullscreenKind::Desktop),
            None
        );
        assert_eq!(dynamic.desktop_resolution_as(FullscreenKind::Desktop), None);
        let monitor = Monitor {
            name: None,
            physical_height: 1600,
            physical_width: 2560,
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: Some(60_000),
            scale_factor: 2.0,
            video_modes: modes.to_vec(),
        };
        let mut window = Window::default();
        fixed.configure(&mut window, Some(&monitor));
        assert_eq!(
            window.mode,
            WindowMode::Fullscreen(
                MonitorSelection::Current,
                VideoModeSelection::Specific(mode(1280, 720, 120))
            )
        );
        // Before the monitor is known, the window covers the screen as it is.
        fixed.configure(&mut window, None);
        assert_eq!(
            window.mode,
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        );
        // A dedicated fullscreen at a dynamic resolution takes the screen's
        // own size; on the desktop, the window covers the screen as it is.
        dynamic.configure_as(&mut window, Some(&monitor), true);
        assert_eq!(
            window.mode,
            WindowMode::Fullscreen(
                MonitorSelection::Current,
                VideoModeSelection::Specific(mode(2560, 1600, 60))
            )
        );
        fixed.configure_as(&mut window, Some(&monitor), false);
        assert_eq!(
            window.mode,
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        );
    }

    #[test]
    fn trying_the_controls_the_gamepads_shortcuts_do_nothing() {
        // Whether fullscreen switched, and the sound was asked to go off.
        let press = |test: bool, second: GamepadButton| {
            let mut app = App::new();
            app.add_message::<ToggleMute>()
                .insert_resource(Settings::default())
                .insert_resource(ControlsTest { on: test })
                .add_systems(Update, gamepad_shortcuts);
            let mut pad = Gamepad::default();
            pad.digital_mut().press(GamepadButton::Select);
            pad.digital_mut().press(second);
            app.world_mut().spawn(pad);
            app.update();
            let muted = !app.world().resource::<Messages<ToggleMute>>().is_empty();
            (app.world().resource::<Settings>().fullscreen, muted)
        };
        assert_eq!(press(false, FULLSCREEN_BUTTON), (true, false));
        assert_eq!(press(false, MUTE_BUTTON), (false, true));
        assert_eq!(press(true, FULLSCREEN_BUTTON), (false, false));
        assert_eq!(press(true, MUTE_BUTTON), (false, false));
    }

    #[test]
    fn select_with_r3_or_y_is_a_shortcut_and_alone_it_restarts_on_release() {
        use GamepadButton as B;
        let mut buttons = ButtonInput::<GamepadButton>::default();
        let mut hold = ShortcutHold::default();
        // A frame of the gamepad: what was pressed and let go of.
        let mut frame = |buttons: &mut ButtonInput<GamepadButton>, press: &[B], release: &[B]| {
            buttons.clear();
            for &button in press {
                buttons.press(button);
            }
            for &button in release {
                buttons.release(button);
            }
            read_shortcut(buttons, &mut hold)
        };
        // Select down: nobody sees it yet.
        assert_eq!(frame(&mut buttons, &[B::Select], &[]), None);
        assert!(!buttons.just_pressed(B::Select));
        // With R3: fullscreen, and R3 does nothing else.
        assert_eq!(
            frame(&mut buttons, &[B::RightThumb], &[]),
            Some(Shortcut::Fullscreen)
        );
        assert!(!buttons.just_pressed(B::RightThumb));
        // Start is not a shortcut's: it pauses, even with Select held.
        assert_eq!(frame(&mut buttons, &[B::Start], &[]), None);
        assert!(buttons.just_pressed(B::Start));
        // With Y: the sound.
        assert_eq!(frame(&mut buttons, &[B::North], &[]), Some(Shortcut::Mute));
        assert!(!buttons.just_pressed(B::North));
        // Let go of after a shortcut: Select does nothing.
        assert_eq!(
            frame(
                &mut buttons,
                &[],
                &[B::Select, B::RightThumb, B::Start, B::North]
            ),
            None
        );
        assert!(!buttons.just_pressed(B::Select));
        // Select alone: pressed as it is let go of, for that frame only.
        frame(&mut buttons, &[B::Select], &[]);
        frame(&mut buttons, &[], &[B::Select]);
        assert!(buttons.just_pressed(B::Select));
        frame(&mut buttons, &[], &[]);
        assert!(!buttons.pressed(B::Select) && !buttons.just_pressed(B::Select));
        // Start and R3 alone are pressed as ever.
        frame(&mut buttons, &[B::Start, B::RightThumb], &[]);
        assert!(buttons.just_pressed(B::Start) && buttons.just_pressed(B::RightThumb));
    }

    #[test]
    fn the_rivals_ride_slower_on_easy_and_faster_on_hard() {
        for skill in [0.88, 0.95] {
            assert_eq!(Difficulty::Medium.rival_skill(skill), skill);
            assert!(Difficulty::Easy.rival_skill(skill) < skill - 0.05);
            assert!(Difficulty::Hard.rival_skill(skill) > skill);
            assert!(Difficulty::Hard.rival_skill(skill) <= 1.0);
            // Very hard, more than hard: the quickest past what the bike
            // gives (braking later and riding the corners harder).
            let very_hard = Difficulty::VeryHard.rival_skill(skill);
            assert!(very_hard > Difficulty::Hard.rival_skill(skill));
        }
        assert!(Difficulty::VeryHard.rival_skill(0.95) > 1.0);
        assert_eq!(DIFFICULTIES.last(), Some(&Difficulty::VeryHard));
        assert_eq!(Difficulty::VeryHard.name(Language::Spanish), "Muy difícil");
    }

    #[test]
    fn a_fixed_limit_holds_the_frames_back() {
        let mut app = App::new();
        app.insert_resource(Settings {
            fps: FpsLimit::Cap(50),
            ..default()
        })
        .init_resource::<FrameCap>()
        .add_systems(Update, (cap_frames, limit_frames).chain());
        let start = Instant::now();
        for _ in 0..6 {
            app.update();
        }
        // El primero no espera; los otros cinco, lo que falte hasta 20 ms.
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(98), "{elapsed:?}");
    }

    #[test]
    fn the_fullscreen_keys_go_there_and_back() {
        let mut app = App::new();
        app.insert_resource(Settings {
            fullscreen: false,
            ..default()
        })
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ControlsTest>()
        .add_systems(Update, toggle_fullscreen);
        let modifier = if cfg!(target_os = "macos") {
            KeyCode::SuperLeft
        } else {
            KeyCode::AltLeft
        };
        let press = |app: &mut App, keys: &[KeyCode]| {
            let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keyboard.release_all();
            keyboard.clear();
            for &key in keys {
                keyboard.press(key);
            }
            app.update();
            app.world().resource::<Settings>().fullscreen
        };
        // Enter solo no hace nada; con la otra tecla, conmuta pantalla completa.
        assert!(!press(&mut app, &[KeyCode::Enter]));
        assert!(press(&mut app, &[modifier, KeyCode::Enter]));
        assert!(!press(&mut app, &[modifier, KeyCode::Enter]));
        // Not while the controls are tried out.
        app.insert_resource(ControlsTest { on: true });
        assert!(!press(&mut app, &[modifier, KeyCode::Enter]));
    }

    #[test]
    fn keys_and_buttons_have_readable_names() {
        let es = Language::Spanish;
        assert_eq!(key_name(KeyCode::KeyW, es), "W");
        assert_eq!(key_name(KeyCode::Digit3, es), "3");
        assert_eq!(key_name(KeyCode::Numpad3, es), "3 (num.)");
        assert_eq!(key_name(KeyCode::ArrowUp, es), "Arriba");
        assert_eq!(key_name(KeyCode::ArrowUp, Language::English), "Up");
        assert_eq!(key_name(KeyCode::F5, es), "F5");
        assert_eq!(button_name(GamepadButton::RightTrigger2, es), "RT");
        assert_eq!(
            button_name(GamepadButton::DPadLeft, Language::English),
            "D-pad left"
        );
        let controls = Controls::default();
        assert_eq!(controls.describe_keys(Action::Throttle, es), "W, Arriba");
        assert_eq!(controls.describe_buttons(Action::Throttle, es), "RT, A");
        assert_eq!(
            controls.describe_keys(Action::Throttle, Language::English),
            "W, Up"
        );
        assert_eq!(
            controls.describe_keys(Action::Wheelie, es),
            "Espacio, Mayús izq."
        );
        assert_eq!(controls.describe_buttons(Action::Wheelie, es), "B");
    }

    #[test]
    fn saved_controls_without_a_wheelie_button_get_b() {
        let mut saved = Controls::default();
        saved.wheelie.buttons.clear();
        saved.give_the_wheelie_a_button();
        assert_eq!(saved.wheelie.buttons, [GamepadButton::East]);
        // Unless B already does something else.
        let mut taken = Controls::default();
        taken.wheelie.buttons.clear();
        taken.brake.buttons = vec![GamepadButton::East];
        taken.give_the_wheelie_a_button();
        assert!(taken.wheelie.buttons.is_empty());
        assert_eq!(taken.brake.buttons, [GamepadButton::East]);
    }
}
