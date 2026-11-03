//! The editor's interface: a bar along the top (the circuit, the modes,
//! saving, undoing, the help, leaving), a column of buttons down the left
//! for what the mode in use does, each with its key and what it does beside
//! it, and the whole legend in a window that Tab opens and closes.
//!
//! What the editor does is an [`Action`]; [`BINDINGS`] says which key does
//! which in which mode, what it is called (in Spanish and English, as the
//! game's language is) and whether it has a button. The keys
//! ([`read_keys`]) and the buttons ([`press_buttons`]) both put actions in
//! [`Actions`], which the editor's systems carry out the same whichever
//! gave them; the legend beside each button and in the window come from the
//! same table, so they never say other than what the keys do.

use bevy::prelude::*;

use super::roadside::Kind;
use super::terrain::Shaping;
use super::{Editor, Mode};
use crate::hud::UiFont;
use crate::locale::Language;
use crate::menu::GameState;
use crate::settings::Settings;
use crate::world::sculpt::{Paint, Tool};

/// Something the editor does, asked for by a key or a button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    // Anywhere.
    CycleMode,
    SetMode(Mode),
    Save,
    Undo,
    Redo,
    Help,
    Exit,
    PrevCircuit,
    NextCircuit,
    ShowAll,
    Next,
    Prev,
    // The lap.
    AddAfter,
    Remove,
    AddBanking,
    FactorDown,
    FactorUp,
    ToggleRaised,
    GripUp,
    GripDown,
    // The terrain.
    Tool(Tool),
    Smaller,
    Bigger,
    Weaker,
    Stronger,
    // By the road.
    Adding(Kind),
    AddHere,
    Flip,
}

/// Where a key does what it does: in every mode, or in one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Anywhere,
    In(Mode),
}

/// A key (with Ctrl or Shift held, or not), what it does where, what that
/// is called, and whether it has a button.
pub struct Binding {
    pub scope: Scope,
    pub key: KeyCode,
    /// Another key that does the same, if any (not shown).
    pub also: Option<KeyCode>,
    pub ctrl: bool,
    pub shift: bool,
    pub action: Action,
    pub spanish: &'static str,
    pub english: &'static str,
    pub button: bool,
}

#[allow(clippy::too_many_arguments)]
const fn bind(
    scope: Scope,
    key: KeyCode,
    ctrl: bool,
    shift: bool,
    action: Action,
    spanish: &'static str,
    english: &'static str,
    button: bool,
) -> Binding {
    Binding {
        scope,
        key,
        also: None,
        ctrl,
        shift,
        action,
        spanish,
        english,
        button,
    }
}

const ANY: Scope = Scope::Anywhere;
const LAP: Scope = Scope::In(Mode::Lap);
const TERRAIN: Scope = Scope::In(Mode::Terrain);
const ROAD: Scope = Scope::In(Mode::Roadside);

/// Every key of the editor and what it does: the one table the keys, the
/// buttons and the legends come from.
pub const BINDINGS: &[Binding] = &[
    bind(
        ANY,
        KeyCode::KeyM,
        false,
        false,
        Action::CycleMode,
        "Cambiar de modo",
        "Switch mode",
        false,
    ),
    bind(
        ANY,
        KeyCode::KeyS,
        true,
        false,
        Action::Save,
        "Guardar",
        "Save",
        true,
    ),
    bind(
        ANY,
        KeyCode::KeyZ,
        true,
        false,
        Action::Undo,
        "Deshacer",
        "Undo",
        true,
    ),
    bind(
        ANY,
        KeyCode::KeyY,
        true,
        false,
        Action::Redo,
        "Rehacer",
        "Redo",
        true,
    ),
    bind(
        ANY,
        KeyCode::KeyZ,
        true,
        true,
        Action::Redo,
        "Rehacer",
        "Redo",
        false,
    ),
    bind(
        ANY,
        KeyCode::Tab,
        false,
        false,
        Action::Help,
        "Ayuda",
        "Help",
        true,
    ),
    bind(
        ANY,
        KeyCode::Escape,
        false,
        false,
        Action::Exit,
        "Salir",
        "Leave",
        true,
    ),
    bind(
        ANY,
        KeyCode::PageUp,
        false,
        false,
        Action::PrevCircuit,
        "Circuito anterior",
        "Last circuit",
        true,
    ),
    bind(
        ANY,
        KeyCode::PageDown,
        false,
        false,
        Action::NextCircuit,
        "Circuito siguiente",
        "Next circuit",
        true,
    ),
    bind(
        ANY,
        KeyCode::Home,
        false,
        false,
        Action::ShowAll,
        "Ver todo el circuito",
        "Whole circuit in view",
        true,
    ),
    bind(
        ANY,
        KeyCode::Space,
        false,
        false,
        Action::Next,
        "Elegir el siguiente",
        "Pick the next",
        false,
    ),
    bind(
        ANY,
        KeyCode::Space,
        false,
        true,
        Action::Prev,
        "Elegir el anterior",
        "Pick the last",
        false,
    ),
    Binding {
        also: Some(KeyCode::Insert),
        ..bind(
            LAP,
            KeyCode::KeyN,
            false,
            false,
            Action::AddAfter,
            "Añadir tras el elegido (punto, marca) o partir el tramo",
            "Add after the one picked (point, mark), or split the stretch",
            true,
        )
    },
    Binding {
        also: Some(KeyCode::Backspace),
        ..bind(
            LAP,
            KeyCode::Delete,
            false,
            false,
            Action::Remove,
            "Quitar el elegido",
            "Take the one picked out",
            true,
        )
    },
    bind(
        LAP,
        KeyCode::KeyB,
        false,
        false,
        Action::AddBanking,
        "Tramo de peralte (en el cursor del panel)",
        "Banking (at the panel's cursor)",
        true,
    ),
    bind(
        LAP,
        KeyCode::BracketLeft,
        false,
        false,
        Action::FactorDown,
        "Menos peralte en el tramo",
        "Less banking on the stretch",
        true,
    ),
    bind(
        LAP,
        KeyCode::BracketRight,
        false,
        false,
        Action::FactorUp,
        "Más peralte en el tramo",
        "More banking on the stretch",
        true,
    ),
    bind(
        LAP,
        KeyCode::KeyT,
        false,
        false,
        Action::ToggleRaised,
        "Tramo elevado sobre soportes, o no",
        "Stretch raised on supports, or not",
        true,
    ),
    bind(
        LAP,
        KeyCode::KeyG,
        false,
        false,
        Action::GripUp,
        "Más agarre en el tramo",
        "More grip on the stretch",
        true,
    ),
    bind(
        LAP,
        KeyCode::KeyG,
        false,
        true,
        Action::GripDown,
        "Menos agarre en el tramo",
        "Less grip on the stretch",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::Digit1,
        false,
        false,
        Action::Tool(Tool::Raise),
        "Subir",
        "Raise",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::Digit2,
        false,
        false,
        Action::Tool(Tool::Lower),
        "Bajar",
        "Lower",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::Digit3,
        false,
        false,
        Action::Tool(Tool::Smooth),
        "Suavizar",
        "Smooth",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::Digit4,
        false,
        false,
        Action::Tool(Tool::Flatten),
        "Aplanar",
        "Flatten",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::Digit5,
        false,
        false,
        Action::Tool(Tool::Restore),
        "Restaurar",
        "Restore",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::Digit6,
        false,
        false,
        Action::Tool(Tool::Paint(Paint::Grass)),
        "Pintar hierba",
        "Paint grass",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::Digit7,
        false,
        false,
        Action::Tool(Tool::Paint(Paint::Sand)),
        "Pintar arena",
        "Paint sand",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::Digit8,
        false,
        false,
        Action::Tool(Tool::Paint(Paint::Rock)),
        "Pintar roca",
        "Paint rock",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::Digit9,
        false,
        false,
        Action::Tool(Tool::Paint(Paint::Land)),
        "Quitar pintura",
        "Unpaint",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::BracketLeft,
        false,
        false,
        Action::Smaller,
        "Pincel más pequeño (o Mayús+rueda)",
        "Smaller brush (or Shift+wheel)",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::BracketRight,
        false,
        false,
        Action::Bigger,
        "Pincel más grande (o Mayús+rueda)",
        "Bigger brush (or Shift+wheel)",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::Comma,
        false,
        false,
        Action::Weaker,
        "Pincel más suave (o Ctrl+rueda)",
        "Weaker brush (or Ctrl+wheel)",
        true,
    ),
    bind(
        TERRAIN,
        KeyCode::Period,
        false,
        false,
        Action::Stronger,
        "Pincel más fuerte (o Ctrl+rueda)",
        "Stronger brush (or Ctrl+wheel)",
        true,
    ),
    bind(
        ROAD,
        KeyCode::Digit1,
        false,
        false,
        Action::Adding(Kind::Gantry),
        "Arco (checkpoint)",
        "Arch (checkpoint)",
        true,
    ),
    bind(
        ROAD,
        KeyCode::Digit2,
        false,
        false,
        Action::Adding(Kind::Billboard),
        "Cartel",
        "Billboard",
        true,
    ),
    bind(
        ROAD,
        KeyCode::Digit3,
        false,
        false,
        Action::Adding(Kind::Chevron),
        "Señal de curva",
        "Chevron sign",
        true,
    ),
    bind(
        ROAD,
        KeyCode::Digit4,
        false,
        false,
        Action::Adding(Kind::BrakeMarker),
        "Marcador de frenada",
        "Brake marker",
        true,
    ),
    bind(
        ROAD,
        KeyCode::Digit5,
        false,
        false,
        Action::Adding(Kind::Grandstand),
        "Tribuna",
        "Grandstand",
        true,
    ),
    bind(
        ROAD,
        KeyCode::KeyN,
        false,
        false,
        Action::AddHere,
        "Añadir en el cursor (o en el centro)",
        "Add at the cursor (or the middle)",
        true,
    ),
    bind(
        ROAD,
        KeyCode::KeyX,
        false,
        false,
        Action::Flip,
        "Al otro lado de la carretera",
        "To the other side of the road",
        true,
    ),
    Binding {
        also: Some(KeyCode::Backspace),
        ..bind(
            ROAD,
            KeyCode::Delete,
            false,
            false,
            Action::Remove,
            "Quitar el elegido",
            "Take the one picked out",
            true,
        )
    },
];

/// What the mouse and the camera's keys do, for the legend (no buttons).
const MOUSE: &[(&str, &str, &str)] = &[
    (
        "Clic izq.",
        "Elegir y arrastrar (puntos en el mapa; marcas y tramos en el panel; el pincel; lo de la carretera)",
        "Pick and drag (points on the map; marks and stretches on the panel; the brush; what stands by the road)",
    ),
    ("Clic der.", "Mover la vista", "Move the view"),
    ("Rueda", "Acercar / alejar", "Zoom in / out"),
    ("W A S D", "Mover la vista", "Move the view"),
    ("Q / E", "Girar", "Turn"),
    ("R / F", "Inclinar", "Tilt"),
];

impl Binding {
    pub fn label(&self, language: Language) -> &'static str {
        match language {
            Language::Spanish => self.spanish,
            Language::English => self.english,
        }
    }

    /// Its key as written on its badge: `Ctrl+S`, `Mayús+G`, `[`.
    pub fn keys(&self, language: Language) -> String {
        let mut keys = String::new();
        if self.ctrl {
            keys.push_str("Ctrl+");
        }
        if self.shift {
            keys.push_str(match language {
                Language::Spanish => "Mayús+",
                Language::English => "Shift+",
            });
        }
        keys.push_str(&key_name(self.key, language));
        keys
    }

    fn applies(&self, mode: Mode) -> bool {
        match self.scope {
            Scope::Anywhere => true,
            Scope::In(scope) => scope == mode,
        }
    }
}

/// A key's name, short.
pub fn key_name(key: KeyCode, language: Language) -> String {
    let spanish = language == Language::Spanish;
    match key {
        KeyCode::Escape => "Esc".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::Space => if spanish { "Espacio" } else { "Space" }.into(),
        KeyCode::Delete => if spanish { "Supr" } else { "Del" }.into(),
        KeyCode::Backspace => "⌫".into(),
        KeyCode::Insert => "Ins".into(),
        KeyCode::PageUp => if spanish { "RePág" } else { "PgUp" }.into(),
        KeyCode::PageDown => if spanish { "AvPág" } else { "PgDn" }.into(),
        KeyCode::Home => if spanish { "Inicio" } else { "Home" }.into(),
        KeyCode::BracketLeft => "[".into(),
        KeyCode::BracketRight => "]".into(),
        KeyCode::Comma => ",".into(),
        KeyCode::Period => ".".into(),
        other => {
            let name = format!("{other:?}");
            name.strip_prefix("Key")
                .or_else(|| name.strip_prefix("Digit"))
                .unwrap_or(&name)
                .to_string()
        }
    }
}

/// The actions asked for this frame, by the keys and the buttons, and
/// whether the legend's window is open.
#[derive(Resource, Default)]
pub struct Actions {
    pub asked: Vec<Action>,
    pub help: bool,
    /// Whether the mouse is over the editor's interface (its clicks are
    /// not the map's).
    pub over_ui: bool,
}

impl Actions {
    pub fn has(&self, action: Action) -> bool {
        self.asked.contains(&action)
    }
}

/// The actions the keys ask for: those of the mode in use, Ctrl and Shift
/// as each says; with the legend open, only closing it.
pub fn read_keys(
    keys: Res<ButtonInput<KeyCode>>,
    editor: Res<Editor>,
    mut actions: ResMut<Actions>,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if actions.help {
        if keys.any_just_pressed([KeyCode::Tab, KeyCode::Escape]) {
            actions.asked.push(Action::Help);
        }
        return;
    }
    for binding in BINDINGS {
        let pressed = keys.just_pressed(binding.key)
            || binding.also.is_some_and(|key| keys.just_pressed(key));
        if pressed && binding.ctrl == ctrl && binding.shift == shift && binding.applies(editor.mode)
        {
            actions.asked.push(binding.action);
        }
    }
}

/// A button of the interface, and the action it asks for.
#[derive(Component)]
pub struct ActionButton(pub Action);

/// What belongs to the editor's interface (the mouse over it is not over
/// the map).
#[derive(Component)]
pub struct EditorUi;

/// The column of a mode's buttons, shown in that mode.
#[derive(Component)]
pub struct ModeColumn(pub Mode);

/// The legend's window.
#[derive(Component)]
pub struct HelpWindow;

/// The texts the interface keeps up to date: the circuit and its lap's
/// figures (the top bar), what is picked and how the brush is (the
/// column), and what the editor last said.
#[derive(Component)]
pub enum UiText {
    Title,
    Figures,
    Info,
    Message,
}

/// The actions the buttons pressed ask for, and whether the mouse is over
/// the interface.
pub fn press_buttons(
    buttons: Query<(&Interaction, &ActionButton), Changed<Interaction>>,
    hovered: Query<&Interaction, With<EditorUi>>,
    mut actions: ResMut<Actions>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            actions.asked.push(button.0);
        }
    }
    actions.over_ui = hovered
        .iter()
        .any(|interaction| *interaction != Interaction::None);
}

/// The legend's window open or shut, as asked.
pub fn toggle_help(mut actions: ResMut<Actions>) {
    if actions.has(Action::Help) {
        actions.help = !actions.help;
    }
}

/// The actions asked for, done with.
pub fn clear_actions(mut actions: ResMut<Actions>) {
    actions.asked.clear();
}

/// Colours of the interface.
const BAR: Color = Color::srgba(0.05, 0.06, 0.08, 0.88);
const BUTTON: Color = Color::srgba(0.16, 0.18, 0.22, 0.95);
const HOVERED: Color = Color::srgba(0.26, 0.3, 0.36, 0.98);
const CHOSEN: Color = Color::srgb(0.04, 0.4, 1.0);
const BADGE: Color = Color::srgb(0.9, 0.9, 0.92);
const BADGE_INK: Color = Color::srgb(0.08, 0.08, 0.1);
const DIM: Color = Color::srgba(0.85, 0.87, 0.9, 0.85);

/// A button: its key on a badge and what it does beside it.
fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    action: Action,
    keys: &str,
    label: &str,
    grow: bool,
) {
    parent
        .spawn((
            ActionButton(action),
            EditorUi,
            Button,
            Interaction::default(),
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
                padding: UiRect::axes(px(8), px(5)),
                border_radius: BorderRadius::all(px(5)),
                flex_grow: if grow { 1.0 } else { 0.0 },
                ..default()
            },
            BackgroundColor(BUTTON),
        ))
        .with_children(|button| {
            if !keys.is_empty() {
                button
                    .spawn((
                        Node {
                            padding: UiRect::axes(px(5), px(1)),
                            border_radius: BorderRadius::all(px(3)),
                            min_width: px(22),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(BADGE),
                    ))
                    .with_children(|badge| {
                        badge.spawn((
                            Text::new(keys),
                            crate::ui::text_font(font, 13.0),
                            TextColor(BADGE_INK),
                        ));
                    });
            }
            button.spawn((
                Text::new(label),
                crate::ui::text_font(font, 14.0),
                TextColor(Color::WHITE),
            ));
        });
}

/// The bindings with a button, of `scope`, in the table's order.
fn buttons_of(scope: Scope) -> impl Iterator<Item = &'static Binding> {
    BINDINGS
        .iter()
        .filter(move |binding| binding.button && binding.scope == scope)
}

/// The interface: the top bar, a column for each mode and the legend's
/// window (shut).
pub fn spawn_ui(mut commands: Commands, font: Res<UiFont>, settings: Option<Res<Settings>>) {
    let language = settings.map_or(Language::default(), |settings| settings.language);
    let font = &*font;
    let names = |mode: Mode| match (mode, language) {
        (Mode::Lap, Language::Spanish) => "Trazado",
        (Mode::Lap, Language::English) => "Lap",
        (Mode::Terrain, Language::Spanish) => "Terreno",
        (Mode::Terrain, Language::English) => "Terrain",
        (Mode::Roadside, Language::Spanish) => "Junto a la carretera",
        (Mode::Roadside, Language::English) => "By the road",
    };
    // The top bar.
    commands
        .spawn((
            DespawnOnExit(GameState::Editor),
            EditorUi,
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                top: px(0),
                left: px(0),
                width: percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(12), px(6)),
                row_gap: px(4),
                ..default()
            },
            BackgroundColor(BAR),
        ))
        .with_children(|bar| {
            bar.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    UiText::Title,
                    Text::new("EDITOR"),
                    crate::ui::text_font(font, 18.0),
                    TextColor(Color::WHITE),
                    Node {
                        margin: UiRect::right(px(10)),
                        ..default()
                    },
                ));
                for mode in [Mode::Lap, Mode::Terrain, Mode::Roadside] {
                    spawn_button(row, font, Action::SetMode(mode), "", names(mode), false);
                }
                row.spawn((
                    Text::new(match language {
                        Language::Spanish => "M: cambiar",
                        Language::English => "M: switch",
                    }),
                    crate::ui::text_font(font, 13.0),
                    TextColor(DIM),
                ));
                // Pushed to the right.
                row.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                for binding in buttons_of(ANY) {
                    spawn_button(
                        row,
                        font,
                        binding.action,
                        &binding.keys(language),
                        binding.label(language),
                        false,
                    );
                }
            });
            bar.spawn((
                UiText::Figures,
                Text::new(""),
                crate::ui::text_font(font, 13.0),
                TextColor(DIM),
            ));
        });
    // A column of buttons for each mode, down the left.
    for mode in [Mode::Lap, Mode::Terrain, Mode::Roadside] {
        commands
            .spawn((
                DespawnOnExit(GameState::Editor),
                EditorUi,
                ModeColumn(mode),
                Interaction::default(),
                Node {
                    position_type: PositionType::Absolute,
                    top: px(86),
                    left: px(12),
                    width: px(330),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(10)),
                    row_gap: px(5),
                    border_radius: BorderRadius::all(px(8)),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(BAR),
            ))
            .with_children(|column| {
                column.spawn((
                    Text::new(names(mode).to_uppercase()),
                    crate::ui::text_font(font, 16.0),
                    TextColor(Color::WHITE),
                ));
                for binding in buttons_of(Scope::In(mode)) {
                    spawn_button(
                        column,
                        font,
                        binding.action,
                        &binding.keys(language),
                        binding.label(language),
                        false,
                    );
                }
                column.spawn((
                    UiText::Info,
                    Text::new(""),
                    crate::ui::text_font(font, 13.0),
                    TextColor(DIM),
                    Node {
                        margin: UiRect::top(px(6)),
                        ..default()
                    },
                ));
                column.spawn((
                    UiText::Message,
                    Text::new(""),
                    crate::ui::text_font(font, 13.0),
                    TextColor(Color::srgb(1.0, 0.85, 0.4)),
                ));
            });
    }
    // The legend's window, shut.
    commands
        .spawn((
            DespawnOnExit(GameState::Editor),
            EditorUi,
            HelpWindow,
            Interaction::default(),
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .with_children(|shade| {
            shade
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(18)),
                        row_gap: px(10),
                        border_radius: BorderRadius::all(px(10)),
                        max_width: percent(92.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.07, 0.08, 0.1, 0.97)),
                ))
                .with_children(|window| {
                    window.spawn((
                        Text::new(match language {
                            Language::Spanish => "TECLAS DEL EDITOR   (Tab o Esc para cerrar)",
                            Language::English => "THE EDITOR'S KEYS   (Tab or Esc to close)",
                        }),
                        crate::ui::text_font(font, 18.0),
                        TextColor(Color::WHITE),
                    ));
                    window
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(28),
                            flex_wrap: FlexWrap::Wrap,
                            ..default()
                        })
                        .with_children(|columns| {
                            let groups: [(&str, &str, Option<Scope>); 5] = [
                                ("General", "Anywhere", Some(ANY)),
                                ("Ratón y vista", "Mouse and view", None),
                                (names(Mode::Lap), names(Mode::Lap), Some(LAP)),
                                (names(Mode::Terrain), names(Mode::Terrain), Some(TERRAIN)),
                                (names(Mode::Roadside), names(Mode::Roadside), Some(ROAD)),
                            ];
                            for (spanish, english, scope) in groups {
                                columns
                                    .spawn(Node {
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(3),
                                        max_width: px(330),
                                        ..default()
                                    })
                                    .with_children(|group| {
                                        let title = match language {
                                            Language::Spanish => spanish,
                                            Language::English => english,
                                        };
                                        group.spawn((
                                            Text::new(title.to_uppercase()),
                                            crate::ui::text_font(font, 15.0),
                                            TextColor(CHOSEN.lighter(0.25)),
                                        ));
                                        let lines: Vec<(String, &str)> = match scope {
                                            Some(scope) => BINDINGS
                                                .iter()
                                                .filter(|binding| binding.scope == scope)
                                                .map(|binding| {
                                                    (
                                                        binding.keys(language),
                                                        binding.label(language),
                                                    )
                                                })
                                                .collect(),
                                            None => MOUSE
                                                .iter()
                                                .map(|&(keys, spanish, english)| {
                                                    (
                                                        keys.to_string(),
                                                        match language {
                                                            Language::Spanish => spanish,
                                                            Language::English => english,
                                                        },
                                                    )
                                                })
                                                .collect(),
                                        };
                                        for (keys, label) in lines {
                                            group
                                                .spawn(Node {
                                                    flex_direction: FlexDirection::Row,
                                                    column_gap: px(8),
                                                    ..default()
                                                })
                                                .with_children(|line| {
                                                    line.spawn((
                                                        Text::new(keys),
                                                        crate::ui::text_font(font, 13.0),
                                                        TextColor(Color::WHITE),
                                                        Node {
                                                            width: px(92),
                                                            flex_shrink: 0.0,
                                                            ..default()
                                                        },
                                                    ));
                                                    line.spawn((
                                                        Text::new(label),
                                                        crate::ui::text_font(font, 13.0),
                                                        TextColor(DIM),
                                                    ));
                                                });
                                        }
                                    });
                            }
                        });
                });
        });
}

/// The interface as the editor is: the column of the mode in use shown, the
/// legend's window open or shut, and each button lit if it is what is
/// chosen (the mode, the brush's tool, what N adds), lighter under the
/// mouse.
#[allow(clippy::type_complexity)]
pub fn style_ui(
    editor: Res<Editor>,
    shaping: Res<Shaping>,
    actions: Res<Actions>,
    mut columns: Query<(&ModeColumn, &mut Node), Without<HelpWindow>>,
    mut help: Query<&mut Node, With<HelpWindow>>,
    mut buttons: Query<(&ActionButton, &Interaction, &mut BackgroundColor)>,
) {
    for (column, mut node) in &mut columns {
        node.display = if column.0 == editor.mode {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut help {
        node.display = if actions.help {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (button, interaction, mut background) in &mut buttons {
        let chosen = match button.0 {
            Action::SetMode(mode) => mode == editor.mode,
            Action::Tool(tool) => tool == shaping.brush.tool,
            Action::Adding(kind) => Some(kind) == editor.roadside.adding,
            Action::Help => actions.help,
            _ => false,
        };
        background.0 = match (chosen, interaction) {
            (true, _) => CHOSEN,
            (false, Interaction::None) => BUTTON,
            (false, _) => HOVERED,
        };
    }
}

/// The interface's texts kept up to date (see [`UiText`]).
pub fn show_texts(
    editor: Res<Editor>,
    shaping: Res<Shaping>,
    circuits: Res<crate::circuits::Circuits>,
    mut texts: Query<(&UiText, &mut Text)>,
) {
    let circuit = circuits.at(editor.circuit.min(circuits.len() - 1));
    let unsaved = editor.edit.changed || shaping.changed || editor.roadside.changed;
    for (which, mut text) in &mut texts {
        let wanted = match which {
            UiText::Title => format!(
                "EDITOR · {}{}",
                circuit.name,
                if unsaved { " ●" } else { "" }
            ),
            UiText::Figures => super::figures(&editor),
            UiText::Info => match editor.mode {
                Mode::Lap => super::picked_line(&editor),
                Mode::Terrain => super::terrain::shaping_text(&shaping),
                Mode::Roadside => super::roadside::roadside_text(&editor.roadside),
            },
            UiText::Message => editor.message.clone(),
        };
        if **text != wanted {
            **text = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_bindings_of_a_mode_share_a_key() {
        for mode in [Mode::Lap, Mode::Terrain, Mode::Roadside] {
            let keys: Vec<_> = BINDINGS
                .iter()
                .filter(|binding| binding.applies(mode))
                .flat_map(|binding| {
                    std::iter::once(binding.key)
                        .chain(binding.also)
                        .map(move |key| (key, binding.ctrl, binding.shift))
                })
                .collect();
            for (k, key) in keys.iter().enumerate() {
                assert!(!keys[k + 1..].contains(key), "{mode:?}: {key:?} twice");
            }
        }
    }

    #[test]
    fn keys_are_written_short_as_on_a_badge() {
        let save = BINDINGS.iter().find(|b| b.action == Action::Save).unwrap();
        assert_eq!(save.keys(Language::Spanish), "Ctrl+S");
        let grip = BINDINGS
            .iter()
            .find(|b| b.action == Action::GripDown)
            .unwrap();
        assert_eq!(grip.keys(Language::Spanish), "Mayús+G");
        assert_eq!(grip.keys(Language::English), "Shift+G");
        assert_eq!(key_name(KeyCode::Digit7, Language::English), "7");
        assert_eq!(key_name(KeyCode::Delete, Language::Spanish), "Supr");
    }

    #[test]
    fn every_action_of_a_mode_has_a_key_and_its_names() {
        for binding in BINDINGS {
            assert!(!binding.spanish.is_empty() && !binding.english.is_empty());
        }
        for tool in Tool::ALL {
            assert!(
                BINDINGS.iter().any(|b| b.action == Action::Tool(tool)),
                "{tool:?}"
            );
        }
        for kind in Kind::ALL {
            assert!(
                BINDINGS.iter().any(|b| b.action == Action::Adding(kind)),
                "{kind:?}"
            );
        }
    }
}
