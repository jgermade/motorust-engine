//! The mapping of the controls, a view of Options > Control: the keyboard
//! or the gamepad (the one in use when it opens, or chosen with the keys
//! that change section), drawn with the keys or buttons held down lit while
//! testing or remapping (and, on the keyboard, the ones that do something
//! tinted), each action with
//! its keys or buttons underneath, and at its foot, beside Back: "Test"
//! lets the keys or buttons be tried out, lighting what they do, without
//! working the menu; "Remap" waits for the key or button to remap and then
//! for the new one, or several pressed together, that is to do its action
//! instead; "Reset" puts the device's back as they were at first. The
//! keyboard and the gamepad are mapped apart. The header (the devices) is
//! in Control's colour, what stands out below it in a neutral one.

use bevy::prelude::*;

use crate::hud::UiFont;
use crate::legend::{Glyph, Label, button_badge, section_badges, spawn_lone_badge};
use crate::locale::Language;
use crate::pad_art::{DrawnPad, PadArt};
use crate::settings::{ACTIONS, Action, Controls, key_name};
use crate::ui::{ActiveDevice, MenuSound, Request, UiIcons, text_font};

/// Seconds to press the key or button before the remapping is called off.
const CAPTURE_TIME: f32 = 8.0;
/// Seconds what was done is shown.
const NOTE_TIME: f32 = 3.0;

/// Which device is being mapped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapDevice {
    Keyboard,
    Gamepad,
}

const DEVICES: [MapDevice; 2] = [MapDevice::Keyboard, MapDevice::Gamepad];

impl MapDevice {
    /// The one `device` is.
    pub fn of(device: ActiveDevice) -> Self {
        match device {
            ActiveDevice::Keyboard => MapDevice::Keyboard,
            _ => MapDevice::Gamepad,
        }
    }

    fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (MapDevice::Keyboard, Language::Spanish) => "Teclado",
            (MapDevice::Keyboard, Language::English) => "Keyboard",
            (MapDevice::Gamepad, Language::Spanish) => "Mando",
            (MapDevice::Gamepad, Language::English) => "Gamepad",
        }
    }
}

/// What is chosen in the view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapFocus {
    /// The row of devices: across, the other one.
    Devices,
    Remap,
    Test,
    Reset,
    Back,
}

/// The buttons at the foot of the view, side by side: Test (chosen as the
/// view opens, and down from the devices), Remap, Reset and Back.
const BUTTONS: [MapFocus; 4] = [
    MapFocus::Test,
    MapFocus::Remap,
    MapFocus::Reset,
    MapFocus::Back,
];

/// A key or a gamepad button.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pressed {
    Key(KeyCode),
    Button(GamepadButton),
}

/// Where the remapping is at.
#[derive(Clone, PartialEq, Debug)]
pub enum Step {
    Idle,
    /// Waiting for the key or button to remap, since then (s).
    Choose {
        since: f32,
    },
    /// Waiting for the new key or combination that is to do `action` in
    /// place of `from`; those pressed so far.
    Assign {
        since: f32,
        action: Action,
        from: Pressed,
        held: Vec<Pressed>,
    },
    /// Trying the keys or buttons out: each one held lit, with what it
    /// does, until Esc, or on a gamepad B held for [`END_HOLD`] s.
    Test,
}

/// What was just done, to say so for a moment.
#[derive(Clone, PartialEq, Debug)]
pub enum Note {
    Remapped {
        action: Action,
        from: Pressed,
        to: Vec<Pressed>,
    },
    /// The key or button chosen does nothing.
    Unbound(Pressed),
    Cancelled,
    Reset(MapDevice),
}

/// The view, while it is open.
#[derive(Clone, PartialEq, Debug)]
pub struct Mapping {
    pub device: MapDevice,
    pub focus: MapFocus,
    pub step: Step,
    /// What was just done, and until when (s).
    pub note: Option<(Note, f32)>,
    /// Trying the controls out, since when (s) a gamepad's B has been held
    /// down, to end the test once it has been for [`END_HOLD`] s.
    pub ending: Option<f32>,
}

/// The gamepad's button that ends a test of the controls held down, and for
/// how long (s): long enough not to end it by trying B out.
const END_BUTTON: GamepadButton = GamepadButton::East;
const END_HOLD: f32 = 3.0;

impl Mapping {
    /// The view of `device`, with "Test" chosen.
    pub fn open(device: MapDevice) -> Self {
        Self {
            device,
            focus: BUTTONS[0],
            step: Step::Idle,
            note: None,
            ending: None,
        }
    }

    /// Whether it waits for a key or a button.
    pub fn capturing(&self) -> bool {
        self.step != Step::Idle
    }

    /// Moves round the view: up and down between the devices and the
    /// buttons at its foot (Back among them); across, the other device or
    /// another button. Whether it did anything.
    fn navigate(&mut self, request: &Request) -> bool {
        let before = (self.focus, self.device);
        if request.tab != 0 {
            self.device = crate::settings::cycle(&DEVICES, self.device, request.tab);
        }
        if request.step != 0 {
            self.focus = if self.focus == MapFocus::Devices {
                BUTTONS[0]
            } else {
                MapFocus::Devices
            };
        } else if request.side != 0 {
            if self.focus == MapFocus::Devices {
                self.device = crate::settings::cycle(&DEVICES, self.device, request.side);
            } else {
                self.focus = crate::settings::cycle(&BUTTONS, self.focus, request.side);
            }
        }
        (self.focus, self.device) != before
    }
}

/// The keys and buttons of this frame, as the view reads them.
pub struct Held<'a> {
    pub keyboard: &'a ButtonInput<KeyCode>,
    pub gamepads: Vec<&'a Gamepad>,
}

impl Held<'_> {
    /// The keys or buttons of `device` just pressed: not M, which mutes,
    /// nor Esc, which calls off a remapping.
    fn just_pressed(&self, device: MapDevice) -> Vec<Pressed> {
        match device {
            MapDevice::Keyboard => self
                .keyboard
                .get_just_pressed()
                .filter(|&&key| key != KeyCode::KeyM && key != KeyCode::Escape)
                .map(|&key| Pressed::Key(key))
                .collect(),
            MapDevice::Gamepad => self
                .gamepads
                .iter()
                .flat_map(|gamepad| gamepad.get_just_pressed())
                .map(|&button| Pressed::Button(button))
                .collect(),
        }
    }

    /// The keys or buttons of `device` held down.
    fn all_held(&self, device: MapDevice) -> Vec<Pressed> {
        match device {
            MapDevice::Keyboard => self
                .keyboard
                .get_pressed()
                .map(|&key| Pressed::Key(key))
                .collect(),
            MapDevice::Gamepad => self
                .gamepads
                .iter()
                .flat_map(|gamepad| gamepad.get_pressed())
                .map(|&button| Pressed::Button(button))
                .collect(),
        }
    }

    /// Whether what is held of `device` does `action` (a key or button of
    /// it, or all of one of its combinations).
    fn does(&self, controls: &Controls, device: MapDevice, action: Action) -> bool {
        match device {
            MapDevice::Keyboard => controls.key_value(action, self.keyboard) > 0.0,
            MapDevice::Gamepad => self
                .gamepads
                .iter()
                .any(|gamepad| controls.button_value(action, gamepad) > 0.5),
        }
    }

    /// Whether `input` is held down.
    pub fn is_held(&self, input: Pressed) -> bool {
        match input {
            Pressed::Key(key) => self.keyboard.pressed(key),
            Pressed::Button(button) => self.gamepads.iter().any(|pad| pad.pressed(button)),
        }
    }
}

/// Adds the keys or buttons of `device` just pressed to `held`; once none
/// of them is held down any more, they are the combination (one, or several
/// pressed together).
fn combination(held: &mut Vec<Pressed>, input: &Held, device: MapDevice) -> Option<Vec<Pressed>> {
    for pressed in input.just_pressed(device) {
        if !held.contains(&pressed) {
            held.push(pressed);
        }
    }
    (!held.is_empty() && !held.iter().any(|&pressed| input.is_held(pressed))).then(|| held.clone())
}

/// What `from` did is done with `to` instead (see [`Controls::remap`]).
fn remap(controls: &mut Controls, from: Pressed, to: &[Pressed]) -> Option<Action> {
    match from {
        Pressed::Key(key) => {
            let keys: Vec<KeyCode> = to
                .iter()
                .filter_map(|pressed| match pressed {
                    Pressed::Key(key) => Some(*key),
                    Pressed::Button(_) => None,
                })
                .collect();
            controls.remap(key, &keys)
        }
        Pressed::Button(button) => {
            let buttons: Vec<GamepadButton> = to
                .iter()
                .filter_map(|pressed| match pressed {
                    Pressed::Button(button) => Some(*button),
                    Pressed::Key(_) => None,
                })
                .collect();
            controls.remap(button, &buttons)
        }
    }
}

/// The action of a key or button, if it has one.
fn action_of(controls: &Controls, input: Pressed) -> Option<Action> {
    match input {
        Pressed::Key(key) => controls.action_of(key),
        Pressed::Button(button) => controls.action_of(button),
    }
}

/// Works the view with this frame's `request` and keys and buttons
/// (`input`). Whether it is to close, back to the options of Control.
pub fn use_mapping(
    mapping: &mut Mapping,
    request: &Request,
    input: &Held,
    controls: &mut Controls,
    now: f32,
    sounds: &mut MessageWriter<MenuSound>,
) -> bool {
    if mapping.note.as_ref().is_some_and(|(_, until)| now > *until) {
        mapping.note = None;
    }
    let cancel = input.keyboard.just_pressed(KeyCode::Escape);
    let note = |note: Note| Some((note, now + NOTE_TIME));
    match mapping.step.clone() {
        Step::Choose { since } => {
            if cancel || now - since > CAPTURE_TIME {
                mapping.step = Step::Idle;
                mapping.note = note(Note::Cancelled);
                sounds.write(MenuSound::Move);
            } else if let Some(&from) = input.just_pressed(mapping.device).first() {
                match action_of(controls, from) {
                    Some(action) => {
                        mapping.step = Step::Assign {
                            since: now,
                            action,
                            from,
                            held: Vec::new(),
                        };
                        sounds.write(MenuSound::Confirm);
                    }
                    None => {
                        mapping.step = Step::Idle;
                        mapping.note = note(Note::Unbound(from));
                        sounds.write(MenuSound::Move);
                    }
                }
            }
            false
        }
        Step::Assign {
            since,
            action,
            from,
            mut held,
        } => {
            if cancel || (held.is_empty() && now - since > CAPTURE_TIME) {
                mapping.step = Step::Idle;
                mapping.note = note(Note::Cancelled);
                sounds.write(MenuSound::Move);
            } else if let Some(to) = combination(&mut held, input, mapping.device) {
                remap(controls, from, &to);
                mapping.step = Step::Idle;
                mapping.note = note(Note::Remapped { action, from, to });
                sounds.write(MenuSound::Confirm);
            } else {
                mapping.step = Step::Assign {
                    since,
                    action,
                    from,
                    held,
                };
            }
            false
        }
        Step::Test => {
            // B held down on a gamepad, since when.
            let holding = input
                .gamepads
                .iter()
                .any(|gamepad| gamepad.pressed(END_BUTTON));
            mapping.ending = holding.then(|| mapping.ending.unwrap_or(now));
            let held_long = mapping.ending.is_some_and(|since| now - since >= END_HOLD);
            if cancel || held_long {
                mapping.step = Step::Idle;
                mapping.ending = None;
                sounds.write(MenuSound::Move);
            }
            false
        }
        Step::Idle => {
            if request.back {
                sounds.write(MenuSound::Move);
                return true;
            }
            if mapping.navigate(request) {
                sounds.write(MenuSound::Move);
                return false;
            }
            if !request.confirm {
                return false;
            }
            match mapping.focus {
                MapFocus::Devices => {
                    mapping.focus = BUTTONS[0];
                    sounds.write(MenuSound::Move);
                }
                MapFocus::Remap => {
                    mapping.step = Step::Choose { since: now };
                    mapping.note = None;
                    sounds.write(MenuSound::Confirm);
                }
                MapFocus::Test => {
                    mapping.step = Step::Test;
                    mapping.note = None;
                    sounds.write(MenuSound::Confirm);
                }
                MapFocus::Reset => {
                    match mapping.device {
                        MapDevice::Keyboard => controls.reset::<KeyCode>(),
                        MapDevice::Gamepad => controls.reset::<GamepadButton>(),
                    }
                    mapping.note = note(Note::Reset(mapping.device));
                    sounds.write(MenuSound::Confirm);
                }
                MapFocus::Back => {
                    sounds.write(MenuSound::Move);
                    return true;
                }
            }
            false
        }
    }
}

// The view.

/// The colour of Control, whose view this is: its header, the devices.
const ACCENT: Color = Color::srgb(0.22, 0.76, 1.0);
/// What stands out in its content (the keys and buttons held, the button
/// chosen, the actions done in a test): neutral, as in every view under a
/// section, the section's colour being the header's.
const HIGHLIGHT: Color = Color::srgb(0.9, 0.91, 0.93);
/// A button held, tinted over the light drawing of the gamepad.
const HELD_ON_PAD: Color = Color::srgb(0.3, 0.32, 0.36);
/// The key or button being remapped, and the new ones pressed.
const FROM: Color = Color::srgb(1.0, 0.55, 0.2);
const NEW: Color = Color::srgb(1.0, 0.82, 0.2);
const INK: Color = Color::srgb(0.06, 0.05, 0.04);

/// What the view is drawn from.
pub struct MappingLook<'a> {
    pub font: &'a UiFont,
    pub icons: &'a UiIcons,
    pub lang: Language,
    /// The device in use, for the keys or buttons that change the device.
    pub using: ActiveDevice,
    /// How the buttons are marked on the gamepad (Xbox, PlayStation...).
    pub pad: ActiveDevice,
    /// The gamepad's illustration; drawn, [`PAD_HEIGHT`] tall (see
    /// [`crate::pad_art`]), once it is; and whether its face buttons'
    /// letters are to be put on it (they are not printed on the drawing: a
    /// Switch pad's are not an Xbox pad's).
    pub art: PadArt,
    pub drawn: Option<&'a DrawnPad>,
    pub letters: bool,
    pub controls: &'a Controls,
    pub held: &'a Held<'a>,
}

/// How far the keys that change the device are from the sides (px), as
/// the ones that change the section of the options.
const BADGE_INSET: f32 = 20.0;

/// How tall the gamepad is drawn (logical px).
pub const PAD_HEIGHT: f32 = 270.0;
/// Room above it, under the devices (px).
const PAD_GAP: f32 = 16.0;

/// What a key or button says on it.
enum Mark {
    Text(String),
    Glyph(Glyph),
}

/// The keys of the keyboard, row by row, and how wide each is (in keys).
const KEY_ROWS: [&[(KeyCode, f32)]; 5] = {
    use KeyCode as K;
    [
        &[
            (K::Escape, 1.25),
            (K::Digit1, 1.0),
            (K::Digit2, 1.0),
            (K::Digit3, 1.0),
            (K::Digit4, 1.0),
            (K::Digit5, 1.0),
            (K::Digit6, 1.0),
            (K::Digit7, 1.0),
            (K::Digit8, 1.0),
            (K::Digit9, 1.0),
            (K::Digit0, 1.0),
            (K::Minus, 1.0),
            (K::Equal, 1.0),
            (K::Backspace, 1.75),
        ],
        &[
            (K::Tab, 1.5),
            (K::KeyQ, 1.0),
            (K::KeyW, 1.0),
            (K::KeyE, 1.0),
            (K::KeyR, 1.0),
            (K::KeyT, 1.0),
            (K::KeyY, 1.0),
            (K::KeyU, 1.0),
            (K::KeyI, 1.0),
            (K::KeyO, 1.0),
            (K::KeyP, 1.0),
            (K::BracketLeft, 1.0),
            (K::BracketRight, 1.0),
            (K::Backslash, 1.5),
        ],
        &[
            (K::CapsLock, 1.75),
            (K::KeyA, 1.0),
            (K::KeyS, 1.0),
            (K::KeyD, 1.0),
            (K::KeyF, 1.0),
            (K::KeyG, 1.0),
            (K::KeyH, 1.0),
            (K::KeyJ, 1.0),
            (K::KeyK, 1.0),
            (K::KeyL, 1.0),
            (K::Semicolon, 1.0),
            (K::Quote, 1.0),
            (K::Enter, 2.25),
        ],
        &[
            (K::ShiftLeft, 2.25),
            (K::KeyZ, 1.0),
            (K::KeyX, 1.0),
            (K::KeyC, 1.0),
            (K::KeyV, 1.0),
            (K::KeyB, 1.0),
            (K::KeyN, 1.0),
            (K::KeyM, 1.0),
            (K::Comma, 1.0),
            (K::Period, 1.0),
            (K::Slash, 1.0),
            (K::ShiftRight, 2.75),
        ],
        &[
            (K::ControlLeft, 1.5),
            (K::AltLeft, 1.25),
            (K::SuperLeft, 1.25),
            (K::Space, 7.0),
            (K::SuperRight, 1.25),
            (K::AltRight, 1.25),
            (K::ControlRight, 1.5),
        ],
    ]
};

/// The size of a key (px) and the gap between keys.
const KEY: f32 = 30.0;
const KEY_GAP: f32 = 3.0;

/// What a key says on it: short names for the wide ones.
fn key_mark(key: KeyCode, lang: Language) -> Mark {
    use KeyCode as K;
    let english = lang == Language::English;
    let mac = cfg!(target_os = "macos");
    let text = match key {
        K::ArrowUp => return Mark::Glyph(Glyph::Up),
        K::ArrowDown => return Mark::Glyph(Glyph::Down),
        K::ArrowLeft => return Mark::Glyph(Glyph::Left),
        K::ArrowRight => return Mark::Glyph(Glyph::Right),
        K::Backspace if english => "Bksp",
        K::Backspace => "Borrar",
        K::CapsLock if english => "Caps",
        K::CapsLock => "Bloq",
        K::ShiftLeft | K::ShiftRight if english => "Shift",
        K::ShiftLeft | K::ShiftRight => "Mayús",
        K::Space if english => "Space",
        K::Space => "Espacio",
        K::ControlLeft | K::ControlRight => "Ctrl",
        K::AltLeft | K::AltRight if mac => "Opt",
        K::AltLeft | K::AltRight => "Alt",
        K::SuperLeft | K::SuperRight if mac => "Cmd",
        K::SuperLeft | K::SuperRight => "Win",
        other => return Mark::Text(key_name(other, lang)),
    };
    Mark::Text(text.into())
}

/// Spawns the view in the options panel: its title, the devices, the one
/// chosen drawn, what each action does on it, what is going on, the
/// buttons and Back.
pub fn spawn_mapping(panel: &mut ChildSpawnerCommands, mapping: &Mapping, look: &MappingLook) {
    let lang = look.lang;
    panel.spawn((
        Text::new(match lang {
            Language::Spanish => "OPCIONES  >  CONTROL",
            Language::English => "OPTIONS  >  CONTROL",
        }),
        text_font(look.font, 32.0),
        TextShadow {
            offset: Vec2::splat(2.5),
            color: Color::srgba(0.0, 0.0, 0.0, 0.6),
        },
        Node {
            align_self: AlignSelf::Center,
            ..default()
        },
    ));
    // The devices, as the sections of the options: the keys or buttons
    // that change the device at the ends.
    let [previous, next] = section_badges(look.using);
    panel
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(BADGE_INSET)),
            ..default()
        })
        .with_children(|bar| {
            spawn_lone_badge(bar, look.font, look.icons, look.using, &previous);
            bar.spawn(Node {
                justify_content: JustifyContent::Center,
                column_gap: px(10),
                ..default()
            })
            .with_children(|row| spawn_devices(row, mapping, look));
            spawn_lone_badge(bar, look.font, look.icons, look.using, &next);
        });
    panel.spawn((
        Node {
            height: px(2),
            min_height: px(2),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(ACCENT.with_alpha(0.7)),
    ));
    // The device, drawn, and what each action does on it: under the
    // keyboard, in two columns; beside the gamepad, in one.
    let status_line = |panel: &mut ChildSpawnerCommands| {
        let (status, color) = status(mapping, look);
        panel.spawn((
            Text::new(status),
            text_font(look.font, 17.0),
            TextColor(color),
            TextLayout::justify(Justify::Center),
            Node {
                align_self: AlignSelf::Center,
                min_height: px(22),
                ..default()
            },
        ));
    };
    match mapping.device {
        MapDevice::Keyboard => {
            panel
                .spawn(Node {
                    justify_content: JustifyContent::Center,
                    height: px(196),
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|area| spawn_keyboard(area, mapping, look));
            status_line(panel);
            spawn_actions(panel, mapping, look, 2);
        }
        MapDevice::Gamepad => {
            panel
                .spawn(Node {
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    column_gap: px(28),
                    height: px(PAD_HEIGHT + 8.0),
                    flex_shrink: 0.0,
                    margin: UiRect::top(px(PAD_GAP)),
                    ..default()
                })
                .with_children(|row| {
                    spawn_gamepad(row, mapping, look);
                    row.spawn(Node {
                        width: px(270),
                        ..default()
                    })
                    .with_children(|list| spawn_actions(list, mapping, look, 1));
                });
            status_line(panel);
        }
    }
    // At the foot, side by side: test, remap, reset and back.
    panel
        .spawn(Node {
            justify_content: JustifyContent::Center,
            column_gap: px(12),
            margin: UiRect::top(Val::Auto),
            ..default()
        })
        .with_children(|foot| {
            let remapping = matches!(mapping.step, Step::Choose { .. } | Step::Assign { .. });
            let testing = mapping.step == Step::Test;
            let remap = match (remapping, lang) {
                (false, Language::Spanish) => "Reasignar",
                (false, Language::English) => "Remap",
                (true, Language::Spanish) => "Esperando...",
                (true, Language::English) => "Waiting...",
            };
            let test = match (testing, lang) {
                (false, Language::Spanish) => "Probar",
                (false, Language::English) => "Test",
                (true, Language::Spanish) => "Probando...",
                (true, Language::English) => "Testing...",
            };
            let labels = [test, remap, lang.reset(), lang.back()];
            for (focus, label) in BUTTONS.into_iter().zip(labels) {
                spawn_button(
                    foot,
                    look,
                    label,
                    mapping.focus == focus,
                    mapping.capturing(),
                );
            }
        });
}

/// The devices, as tabs: the one mapped filled, edged lighter when the
/// row is chosen.
fn spawn_devices(row: &mut ChildSpawnerCommands, mapping: &Mapping, look: &MappingLook) {
    for device in DEVICES {
        let active = device == mapping.device;
        let edge = if active && mapping.focus == MapFocus::Devices {
            ACCENT.mix(&Color::WHITE, 0.6)
        } else if active {
            ACCENT
        } else {
            ACCENT.with_alpha(0.16)
        };
        row.spawn((
            Node {
                padding: UiRect::axes(px(18), px(5)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(if active {
                ACCENT
            } else {
                ACCENT.with_alpha(0.16)
            }),
            BorderColor::all(edge),
        ))
        .with_child((
            Text::new(device.name(look.lang)),
            text_font(look.font, 18.0),
            TextColor(if active {
                INK
            } else {
                ACCENT.mix(&Color::WHITE, 0.55)
            }),
        ));
    }
}

/// A button of the view: filled in the colour of Control when chosen.
fn spawn_button(
    row: &mut ChildSpawnerCommands,
    look: &MappingLook,
    label: &str,
    chosen: bool,
    waiting: bool,
) {
    let fill = match (chosen, waiting) {
        (true, true) => NEW,
        (true, false) => HIGHLIGHT,
        _ => Color::srgba(1.0, 1.0, 1.0, 0.08),
    };
    row.spawn((
        Node {
            min_width: px(140),
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(20), px(6)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(fill),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.3)),
    ))
    .with_child((
        Text::new(label),
        text_font(look.font, 19.0),
        TextColor(if chosen { INK } else { Color::WHITE }),
    ));
}

/// Whether `input`, held down, lights up: only while testing or remapping.
/// Otherwise the keys and buttons held are the ones moving round the view.
fn lights_up(mapping: &Mapping, held: &Held, input: Pressed) -> bool {
    mapping.capturing() && held.is_held(input)
}

/// How a key or button is drawn: lit if held down while testing or
/// remapping (or new, or the one being remapped), tinted if it does
/// something.
fn shade(input: Pressed, mapping: &Mapping, look: &MappingLook) -> (Color, Color, Color) {
    let (from, new) = match &mapping.step {
        Step::Assign { from, held, .. } => (Some(*from), held.contains(&input)),
        _ => (None, false),
    };
    if new {
        (NEW, NEW, INK)
    } else if from == Some(input) {
        (FROM, FROM, INK)
    } else if lights_up(mapping, look.held, input) {
        (HIGHLIGHT, HIGHLIGHT, INK)
    } else if action_of(look.controls, input).is_some() {
        (
            Color::srgba(1.0, 1.0, 1.0, 0.22),
            Color::srgba(1.0, 1.0, 1.0, 0.55),
            Color::WHITE,
        )
    } else {
        (
            Color::srgba(1.0, 1.0, 1.0, 0.07),
            Color::srgba(1.0, 1.0, 1.0, 0.18),
            Color::srgba(1.0, 1.0, 1.0, 0.6),
        )
    }
}

/// A key or button: its shape, colours and what it says.
#[allow(clippy::too_many_arguments)]
fn spawn_cap(
    parent: &mut ChildSpawnerCommands,
    look: &MappingLook,
    node: Node,
    round: bool,
    (fill, edge, ink): (Color, Color, Color),
    mark: Mark,
    size: f32,
) {
    parent
        .spawn((
            Node {
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: if round {
                    BorderRadius::MAX
                } else {
                    BorderRadius::all(px(5))
                },
                ..node
            },
            BackgroundColor(fill),
            BorderColor::all(edge),
        ))
        .with_children(|cap| match mark {
            Mark::Text(text) => {
                cap.spawn((Text::new(text), text_font(look.font, size), TextColor(ink)));
            }
            Mark::Glyph(glyph) => {
                cap.spawn((
                    ImageNode::new(look.icons.glyph(glyph)).with_color(ink),
                    Node {
                        width: px(size),
                        height: px(size),
                        ..default()
                    },
                ));
            }
        });
}

fn spawn_keyboard(area: &mut ChildSpawnerCommands, mapping: &Mapping, look: &MappingLook) {
    let key = |parent: &mut ChildSpawnerCommands, code: KeyCode, width: f32| {
        let input = Pressed::Key(code);
        spawn_cap(
            parent,
            look,
            Node {
                width: px(KEY * width + KEY_GAP * (width - 1.0).max(0.0)),
                height: px(KEY),
                ..default()
            },
            false,
            shade(input, mapping, look),
            key_mark(code, look.lang),
            if width > 1.0 { 12.0 } else { 14.0 },
        );
    };
    area.spawn(Node {
        column_gap: px(KEY_GAP * 4.0),
        align_items: AlignItems::FlexEnd,
        margin: UiRect::top(px(8)),
        ..default()
    })
    .with_children(|board| {
        board
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(KEY_GAP),
                ..default()
            })
            .with_children(|rows| {
                for row in KEY_ROWS {
                    rows.spawn(Node {
                        column_gap: px(KEY_GAP),
                        ..default()
                    })
                    .with_children(|keys| {
                        for &(code, width) in row {
                            key(keys, code, width);
                        }
                    });
                }
            });
        // The arrows, under the right of the keyboard.
        board
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(KEY_GAP),
                ..default()
            })
            .with_children(|arrows| {
                key(arrows, KeyCode::ArrowUp, 1.0);
                arrows
                    .spawn(Node {
                        column_gap: px(KEY_GAP),
                        ..default()
                    })
                    .with_children(|row| {
                        for code in [KeyCode::ArrowLeft, KeyCode::ArrowDown, KeyCode::ArrowRight] {
                            key(row, code, 1.0);
                        }
                    });
            });
    });
}

/// Every action with its keys (or buttons, on the gamepad's page), in
/// `columns`: the one being remapped in orange, the one just remapped in
/// yellow, while testing the ones done lit.
fn spawn_actions(
    parent: &mut ChildSpawnerCommands,
    mapping: &Mapping,
    look: &MappingLook,
    columns: u16,
) {
    let lang = look.lang;
    parent
        .spawn(Node {
            display: Display::Grid,
            grid_template_columns: RepeatedGridTrack::flex(columns, 1.0),
            column_gap: px(28),
            row_gap: px(if columns == 1 { 6 } else { 2 }),
            padding: UiRect::horizontal(px(if columns == 1 { 0 } else { 20 })),
            ..default()
        })
        .with_children(|grid| {
            for action in ACTIONS {
                let busy = matches!(&mapping.step, Step::Assign { action: a, .. } if *a == action);
                let done = matches!(
                    &mapping.note,
                    Some((Note::Remapped { action: a, .. }, _)) if *a == action
                );
                let tried = mapping.step == Step::Test
                    && look.held.does(look.controls, mapping.device, action);
                let color = if busy {
                    FROM
                } else if done {
                    NEW
                } else if tried {
                    INK
                } else {
                    Color::srgba(1.0, 1.0, 1.0, 0.85)
                };
                let value = match mapping.device {
                    MapDevice::Keyboard => look.controls.describe_keys(action, lang),
                    MapDevice::Gamepad => look.controls.describe_buttons(action, lang),
                };
                grid.spawn((
                    Node {
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: px(12),
                        padding: UiRect::axes(px(6), px(1)),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    },
                    BackgroundColor(if tried { HIGHLIGHT } else { Color::NONE }),
                ))
                .with_children(|line| {
                    line.spawn((
                        Text::new(action.name(lang)),
                        text_font(look.font, 15.0),
                        TextColor(color),
                    ));
                    line.spawn((
                        Text::new(value),
                        text_font(look.font, 15.0),
                        TextColor(color),
                    ));
                });
            }
        });
}

/// The tint over a button of the drawn gamepad: lit while held testing or
/// remapping (or new, or the one being remapped), none otherwise; what the buttons do is listed
/// beside it.
fn tint(input: Pressed, mapping: &Mapping, look: &MappingLook) -> Option<Color> {
    let (from, new) = match &mapping.step {
        Step::Assign { from, held, .. } => (Some(*from), held.contains(&input)),
        _ => (None, false),
    };
    if new {
        Some(NEW)
    } else if from == Some(input) {
        Some(FROM)
    } else if lights_up(mapping, look.held, input) {
        Some(HIGHLIGHT)
    } else {
        None
    }
}

/// The gamepad, drawn (see [`crate::pad_art`]): each button tinted over it,
/// the triggers above it, and, if the drawing does not print them, the
/// letters of the face buttons.
fn spawn_gamepad(area: &mut ChildSpawnerCommands, mapping: &Mapping, look: &MappingLook) {
    let Some(drawn) = look.drawn else {
        // Still being drawn: its room, kept.
        let size = look.art.size(PAD_HEIGHT);
        area.spawn(Node {
            width: px(size.x),
            height: px(size.y),
            flex_shrink: 0.0,
            ..default()
        });
        return;
    };
    area.spawn((
        Node {
            width: px(drawn.size.x),
            height: px(drawn.size.y),
            flex_shrink: 0.0,
            ..default()
        },
        ImageNode::new(drawn.whole.clone()),
    ))
    .with_children(|pad| {
        let place = |rect: Rect| Node {
            position_type: PositionType::Absolute,
            left: px(rect.min.x),
            top: px(rect.min.y),
            width: px(rect.width()),
            height: px(rect.height()),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        };
        for (button, image, rect) in &drawn.buttons {
            let input = Pressed::Button(*button);
            let face = matches!(
                button,
                GamepadButton::South
                    | GamepadButton::East
                    | GamepadButton::West
                    | GamepadButton::North
            );
            let tinted = tint(input, mapping, look);
            if tinted.is_none() && !(face && look.letters) {
                continue;
            }
            let mut spawned = pad.spawn(place(*rect));
            if let Some(color) = tinted {
                // White on the light drawing would not show: held, darker.
                let color = if color == HIGHLIGHT {
                    HELD_ON_PAD
                } else {
                    color
                };
                spawned.insert(ImageNode::new(image.clone()).with_color(color));
            }
            if face
                && look.letters
                && let Label::Text(letter) = button_badge(look.pad, *button).label
            {
                spawned.with_child((
                    Text::new(letter),
                    text_font(look.font, (rect.height() * 0.5).clamp(9.0, 16.0)),
                    TextColor(Color::srgb(0.3, 0.32, 0.38)),
                ));
            }
        }
        // The triggers, where the drawing says (they are not in its view).
        for (button, rect) in &drawn.triggers {
            let input = Pressed::Button(*button);
            let height = rect.height().max(15.0);
            let bar = Rect::new(rect.min.x, rect.max.y - height, rect.max.x, rect.max.y);
            let (fill, edge, ink) = match tint(input, mapping, look) {
                Some(color) => (color, color, INK),
                None => (
                    Color::srgba(1.0, 1.0, 1.0, 0.12),
                    Color::srgba(1.0, 1.0, 1.0, 0.3),
                    Color::srgba(1.0, 1.0, 1.0, 0.7),
                ),
            };
            let label = match button_badge(look.pad, *button).label {
                Label::Text(text) => text,
                Label::Glyphs(_) => String::new(),
            };
            spawn_cap(
                pad,
                look,
                place(bar),
                false,
                (fill, edge, ink),
                Mark::Text(label),
                (height * 0.65).clamp(9.0, 13.0),
            );
        }
    });
}

/// The name of a key or a button.
fn input_name(input: Pressed, look: &MappingLook) -> String {
    match input {
        Pressed::Key(key) => key_name(key, look.lang),
        Pressed::Button(button) => crate::settings::button_name(button, look.lang),
    }
}

/// The line that says what is going on: what to press, what was done, or
/// what the keys or buttons held down do.
fn status(mapping: &Mapping, look: &MappingLook) -> (String, Color) {
    let lang = look.lang;
    let keyboard = mapping.device == MapDevice::Keyboard;
    let names = |inputs: &[Pressed]| {
        inputs
            .iter()
            .map(|&input| input_name(input, look))
            .collect::<Vec<_>>()
            .join("+")
    };
    match &mapping.step {
        Step::Choose { .. } => (
            match (lang, keyboard) {
                (Language::Spanish, true) => {
                    "Pulsa la tecla que quieres reasignar   (Esc: cancelar)"
                }
                (Language::English, true) => "Press the key to remap   (Esc: cancel)",
                (Language::Spanish, false) => "Pulsa el botón que quieres reasignar",
                (Language::English, false) => "Press the button to remap",
            }
            .into(),
            NEW,
        ),
        Step::Assign { action, from, .. } => (
            match lang {
                Language::Spanish => format!(
                    "\"{}\" ({}): pulsa {}, o varios a la vez",
                    action.name(lang),
                    input_name(*from, look),
                    if keyboard {
                        "la tecla nueva"
                    } else {
                        "el botón nuevo"
                    }
                ),
                Language::English => format!(
                    "\"{}\" ({}): press the new {}, or several together",
                    action.name(lang),
                    input_name(*from, look),
                    if keyboard { "key" } else { "button" }
                ),
            },
            NEW,
        ),
        Step::Test => {
            let held = look.held.all_held(mapping.device);
            if held.is_empty() {
                let end = format!(
                    "Esc / {} {}",
                    input_name(Pressed::Button(END_BUTTON), look),
                    match lang {
                        Language::Spanish => "pulsado 3 s",
                        Language::English => "held 3 s",
                    }
                );
                let text = match (lang, keyboard) {
                    (Language::Spanish, true) => {
                        format!(
                            "Pulsa teclas, también a la vez, para probarlas   ({end}: terminar)"
                        )
                    }
                    (Language::English, true) => {
                        format!("Press keys, together too, to try them out   ({end}: done)")
                    }
                    (Language::Spanish, false) => {
                        format!(
                            "Pulsa botones, también a la vez, para probarlos   ({end}: terminar)"
                        )
                    }
                    (Language::English, false) => {
                        format!("Press buttons, together too, to try them out   ({end}: done)")
                    }
                };
                (text, Color::srgba(1.0, 1.0, 1.0, 0.8))
            } else {
                let done: Vec<&str> = ACTIONS
                    .into_iter()
                    .filter(|&action| look.held.does(look.controls, mapping.device, action))
                    .map(|action| action.name(lang))
                    .collect();
                let done = if done.is_empty() {
                    "—".to_string()
                } else {
                    done.join(", ")
                };
                (format!("{}  >  {done}", names(&held)), HIGHLIGHT)
            }
        }
        Step::Idle => match &mapping.note {
            Some((Note::Remapped { action, from, to }, _)) => (
                format!(
                    "{}:  {}  >  {}",
                    action.name(lang),
                    input_name(*from, look),
                    names(to)
                ),
                NEW,
            ),
            Some((Note::Unbound(input), _)) => (
                match lang {
                    Language::Spanish => format!("{} no hace nada", input_name(*input, look)),
                    Language::English => format!("{} does nothing", input_name(*input, look)),
                },
                Color::srgba(1.0, 1.0, 1.0, 0.7),
            ),
            Some((Note::Cancelled, _)) => (
                match lang {
                    Language::Spanish => "Cancelado".into(),
                    Language::English => "Cancelled".into(),
                },
                Color::srgba(1.0, 1.0, 1.0, 0.7),
            ),
            Some((Note::Reset(device), _)) => (
                match lang {
                    Language::Spanish => format!("{}: como al principio", device.name(lang)),
                    Language::English => format!("{}: back to the defaults", device.name(lang)),
                },
                NEW,
            ),
            // Nothing going on: what the keys held do is for a test.
            None => (String::new(), Color::WHITE),
        },
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    fn request() -> Request {
        Request::default()
    }

    /// The view at work: the keys pressed this frame (`press`), held from
    /// before (`hold`) and let go of (`release`).
    struct Bench {
        mapping: Mapping,
        controls: Controls,
        keyboard: ButtonInput<KeyCode>,
        now: f32,
        /// A gamepad, its buttons held as the test says.
        pad: Gamepad,
    }

    impl Bench {
        fn new() -> Self {
            Self {
                mapping: Mapping::open(MapDevice::Keyboard),
                controls: Controls::default(),
                keyboard: ButtonInput::default(),
                now: 0.0,
                pad: Gamepad::default(),
            }
        }

        fn frame(&mut self, request: Request, press: &[KeyCode], release: &[KeyCode]) -> bool {
            self.keyboard.clear();
            for &key in press {
                self.keyboard.press(key);
            }
            for &key in release {
                self.keyboard.release(key);
            }
            self.now += 0.1;
            let held = Held {
                keyboard: &self.keyboard,
                gamepads: vec![&self.pad],
            };
            let mut world = World::new();
            world.init_resource::<Messages<MenuSound>>();
            let mut state =
                bevy::ecs::system::SystemState::<MessageWriter<MenuSound>>::new(&mut world);
            let mut sounds = state.get_mut(&mut world).unwrap();
            use_mapping(
                &mut self.mapping,
                &request,
                &held,
                &mut self.controls,
                self.now,
                &mut sounds,
            )
        }
    }

    #[test]
    fn remap_waits_for_the_key_to_remap_then_for_the_new_one() {
        let mut bench = Bench::new();
        bench.mapping.focus = MapFocus::Remap;
        let confirm = Request {
            confirm: true,
            ..request()
        };
        // Remap: then W (the throttle's).
        bench.frame(confirm, &[KeyCode::Enter], &[]);
        assert!(matches!(bench.mapping.step, Step::Choose { .. }));
        bench.frame(request(), &[KeyCode::KeyW], &[KeyCode::Enter]);
        assert!(matches!(
            bench.mapping.step,
            Step::Assign {
                action: Action::Throttle,
                ..
            }
        ));
        // Let go of W; then I is pressed, and counts once let go of.
        bench.frame(request(), &[], &[KeyCode::KeyW]);
        bench.frame(request(), &[KeyCode::KeyI], &[]);
        assert!(matches!(bench.mapping.step, Step::Assign { .. }));
        bench.frame(request(), &[], &[KeyCode::KeyI]);
        assert_eq!(bench.mapping.step, Step::Idle);
        assert_eq!(
            bench.controls.throttle.keys,
            [KeyCode::KeyI, KeyCode::ArrowUp]
        );
        assert!(matches!(
            bench.mapping.note,
            Some((Note::Remapped { .. }, _))
        ));
    }

    #[test]
    fn several_keys_pressed_together_make_a_combination() {
        let mut bench = Bench::new();
        bench.mapping.focus = MapFocus::Remap;
        bench.frame(
            Request {
                confirm: true,
                ..request()
            },
            &[KeyCode::Enter],
            &[],
        );
        bench.frame(request(), &[KeyCode::KeyR], &[KeyCode::Enter]);
        bench.frame(request(), &[], &[KeyCode::KeyR]);
        // Ctrl down, then R with it, then both let go of.
        bench.frame(request(), &[KeyCode::ControlLeft], &[]);
        bench.frame(request(), &[KeyCode::KeyR], &[]);
        bench.frame(request(), &[], &[KeyCode::ControlLeft]);
        assert!(
            matches!(bench.mapping.step, Step::Assign { .. }),
            "R still held"
        );
        bench.frame(request(), &[], &[KeyCode::KeyR]);
        assert_eq!(
            bench.controls.restart.key_combos,
            [vec![KeyCode::ControlLeft, KeyCode::KeyR]]
        );
    }

    #[test]
    fn a_key_that_does_nothing_or_esc_calls_it_off() {
        let mut bench = Bench::new();
        bench.mapping.focus = MapFocus::Remap;
        let confirm = Request {
            confirm: true,
            ..request()
        };
        bench.frame(confirm, &[KeyCode::Enter], &[]);
        bench.frame(request(), &[KeyCode::KeyK], &[KeyCode::Enter]);
        assert_eq!(bench.mapping.step, Step::Idle);
        assert!(matches!(bench.mapping.note, Some((Note::Unbound(_), _))));
        bench.frame(confirm, &[KeyCode::Enter], &[KeyCode::KeyK]);
        bench.frame(request(), &[KeyCode::Escape], &[KeyCode::Enter]);
        assert_eq!(bench.mapping.step, Step::Idle);
        assert!(matches!(bench.mapping.note, Some((Note::Cancelled, _))));
        assert_eq!(bench.controls, Controls::default());
    }

    #[test]
    fn the_view_is_walked_and_resets_its_device() {
        let mut bench = Bench::new();
        bench.controls.remap(KeyCode::KeyW, &[KeyCode::KeyI]);
        // Test chosen at first, Remap after it, then Reset.
        assert_eq!(bench.mapping.focus, MapFocus::Test);
        let across = Request {
            side: 1,
            ..request()
        };
        bench.frame(across, &[], &[]);
        assert_eq!(bench.mapping.focus, MapFocus::Remap);
        bench.frame(across, &[], &[]);
        assert_eq!(bench.mapping.focus, MapFocus::Reset);
        bench.frame(
            Request {
                confirm: true,
                ..request()
            },
            &[],
            &[],
        );
        assert_eq!(bench.controls, Controls::default());
        // Up to the devices and across: the gamepad.
        bench.frame(
            Request {
                step: -1,
                ..request()
            },
            &[],
            &[],
        );
        assert_eq!(bench.mapping.focus, MapFocus::Devices);
        bench.frame(
            Request {
                side: 1,
                ..request()
            },
            &[],
            &[],
        );
        assert_eq!(bench.mapping.device, MapDevice::Gamepad);
        // Back closes it.
        assert!(bench.frame(
            Request {
                back: true,
                ..request()
            },
            &[],
            &[],
        ));
    }

    #[test]
    fn a_test_lights_what_is_pressed_and_works_nothing_until_esc() {
        let mut bench = Bench::new();
        bench.mapping.focus = MapFocus::Test;
        let confirm = Request {
            confirm: true,
            ..request()
        };
        bench.frame(confirm, &[KeyCode::Enter], &[]);
        assert!(bench.mapping.step == Step::Test);
        // W held: the throttle is done; the menu is not worked, even by
        // what would go back or confirm, and nothing is remapped.
        bench.frame(request(), &[KeyCode::KeyW], &[KeyCode::Enter]);
        let held = Held {
            keyboard: &bench.keyboard,
            gamepads: Vec::new(),
        };
        assert!(held.does(&bench.controls, MapDevice::Keyboard, Action::Throttle));
        assert!(!held.does(&bench.controls, MapDevice::Keyboard, Action::Brake));
        let back = Request {
            back: true,
            ..request()
        };
        assert!(!bench.frame(back, &[], &[KeyCode::KeyW]));
        assert!(!bench.frame(confirm, &[KeyCode::Enter], &[]));
        assert!(bench.mapping.step == Step::Test);
        assert_eq!(bench.controls, Controls::default());
        // Esc ends it, and Back works again.
        bench.frame(request(), &[KeyCode::Escape], &[KeyCode::Enter]);
        assert_eq!(bench.mapping.step, Step::Idle);
        assert!(bench.frame(back, &[], &[KeyCode::Escape]));
    }

    #[test]
    fn what_is_held_lights_up_only_testing_or_remapping() {
        let mut bench = Bench::new();
        bench.frame(request(), &[KeyCode::ArrowRight], &[]);
        let lit = |bench: &Bench| {
            let held = Held {
                keyboard: &bench.keyboard,
                gamepads: Vec::new(),
            };
            lights_up(&bench.mapping, &held, Pressed::Key(KeyCode::ArrowRight))
        };
        // Moving round the view: not lit.
        assert!(!lit(&bench));
        bench.mapping.step = Step::Test;
        assert!(lit(&bench));
        bench.mapping.step = Step::Choose { since: bench.now };
        assert!(lit(&bench));
    }

    #[test]
    fn while_testing_the_menu_does_nothing() {
        let mut bench = Bench::new();
        bench.frame(
            Request {
                confirm: true,
                ..request()
            },
            &[KeyCode::Enter],
            &[KeyCode::Enter],
        );
        assert_eq!(bench.mapping.step, Step::Test);
        // Arrows across and down, A (confirm), B (back) and the shoulder
        // buttons: the view stays as it is, still testing.
        let requests = [
            Request {
                side: 1,
                ..request()
            },
            Request {
                step: 1,
                ..request()
            },
            Request {
                confirm: true,
                ..request()
            },
            Request {
                back: true,
                ..request()
            },
            Request {
                tab: 1,
                ..request()
            },
        ];
        for request in requests {
            assert!(!bench.frame(request, &[], &[]));
            assert_eq!(bench.mapping.step, Step::Test);
            assert_eq!(bench.mapping.focus, MapFocus::Test);
            assert_eq!(bench.mapping.device, MapDevice::Keyboard);
        }
        // Esc ends it.
        bench.frame(request(), &[KeyCode::Escape], &[]);
        assert_eq!(bench.mapping.step, Step::Idle);
    }

    #[test]
    fn b_held_for_three_seconds_ends_a_test_and_waiting_does_not() {
        let mut bench = Bench::new();
        bench.mapping.focus = MapFocus::Test;
        bench.frame(
            Request {
                confirm: true,
                ..request()
            },
            &[KeyCode::Enter],
            &[KeyCode::Enter],
        );
        // However long nothing is pressed: still on.
        for _ in 0..200 {
            bench.frame(request(), &[], &[]);
        }
        assert_eq!(bench.mapping.step, Step::Test);
        // B pressed and let go of: tried out, still on.
        bench.pad.digital_mut().press(END_BUTTON);
        bench.frame(request(), &[], &[]);
        bench.pad.digital_mut().release(END_BUTTON);
        bench.frame(request(), &[], &[]);
        assert_eq!(bench.mapping.step, Step::Test);
        // Held: on until three seconds, then over.
        bench.pad.digital_mut().press(END_BUTTON);
        for _ in 0..29 {
            bench.frame(request(), &[], &[]);
        }
        assert_eq!(bench.mapping.step, Step::Test);
        for _ in 0..3 {
            bench.frame(request(), &[], &[]);
        }
        assert_eq!(bench.mapping.step, Step::Idle);
    }
}
