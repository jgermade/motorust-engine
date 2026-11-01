//! Choosing what to play, after the welcome screen (one player) or the
//! players' lobby (two), with the look of the start menu: translucent black
//! cards that light up in their colour, a chequered flag sliding across
//! the one chosen ([`CardMaterial`]), and slanted lettering.
//!
//! One player: the arcade championship on its own, big, and below it free
//! riding, a race, elimination and against the clock. Two players: free
//! riding, a race and elimination.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::circuits::{Circuits, CurrentCircuit};
use crate::gamepads::ActiveController;
use crate::hud::UiFont;
use crate::legend::{Keys, menu_prompts, music_toggle_prompt, spawn_legend};
use crate::locale::Language;
use crate::menu::GameState;
use crate::race::{
    CHAMPIONSHIP_LAPS, Championship, ELIMINATION_LAPS, Mode, Rules, start_championship,
};
use crate::settings::Settings;
use crate::ui::{
    ActiveDevice, InputDevice, MenuInput, MenuSound, UiIcons, current_active_device, spawn_title,
    text_font, ui_pixels,
};
use crate::vector_art::{Art, VectorArt};
use crate::welcome::{CardMaterial, FADE, GameMode, IDLE, MenuWidth};

pub struct ModesPlugin;

impl Plugin for ModesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModesMenu>()
            .init_resource::<ModeCards>()
            .add_systems(OnEnter(GameState::Modes), open)
            .add_systems(
                Update,
                (use_modes, show_modes, light_cards)
                    .chain()
                    .run_if(in_state(GameState::Modes)),
            );
    }
}

/// The modes on the screen, row by row, for one player or two.
pub fn rows(two_players: bool) -> Vec<Vec<Mode>> {
    if two_players {
        vec![vec![Mode::Race, Mode::Elimination, Mode::Free]]
    } else {
        vec![
            vec![Mode::Arcade],
            vec![Mode::Race, Mode::Elimination],
            vec![Mode::Checkpoint, Mode::Free],
        ]
    }
}

/// The card chosen: its row and its place in it.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModesMenu {
    pub row: usize,
    pub column: usize,
}

impl ModesMenu {
    /// Moves `step` rows (up -1, down 1) and `side` cards along (left -1,
    /// right 1), round and round, among `rows`.
    pub fn navigate(&mut self, step: i32, side: i32, rows: &[Vec<Mode>]) {
        if step != 0 {
            self.row = (self.row as i32 + step).rem_euclid(rows.len() as i32) as usize;
        }
        let along = rows[self.row].len();
        if side != 0 {
            self.column = (self.column as i32 + side).rem_euclid(along as i32) as usize;
        }
        self.column = self.column.min(along - 1);
    }

    pub fn chosen(&self, rows: &[Vec<Mode>]) -> Mode {
        rows[self.row][self.column.min(rows[self.row].len() - 1)]
    }
}

/// The colour each mode lights up in.
pub fn lit(mode: Mode) -> Color {
    match mode {
        Mode::Arcade => Color::srgb(0.04, 0.4, 1.0),
        Mode::Race => Color::srgb(0.5, 0.19, 0.88),
        Mode::Elimination => Color::srgb(0.88, 0.16, 0.16),
        Mode::Checkpoint => Color::srgb(1.0, 0.43, 0.0),
        Mode::Free => Color::srgb(0.08, 0.66, 0.34),
    }
}

/// Where the chequered flag slides on each card in a period (units).
fn flow(mode: Mode) -> Vec2 {
    match mode {
        Mode::Arcade => Vec2::new(51.615, -180.0),
        Mode::Race | Mode::Checkpoint => Vec2::new(180.0, 0.0),
        Mode::Elimination | Mode::Free => Vec2::new(-180.0, 0.0),
    }
}

const MODES: [Mode; 5] = [
    Mode::Arcade,
    Mode::Race,
    Mode::Elimination,
    Mode::Checkpoint,
    Mode::Free,
];

fn index(mode: Mode) -> usize {
    MODES.iter().position(|&m| m == mode).unwrap_or(0)
}

/// The look of each card, kept between visits so that they do not flash.
#[derive(Resource)]
struct ModeCards([Handle<CardMaterial>; 5]);

impl FromWorld for ModeCards {
    fn from_world(world: &mut World) -> Self {
        let mut materials = world.resource_mut::<Assets<CardMaterial>>();
        Self(MODES.map(|mode| materials.add(CardMaterial::new(IDLE, flow(mode), 700.0, 0.0))))
    }
}

/// The name of a mode, as on its card.
pub fn name(mode: Mode, lang: Language) -> &'static str {
    match (mode, lang) {
        (Mode::Arcade, _) => "ARCADE",
        (Mode::Free, Language::Spanish) => "LIBRE",
        (Mode::Free, Language::English) => "FREE RIDE",
        (Mode::Race, Language::Spanish) => "CARRERA",
        (Mode::Race, Language::English) => "RACE",
        (Mode::Elimination, Language::Spanish) => "ELIMINACIÓN",
        (Mode::Elimination, Language::English) => "ELIMINATION",
        (Mode::Checkpoint, _) => "CHECKPOINT",
    }
}

/// What is being played, as a badge in the colour of its card, with what
/// goes with it beside it (the track, for instance): on the screens after
/// the modes and in the pause menu.
pub fn spawn_mode_badge(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    mode: Mode,
    beside: Option<&str>,
    lang: Language,
) {
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(12),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    padding: UiRect::axes(px(14), px(4)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(lit(mode)),
            ))
            .with_child((
                Text::new(name(mode, lang)),
                text_font(font, 20.0),
                TextColor(Color::WHITE),
                TextShadow::default(),
            ));
            if let Some(beside) = beside {
                row.spawn((
                    Text::new(beside),
                    text_font(font, 22.0),
                    TextShadow::default(),
                ));
            }
        });
}

/// What a mode is, under its name, with `tracks` tracks.
fn description(mode: Mode, tracks: usize, lang: Language) -> String {
    match (mode, lang) {
        (Mode::Arcade, Language::Spanish) => format!(
            "Las {tracks} pistas, una tras otra, a {CHAMPIONSHIP_LAPS} vueltas: puntos en cada carrera"
        ),
        (Mode::Arcade, Language::English) => format!(
            "All {tracks} tracks, one after another, {CHAMPIONSHIP_LAPS} laps each: points in every race"
        ),
        (Mode::Free, Language::Spanish) => "Elige pista y corre, sin más".into(),
        (Mode::Free, Language::English) => "Choose a track and ride".into(),
        (Mode::Race, Language::Spanish) => "Elige pista y número de vueltas".into(),
        (Mode::Race, Language::English) => "Choose the track and the laps".into(),
        (Mode::Elimination, Language::Spanish) => {
            format!("Elige pista: {ELIMINATION_LAPS} vueltas, cada una cae el último")
        }
        (Mode::Elimination, Language::English) => {
            format!("Choose a track: {ELIMINATION_LAPS} laps, the last one out on each")
        }
        (Mode::Checkpoint, Language::Spanish) => "Contra el reloj: cada control da segundos".into(),
        (Mode::Checkpoint, Language::English) => {
            "Against the clock: checkpoints give seconds".into()
        }
    }
}

fn open(mut menu: ResMut<ModesMenu>) {
    *menu = ModesMenu::default();
}

#[allow(clippy::too_many_arguments)]
fn use_modes(
    mut input: MenuInput,
    game_mode: Res<GameMode>,
    mut menu: ResMut<ModesMenu>,
    mut rules: ResMut<Rules>,
    mut championship: ResMut<Championship>,
    mut current: ResMut<CurrentCircuit>,
    mut next: ResMut<NextState<GameState>>,
    mut sounds: MessageWriter<MenuSound>,
) {
    let request = input.read();
    let two = *game_mode == GameMode::SplitScreen;
    let rows = rows(two);
    if request.back {
        sounds.write(MenuSound::Move);
        next.set(if two {
            GameState::Lobby
        } else {
            GameState::Welcome
        });
        return;
    }
    let before = *menu;
    menu.navigate(request.step, request.side, &rows);
    if *menu != before {
        sounds.write(MenuSound::Move);
    }
    if !(request.confirm || request.start) {
        return;
    }
    sounds.write(MenuSound::Confirm);
    let mode = menu.chosen(&rows);
    rules.mode = mode;
    if mode.is_championship() {
        // The tracks come in order: straight to choosing the bike.
        start_championship(&mut championship, &mut current);
        next.set(GameState::Select);
    } else {
        next.set(GameState::Tracks);
    }
}

#[derive(Component)]
struct ModesScreen;

/// What the screen shows: when it changes, it is built again.
#[derive(Clone, PartialEq, Debug)]
struct Shown {
    two: bool,
    /// Whether the music is on, as the legend says.
    music: bool,
    language: Language,
    device: ActiveDevice,
    /// How wide the cards are (units of the interface).
    width: f32,
    /// Physical pixels to a unit of the interface (see [`ui_pixels`]).
    scale_factor: f32,
}

/// How much bigger the cards of the game modes are than the start menu,
/// their lettering as big as on cards its size.
const GROW: f32 = 1.15;

/// How wide the cards of the game modes are (px), with the start menu
/// `menu` px wide (the title's width) in a window `window` px wide: a
/// little wider than the start menu ([`GROW`]), whatever the resolution,
/// but never wider than most of the window.
fn cards_width(menu: f32, window: f32) -> f32 {
    (menu * GROW).min(window * 0.92)
}

#[allow(clippy::too_many_arguments)]
fn show_modes(
    mut commands: Commands,
    game_mode: Res<GameMode>,
    settings: Res<Settings>,
    device: Res<InputDevice>,
    active_controller: Option<Res<ActiveController>>,
    (window, ui_scale): (Query<&Window, With<PrimaryWindow>>, Option<Res<UiScale>>),
    menu_width: Res<MenuWidth>,
    (icons, circuits): (Res<UiIcons>, Res<Circuits>),
    cards: Res<ModeCards>,
    mut art: ResMut<VectorArt>,
    mut images: ResMut<Assets<Image>>,
    font: Res<UiFont>,
    shown: Query<Entity, With<ModesScreen>>,
    mut last: Local<Option<Shown>>,
) {
    let lang = settings.language;
    let active_dev = current_active_device(*device, active_controller.as_deref());
    let window = window.iter().next();
    let now = Shown {
        two: *game_mode == GameMode::SplitScreen,
        music: settings.music_enabled,
        language: lang,
        device: active_dev,
        // The window's width in units of the interface, as the menu's.
        width: cards_width(
            menu_width.0,
            window.map_or(1280.0, Window::width) / ui_scale.as_deref().map_or(1.0, |scale| scale.0),
        ),
        scale_factor: ui_pixels(window, ui_scale.as_deref()),
    };
    if last.as_ref() == Some(&now) && !shown.is_empty() {
        return;
    }
    *last = Some(now.clone());
    for entity in &shown {
        commands.entity(entity).despawn();
    }

    // A little bigger than the start menu, the cards in proportion and
    // their lettering as on cards its size.
    let width = now.width;
    let gap = width * 0.025;
    let rows = rows(now.two);
    let scale = now.scale_factor;
    let mut lettering = |text: &str, height: f32| {
        let drawn = art.get(
            Art::Text(text.into()),
            (height * scale).round() as u32,
            &mut images,
        );
        (drawn.0, drawn.1.as_vec2() / scale)
    };
    // The cards' size, and their lettering's: the championship's as tall as
    // it fits; the others all alike, as tall as the widest name fits. With
    // two players, in a row of their own, they are as tall as the
    // championship, with room for their descriptions.
    let single = rows.len() == 1;
    let card = |row: &[Mode]| {
        let big = row.len() == 1;
        let height = if big || single {
            width * 0.2
        } else {
            width * 0.14
        };
        let card_width = (width - gap * (row.len() - 1) as f32) / row.len() as f32;
        let letters = (if big { height * 0.42 } else { height * 0.36 }) / GROW;
        (big, height, card_width, letters)
    };
    let mut fitted = |mode: Mode, (_, _, card_width, letters): (bool, f32, f32, f32)| {
        let widest = card_width / GROW * 0.86;
        let wide = lettering(name(mode, lang), letters).1.x;
        if wide > widest {
            letters * widest / wide
        } else {
            letters
        }
    };
    let mut small_letters = f32::MAX;
    for row in rows.iter().filter(|row| row.len() > 1) {
        for &mode in row {
            small_letters = small_letters.min(fitted(mode, card(row)));
        }
    }
    let big_letters: Vec<f32> = rows
        .iter()
        .filter(|row| row.len() == 1)
        .map(|row| fitted(row[0], card(row)))
        .collect();

    commands
        .spawn((
            ModesScreen,
            DespawnOnExit(GameState::Modes),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(gap),
                padding: UiRect::bottom(px(40)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.02, 0.06, 0.3)),
        ))
        .with_children(|screen| {
            spawn_title(
                screen,
                &font,
                match (now.two, lang) {
                    (false, Language::Spanish) => "MODO DE JUEGO  ·  1P",
                    (false, Language::English) => "GAME MODE  ·  1P",
                    (true, Language::Spanish) => "MODO DE JUEGO  ·  2P",
                    (true, Language::English) => "GAME MODE  ·  2P",
                },
                44.0,
            );
            for row in &rows {
                // The championship, alone in its row, is taller.
                let (big, height, card_width, _) = card(row);
                let letters = if big {
                    big_letters.first().copied().unwrap_or(small_letters)
                } else {
                    small_letters
                };
                screen
                    .spawn(Node {
                        width: px(width),
                        column_gap: px(gap),
                        ..default()
                    })
                    .with_children(|line| {
                        for &mode in row {
                            let (image, size) = lettering(name(mode, lang), letters);
                            line.spawn((
                                Node {
                                    width: px(card_width),
                                    height: px(height),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    row_gap: px(height * 0.06),
                                    border_radius: BorderRadius::all(px(18.0 * width / 1417.0)),
                                    ..default()
                                },
                                MaterialNode(cards.0[index(mode)].clone()),
                            ))
                            .with_children(|card| {
                                card.spawn((
                                    ImageNode::new(image),
                                    Node {
                                        width: px(size.x),
                                        height: px(size.y),
                                        ..default()
                                    },
                                ));
                                card.spawn((
                                    Text::new(description(mode, circuits.len(), lang)),
                                    text_font(&font, if big { 20.0 } else { 15.0 }),
                                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                                    TextLayout::justify(Justify::Center),
                                    Node {
                                        max_width: px(card_width * 0.9),
                                        ..default()
                                    },
                                ));
                            });
                        }
                    });
            }
            let mut prompts = menu_prompts(
                active_dev,
                &[
                    (Keys::Confirm, lang.hint_accept()),
                    (Keys::Back, lang.back()),
                ],
            );
            prompts.push(music_toggle_prompt(active_dev, lang, now.music));
            spawn_legend(screen, &font, &icons, active_dev, &prompts);
        });
}

/// The chosen card lights up in its colour and the rest go dark, little by
/// little.
fn light_cards(
    time: Res<Time<Real>>,
    menu: Res<ModesMenu>,
    game_mode: Res<GameMode>,
    cards: Res<ModeCards>,
    mut materials: ResMut<Assets<CardMaterial>>,
) {
    let step = (time.delta_secs() / FADE).min(1.0);
    let chosen = menu.chosen(&rows(*game_mode == GameMode::SplitScreen));
    for (mode, handle) in MODES.iter().zip(&cards.0) {
        let (color, flag) = if *mode == chosen {
            (lit(*mode), 1.0)
        } else {
            (IDLE, 0.0)
        };
        let Some(material) = materials.get(handle) else {
            continue;
        };
        if material.is(color, flag) {
            continue;
        }
        if let Some(mut material) = materials.get_mut(handle) {
            material.fade_to(color, flag, step);
        }
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    #[test]
    fn the_modes_are_a_little_wider_than_the_start_menu_at_any_resolution() {
        let menu = 620.0;
        // The same, from a small window to a large one.
        for window in [1280.0, 1600.0, 1920.0, 2560.0] {
            assert_eq!(cards_width(menu, window), menu * GROW);
        }
        // But inside a narrow window.
        assert_eq!(cards_width(menu, 600.0), 600.0 * 0.92);
    }

    #[test]
    fn one_player_gets_the_championship_big_and_four_below() {
        let rows = rows(false);
        assert_eq!(rows[0], [Mode::Arcade]);
        assert_eq!(rows[1..].iter().flatten().count(), 4);
        let two = rows_flat(true);
        assert_eq!(two, [Mode::Race, Mode::Elimination, Mode::Free]);
    }

    fn rows_flat(two: bool) -> Vec<Mode> {
        rows(two).into_iter().flatten().collect()
    }

    #[test]
    fn the_cards_are_walked_row_by_row() {
        let rows = rows(false);
        let mut menu = ModesMenu::default();
        assert_eq!(menu.chosen(&rows), Mode::Arcade);
        menu.navigate(1, 0, &rows);
        assert_eq!(menu.chosen(&rows), Mode::Race);
        menu.navigate(0, 1, &rows);
        assert_eq!(menu.chosen(&rows), Mode::Elimination);
        menu.navigate(1, 0, &rows);
        assert_eq!(menu.chosen(&rows), Mode::Free);
        // Up to the championship and back down: the same side.
        menu.navigate(1, 0, &rows);
        assert_eq!(menu.chosen(&rows), Mode::Arcade);
        menu.navigate(0, 1, &rows);
        assert_eq!(menu.chosen(&rows), Mode::Arcade);
        menu.navigate(-1, 0, &rows);
        assert_eq!(menu.chosen(&rows), Mode::Checkpoint);
        // Two players: one row, left and right.
        let two = super::rows(true);
        let mut menu = ModesMenu::default();
        menu.navigate(0, -1, &two);
        assert_eq!(menu.chosen(&two), Mode::Free);
        menu.navigate(1, 0, &two);
        assert_eq!(menu.chosen(&two), Mode::Free);
    }

    pub fn every_mode_has_a_name_and_a_description_in_each_language_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for mode in MODES {
            for lang in [Language::Spanish, Language::English] {
                assert!(!name(mode, lang).is_empty());
                assert!(!description(mode, circuits.len(), lang).is_empty());
            }
        }
    }

    #[test]
    fn every_mode_has_a_name_and_a_description_in_each_language() {
        every_mode_has_a_name_and_a_description_in_each_language_on(&crate::testing::DEMO);
    }

    #[test]
    fn checkpoint_s_orange_is_vivid_with_its_white_lettering_as_readable() {
        // Its brightest channel full and none of the others (the most
        // vivid there is of its hue), and as dark as before: the white
        // lettering on it 2.8 : 1 (relative luminance).
        let orange = lit(Mode::Checkpoint).to_srgba();
        assert_eq!((orange.red, orange.blue), (1.0, 0.0));
        let linear = lit(Mode::Checkpoint).to_linear();
        let luminance = 0.2126 * linear.red + 0.7152 * linear.green + 0.0722 * linear.blue;
        let contrast = 1.05 / (luminance + 0.05);
        assert!((2.75..2.9).contains(&contrast), "{contrast}");
    }
}
