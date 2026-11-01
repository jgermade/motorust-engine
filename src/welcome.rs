//! Pantalla de bienvenida: el título del juego mientras la cámara recorre la
//! pista, con opciones de 1 jugador, 2 jugadores, Opciones y Salir.
//! En la web no se sale del juego (se cierra la pestaña).
//!
//! The menu follows the menu design: two cards side by side (one helmet for
//! 1 player, two facing each other for 2 players) and two full-width
//! banners, translucent black; the chosen one lights up in its colour with a
//! faint chequered flag sliding across it ([`CardMaterial`]). The menu is as
//! wide as the title of the game.

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::ui::UiSystems;
use bevy::window::PrimaryWindow;

use crate::camera::{CameraGlide, CameraPlayer};
use crate::circuits::{Circuits, CurrentCircuit};
use crate::gamepads::ActiveController;
use crate::hud::UiFont;
use crate::legend::{Keys, menu_prompts, music_toggle_prompt, spawn_legend};
use crate::locale::Language;
use crate::menu::GameState;
use crate::options::{OptionsInput, OptionsMenu, OptionsReturn};
use crate::settings::{Settings, WEB};
use crate::track::Track;
use crate::ui::{
    ActiveDevice, EXIT_RED, InputDevice, MenuInput, MenuSound, Row, UiIcons, current_active_device,
    spawn_logo, spawn_panel, spawn_title, ui_pixels, version_label,
};
use crate::vector_art::{Art, VectorArt, advance};
use crate::world::scenery::{Scenery, span};

/// La cámara recorre la pista a esta velocidad (m/s), a un lado del centro
/// de la carretera (m, positivo a la izquierda) y a esta altura sobre ella;
/// mira a un punto de la carretera más adelante (m), a esta altura, con este
/// campo de visión (rad).
const FLYBY_SPEED: f32 = 22.0;
/// How far before the line the flight over a track sets off (m), whenever
/// another track is chosen: its start is always seen first, which can be
/// shown before the rest of it is ready (see `crate::world::preview`).
const FLYBY_RUN_UP: f32 = 90.0;
const FLYBY_SIDE: f32 = -4.0;
const FLYBY_HEIGHT: f32 = 6.0;
const FLYBY_AHEAD: f32 = 45.0;
const FLYBY_LOOK_HEIGHT: f32 = 1.5;
const FLYBY_FOV: f32 = 0.9;
/// How high it flies over the road through a tunnel (m): well under the
/// rock, which stands [`crate::world::structures::TUNNEL_HEIGHT`] over the
/// road, where at its own height it would fly into the ceiling and its
/// lamps. It comes down to it over this many metres before the mouth, and
/// goes back up over as many past it.
const FLYBY_TUNNEL_HEIGHT: f32 = 2.8;
const FLYBY_TUNNEL_FADE: f32 = 25.0;
/// How high it flies under the arches over the road, the start's and the
/// checkpoints' (m): well under their beam, which at its own height it
/// would fly through. It is down this far either side of an arch (m), and
/// comes down to it and goes back up over this many metres more.
const FLYBY_ARCH_HEIGHT: f32 = 3.2;
const FLYBY_ARCH_UNDER: f32 = 6.0;
const FLYBY_ARCH_FADE: f32 = 22.0;

/// Size of the title of the game (px). The menu is exactly as wide.
pub const TITLE_SIZE: f32 = 110.0;

/// The menu design is 1417 units wide; everything below is in its units.
const DESIGN_WIDTH: f32 = 1417.0;
/// Space between the row of cards and the first banner, and between banners.
const ROW_GAPS: [f32; 2] = [40.0, 36.0];
const CORNER_RADIUS: f32 = 18.0;
/// Fill of the buttons that are not chosen: black, 60 % opaque in the
/// design. The design mixes it with what is behind in sRGB, which darkens
/// more than mixing in linear light as Bevy does: 87 % opaque here looks the
/// same (1 - 0.4^2.2).
pub const IDLE: Color = Color::srgba(0.0, 0.0, 0.0, 0.867);
/// Seconds a button takes to light up or go dark.
pub const FADE: f32 = 0.15;

/// The buttons of the menu, in order: 1 player, 2 players, Options, Exit.
struct Button {
    size: Vec2,
    /// Colour when chosen.
    lit: Color,
    /// Where the chequered flag slides in one period (units): sideways for
    /// the cards, along the slant of the squares for the banners.
    flow: Vec2,
}

const BUTTONS: [Button; 4] = [
    Button {
        size: Vec2::new(683.0, 342.0),
        lit: Color::srgb(10.0 / 255.0, 100.0 / 255.0, 1.0),
        flow: Vec2::new(180.0, 0.0),
    },
    Button {
        size: Vec2::new(684.0, 342.0),
        lit: Color::srgb(128.0 / 255.0, 48.0 / 255.0, 224.0 / 255.0),
        flow: Vec2::new(-180.0, 0.0),
    },
    Button {
        size: Vec2::new(1417.0, 146.0),
        lit: Color::srgb(251.0 / 255.0, 191.0 / 255.0, 0.0),
        flow: Vec2::new(51.615, -180.0),
    },
    Button {
        size: Vec2::new(1417.0, 146.0),
        lit: EXIT_RED,
        flow: Vec2::new(-51.615, 180.0),
    },
];

/// Where the helmets go on the cards (units, from the top left corner of
/// the card): left, top, width and height.
const HELMET_1P: Rect = rect(46.6, 53.78, 261.88, 269.62);
const HELMETS_2P: [Rect; 2] = [
    rect(26.77, 76.6, 217.36, 223.78),
    rect(439.88, 76.6, 217.36, 223.78),
];

const fn rect(left: f32, top: f32, width: f32, height: f32) -> Rect {
    Rect {
        min: Vec2::new(left, top),
        max: Vec2::new(left + width, top + height),
    }
}
/// Where the lettering of each button is centred (units) and how tall its
/// capitals are.
const LETTERS: [(Vec2, f32); 4] = [
    (Vec2::new(484.52, 187.0), 160.0),
    (Vec2::new(350.06, 181.0), 98.0),
    (Vec2::new(712.29, 74.03), 72.13),
    (Vec2::new(708.2, 73.6), 66.88),
];

pub struct WelcomePlugin;

impl Plugin for WelcomePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "menu_card.wgsl");
        app.add_plugins(UiMaterialPlugin::<CardMaterial>::default())
            .init_resource::<Welcome>()
            .init_resource::<GameMode>()
            .init_resource::<MenuWidth>()
            .init_resource::<VectorArt>()
            .init_resource::<MenuCards>()
            .add_systems(OnEnter(GameState::Welcome), open)
            .add_systems(
                Update,
                (
                    (
                        use_welcome.after(OptionsInput),
                        (show_welcome, light_cards).chain(),
                    )
                        .chain()
                        .run_if(in_state(GameState::Welcome)),
                    fly_over_track.run_if(
                        in_state(GameState::Welcome)
                            .or_else(in_state(GameState::Lobby))
                            .or_else(in_state(GameState::Modes))
                            // Choosing the track from the road, it flies
                            // on, over the track being chosen.
                            .or_else(
                                in_state(GameState::Tracks)
                                    .and_then(crate::track_select::from_the_road),
                            ),
                    ),
                ),
            )
            .add_systems(
                PostUpdate,
                fit_menu_to_title
                    .after(UiSystems::Layout)
                    .run_if(in_state(GameState::Welcome)),
            );
    }
}

/// Modo de juego elegido en la pantalla principal.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameMode {
    #[default]
    SinglePlayer,
    SplitScreen,
}

/// Qué página se ve y qué opción está elegida.
#[derive(Resource, Default)]
pub struct Welcome {
    pub page: Page,
    pub selected: usize,
    /// The card of the top row last chosen (0 or 1), to go back to it from
    /// the banners.
    pub column: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    #[default]
    Main,
    QuitConfirm,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    SinglePlayer,
    SplitScreen,
    Options,
    Quit,
    CancelQuit,
    ConfirmQuit,
}

pub fn items(page: Page) -> Vec<Item> {
    match page {
        Page::Main => {
            if WEB {
                vec![Item::SinglePlayer, Item::SplitScreen, Item::Options]
            } else {
                vec![
                    Item::SinglePlayer,
                    Item::SplitScreen,
                    Item::Options,
                    Item::Quit,
                ]
            }
        }
        Page::QuitConfirm => vec![Item::CancelQuit, Item::ConfirmQuit],
    }
}

/// The option chosen after moving `step` (up -1, down 1) and `side` (left -1,
/// right 1) from `selected`, on a menu of `count` options whose first two
/// are side by side; and the card of the top row to remember.
fn navigate(selected: usize, column: usize, step: i32, side: i32, count: usize) -> (usize, usize) {
    // Rows: the two cards, then one banner per row.
    let row = selected.saturating_sub(1);
    let rows = count - 1;
    let mut selected = selected;
    if step != 0 {
        let row = (row as i32 + step).rem_euclid(rows as i32) as usize;
        selected = if row == 0 { column } else { row + 1 };
    }
    if side != 0 && selected < 2 {
        selected = if side > 0 { 1 } else { 0 };
    }
    let column = if selected < 2 { selected } else { column };
    (selected, column)
}

fn open(mut welcome: ResMut<Welcome>, mut options_menu: ResMut<OptionsMenu>) {
    *welcome = Welcome::default();
    options_menu.close();
}

#[allow(clippy::too_many_arguments)]
fn use_welcome(
    mut input: MenuInput,
    mut welcome: ResMut<Welcome>,
    mut options_menu: ResMut<OptionsMenu>,
    mut game_mode: ResMut<GameMode>,
    mut next: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut sounds: MessageWriter<MenuSound>,
) {
    // While the options are open, and in the frame they close (the key that
    // closed them must not also act here).
    if options_menu.is_open || options_menu.is_changed() {
        return;
    }

    let request = input.read();
    let items = items(welcome.page);

    // Esc o B:
    if request.back {
        match welcome.page {
            Page::Main if !WEB => {
                welcome.page = Page::QuitConfirm;
                welcome.selected = 0;
                sounds.write(MenuSound::Move);
            }
            Page::Main => {}
            Page::QuitConfirm => {
                welcome.page = Page::Main;
                welcome.selected = self::items(Page::Main).len() - 1;
                sounds.write(MenuSound::Move);
            }
        }
        return;
    }

    match welcome.page {
        Page::Main => {
            let (selected, column) = navigate(
                welcome.selected,
                welcome.column,
                request.step,
                request.side,
                items.len(),
            );
            if selected != welcome.selected {
                welcome.selected = selected;
                sounds.write(MenuSound::Move);
            }
            welcome.column = column;
        }
        Page::QuitConfirm => {
            let delta = if request.step != 0 {
                request.step
            } else {
                request.side
            };
            if delta != 0 {
                welcome.selected =
                    (welcome.selected as i32 + delta).rem_euclid(items.len() as i32) as usize;
                sounds.write(MenuSound::Move);
            }
        }
    }

    if !(request.confirm || request.start) {
        return;
    }
    match items[welcome.selected.min(items.len() - 1)] {
        Item::SinglePlayer => {
            sounds.write(MenuSound::Confirm);
            *game_mode = GameMode::SinglePlayer;
            next.set(GameState::Modes);
        }
        Item::SplitScreen => {
            sounds.write(MenuSound::Confirm);
            *game_mode = GameMode::SplitScreen;
            next.set(GameState::Lobby);
        }
        Item::Options => {
            sounds.write(MenuSound::Confirm);
            options_menu.open(OptionsReturn::Welcome);
        }
        Item::Quit => {
            sounds.write(MenuSound::Move);
            welcome.page = Page::QuitConfirm;
            welcome.selected = 0;
        }
        Item::CancelQuit => {
            sounds.write(MenuSound::Move);
            welcome.page = Page::Main;
            welcome.selected = self::items(Page::Main).len() - 1;
        }
        Item::ConfirmQuit => {
            exit.write(AppExit::Success);
        }
    }
}

/// How wide the menu is (px): as wide as the title, measured once it is laid
/// out. It starts with the width the font gives. The game modes screen is
/// sized from it too, so both look alike at any resolution.
#[derive(Resource)]
pub struct MenuWidth(pub f32);

impl Default for MenuWidth {
    fn default() -> Self {
        Self(advance(&crate::game_info::info().logo.text(), TITLE_SIZE).round())
    }
}

/// The menu buttons look like this (see `menu_card.wgsl`).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct CardMaterial {
    /// Fill colour (sRGB, with alpha).
    #[uniform(0)]
    color: Vec4,
    /// xy: where the flag slides in one period (units); z: width of the
    /// button (units); w: how much the flag shows (0 to 1).
    #[uniform(1)]
    flow: Vec4,
}

impl CardMaterial {
    /// A button `width` units wide filled with `color`, whose chequered flag
    /// slides `flow` units in a period, as much shown as `flag` (0 to 1).
    pub fn new(color: Color, flow: Vec2, width: f32, flag: f32) -> Self {
        Self {
            color: Srgba::from(color).to_vec4(),
            flow: flow.extend(width).extend(flag),
        }
    }

    /// Lights it up towards `color` with its flag showing `flag`, a
    /// fraction `step` of the way: done little by little, it fades.
    pub fn fade_to(&mut self, color: Color, flag: f32, step: f32) {
        self.color = approach(self.color, Srgba::from(color).to_vec4(), step);
        self.flow.w = approach(Vec4::splat(self.flow.w), Vec4::splat(flag), step).x;
    }

    /// Whether it has finished fading to `color` with its flag at `flag`.
    pub fn is(&self, color: Color, flag: f32) -> bool {
        self.color == Srgba::from(color).to_vec4() && self.flow.w == flag
    }
}

impl UiMaterial for CardMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://motorust_engine/menu_card.wgsl".into()
    }
}

/// The look of each button, kept between visits so that they do not flash.
#[derive(Resource)]
struct MenuCards([Handle<CardMaterial>; 4]);

impl FromWorld for MenuCards {
    fn from_world(world: &mut World) -> Self {
        let mut materials = world.resource_mut::<Assets<CardMaterial>>();
        Self(BUTTONS.each_ref().map(|button| {
            materials.add(CardMaterial {
                color: Srgba::from(IDLE).to_vec4(),
                flow: button.flow.extend(button.size.x).extend(0.0),
            })
        }))
    }
}

#[derive(Component)]
struct WelcomeScreen;

#[derive(Component)]
struct WelcomeTitle;

/// What the screen shows: when it changes, the screen is built again.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Shown {
    page: Page,
    /// The chosen option, on the pages that show it as text.
    selected: Option<usize>,
    options_open: bool,
    /// Whether the music is on, as the legend says.
    music: bool,
    language: Language,
    device: ActiveDevice,
    width: f32,
    /// Physical pixels to a unit of the interface (see [`ui_pixels`]): the
    /// art is drawn at its size on the screen.
    scale_factor: f32,
}

#[allow(clippy::too_many_arguments)]
fn show_welcome(
    mut commands: Commands,
    welcome: Res<Welcome>,
    options_menu: Res<OptionsMenu>,
    settings: Res<Settings>,
    device: Res<InputDevice>,
    active_controller: Option<Res<ActiveController>>,
    width: Res<MenuWidth>,
    (window, ui_scale): (Query<&Window, With<PrimaryWindow>>, Option<Res<UiScale>>),
    icons: Res<UiIcons>,
    cards: Res<MenuCards>,
    mut art: ResMut<VectorArt>,
    mut images: ResMut<Assets<Image>>,
    font: Res<UiFont>,
    shown: Query<Entity, With<WelcomeScreen>>,
    mut last: Local<Option<Shown>>,
) {
    let lang = settings.language;
    let active_dev = current_active_device(*device, active_controller.as_deref());
    let now = Shown {
        page: welcome.page,
        selected: (welcome.page == Page::QuitConfirm).then_some(welcome.selected),
        options_open: options_menu.is_open,
        music: settings.music_enabled,
        language: lang,
        device: active_dev,
        width: width.0,
        scale_factor: ui_pixels(window.iter().next(), ui_scale.as_deref()),
    };
    // Built already (the screen goes when leaving the state, and is hidden
    // while the options are open).
    if *last == Some(now) && (options_menu.is_open || !shown.is_empty()) {
        return;
    }
    *last = Some(now);

    for entity in &shown {
        commands.entity(entity).despawn();
    }

    // Si el menú de opciones está abierto, ocultamos la pantalla de bienvenida
    if options_menu.is_open {
        return;
    }

    // The art, drawn at its size on the screen (physical pixels per design
    // unit), and the lettering in the language of the game.
    let pixels = width.0 / DESIGN_WIDTH * now.scale_factor;
    let mut draw =
        |piece: Art, units: f32| art.get(piece, (units * pixels).round() as u32, &mut images);
    let helmets = [
        draw(Art::Helmet, HELMET_1P.height()),
        draw(Art::Helmet, HELMETS_2P[0].height()),
    ];
    let texts = [
        "1P".to_string(),
        "2P".to_string(),
        lang.options().to_uppercase(),
        lang.exit_game().to_uppercase(),
    ];
    let labels: [_; 4] = std::array::from_fn(|index| {
        let (_, height) = LETTERS[index];
        let text = Art::Text(texts[index].clone());
        let label = draw(text.clone(), height);
        // As tall as in the design, unless that makes it too wide.
        let widest = BUTTONS[index].size.x * 0.9 * pixels;
        if label.1.x as f32 > widest {
            draw(text, height * widest / label.1.x as f32)
        } else {
            label
        }
    });
    let menu_art = MenuArt {
        pixels,
        helmets,
        labels,
    };

    commands
        .spawn((
            WelcomeScreen,
            DespawnOnExit(GameState::Welcome),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.02, 0.06, 0.3)),
        ))
        .with_children(|screen| {
            match welcome.page {
                Page::Main => {
                    screen
                        .spawn(Node {
                            margin: UiRect::bottom(px(8)),
                            ..default()
                        })
                        .with_children(|title| {
                            spawn_logo(title, &font, TITLE_SIZE);
                        })
                        .insert(WelcomeTitle);
                    spawn_menu(screen, width.0, &cards, &menu_art);

                    let mut hints = vec![(Keys::Confirm, lang.hint_accept())];
                    if !WEB {
                        hints.push((Keys::Back, lang.hint_exit()));
                    }
                    let mut prompts = menu_prompts(active_dev, &hints);
                    prompts.push(music_toggle_prompt(
                        active_dev,
                        lang,
                        settings.music_enabled,
                    ));
                    spawn_legend(screen, &font, &icons, active_dev, &prompts);
                }
                Page::QuitConfirm => {
                    spawn_title(
                        screen,
                        &font,
                        match lang {
                            Language::Spanish => "¿SALIR DEL JUEGO?",
                            Language::English => "EXIT GAME?",
                        },
                        48.0,
                    );

                    // The same panel as the pause menu's, with leaving in red.
                    let rows = [Row::new(lang.hint_cancel()), Row::danger(lang.exit_game())];
                    spawn_panel(screen, &font, &rows, welcome.selected, false);

                    let prompts = menu_prompts(
                        active_dev,
                        &[
                            (Keys::Confirm, lang.hint_accept()),
                            (Keys::Back, lang.hint_cancel()),
                        ],
                    );
                    spawn_legend(screen, &font, &icons, active_dev, &prompts);
                }
            }

            screen.spawn(version_label(&font));
        });
}

/// The images of the menu, and their size, in physical pixels.
struct MenuArt {
    /// Physical pixels per design unit.
    pixels: f32,
    /// The helmet of the 1 player card and of the 2 players card.
    helmets: [(Handle<Image>, UVec2); 2],
    /// The lettering of each button.
    labels: [(Handle<Image>, UVec2); 4],
}

/// The buttons, `width` px wide, as in the menu design.
fn spawn_menu(screen: &mut ChildSpawnerCommands, width: f32, cards: &MenuCards, art: &MenuArt) {
    let scale = width / DESIGN_WIDTH;
    let button = |index: usize| ButtonLook {
        index,
        scale,
        pixels: art.pixels,
        material: cards.0[index].clone(),
        label: art.labels[index].clone(),
    };
    screen
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            width: px(width),
            ..default()
        })
        .with_children(|menu| {
            menu.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                width: percent(100),
                ..default()
            })
            .with_children(|row| {
                spawn_button(row, button(0)).with_children(|card| {
                    card.spawn((
                        ImageNode::new(art.helmets[0].0.clone()),
                        place(HELMET_1P, scale),
                    ));
                });
                spawn_button(row, button(1)).with_children(|card| {
                    for (helmet, flip_x) in HELMETS_2P.into_iter().zip([false, true]) {
                        card.spawn((
                            ImageNode {
                                image: art.helmets[1].0.clone(),
                                flip_x,
                                ..default()
                            },
                            place(helmet, scale),
                        ));
                    }
                });
            });
            for (index, gap) in (2..items(Page::Main).len()).zip(ROW_GAPS) {
                menu.spawn(Node {
                    height: px(gap * scale),
                    ..default()
                });
                spawn_button(menu, button(index));
            }
        });
}

/// What a button needs to be built.
struct ButtonLook {
    index: usize,
    /// Logical pixels per design unit.
    scale: f32,
    /// Physical pixels per design unit.
    pixels: f32,
    material: Handle<CardMaterial>,
    /// The image of its lettering and its size (physical px).
    label: (Handle<Image>, UVec2),
}

/// A button with its lettering; the helmets go in after.
fn spawn_button<'a>(parent: &'a mut ChildSpawnerCommands, look: ButtonLook) -> EntityCommands<'a> {
    let ButtonLook {
        index,
        scale,
        pixels,
        material,
        label: (image, image_size),
    } = look;
    let size = BUTTONS[index].size;
    // Centred where the design has it, one image pixel per screen pixel.
    let (center, _) = LETTERS[index];
    let letters = Rect::from_center_size(center, image_size.as_vec2() / pixels);
    let mut button = parent.spawn((
        Node {
            width: px(size.x * scale),
            height: px(size.y * scale),
            border_radius: BorderRadius::all(px(CORNER_RADIUS * scale)),
            ..default()
        },
        MaterialNode(material),
    ));
    button.with_child((ImageNode::new(image), place(letters, scale)));
    button
}

/// A node over its card at `rect` (design units).
fn place(rect: Rect, scale: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(rect.min.x * scale),
        top: px(rect.min.y * scale),
        width: px(rect.width() * scale),
        height: px(rect.height() * scale),
        ..default()
    }
}

/// The chosen button lights up and the rest go dark, little by little.
fn light_cards(
    time: Res<Time<Real>>,
    welcome: Res<Welcome>,
    cards: Res<MenuCards>,
    mut materials: ResMut<Assets<CardMaterial>>,
) {
    let step = (time.delta_secs() / FADE).min(1.0);
    for (index, handle) in cards.0.iter().enumerate() {
        let chosen = welcome.page == Page::Main && welcome.selected == index;
        let (color, flag) = if chosen {
            (Srgba::from(BUTTONS[index].lit).to_vec4(), 1.0)
        } else {
            (Srgba::from(IDLE).to_vec4(), 0.0)
        };
        let Some(material) = materials.get(handle) else {
            continue;
        };
        if material.color == color && material.flow.w == flag {
            continue;
        }
        let Some(mut material) = materials.get_mut(handle) else {
            continue;
        };
        material.color = approach(material.color, color, step);
        material.flow.w = approach(Vec4::splat(material.flow.w), Vec4::splat(flag), step).x;
    }
}

/// `from` moved a fraction `step` of the way to `to`, and all the way when
/// it is close.
fn approach(from: Vec4, to: Vec4, step: f32) -> Vec4 {
    let next = from.lerp(to, step);
    if next.distance(to) < 0.004 { to } else { next }
}

/// The menu is as wide as the title: once the title is laid out, its width
/// is the menu's (and the screen is built again if it was not).
fn fit_menu_to_title(
    title: Query<&ComputedNode, With<WelcomeTitle>>,
    mut width: ResMut<MenuWidth>,
) {
    let Some(node) = title.iter().next() else {
        return;
    };
    let measured = (node.size().x * node.inverse_scale_factor).round();
    if measured > 0.0 && measured != width.0 {
        width.0 = measured;
    }
}

/// Where on a lap `lap` metres long the flight over it sets off: just
/// before the line.
fn flyby_start(lap: f32) -> f32 {
    (lap - FLYBY_RUN_UP).rem_euclid(lap)
}

/// How high the camera flies over the road `travelled` metres along a lap
/// of `lap` metres: its own height, and under the rock through a tunnel,
/// coming down to it before the mouth and up again past it, and under the
/// arches over the road, coming down to pass under each and up again.
fn flyby_height(scenery: &Scenery, travelled: f32, lap: f32) -> f32 {
    let under_arch = scenery
        .gantries
        .iter()
        .map(|gantry| {
            let about = span(
                gantry.along - FLYBY_ARCH_UNDER - FLYBY_ARCH_FADE,
                gantry.along + FLYBY_ARCH_UNDER + FLYBY_ARCH_FADE,
            );
            about.weight(travelled, lap, FLYBY_ARCH_FADE)
        })
        .fold(0.0, f32::max);
    let under_rock = scenery
        .tunnels
        .iter()
        .map(|tunnel| {
            // The fade is before the mouth and past it, not inside, so it
            // is already down as it goes in.
            let about = span(
                tunnel.from - FLYBY_TUNNEL_FADE,
                tunnel.to + FLYBY_TUNNEL_FADE,
            );
            about.weight(travelled, lap, FLYBY_TUNNEL_FADE)
        })
        .fold(0.0, f32::max);
    FLYBY_HEIGHT
        .lerp(FLYBY_ARCH_HEIGHT, under_arch)
        .min(FLYBY_HEIGHT.lerp(FLYBY_TUNNEL_HEIGHT, under_rock))
}

/// La cámara recorre la pista despacio, un poco por encima de la carretera,
/// from just before the line of each track chosen.
#[allow(clippy::too_many_arguments)]
fn fly_over_track(
    time: Res<Time>,
    track: Res<Track>,
    circuit: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    camera: Single<(&mut Transform, &mut Projection), With<CameraPlayer>>,
    mut glide: ResMut<CameraGlide>,
    mut travelled: Local<f32>,
    mut chosen: Local<bool>,
) {
    let dt = time.delta_secs();
    let length = track.sample_count() as f32 * track.step_length();
    // Another track chosen: its own track takes the place of the last at
    // the start of the next frame (see `crate::world`), from then on from
    // just before its line.
    if circuit.is_changed() {
        *chosen = true;
    } else if *chosen {
        *chosen = false;
        *travelled = flyby_start(length);
    }
    *travelled = (*travelled + FLYBY_SPEED * dt) % length;
    // Un punto de la carretera a `distance` m de la meta, entre dos muestras.
    let at = |distance: f32, lateral: f32| {
        let samples = distance.rem_euclid(length) / track.step_length();
        let index = samples as usize;
        track
            .surface_point(index, lateral)
            .lerp(track.surface_point(index + 1, lateral), samples.fract())
    };
    let height = flyby_height(&circuits.get(*circuit).scenery, *travelled, length);
    let position = at(*travelled, FLYBY_SIDE) + Vec3::Y * height;
    let target = at(*travelled + FLYBY_AHEAD, 0.0) + Vec3::Y * FLYBY_LOOK_HEIGHT;
    let wanted = Transform::from_translation(position).looking_at(target, Vec3::Y);
    let (mut transform, mut projection) = camera.into_inner();
    glide.apply(&mut transform, &mut projection, wanted, FLYBY_FOV, dt);
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;
    use crate::world::road::{GANTRY_BEAM, GANTRY_HEIGHT};
    use crate::world::structures::TUNNEL_HEIGHT;

    pub fn the_camera_flies_under_the_rock_through_every_tunnel_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let lap = track.sample_count() as f32 * track.step_length();
            let height = |at: f32| flyby_height(&circuit.scenery, at, lap);
            let by_an_arch = |at: f32| {
                circuit.scenery.gantries.iter().any(|gantry| {
                    let off = (at - gantry.along).rem_euclid(lap);
                    off.min(lap - off) < FLYBY_ARCH_UNDER + FLYBY_ARCH_FADE
                })
            };
            for tunnel in circuit.scenery.tunnels {
                let length = (tunnel.to - tunnel.from).rem_euclid(lap);
                // All the way through, well under the rock.
                for step in 0..=20 {
                    let at = tunnel.from + length * step as f32 / 20.0;
                    let flying = height(at);
                    assert!(
                        flying < TUNNEL_HEIGHT - 1.5,
                        "{}: {flying} m at {at} m, under rock {TUNNEL_HEIGHT} m",
                        circuit.name
                    );
                }
                // Already down at the mouth, and at its own height well
                // before it and well past it (but by an arch).
                assert!(height(tunnel.from) < FLYBY_TUNNEL_HEIGHT + 1e-3);
                assert!(height(tunnel.to) < FLYBY_TUNNEL_HEIGHT + 1e-3);
                for away in [
                    tunnel.from - 2.0 * FLYBY_TUNNEL_FADE,
                    tunnel.to + 2.0 * FLYBY_TUNNEL_FADE,
                ]
                .into_iter()
                .filter(|&away| !by_an_arch(away))
                {
                    assert!(
                        (height(away) - FLYBY_HEIGHT).abs() < 1e-3,
                        "{}: {} m at {away} m",
                        circuit.name,
                        height(away)
                    );
                }
            }
            // With no tunnel it flies at its own height all the way round,
            // but by the arches.
            if circuit.scenery.tunnels.is_empty() {
                for step in 0..40 {
                    let at = lap * step as f32 / 40.0;
                    if !by_an_arch(at) {
                        assert!((height(at) - FLYBY_HEIGHT).abs() < 1e-3, "{}", circuit.name);
                    }
                }
            }
        }
    }

    #[test]
    fn the_camera_flies_under_the_rock_through_every_tunnel() {
        the_camera_flies_under_the_rock_through_every_tunnel_on(&crate::testing::DEMO);
    }

    pub fn the_camera_flies_under_the_beam_of_every_arch_over_the_road_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let beam = GANTRY_HEIGHT - GANTRY_BEAM;
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let lap = track.sample_count() as f32 * track.step_length();
            let height = |at: f32| flyby_height(&circuit.scenery, at, lap);
            assert!(!circuit.scenery.gantries.is_empty(), "{}", circuit.name);
            for gantry in circuit.scenery.gantries {
                // Well under the beam from a little before the arch to a
                // little past it.
                for step in -4..=4 {
                    let at = gantry.along + FLYBY_ARCH_UNDER * step as f32 / 4.0;
                    let flying = height(at);
                    assert!(
                        flying < beam - 2.0,
                        "{}: {flying} m at {at} m, under a beam {beam} m up",
                        circuit.name
                    );
                }
            }
        }
    }

    #[test]
    fn the_camera_flies_under_the_beam_of_every_arch_over_the_road() {
        the_camera_flies_under_the_beam_of_every_arch_over_the_road_on(&crate::testing::DEMO);
    }

    pub fn the_flight_over_a_track_sets_off_just_before_its_line_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let lap = track.sample_count() as f32 * track.step_length();
            let start = flyby_start(lap);
            assert!(
                (lap - start - FLYBY_RUN_UP).abs() < 1e-3,
                "{}",
                circuit.name
            );
            // Within the look at the start a track not yet ready is shown
            // with, the line ahead.
            let stretch = crate::world::preview_stretch(lap);
            assert!(stretch.contains(start, lap), "{}", circuit.name);
        }
    }

    #[test]
    fn the_flight_over_a_track_sets_off_just_before_its_line() {
        the_flight_over_a_track_sets_off_just_before_its_line_on(&crate::testing::DEMO);
    }

    #[test]
    fn the_welcome_screen_offers_what_was_asked() {
        if WEB {
            assert_eq!(
                items(Page::Main),
                [Item::SinglePlayer, Item::SplitScreen, Item::Options]
            );
        } else {
            assert_eq!(
                items(Page::Main),
                [
                    Item::SinglePlayer,
                    Item::SplitScreen,
                    Item::Options,
                    Item::Quit
                ]
            );
        }
        assert_eq!(
            items(Page::QuitConfirm),
            [Item::CancelQuit, Item::ConfirmQuit]
        );
    }

    #[test]
    fn the_welcome_screen_carries_the_version() {
        // The engine's own version, as no game said its build.
        assert_eq!(crate::version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_menu_is_walked_like_the_design() {
        // 1P, 2P side by side; Options and Exit below.
        let go = |from: (usize, usize), step, side| navigate(from.0, from.1, step, side, 4);
        assert_eq!(go((0, 0), 0, 1), (1, 1));
        assert_eq!(go((1, 1), 0, -1), (0, 0));
        assert_eq!(go((1, 1), 1, 0), (2, 1));
        assert_eq!(go((2, 1), 1, 0), (3, 1));
        assert_eq!(go((3, 1), 1, 0), (1, 1), "back to the card last chosen");
        assert_eq!(go((2, 1), -1, 0), (1, 1));
        assert_eq!(go((0, 0), -1, 0), (3, 0), "up from the top wraps around");
        assert_eq!(
            go((2, 0), 0, 1),
            (2, 0),
            "the banners ignore left and right"
        );
        // On the web there is no Exit.
        assert_eq!(navigate(2, 0, 1, 0, 3), (0, 0));
        assert_eq!(navigate(0, 0, -1, 0, 3), (2, 0));
    }

    #[test]
    fn the_menu_keeps_the_proportions_of_the_design() {
        let cards = BUTTONS[0].size.x + BUTTONS[1].size.x;
        // The cards and the space between them make up the banners' width.
        assert_eq!(cards + 50.0, BUTTONS[2].size.x);
        assert_eq!(BUTTONS[2].size.x, DESIGN_WIDTH);
        // Everything inside its button.
        let inside = |rect: Rect, size: Vec2| {
            rect.min.x >= 0.0 && rect.min.y >= 0.0 && rect.max.x <= size.x && rect.max.y <= size.y
        };
        assert!(inside(HELMET_1P, BUTTONS[0].size));
        assert!(HELMETS_2P.iter().all(|&h| inside(h, BUTTONS[1].size)));
        for (index, (center, height)) in LETTERS.into_iter().enumerate() {
            let rect = Rect::from_center_size(center, Vec2::new(10.0, height));
            assert!(inside(rect, BUTTONS[index].size), "{index}");
        }
    }

    #[test]
    fn the_menu_starts_as_wide_as_the_title() {
        let width = MenuWidth::default().0;
        assert!((400.0..900.0).contains(&width), "{width}");
    }
}
