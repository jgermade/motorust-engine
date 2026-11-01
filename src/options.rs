//! Menú de opciones unificado con pestañas: Juego, Control, Sonido y Gráficos.
//! Accesible tanto desde la pantalla de bienvenida como desde el menú de pausa.

use bevy::prelude::*;
use bevy::ui::{UiGlobalTransform, UiSystems};
use bevy::window::PrimaryWindow;

use crate::bike::curves::{
    CurveKind, CurveShape, KEYBOARD_MODES, KeyboardMode, KeyboardShapes, LEAN_LOOKS, LEARN_LINES,
};
use crate::gamepads::{ActiveController, Gamepads};
use crate::hud::UiFont;
use crate::legend::{
    Glyph, Keys, menu_prompts, section_badges, shortcut_prompts, spawn_legend, spawn_light_prompts,
    spawn_lone_badge,
};
use crate::locale::{LANGUAGES, Language};
use crate::mapping::{
    Held, MapDevice, Mapping, MappingLook, PAD_HEIGHT, Step, spawn_mapping, use_mapping,
};
use crate::pad_art::{PadArt, PadArts, finish_pad_arts};
use crate::pause::{DebugOptions, ReturnRate, ReverseRate, SteerRate, StickCurve, TurnLimit};
use crate::render_scale::{NativeShare, drawn_size};
use crate::settings::{
    Antialiasing, CHOOSE_FULLSCREEN, ControlsTest, DEFAULT_FRAME_LATENCY, DIFFICULTIES,
    FALSE_STARTS, FORCE_PERFORMANCES, FPS_LIMITS, FRAME_LATENCIES, FULLSCREEN_KINDS, FpsLimit,
    GRASS_TILINGS, LIGHTS_HOLDS, RACE_STARTS, RELIEFS, RENDER_SCALES, RESOLUTIONS, RaceStart,
    SHADOW_QUALITIES, START_LIGHTS, ScreenRate, Settings, ShadowQuality, StartedWith, TRACK_VIEWS,
    WEB, cycle,
};
use crate::ui::{
    ActiveDevice, InputDevice, MenuInput, MenuSound, Row, UiIcons, current_active_device,
    text_font, ui_pixels,
};
use crate::world::detail::{Detail, Level, TwoSamples, offered};

/// Height of the panel (px), the same on every tab: a tab with more options
/// than it holds scrolls them (see [`ListScroll`]).
const PANEL_HEIGHT: f32 = 600.0;
/// Room kept at the bottom of the screen for the legend (px).
const LEGEND_SPACE: f32 = 56.0;

/// The list of a tab's options (or a page's), which scrolls when they do
/// not all fit in the panel, above Reset and Back.
#[derive(Component)]
struct OptionsList {
    tab: OptionsTab,
    page: Option<Page>,
}

/// An option's row in the list, at its place.
#[derive(Component)]
struct OptionsRow(usize);

/// The thin bar beside the list when it scrolls, and its thumb.
#[derive(Component)]
struct ScrollBar;
#[derive(Component)]
struct ScrollThumb;

/// How the list of options is scrolled (logical px): how far down, how
/// tall a stretch of it is shown, and how tall it is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ListScroll {
    offset: f32,
    view: f32,
    content: f32,
}

impl ListScroll {
    /// The list `node` as it was laid out.
    fn of(node: &ComputedNode) -> Self {
        let logical = node.inverse_scale_factor;
        Self {
            offset: node.scroll_position.y * logical,
            view: node.size.y * logical,
            content: node.content_size.y * logical,
        }
    }

    /// Scrolled as little as needed to show the stretch `height` tall from
    /// `top` (down the list), and no further than its end.
    fn showing(self, top: f32, height: f32) -> Self {
        let mut offset = self.offset;
        if top + height > offset + self.view {
            offset = top + height - self.view;
        }
        if top < offset {
            offset = top;
        }
        Self {
            offset: offset.clamp(0.0, (self.content - self.view).max(0.0)),
            ..self
        }
    }

    /// The thumb of the bar beside it, if it does not all fit: where it
    /// starts and how tall it is, as shares of the bar.
    fn thumb(self) -> Option<(f32, f32)> {
        (self.content > self.view + 0.5)
            .then(|| (self.offset / self.content, self.view / self.content))
    }
}

/// Where a row laid out in the list is: its top down the list and its
/// height (logical px).
fn row_in(
    (list, list_at): (&ComputedNode, &UiGlobalTransform),
    (row, row_at): (&ComputedNode, &UiGlobalTransform),
) -> (f32, f32) {
    let top = |node: &ComputedNode, at: &UiGlobalTransform| at.translation.y - node.size.y / 2.0;
    let logical = list.inverse_scale_factor;
    (
        (top(row, row_at) - top(list, list_at) + list.scroll_position.y) * logical,
        row.size.y * logical,
    )
}

/// Keeps the chosen option in view, scrolling the list as little as needed
/// (where the panel, built again, could not tell: coming back from a page
/// to its row), and the bar beside it showing how far down it is.
fn follow_the_chosen_option(
    menu: Res<OptionsMenu>,
    mut lists: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<OptionsList>>,
    rows: Query<(&OptionsRow, &ComputedNode, &UiGlobalTransform)>,
    mut bars: Query<&mut Visibility, With<ScrollBar>>,
    mut thumbs: Query<&mut Node, With<ScrollThumb>>,
) {
    let Ok((list, list_at, mut position)) = lists.single_mut() else {
        return;
    };
    let laid_out = ListScroll::of(list);
    let mut scroll = laid_out;
    let chosen = rows
        .iter()
        .find(|(row, ..)| !menu.on_tabs && row.0 == menu.selected);
    if let Some((_, row, row_at)) = chosen {
        let (top, height) = row_in((list, list_at), (row, row_at));
        scroll = scroll.showing(top, height);
    }
    if (scroll.offset - laid_out.offset).abs() > 0.5 {
        position.y = scroll.offset;
    }
    let thumb = scroll.thumb();
    for mut visibility in &mut bars {
        visibility.set_if_neq(if thumb.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    if let Some((top, height)) = thumb {
        for mut node in &mut thumbs {
            let (top, height) = (percent(top * 100.0), percent(height * 100.0));
            if node.top != top || node.height != height {
                node.top = top;
                node.height = height;
            }
        }
    }
}

pub struct OptionsPlugin;

impl Plugin for OptionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OptionsMenu>()
            .init_resource::<PadArts>()
            .init_resource::<ControlsTest>()
            .add_systems(
                Update,
                (
                    use_options_menu
                        .run_if(|menu: Res<OptionsMenu>| menu.is_open)
                        .in_set(OptionsInput),
                    finish_pad_arts,
                    show_options_menu,
                    mark_controls_test,
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                follow_the_chosen_option.after(UiSystems::Layout),
            );
    }
}

/// Whether the controls are being tried out, for the gamepad's shortcuts
/// (see [`ControlsTest`]).
fn mark_controls_test(menu: Res<OptionsMenu>, mut test: ResMut<ControlsTest>) {
    let on = menu.is_open
        && menu
            .mapping
            .as_ref()
            .is_some_and(|mapping| mapping.step == Step::Test);
    if test.on != on {
        *test = ControlsTest { on };
    }
}

/// The options read the keys before the screen under them (the start menu
/// or the pause menu), which skips the frame the options close in: the key
/// that closes them must not act there too. The screen opening them runs
/// after, so the key that opens them does not act on them either.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct OptionsInput;

/// A dónde vuelve el menú de opciones al cerrarse.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum OptionsReturn {
    #[default]
    Welcome,
    Pause,
}

/// Pestañas del menú de opciones.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum OptionsTab {
    #[default]
    Game,
    Control,
    Sound,
    Graphics,
}

const TABS: [OptionsTab; 4] = [
    OptionsTab::Game,
    OptionsTab::Control,
    OptionsTab::Sound,
    OptionsTab::Graphics,
];

impl OptionsTab {
    pub fn name(self, lang: Language) -> &'static str {
        match self {
            OptionsTab::Game => lang.tab_game(),
            OptionsTab::Control => lang.tab_controls(),
            OptionsTab::Sound => lang.tab_sound(),
            OptionsTab::Graphics => lang.tab_graphics(),
        }
    }

    pub fn next(self, step: i32) -> Self {
        cycle(&TABS, self, step)
    }

    /// The colour of each section: its tab, the edge of the panel and the
    /// option chosen in it.
    pub fn accent(self) -> Color {
        match self {
            OptionsTab::Game => Color::srgb(1.0, 0.72, 0.18),
            OptionsTab::Control => Color::srgb(0.22, 0.76, 1.0),
            OptionsTab::Sound => Color::srgb(1.0, 0.38, 0.66),
            OptionsTab::Graphics => Color::srgb(0.36, 0.9, 0.44),
        }
    }
}

/// Estado global del menú de opciones.
#[derive(Resource, Default, Clone, Debug)]
pub struct OptionsMenu {
    pub is_open: bool,
    pub tab: OptionsTab,
    /// Whether the row of sections is chosen, rather than an option of the
    /// section (`selected`): the arrows across then change the section.
    pub on_tabs: bool,
    pub selected: usize,
    /// The view of the controls' mapping, while it is open.
    pub mapping: Option<Mapping>,
    /// A page of a section's of its own, while it is open: how the stick
    /// steers, or the keyboard, or the advanced options (Control), or the
    /// performance HUD or the advanced options (Graphics).
    pub page: Option<Page>,
    /// Which of the keyboard's curves the keyboard's page is on (and their
    /// panel brings out), and, while a custom curve's four numbers are
    /// being edited, which of them.
    pub curve: CurveKind,
    pub point: Option<usize>,
    pub return_to: OptionsReturn,
    pub rescan_msg_until: f32,
}

/// The pages of a section's, each a list of its own in the panel: on
/// Control, how the stick and the keyboard steer and the advanced options
/// (the keyboard's curves); on Graphics, what the performance HUD shows and
/// the advanced options.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Stick,
    Keyboard,
    /// The keyboard's page with its behaviour on Bézier: its curves.
    KeyboardCurves,
    /// The stick's page with its behaviour on Bézier: its curves.
    StickCurves,
    ControlAdvanced,
    PerformanceHud,
    Advanced,
}

impl Page {
    /// Where its row is in its section's options, which opens it.
    fn row(self) -> usize {
        let at = |items: Vec<_>, row| items.iter().position(|&item| item == row);
        match self {
            Page::Stick | Page::StickCurves => at(
                control_items(None, &KeyboardShapes::default()),
                ControlItem::StickBehaviour,
            ),
            Page::Keyboard | Page::KeyboardCurves => at(
                control_items(None, &KeyboardShapes::default()),
                ControlItem::KeyboardBehaviour,
            ),
            Page::ControlAdvanced => at(
                control_items(None, &KeyboardShapes::default()),
                ControlItem::Advanced,
            ),
            Page::PerformanceHud => graphics_items(None)
                .iter()
                .position(|&item| item == GraphicsItem::PerformanceHud),
            Page::Advanced => graphics_items(None)
                .iter()
                .position(|&item| item == GraphicsItem::Advanced),
        }
        .unwrap_or(0)
    }

    /// As the panel's title says it, after "OPCIONES >".
    fn title(self, lang: Language) -> &'static str {
        match (self, lang) {
            (Page::Stick | Page::StickCurves, _) => "STICK",
            (Page::Keyboard | Page::KeyboardCurves, Language::Spanish) => "TECLADO",
            (Page::Keyboard | Page::KeyboardCurves, Language::English) => "KEYBOARD",
            (Page::Advanced | Page::ControlAdvanced, Language::Spanish) => "AVANZADO",
            (Page::Advanced | Page::ControlAdvanced, Language::English) => "ADVANCED",
            (Page::PerformanceHud, Language::Spanish) => "HUD DE RENDIMIENTO",
            (Page::PerformanceHud, Language::English) => "PERFORMANCE HUD",
        }
    }
}

impl OptionsMenu {
    pub fn open(&mut self, return_to: OptionsReturn) {
        self.is_open = true;
        self.on_tabs = false;
        self.selected = 0;
        self.mapping = None;
        self.page = None;
        self.return_to = return_to;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.mapping = None;
        self.page = None;
        self.point = None;
    }

    /// Back from a page to its section, on the page's row.
    fn leave_page(&mut self, page: Page) {
        self.page = None;
        self.point = None;
        self.selected = page.row();
    }

    /// Moves round the menu: up and down (`step`) through the row of
    /// sections, the options of the section and its foot (Reset and Back,
    /// side by side: down to it, Back), round and round; across (`side`) on
    /// the row of sections changes the section, and confirming there goes
    /// down to its first option; across on the foot, its other button.
    /// `count` is how many options the section has, the foot's included.
    /// Whether it did anything.
    fn navigate(&mut self, step: i32, side: i32, confirm: bool, count: usize) -> bool {
        if self.on_tabs {
            if side != 0 {
                self.tab = self.tab.next(side);
                self.selected = 0;
                return true;
            }
            if step != 0 || confirm {
                self.on_tabs = false;
                self.selected = if step < 0 { count - 1 } else { 0 };
                return true;
            }
            return false;
        }
        let foot = count - FOOTER;
        let on_foot = self.selected >= foot;
        if step == 0 {
            if on_foot && side != 0 {
                self.selected = if self.selected == foot {
                    foot + 1
                } else {
                    foot
                };
                return true;
            }
            return false;
        }
        let next = if on_foot && step < 0 {
            foot as i32 - 1
        } else if on_foot {
            count as i32
        } else if self.selected as i32 + step == foot as i32 {
            // Onto the foot: Back.
            count as i32 - 1
        } else {
            self.selected as i32 + step
        };
        if next < 0 || next >= count as i32 {
            self.on_tabs = true;
        } else {
            self.selected = next as usize;
        }
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GameItem {
    Language,
    Difficulty,
    /// How the track is seen while it is chosen.
    TrackView,
    RaceStart,
    /// How many red lights the start has.
    StartLights,
    /// How long they stay on once all are lit.
    LightsHold,
    /// How many false starts are let off, with the bikes free.
    FalseStarts,
    /// Whether the results go to the middle once everybody is in.
    CentreResults,
    /// Whether the race's whole time is shown under the lap's.
    TotalTime,
    /// Whether the bikes change gears.
    Gears,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ControlItem {
    /// The view where the keys and buttons are remapped.
    Mapping,
    /// The pages of how the stick and the keyboard steer.
    StickBehaviour,
    KeyboardBehaviour,
    StickTurnLimit,
    /// How much the bike turns for how far the stick is pushed.
    StickCurve,
    KeyboardTurnLimit,
    KeyboardTurnRate,
    KeyboardReturnRate,
    /// How fast the keyboard turns over to the other side.
    KeyboardReverseRate,
    /// The page of the advanced options: how the keyboard behaves.
    Advanced,
    /// Classic or by the curves, below (on the keyboard's page then), and
    /// the same for the stick.
    KeyboardMode,
    StickMode,
    /// A curve's shape; under it, when custom, its four numbers on one
    /// line, and its time (or for D the lean at top speed).
    CurveShape(CurveKind),
    CurveCustom(CurveKind),
    CurveScale(CurveKind),
    /// How the lean shown follows the lean felt.
    LeanLook,
    /// Whether the game learns every curve from how the player rides.
    Learning,
    ShowLegend,
    RescanGamepads,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SoundItem {
    FxVolume,
    MusicVolume,
    MusicEnabled,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GraphicsItem {
    Fullscreen,
    /// The kind of fullscreen, where it can be chosen (see
    /// [`CHOOSE_FULLSCREEN`]).
    FullscreenKind,
    Resolution,
    /// How many frames a second at most.
    Fps,
    /// Whether the shadows and the render scale follow the frame rate, and
    /// the rate they keep.
    ForcePerformance,
    /// How much of the window's resolution the world is drawn at.
    RenderScale,
    /// How far and fine the shadows are, or automatic.
    Shadows,
    /// How the edges are smoothed.
    Antialiasing,
    /// The relief of the scenery's textures.
    Relief,
    /// Whether the road has relief too.
    RoadRelief,
    /// Whether the relief is dropped at speed.
    FlatAtSpeed,
    /// How the lawn's repetition is broken up.
    GrassTiling,
    /// The sea mirroring, or the cartoon one.
    Water,
    /// The page of what the performance HUD shows: the frame rate, the
    /// pixels the world is drawn at and where the player is on the map.
    PerformanceHud,
    ShowFps,
    ShowResolution,
    ShowAntialiasing,
    ShowTextures,
    ShowCoordinates,
    /// The keyboard's curves under it (see [`crate::curves_hud`]).
    ShowKeyboardCurves,
    /// The page of the advanced options, those below.
    Advanced,
    /// The trees modelled or the classic ones.
    Trees,
    /// Whether the trees far away are drawn with lighter meshes.
    FarTrees,
    /// Whether the world is drawn lighter with the screen split.
    SplitDetail,
    /// Whether the frame rate is written to a file (see
    /// [`crate::perf_log`]).
    PerformanceLog,
    PipelinedRendering,
    FrameLatency,
}

fn game_items() -> Vec<GameItem> {
    vec![
        GameItem::Language,
        GameItem::Difficulty,
        GameItem::TrackView,
        GameItem::RaceStart,
        GameItem::StartLights,
        GameItem::LightsHold,
        GameItem::FalseStarts,
        GameItem::CentreResults,
        GameItem::TotalTime,
        GameItem::Gears,
    ]
}

/// Room above a row that begins a group of its own (px).
const GROUP_GAP: f32 = 14.0;

/// Whether the row at `index` of `tab` (on `page` of its, if one is open)
/// begins a group of its own, with room above: on Control, the steering
/// after the mapping, the legend after the steering, and looking for the
/// gamepads after the legend and its shortcuts; on Graphics, the
/// performance forced with what it chooses under it, and the pages.
fn starts_group(
    tab: OptionsTab,
    page: Option<Page>,
    index: usize,
    shapes: &KeyboardShapes,
) -> bool {
    // On the keyboard's (or the stick's) curves, each curve with its rows,
    // and the lean shown with learning.
    if matches!(page, Some(Page::KeyboardCurves | Page::StickCurves)) {
        return index > 0
            && matches!(
                control_items(page, shapes).get(index),
                Some(ControlItem::CurveShape(_) | ControlItem::LeanLook)
            );
    }
    if page.is_some() {
        return false;
    }
    match tab {
        OptionsTab::Control => matches!(
            control_items(page, shapes).get(index),
            Some(
                ControlItem::StickBehaviour | ControlItem::ShowLegend | ControlItem::RescanGamepads
            )
        ),
        OptionsTab::Graphics => matches!(
            graphics_items(page).get(index),
            Some(GraphicsItem::ForcePerformance | GraphicsItem::PerformanceHud)
        ),
        _ => false,
    }
}

/// Whether the shortcuts for the sound and fullscreen go under the row at
/// `index` of `tab` (and `page`): under Control's legend.
fn shortcuts_after(tab: OptionsTab, page: Option<Page>, index: usize) -> bool {
    tab == OptionsTab::Control
        && control_items(page, &KeyboardShapes::default()).get(index)
            == Some(&ControlItem::ShowLegend)
}

/// Control's options: the mapping of the controls, the pages of how the
/// stick and the keyboard steer and of the advanced options, the legend;
/// or, with a page open, its own (the keyboard's or the stick's curves as
/// `shapes` has them, see [`shapes_of`]: a custom one's numbers only while
/// it is custom).
fn control_items(page: Option<Page>, shapes: &KeyboardShapes) -> Vec<ControlItem> {
    match page {
        // (Graphics' pages are not Control's.)
        None | Some(Page::PerformanceHud | Page::Advanced) => vec![
            ControlItem::Mapping,
            ControlItem::StickBehaviour,
            ControlItem::KeyboardBehaviour,
            ControlItem::Advanced,
            ControlItem::ShowLegend,
            ControlItem::RescanGamepads,
        ],
        Some(Page::Stick) => vec![ControlItem::StickTurnLimit, ControlItem::StickCurve],
        Some(Page::Keyboard) => vec![
            ControlItem::KeyboardTurnLimit,
            ControlItem::KeyboardTurnRate,
            ControlItem::KeyboardReturnRate,
            ControlItem::KeyboardReverseRate,
        ],
        Some(Page::ControlAdvanced) => vec![ControlItem::KeyboardMode, ControlItem::StickMode],
        Some(Page::KeyboardCurves | Page::StickCurves) => {
            let mut items = Vec::new();
            for &kind in shapes.kinds() {
                items.push(ControlItem::CurveShape(kind));
                if shapes.shape(kind) == CurveShape::Custom {
                    items.push(ControlItem::CurveCustom(kind));
                }
                if shapes.scale(kind).is_some() {
                    items.push(ControlItem::CurveScale(kind));
                }
            }
            items.push(ControlItem::LeanLook);
            items.push(ControlItem::Learning);
            items
        }
    }
}

fn sound_items() -> Vec<SoundItem> {
    vec![
        SoundItem::FxVolume,
        SoundItem::MusicVolume,
        SoundItem::MusicEnabled,
    ]
}

/// Graphics' options: the window and the frame rate, the performance
/// forced and, under it, the render scale and the shadows it chooses when
/// on, the edges, the water, and the pages of the performance HUD and of
/// the advanced options. With a page open, its own: what the performance
/// HUD shows; or the trees, the detail for two players, the performance
/// log and how the frames go to the GPU (those three not on the web).
fn graphics_items(page: Option<Page>) -> Vec<GraphicsItem> {
    match page {
        Some(Page::PerformanceHud) => {
            return vec![
                GraphicsItem::ShowFps,
                GraphicsItem::ShowResolution,
                GraphicsItem::ShowAntialiasing,
                GraphicsItem::ShowTextures,
                GraphicsItem::ShowCoordinates,
                GraphicsItem::ShowKeyboardCurves,
            ];
        }
        Some(Page::Advanced) => {
            let mut items = vec![
                GraphicsItem::Trees,
                GraphicsItem::FarTrees,
                GraphicsItem::SplitDetail,
            ];
            if !WEB {
                items.extend([
                    GraphicsItem::PerformanceLog,
                    GraphicsItem::PipelinedRendering,
                    GraphicsItem::FrameLatency,
                ]);
            }
            return items;
        }
        _ => {}
    }
    let mut items = vec![GraphicsItem::Fullscreen];
    if CHOOSE_FULLSCREEN {
        items.push(GraphicsItem::FullscreenKind);
    }
    items.push(GraphicsItem::Resolution);
    // On the web the browser decides when each frame is drawn.
    if !WEB {
        items.push(GraphicsItem::Fps);
    }
    items.extend([
        GraphicsItem::ForcePerformance,
        GraphicsItem::RenderScale,
        GraphicsItem::Shadows,
        GraphicsItem::Antialiasing,
        GraphicsItem::Relief,
        GraphicsItem::RoadRelief,
        GraphicsItem::FlatAtSpeed,
        GraphicsItem::GrassTiling,
        GraphicsItem::Water,
        GraphicsItem::PerformanceHud,
        GraphicsItem::Advanced,
    ]);
    items
}

/// A render scale as it is shown: the share and, when it is not the whole
/// window, the pixels the world is drawn at in `window` ("70 % (2117 ×
/// 1329)"), at most the screen's own (see [`NativeShare`]).
fn render_scale_name(percent: u32, window: Option<&Window>, native: NativeShare) -> String {
    match window {
        Some(window) if window.physical_width() > 0 => {
            let full = window.physical_size();
            let size = drawn_size(full, percent, native);
            if size == full {
                format!("{percent} %")
            } else {
                format!("{percent} % ({} × {})", size.x, size.y)
            }
        }
        _ => format!("{percent} %"),
    }
}

/// How many options `tab` (on `page` of its, if one is open) has, besides
/// Reset and Back.
fn options_count(tab: OptionsTab, page: Option<Page>, shapes: &KeyboardShapes) -> usize {
    match tab {
        OptionsTab::Game => game_items().len(),
        OptionsTab::Control => control_items(page, shapes).len(),
        OptionsTab::Sound => sound_items().len(),
        OptionsTab::Graphics => graphics_items(page).len(),
    }
}

/// At the foot of every section, side by side: Reset, which puts its
/// options back as they were at first, and Back. They are its last two
/// options.
const FOOTER: usize = 2;

/// How many options `tab` (on `page`) has, Reset and Back at its foot
/// included.
fn tab_items_count(tab: OptionsTab, page: Option<Page>, shapes: &KeyboardShapes) -> usize {
    options_count(tab, page, shapes) + FOOTER
}

/// The settings as they are at first.
fn first_settings() -> &'static Settings {
    static FIRST: std::sync::OnceLock<Settings> = std::sync::OnceLock::new();
    FIRST.get_or_init(Settings::default)
}

/// Graphics' advanced options, those of its page.
fn advanced(settings: &Settings) -> (bool, bool, bool, bool, bool, u32) {
    (
        settings.modelled_trees,
        settings.lighter_far_trees,
        settings.lighter_split_screen,
        settings.performance_log,
        settings.pipelined_rendering,
        settings.frame_latency,
    )
}

/// What the performance HUD shows: the options of its page of Graphics'.
fn performance_hud(settings: &Settings) -> (bool, bool, bool, bool, bool, bool) {
    (
        settings.show_fps,
        settings.show_resolution,
        settings.show_antialiasing,
        settings.show_textures,
        settings.show_coordinates,
        settings.show_keyboard_curves,
    )
}

/// The name of number `point` (0 to 3, as `cubic-bezier` lists them) of
/// curve `kind`: which end of the curve it shapes (x1 and y1 its start, x2
/// and y2 its end) and what it measures there.
fn point_name(point: usize, kind: CurveKind, lang: Language) -> String {
    let (across, up) = kind.axes(lang);
    let end = match (point < 2, lang) {
        (true, Language::Spanish) => "Inicio",
        (true, Language::English) => "Start",
        (false, Language::Spanish) => "Final",
        (false, Language::English) => "End",
    };
    let axis = if point.is_multiple_of(2) { across } else { up };
    format!("{end} · {axis}")
}

/// Number `point` (0 to 3, as `cubic-bezier` lists them) of `curve`.
fn point_mut(curve: &mut crate::bezier::Bezier, point: usize) -> &mut f32 {
    match point {
        0 => &mut curve.x1,
        1 => &mut curve.y1,
        2 => &mut curve.x2,
        _ => &mut curve.y2,
    }
}

/// A custom curve's four numbers on one line, the one being edited (if
/// any) in brackets.
fn custom_line(curve: crate::bezier::Bezier, editing: Option<usize>) -> String {
    [curve.x1, curve.y1, curve.x2, curve.y2]
        .iter()
        .enumerate()
        .map(|(point, value)| {
            if editing == Some(point) {
                format!("[{value:.2}]")
            } else {
                format!(" {value:.2} ")
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// The curve a row of the keyboard's curves is about.
fn curve_of(item: ControlItem) -> Option<CurveKind> {
    match item {
        ControlItem::CurveShape(kind)
        | ControlItem::CurveCustom(kind)
        | ControlItem::CurveScale(kind) => Some(kind),
        _ => None,
    }
}

/// The keyboard's page for its behaviour: its rates and limit as always,
/// or its curves in Bézier.
fn keyboard_page(mode: KeyboardMode) -> Page {
    match mode {
        KeyboardMode::Classic => Page::Keyboard,
        KeyboardMode::Bezier => Page::KeyboardCurves,
    }
}

/// The same for the stick: its limit and response as always, or its
/// curves in Bézier.
fn stick_page(mode: KeyboardMode) -> Page {
    match mode {
        KeyboardMode::Classic => Page::Stick,
        KeyboardMode::Bezier => Page::StickCurves,
    }
}

/// The curves `page` is about: the stick's on its page of curves, the
/// keyboard's otherwise.
fn shapes_of(settings: &Settings, page: Option<Page>) -> &KeyboardShapes {
    if page == Some(Page::StickCurves) {
        &settings.stick_shapes
    } else {
        &settings.keyboard_shapes
    }
}

fn shapes_mut(settings: &mut Settings, page: Option<Page>) -> &mut KeyboardShapes {
    if page == Some(Page::StickCurves) {
        &mut settings.stick_shapes
    } else {
        &mut settings.keyboard_shapes
    }
}

/// A coordinate of a control point of the keyboard's curves, `step` steps
/// of 0.05 on, kept within the square and on the hundredths.
fn step_point(value: f32, step: i32) -> f32 {
    ((value + 0.05 * step as f32).clamp(0.0, 1.0) * 100.0).round() / 100.0
}

/// How the keyboard steers: the options of its page of Control's.
fn keyboard(debug: &DebugOptions) -> (TurnLimit, SteerRate, ReturnRate, ReverseRate) {
    (
        debug.keyboard_limit,
        debug.keyboard_rate,
        debug.keyboard_return,
        debug.keyboard_reverse,
    )
}

/// How the stick and the keyboard steer, and whether the legend is shown:
/// the options of Control.
fn steering(
    debug: &DebugOptions,
) -> (
    TurnLimit,
    StickCurve,
    TurnLimit,
    SteerRate,
    ReturnRate,
    ReverseRate,
    bool,
) {
    (
        debug.stick_limit,
        debug.stick_curve,
        debug.keyboard_limit,
        debug.keyboard_rate,
        debug.keyboard_return,
        debug.keyboard_reverse,
        debug.show_legend,
    )
}

/// Whether the options of `tab` are as they are at first (the keys and
/// buttons aside: the mapping of Control resets those).
fn section_is_default(
    tab: OptionsTab,
    page: Option<Page>,
    settings: &Settings,
    debug: &DebugOptions,
) -> bool {
    let first = first_settings();
    let at_first = DebugOptions::default();
    match (tab, page) {
        (OptionsTab::Control, Some(Page::Stick)) => {
            return (debug.stick_limit, debug.stick_curve)
                == (at_first.stick_limit, at_first.stick_curve);
        }
        (OptionsTab::Control, Some(Page::Keyboard)) => {
            return keyboard(debug) == keyboard(&at_first);
        }
        (OptionsTab::Control, Some(Page::KeyboardCurves)) => {
            return settings.keyboard_shapes == first.keyboard_shapes;
        }
        (OptionsTab::Control, Some(Page::StickCurves)) => {
            return settings.stick_shapes == first.stick_shapes;
        }
        (OptionsTab::Control, Some(Page::ControlAdvanced)) => {
            return (settings.keyboard_mode, settings.stick_mode)
                == (first.keyboard_mode, first.stick_mode);
        }
        (OptionsTab::Graphics, Some(Page::Advanced)) => {
            return advanced(settings) == advanced(first);
        }
        (OptionsTab::Graphics, Some(Page::PerformanceHud)) => {
            return performance_hud(settings) == performance_hud(first);
        }
        _ => {}
    }
    match tab {
        OptionsTab::Game => {
            (
                settings.language,
                settings.difficulty,
                settings.track_view,
                settings.start,
                settings.start_lights,
                settings.lights_hold,
                settings.false_starts,
                settings.centre_results,
                settings.show_total_time,
                settings.gears,
            ) == (
                first.language,
                first.difficulty,
                first.track_view,
                first.start,
                first.start_lights,
                first.lights_hold,
                first.false_starts,
                first.centre_results,
                first.show_total_time,
                first.gears,
            )
        }
        OptionsTab::Control => {
            steering(debug) == steering(&DebugOptions::default())
                && settings.keyboard_mode == first.keyboard_mode
                && settings.keyboard_shapes == first.keyboard_shapes
                && settings.stick_mode == first.stick_mode
                && settings.stick_shapes == first.stick_shapes
        }
        OptionsTab::Sound => {
            (
                settings.fx_volume,
                settings.music_volume,
                settings.music_enabled,
            ) == (first.fx_volume, first.music_volume, first.music_enabled)
        }
        // Its pages' options too.
        OptionsTab::Graphics => {
            let window = |settings: &Settings| {
                (
                    settings.fullscreen,
                    settings.fullscreen_kind,
                    settings.resolution,
                    settings.fps,
                    settings.force_performance,
                    settings.render_scale,
                    settings.shadows,
                    settings.antialiasing,
                    (
                        settings.relief,
                        settings.road_relief,
                        settings.flat_at_speed,
                        settings.grass_tiling,
                    ),
                    settings.cartoon_water,
                )
            };
            window(settings) == window(first)
                && performance_hud(settings) == performance_hud(first)
                && advanced(settings) == advanced(first)
        }
    }
}

/// Puts the options of `tab` back as they are at first (see
/// [`section_is_default`]).
fn reset_section(
    tab: OptionsTab,
    page: Option<Page>,
    settings: &mut Settings,
    debug: &mut DebugOptions,
) {
    let first = first_settings();
    let at_first = DebugOptions::default();
    match (tab, page) {
        (OptionsTab::Control, Some(Page::Stick)) => {
            debug.stick_limit = at_first.stick_limit;
            debug.stick_curve = at_first.stick_curve;
            return;
        }
        (OptionsTab::Control, Some(Page::Keyboard)) => {
            debug.keyboard_limit = at_first.keyboard_limit;
            debug.keyboard_rate = at_first.keyboard_rate;
            debug.keyboard_return = at_first.keyboard_return;
            debug.keyboard_reverse = at_first.keyboard_reverse;
            return;
        }
        (OptionsTab::Control, Some(Page::KeyboardCurves)) => {
            settings.keyboard_shapes = first.keyboard_shapes;
            return;
        }
        (OptionsTab::Control, Some(Page::StickCurves)) => {
            settings.stick_shapes = first.stick_shapes;
            return;
        }
        (OptionsTab::Control, Some(Page::ControlAdvanced)) => {
            settings.keyboard_mode = first.keyboard_mode;
            settings.stick_mode = first.stick_mode;
            return;
        }
        (OptionsTab::Graphics, Some(Page::Advanced)) => {
            reset_advanced(settings, first);
            return;
        }
        (OptionsTab::Graphics, Some(Page::PerformanceHud)) => {
            reset_performance_hud(settings, first);
            return;
        }
        _ => {}
    }
    match tab {
        OptionsTab::Game => {
            settings.language = first.language;
            settings.difficulty = first.difficulty;
            settings.track_view = first.track_view;
            settings.start = first.start;
            settings.start_lights = first.start_lights;
            settings.lights_hold = first.lights_hold;
            settings.false_starts = first.false_starts;
            settings.centre_results = first.centre_results;
            settings.show_total_time = first.show_total_time;
            settings.gears = first.gears;
        }
        OptionsTab::Control => {
            *debug = DebugOptions::default();
            settings.keyboard_mode = first.keyboard_mode;
            settings.keyboard_shapes = first.keyboard_shapes;
            settings.stick_mode = first.stick_mode;
            settings.stick_shapes = first.stick_shapes;
        }
        OptionsTab::Sound => {
            settings.fx_volume = first.fx_volume;
            settings.music_volume = first.music_volume;
            settings.music_enabled = first.music_enabled;
        }
        OptionsTab::Graphics => {
            settings.fullscreen = first.fullscreen;
            settings.fullscreen_kind = first.fullscreen_kind;
            settings.resolution = first.resolution;
            settings.fps = first.fps;
            settings.force_performance = first.force_performance;
            settings.render_scale = first.render_scale;
            settings.shadows = first.shadows;
            settings.antialiasing = first.antialiasing;
            settings.relief = first.relief;
            settings.road_relief = first.road_relief;
            settings.flat_at_speed = first.flat_at_speed;
            settings.grass_tiling = first.grass_tiling;
            settings.cartoon_water = first.cartoon_water;
            reset_performance_hud(settings, first);
            reset_advanced(settings, first);
        }
    }
}

/// The advanced options of Graphics' put back as they are at first.
fn reset_advanced(settings: &mut Settings, first: &Settings) {
    settings.modelled_trees = first.modelled_trees;
    settings.lighter_far_trees = first.lighter_far_trees;
    settings.lighter_split_screen = first.lighter_split_screen;
    settings.performance_log = first.performance_log;
    settings.pipelined_rendering = first.pipelined_rendering;
    settings.frame_latency = first.frame_latency;
}

/// What the performance HUD shows put back as it is at first.
fn reset_performance_hud(settings: &mut Settings, first: &Settings) {
    settings.show_fps = first.show_fps;
    settings.show_resolution = first.show_resolution;
    settings.show_antialiasing = first.show_antialiasing;
    settings.show_textures = first.show_textures;
    settings.show_coordinates = first.show_coordinates;
    settings.show_keyboard_curves = first.show_keyboard_curves;
}

#[allow(clippy::too_many_arguments)]
fn use_options_menu(
    mut input: MenuInput,
    time: Res<Time<Real>>,
    mut options_menu: ResMut<OptionsMenu>,
    mut settings: ResMut<Settings>,
    mut debug_options: ResMut<DebugOptions>,
    mut gamepads: ResMut<Gamepads>,
    device: Res<InputDevice>,
    active_controller: Option<Res<ActiveController>>,
    two_samples: Option<Res<TwoSamples>>,
    mut sounds: MessageWriter<MenuSound>,
) {
    let request = input.read();
    let now = time.elapsed_secs();

    // The mapping of the controls, while it is open: it follows the device
    // in use.
    if let Some(mapping) = options_menu.mapping.as_mut() {
        if device.is_changed() && !mapping.capturing() {
            mapping.device =
                MapDevice::of(current_active_device(*device, active_controller.as_deref()));
        }
        let held = Held {
            keyboard: &input.keyboard,
            gamepads: input.gamepads.iter().collect(),
        };
        if use_mapping(
            mapping,
            &request,
            &held,
            &mut settings.controls,
            now,
            &mut sounds,
        ) {
            options_menu.mapping = None;
        }
        return;
    }

    // A custom curve's four numbers, while they are edited: across, from
    // one to another; up and down, the number, by 0.05; confirming or going
    // back, done.
    if let Some(point) = options_menu.point {
        if request.back || request.confirm {
            options_menu.point = None;
            sounds.write(MenuSound::Move);
        } else if request.side != 0 {
            options_menu.point = Some((point as i32 + request.side).clamp(0, 3) as usize);
            sounds.write(MenuSound::Move);
        } else if request.step != 0 {
            let shapes = shapes_mut(&mut settings, options_menu.page);
            let curve = &mut shapes.choice_mut(options_menu.curve).custom;
            let value = point_mut(curve, point);
            *value = step_point(*value, -request.step);
            sounds.write(MenuSound::Move);
        }
        return;
    }

    // Salir / Volver con Esc o B: from a page of Control's, back to
    // Control.
    let page = options_menu.page;
    if request.back {
        match page {
            Some(page) => options_menu.leave_page(page),
            None => options_menu.close(),
        }
        sounds.write(MenuSound::Move);
        return;
    }

    // Change of section: Tab (Shift + Tab goes back), Q and E, or the
    // shoulder buttons and triggers. A page of Control's closes.
    if request.tab != 0 {
        options_menu.tab = options_menu.tab.next(request.tab);
        options_menu.selected = 0;
        options_menu.page = None;
        sounds.write(MenuSound::Move);
        return;
    }

    // Up and down through the row of sections and the options; across on
    // the row of sections, another section.
    let count = tab_items_count(options_menu.tab, page, shapes_of(&settings, page));
    let on_tabs = options_menu.on_tabs;
    if options_menu.navigate(request.step, request.side, request.confirm, count) {
        sounds.write(MenuSound::Move);
        // Up to the row of sections from a page of Control's: Control.
        if options_menu.on_tabs && page.is_some() {
            options_menu.page = None;
        }
        // On the keyboard's curves, the curve of the row chosen.
        if options_menu.tab == OptionsTab::Control && !options_menu.on_tabs {
            let items = control_items(options_menu.page, shapes_of(&settings, options_menu.page));
            if let Some(kind) = items.get(options_menu.selected).copied().and_then(curve_of) {
                options_menu.curve = kind;
            }
        }
    }
    if on_tabs || request.step != 0 {
        return;
    }

    // Reset and Back, at the foot of every section.
    let foot = count - FOOTER;
    if options_menu.selected >= foot {
        if request.confirm && options_menu.selected == foot {
            reset_section(options_menu.tab, page, &mut settings, &mut debug_options);
            sounds.write(MenuSound::Confirm);
        } else if request.confirm {
            match page {
                Some(page) => options_menu.leave_page(page),
                None => options_menu.close(),
            }
            sounds.write(MenuSound::Move);
        }
        return;
    }

    let change = if request.side != 0 {
        request.side
    } else {
        i32::from(request.confirm)
    };

    match options_menu.tab {
        OptionsTab::Game => {
            let items = game_items();
            let item = items[options_menu.selected.min(items.len() - 1)];
            match item {
                GameItem::Language if change != 0 => {
                    settings.language = cycle(&LANGUAGES, settings.language, change);
                    sounds.write(MenuSound::Move);
                }
                GameItem::Difficulty if change != 0 => {
                    settings.difficulty = cycle(&DIFFICULTIES, settings.difficulty, change);
                    sounds.write(MenuSound::Move);
                }
                GameItem::TrackView if change != 0 => {
                    settings.track_view = cycle(&TRACK_VIEWS, settings.track_view, change);
                    sounds.write(MenuSound::Move);
                }
                GameItem::RaceStart if change != 0 => {
                    settings.start = cycle(&RACE_STARTS, settings.start, change);
                    sounds.write(MenuSound::Move);
                }
                GameItem::StartLights if change != 0 => {
                    settings.start_lights = cycle(&START_LIGHTS, settings.start_lights, change);
                    sounds.write(MenuSound::Move);
                }
                GameItem::LightsHold if change != 0 => {
                    settings.lights_hold = cycle(&LIGHTS_HOLDS, settings.lights_hold, change);
                    sounds.write(MenuSound::Move);
                }
                GameItem::FalseStarts if change != 0 => {
                    settings.false_starts = cycle(&FALSE_STARTS, settings.false_starts, change);
                    sounds.write(MenuSound::Move);
                }
                GameItem::CentreResults if change != 0 => {
                    settings.centre_results = !settings.centre_results;
                    sounds.write(MenuSound::Move);
                }
                GameItem::TotalTime if change != 0 => {
                    settings.show_total_time = !settings.show_total_time;
                    sounds.write(MenuSound::Move);
                }
                GameItem::Gears if change != 0 => {
                    settings.gears = !settings.gears;
                    sounds.write(MenuSound::Move);
                }
                _ => {}
            }
        }
        OptionsTab::Control => {
            let items = control_items(page, shapes_of(&settings, page));
            let item = items[options_menu.selected.min(items.len() - 1)];
            match item {
                ControlItem::Mapping if request.confirm => {
                    let device = current_active_device(*device, active_controller.as_deref());
                    options_menu.mapping = Some(Mapping::open(MapDevice::of(device)));
                    sounds.write(MenuSound::Confirm);
                }
                // A page of its own, from its first option.
                ControlItem::StickBehaviour
                | ControlItem::KeyboardBehaviour
                | ControlItem::Advanced
                    if request.confirm || request.side > 0 =>
                {
                    options_menu.page = Some(match item {
                        ControlItem::StickBehaviour => stick_page(settings.stick_mode),
                        ControlItem::KeyboardBehaviour => keyboard_page(settings.keyboard_mode),
                        _ => Page::ControlAdvanced,
                    });
                    options_menu.selected = 0;
                    sounds.write(MenuSound::Confirm);
                }
                ControlItem::KeyboardMode if change != 0 => {
                    settings.keyboard_mode = cycle(&KEYBOARD_MODES, settings.keyboard_mode, change);
                    sounds.write(MenuSound::Move);
                }
                ControlItem::StickMode if change != 0 => {
                    settings.stick_mode = cycle(&KEYBOARD_MODES, settings.stick_mode, change);
                    sounds.write(MenuSound::Move);
                }
                // While the game learns them, the curves are its own.
                ControlItem::CurveShape(kind)
                    if change != 0 && !shapes_of(&settings, page).learning() =>
                {
                    shapes_mut(&mut settings, page).step_shape(kind, change);
                    sounds.write(MenuSound::Move);
                }
                // Into its four numbers, from the first (or across from
                // the last).
                ControlItem::CurveCustom(_) if change != 0 => {
                    options_menu.point = Some(if request.side < 0 { 3 } else { 0 });
                    sounds.write(MenuSound::Confirm);
                }
                // Learnt while the game learns them.
                ControlItem::CurveScale(kind)
                    if change != 0 && !shapes_of(&settings, page).learning() =>
                {
                    shapes_mut(&mut settings, page).step_scale(kind, change);
                    sounds.write(MenuSound::Move);
                }
                ControlItem::LeanLook if change != 0 => {
                    let shapes = shapes_mut(&mut settings, page);
                    shapes.look = cycle(&LEAN_LOOKS, shapes.look, change);
                    sounds.write(MenuSound::Move);
                }
                ControlItem::Learning if change != 0 => {
                    let shapes = shapes_mut(&mut settings, page);
                    shapes.set_learn(cycle(&LEARN_LINES, shapes.learn, change));
                    sounds.write(MenuSound::Move);
                }
                ControlItem::StickTurnLimit if change != 0 => {
                    debug_options.stick_limit = debug_options.stick_limit.next(change);
                    sounds.write(MenuSound::Move);
                }
                ControlItem::StickCurve if change != 0 => {
                    debug_options.stick_curve = debug_options.stick_curve.next(change);
                    sounds.write(MenuSound::Move);
                }
                ControlItem::KeyboardTurnLimit if change != 0 => {
                    debug_options.keyboard_limit = debug_options.keyboard_limit.next(change);
                    sounds.write(MenuSound::Move);
                }
                ControlItem::KeyboardTurnRate if change != 0 => {
                    debug_options.keyboard_rate = debug_options.keyboard_rate.next(change);
                    sounds.write(MenuSound::Move);
                }
                ControlItem::KeyboardReturnRate if change != 0 => {
                    debug_options.keyboard_return = debug_options.keyboard_return.next(change);
                    sounds.write(MenuSound::Move);
                }
                ControlItem::KeyboardReverseRate if change != 0 => {
                    debug_options.keyboard_reverse = debug_options.keyboard_reverse.next(change);
                    sounds.write(MenuSound::Move);
                }
                ControlItem::ShowLegend if change != 0 => {
                    debug_options.show_legend = !debug_options.show_legend;
                    sounds.write(MenuSound::Move);
                }
                ControlItem::RescanGamepads if request.confirm => {
                    gamepads.search_again();
                    options_menu.rescan_msg_until = now + 3.0;
                    sounds.write(MenuSound::Confirm);
                }
                _ => {}
            }
        }
        OptionsTab::Sound => {
            let items = sound_items();
            let item = items[options_menu.selected.min(items.len() - 1)];
            match item {
                SoundItem::FxVolume if request.side != 0 => {
                    let next = (settings.fx_volume as i32 + request.side * 10).clamp(0, 100);
                    settings.fx_volume = next as u32;
                    sounds.write(MenuSound::Move);
                }
                SoundItem::MusicVolume if request.side != 0 => {
                    let next = (settings.music_volume as i32 + request.side * 10).clamp(0, 100);
                    settings.music_volume = next as u32;
                    sounds.write(MenuSound::Move);
                }
                SoundItem::MusicEnabled if change != 0 => {
                    settings.music_enabled = !settings.music_enabled;
                    sounds.write(MenuSound::Move);
                }
                _ => {}
            }
        }
        OptionsTab::Graphics => {
            let items = graphics_items(page);
            let item = items[options_menu.selected.min(items.len() - 1)];
            match item {
                // A page of its own, from its first option.
                GraphicsItem::PerformanceHud | GraphicsItem::Advanced
                    if request.confirm || request.side > 0 =>
                {
                    options_menu.page = Some(if item == GraphicsItem::PerformanceHud {
                        Page::PerformanceHud
                    } else {
                        Page::Advanced
                    });
                    options_menu.selected = 0;
                    sounds.write(MenuSound::Confirm);
                }
                GraphicsItem::Fullscreen if change != 0 => {
                    settings.fullscreen = !settings.fullscreen;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::FullscreenKind if change != 0 => {
                    settings.fullscreen_kind =
                        cycle(&FULLSCREEN_KINDS, settings.fullscreen_kind, change);
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::Resolution if change != 0 => {
                    if !WEB {
                        settings.resolution = cycle(RESOLUTIONS, settings.resolution, change);
                        sounds.write(MenuSound::Move);
                    }
                }
                // While the performance forced chooses them, they stay.
                GraphicsItem::RenderScale if change != 0 => {
                    settings.render_scale = cycle(&RENDER_SCALES, settings.render_scale, change);
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::Fps if change != 0 => {
                    settings.fps = cycle(&FPS_LIMITS, settings.fps, change);
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::ForcePerformance if change != 0 => {
                    settings.force_performance =
                        cycle(&FORCE_PERFORMANCES, settings.force_performance, change);
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::Shadows if change != 0 => {
                    settings.shadows = cycle(&SHADOW_QUALITIES, settings.shadows, change);
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::Antialiasing if change != 0 => {
                    let offered = offered(TwoSamples::of(two_samples.as_deref()));
                    settings.antialiasing = cycle(&offered, settings.antialiasing, change);
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::Relief if change != 0 => {
                    settings.relief = cycle(&RELIEFS, settings.relief, change);
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::RoadRelief if change != 0 => {
                    settings.road_relief = !settings.road_relief;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::FlatAtSpeed if change != 0 => {
                    settings.flat_at_speed = !settings.flat_at_speed;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::GrassTiling if change != 0 => {
                    settings.grass_tiling = cycle(&GRASS_TILINGS, settings.grass_tiling, change);
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::Water if change != 0 => {
                    settings.cartoon_water = !settings.cartoon_water;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::ShowFps if change != 0 => {
                    settings.show_fps = !settings.show_fps;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::ShowResolution if change != 0 => {
                    settings.show_resolution = !settings.show_resolution;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::ShowAntialiasing if change != 0 => {
                    settings.show_antialiasing = !settings.show_antialiasing;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::ShowTextures if change != 0 => {
                    settings.show_textures = !settings.show_textures;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::ShowCoordinates if change != 0 => {
                    settings.show_coordinates = !settings.show_coordinates;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::ShowKeyboardCurves if change != 0 => {
                    settings.show_keyboard_curves = !settings.show_keyboard_curves;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::Trees if change != 0 => {
                    settings.modelled_trees = !settings.modelled_trees;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::FarTrees if change != 0 => {
                    settings.lighter_far_trees = !settings.lighter_far_trees;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::SplitDetail if change != 0 => {
                    settings.lighter_split_screen = !settings.lighter_split_screen;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::PerformanceLog if change != 0 => {
                    settings.performance_log = !settings.performance_log;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::PipelinedRendering if change != 0 => {
                    settings.pipelined_rendering = !settings.pipelined_rendering;
                    sounds.write(MenuSound::Move);
                }
                GraphicsItem::FrameLatency if change != 0 => {
                    settings.frame_latency =
                        cycle(&FRAME_LATENCIES, settings.frame_latency, change);
                    sounds.write(MenuSound::Move);
                }
                _ => {}
            }
        }
    }
}

/// A shadows' level as Options > Graphics names it.
fn level_name(level: Level, lang: Language) -> String {
    match (level, lang) {
        (Level::High, Language::Spanish) => "Altas",
        (Level::High, Language::English) => "High",
        (Level::Medium, Language::Spanish) => "Medias",
        (Level::Medium, Language::English) => "Medium",
        (Level::Low, Language::Spanish) => "Bajas",
        (Level::Low, Language::English) => "Low",
    }
    .to_string()
}

/// The shadows' quality as Options > Graphics names it: automatic, with
/// the level it has come to (`auto`), or the level chosen.
fn shadows_name(quality: ShadowQuality, auto: Level, lang: Language) -> String {
    match Level::of(quality) {
        Some(chosen) => level_name(chosen, lang),
        None => {
            let auto = level_name(auto, lang).to_lowercase();
            match lang {
                Language::Spanish => format!("Automáticas ({auto})"),
                Language::English => format!("Automatic ({auto})"),
            }
        }
    }
}

/// The frame rate cap as Options > Graphics names it: at the screen's rate,
/// with the rate it runs at once known (`screen`, Hz: "Screen (60 Hz)").
fn fps_limit_name(fps: FpsLimit, screen: Option<f32>, lang: Language) -> String {
    match (fps, screen) {
        (FpsLimit::Screen, Some(hz)) => format!("{} ({hz:.0} Hz)", fps.name(lang)),
        _ => fps.name(lang),
    }
}

#[derive(Component)]
struct OptionsModal;

/// Where the options are drawn in the interface's global order: over the
/// race's (5 to 10) and the benchmark's label (30), under the results (90).
const OPTIONS_LAYER: i32 = 50;

/// How far the text of each option is from the sides of its row (px): the
/// keys that change the section stand in line with it.
const ROW_INSET: f32 = 20.0;

/// How thick the edge of the chosen tab is (px). Every tab keeps room for
/// it, so the tabs do not change size when the focus comes and goes.
const TAB_EDGE: f32 = 2.0;

/// A section's tab, in its colour: the one shown (`active`) filled with
/// it, the others faintly. When the row of sections is chosen (`on_tabs`),
/// the one shown is edged in a lighter tone of its colour.
fn spawn_tab(
    tabs: &mut ChildSpawnerCommands,
    font: &UiFont,
    tab: OptionsTab,
    active: bool,
    on_tabs: bool,
    lang: Language,
) {
    let color = tab.accent();
    let fill = if active {
        color
    } else {
        color.with_alpha(0.16)
    };
    // The edge of the others is their fill, so they look as big.
    let edge = if active && on_tabs {
        color.mix(&Color::WHITE, 0.6)
    } else {
        fill
    };
    tabs.spawn((
        Node {
            padding: UiRect::axes(px(18.0 - TAB_EDGE), px(7.0 - TAB_EDGE)),
            border: UiRect::all(px(TAB_EDGE)),
            border_radius: BorderRadius::all(px(10)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(fill),
        BorderColor::all(edge),
    ))
    .with_child((
        Text::new(tab.name(lang)),
        text_font(font, 18.0),
        TextColor(if active {
            INK
        } else {
            color.mix(&Color::WHITE, 0.55)
        }),
    ));
}

/// The ink of an option chosen, on the colour of its section.
const INK: Color = Color::srgb(0.06, 0.05, 0.04);
/// The words of an option the performance forced may lower by itself: the
/// amber of the performance HUD's values it has lowered.
const AUTOMATIC: Color = Color::srgb(1.0, 0.72, 0.2);

/// A button at the foot of a section (see [`FOOTER`]): filled in the
/// section's colour when chosen; its words faint if it has nothing to do.
fn spawn_foot_button(
    foot: &mut ChildSpawnerCommands,
    font: &UiFont,
    row: &Row,
    accent: Color,
    chosen: bool,
) {
    let (fill, ink) = match (chosen, row.disabled) {
        (true, true) => row.chosen_colors(),
        (true, false) => (accent, INK),
        (false, true) => (
            Color::srgba(1.0, 1.0, 1.0, 0.04),
            Color::srgba(1.0, 1.0, 1.0, 0.4),
        ),
        (false, false) => (Color::srgba(1.0, 1.0, 1.0, 0.08), Color::WHITE),
    };
    foot.spawn((
        Node {
            min_width: px(150),
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(20), px(6)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(fill),
        BorderColor::all(accent.with_alpha(0.5)),
    ))
    .with_child((Text::new(&row.label), text_font(font, 19.0), TextColor(ink)));
}

/// Side of the box of an option on or off (px).
const CHECK_SIZE: f32 = 22.0;

/// The colours of the box of an option on or off: its frame, its fill and
/// its tick. On, it is filled with the colour of the section; on the
/// chosen row, which is already in that colour, it goes dark instead.
fn check_colors(on: bool, chosen: bool, disabled: bool, accent: Color) -> (Color, Color, Color) {
    let faint = |color: Color| {
        if disabled {
            color.with_alpha(color.alpha() * 0.4)
        } else {
            color
        }
    };
    let (frame, fill, tick) = match (on, chosen) {
        (true, false) => (accent, accent, INK),
        (true, true) => (INK, INK, accent),
        (false, false) => (
            Color::srgba(1.0, 1.0, 1.0, 0.55),
            Color::srgba(1.0, 1.0, 1.0, 0.06),
            Color::NONE,
        ),
        (false, true) => (INK.with_alpha(0.7), INK.with_alpha(0.08), Color::NONE),
    };
    (faint(frame), faint(fill), faint(tick))
}

/// The box of an option on or off, ticked when it is on.
fn spawn_check(
    parent: &mut ChildSpawnerCommands,
    icons: &UiIcons,
    on: bool,
    chosen: bool,
    disabled: bool,
    accent: Color,
) {
    let (frame, fill, tick) = check_colors(on, chosen, disabled, accent);
    parent
        .spawn((
            Node {
                width: px(CHECK_SIZE),
                height: px(CHECK_SIZE),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(fill),
            BorderColor::all(frame),
        ))
        .with_children(|square| {
            if on {
                square.spawn((
                    ImageNode::new(icons.glyph(Glyph::Check)).with_color(tick),
                    Node {
                        width: px(CHECK_SIZE - 6.0),
                        height: px(CHECK_SIZE - 6.0),
                        ..default()
                    },
                ));
            }
        });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn show_options_menu(
    mut commands: Commands,
    options_menu: Res<OptionsMenu>,
    settings: Res<Settings>,
    debug_options: Res<DebugOptions>,
    device: Res<InputDevice>,
    active_controller: Option<Res<ActiveController>>,
    window: Query<&Window, With<PrimaryWindow>>,
    font: Res<UiFont>,
    icons: Res<UiIcons>,
    shown: Query<Entity, With<OptionsModal>>,
    time: Res<Time<Real>>,
    started: Option<Res<StartedWith>>,
    // The keys and buttons held, and the gamepads' names (for the mapping).
    (keyboard, gamepads, names): (
        Res<ButtonInput<KeyCode>>,
        Query<&Gamepad>,
        Query<&Name, With<Gamepad>>,
    ),
    // The mapping's gamepads, drawn at the interface's scale.
    (mut pad_arts, ui_scale, detail, native, screen): (
        ResMut<PadArts>,
        Option<Res<UiScale>>,
        Option<Res<Detail>>,
        Option<Res<NativeShare>>,
        Option<Res<ScreenRate>>,
    ),
    // The list of options shown until now, as it was laid out.
    (lists, laid_rows): (
        Query<(&OptionsList, &ComputedNode, &UiGlobalTransform)>,
        Query<(&OptionsRow, &ComputedNode, &UiGlobalTransform)>,
    ),
    mut last: Local<Option<String>>,
) {
    let started = started.as_deref();
    if !options_menu.is_open {
        for entity in &shown {
            commands.entity(entity).despawn();
        }
        *last = None;
        return;
    }

    let lang = settings.language;
    let window_inner = window.iter().next();
    let native = native.as_deref().copied().unwrap_or_default();
    let now = time.elapsed_secs();
    // Beside a setting that takes effect when the game starts again.
    let restart = match lang {
        Language::Spanish => "al reiniciar   ",
        Language::English => "on restart   ",
    };

    // Filas para la pestaña activa
    let rows: Vec<Row> = match options_menu.tab {
        OptionsTab::Game => game_items()
            .into_iter()
            .map(|item| match item {
                GameItem::Language => {
                    Row::with_value(lang.language(), format!("<  {}  >", lang.name()))
                }
                GameItem::Difficulty => Row::with_value(
                    match lang {
                        Language::Spanish => "Dificultad",
                        Language::English => "Difficulty",
                    },
                    format!("<  {}  >", settings.difficulty.name(lang)),
                ),
                GameItem::TrackView => Row::with_value(
                    match lang {
                        Language::Spanish => "Vista al elegir pista",
                        Language::English => "View when choosing the track",
                    },
                    format!("<  {}  >", settings.track_view.name(lang)),
                ),
                GameItem::RaceStart => Row::with_value(
                    match lang {
                        Language::Spanish => "Salida",
                        Language::English => "Race start",
                    },
                    format!("<  {}  >", settings.start.name(lang)),
                ),
                // Faint when the race starts at the throttle, with no lights.
                GameItem::StartLights => Row {
                    disabled: !settings.start.at_lights(),
                    ..Row::with_value(
                        match lang {
                            Language::Spanish => "Luces rojas del semáforo",
                            Language::English => "Red start lights",
                        },
                        format!("<  {}  >", settings.start_lights),
                    )
                },
                // Faint, as the lights, when the race starts at the throttle.
                GameItem::LightsHold => Row {
                    disabled: !settings.start.at_lights(),
                    ..Row::with_value(
                        match lang {
                            Language::Spanish => "Espera tras la última luz",
                            Language::English => "Wait after the last light",
                        },
                        format!("<  {}  >", settings.lights_hold.name(lang)),
                    )
                },
                // Faint unless the bikes are free at the lights.
                GameItem::FalseStarts => Row {
                    disabled: settings.start != RaceStart::LightsUnheld,
                    ..Row::with_value(
                        match lang {
                            Language::Spanish => "Salidas nulas permitidas",
                            Language::English => "False starts let off",
                        },
                        format!("<  {}  >", settings.false_starts),
                    )
                },
                GameItem::CentreResults => Row::check(
                    match lang {
                        Language::Spanish => "Centrar resultados al llegar todos",
                        Language::English => "Centre results when all are in",
                    },
                    settings.centre_results,
                ),
                GameItem::TotalTime => Row::check(
                    match lang {
                        Language::Spanish => "Mostrar el tiempo total",
                        Language::English => "Show the total time",
                    },
                    settings.show_total_time,
                ),
                GameItem::Gears => Row::check(
                    match lang {
                        Language::Spanish => "Marchas (cambio automático)",
                        Language::English => "Gears (automatic)",
                    },
                    settings.gears,
                ),
            })
            .collect(),
        OptionsTab::Control => {
            control_items(options_menu.page, shapes_of(&settings, options_menu.page))
                .into_iter()
                .map(|item| match item {
                    ControlItem::StickBehaviour => Row::with_value(
                        match lang {
                            Language::Spanish => "Comportamiento Stick",
                            Language::English => "Stick behaviour",
                        },
                        match settings.stick_mode {
                            KeyboardMode::Classic => ">".to_string(),
                            mode => format!("{}   >", mode.name(lang)),
                        },
                    ),
                    ControlItem::KeyboardBehaviour => Row::with_value(
                        match lang {
                            Language::Spanish => "Comportamiento Teclado",
                            Language::English => "Keyboard behaviour",
                        },
                        match settings.keyboard_mode {
                            KeyboardMode::Classic => ">".to_string(),
                            mode => format!("{}   >", mode.name(lang)),
                        },
                    ),
                    ControlItem::Mapping => Row::with_value(
                        match lang {
                            Language::Spanish => "Asignación",
                            Language::English => "Mapping",
                        },
                        match lang {
                            Language::Spanish => "Teclado y mando   >",
                            Language::English => "Keyboard and gamepad   >",
                        },
                    ),
                    ControlItem::StickTurnLimit => Row::with_value(
                        match lang {
                            Language::Spanish => "Limitar giro con velocidad",
                            Language::English => "Turn limit with speed",
                        },
                        format!("<  {}  >", debug_options.stick_limit.name(lang)),
                    ),
                    ControlItem::StickCurve => Row::with_value(
                        match lang {
                            Language::Spanish => "Respuesta del stick",
                            Language::English => "Stick response",
                        },
                        format!("<  {}  >", debug_options.stick_curve.name(lang)),
                    ),
                    ControlItem::KeyboardTurnLimit => Row::with_value(
                        match lang {
                            Language::Spanish => "Limitar giro con velocidad",
                            Language::English => "Turn limit with speed",
                        },
                        format!("<  {}  >", debug_options.keyboard_limit.name(lang)),
                    ),
                    ControlItem::KeyboardTurnRate => Row::with_value(
                        match lang {
                            Language::Spanish => "Rapidez de giro sostenido",
                            Language::English => "Sustained turn rate",
                        },
                        format!("<  {}  >", debug_options.keyboard_rate.name(lang)),
                    ),
                    ControlItem::KeyboardReturnRate => Row::with_value(
                        match lang {
                            Language::Spanish => "Rapidez al soltar giro",
                            Language::English => "Return rate released",
                        },
                        format!("<  {}  >", debug_options.keyboard_return.name(lang)),
                    ),
                    ControlItem::KeyboardReverseRate => Row::with_value(
                        match lang {
                            Language::Spanish => "Rapidez de cambio de sentido",
                            Language::English => "Turning the other way",
                        },
                        format!("<  {}  >", debug_options.keyboard_reverse.name(lang)),
                    ),
                    ControlItem::Advanced => Row::with_value(
                        match lang {
                            Language::Spanish => "Avanzado",
                            Language::English => "Advanced",
                        },
                        ">",
                    ),
                    ControlItem::KeyboardMode => Row::with_value(
                        match lang {
                            Language::Spanish => "Comportamiento de teclado",
                            Language::English => "Keyboard behaviour",
                        },
                        format!("<  {}  >", settings.keyboard_mode.name(lang)),
                    ),
                    ControlItem::StickMode => Row::with_value(
                        match lang {
                            Language::Spanish => "Comportamiento del stick",
                            Language::English => "Stick behaviour",
                        },
                        format!("<  {}  >", settings.stick_mode.name(lang)),
                    ),
                    // A learnt curve, while the game learns them all, in the
                    // amber of what changes by itself.
                    ControlItem::CurveShape(kind) => {
                        let shapes = shapes_of(&settings, options_menu.page);
                        Row::with_value(
                            format!("{} · {}", kind.letter(), kind.name(lang)),
                            format!("<  {}  >", shapes.shape(kind).name(lang)),
                        )
                        .automatic(shapes.learning())
                    }
                    ControlItem::CurveCustom(kind) => {
                        let editing = options_menu.point.filter(|_| options_menu.curve == kind);
                        Row::with_value(
                            match editing {
                                Some(point) => format!("      {}", point_name(point, kind, lang)),
                                None => "      cubic-bezier".to_string(),
                            },
                            custom_line(
                                shapes_of(&settings, options_menu.page).choice(kind).custom,
                                editing,
                            ),
                        )
                    }
                    ControlItem::CurveScale(kind) => Row::with_value(
                        format!("      {}", kind.scale_name(lang)),
                        format!(
                            "<  {}  >",
                            kind.scale_text(
                                shapes_of(&settings, options_menu.page)
                                    .scale(kind)
                                    .unwrap_or_default(),
                                lang
                            )
                        ),
                    )
                    .automatic(shapes_of(&settings, options_menu.page).learning()),
                    ControlItem::LeanLook => Row::with_value(
                        match lang {
                            Language::Spanish => "Tumbada visible",
                            Language::English => "Lean shown",
                        },
                        format!(
                            "<  {}  >",
                            shapes_of(&settings, options_menu.page).look.name(lang)
                        ),
                    ),
                    ControlItem::Learning => Row::with_value(
                        match lang {
                            Language::Spanish => "Auto-aprendizaje",
                            Language::English => "Self-learning",
                        },
                        format!(
                            "<  {}  >",
                            shapes_of(&settings, options_menu.page).learn.name(lang)
                        ),
                    )
                    .automatic(shapes_of(&settings, options_menu.page).learning()),
                    ControlItem::ShowLegend => {
                        Row::check(lang.show_legend(), debug_options.show_legend)
                    }
                    ControlItem::RescanGamepads => {
                        if now < options_menu.rescan_msg_until {
                            Row::with_value(
                                lang.rescan_gamepads(),
                                match lang {
                                    Language::Spanish => "Mandos actualizados",
                                    Language::English => "Controllers updated",
                                },
                            )
                        } else {
                            Row::new(lang.rescan_gamepads())
                        }
                    }
                })
                .collect()
        }
        OptionsTab::Sound => sound_items()
            .into_iter()
            .map(|item| match item {
                SoundItem::FxVolume => Row::with_value(
                    lang.sound_fx_volume(),
                    format!("<  {}%  >", settings.fx_volume),
                ),
                SoundItem::MusicVolume => Row::with_value(
                    lang.sound_music_volume(),
                    format!("<  {}%  >", settings.music_volume),
                ),
                SoundItem::MusicEnabled => Row::with_value(
                    lang.sound_music(),
                    if settings.music_enabled {
                        format!("<  {}  >", lang.enabled())
                    } else {
                        format!("<  {}  >", lang.disabled())
                    },
                ),
            })
            .collect(),
        OptionsTab::Graphics => {
            // What the performance forced may lower by itself, in amber.
            let lowered = crate::world::detail::lowered(&settings);
            graphics_items(options_menu.page)
                .into_iter()
                .map(|item| match item {
                    GraphicsItem::PerformanceHud => Row::with_value(
                        match lang {
                            Language::Spanish => "HUD de rendimiento",
                            Language::English => "Performance HUD",
                        },
                        ">",
                    ),
                    GraphicsItem::ShowFps => Row::check(
                        match lang {
                            Language::Spanish => "Mostrar FPS",
                            Language::English => "Show FPS",
                        },
                        settings.show_fps,
                    ),
                    GraphicsItem::ShowResolution => Row::check(
                        match lang {
                            Language::Spanish => "Mostrar resolución activa",
                            Language::English => "Show active resolution",
                        },
                        settings.show_resolution,
                    ),
                    GraphicsItem::ShowAntialiasing => Row::check(
                        match lang {
                            Language::Spanish => "Mostrar antialiasing activo",
                            Language::English => "Show active antialiasing",
                        },
                        settings.show_antialiasing,
                    ),
                    GraphicsItem::ShowTextures => Row::check(
                        match lang {
                            Language::Spanish => "Mostrar texturas activas",
                            Language::English => "Show active textures",
                        },
                        settings.show_textures,
                    ),
                    GraphicsItem::ShowCoordinates => Row::check(
                        match lang {
                            Language::Spanish => "Mostrar coordenadas en el mapa",
                            Language::English => "Show coordinates on the map",
                        },
                        settings.show_coordinates,
                    ),
                    GraphicsItem::ShowKeyboardCurves => Row::check(
                        match lang {
                            Language::Spanish => "Mostrar curvas del teclado",
                            Language::English => "Show the keyboard's curves",
                        },
                        settings.show_keyboard_curves,
                    ),
                    GraphicsItem::Advanced => Row::with_value(
                        match lang {
                            Language::Spanish => "Avanzado",
                            Language::English => "Advanced",
                        },
                        ">",
                    ),
                    GraphicsItem::Fullscreen => Row::check(lang.fullscreen(), settings.fullscreen),
                    GraphicsItem::FullscreenKind => Row::with_value(
                        lang.fullscreen_kind(),
                        format!("<  {}  >", settings.fullscreen_kind.name(lang)),
                    ),
                    GraphicsItem::Resolution => {
                        let name = settings.resolution.name(window_inner, lang);
                        if WEB {
                            Row::disabled(lang.resolution(), name)
                        } else {
                            Row::with_value(lang.resolution(), format!("<  {name}  >"))
                        }
                    }
                    GraphicsItem::RenderScale => {
                        let label = match lang {
                            Language::Spanish => "Escala de resolución",
                            Language::English => "Resolution scale",
                        };
                        // With the performance forced, in amber: the most it is,
                        // lowered from there by itself.
                        Row::with_value(
                            label,
                            format!(
                                "<  {}  >",
                                render_scale_name(settings.render_scale, window_inner, native)
                            ),
                        )
                        .automatic(lowered.scale)
                    }
                    GraphicsItem::Fps => Row::with_value(
                        match lang {
                            Language::Spanish => "Límite FPS",
                            Language::English => "FPS limit",
                        },
                        format!(
                            "<  {}  >",
                            fps_limit_name(settings.fps, screen.as_deref().and_then(|s| s.0), lang)
                        ),
                    ),
                    GraphicsItem::Shadows => {
                        let label = match lang {
                            Language::Spanish => "Sombras",
                            Language::English => "Shadows",
                        };
                        let now = detail.as_deref().map_or(Level::High, |detail| detail.level);
                        // With the performance forced, in amber: the most they
                        // are, lowered from there by themselves.
                        Row::with_value(
                            label,
                            format!("<  {}  >", shadows_name(settings.shadows, now, lang)),
                        )
                        .automatic(lowered.shadows)
                    }
                    GraphicsItem::ForcePerformance => Row::with_value(
                        match lang {
                            Language::Spanish => "Forzar rendimiento",
                            Language::English => "Force performance",
                        },
                        format!("<  {}  >", settings.force_performance.name(lang)),
                    ),
                    GraphicsItem::Water => Row::with_value(
                        match lang {
                            Language::Spanish => "Agua",
                            Language::English => "Water",
                        },
                        format!(
                            "<  {}  >",
                            match (settings.cartoon_water, lang) {
                                (true, Language::Spanish) => "Estilo Cartoon",
                                (true, Language::English) => "Cartoon style",
                                (false, Language::Spanish) => "Con reflejo",
                                (false, Language::English) => "Mirroring",
                            }
                        ),
                    ),
                    // With the performance forced and the multisampling chosen,
                    // in amber: it may be given up for FXAA by itself.
                    GraphicsItem::Antialiasing => {
                        let name = match (settings.antialiasing, lang) {
                            (Antialiasing::Msaa, _) => "MSAA 4×",
                            (Antialiasing::Msaa2, _) => "MSAA 2×",
                            (Antialiasing::Smaa, _) => "SMAA",
                            (Antialiasing::Fxaa, _) => "FXAA",
                            (Antialiasing::Off, Language::Spanish) => "No",
                            (Antialiasing::Off, Language::English) => "Off",
                        };
                        Row::with_value("Antialiasing", format!("<  {name}  >"))
                            .automatic(lowered.antialiasing)
                    }
                    GraphicsItem::Relief => Row::with_value(
                        match lang {
                            Language::Spanish => "Relieve",
                            Language::English => "Relief",
                        },
                        // With the performance forced and the parallax chosen,
                        // in amber: it may be given up by itself.
                        format!("<  {}  >", settings.relief.name(lang)),
                    )
                    .automatic(lowered.relief),
                    GraphicsItem::RoadRelief => Row::check(
                        match lang {
                            Language::Spanish => "Relieve en la carretera",
                            Language::English => "Relief on the road",
                        },
                        settings.road_relief,
                    ),
                    GraphicsItem::FlatAtSpeed => Row::check(
                        match lang {
                            Language::Spanish => "Sin relieve a alta velocidad",
                            Language::English => "No relief at high speed",
                        },
                        settings.flat_at_speed,
                    ),
                    GraphicsItem::GrassTiling => Row::with_value(
                        match lang {
                            Language::Spanish => "Repetición de la hierba",
                            Language::English => "Grass tiling",
                        },
                        format!("<  {}  >", settings.grass_tiling.name(lang)),
                    ),
                    GraphicsItem::Trees => Row::with_value(
                        match lang {
                            Language::Spanish => "Árboles",
                            Language::English => "Trees",
                        },
                        format!(
                            "<  {}  >",
                            match (settings.modelled_trees, lang) {
                                (true, Language::Spanish) => "Nuevos",
                                (true, Language::English) => "New",
                                (false, Language::Spanish) => "Clásicos",
                                (false, Language::English) => "Classic",
                            }
                        ),
                    ),
                    GraphicsItem::FarTrees => Row::check(
                        match lang {
                            Language::Spanish => "Árboles lejanos más ligeros",
                            Language::English => "Lighter far trees",
                        },
                        settings.lighter_far_trees,
                    ),
                    GraphicsItem::SplitDetail => Row::check(
                        match lang {
                            Language::Spanish => "Menos detalle a dos jugadores",
                            Language::English => "Less detail for two players",
                        },
                        settings.lighter_split_screen,
                    ),
                    GraphicsItem::PerformanceLog => Row::check(
                        match lang {
                            Language::Spanish => "Registro de rendimiento",
                            Language::English => "Performance log",
                        },
                        settings.performance_log,
                    ),
                    // These two take effect when the game starts again.
                    GraphicsItem::PipelinedRendering => Row {
                        value: started
                            .is_some_and(|s| s.pipelined_rendering != settings.pipelined_rendering)
                            .then(|| restart.trim_end().to_string()),
                        ..Row::check(
                            match lang {
                                Language::Spanish => "Renderizado en paralelo",
                                Language::English => "Pipelined rendering",
                            },
                            settings.pipelined_rendering,
                        )
                    },
                    GraphicsItem::FrameLatency => Row::with_value(
                        match lang {
                            Language::Spanish => "Fotogramas en cola",
                            Language::English => "Frame latency",
                        },
                        format!(
                            "{}<  {}{}  >",
                            if started.is_some_and(|s| s.frame_latency != settings.frame_latency) {
                                restart
                            } else {
                                ""
                            },
                            settings.frame_latency,
                            if settings.frame_latency == DEFAULT_FRAME_LATENCY {
                                " (Bevy)"
                            } else {
                                ""
                            }
                        ),
                    ),
                })
                .collect()
        }
    };

    // The foot of the section: Reset, faint when there is nothing to put
    // back, and Back.
    let footer = [
        Row {
            disabled: section_is_default(
                options_menu.tab,
                options_menu.page,
                &settings,
                &debug_options,
            ),
            ..Row::new(lang.reset())
        },
        Row::new(lang.back()),
    ];

    let active_dev = current_active_device(*device, active_controller.as_deref());
    // The mapping view lights the keys and buttons held down.
    let held = Held {
        keyboard: &keyboard,
        gamepads: gamepads.iter().collect(),
    };
    let mut held_down: Vec<String> = keyboard
        .get_pressed()
        .map(|key| format!("{key:?}"))
        .chain(
            gamepads
                .iter()
                .flat_map(|pad| pad.get_pressed())
                .map(|button| format!("{button:?}")),
        )
        .collect();
    held_down.sort();
    // The gamepad of the mapping, drawn as the one in use: by its name.
    let pad_style = current_active_device(InputDevice::Gamepad, active_controller.as_deref());
    let pad_name = active_controller
        .as_deref()
        .and_then(|controller| controller.gamepad)
        .and_then(|pad| names.get(pad).ok())
        .map(|name| name.as_str().to_string());
    let art = PadArt::of(pad_style, pad_name.as_deref());
    let scale_factor = ui_pixels(window_inner, ui_scale.as_deref());
    // It is drawn in the background from the Control tab on, to be ready
    // by the time it is shown (on the web the background is this same
    // thread: only once it is shown).
    let pad_shown = options_menu
        .mapping
        .as_ref()
        .is_some_and(|mapping| mapping.device == MapDevice::Gamepad);
    let pad_drawn = (pad_shown || (!WEB && options_menu.tab == OptionsTab::Control)).then(|| {
        pad_arts
            .get(
                art,
                pad_style == ActiveDevice::Switch,
                PAD_HEIGHT,
                scale_factor,
            )
            .is_some()
    });
    // Built again only when something shown changes (the window, for
    // example, changes with every move of the mouse; its scale factor
    // changes the gamepad's pixels).
    let wanted = format!(
        "{:?} {art:?} {scale_factor} {pad_drawn:?} {:?} {:?} {held_down:?} {:?} {} {} {:?} {:?} {:?} {}",
        options_menu.mapping,
        settings.controls,
        current_active_device(InputDevice::Gamepad, active_controller.as_deref()),
        options_menu.tab,
        options_menu.on_tabs,
        options_menu.selected,
        lang,
        active_dev,
        debug_options.show_legend,
        rows.iter()
            .chain(&footer)
            .map(|row| format!(
                "{}={:?}{:?}{}{}",
                row.label, row.value, row.check, row.disabled, row.automatic
            ))
            .collect::<Vec<_>>()
            .join("|"),
    );
    if last.as_ref() == Some(&wanted) && !shown.is_empty() {
        return;
    }
    *last = Some(wanted);
    // Scrolled as the list shown until now, if it is the same tab's (and
    // page's), and on as far as needed to show the option chosen now; at
    // the top otherwise.
    let scroll = lists
        .iter()
        .find(|(list, ..)| list.tab == options_menu.tab && list.page == options_menu.page)
        .map_or_else(ListScroll::default, |(_, list, list_at)| {
            let scroll = ListScroll::of(list);
            laid_rows
                .iter()
                .find(|(row, ..)| !options_menu.on_tabs && row.0 == options_menu.selected)
                .map_or(scroll, |(_, row, row_at)| {
                    let (top, height) = row_in((list, list_at), (row, row_at));
                    scroll.showing(top, height)
                })
        });
    for entity in &shown {
        commands.entity(entity).despawn();
    }

    let accent = options_menu.tab.accent();
    // The keys that change the section go beside the tabs instead.
    let prompts = menu_prompts(
        active_dev,
        &[
            (Keys::Confirm, lang.hint_accept()),
            (Keys::Back, lang.back()),
        ],
    );
    let [previous, next] = section_badges(active_dev);
    commands
        .spawn((
            OptionsModal,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                // Centred, above the legend. The panel is as tall on every
                // tab, so the tabs stay where they are.
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::bottom(px(LEGEND_SPACE)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.02, 0.05, 0.78)),
            // Over everything of the race (the start lights, the minimaps,
            // the speedometers, the pause menu it opens from), its legend
            // with it: those are drawn in the global order, over any order
            // that is only local.
            GlobalZIndex(OPTIONS_LAYER),
        ))
        .with_children(|modal| {
            // Panel de Opciones grande
            modal
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Stretch,
                        width: px(780),
                        max_width: percent(94),
                        height: px(PANEL_HEIGHT),
                        max_height: percent(100),
                        overflow: Overflow::clip(),
                        padding: UiRect::axes(px(24), px(20)),
                        border_radius: BorderRadius::all(px(18)),
                        border: UiRect::all(px(1)),
                        row_gap: px(12),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.02, 0.04, 0.1, 0.94)),
                    // Edged in the colour of the section.
                    BorderColor::all(accent.with_alpha(0.55)),
                ))
                .with_children(|panel| {
                    // The mapping of the controls, in place of the sections.
                    if let Some(mapping) = &options_menu.mapping {
                        let drawn = pad_shown
                            .then(|| {
                                pad_arts.get(
                                    art,
                                    pad_style == ActiveDevice::Switch,
                                    PAD_HEIGHT,
                                    scale_factor,
                                )
                            })
                            .flatten();
                        let look = MappingLook {
                            font: &font,
                            icons: &icons,
                            lang,
                            using: active_dev,
                            pad: pad_style,
                            art,
                            drawn,
                            letters: pad_style == ActiveDevice::Switch,
                            controls: &settings.controls,
                            held: &held,
                        };
                        spawn_mapping(panel, mapping, &look);
                        return;
                    }
                    // Título
                    // On a page of Control's, which.
                    let title = match options_menu.page {
                        Some(page) => {
                            format!("{} > {}", lang.options().to_uppercase(), page.title(lang))
                        }
                        None => lang.options().to_uppercase(),
                    };
                    panel.spawn((
                        Text::new(title),
                        text_font(&font, 36.0),
                        TextShadow {
                            offset: Vec2::splat(2.5),
                            color: Color::srgba(0.0, 0.0, 0.0, 0.6),
                        },
                        Node {
                            align_self: AlignSelf::Center,
                            margin: UiRect::bottom(px(4)),
                            ..default()
                        },
                    ));

                    // Barra de pestañas: the keys that change the section
                    // at its ends, in line with the text of the options,
                    // and the sections in the middle.
                    panel
                        .spawn((Node {
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(ROW_INSET)),
                            margin: UiRect::bottom(px(6)),
                            ..default()
                        },))
                        .with_children(|tab_bar| {
                            // The key for the section before, on the left.
                            spawn_lone_badge(tab_bar, &font, &icons, active_dev, &previous);
                            tab_bar
                                .spawn(Node {
                                    flex_direction: FlexDirection::Row,
                                    align_items: AlignItems::Center,
                                    column_gap: px(10),
                                    ..default()
                                })
                                .with_children(|tabs| {
                                    for tab in TABS {
                                        spawn_tab(
                                            tabs,
                                            &font,
                                            tab,
                                            tab == options_menu.tab,
                                            options_menu.on_tabs,
                                            lang,
                                        );
                                    }
                                });
                            // The key for the section after, on the right.
                            spawn_lone_badge(tab_bar, &font, &icons, active_dev, &next);
                        });
                    // A line in the colour of the section under the tabs.
                    panel.spawn((
                        Node {
                            height: px(2),
                            min_height: px(2),
                            flex_shrink: 0.0,
                            margin: UiRect::bottom(px(4)),
                            ..default()
                        },
                        BackgroundColor(accent.with_alpha(0.7)),
                    ));

                    // Lista de opciones de la pestaña. It fills the panel,
                    // scrolling when they do not all fit (the chosen one
                    // kept in view, a thin bar beside it), above Reset and
                    // Back, which stay where they are.
                    let (size, padding, gap) = (20.0, 5.0, 3.0);
                    let thumb = scroll.thumb();
                    panel
                        .spawn(Node {
                            flex_direction: FlexDirection::Column,
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            min_height: px(0),
                            ..default()
                        })
                        .with_children(|frame| {
                            frame
                                .spawn((
                                    OptionsList {
                                        tab: options_menu.tab,
                                        page: options_menu.page,
                                    },
                                    Node {
                                        flex_direction: FlexDirection::Column,
                                        align_items: AlignItems::Stretch,
                                        flex_grow: 1.0,
                                        min_height: px(0),
                                        row_gap: px(gap),
                                        overflow: Overflow::scroll_y(),
                                        ..default()
                                    },
                                    ScrollPosition(Vec2::new(0.0, scroll.offset)),
                                ))
                                .with_children(|items_container| {
                                    for (index, row) in rows.iter().enumerate() {
                                        let chosen =
                                            !options_menu.on_tabs && index == options_menu.selected;
                                        // Chosen, in the colour of the section.
                                        let (chosen_background, chosen_text) =
                                            if row.disabled || row.danger {
                                                row.chosen_colors()
                                            } else {
                                                (accent, INK)
                                            };
                                        let color = TextColor(if chosen {
                                            chosen_text
                                        } else if row.disabled {
                                            Color::srgba(1.0, 1.0, 1.0, 0.4)
                                        } else if row.automatic {
                                            AUTOMATIC
                                        } else {
                                            Color::WHITE
                                        });

                                        items_container
                                            .spawn((
                                                OptionsRow(index),
                                                Node {
                                                    flex_shrink: 0.0,
                                                    justify_content: if row.value.is_some()
                                                        || row.check.is_some()
                                                    {
                                                        JustifyContent::SpaceBetween
                                                    } else {
                                                        JustifyContent::Center
                                                    },
                                                    align_items: AlignItems::Center,
                                                    column_gap: px(30),
                                                    padding: UiRect::axes(
                                                        px(ROW_INSET),
                                                        px(padding),
                                                    ),
                                                    margin: if starts_group(
                                                        options_menu.tab,
                                                        options_menu.page,
                                                        index,
                                                        shapes_of(&settings, options_menu.page),
                                                    ) {
                                                        UiRect::top(px(GROUP_GAP))
                                                    } else {
                                                        UiRect::ZERO
                                                    },
                                                    border_radius: BorderRadius::all(px(8)),
                                                    ..default()
                                                },
                                                BackgroundColor(if chosen {
                                                    chosen_background
                                                } else {
                                                    Color::NONE
                                                }),
                                            ))
                                            .with_children(|line| {
                                                line.spawn((
                                                    Text::new(&row.label),
                                                    text_font(&font, size),
                                                    color,
                                                ));
                                                // The value and the box, together
                                                // on the right.
                                                line.spawn(Node {
                                                    align_items: AlignItems::Center,
                                                    column_gap: px(14),
                                                    ..default()
                                                })
                                                .with_children(|right| {
                                                    if let Some(value) = &row.value {
                                                        right.spawn((
                                                            Text::new(value),
                                                            text_font(&font, size),
                                                            color,
                                                        ));
                                                    }
                                                    if let Some(on) = row.check {
                                                        spawn_check(
                                                            right,
                                                            &icons,
                                                            on,
                                                            chosen,
                                                            row.disabled,
                                                            accent,
                                                        );
                                                    }
                                                });
                                            });
                                        // Under the legend, in a lighter hand: the
                                        // shortcuts for the sound and fullscreen,
                                        // on the device in use.
                                        if shortcuts_after(
                                            options_menu.tab,
                                            options_menu.page,
                                            index,
                                        ) {
                                            items_container
                                                .spawn(Node {
                                                    flex_shrink: 0.0,
                                                    padding: UiRect::axes(px(ROW_INSET), px(2)),
                                                    ..default()
                                                })
                                                .with_children(|note| {
                                                    let prompts =
                                                        shortcut_prompts(active_dev, lang);
                                                    spawn_light_prompts(
                                                        note, &font, &icons, &prompts,
                                                    );
                                                });
                                        }
                                    }
                                });
                            // Beside it, in the panel's margin: the bar, its thumb
                            // as tall as the share of the options shown, as far
                            // down as the list is scrolled.
                            frame
                                .spawn((
                                    ScrollBar,
                                    Node {
                                        position_type: PositionType::Absolute,
                                        top: px(0),
                                        bottom: px(0),
                                        right: px(-14),
                                        width: px(4),
                                        border_radius: BorderRadius::all(px(2)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
                                    if thumb.is_some() {
                                        Visibility::Inherited
                                    } else {
                                        Visibility::Hidden
                                    },
                                ))
                                .with_children(|bar| {
                                    let (top, height) = thumb.unwrap_or((0.0, 1.0));
                                    bar.spawn((
                                        ScrollThumb,
                                        Node {
                                            position_type: PositionType::Absolute,
                                            left: px(0),
                                            right: px(0),
                                            top: percent(top * 100.0),
                                            height: percent(height * 100.0),
                                            border_radius: BorderRadius::all(px(2)),
                                            ..default()
                                        },
                                        BackgroundColor(accent.with_alpha(0.7)),
                                    ));
                                });
                        });
                    // The foot, side by side, where it is on every tab.
                    panel
                        .spawn(Node {
                            justify_content: JustifyContent::Center,
                            column_gap: px(16),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|foot| {
                            for (place, row) in footer.iter().enumerate() {
                                let chosen = !options_menu.on_tabs
                                    && options_menu.selected == rows.len() + place;
                                spawn_foot_button(foot, &font, row, accent, chosen);
                            }
                        });
                });

            // Leyenda de controles al pie
            spawn_legend(modal, &font, &icons, active_dev, &prompts);
        });
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn on_control_the_steering_and_the_legend_stand_apart() {
        let items = control_items(None, &KeyboardShapes::default());
        let starts: Vec<ControlItem> = (0..items.len())
            .filter(|&index| {
                starts_group(OptionsTab::Control, None, index, &KeyboardShapes::default())
            })
            .map(|index| items[index])
            .collect();
        assert_eq!(
            starts,
            [
                ControlItem::StickBehaviour,
                ControlItem::ShowLegend,
                ControlItem::RescanGamepads
            ]
        );
        // The shortcuts under the legend, on Control only.
        let legend = items
            .iter()
            .position(|&item| item == ControlItem::ShowLegend)
            .unwrap();
        assert!(shortcuts_after(OptionsTab::Control, None, legend));
        assert!(!shortcuts_after(OptionsTab::Control, None, legend + 1));
        assert!(!shortcuts_after(OptionsTab::Game, None, legend));
        assert!((0..8).all(|index| !starts_group(
            OptionsTab::Game,
            None,
            index,
            &KeyboardShapes::default()
        )));
        // A page of Control's: its own options, in no groups.
        let keyboard = Some(Page::Keyboard);
        assert_eq!(control_items(keyboard, &KeyboardShapes::default()).len(), 4);
        assert!((0..4).all(|index| !starts_group(
            OptionsTab::Control,
            keyboard,
            index,
            &KeyboardShapes::default()
        )));
        assert_eq!(
            control_items(Some(Page::Stick), &KeyboardShapes::default()),
            [ControlItem::StickTurnLimit, ControlItem::StickCurve]
        );
    }

    #[test]
    fn the_list_scrolls_as_little_as_needed_to_show_the_chosen_option() {
        let list = ListScroll {
            offset: 0.0,
            view: 300.0,
            content: 420.0,
        };
        // In view: as it is.
        assert_eq!(list.showing(100.0, 34.0), list);
        // Below: down until its foot is at the list's; above, up until its
        // top is at the list's.
        let down = list.showing(380.0, 34.0);
        assert_eq!(down.offset, 114.0);
        assert_eq!(down.showing(37.0, 34.0).offset, 37.0);
        // Never past either end.
        assert_eq!(list.showing(400.0, 40.0).offset, 120.0);
        assert_eq!(down.showing(-5.0, 34.0).offset, 0.0);
        // The bar: its thumb as tall as the share shown, as far down as the
        // list is scrolled; none when it all fits.
        assert_eq!(down.thumb(), Some((114.0 / 420.0, 300.0 / 420.0)));
        let fits = ListScroll {
            content: 300.0,
            ..list
        };
        assert_eq!(fits.thumb(), None);
        assert_eq!(fits.showing(250.0, 34.0).offset, 0.0);
    }

    #[test]
    fn graphics_keeps_its_advanced_options_on_a_page_of_their_own() {
        let main = graphics_items(None);
        let advanced = graphics_items(Some(Page::Advanced));
        assert_eq!(
            advanced,
            [
                GraphicsItem::Trees,
                GraphicsItem::FarTrees,
                GraphicsItem::SplitDetail,
                GraphicsItem::PerformanceLog,
                GraphicsItem::PipelinedRendering,
                GraphicsItem::FrameLatency
            ]
        );
        assert!(advanced.iter().all(|item| !main.contains(item)));
        assert_eq!(main.last(), Some(&GraphicsItem::Advanced));
        // Back to its row on Graphics.
        let mut menu = OptionsMenu::default();
        menu.open(OptionsReturn::Welcome);
        menu.tab = OptionsTab::Graphics;
        menu.page = Some(Page::Advanced);
        menu.leave_page(Page::Advanced);
        assert_eq!(main[menu.selected], GraphicsItem::Advanced);
        // Reset there puts back its own options only.
        let mut settings = Settings {
            performance_log: false,
            frame_latency: 3,
            modelled_trees: false,
            shadows: ShadowQuality::Low,
            show_coordinates: true,
            ..default()
        };
        let mut debug = DebugOptions::default();
        let page = Some(Page::Advanced);
        assert!(!section_is_default(
            OptionsTab::Graphics,
            page,
            &settings,
            &debug
        ));
        reset_section(OptionsTab::Graphics, page, &mut settings, &mut debug);
        assert!(section_is_default(
            OptionsTab::Graphics,
            page,
            &settings,
            &debug
        ));
        assert!(settings.modelled_trees);
        assert_eq!(settings.shadows, ShadowQuality::Low);
        assert!(settings.show_coordinates);
    }

    #[test]
    fn graphics_puts_the_fps_limit_under_the_resolution_and_what_the_performance_forces_under_it() {
        let main = graphics_items(None);
        let at = |item| main.iter().position(|&i| i == item).unwrap();
        // The cap, renamed, right under the resolution.
        assert_eq!(at(GraphicsItem::Fps), at(GraphicsItem::Resolution) + 1);
        // The render scale and the shadows right under the performance
        // forced, which chooses them when on; a group of their own.
        let forced = at(GraphicsItem::ForcePerformance);
        assert_eq!(at(GraphicsItem::RenderScale), forced + 1);
        assert_eq!(at(GraphicsItem::Shadows), forced + 2);
        assert!(starts_group(
            OptionsTab::Graphics,
            None,
            forced,
            &KeyboardShapes::default()
        ));
        assert!(!starts_group(
            OptionsTab::Graphics,
            None,
            forced + 1,
            &KeyboardShapes::default()
        ));
        // The water, and the two pages at the foot of the list.
        assert!(main.contains(&GraphicsItem::Water));
        assert_eq!(
            main[main.len() - 2..],
            [GraphicsItem::PerformanceHud, GraphicsItem::Advanced]
        );
        // Off at first; the cap says the screen's rate when known.
        assert_eq!(first_settings().force_performance, ForcePerformance::Off);
        assert_eq!(
            fps_limit_name(FpsLimit::Screen, Some(59.94), Language::Spanish),
            "De la pantalla (60 Hz)"
        );
        assert_eq!(
            fps_limit_name(FpsLimit::Cap(120), Some(60.0), Language::English),
            "120"
        );
        assert_eq!(ForcePerformance::Fps120.name(Language::Spanish), "120 fps");
    }

    #[test]
    fn the_relief_is_on_graphics_own_list_under_the_antialiasing_and_resets_with_it() {
        use crate::settings::Relief;
        let main = graphics_items(None);
        let at = |item| main.iter().position(|&i| i == item).unwrap();
        assert_eq!(at(GraphicsItem::Relief), at(GraphicsItem::Antialiasing) + 1);
        let mut settings = Settings {
            relief: Relief::Normal,
            ..default()
        };
        let mut debug = DebugOptions::default();
        let graphics = OptionsTab::Graphics;
        assert!(!section_is_default(graphics, None, &settings, &debug));
        reset_section(graphics, None, &mut settings, &mut debug);
        assert_eq!(settings.relief, Relief::Off);
        assert!(section_is_default(graphics, None, &settings, &debug));
    }

    #[test]
    fn the_relief_s_options_follow_it_and_reset_with_graphics() {
        let main = graphics_items(None);
        let at = |item| main.iter().position(|&i| i == item).unwrap();
        assert_eq!(at(GraphicsItem::RoadRelief), at(GraphicsItem::Relief) + 1);
        assert_eq!(at(GraphicsItem::FlatAtSpeed), at(GraphicsItem::Relief) + 2);
        let mut settings = Settings {
            road_relief: true,
            flat_at_speed: false,
            ..default()
        };
        let mut debug = DebugOptions::default();
        let graphics = OptionsTab::Graphics;
        assert!(!section_is_default(graphics, None, &settings, &debug));
        reset_section(graphics, None, &mut settings, &mut debug);
        assert!(!settings.road_relief && settings.flat_at_speed);
        assert!(section_is_default(graphics, None, &settings, &debug));
    }

    #[test]
    fn the_grass_tiling_is_under_the_relief_and_resets_with_graphics() {
        use crate::settings::GrassTiling;
        let main = graphics_items(None);
        let at = |item| main.iter().position(|&i| i == item).unwrap();
        assert_eq!(
            at(GraphicsItem::GrassTiling),
            at(GraphicsItem::FlatAtSpeed) + 1
        );
        let mut settings = Settings {
            grass_tiling: GrassTiling::Stochastic,
            ..default()
        };
        let mut debug = DebugOptions::default();
        let graphics = OptionsTab::Graphics;
        assert!(!section_is_default(graphics, None, &settings, &debug));
        reset_section(graphics, None, &mut settings, &mut debug);
        assert_eq!(settings.grass_tiling, GrassTiling::Wide);
        assert!(section_is_default(graphics, None, &settings, &debug));
    }

    #[test]
    fn the_performance_hud_shows_what_its_page_says_and_resets_on_its_own() {
        let page = Some(Page::PerformanceHud);
        assert_eq!(
            graphics_items(page),
            [
                GraphicsItem::ShowFps,
                GraphicsItem::ShowResolution,
                GraphicsItem::ShowAntialiasing,
                GraphicsItem::ShowTextures,
                GraphicsItem::ShowCoordinates,
                GraphicsItem::ShowKeyboardCurves
            ]
        );
        assert_eq!(
            graphics_items(None)[Page::PerformanceHud.row()],
            GraphicsItem::PerformanceHud
        );
        assert_eq!(
            Page::PerformanceHud.title(Language::Spanish),
            "HUD DE RENDIMIENTO"
        );
        let mut settings = Settings {
            show_fps: false,
            show_resolution: true,
            show_antialiasing: true,
            show_textures: true,
            show_keyboard_curves: true,
            cartoon_water: true,
            ..default()
        };
        let mut debug = DebugOptions::default();
        let graphics = OptionsTab::Graphics;
        assert!(!section_is_default(graphics, page, &settings, &debug));
        reset_section(graphics, page, &mut settings, &mut debug);
        assert!(section_is_default(graphics, page, &settings, &debug));
        assert!(settings.show_fps && !settings.show_resolution);
        assert!(!settings.show_antialiasing && !settings.show_textures);
        assert!(!settings.show_keyboard_curves);
        // The water is on Graphics' own list: its Reset puts it back.
        assert!(!section_is_default(graphics, None, &settings, &debug));
        reset_section(graphics, None, &mut settings, &mut debug);
        assert!(!settings.cartoon_water);
        assert!(section_is_default(graphics, None, &settings, &debug));
    }

    #[test]
    fn control_s_advanced_page_chooses_how_the_keyboard_behaves_and_its_page_follows() {
        let advanced = Some(Page::ControlAdvanced);
        assert_eq!(
            control_items(advanced, &KeyboardShapes::default()),
            [ControlItem::KeyboardMode, ControlItem::StickMode]
        );
        let mut menu = OptionsMenu::default();
        menu.open(OptionsReturn::Welcome);
        menu.tab = OptionsTab::Control;
        menu.page = advanced;
        menu.leave_page(Page::ControlAdvanced);
        assert_eq!(
            control_items(None, &KeyboardShapes::default())[menu.selected],
            ControlItem::Advanced
        );
        // The keyboard's page: its rates as always, its curves in Bézier,
        // both back on its row.
        assert_eq!(keyboard_page(KeyboardMode::Classic), Page::Keyboard);
        assert_eq!(keyboard_page(KeyboardMode::Bezier), Page::KeyboardCurves);
        assert_eq!(stick_page(KeyboardMode::Classic), Page::Stick);
        assert_eq!(stick_page(KeyboardMode::Bezier), Page::StickCurves);
        assert_eq!(Page::StickCurves.row(), Page::Stick.row());
        // The stick's curves: its response first, then the keyboard's.
        let stick = Settings::default();
        let page = Some(Page::StickCurves);
        let items = control_items(page, shapes_of(&stick, page));
        assert_eq!(items[0], ControlItem::CurveShape(CurveKind::Response));
        assert_eq!(items.len(), 7 + 5 + 2);
        assert!(shapes_of(&stick, page).stick);
        assert!(!shapes_of(&stick, Some(Page::KeyboardCurves)).stick);
        // Each curve's shape, under it a custom one's numbers and its time
        // (but E's), and learning at the end.
        let mut shapes = KeyboardShapes::default();
        let curves = |shapes: &KeyboardShapes| control_items(Some(Page::KeyboardCurves), shapes);
        assert_eq!(curves(&shapes).len(), 6 + 5 + 2);
        assert_eq!(
            curves(&shapes)[0],
            ControlItem::CurveShape(CurveKind::LeanIn)
        );
        assert_eq!(
            curves(&shapes)[1],
            ControlItem::CurveScale(CurveKind::LeanIn)
        );
        assert_eq!(*curves(&shapes).last().unwrap(), ControlItem::Learning);
        shapes.lean_in.shape = CurveShape::Custom;
        assert_eq!(
            curves(&shapes)[1],
            ControlItem::CurveCustom(CurveKind::LeanIn)
        );
        assert!(starts_group(
            OptionsTab::Control,
            Some(Page::KeyboardCurves),
            3,
            &shapes
        ));
        // Learning, no numbers to edit: every curve is learnt.
        shapes.set_learn(crate::bike::curves::LearnLine::Centre);
        assert!(!curves(&shapes).contains(&ControlItem::CurveCustom(CurveKind::LeanIn)));
        assert_eq!(Page::KeyboardCurves.row(), Page::Keyboard.row());
        assert_eq!(
            Page::KeyboardCurves.title(Language::Spanish),
            Page::Keyboard.title(Language::Spanish)
        );

        let control = OptionsTab::Control;
        let curves = Some(Page::KeyboardCurves);
        let mut settings = Settings::default();
        let mut debug = DebugOptions::default();
        assert!(section_is_default(control, advanced, &settings, &debug));
        settings.keyboard_mode = KeyboardMode::Bezier;
        settings.keyboard_shapes.turn.shape = CurveShape::Linear;
        assert!(!section_is_default(control, advanced, &settings, &debug));
        assert!(!section_is_default(control, curves, &settings, &debug));
        // Each page puts back its own.
        reset_section(control, curves, &mut settings, &mut debug);
        assert!(section_is_default(control, curves, &settings, &debug));
        assert_eq!(settings.keyboard_mode, KeyboardMode::Bezier);
        assert!(!section_is_default(control, None, &settings, &debug));
        reset_section(control, advanced, &mut settings, &mut debug);
        assert!(section_is_default(control, None, &settings, &debug));

        // The four numbers named by the end they shape and what they
        // measure, on one line with the one edited in brackets; stepped by
        // 0.05 within the square, on the hundredths.
        assert_eq!(
            point_name(0, CurveKind::Speed, Language::Spanish),
            "Inicio · velocidad"
        );
        assert_eq!(
            point_name(3, CurveKind::Turn, Language::English),
            "End · turn"
        );
        let line = custom_line(crate::bezier::Bezier::new(0.1, 0.25, 0.5, 1.0), Some(1));
        assert_eq!(line, " 0.10   [0.25]   0.50    1.00 ");
        assert_eq!(step_point(0.98, 1), 1.0);
        assert_eq!(step_point(0.333, -1), 0.28);
        assert_eq!(step_point(0.02, -1), 0.0);
    }

    #[test]
    fn a_page_of_controls_opens_from_its_row_and_goes_back_to_it() {
        let mut menu = OptionsMenu::default();
        menu.open(OptionsReturn::Welcome);
        menu.tab = OptionsTab::Control;
        menu.page = Some(Page::Keyboard);
        menu.selected = 2;
        menu.leave_page(Page::Keyboard);
        assert_eq!(menu.page, None);
        assert_eq!(
            control_items(None, &KeyboardShapes::default())[menu.selected],
            ControlItem::KeyboardBehaviour
        );
        // Closing the options leaves no page open for the next time.
        menu.page = Some(Page::Stick);
        menu.close();
        menu.open(OptionsReturn::Welcome);
        assert_eq!(menu.page, None);
    }

    #[test]
    fn the_arrows_reach_the_row_of_sections_and_change_section_there() {
        let mut menu = OptionsMenu::default();
        menu.open(OptionsReturn::Welcome);
        let count = tab_items_count(menu.tab, None, &KeyboardShapes::default());
        // Up from the first option: the row of sections.
        assert!(menu.navigate(-1, 0, false, count));
        assert!(menu.on_tabs);
        // Across there: the next section, and the one before.
        assert!(menu.navigate(0, 1, false, count));
        assert_eq!(menu.tab, OptionsTab::Control);
        assert!(menu.navigate(0, -1, false, count));
        assert!(menu.navigate(0, -1, false, count));
        assert_eq!(menu.tab, OptionsTab::Graphics);
        assert!(menu.on_tabs);
        // Down: its first option; up from the row: its last one.
        let count = tab_items_count(menu.tab, None, &KeyboardShapes::default());
        assert!(menu.navigate(1, 0, false, count));
        assert!(!menu.on_tabs);
        assert_eq!(menu.selected, 0);
        menu.on_tabs = true;
        assert!(menu.navigate(-1, 0, false, count));
        assert_eq!(menu.selected, count - 1);
        // Down from the last option: round to the row of sections.
        assert!(menu.navigate(1, 0, false, count));
        assert!(menu.on_tabs);
        // Confirming on the row goes down into the section.
        assert!(menu.navigate(0, 0, true, count));
        assert!(!menu.on_tabs && menu.selected == 0);
        // Across on an option is left to the option.
        assert!(!menu.navigate(0, 1, false, count));
        assert_eq!(menu.tab, OptionsTab::Graphics);
    }

    use crate::settings::{Controls, Difficulty, ForcePerformance, TrackView};

    #[test]
    fn reset_and_back_stand_side_by_side_at_the_foot() {
        let mut menu = OptionsMenu::default();
        menu.open(OptionsReturn::Welcome);
        menu.tab = OptionsTab::Sound;
        let count = tab_items_count(menu.tab, None, &KeyboardShapes::default());
        assert_eq!(count, sound_items().len() + FOOTER);
        // Down from the last option: Back.
        menu.selected = count - FOOTER - 1;
        assert!(menu.navigate(1, 0, false, count));
        assert_eq!(menu.selected, count - 1);
        // Across: Reset, and Back again.
        assert!(menu.navigate(0, -1, false, count));
        assert_eq!(menu.selected, count - 2);
        assert!(menu.navigate(0, 1, false, count));
        assert_eq!(menu.selected, count - 1);
        // Up from either: the last option; down: the row of sections.
        menu.selected = count - 2;
        assert!(menu.navigate(-1, 0, false, count));
        assert_eq!(menu.selected, count - FOOTER - 1);
        menu.selected = count - 2;
        assert!(menu.navigate(1, 0, false, count));
        assert!(menu.on_tabs);
    }

    #[test]
    fn reset_puts_back_the_options_of_its_section_only() {
        let mut settings = Settings::default();
        let mut debug = DebugOptions::default();
        for tab in TABS {
            assert!(section_is_default(tab, None, &settings, &debug), "{tab:?}");
        }
        settings.difficulty = Difficulty::Hard;
        settings.music_volume = 10;
        settings.fps = FPS_LIMITS[FPS_LIMITS.len() - 1];
        settings.frame_latency = 3;
        debug.show_legend = true;
        settings.controls.remap(KeyCode::KeyW, &[KeyCode::KeyI]);
        for tab in TABS {
            assert!(!section_is_default(tab, None, &settings, &debug), "{tab:?}");
        }
        reset_section(OptionsTab::Sound, None, &mut settings, &mut debug);
        assert_eq!(settings.music_volume, Settings::default().music_volume);
        assert!(section_is_default(
            OptionsTab::Sound,
            None,
            &settings,
            &debug
        ));
        assert_eq!(settings.difficulty, Difficulty::Hard);
        reset_section(OptionsTab::Control, None, &mut settings, &mut debug);
        assert!(!debug.show_legend);
        // The keys and buttons are reset in the mapping, not here.
        assert_ne!(settings.controls, Controls::default());
        reset_section(OptionsTab::Game, None, &mut settings, &mut debug);
        reset_section(OptionsTab::Graphics, None, &mut settings, &mut debug);
        for tab in TABS {
            assert!(section_is_default(tab, None, &settings, &debug), "{tab:?}");
        }
        // On a page of Control's, its own options only.
        let (stick, keyboard) = (Some(Page::Stick), Some(Page::Keyboard));
        debug.stick_limit = TurnLimit::Off;
        debug.stick_curve = StickCurve::Quadratic;
        debug.keyboard_reverse = ReverseRate::Slow;
        let control = OptionsTab::Control;
        assert!(!section_is_default(control, stick, &settings, &debug));
        assert!(!section_is_default(control, keyboard, &settings, &debug));
        reset_section(control, keyboard, &mut settings, &mut debug);
        assert!(section_is_default(control, keyboard, &settings, &debug));
        assert_eq!(debug.stick_limit, TurnLimit::Off);
        assert_eq!(debug.stick_curve, StickCurve::Quadratic);
        reset_section(control, stick, &mut settings, &mut debug);
        assert_eq!(debug.stick_curve, StickCurve::Normal);
        assert!(section_is_default(control, None, &settings, &debug));
    }

    #[test]
    fn game_offers_the_view_of_the_track_and_starts_from_the_road() {
        // From the road at first, so that the camera of the start menu
        // and the modes goes on into the track screen.
        let first = Settings::default();
        assert_eq!(first.track_view, TrackView::Trackside);
        assert!(first.track_view.at_the_road());
        assert!(game_items().contains(&GameItem::TrackView));
        // Both views are offered, each with a name of its own in both
        // languages, and only one of them is from the road.
        assert_eq!(TRACK_VIEWS.len(), 2);
        for lang in [Language::Spanish, Language::English] {
            let names: Vec<&str> = TRACK_VIEWS.iter().map(|view| view.name(lang)).collect();
            assert_ne!(names[0], names[1], "{lang:?}");
            assert!(names.iter().all(|name| !name.is_empty()), "{lang:?}");
        }
        assert_eq!(
            TRACK_VIEWS.iter().filter(|view| view.at_the_road()).count(),
            1
        );
        // Changed, the section is no longer as it was, and Reset puts it
        // back.
        let mut settings = first.clone();
        let mut debug = DebugOptions::default();
        settings.track_view = cycle(&TRACK_VIEWS, settings.track_view, 1);
        assert_eq!(settings.track_view, TrackView::Aerial);
        assert!(!section_is_default(
            OptionsTab::Game,
            None,
            &settings,
            &debug
        ));
        reset_section(OptionsTab::Game, None, &mut settings, &mut debug);
        assert_eq!(settings.track_view, first.track_view);
    }

    #[test]
    fn each_section_has_its_own_colour() {
        let colors: Vec<Color> = TABS.iter().map(|tab| tab.accent()).collect();
        for (i, a) in colors.iter().enumerate() {
            for b in &colors[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn a_box_on_is_in_the_colour_of_its_section_and_dark_when_chosen() {
        let accent = OptionsTab::Graphics.accent();
        let (frame, fill, tick) = check_colors(true, false, false, accent);
        assert_eq!((frame, fill, tick), (accent, accent, INK));
        // On the chosen row, already in the colour of the section.
        assert_eq!(check_colors(true, true, false, accent), (INK, INK, accent));
        // Off, empty: no tick, and only a faint fill.
        let (_, fill, tick) = check_colors(false, false, false, accent);
        assert_eq!(tick.alpha(), 0.0);
        assert!(fill.alpha() < 0.1);
        // Faint when it cannot be changed.
        let (frame, ..) = check_colors(true, false, true, accent);
        assert!(frame.alpha() < accent.alpha());
    }
}
