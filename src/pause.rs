//! Menú de pausa: Esc o Start paran la carrera y abren un menú para seguir,
//! go back to the bike selection, change the options or go back to the
//! main menu. Under its title it says what is being played and where.

use bevy::prelude::*;

use crate::circuits::{Circuits, CurrentCircuit};
use crate::gamepads::ActiveController;
use crate::hud::UiFont;
use crate::legend::{Keys, menu_prompts, spawn_legend};
use crate::locale::Language;
use crate::menu::GameState;
use crate::modes;
use crate::options::{OptionsInput, OptionsMenu, OptionsReturn};
use crate::race::{Championship, Rules};
use crate::settings::{Action as Control, Settings, cycle};
use crate::ui::{
    InputDevice, MenuInput, MenuSound, Row, UiIcons, current_active_device, spawn_panel,
    spawn_title,
};

pub struct PausePlugin;

impl Plugin for PausePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pause>()
            .init_resource::<DebugOptions>()
            .init_resource::<OptionsMenu>()
            .add_systems(OnExit(GameState::Race), close)
            .add_systems(
                Update,
                (use_pause_menu.after(OptionsInput), show_pause_menu)
                    .chain()
                    .run_if(in_state(GameState::Race)),
            );
    }
}

/// Opciones de depuración, para probar variantes del manejo.
#[derive(Resource)]
pub struct DebugOptions {
    /// Con el stick, cuánto limitar el giro según la velocidad (de entrada,
    /// medio).
    pub stick_limit: TurnLimit,
    /// With the stick, how much the bike turns for how far it is pushed
    /// (normal at first: as always).
    pub stick_curve: StickCurve,
    /// Con el teclado o la cruceta, lo mismo (de entrada, medio).
    pub keyboard_limit: TurnLimit,
    /// Con el teclado o la cruceta, cuánto gira al dejar pulsada la tecla
    /// (de entrada, medio).
    pub keyboard_rate: SteerRate,
    /// Con el teclado o la cruceta, la rapidez con la que recupera al soltar
    /// la tecla de girar (de entrada, media).
    pub keyboard_return: ReturnRate,
    /// With the keyboard or the d-pad, how fast the bike and the rider swing
    /// over to the other side (fast at first: as always).
    pub keyboard_reverse: ReverseRate,
    /// Si se ve en la carrera la leyenda con los controles (de entrada, no).
    pub show_legend: bool,
}

impl Default for DebugOptions {
    fn default() -> Self {
        Self {
            stick_limit: TurnLimit::Medium,
            stick_curve: StickCurve::Normal,
            keyboard_limit: TurnLimit::Medium,
            keyboard_rate: SteerRate::Medium,
            keyboard_return: ReturnRate::Medium,
            keyboard_reverse: ReverseRate::Fast,
            show_legend: false,
        }
    }
}

/// How much the bike turns, and leans, for how far the stick is pushed:
/// the curve from the stick to the steering, past the stick's dead zone.
/// All of them turn fully with the stick all the way over; they differ
/// near the middle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StickCurve {
    /// As always: between linear and quadratic, a little finer than linear
    /// near the middle.
    Normal,
    /// As far as the stick is pushed.
    Linear,
    /// The square of it: finer near the middle, for lively sticks.
    Quadratic,
    /// The cube of it: finest near the middle, for the liveliest sticks.
    Cubic,
    /// The square root of it: liveliest near the middle.
    SquareRoot,
}

const STICK_CURVES: [StickCurve; 5] = [
    StickCurve::Normal,
    StickCurve::Linear,
    StickCurve::Quadratic,
    StickCurve::Cubic,
    StickCurve::SquareRoot,
];

impl StickCurve {
    /// The power the stick's travel past its dead zone (from 0 to 1) is
    /// raised to.
    pub fn exponent(self) -> f32 {
        match self {
            StickCurve::Normal => 1.35,
            StickCurve::Linear => 1.0,
            StickCurve::Quadratic => 2.0,
            StickCurve::Cubic => 3.0,
            StickCurve::SquareRoot => 0.5,
        }
    }

    pub fn name(self, lang: Language) -> &'static str {
        match lang {
            Language::Spanish => match self {
                StickCurve::Normal => "normal",
                StickCurve::Linear => "lineal",
                StickCurve::Quadratic => "cuadrática",
                StickCurve::Cubic => "cúbica",
                StickCurve::SquareRoot => "raíz cuadrada",
            },
            Language::English => match self {
                StickCurve::Normal => "normal",
                StickCurve::Linear => "linear",
                StickCurve::Quadratic => "quadratic",
                StickCurve::Cubic => "cubic",
                StickCurve::SquareRoot => "square root",
            },
        }
    }

    /// The next one (`step` 1) or the one before (-1), round and round.
    pub fn next(self, step: i32) -> Self {
        cycle(&STICK_CURVES, self, step)
    }
}

/// Cuánto gira el teclado o la cruceta al dejar pulsada la tecla: lento,
/// medio o alto (lo de siempre).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SteerRate {
    Slow,
    Medium,
    Fast,
}

const STEER_RATES: [SteerRate; 3] = [SteerRate::Slow, SteerRate::Medium, SteerRate::Fast];

impl SteerRate {
    /// Factor que multiplica la rapidez con la que gira el teclado.
    pub fn factor(self) -> f32 {
        match self {
            SteerRate::Slow => 0.5,
            SteerRate::Medium => 0.75,
            SteerRate::Fast => 1.0,
        }
    }

    pub fn name(self, lang: Language) -> &'static str {
        match lang {
            Language::Spanish => match self {
                SteerRate::Slow => "lento",
                SteerRate::Medium => "medio",
                SteerRate::Fast => "alto",
            },
            Language::English => match self {
                SteerRate::Slow => "slow",
                SteerRate::Medium => "medium",
                SteerRate::Fast => "fast",
            },
        }
    }

    /// El nivel siguiente (`step` 1) o el anterior (-1), dando la vuelta.
    pub fn next(self, step: i32) -> Self {
        cycle(&STEER_RATES, self, step)
    }
}

/// Rapidez con la que recupera el teclado o la cruceta al soltar la tecla de
/// girar: lenta, media o rápida (lo de antes).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReturnRate {
    Slow,
    Medium,
    Fast,
}

const RETURN_RATES: [ReturnRate; 3] = [ReturnRate::Slow, ReturnRate::Medium, ReturnRate::Fast];

impl ReturnRate {
    /// Factor que multiplica la rapidez de recuperación al soltar. Medium,
    /// the default, is what slow used to be (straight again about 0.9 s
    /// after letting go); slow is two thirds of it, as before (1.1 s), and
    /// fast is the original (0.6 s).
    pub fn factor(self) -> f32 {
        match self {
            ReturnRate::Slow => 1.0 / 3.0,
            ReturnRate::Medium => 0.5,
            ReturnRate::Fast => 1.0,
        }
    }

    pub fn name(self, lang: Language) -> &'static str {
        match lang {
            Language::Spanish => match self {
                ReturnRate::Slow => "lenta",
                ReturnRate::Medium => "media",
                ReturnRate::Fast => "rápida",
            },
            Language::English => match self {
                ReturnRate::Slow => "slow",
                ReturnRate::Medium => "medium",
                ReturnRate::Fast => "fast",
            },
        }
    }

    /// El nivel siguiente (`step` 1) o el anterior (-1), dando la vuelta.
    pub fn next(self, step: i32) -> Self {
        cycle(&RETURN_RATES, self, step)
    }
}

/// How fast the bike and the rider swing over to the other side when the
/// keyboard or the d-pad turns the other way while turning one way: as
/// always (fast), or slower.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReverseRate {
    Slow,
    Medium,
    Fast,
}

const REVERSE_RATES: [ReverseRate; 3] = [ReverseRate::Slow, ReverseRate::Medium, ReverseRate::Fast];

impl ReverseRate {
    /// How much slower than usual the bike's lean and the rider's body go
    /// over to the other side when the keyboard or the d-pad turns the other
    /// way (`steer_direct`, the turn they give, not nothing): none at all
    /// fast, as always; medium at 60 % of the usual speed, slow at 35 %. The
    /// turn itself changes sides at once whatever the rate.
    pub fn slowing(self, steer_direct: f32) -> f32 {
        if steer_direct == 0.0 {
            return 0.0;
        }
        match self {
            ReverseRate::Slow => 0.65,
            ReverseRate::Medium => 0.4,
            ReverseRate::Fast => 0.0,
        }
    }

    pub fn name(self, lang: Language) -> &'static str {
        match lang {
            Language::Spanish => match self {
                ReverseRate::Slow => "lento",
                ReverseRate::Medium => "medio",
                ReverseRate::Fast => "rápido",
            },
            Language::English => match self {
                ReverseRate::Slow => "slow",
                ReverseRate::Medium => "medium",
                ReverseRate::Fast => "fast",
            },
        }
    }

    /// El nivel siguiente (`step` 1) o el anterior (-1), dando la vuelta.
    pub fn next(self, step: i32) -> Self {
        cycle(&REVERSE_RATES, self, step)
    }
}

/// Cuánto se limita el giro con la velocidad: nada, o una parte del límite
/// (alto, todo: ver `DIGITAL_STEER_FAST` en `bike`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnLimit {
    Off,
    Low,
    Medium,
    High,
}

const TURN_LIMITS: [TurnLimit; 4] = [
    TurnLimit::Off,
    TurnLimit::Low,
    TurnLimit::Medium,
    TurnLimit::High,
];

impl TurnLimit {
    /// Qué parte del límite se aplica.
    pub fn strength(self) -> f32 {
        match self {
            TurnLimit::Off => 0.0,
            TurnLimit::Low => 1.0 / 3.0,
            TurnLimit::Medium => 2.0 / 3.0,
            TurnLimit::High => 1.0,
        }
    }

    pub fn name(self, lang: Language) -> &'static str {
        match lang {
            Language::Spanish => match self {
                TurnLimit::Off => "no",
                TurnLimit::Low => "bajo",
                TurnLimit::Medium => "medio",
                TurnLimit::High => "alto",
            },
            Language::English => match self {
                TurnLimit::Off => "off",
                TurnLimit::Low => "low",
                TurnLimit::Medium => "medium",
                TurnLimit::High => "high",
            },
        }
    }

    /// El nivel siguiente (`step` 1) o el anterior (-1), dando la vuelta:
    /// después del más alto, sin límite.
    pub fn next(self, step: i32) -> Self {
        cycle(&TURN_LIMITS, self, step)
    }
}

/// Si la carrera está en pausa, qué página del menú se ve y qué opción está
/// elegida.
#[derive(Resource, Default)]
pub struct Pause {
    page: Option<Page>,
    selected: usize,
}

impl Pause {
    pub fn is_open(&self) -> bool {
        self.page.is_some()
    }
}

/// Para los `run_if` de lo que no debe pasar con la carrera en pausa.
pub fn running(pause: Res<Pause>) -> bool {
    !pause.is_open()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Main,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Resume,
    /// Back to the screen before the race: choosing the bike.
    Back,
    OpenOptions,
    MainMenu,
}

/// Las opciones de cada página: su texto y lo que hacen.
pub fn items(page: Page, lang: Language) -> Vec<(String, Action)> {
    match page {
        Page::Main => vec![
            (
                match lang {
                    Language::Spanish => "Continuar".into(),
                    Language::English => "Resume".into(),
                },
                Action::Resume,
            ),
            (
                match lang {
                    Language::Spanish => "Volver".into(),
                    Language::English => "Back".into(),
                },
                Action::Back,
            ),
            (
                match lang {
                    Language::Spanish => "Opciones".into(),
                    Language::English => "Options".into(),
                },
                Action::OpenOptions,
            ),
            (
                match lang {
                    Language::Spanish => "Volver al menú principal".into(),
                    Language::English => "Return to main menu".into(),
                },
                Action::MainMenu,
            ),
        ],
    }
}

/// Abre y cierra la pausa, se mueve por las opciones y las aplica.
#[allow(clippy::too_many_arguments)]
fn use_pause_menu(
    mut input: MenuInput,
    settings: Res<Settings>,
    mut pause: ResMut<Pause>,
    mut options_menu: Option<ResMut<OptionsMenu>>,
    mut time: ResMut<Time<Virtual>>,
    mut next: ResMut<NextState<GameState>>,
    mut sounds: MessageWriter<MenuSound>,
    outcome: Option<Res<crate::race::Outcome>>,
) {
    let over = outcome.is_some();
    // While the options are open, and in the frame they close (the key that
    // closed them must not also act here).
    if options_menu
        .as_ref()
        .is_some_and(|m| m.is_open || m.is_changed())
    {
        return;
    }

    let request = input.read();
    // La tecla o el botón de pausa (Esc o Start, si no se han cambiado).
    let toggle =
        settings
            .controls
            .just_pressed(Control::Pause, &input.keyboard, input.gamepads.iter());
    let Some(page) = pause.page else {
        // Not over the results of a race.
        if toggle && !over {
            *pause = Pause {
                page: Some(Page::Main),
                selected: 0,
            };
            sounds.write(MenuSound::Confirm);
            time.pause();
        }
        return;
    };
    let items = items(page, settings.language);
    if request.step != 0 {
        pause.selected =
            (pause.selected as i32 + request.step).rem_euclid(items.len() as i32) as usize;
        sounds.write(MenuSound::Move);
    }

    let action = if toggle || request.back {
        Some(Action::Resume)
    } else if request.confirm {
        Some(items[pause.selected].1)
    } else {
        None
    };

    match action {
        Some(Action::Resume) => {
            sounds.write(MenuSound::Move);
            *pause = Pause::default();
            time.unpause();
        }
        Some(Action::Back) => {
            sounds.write(MenuSound::Confirm);
            *pause = Pause::default();
            time.unpause();
            next.set(GameState::Select);
        }
        Some(Action::OpenOptions) => {
            sounds.write(MenuSound::Confirm);
            if let Some(ref mut menu) = options_menu {
                menu.open(OptionsReturn::Pause);
            }
        }
        Some(Action::MainMenu) => {
            sounds.write(MenuSound::Confirm);
            *pause = Pause::default();
            time.unpause();
            next.set(GameState::Welcome);
        }
        None => {}
    }
}

/// Al salir de la carrera la pausa se cierra y el tiempo vuelve a correr.
fn close(mut pause: ResMut<Pause>, mut time: ResMut<Time<Virtual>>) {
    *pause = Pause::default();
    time.unpause();
}

/// El menú en pantalla, rehecho cada vez que cambia algo.
#[derive(Component)]
struct PauseMenu;

#[allow(clippy::too_many_arguments)]
fn show_pause_menu(
    mut commands: Commands,
    pause: Res<Pause>,
    options_menu: Option<Res<OptionsMenu>>,
    settings: Res<Settings>,
    device: Option<Res<InputDevice>>,
    active_controller: Option<Res<ActiveController>>,
    font: Res<UiFont>,
    icons: Res<UiIcons>,
    shown: Query<Entity, With<PauseMenu>>,
    rules: Option<Res<Rules>>,
    current: Option<Res<CurrentCircuit>>,
    circuits: Option<Res<Circuits>>,
    championship: Option<Res<Championship>>,
) {
    if !pause.is_changed()
        && options_menu.as_ref().is_some_and(|m| !m.is_changed())
        && !settings.is_changed()
        && device.as_ref().is_some_and(|d| !d.is_changed())
        && active_controller.as_ref().is_some_and(|c| !c.is_changed())
    {
        return;
    }
    for entity in &shown {
        commands.entity(entity).despawn();
    }
    if options_menu.as_ref().is_some_and(|m| m.is_open) {
        return;
    }
    let Some(page) = pause.page else {
        return;
    };
    let lang = settings.language;
    let active_dev = current_active_device(
        device.as_deref().copied().unwrap_or_default(),
        active_controller.as_deref(),
    );
    let title = match lang {
        Language::Spanish => "PAUSA",
        Language::English => "PAUSE",
    };
    // Resuming is the main action: in blue.
    let rows: Vec<Row> = items(page, lang)
        .into_iter()
        .map(|(label, action)| match action {
            Action::Resume => Row::primary(label),
            _ => Row::new(label),
        })
        .collect();
    let back = lang.hint_resume();
    let prompts = menu_prompts(
        active_dev,
        &[(Keys::Confirm, lang.hint_accept()), (Keys::Back, back)],
    );

    commands
        .spawn((
            PauseMenu,
            DespawnOnExit(GameState::Race),
            Node {
                width: percent(100),
                height: percent(100),
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(14),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.02, 0.06, 0.7)),
            // Por encima del marcador.
            GlobalZIndex(10),
        ))
        .with_children(|menu| {
            spawn_title(menu, &font, title, 52.0);
            // What is being played and where.
            if let (Some(rules), Some(current), Some(circuits)) = (rules, current, circuits) {
                let track = circuits.get(*current).name(lang);
                let beside = match championship.filter(|_| rules.mode.is_championship()) {
                    Some(championship) => {
                        let round = championship.round.min(circuits.len() - 1) + 1;
                        format!("{track}  ·  {round}/{}", circuits.len())
                    }
                    None => track.to_string(),
                };
                modes::spawn_mode_badge(menu, &font, rules.mode, Some(&beside), lang);
            }
            spawn_panel(menu, &font, &rows, pause.selected, false);
            spawn_legend(menu, &font, &icons, active_dev, &prompts);
        });
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    /// A race with the pause menu, and the keyboard to drive it.
    fn race() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::input::InputPlugin)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Font>()
            .init_asset::<Image>()
            .init_state::<GameState>()
            .init_resource::<Settings>()
            .init_resource::<InputDevice>()
            .init_resource::<UiFont>()
            .init_resource::<UiIcons>()
            .add_message::<MenuSound>()
            .add_message::<AppExit>()
            .add_plugins((
                PausePlugin,
                crate::options::OptionsPlugin,
                crate::gamepads::GamepadsPlugin,
            ));
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Race);
        app.update();
        app.update();
        app
    }

    /// Presses and lets go of `key`, a frame each.
    fn press(app: &mut App, key: KeyCode) {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::{Key, KeyboardInput};
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code: key,
                logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        }
    }

    fn page(app: &App) -> (Option<Page>, usize) {
        let pause = app.world().resource::<Pause>();
        (pause.page, pause.selected)
    }

    #[test]
    fn back_from_the_pause_goes_to_the_bike_selection() {
        let mut app = race();
        press(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), (Some(Page::Main), 0));
        press(&mut app, KeyCode::ArrowDown);
        assert_eq!(page(&app), (Some(Page::Main), 1));
        press(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::Select
        );
        assert_eq!(page(&app), (None, 0), "the pause is over");
    }

    #[test]
    fn the_key_that_closes_the_options_does_not_resume_the_race() {
        let mut app = race();
        press(&mut app, KeyCode::Escape);
        press(&mut app, KeyCode::ArrowDown);
        press(&mut app, KeyCode::ArrowDown);
        press(&mut app, KeyCode::Enter);
        assert!(app.world().resource::<OptionsMenu>().is_open);
        press(&mut app, KeyCode::Escape);
        assert!(!app.world().resource::<OptionsMenu>().is_open);
        assert_eq!(
            page(&app),
            (Some(Page::Main), 2),
            "still paused, on Options"
        );
    }

    #[test]
    fn the_pause_menu_offers_what_was_asked() {
        let main: Vec<Action> = items(Page::Main, Language::Spanish)
            .into_iter()
            .map(|i| i.1)
            .collect();
        assert_eq!(
            main,
            [
                Action::Resume,
                Action::Back,
                Action::OpenOptions,
                Action::MainMenu,
            ]
        );
        let main_es: Vec<String> = items(Page::Main, Language::Spanish)
            .into_iter()
            .map(|i| i.0)
            .collect();
        assert_eq!(
            main_es,
            [
                "Continuar",
                "Volver",
                "Opciones",
                "Volver al menú principal"
            ]
        );
        let main_en: Vec<String> = items(Page::Main, Language::English)
            .into_iter()
            .map(|i| i.0)
            .collect();
        assert_eq!(
            main_en,
            ["Resume", "Back", "Options", "Return to main menu"]
        );

        // Rates and limits behavior
        assert_eq!(TurnLimit::Off.next(-1), TurnLimit::High);
        assert_eq!(TurnLimit::High.strength(), 1.0);
        assert_eq!(SteerRate::Slow.next(-1), SteerRate::Fast);
        assert_eq!(SteerRate::Fast.factor(), 1.0);
        assert!(SteerRate::Slow.factor() < SteerRate::Medium.factor());
        assert!(SteerRate::Medium.factor() < SteerRate::Fast.factor());
        assert_eq!(ReturnRate::Slow.next(-1), ReturnRate::Fast);
        assert_eq!(ReturnRate::Fast.factor(), 1.0);
        assert!(ReturnRate::Slow.factor() < ReturnRate::Medium.factor());
        assert!(ReturnRate::Medium.factor() < ReturnRate::Fast.factor());
    }
}
