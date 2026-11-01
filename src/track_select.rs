//! Elegir pista: la pista se ve desde lo alto, girando despacio, con su
//! nombre, una descripción, su longitud y un plano del trazado. Izquierda y
//! derecha cambian de pista (y el mundo se vuelve a construir), Enter o A
//! pasan a elegir moto, y Esc o B vuelven a los modos de juego, como dice
//! la leyenda de abajo ([`crate::legend`]).
//!
//! The card also says what is being played ([`crate::race::Mode`]) and the
//! best of the track for it; in a race, up and down choose its laps.

use bevy::prelude::*;

use crate::bike::RIDERS;
use crate::camera::{CameraGlide, CameraPlayer};
use crate::circuits::{Circuits, CurrentCircuit};
use crate::gamepads::ActiveController;
use crate::hud::UiFont;
use crate::legend::{
    KeyPrompt, Keys, ScreenLegend, menu_prompts, music_toggle_prompt, show_screen_legend,
};
use crate::locale::Language;
use crate::menu::GameState;
use crate::minimap::TrackMap;
use crate::modes;
use crate::race::hud::clock;
use crate::race::{LAPS, Mode, Rules};
use crate::settings::Settings;
use crate::track::Track;
use crate::ui::{
    InputDevice, MenuInput, MenuSound, UiIcons, current_active_device, spawn_title, text_font,
};
use crate::world::CircuitLoading;

/// La cámara gira alrededor de la pista a esta velocidad (rad/s), a esta
/// distancia del centro y a esta altura (fracciones del tamaño de la pista),
/// con este campo de visión (rad).
const AERIAL_SPEED: f32 = 0.06;
const AERIAL_DISTANCE: f32 = 0.7;
const AERIAL_HEIGHT: f32 = 0.5;
const AERIAL_FOV: f32 = 0.8;
/// The track's card is always this wide (px), whatever its text, so that
/// its map never moves from one track to the next: its padding, the map,
/// the gap and room for the widest line of any description (about 713 px).
const CARD_WIDTH: f32 = 1020.0;

pub struct TrackSelectPlugin;

impl Plugin for TrackSelectPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Tracks), spawn_screen)
            .add_systems(
                Update,
                (
                    choose_track,
                    show_track,
                    show_rules,
                    show_legend,
                    // From above, or from the road as the menus before it
                    // (then the start menu's camera flies on, see
                    // `crate::welcome`).
                    aerial_camera.run_if(from_above),
                )
                    .run_if(in_state(GameState::Tracks)),
            );
    }
}

#[derive(Component)]
struct TrackName;

#[derive(Component)]
struct TrackDescription;

#[derive(Component)]
struct TrackLength;

/// Filtros para tocar los textos a la vez.
type NameFilter = (
    With<TrackName>,
    Without<TrackDescription>,
    Without<TrackLength>,
);
type DescriptionFilter = (
    With<TrackDescription>,
    Without<TrackName>,
    Without<TrackLength>,
);
type LengthFilter = (
    With<TrackLength>,
    Without<TrackName>,
    Without<TrackDescription>,
);

/// The best of the track for the mode being played.
#[derive(Component)]
struct TrackRecord;

/// In a race, the laps chosen.
#[derive(Component)]
struct LapsChosen;

/// Punto que marca qué pista se está viendo.
#[derive(Component)]
struct Dot(usize);

fn spawn_screen(
    mut commands: Commands,
    font: Res<UiFont>,
    map: Res<TrackMap>,
    settings: Res<Settings>,
    rules: Res<Rules>,
    circuits: Res<Circuits>,
) {
    let lang = settings.language;
    let mode = rules.mode;
    commands
        .spawn((
            DespawnOnExit(GameState::Tracks),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                // Abajo, sitio para la leyenda.
                padding: UiRect {
                    left: px(24),
                    right: px(24),
                    top: px(20),
                    bottom: px(64),
                },
                ..default()
            },
        ))
        .with_children(|screen| {
            // The title, and under it what is being played.
            screen
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|top| {
                    spawn_title(top, &font, lang.select_track(), 44.0);
                    modes::spawn_mode_badge(top, &font, mode, None, lang);
                });
            // Flechas a los lados, si hay entre qué elegir.
            screen
                .spawn(Node {
                    width: percent(100),
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|arrows| {
                    if circuits.len() > 1 {
                        for arrow in ["<", ">"] {
                            arrows.spawn((
                                Text::new(arrow),
                                text_font(&font, 80.0),
                                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.75)),
                                TextShadow::default(),
                            ));
                        }
                    }
                });
            screen
                .spawn((
                    Node {
                        // As wide on every track; in a narrow window, as
                        // wide as it, its text wrapping.
                        width: px(CARD_WIDTH),
                        max_width: percent(100),
                        align_items: AlignItems::Center,
                        column_gap: px(28),
                        padding: UiRect::axes(px(28), px(18)),
                        border_radius: BorderRadius::all(px(14)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.02, 0.04, 0.1, 0.65)),
                ))
                .with_children(|card| {
                    card.spawn((
                        ImageNode::new(map.image.clone()),
                        Node {
                            width: px(190),
                            height: px(190),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    // The text takes the rest of the card, however long.
                    card.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Start,
                        row_gap: px(10),
                        flex_grow: 1.0,
                        flex_basis: px(0),
                        min_width: px(0),
                        ..default()
                    })
                    .with_children(|text| {
                        text.spawn((
                            TrackName,
                            Text::new(""),
                            text_font(&font, 46.0),
                            TextShadow::default(),
                        ));
                        text.spawn((TrackDescription, Text::new(""), text_font(&font, 20.0)));
                        text.spawn((
                            TrackLength,
                            Text::new(""),
                            text_font(&font, 20.0),
                            TextColor(Color::srgb(1.0, 0.8, 0.2)),
                        ));
                        text.spawn((
                            TrackRecord,
                            Text::new(""),
                            text_font(&font, 18.0),
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
                            // As tall without a record as with one: the card
                            // and its map stay put.
                            Node {
                                min_height: px(22),
                                ..default()
                            },
                        ));
                        if mode == Mode::Race {
                            spawn_laps_chooser(text, &font, lang);
                        }
                        text.spawn(Node {
                            column_gap: px(10),
                            margin: UiRect::vertical(px(4)),
                            ..default()
                        })
                        .with_children(|dots| {
                            for index in 0..circuits.len() {
                                dots.spawn((
                                    Dot(index),
                                    Node {
                                        width: px(12),
                                        height: px(12),
                                        border_radius: BorderRadius::MAX,
                                        ..default()
                                    },
                                    BackgroundColor(Color::WHITE),
                                ));
                            }
                        });
                    });
                });
        });
}

/// The laps of a race: their number between a chevron up and one down
/// (the font has no arrows: its ‹ and › turned), as up and down change
/// them.
fn spawn_laps_chooser(parent: &mut ChildSpawnerCommands, font: &UiFont, lang: Language) {
    let gold = Color::srgb(1.0, 0.8, 0.2);
    parent
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(16),
                padding: UiRect::axes(px(16), px(4)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(match lang {
                    Language::Spanish => "VUELTAS",
                    Language::English => "LAPS",
                }),
                text_font(font, 20.0),
                TextColor(gold),
            ));
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|column| {
                let chevron = |degrees: f32| {
                    (
                        Text::new("›"),
                        text_font(font, 34.0),
                        TextColor(gold),
                        UiTransform::from_rotation(Rot2::degrees(degrees)),
                        Node {
                            height: px(22),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                    )
                };
                column.spawn(chevron(-90.0));
                column.spawn((
                    LapsChosen,
                    Text::new(""),
                    text_font(font, 34.0),
                    TextShadow::default(),
                ));
                column.spawn(chevron(90.0));
            });
        });
}

/// The laps of a race after a step up (-1) or down (1) from `laps`: more
/// going up, within [`LAPS`].
pub fn laps_after(laps: u32, step: i32) -> u32 {
    (laps as i32 - step).clamp(*LAPS.start() as i32, *LAPS.end() as i32) as u32
}

/// Izquierda y derecha cambian de pista; en una carrera, arriba y abajo sus
/// vueltas; Enter, Espacio, A o Start pasan a elegir moto, y Esc o B
/// vuelven a los modos de juego.
/// While the circuit chosen is still on its way ([`CircuitLoading`]),
/// going on to the bikes waits for it.
#[allow(clippy::too_many_arguments)]
fn choose_track(
    mut input: MenuInput,
    mut choice: ResMut<CurrentCircuit>,
    circuits: Res<Circuits>,
    mut rules: ResMut<Rules>,
    mut next: ResMut<NextState<GameState>>,
    mut sounds: MessageWriter<MenuSound>,
    loading: Res<CircuitLoading>,
    mut waiting: Local<bool>,
) {
    let request = input.read();
    if *waiting && !loading.0 {
        *waiting = false;
        next.set(GameState::Select);
        return;
    }
    if request.side != 0 && circuits.len() > 1 {
        choice.0 = (choice.0 as i32 + request.side).rem_euclid(circuits.len() as i32) as usize;
        sounds.write(MenuSound::Move);
        *waiting = false;
    }
    if rules.mode == Mode::Race && request.step != 0 {
        let laps = laps_after(rules.laps, request.step);
        if laps != rules.laps {
            rules.laps = laps;
            sounds.write(MenuSound::Move);
        }
    }
    if request.confirm || request.start {
        sounds.write(MenuSound::Confirm);
        if loading.0 {
            *waiting = true;
        } else {
            next.set(GameState::Select);
        }
    } else if request.back {
        sounds.write(MenuSound::Move);
        *waiting = false;
        next.set(GameState::Modes);
    }
}

/// The best of the track for the mode (the best lap, or against the clock
/// the most laps) and, in a race, its laps.
fn show_rules(
    choice: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    rules: Res<Rules>,
    settings: Res<Settings>,
    mut record: Query<&mut Text, (With<TrackRecord>, Without<LapsChosen>)>,
    mut laps: Query<&mut Text, (With<LapsChosen>, Without<TrackRecord>)>,
) {
    let lang = settings.language;
    let name = circuits.get(*choice).name;
    let records = &settings.records;
    let best = match rules.mode {
        Mode::Checkpoint => records.checkpoint_best(name).map(|laps| match lang {
            Language::Spanish => format!("Récord: {laps} vueltas"),
            Language::English => format!("Record: {laps} laps"),
        }),
        _ => records.best_lap(name).map(|time| match lang {
            Language::Spanish => format!("Récord de vuelta: {}", clock(time)),
            Language::English => format!("Lap record: {}", clock(time)),
        }),
    }
    .unwrap_or_default();
    for mut text in &mut record {
        if text.0 != best {
            text.0.clone_from(&best);
        }
    }
    let count = rules.laps.to_string();
    for mut text in &mut laps {
        if text.0 != count {
            text.0.clone_from(&count);
        }
    }
}

/// Nombre, descripción, longitud y pista elegida.
#[allow(clippy::too_many_arguments)]
fn show_track(
    choice: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    track: Res<Track>,
    settings: Res<Settings>,
    mut name: Single<&mut Text, NameFilter>,
    mut description: Single<&mut Text, DescriptionFilter>,
    mut length: Single<&mut Text, LengthFilter>,
    mut dots: Query<(&Dot, &mut BackgroundColor)>,
    loading: Res<CircuitLoading>,
) {
    let lang = settings.language;
    let circuit = circuits.get(*choice);
    let wanted_name = circuit.name(lang);
    let wanted_description = circuit.description(lang);
    if name.0 != wanted_name {
        name.0 = wanted_name.to_string();
        description.0 = wanted_description.to_string();
    }
    let kilometres = match lang {
        Language::Spanish => format!("{:.1}", track.lap_length() / 1000.0).replace('.', ","),
        Language::English => format!("{:.1}", track.lap_length() / 1000.0),
    };
    // While it is on its way, says so.
    let label = match (lang, loading.0) {
        (Language::Spanish, true) => "Cargando el circuito…".to_string(),
        (Language::English, true) => "Loading the track…".to_string(),
        (Language::Spanish, false) => {
            format!("{kilometres} km por vuelta   ·   {RIDERS} motos")
        }
        (Language::English, false) => {
            format!("{kilometres} km per lap   ·   {RIDERS} bikes")
        }
    };
    if length.0 != label {
        length.0 = label;
    }
    for (dot, mut color) in &mut dots {
        let alpha = if dot.0 == choice.0 { 1.0 } else { 0.3 };
        let wanted = Color::srgba(1.0, 1.0, 1.0, alpha);
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}

/// La leyenda de los controles, con el teclado o el mando que se usa.
#[allow(clippy::too_many_arguments)]
fn show_legend(
    mut commands: Commands,
    settings: Res<Settings>,
    device: Res<InputDevice>,
    active_controller: Option<Res<ActiveController>>,
    font: Res<UiFont>,
    icons: Res<UiIcons>,
    shown: Query<Entity, With<ScreenLegend>>,
    mut last: Local<Option<Vec<KeyPrompt>>>,
) {
    let lang = settings.language;
    let active_dev = current_active_device(*device, active_controller.as_deref());
    let hints = [
        (Keys::Confirm, lang.hint_choose_bike()),
        (Keys::Back, lang.back()),
    ];
    let mut prompts = menu_prompts(active_dev, &hints);
    prompts.push(music_toggle_prompt(
        active_dev,
        lang,
        settings.music_enabled,
    ));
    show_screen_legend(
        &mut commands,
        &shown,
        &mut last,
        prompts,
        active_dev,
        &font,
        &icons,
        GameState::Tracks,
    );
}

/// Whether the track is chosen from the road, as in the menus before it:
/// the start menu's camera goes on flying over it (see
/// [`crate::settings::TrackView`]).
pub fn from_the_road(settings: Res<Settings>) -> bool {
    settings.track_view.at_the_road()
}

/// Whether it is chosen from above, turning round the whole lap.
fn from_above(settings: Res<Settings>) -> bool {
    !settings.track_view.at_the_road()
}

/// La cámara gira despacio alrededor de la pista, desde lo alto.
fn aerial_camera(
    time: Res<Time>,
    track: Res<Track>,
    camera: Single<(&mut Transform, &mut Projection), With<CameraPlayer>>,
    mut glide: ResMut<CameraGlide>,
    mut angle: Local<f32>,
) {
    let dt = time.delta_secs();
    *angle = (*angle + AERIAL_SPEED * dt) % std::f32::consts::TAU;
    let (min, max) = track.bounds();
    let center = (min + max) / 2.0;
    let size = (max - min).max_element();
    let around = Vec2::from_angle(*angle) * size * AERIAL_DISTANCE;
    let position = Vec3::new(
        center.x + around.x,
        size * AERIAL_HEIGHT,
        center.y + around.y,
    );
    let wanted = Transform::from_translation(position)
        .looking_at(Vec3::new(center.x, 0.0, center.y), Vec3::Y);
    let (mut transform, mut projection) = camera.into_inner();
    glide.apply(&mut transform, &mut projection, wanted, AERIAL_FOV, dt);
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;
    use crate::circuits::CurrentCircuit;

    /// Both views: the camera from above runs for one of them, and for
    /// the other the start menu's flies on (see `crate::welcome`).
    #[test]
    fn track_select_systems_initialize_without_conflicts() {
        for view in crate::settings::TRACK_VIEWS {
            track_select_runs_with(view);
        }
    }

    fn track_select_runs_with(view: crate::settings::TrackView) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Font>()
            .init_asset::<Image>()
            .init_state::<GameState>()
            .insert_resource(Settings {
                track_view: view,
                ..Settings::default()
            })
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<MenuSound>()
            .init_resource::<crate::hud::UiFont>()
            .init_resource::<crate::welcome::GameMode>()
            .init_resource::<Rules>()
            .init_resource::<InputDevice>()
            .init_resource::<CurrentCircuit>()
            .insert_resource(*crate::testing::DEMO)
            .init_resource::<CircuitLoading>()
            .init_resource::<CameraGlide>()
            .init_resource::<UiIcons>()
            .add_plugins(TrackSelectPlugin);

        let track = Track::new(&crate::testing::demo_circuit().layout);
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(crate::minimap::draw(&track, &[]));
        app.insert_resource(crate::minimap::TrackMap {
            image,
            projection: crate::minimap::Projection::new(&track),
        });
        app.insert_resource(track);

        app.world_mut().spawn((
            Camera3d::default(),
            Camera::default(),
            Projection::Perspective(PerspectiveProjection::default()),
        ));

        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Tracks);
        app.update();
    }

    #[test]
    fn up_and_down_choose_the_laps_of_a_race_within_their_range() {
        assert_eq!(laps_after(3, -1), 4);
        assert_eq!(laps_after(3, 1), 2);
        assert_eq!(laps_after(*LAPS.start(), 1), *LAPS.start());
        assert_eq!(laps_after(*LAPS.end(), -1), *LAPS.end());
    }
}
