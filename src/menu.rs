//! Las pantallas del juego y la de elegir moto y piloto.
//!
//! Se empieza en la de bienvenida ([`crate::welcome`]), de ahí se pasa a
//! elegir pista ([`crate::track_select`]) y luego moto. Las motos de la
//! carrera esperan en la parrilla de salida y la cámara gira despacio
//! alrededor de la elegida; al cambiar de moto se desliza hasta la siguiente.
//! Al confirmar empieza la carrera con esa moto; con Esc o B se vuelve a
//! elegir pista.

use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::bike::{BikeAssets, BikeState, RIDERS, Roster};
use crate::camera::{CameraGlide, CameraPlayer, right_stick};
use crate::gamepads::ActiveController;
use crate::hud::UiFont;
use crate::legend::{KeyPrompt, Keys, ScreenLegend, menu_prompts, show_screen_legend};
use crate::lobby::{ControllerBinding, LocalPlayersConfig};
use crate::locale::Language;
use crate::race::Rules;
use crate::settings::Settings;
use crate::track::Track;
use crate::ui::{InputDevice, MenuInput, MenuSound, UiIcons, current_active_device};
use crate::welcome::GameMode;

/// En qué pantalla está el juego.
#[derive(States, Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GameState {
    /// Bienvenida, con las opciones.
    #[default]
    Welcome,
    /// Elegir jugador y control (2 jugadores locales).
    Lobby,
    /// Choosing what to play: the championship, a race, against the
    /// clock... ([`crate::modes`]).
    Modes,
    /// Elegir pista.
    Tracks,
    /// Elegir moto y piloto.
    Select,
    Race,
}

/// La moto que ha elegido el jugador en modo individual: su índice en [`Roster`].
#[derive(Resource, Default)]
pub struct Choice(pub usize);

/// Las motos elegidas para 2 jugadores en pantalla dividida.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitChoices {
    pub p1: usize,
    pub p2: usize,
    pub p1_ready: bool,
    pub p2_ready: bool,
}

impl Default for SplitChoices {
    fn default() -> Self {
        Self {
            p1: 0,
            p2: 1,
            p1_ready: false,
            p2_ready: false,
        }
    }
}

/// Distancia y altura de la cámara alrededor de la moto, altura a la que
/// mira (m), lo deprisa que da la vuelta (rad/s) y su campo de visión (rad).
const ORBIT_DISTANCE: f32 = 5.0;
const ORBIT_HEIGHT: f32 = 2.2;
const LOOK_HEIGHT: f32 = 0.5;
const ORBIT_SPEED: f32 = 0.25;
const ORBIT_FOV: f32 = 0.65;
/// Lo deprisa que se gira la vista con el stick derecho a tope (rad/s).
const ORBIT_STICK_SPEED: f32 = 2.5;
/// Rapidez con la que la cámara llega a la moto elegida (1/s).
const GLIDE_RESPONSE: f32 = 4.0;
/// Desde dónde se empieza a ver cada moto: de tres cuartos por delante y a
/// la derecha (rad, desde detrás).
const FIRST_VIEW: f32 = 2.3;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .init_resource::<Choice>()
            .init_resource::<SplitChoices>()
            .init_resource::<OrbitState>()
            .add_systems(
                OnEnter(GameState::Select),
                (spawn_showcase, reset_split_choices, spawn_menu, look_afresh),
            )
            .add_systems(
                Update,
                (
                    choose,
                    orbit_camera,
                    show_choice,
                    show_split_choices,
                    show_legend,
                )
                    .run_if(in_state(GameState::Select)),
            );
    }
}

fn reset_split_choices(mut split_choices: ResMut<SplitChoices>) {
    split_choices.p1_ready = false;
    split_choices.p2_ready = false;
}

/// Moto en exposición: su índice en [`Roster`].
#[derive(Component)]
struct Showcase(usize);

// --- Roles de texto para modo individual (evitan conflictos de queries en Bevy) ---
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum SingleSelectRole {
    Name,
    Desc,
}

#[derive(Component)]
struct Dot(usize);

// --- Roles de texto para pantalla dividida ---
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum SplitSelectRole {
    P1Name,
    P1Desc,
    P1Status,
    P2Name,
    P2Desc,
    P2Status,
}

#[derive(Component)]
struct SplitDot {
    player: usize,
    index: usize,
}

/// Las motos en la parrilla de salida, por orden.
fn spawn_showcase(
    mut commands: Commands,
    roster: Res<Roster>,
    track: Res<Track>,
    mut assets: BikeAssets,
) {
    for (index, entry) in roster.iter().enumerate() {
        let state = BikeState::parked_on(&track, track.grid_slot(index));
        let bike = assets.spawn(&mut commands, entry, state);
        commands
            .entity(bike)
            .insert((Showcase(index), DespawnOnExit(GameState::Select)));
    }
}

fn spawn_menu(
    mut commands: Commands,
    font: Res<UiFont>,
    settings: Res<Settings>,
    game_mode: Res<GameMode>,
    local_players: Res<LocalPlayersConfig>,
) {
    let lang = settings.language;
    let text = |size: f32| TextFont {
        font: font.0.clone().into(),
        font_size: FontSize::Px(size),
        ..default()
    };
    let shadow = TextShadow {
        offset: Vec2::splat(3.0),
        color: Color::srgba(0.0, 0.0, 0.0, 0.6),
    };
    let panel = Color::srgba(0.02, 0.04, 0.1, 0.72);

    if *game_mode == GameMode::SplitScreen {
        // Línea divisoria central entre Jugador 1 y Jugador 2
        commands.spawn((
            DespawnOnExit(GameState::Select),
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

        let p1_device_name = local_players
            .p1
            .map(|b| b.name(lang, None))
            .unwrap_or_else(|| "P1".to_string());
        let p2_device_name = local_players
            .p2
            .map(|b| b.name(lang, None))
            .unwrap_or_else(|| "P2".to_string());

        // --- MITAD SUPERIOR: JUGADOR 1 ---
        commands
            .spawn((
                DespawnOnExit(GameState::Select),
                Node {
                    position_type: PositionType::Absolute,
                    top: px(0),
                    left: px(0),
                    width: percent(100),
                    height: percent(50),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(24), px(12)),
                    ..default()
                },
            ))
            .with_children(|screen| {
                screen
                    .spawn(Node {
                        column_gap: px(14),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|header| {
                        header.spawn((
                            Text::new(lang.player_1()),
                            text(26.0),
                            TextColor(Color::srgb(0.35, 0.8, 1.0)),
                            shadow,
                        ));
                        header.spawn((
                            Text::new(format!("({p1_device_name})")),
                            text(18.0),
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                        ));
                    });

                screen
                    .spawn(Node {
                        width: percent(100),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(16)),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new("<"),
                            text(56.0),
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                            shadow,
                        ));

                        row.spawn((
                            Node {
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                row_gap: px(6),
                                padding: UiRect::axes(px(32), px(10)),
                                border_radius: BorderRadius::all(px(12)),
                                border: UiRect::all(px(2)),
                                min_width: px(380),
                                ..default()
                            },
                            BorderColor::all(Color::srgba(0.35, 0.8, 1.0, 0.5)),
                            BackgroundColor(panel),
                        ))
                        .with_children(|card| {
                            card.spawn((
                                SplitSelectRole::P1Name,
                                Text::new(""),
                                TextColor::default(),
                                text(32.0),
                                shadow,
                            ));
                            card.spawn((
                                SplitSelectRole::P1Desc,
                                Text::new(""),
                                TextColor::default(),
                                text(15.0),
                            ));
                            card.spawn(Node {
                                column_gap: px(8),
                                margin: UiRect::vertical(px(4)),
                                ..default()
                            })
                            .with_children(|dots| {
                                for index in 0..RIDERS {
                                    dots.spawn((
                                        SplitDot { player: 0, index },
                                        Node {
                                            width: px(10),
                                            height: px(10),
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(Color::WHITE),
                                    ));
                                }
                            });
                            card.spawn((
                                SplitSelectRole::P1Status,
                                Text::new(""),
                                TextColor::default(),
                                text(16.0),
                                shadow,
                            ));
                        });

                        row.spawn((
                            Text::new(">"),
                            text(56.0),
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                            shadow,
                        ));
                    });

                screen.spawn(Node {
                    height: px(4),
                    ..default()
                });
            });

        // --- MITAD INFERIOR: JUGADOR 2 ---
        commands
            .spawn((
                DespawnOnExit(GameState::Select),
                Node {
                    position_type: PositionType::Absolute,
                    top: percent(50.0),
                    left: px(0),
                    width: percent(100),
                    height: percent(50),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(24), px(12)),
                    ..default()
                },
            ))
            .with_children(|screen| {
                screen
                    .spawn(Node {
                        column_gap: px(14),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|header| {
                        header.spawn((
                            Text::new(lang.player_2()),
                            text(26.0),
                            TextColor(Color::srgb(1.0, 0.65, 0.3)),
                            shadow,
                        ));
                        header.spawn((
                            Text::new(format!("({p2_device_name})")),
                            text(18.0),
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                        ));
                    });

                screen
                    .spawn(Node {
                        width: percent(100),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(16)),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new("<"),
                            text(56.0),
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                            shadow,
                        ));

                        row.spawn((
                            Node {
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                row_gap: px(6),
                                padding: UiRect::axes(px(32), px(10)),
                                border_radius: BorderRadius::all(px(12)),
                                border: UiRect::all(px(2)),
                                min_width: px(380),
                                ..default()
                            },
                            BorderColor::all(Color::srgba(1.0, 0.65, 0.3, 0.5)),
                            BackgroundColor(panel),
                        ))
                        .with_children(|card| {
                            card.spawn((
                                SplitSelectRole::P2Name,
                                Text::new(""),
                                TextColor::default(),
                                text(32.0),
                                shadow,
                            ));
                            card.spawn((
                                SplitSelectRole::P2Desc,
                                Text::new(""),
                                TextColor::default(),
                                text(15.0),
                            ));
                            card.spawn(Node {
                                column_gap: px(8),
                                margin: UiRect::vertical(px(4)),
                                ..default()
                            })
                            .with_children(|dots| {
                                for index in 0..RIDERS {
                                    dots.spawn((
                                        SplitDot { player: 1, index },
                                        Node {
                                            width: px(10),
                                            height: px(10),
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(Color::WHITE),
                                    ));
                                }
                            });
                            card.spawn((
                                SplitSelectRole::P2Status,
                                Text::new(""),
                                TextColor::default(),
                                text(16.0),
                                shadow,
                            ));
                        });

                        row.spawn((
                            Text::new(">"),
                            text(56.0),
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                            shadow,
                        ));
                    });

                // Going back is in the legend; each panel says how to get
                // ready.
            });
    } else {
        commands
            .spawn((
                DespawnOnExit(GameState::Select),
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
                screen.spawn((Text::new(lang.select_bike()), text(44.0), shadow));
                screen
                    .spawn(Node {
                        width: percent(100),
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    })
                    .with_children(|arrows| {
                        for arrow in ["<", ">"] {
                            arrows.spawn((
                                Text::new(arrow),
                                text(80.0),
                                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.75)),
                                shadow,
                            ));
                        }
                    });
                screen
                    .spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: px(10),
                            padding: UiRect::axes(px(40), px(18)),
                            border_radius: BorderRadius::all(px(14)),
                            ..default()
                        },
                        BackgroundColor(panel),
                    ))
                    .with_children(|card| {
                        card.spawn((
                            SingleSelectRole::Name,
                            Text::new(""),
                            TextColor::default(),
                            text(46.0),
                            shadow,
                        ));
                        card.spawn((
                            SingleSelectRole::Desc,
                            Text::new(""),
                            TextColor::default(),
                            text(20.0),
                        ));
                        card.spawn(Node {
                            column_gap: px(10),
                            margin: UiRect::vertical(px(6)),
                            ..default()
                        })
                        .with_children(|dots| {
                            for index in 0..RIDERS {
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
    }
}

struct BindingAction {
    side: i32,
    confirm: bool,
    back: bool,
}

fn read_player_binding(
    binding: Option<ControllerBinding>,
    keyboard: &ButtonInput<KeyCode>,
    gamepads: &Query<(Entity, &Gamepad)>,
    hold: &mut (bool, bool),
) -> BindingAction {
    let Some(binding) = binding else {
        return BindingAction {
            side: 0,
            confirm: false,
            back: false,
        };
    };

    match binding {
        ControllerBinding::Wasd => {
            let left = keyboard.just_pressed(KeyCode::KeyA);
            let right = keyboard.just_pressed(KeyCode::KeyD);
            let side = if right {
                1
            } else if left {
                -1
            } else {
                0
            };
            let confirm = keyboard.just_pressed(KeyCode::Space)
                || keyboard.just_pressed(KeyCode::KeyW)
                || keyboard.just_pressed(KeyCode::Enter);
            let back =
                keyboard.just_pressed(KeyCode::Escape) || keyboard.just_pressed(KeyCode::KeyQ);
            BindingAction {
                side,
                confirm,
                back,
            }
        }
        ControllerBinding::Arrows => {
            let left = keyboard.just_pressed(KeyCode::ArrowLeft);
            let right = keyboard.just_pressed(KeyCode::ArrowRight);
            let side = if right {
                1
            } else if left {
                -1
            } else {
                0
            };
            let confirm = keyboard.just_pressed(KeyCode::Enter)
                || keyboard.just_pressed(KeyCode::ArrowUp)
                || keyboard.just_pressed(KeyCode::ControlRight)
                || keyboard.just_pressed(KeyCode::NumpadEnter);
            let back =
                keyboard.just_pressed(KeyCode::Escape) || keyboard.just_pressed(KeyCode::Backspace);
            BindingAction {
                side,
                confirm,
                back,
            }
        }
        ControllerBinding::Gamepad(entity) => {
            if let Ok((_, gamepad)) = gamepads.get(entity) {
                let stick_x = gamepad.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
                let is_left = stick_x < -0.5;
                let is_right = stick_x > 0.5;
                let left_edge = is_left && !hold.0;
                let right_edge = is_right && !hold.1;
                hold.0 = is_left;
                hold.1 = is_right;

                let left = left_edge || gamepad.just_pressed(GamepadButton::DPadLeft);
                let right = right_edge || gamepad.just_pressed(GamepadButton::DPadRight);
                let side = if right {
                    1
                } else if left {
                    -1
                } else {
                    0
                };
                let confirm = gamepad.just_pressed(GamepadButton::South)
                    || gamepad.just_pressed(GamepadButton::Start);
                let back = gamepad.just_pressed(GamepadButton::East);
                BindingAction {
                    side,
                    confirm,
                    back,
                }
            } else {
                BindingAction {
                    side: 0,
                    confirm: false,
                    back: false,
                }
            }
        }
    }
}

#[derive(Default)]
struct StickHold {
    p1: (bool, bool),
    p2: (bool, bool),
}

#[allow(clippy::too_many_arguments)]
fn choose(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    config: Res<LocalPlayersConfig>,
    game_mode: Res<GameMode>,
    mut choice: ResMut<Choice>,
    mut split_choices: ResMut<SplitChoices>,
    mut next: ResMut<NextState<GameState>>,
    mut sounds: MessageWriter<MenuSound>,
    mut input: MenuInput,
    mut stick_hold: Local<StickHold>,
    rules: Res<Rules>,
) {
    // Back to choosing the track, or in a championship (whose tracks are
    // not chosen) to the modes.
    let back_to = if rules.mode.is_championship() {
        GameState::Modes
    } else {
        GameState::Tracks
    };
    if *game_mode == GameMode::SplitScreen {
        let stick = &mut *stick_hold;
        let p1_action = read_player_binding(config.p1, &keyboard, &gamepads, &mut stick.p1);
        let p2_action = read_player_binding(config.p2, &keyboard, &gamepads, &mut stick.p2);

        // Player 1
        if !split_choices.p1_ready {
            if p1_action.side != 0 {
                split_choices.p1 =
                    (split_choices.p1 as i32 + p1_action.side).rem_euclid(RIDERS as i32) as usize;
                choice.0 = split_choices.p1;
                sounds.write(MenuSound::Move);
            }
            if p1_action.confirm {
                split_choices.p1_ready = true;
                sounds.write(MenuSound::Confirm);
            }
        } else if p1_action.back {
            split_choices.p1_ready = false;
            sounds.write(MenuSound::Move);
        }

        // Player 2
        if !split_choices.p2_ready {
            if p2_action.side != 0 {
                split_choices.p2 =
                    (split_choices.p2 as i32 + p2_action.side).rem_euclid(RIDERS as i32) as usize;
                choice.0 = split_choices.p2;
                sounds.write(MenuSound::Move);
            }
            if p2_action.confirm {
                split_choices.p2_ready = true;
                sounds.write(MenuSound::Confirm);
            }
        } else if p2_action.back {
            split_choices.p2_ready = false;
            sounds.write(MenuSound::Move);
        }

        if split_choices.p1_ready && split_choices.p2_ready {
            sounds.write(MenuSound::Confirm);
            next.set(GameState::Race);
        } else if !split_choices.p1_ready
            && !split_choices.p2_ready
            && (p1_action.back || p2_action.back)
        {
            sounds.write(MenuSound::Move);
            next.set(back_to);
        }
    } else {
        let request = input.read();
        if request.side != 0 {
            choice.0 = (choice.0 as i32 + request.side).rem_euclid(RIDERS as i32) as usize;
        }
        if request.confirm || request.start {
            sounds.write(MenuSound::Confirm);
            next.set(GameState::Race);
        } else if request.back {
            sounds.write(MenuSound::Move);
            next.set(back_to);
        }
    }
}

/// Dónde mira la cámara y cuánto ha girado alrededor de la moto.
#[derive(Default)]
struct Orbit {
    focus: Option<Vec3>,
    angle: f32,
}

#[derive(Resource, Default)]
struct OrbitState {
    p1: Orbit,
    p2: Orbit,
}

/// Coming to the bikes, the camera looks at the bike chosen at once, not
/// from where it looked the last time (another track's bikes, maybe): only
/// changing bike here glides from one to the other.
fn look_afresh(mut orbits: ResMut<OrbitState>) {
    orbits.p1.focus = None;
    orbits.p2.focus = None;
}

#[allow(clippy::too_many_arguments)]
fn orbit_camera(
    time: Res<Time>,
    choice: Res<Choice>,
    split_choices: Res<SplitChoices>,
    game_mode: Res<GameMode>,
    gamepads: Query<&Gamepad>,
    bikes: Query<(&Showcase, &Transform), Without<Camera3d>>,
    mut cameras: Query<(&CameraPlayer, &mut Transform, &mut Projection), With<Camera3d>>,
    mut glide: ResMut<CameraGlide>,
    mut orbits: ResMut<OrbitState>,
) {
    let dt = time.delta_secs();
    let stick = right_stick(&gamepads).x;

    for (cam_player, mut transform, mut projection) in &mut cameras {
        let bike_idx = if *game_mode == GameMode::SplitScreen {
            if cam_player.0 == 0 {
                split_choices.p1
            } else {
                split_choices.p2
            }
        } else {
            choice.0
        };

        let Some((_, bike)) = bikes.iter().find(|(showcase, _)| showcase.0 == bike_idx) else {
            continue;
        };

        let orbit = if cam_player.0 == 0 {
            &mut orbits.p1
        } else {
            &mut orbits.p2
        };

        let target = bike.translation;
        let focus = match orbit.focus {
            Some(focus) => focus.lerp(target, 1.0 - (-GLIDE_RESPONSE * dt).exp()),
            None => target,
        };
        orbit.focus = Some(focus);

        let cam_stick = if cam_player.0 == 0 { stick } else { 0.0 };
        orbit.angle = (orbit.angle + (ORBIT_SPEED - ORBIT_STICK_SPEED * cam_stick) * dt) % TAU;

        let (heading, _, _) = bike.rotation.to_euler(EulerRot::YXZ);
        let around = Quat::from_rotation_y(heading + FIRST_VIEW + orbit.angle) * Vec3::Z;
        let wanted =
            Transform::from_translation(focus + around * ORBIT_DISTANCE + Vec3::Y * ORBIT_HEIGHT)
                .looking_at(focus + Vec3::Y * LOOK_HEIGHT, Vec3::Y);

        if cam_player.0 == 0 && *game_mode == GameMode::SinglePlayer {
            glide.apply(&mut transform, &mut projection, wanted, ORBIT_FOV, dt);
        } else {
            *transform = wanted;
            if let Projection::Perspective(perspective) = projection.as_mut() {
                perspective.fov = ORBIT_FOV;
            }
        }
    }
}

/// Nombre, descripción y puntos de la moto elegida en modo individual.
fn show_choice(
    choice: Res<Choice>,
    roster: Res<Roster>,
    settings: Res<Settings>,
    game_mode: Res<GameMode>,
    mut texts: Query<(&SingleSelectRole, &mut Text, &mut TextColor)>,
    mut dots: Query<(&Dot, &mut BackgroundColor)>,
) {
    if *game_mode != GameMode::SinglePlayer {
        return;
    }
    let entry = &roster[choice.0];
    let wanted_desc = entry.description(settings.language);

    for (role, mut text, mut color) in &mut texts {
        match role {
            SingleSelectRole::Name => {
                if text.0 != entry.name {
                    text.0 = entry.name.to_string();
                    color.0 = entry.color.into();
                }
            }
            SingleSelectRole::Desc => {
                if text.0 != wanted_desc {
                    text.0 = wanted_desc.to_string();
                }
            }
        }
    }

    for (dot, mut color) in &mut dots {
        let alpha = if dot.0 == choice.0 { 1.0 } else { 0.3 };
        let wanted = Color::srgba(1.0, 1.0, 1.0, alpha);
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}

/// La leyenda de los controles, con el teclado o el mando que se usa. With
/// two players each panel says how to get ready.
#[allow(clippy::too_many_arguments)]
fn show_legend(
    mut commands: Commands,
    settings: Res<Settings>,
    game_mode: Res<GameMode>,
    device: Res<InputDevice>,
    active_controller: Option<Res<ActiveController>>,
    font: Res<UiFont>,
    icons: Res<UiIcons>,
    shown: Query<Entity, With<ScreenLegend>>,
    mut last: Local<Option<Vec<KeyPrompt>>>,
) {
    let lang = settings.language;
    let active_dev = current_active_device(*device, active_controller.as_deref());
    let hints = if *game_mode == GameMode::SinglePlayer {
        vec![(Keys::Confirm, lang.hint_race()), (Keys::Back, lang.back())]
    } else {
        vec![(Keys::Back, lang.back())]
    };
    show_screen_legend(
        &mut commands,
        &shown,
        &mut last,
        menu_prompts(active_dev, &hints),
        active_dev,
        &font,
        &icons,
        GameState::Select,
    );
}

/// Nombre, descripción, estado y puntos de cada moto en modo pantalla dividida.
fn show_split_choices(
    split_choices: Res<SplitChoices>,
    roster: Res<Roster>,
    settings: Res<Settings>,
    local_players: Res<LocalPlayersConfig>,
    game_mode: Res<GameMode>,
    mut texts: Query<(&SplitSelectRole, &mut Text, &mut TextColor)>,
    mut dots: Query<(&SplitDot, &mut BackgroundColor)>,
) {
    if *game_mode != GameMode::SplitScreen {
        return;
    }
    let lang = settings.language;

    let e1 = &roster[split_choices.p1];
    let d1 = e1.description(lang);
    let p1_hint = if split_choices.p1_ready {
        (
            match lang {
                Language::Spanish => "LISTO (atrás para cambiar)".to_string(),
                Language::English => "READY (back to change)".to_string(),
            },
            Color::srgb(0.3, 1.0, 0.4),
        )
    } else {
        let key_hint = match local_players.p1 {
            Some(ControllerBinding::Wasd) => "Espacio / W",
            Some(ControllerBinding::Arrows) => "Enter",
            Some(ControllerBinding::Gamepad(_)) => "A / Start",
            None => "Enter",
        };
        (
            match lang {
                Language::Spanish => format!("< > Elegir   ·   {key_hint}: Listo"),
                Language::English => format!("< > Choose   ·   {key_hint}: Ready"),
            },
            Color::srgba(1.0, 1.0, 1.0, 0.85),
        )
    };

    let e2 = &roster[split_choices.p2];
    let d2 = e2.description(lang);
    let p2_hint = if split_choices.p2_ready {
        (
            match lang {
                Language::Spanish => "LISTO (atrás para cambiar)".to_string(),
                Language::English => "READY (back to change)".to_string(),
            },
            Color::srgb(0.3, 1.0, 0.4),
        )
    } else {
        let key_hint = match local_players.p2 {
            Some(ControllerBinding::Wasd) => "Espacio / W",
            Some(ControllerBinding::Arrows) => "Enter",
            Some(ControllerBinding::Gamepad(_)) => "A / Start",
            None => "Enter",
        };
        (
            match lang {
                Language::Spanish => format!("< > Elegir   ·   {key_hint}: Listo"),
                Language::English => format!("< > Choose   ·   {key_hint}: Ready"),
            },
            Color::srgba(1.0, 1.0, 1.0, 0.85),
        )
    };

    for (role, mut text, mut color) in &mut texts {
        match role {
            SplitSelectRole::P1Name => {
                if text.0 != e1.name {
                    text.0 = e1.name.to_string();
                    color.0 = e1.color.into();
                }
            }
            SplitSelectRole::P1Desc => {
                if text.0 != d1 {
                    text.0 = d1.to_string();
                }
            }
            SplitSelectRole::P1Status => {
                if text.0 != p1_hint.0 {
                    text.0 = p1_hint.0.clone();
                    color.0 = p1_hint.1;
                }
            }
            SplitSelectRole::P2Name => {
                if text.0 != e2.name {
                    text.0 = e2.name.to_string();
                    color.0 = e2.color.into();
                }
            }
            SplitSelectRole::P2Desc => {
                if text.0 != d2 {
                    text.0 = d2.to_string();
                }
            }
            SplitSelectRole::P2Status => {
                if text.0 != p2_hint.0 {
                    text.0 = p2_hint.0.clone();
                    color.0 = p2_hint.1;
                }
            }
        }
    }

    for (dot, mut color) in &mut dots {
        let chosen = if dot.player == 0 {
            split_choices.p1
        } else {
            split_choices.p2
        };
        let alpha = if dot.index == chosen { 1.0 } else { 0.3 };
        let wanted = Color::srgba(1.0, 1.0, 1.0, alpha);
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;
    use crate::circuits::CurrentCircuit;

    pub fn split_screen_select_initializes_and_updates_without_conflict_on(
        circuits: &'static crate::circuits::Circuits,
        roster: &'static crate::bike::Roster,
    ) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Font>()
            .init_asset::<Image>()
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<bevy::render::mesh::skinning::SkinnedMeshInverseBindposes>()
            .init_state::<GameState>()
            .init_resource::<Settings>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<MenuSound>()
            .init_resource::<UiFont>()
            .insert_resource(GameMode::SplitScreen)
            .insert_resource(LocalPlayersConfig {
                p1: Some(ControllerBinding::Wasd),
                p2: Some(ControllerBinding::Arrows),
            })
            .init_resource::<InputDevice>()
            .init_resource::<UiIcons>()
            .init_resource::<CurrentCircuit>()
            .init_resource::<CameraGlide>()
            .init_resource::<Rules>()
            .insert_resource(*circuits)
            .insert_resource(*roster)
            .add_plugins(crate::camera::ChaseCameraPlugin)
            .add_plugins(crate::bike::BikePlugin)
            .add_plugins(MenuPlugin);

        let track = Track::new(&circuits[0].layout);
        app.insert_resource(track);

        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Select);
        app.update();

        // Verificar que SplitChoices esté inicializado
        let split_choices = app.world().resource::<SplitChoices>();
        assert_eq!(split_choices.p1, 0);
        assert_eq!(split_choices.p2, 1);
        assert!(!split_choices.p1_ready);
        assert!(!split_choices.p2_ready);
    }

    #[test]
    fn split_screen_select_initializes_and_updates_without_conflict() {
        split_screen_select_initializes_and_updates_without_conflict_on(
            &crate::testing::DEMO,
            &crate::testing::DEMO_ROSTER,
        );
    }
}
