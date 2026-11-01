//! Pantalla de selección de jugador y asignación de controles para 2 jugadores locales.
//!
//! Cada jugador elige un lado para vincular su control:
//! - Player 1 a la izquierda.
//! - Player 2 a la derecha.
//!
//! Controles disponibles:
//! - Teclado WASD.
//! - Teclado Flechas.
//! - Mandos conectados (gamepads).
//!
//! Al pulsar hacia la izquierda (A, Flecha Izquierda o Stick/Cruceta Izquierda),
//! el dispositivo se vincula al Player 1.
//! Al pulsar hacia la derecha (D, Flecha Derecha o Stick/Cruceta Derecha),
//! el dispositivo se vincula al Player 2.
//! Al moverse en dirección contraria o pulsar atrás, el dispositivo se desvincula.
//! Cuando ambos jugadores están listos, con Enter, A o Start se continúa a elegir pista.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::gamepads::ActiveController;
use crate::hud::UiFont;
use crate::legend::{KeyPrompt, Keys, ScreenLegend, menu_prompts, show_screen_legend};
use crate::locale::Language;
use crate::menu::GameState;
use crate::settings::Settings;
use crate::ui::{InputDevice, MenuSound, UiIcons, current_active_device, spawn_title, text_font};

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalPlayersConfig>()
            .add_systems(OnEnter(GameState::Lobby), open_lobby)
            .add_systems(
                Update,
                (handle_lobby_input, update_lobby_ui, show_legend)
                    .chain()
                    .run_if(in_state(GameState::Lobby)),
            );
    }
}

/// Dispositivo o esquema de control vinculado a un jugador.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ControllerBinding {
    /// Teclado usando WASD y Espacio.
    Wasd,
    /// Teclado usando Flechas y Ctrl/Shift/Numpad.
    Arrows,
    /// Un mando específico conectado.
    Gamepad(Entity),
}

impl ControllerBinding {
    pub fn name(self, lang: Language, gamepad_number: Option<usize>) -> String {
        match self {
            ControllerBinding::Wasd => match lang {
                Language::Spanish => "TECLADO: WASD".to_string(),
                Language::English => "KEYBOARD: WASD".to_string(),
            },
            ControllerBinding::Arrows => match lang {
                Language::Spanish => "TECLADO: FLECHAS".to_string(),
                Language::English => "KEYBOARD: ARROWS".to_string(),
            },
            ControllerBinding::Gamepad(_) => {
                let num = gamepad_number.unwrap_or(1);
                match lang {
                    Language::Spanish => format!("MANDO {num}"),
                    Language::English => format!("GAMEPAD {num}"),
                }
            }
        }
    }

    pub fn controls_hint(self, lang: Language) -> &'static str {
        match self {
            ControllerBinding::Wasd => match lang {
                Language::Spanish => {
                    "W: Acelerar   ·   S: Frenar\nA/D: Girar   ·   Espacio: Caballito"
                }
                Language::English => "W: Throttle   ·   S: Brake\nA/D: Steer   ·   Space: Wheelie",
            },
            ControllerBinding::Arrows => match lang {
                Language::Spanish => {
                    "Arriba: Acelerar   ·   Abajo: Frenar\nIzquierda / Derecha: Girar   ·   Ctrl / Mayús der.: Caballito"
                }
                Language::English => {
                    "Up: Throttle   ·   Down: Brake\nLeft / Right: Steer   ·   Right Ctrl / Shift: Wheelie"
                }
            },
            ControllerBinding::Gamepad(_) => match lang {
                Language::Spanish => {
                    "Gatillo R / A: Acelerar   ·   Gatillo L / X: Frenar\nStick / Cruceta: Girar   ·   B: Caballito"
                }
                Language::English => {
                    "R Trigger / A: Throttle   ·   L Trigger / X: Brake\nStick / D-Pad: Steer   ·   B: Wheelie"
                }
            },
        }
    }
}

/// Configuración de vinculación de controles para los 2 jugadores locales.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LocalPlayersConfig {
    pub p1: Option<ControllerBinding>,
    pub p2: Option<ControllerBinding>,
}

impl LocalPlayersConfig {
    pub fn is_ready(&self) -> bool {
        self.p1.is_some() && self.p2.is_some()
    }

    /// Intenta mover un control hacia la izquierda:
    /// - Si estaba en P2, se desvincula.
    /// - Si estaba sin asignar y P1 está libre, se vincula a P1.
    pub fn move_left(&mut self, binding: ControllerBinding) -> bool {
        if self.p2 == Some(binding) {
            self.p2 = None;
            true
        } else if self.p1.is_none() && self.p2 != Some(binding) {
            self.p1 = Some(binding);
            true
        } else {
            false
        }
    }

    /// Intenta mover un control hacia la derecha:
    /// - Si estaba en P1, se desvincula.
    /// - Si estaba sin asignar y P2 está libre, se vincula a P2.
    pub fn move_right(&mut self, binding: ControllerBinding) -> bool {
        if self.p1 == Some(binding) {
            self.p1 = None;
            true
        } else if self.p2.is_none() && self.p1 != Some(binding) {
            self.p2 = Some(binding);
            true
        } else {
            false
        }
    }

    /// Desvincula el control si está asignado a P1 o P2.
    pub fn unbind(&mut self, binding: ControllerBinding) -> bool {
        let mut changed = false;
        if self.p1 == Some(binding) {
            self.p1 = None;
            changed = true;
        }
        if self.p2 == Some(binding) {
            self.p2 = None;
            changed = true;
        }
        changed
    }
}

#[derive(Component)]
struct LobbyScreen;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum LobbyBox {
    P1,
    P2,
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum LobbyText {
    P1Device,
    P1Desc,
    P1Status,
    P2Device,
    P2Desc,
    P2Status,
    DetectedDevices,
}

fn open_lobby(
    mut commands: Commands,
    font: Res<UiFont>,
    settings: Res<Settings>,
    mut config: ResMut<LocalPlayersConfig>,
) {
    // Se reinician las selecciones al entrar a la pantalla de lobby
    *config = LocalPlayersConfig::default();
    let lang = settings.language;

    commands
        .spawn((
            LobbyScreen,
            DespawnOnExit(GameState::Lobby),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(18),
                padding: UiRect::all(px(24)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.02, 0.06, 0.5)),
        ))
        .with_children(|screen| {
            // Título
            spawn_title(screen, &font, lang.select_players(), 44.0);

            // Subtítulo
            screen.spawn((
                Text::new(lang.select_players_subtitle()),
                text_font(&font, 20.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.75)),
                TextShadow::default(),
                Node {
                    margin: UiRect::bottom(px(14)),
                    ..default()
                },
            ));

            // Contenedor de las dos tarjetas de jugadores
            screen
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Stretch,
                    column_gap: px(40),
                    ..default()
                })
                .with_children(|cards| {
                    // --- TARJETA PLAYER 1 (IZQUIERDA) ---
                    cards
                        .spawn((
                            LobbyBox::P1,
                            Node {
                                width: px(380),
                                min_height: px(280),
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::SpaceBetween,
                                align_items: AlignItems::Center,
                                padding: UiRect::all(px(24)),
                                border: UiRect::all(px(3)),
                                border_radius: BorderRadius::all(px(16)),
                                ..default()
                            },
                            BorderColor::all(Color::srgba(0.3, 0.5, 0.8, 0.4)),
                            BackgroundColor(Color::srgba(0.02, 0.06, 0.15, 0.85)),
                        ))
                        .with_children(|card| {
                            // Cabecera P1
                            card.spawn((
                                Text::new(lang.player_1()),
                                text_font(&font, 26.0),
                                TextColor(Color::srgb(0.35, 0.8, 1.0)),
                                TextShadow::default(),
                            ));

                            // Dispositivo vinculado
                            card.spawn((
                                LobbyText::P1Device,
                                Text::new(lang.waiting_player()),
                                text_font(&font, 24.0),
                                TextColor(Color::WHITE),
                                TextShadow::default(),
                                Node {
                                    margin: UiRect::axes(px(0), px(8)),
                                    ..default()
                                },
                            ));

                            // Descripción / guía de controles
                            card.spawn((
                                LobbyText::P1Desc,
                                Text::new(format!(
                                    "{}\n{}",
                                    lang.player_waiting_left(),
                                    lang.player_stick_tip()
                                )),
                                text_font(&font, 16.0),
                                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                                TextLayout::justify(Justify::Center),
                            ));

                            // Estado (Listo / Esperando)
                            card.spawn((
                                LobbyText::P1Status,
                                Text::new(lang.waiting_player()),
                                text_font(&font, 19.0),
                                TextColor(Color::srgba(0.7, 0.7, 0.7, 0.7)),
                                TextShadow::default(),
                            ));
                        });

                    // --- TARJETA PLAYER 2 (DERECHA) ---
                    cards
                        .spawn((
                            LobbyBox::P2,
                            Node {
                                width: px(380),
                                min_height: px(280),
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::SpaceBetween,
                                align_items: AlignItems::Center,
                                padding: UiRect::all(px(24)),
                                border: UiRect::all(px(3)),
                                border_radius: BorderRadius::all(px(16)),
                                ..default()
                            },
                            BorderColor::all(Color::srgba(0.8, 0.5, 0.3, 0.4)),
                            BackgroundColor(Color::srgba(0.02, 0.06, 0.15, 0.85)),
                        ))
                        .with_children(|card| {
                            // Cabecera P2
                            card.spawn((
                                Text::new(lang.player_2()),
                                text_font(&font, 26.0),
                                TextColor(Color::srgb(1.0, 0.65, 0.3)),
                                TextShadow::default(),
                            ));

                            // Dispositivo vinculado
                            card.spawn((
                                LobbyText::P2Device,
                                Text::new(lang.waiting_player()),
                                text_font(&font, 24.0),
                                TextColor(Color::WHITE),
                                TextShadow::default(),
                                Node {
                                    margin: UiRect::axes(px(0), px(8)),
                                    ..default()
                                },
                            ));

                            // Descripción / guía de controles
                            card.spawn((
                                LobbyText::P2Desc,
                                Text::new(format!(
                                    "{}\n{}",
                                    lang.player_waiting_right(),
                                    lang.player_stick_tip()
                                )),
                                text_font(&font, 16.0),
                                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                                TextLayout::justify(Justify::Center),
                            ));

                            // Estado (Listo / Esperando)
                            card.spawn((
                                LobbyText::P2Status,
                                Text::new(lang.waiting_player()),
                                text_font(&font, 19.0),
                                TextColor(Color::srgba(0.7, 0.7, 0.7, 0.7)),
                                TextShadow::default(),
                            ));
                        });
                });

            // Dispositivos detectados
            screen.spawn((
                LobbyText::DetectedDevices,
                Text::new(""),
                text_font(&font, 16.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                TextShadow::default(),
                Node {
                    margin: UiRect::top(px(6)),
                    ..default()
                },
            ));
        });
}

fn handle_lobby_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut config: ResMut<LocalPlayersConfig>,
    mut next: ResMut<NextState<GameState>>,
    mut sounds: MessageWriter<MenuSound>,
    mut stick_left_held: Local<HashMap<Entity, bool>>,
    mut stick_right_held: Local<HashMap<Entity, bool>>,
) {
    // --- Comprobación de mandos desconectados ---
    if matches!(config.p1, Some(ControllerBinding::Gamepad(entity)) if gamepads.get(entity).is_err())
    {
        config.p1 = None;
    }
    if matches!(config.p2, Some(ControllerBinding::Gamepad(entity)) if gamepads.get(entity).is_err())
    {
        config.p2 = None;
    }

    // --- ENTRADA WASD (Teclado) ---
    if keyboard.just_pressed(KeyCode::KeyA) {
        if config.move_left(ControllerBinding::Wasd) {
            sounds.write(MenuSound::Confirm);
        }
    } else if keyboard.just_pressed(KeyCode::KeyD) && config.move_right(ControllerBinding::Wasd) {
        sounds.write(MenuSound::Confirm);
    }

    // --- ENTRADA FLECHAS (Teclado) ---
    if keyboard.just_pressed(KeyCode::ArrowLeft) {
        if config.move_left(ControllerBinding::Arrows) {
            sounds.write(MenuSound::Confirm);
        }
    } else if keyboard.just_pressed(KeyCode::ArrowRight)
        && config.move_right(ControllerBinding::Arrows)
    {
        sounds.write(MenuSound::Confirm);
    }

    // --- ENTRADA MANDOS (Gamepads) ---
    for (entity, gamepad) in &gamepads {
        let stick_x = gamepad.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
        let left_tilted = stick_x < -0.5;
        let right_tilted = stick_x > 0.5;
        let prev_left = stick_left_held.insert(entity, left_tilted).unwrap_or(false);
        let prev_right = stick_right_held
            .insert(entity, right_tilted)
            .unwrap_or(false);

        let left_just_triggered =
            (left_tilted && !prev_left) || gamepad.just_pressed(GamepadButton::DPadLeft);
        let right_just_triggered =
            (right_tilted && !prev_right) || gamepad.just_pressed(GamepadButton::DPadRight);

        let binding = ControllerBinding::Gamepad(entity);
        if left_just_triggered {
            if config.move_left(binding) {
                sounds.write(MenuSound::Confirm);
            }
        } else if right_just_triggered && config.move_right(binding) {
            sounds.write(MenuSound::Confirm);
        }
    }

    // --- CONFIRMAR (Enter, Espacio, A, Start) ---
    let key_confirm =
        keyboard.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]);
    let pad_confirm = gamepads.iter().any(|(_, pad)| {
        pad.just_pressed(GamepadButton::South) || pad.just_pressed(GamepadButton::Start)
    });

    if (key_confirm || pad_confirm) && config.is_ready() {
        sounds.write(MenuSound::Confirm);
        next.set(GameState::Modes);
        return;
    }

    // --- CANCELAR / ATRÁS (Esc, B) ---
    let key_back = keyboard.just_pressed(KeyCode::Escape);
    let mut pad_back_entity = None;
    for (entity, gamepad) in &gamepads {
        if gamepad.just_pressed(GamepadButton::East) {
            pad_back_entity = Some(entity);
            break;
        }
    }

    if key_back {
        // Si hay controles de teclado vinculados, los desvincula primero
        let unbind_wasd = config.unbind(ControllerBinding::Wasd);
        let unbind_arrows = config.unbind(ControllerBinding::Arrows);
        if unbind_wasd || unbind_arrows {
            sounds.write(MenuSound::Move);
        } else if config.p1.is_none() && config.p2.is_none() {
            // Si ninguno está vinculado, vuelve a la pantalla de bienvenida
            sounds.write(MenuSound::Move);
            next.set(GameState::Welcome);
        } else {
            // Si solo quedaban mandos vinculados, se limpian
            config.p1 = None;
            config.p2 = None;
            sounds.write(MenuSound::Move);
        }
    } else if let Some(entity) = pad_back_entity {
        let binding = ControllerBinding::Gamepad(entity);
        if config.unbind(binding) {
            sounds.write(MenuSound::Move);
        } else if config.p1.is_none() && config.p2.is_none() {
            sounds.write(MenuSound::Move);
            next.set(GameState::Welcome);
        }
    }
}

fn update_lobby_ui(
    config: Res<LocalPlayersConfig>,
    settings: Res<Settings>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut boxes: Query<(&mut BorderColor, &LobbyBox)>,
    mut texts: Query<(&mut Text, Option<&mut TextColor>, &LobbyText)>,
) {
    let lang = settings.language;

    let gamepad_order: Vec<Entity> = gamepads.iter().map(|(e, _)| e).collect();
    let get_gamepad_index = |entity: Entity| -> Option<usize> {
        gamepad_order
            .iter()
            .position(|&e| e == entity)
            .map(|i| i + 1)
    };

    let p1_box_color = if config.p1.is_some() {
        BorderColor::all(Color::srgb(0.35, 0.85, 1.0))
    } else {
        BorderColor::all(Color::srgba(0.3, 0.5, 0.8, 0.4))
    };

    let p2_box_color = if config.p2.is_some() {
        BorderColor::all(Color::srgb(1.0, 0.7, 0.3))
    } else {
        BorderColor::all(Color::srgba(0.8, 0.5, 0.3, 0.4))
    };

    for (mut border, box_kind) in &mut boxes {
        *border = match box_kind {
            LobbyBox::P1 => p1_box_color,
            LobbyBox::P2 => p2_box_color,
        };
    }

    let p1_device_str = match config.p1 {
        Some(binding) => {
            let idx = match binding {
                ControllerBinding::Gamepad(e) => get_gamepad_index(e),
                _ => None,
            };
            binding.name(lang, idx)
        }
        None => lang.waiting_player().to_string(),
    };

    let p1_desc_str = match config.p1 {
        Some(binding) => binding.controls_hint(lang).to_string(),
        None => format!(
            "{}\n{}",
            lang.player_waiting_left(),
            lang.player_stick_tip()
        ),
    };

    let (p1_status_str, p1_status_color) = if config.p1.is_some() {
        (
            lang.player_ready().to_string(),
            TextColor(Color::srgb(0.4, 1.0, 0.5)),
        )
    } else {
        (
            lang.waiting_player().to_string(),
            TextColor(Color::srgba(0.7, 0.7, 0.7, 0.6)),
        )
    };

    let p2_device_str = match config.p2 {
        Some(binding) => {
            let idx = match binding {
                ControllerBinding::Gamepad(e) => get_gamepad_index(e),
                _ => None,
            };
            binding.name(lang, idx)
        }
        None => lang.waiting_player().to_string(),
    };

    let p2_desc_str = match config.p2 {
        Some(binding) => binding.controls_hint(lang).to_string(),
        None => format!(
            "{}\n{}",
            lang.player_waiting_right(),
            lang.player_stick_tip()
        ),
    };

    let (p2_status_str, p2_status_color) = if config.p2.is_some() {
        (
            lang.player_ready().to_string(),
            TextColor(Color::srgb(0.4, 1.0, 0.5)),
        )
    } else {
        (
            lang.waiting_player().to_string(),
            TextColor(Color::srgba(0.7, 0.7, 0.7, 0.6)),
        )
    };

    let gamepad_count = gamepad_order.len();
    let wasd_label = if config.p1 == Some(ControllerBinding::Wasd) {
        "WASD (P1)"
    } else if config.p2 == Some(ControllerBinding::Wasd) {
        "WASD (P2)"
    } else {
        "WASD"
    };
    let arrows_label = if config.p1 == Some(ControllerBinding::Arrows) {
        match lang {
            Language::Spanish => "Flechas (P1)",
            Language::English => "Arrows (P1)",
        }
    } else if config.p2 == Some(ControllerBinding::Arrows) {
        match lang {
            Language::Spanish => "Flechas (P2)",
            Language::English => "Arrows (P2)",
        }
    } else {
        match lang {
            Language::Spanish => "Flechas",
            Language::English => "Arrows",
        }
    };

    let pads_label = match (lang, gamepad_count) {
        (Language::Spanish, 0) => "Sin mandos".to_string(),
        (Language::Spanish, 1) => "1 mando conectado".to_string(),
        (Language::Spanish, n) => format!("{n} mandos conectados"),
        (Language::English, 0) => "No gamepads".to_string(),
        (Language::English, 1) => "1 gamepad connected".to_string(),
        (Language::English, n) => format!("{n} gamepads connected"),
    };

    let detected_str = match lang {
        Language::Spanish => {
            format!("Dispositivos: {wasd_label}  ·  {arrows_label}  ·  {pads_label}")
        }
        Language::English => {
            format!("Devices: {wasd_label}  ·  {arrows_label}  ·  {pads_label}")
        }
    };

    for (mut text, mut color, kind) in &mut texts {
        match kind {
            LobbyText::P1Device => {
                if text.0 != p1_device_str {
                    text.0 = p1_device_str.clone();
                }
            }
            LobbyText::P1Desc => {
                if text.0 != p1_desc_str {
                    text.0 = p1_desc_str.clone();
                }
            }
            LobbyText::P1Status => {
                if text.0 != p1_status_str {
                    text.0 = p1_status_str.clone();
                }
                if let Some(ref mut c) = color {
                    **c = p1_status_color;
                }
            }
            LobbyText::P2Device => {
                if text.0 != p2_device_str {
                    text.0 = p2_device_str.clone();
                }
            }
            LobbyText::P2Desc => {
                if text.0 != p2_desc_str {
                    text.0 = p2_desc_str.clone();
                }
            }
            LobbyText::P2Status => {
                if text.0 != p2_status_str {
                    text.0 = p2_status_str.clone();
                }
                if let Some(ref mut c) = color {
                    **c = p2_status_color;
                }
            }
            LobbyText::DetectedDevices => {
                if text.0 != detected_str {
                    text.0 = detected_str.clone();
                }
            }
        }
    }
}

/// La leyenda de los controles: elegir lado, y continuar cuando están los
/// dos jugadores.
#[allow(clippy::too_many_arguments)]
fn show_legend(
    mut commands: Commands,
    config: Res<LocalPlayersConfig>,
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
    let mut hints = Vec::new();
    if config.is_ready() {
        hints.push((Keys::Confirm, lang.hint_continue()));
    }
    hints.push((Keys::Back, lang.back()));
    show_screen_legend(
        &mut commands,
        &shown,
        &mut last,
        menu_prompts(active_dev, &hints),
        active_dev,
        &font,
        &icons,
        GameState::Lobby,
    );
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn local_players_config_moves_left_and_right() {
        let mut config = LocalPlayersConfig::default();
        assert!(!config.is_ready());

        // WASD se mueve a la izquierda -> P1
        assert!(config.move_left(ControllerBinding::Wasd));
        assert_eq!(config.p1, Some(ControllerBinding::Wasd));
        assert_eq!(config.p2, None);
        assert!(!config.is_ready());

        // Flechas se mueve a la derecha -> P2
        assert!(config.move_right(ControllerBinding::Arrows));
        assert_eq!(config.p1, Some(ControllerBinding::Wasd));
        assert_eq!(config.p2, Some(ControllerBinding::Arrows));
        assert!(config.is_ready());

        // Si WASD pulsa derecha, se desvincula de P1
        assert!(config.move_right(ControllerBinding::Wasd));
        assert_eq!(config.p1, None);
        assert_eq!(config.p2, Some(ControllerBinding::Arrows));
        assert!(!config.is_ready());

        // Si Flechas pulsa izquierda, se desvincula de P2
        assert!(config.move_left(ControllerBinding::Arrows));
        assert_eq!(config.p1, None);
        assert_eq!(config.p2, None);
        assert!(!config.is_ready());
    }

    #[test]
    fn cannot_overwrite_occupied_slot() {
        let mut config = LocalPlayersConfig::default();
        assert!(config.move_left(ControllerBinding::Wasd));

        // Flechas intenta ir a la izquierda (P1), pero P1 está ocupado
        assert!(!config.move_left(ControllerBinding::Arrows));
        assert_eq!(config.p1, Some(ControllerBinding::Wasd));

        // Flechas va a la derecha (P2)
        assert!(config.move_right(ControllerBinding::Arrows));
        assert_eq!(config.p2, Some(ControllerBinding::Arrows));
    }

    #[test]
    fn split_screen_race_transition_runs_without_panicking() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::input::InputPlugin)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(crate::content::ContentPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .add_plugins(bevy::audio::AudioPlugin::default())
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .init_asset::<Font>()
            .init_asset::<bevy::render::mesh::skinning::SkinnedMeshInverseBindposes>()
            .init_state::<GameState>()
            .insert_resource(crate::welcome::GameMode::SplitScreen)
            .init_resource::<Settings>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::hud::UiFont>()
            .init_resource::<crate::pause::DebugOptions>()
            .insert_resource(LocalPlayersConfig {
                p1: Some(ControllerBinding::Wasd),
                p2: Some(ControllerBinding::Arrows),
            })
            .init_resource::<crate::circuits::CurrentCircuit>();
        crate::testing::demo_content(&mut app);

        let track = crate::track::Track::new(&crate::testing::demo_circuit().layout);
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(crate::minimap::draw(&track, &[]));
        app.insert_resource(crate::minimap::TrackMap {
            image,
            projection: crate::minimap::Projection::new(&track),
        });
        app.insert_resource(track)
            .init_resource::<crate::menu::Choice>()
            .init_resource::<Time<Virtual>>()
            .add_message::<bevy::app::AppExit>()
            .add_plugins((
                crate::camera::ChaseCameraPlugin,
                crate::bike::BikePlugin,
                crate::gamepads::GamepadsPlugin,
                crate::hud::HudPlugin,
                crate::minimap::MinimapPlugin,
                crate::pause::PausePlugin,
                crate::sound::SoundPlugin,
                crate::ui::UiPlugin,
            ));

        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Race);
        app.update();
    }
}
