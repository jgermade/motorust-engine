//! Marcador en pantalla: velocímetro y leyenda de los controles (oculta de
//! entrada; se enseña desde Depuración) en la carrera, and the performance
//! HUD at the top right everywhere, as Options > Graphics > Performance HUD
//! says: the frames per second, the pixels the world is drawn at, and the
//! smoothing of the edges and the textures in use; what the performance
//! forced has lowered in amber ([`ADJUSTED`]). El
//! minimapa va aparte, en [`crate::minimap`] (where the player is on it is
//! the performance HUD's too), and the speedometer's dial is drawn in
//! [`crate::speedometer`].

use bevy::prelude::*;

use crate::circuits::{Circuits, CurrentCircuit};
use crate::gamepads::ActiveController;
use crate::legend::{KeyPrompt, correction_prompt, music_prompt, race_prompts, spawn_legend};
use crate::menu::GameState;
use crate::pause::{DebugOptions, Pause};
use crate::race::{Mode, Rules};
use crate::render_scale::{NativeShare, drawn_size, render_percent};
use crate::settings::{Antialiasing, GrassTiling, Relief, Settings};
use crate::sound::music::{FreeMusic, Songs};
use crate::speedometer::{SpeedometerPlugin, spawn_speedometer};
use crate::ui::{InputDevice, UiIcons, current_active_device};
use crate::welcome::GameMode;
use crate::world::detail::{Detail, TwoSamples, in_use};
use crate::world::relief::{Look, ReliefShown};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SpeedometerPlugin)
            .init_resource::<UiFont>()
            .add_systems(Startup, spawn_performance_hud)
            .add_systems(OnEnter(GameState::Race), spawn_race_hud)
            .add_systems(
                Update,
                (
                    show_race_legend.run_if(in_state(GameState::Race)),
                    (update_fps, show_details, keep_the_tag_clear),
                ),
            );
    }
}

/// Fuente de los rótulos: Russo One, con tildes y eñes (la que trae Bevy
/// solo tiene ASCII).
#[derive(Resource)]
pub struct UiFont(pub Handle<Font>);

impl FromWorld for UiFont {
    fn from_world(world: &mut World) -> Self {
        let font = Font::from_bytes(crate::content::file(crate::vector_art::FONT_FILE).to_vec());
        Self(world.resource_mut::<Assets<Font>>().add(font))
    }
}

/// La leyenda con los controles.
#[derive(Component)]
struct Legend;

/// The performance HUD, on a line at the top right: the textures and the
/// smoothing of the edges in use, the pixels the world is drawn at, and the
/// frame rate after them.
#[derive(Component)]
struct PerformanceHud;
#[derive(Component)]
struct FpsCounter;
/// The line of what the performance HUD says left of the frame rate.
#[derive(Component)]
struct HudDetails;
/// A piece of that line: a value (the relief, the lawn's tiling, the
/// antialiasing, the resolution, in that order) or the separator after
/// one, by its place ([`DETAILS`] values, each with its separator).
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum HudPiece {
    Value(usize),
    Separator(usize),
}

/// How many values the line holds.
const DETAILS: usize = 4;
/// What stands between two values, faint, in the interface's font (Bevy's
/// has no bullet).
const SEPARATOR: &str = "  \u{2022}  ";

/// The colour of what the performance HUD says, of what the performance
/// forced has lowered from what the options say, and of the separators.
const SHOWN: Color = Color::srgba(1.0, 1.0, 1.0, 0.8);
const ADJUSTED: Color = Color::srgba(1.0, 0.72, 0.2, 0.95);
const FAINT: Color = Color::srgba(1.0, 1.0, 1.0, 0.35);

/// The colour of a value the performance forced has lowered (`adjusted`)
/// or not.
fn tint(adjusted: bool) -> Color {
    if adjusted { ADJUSTED } else { SHOWN }
}

/// How big the frame rate is (px), and how wide its room, always the same
/// ("999 FPS" in Bevy's font, whose figures are all as wide), so what
/// stands left of it does not move as it changes; how big the resolution
/// is, and how far from the frame rate.
const FPS_SIZE: f32 = 18.0;
const FPS_WIDTH: f32 = 7.0 * 0.6 * FPS_SIZE;
const RESOLUTION_SIZE: f32 = 15.0;
const RESOLUTION_GAP: f32 = 12.0;

fn spawn_performance_hud(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            PerformanceHud,
            Node {
                position_type: PositionType::Absolute,
                top: px(12),
                right: px(16),
                align_items: AlignItems::FlexEnd,
                column_gap: px(RESOLUTION_GAP),
                ..default()
            },
        ))
        .with_children(|hud| {
            hud.spawn((
                HudDetails,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(RESOLUTION_SIZE),
                    ..default()
                },
                TextColor(SHOWN),
                Node {
                    display: Display::None,
                    ..default()
                },
            ))
            .with_children(|line| {
                for k in 0..DETAILS {
                    line.spawn((
                        HudPiece::Value(k),
                        TextSpan::new(""),
                        TextFont {
                            font_size: FontSize::Px(RESOLUTION_SIZE),
                            ..default()
                        },
                        TextColor(SHOWN),
                    ));
                    line.spawn((
                        HudPiece::Separator(k),
                        TextSpan::new(""),
                        TextFont {
                            font: font.0.clone().into(),
                            font_size: FontSize::Px(RESOLUTION_SIZE),
                            ..default()
                        },
                        TextColor(FAINT),
                    ));
                }
            });
            hud.spawn((
                FpsCounter,
                Text::new("-- FPS"),
                TextFont {
                    font_size: FontSize::Px(FPS_SIZE),
                    ..default()
                },
                TextLayout::justify(Justify::Right),
                Node {
                    min_width: px(FPS_WIDTH),
                    ..default()
                },
            ));
        });
}

/// Shown or not, taking its room or not.
fn display(shown: bool) -> Display {
    if shown { Display::Flex } else { Display::None }
}

/// The pixels the world is drawn at in a window `window` pixels big, at
/// `percent` of its resolution (never more than the screen's own, see
/// [`NativeShare`]): "1920x1080", and the share when it is not all of it
/// ("1344x756 70%"); in Bevy's font, as the frame rate.
pub fn resolution_line(window: UVec2, percent: u32, native: NativeShare) -> String {
    let size = drawn_size(window, percent, native);
    if percent >= 100 {
        format!("{}x{}", size.x, size.y)
    } else {
        format!("{}x{} {percent}%", size.x, size.y)
    }
}

/// The performance HUD's line left of the frame rate, as the options say:
/// the textures in use (the ground's relief, the lawn's tiling), the
/// smoothing of the edges in use and the pixels the world is drawn at now,
/// each in amber if the performance forced has lowered it from what the
/// options say, and between them a faint bullet.
#[allow(clippy::too_many_arguments)]
fn show_details(
    settings: Res<Settings>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    detail: Option<Res<Detail>>,
    native: Option<Res<NativeShare>>,
    relief: Option<Res<ReliefShown>>,
    two_samples: Option<Res<TwoSamples>>,
    mut line: Single<&mut Node, With<HudDetails>>,
    mut pieces: Query<(&HudPiece, &mut TextSpan, &mut TextColor)>,
) {
    let detail = detail.as_deref();
    let forced = settings.force_performance.is_on();
    let textures = settings.show_textures;
    let (relief, lowered) = relief_name(
        relief.as_deref().copied().unwrap_or_default(),
        settings.relief,
        detail.is_none_or(|detail| detail.parallax),
    );
    let (smoothing, smoothed) = antialiasing_line(
        settings.antialiasing,
        detail.is_none_or(|detail| detail.msaa),
        TwoSamples::of(two_samples.as_deref()),
    );
    let resolution = window
        .iter()
        .next()
        .filter(|_| settings.show_resolution)
        .map(|window| {
            let percent = render_percent(detail);
            let size = resolution_line(
                window.physical_size(),
                percent,
                native.as_deref().copied().unwrap_or_default(),
            );
            (size, forced && percent < 100)
        });
    let values: [Option<(String, bool)>; DETAILS] = [
        textures.then(|| (relief.to_string(), lowered)),
        textures.then(|| (grass_name(settings.grass_tiling).to_string(), false)),
        settings
            .show_antialiasing
            .then(|| (smoothing.to_string(), smoothed)),
        resolution,
    ];
    let shown = values.each_ref().map(Option::is_some);
    let between = separators(shown, settings.show_fps);
    let display = display(shown.contains(&true));
    if line.display != display {
        line.display = display;
    }
    for (piece, mut span, mut color) in &mut pieces {
        let (text, tinted) = match *piece {
            HudPiece::Value(k) => match &values[k] {
                Some((text, lowered)) => (text.as_str(), tint(*lowered)),
                None => ("", SHOWN),
            },
            HudPiece::Separator(k) => (if between[k] { SEPARATOR } else { "" }, FAINT),
        };
        if span.0 != text {
            span.0 = text.to_string();
        }
        color.set_if_neq(TextColor(tinted));
    }
}

/// Which values of the line (`shown`) a separator follows: each shown one
/// with another shown after it, or the frame rate (`fps`).
fn separators(shown: [bool; DETAILS], fps: bool) -> [bool; DETAILS] {
    std::array::from_fn(|k| shown[k] && (shown[k + 1..].contains(&true) || fps))
}

/// With the screen split, the first player's tag at the top right.
#[derive(Component)]
struct PlayerOneTag;

/// How far from the right the first player's tag stands (px): left of the
/// performance HUD `hud` px wide (logical), or at the edge with nothing
/// shown.
fn tag_right(hud: f32) -> f32 {
    if hud > 0.5 {
        16.0 + hud + RESOLUTION_GAP
    } else {
        16.0
    }
}

/// The first player's tag kept left of the performance HUD, as wide as it
/// is.
fn keep_the_tag_clear(
    hud: Query<&ComputedNode, With<PerformanceHud>>,
    mut tags: Query<&mut Node, With<PlayerOneTag>>,
) {
    let Some(hud) = hud.iter().next() else {
        return;
    };
    let right = px(tag_right(hud.size().x * hud.inverse_scale_factor));
    for mut tag in &mut tags {
        if tag.right != right {
            tag.right = right;
        }
    }
}

/// How wide the speedometer is (px): alone, and with two players, in each
/// half of the screen.
const DIAL: f32 = 220.0;
const SPLIT_DIAL: f32 = 160.0;

fn spawn_race_hud(mut commands: Commands, font: Res<UiFont>, game_mode: Res<GameMode>) {
    if *game_mode == GameMode::SplitScreen {
        // Línea divisoria central entre ambos jugadores
        commands.spawn((
            DespawnOnExit(GameState::Race),
            Node {
                position_type: PositionType::Absolute,
                top: percent(50.0),
                left: px(0),
                width: percent(100.0),
                height: px(4),
                margin: UiRect::top(px(-2.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.08, 0.12)),
        ));

        // Indicador Jugador 1 (Arriba), left of the performance HUD.
        commands.spawn((
            DespawnOnExit(GameState::Race),
            PlayerOneTag,
            Text::new("1P"),
            TextColor(Color::srgb(0.3, 0.8, 1.0)),
            TextFont {
                font: font.0.clone().into(),
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextShadow::default(),
            Node {
                position_type: PositionType::Absolute,
                top: px(12),
                right: px(90),
                ..default()
            },
        ));

        // Velocímetro Jugador 1 (abajo a la derecha de la mitad superior)
        spawn_speedometer(
            &mut commands,
            0,
            SPLIT_DIAL,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(50.0),
                right: px(16),
                margin: UiRect::bottom(px(10.0)),
                ..default()
            },
            &font.0,
        );

        // Indicador Jugador 2 (Abajo)
        commands.spawn((
            DespawnOnExit(GameState::Race),
            Text::new("2P"),
            TextColor(Color::srgb(1.0, 0.4, 0.4)),
            TextFont {
                font: font.0.clone().into(),
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextShadow::default(),
            Node {
                position_type: PositionType::Absolute,
                top: percent(50.0),
                right: px(90),
                margin: UiRect::top(px(12.0)),
                ..default()
            },
        ));

        // Velocímetro Jugador 2 (abajo a la derecha de la mitad inferior)
        spawn_speedometer(
            &mut commands,
            1,
            SPLIT_DIAL,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(16),
                right: px(16),
                ..default()
            },
            &font.0,
        );
    } else {
        // Velocímetro modo 1 jugador
        spawn_speedometer(
            &mut commands,
            0,
            DIAL,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(16),
                right: px(16),
                ..default()
            },
            &font.0,
        );
    }
}

/// La leyenda se ve u oculta según la opción de Opciones y se adapta al
/// mando activo y a los controles elegidos. In free ride, what changes the
/// music, and what plays, is always in it. In the pause menu it gives way
/// to the menu's own legend.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn show_race_legend(
    mut commands: Commands,
    options: Res<DebugOptions>,
    settings: Res<Settings>,
    (rules, free_music, circuit, circuits, songs): (
        Option<Res<Rules>>,
        Option<Res<FreeMusic>>,
        Option<Res<CurrentCircuit>>,
        Option<Res<Circuits>>,
        Option<Res<Songs>>,
    ),
    pause: Res<Pause>,
    device: Option<Res<InputDevice>>,
    active_controller: Option<Res<ActiveController>>,
    font: Res<UiFont>,
    icons: Res<UiIcons>,
    shown: Query<Entity, With<Legend>>,
    mut last: Local<Option<Vec<KeyPrompt>>>,
    (learning, probe): (
        Option<Res<crate::bike::learning::TargetLine>>,
        Option<Res<crate::bike::curves::CurveProbe>>,
    ),
) {
    let active_dev = current_active_device(
        device.as_deref().copied().unwrap_or_default(),
        active_controller.as_deref(),
    );
    let lang = settings.language;
    let mut wanted = options
        .show_legend
        .then(|| race_prompts(active_dev, lang, &settings.controls));
    if rules.is_some_and(|rules| rules.mode == Mode::Free)
        && let (Some(free_music), Some(circuit), Some(circuits), Some(songs)) =
            (free_music, circuit, circuits, songs)
    {
        let what = free_music.describe(circuit.0, &circuits, &songs, lang);
        wanted
            .get_or_insert_default()
            .push(music_prompt(active_dev, lang, what));
    }
    // Learning the keyboard's curves: only noting, or correcting.
    if learning.is_some_and(|target| target.line.is_some()) {
        let stick = probe.is_some_and(|probe| probe.stick);
        let shapes = if stick {
            &settings.stick_shapes
        } else {
            &settings.keyboard_shapes
        };
        let correcting = shapes.correct;
        wanted
            .get_or_insert_default()
            .push(correction_prompt(active_dev, lang, correcting));
    }
    let wanted = wanted.filter(|_| !pause.is_open());
    if *last == wanted && shown.is_empty() == wanted.is_none() {
        return;
    }
    for entity in &shown {
        commands.entity(entity).despawn();
    }
    *last = wanted.clone();
    let Some(prompts) = wanted else {
        return;
    };

    commands
        .spawn((
            DespawnOnExit(GameState::Race),
            Legend,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                ..default()
            },
            GlobalZIndex(5),
        ))
        .with_children(|screen| {
            spawn_legend(screen, &font, &icons, active_dev, &prompts);
        });
}

/// Fotogramas por segundo, suavizados para que se puedan leer.
/// How often the frame rate shown changes (s): it is the average over that
/// time, so it reads steadily instead of flickering frame by frame.
const FPS_EVERY: f32 = 0.5;

/// Frames counted since the frame rate shown last changed, and over how
/// long.
#[derive(Default)]
struct FpsMeter {
    frames: u32,
    elapsed: f32,
}

impl FpsMeter {
    /// Counts a frame that took `dt` s: every [`FPS_EVERY`], the average
    /// frame rate since the last.
    fn tick(&mut self, dt: f32) -> Option<f32> {
        self.frames += 1;
        self.elapsed += dt;
        if self.elapsed < FPS_EVERY {
            return None;
        }
        let fps = self.frames as f32 / self.elapsed;
        *self = Self::default();
        Some(fps)
    }
}

fn update_fps(
    time: Res<Time<Real>>,
    settings: Res<Settings>,
    mut meter: Local<FpsMeter>,
    mut counter: Single<(&mut Text, &mut Node), With<FpsCounter>>,
) {
    let (text, node) = &mut *counter;
    let shown = display(settings.show_fps);
    if node.display != shown {
        node.display = shown;
    }
    let Some(fps) = meter.tick(time.delta_secs()) else {
        return;
    };
    let label = format!("{fps:.0} FPS");
    if text.0 != label {
        text.0 = label;
    }
}

/// The smoothing of the edges in use, as the performance HUD says it, and
/// whether the performance forced has lowered it: the multisampling chosen
/// given up (`msaa`, see [`Detail::msaa`]) for FXAA. Two samples are four
/// where the GPU has not got them (`two_samples`, see [`TwoSamples`]).
pub fn antialiasing_line(
    chosen: Antialiasing,
    msaa: bool,
    two_samples: bool,
) -> (&'static str, bool) {
    let now = in_use(chosen, msaa, two_samples);
    let name = match now {
        Antialiasing::Msaa => "MSAA 4x",
        Antialiasing::Msaa2 => "MSAA 2x",
        Antialiasing::Smaa => "SMAA",
        Antialiasing::Fxaa => "FXAA",
        Antialiasing::Off => "No AA",
    };
    (name, chosen.is_multisampled() && !now.is_multisampled())
}

/// The ground's relief in use, as the performance HUD says it (with the
/// parallax, the normal maps alone, flat at speed, or none), and whether
/// the performance forced has lowered it: the parallax chosen given up
/// (`parallax`, see [`Detail::parallax`]).
pub fn relief_name(shown: ReliefShown, chosen: Relief, parallax: bool) -> (&'static str, bool) {
    let name = match shown.look(false) {
        Look::Parallax => "Parallax",
        Look::Normal => "Normals",
        Look::Flat => "Flat",
        Look::Off => "No relief",
    };
    (name, chosen == Relief::Parallax && !parallax)
}

/// The lawn's tiling, as the performance HUD says it.
pub fn grass_name(tiling: GrassTiling) -> &'static str {
    match tiling {
        GrassTiling::Plain => "Tiled grass",
        GrassTiling::Wide => "Wide grass",
        GrassTiling::Stochastic => "Stochastic grass",
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn the_hud_says_the_smoothing_and_the_textures_in_use_and_what_was_lowered() {
        assert_eq!(
            antialiasing_line(Antialiasing::Msaa, true, true),
            ("MSAA 4x", false)
        );
        // Given up by the performance forced: FXAA, lowered.
        assert_eq!(
            antialiasing_line(Antialiasing::Msaa, false, true),
            ("FXAA", true)
        );
        assert_eq!(
            antialiasing_line(Antialiasing::Msaa2, false, true),
            ("FXAA", true)
        );
        assert_eq!(
            antialiasing_line(Antialiasing::Msaa2, true, true),
            ("MSAA 2x", false)
        );
        // Four samples for two where the GPU has not got them: not lowered.
        assert_eq!(
            antialiasing_line(Antialiasing::Msaa2, true, false),
            ("MSAA 4x", false)
        );
        if !crate::settings::WEB {
            assert_eq!(
                antialiasing_line(Antialiasing::Smaa, false, true),
                ("SMAA", false)
            );
        }
        assert_eq!(
            antialiasing_line(Antialiasing::Off, false, true),
            ("No AA", false)
        );
        let shown = |relief, flat| ReliefShown {
            relief,
            road: false,
            flat,
        };
        assert_eq!(
            relief_name(shown(Relief::Parallax, false), Relief::Parallax, true),
            ("Parallax", false)
        );
        // The parallax given up: normal maps, lowered; the lawn's tiling
        // apart, never lowered.
        assert_eq!(
            relief_name(shown(Relief::Normal, false), Relief::Parallax, false),
            ("Normals", true)
        );
        assert_eq!(grass_name(GrassTiling::Wide), "Wide grass");
        // Flat at speed: not the performance forced's doing.
        assert_eq!(
            relief_name(shown(Relief::Normal, true), Relief::Normal, true),
            ("Flat", false)
        );
    }

    #[test]
    fn a_bullet_stands_between_the_values_shown_and_before_the_frame_rate() {
        assert_eq!(
            separators([true, true, false, true], true),
            [true, true, false, true]
        );
        // Nothing after the last value: no bullet after it.
        assert_eq!(
            separators([true, false, true, false], false),
            [true, false, false, false]
        );
        assert_eq!(separators([false; DETAILS], true), [false; DETAILS]);
    }

    #[test]
    fn the_resolution_line_says_the_pixels_the_world_is_drawn_at() {
        let window = UVec2::new(1920, 1080);
        let whole = NativeShare(1.0);
        assert_eq!(resolution_line(window, 100, whole), "1920x1080");
        assert_eq!(resolution_line(window, 70, whole), "1344x756 70%");
        // On a HiDPI screen of fewer pixels of its own, never more than
        // those.
        let retina = UVec2::new(4096, 2304);
        assert_eq!(
            resolution_line(retina, 100, NativeShare(0.625)),
            "2560x1440"
        );
        // The first player's tag left of the HUD, as wide as it is, or at
        // the edge with nothing shown.
        assert_eq!(tag_right(0.0), 16.0);
        assert_eq!(tag_right(FPS_WIDTH), 16.0 + FPS_WIDTH + RESOLUTION_GAP);
        // Room for "999 FPS" in Bevy's font, whose figures are 0.6 wide.
        assert!((FPS_WIDTH - 75.6).abs() < 0.01);
    }

    #[test]
    fn the_frame_rate_is_shown_as_an_average_twice_a_second() {
        let mut meter = FpsMeter::default();
        // 60 frames a second for two seconds: shown four times.
        let shown: Vec<f32> = (0..120).filter_map(|_| meter.tick(1.0 / 60.0)).collect();
        assert_eq!(shown.len(), 4, "{shown:?}");
        assert!(
            shown.iter().all(|fps| (fps - 60.0).abs() < 2.5),
            "{shown:?}"
        );
        // Uneven frames read as their average.
        let mut meter = FpsMeter::default();
        let shown = (0..40)
            .filter_map(|k| meter.tick(if k % 2 == 0 { 0.01 } else { 0.03 }))
            .next()
            .expect("shown after half a second");
        assert!((shown - 50.0).abs() < 1.0, "{shown}");
    }
}
