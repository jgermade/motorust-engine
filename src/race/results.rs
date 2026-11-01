//! When the players' race is over: its results (each bike's place and
//! time, filling in as the others come in, and the points of the arcade
//! championship), on one side while the camera watches them come in; then
//! in the championship the standings, and on to the next race, to the end
//! of the championship or back to the menus.

use bevy::prelude::*;

use super::hud::{clock, ordinal};
use super::{Championship, Mode, Outcome, POINTS};
use crate::bike::Roster;
use crate::circuits::{Circuits, CurrentCircuit};
use crate::gamepads::ActiveController;
use crate::hud::UiFont;
use crate::legend::{Keys, menu_prompts, spawn_legend};
use crate::locale::Language;
use crate::menu::GameState;
use crate::settings::Settings;
use crate::ui::{InputDevice, MenuInput, MenuSound, UiIcons, current_active_device, text_font};

const GOLD: Color = Color::srgb(1.0, 0.82, 0.2);
const PLAYER: Color = Color::srgb(0.45, 0.82, 1.0);

pub struct ResultsPlugin;

impl Plugin for ResultsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Race), forget_the_page)
            .add_systems(OnExit(GameState::Race), forget_the_page)
            .add_systems(
                Update,
                (use_results, show_results)
                    .chain()
                    .run_if(in_state(GameState::Race).and_then(resource_exists::<Outcome>)),
            );
    }
}

/// Which page of the results is shown: the race's, then the championship's
/// standings; or none, on the way out of them.
#[derive(Resource, Clone, Copy, Default, PartialEq, Eq, Debug)]
enum ResultsPage {
    #[default]
    Race,
    Standings,
    Leaving,
}

fn forget_the_page(mut commands: Commands) {
    commands.remove_resource::<ResultsPage>();
}

/// What comes after the results: the standings of a championship, its next
/// race, the menu after the last one, or, out of a championship, the same
/// race again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Next {
    Standings,
    Race(usize),
    Menu,
    Again,
}

/// Where going back from the results of a race of `mode` goes: out of a
/// championship to the game modes; otherwise to choosing the track.
pub fn back_from(mode: Mode) -> GameState {
    if mode.is_championship() {
        GameState::Modes
    } else {
        GameState::Tracks
    }
}

/// What comes after the page `standings` (or not) of the results of a race
/// of `mode`, with the championship as it is now, over `tracks` tracks.
pub fn next_after(mode: Mode, standings: bool, championship: &Championship, tracks: usize) -> Next {
    if !mode.is_championship() {
        return Next::Again;
    }
    if !standings {
        return Next::Standings;
    }
    if championship.is_over(tracks) {
        Next::Menu
    } else {
        Next::Race(championship.round)
    }
}

#[allow(clippy::too_many_arguments)]
fn use_results(
    mut commands: Commands,
    mut input: MenuInput,
    mut outcome: ResMut<Outcome>,
    page: Option<Res<ResultsPage>>,
    mut championship: ResMut<Championship>,
    mut current: ResMut<CurrentCircuit>,
    circuits: Res<Circuits>,
    mut next: ResMut<NextState<GameState>>,
    mut sounds: MessageWriter<MenuSound>,
) {
    let page = page.map_or(ResultsPage::Race, |page| *page);
    if page == ResultsPage::Leaving {
        return;
    }
    let request = input.read();
    if request.back {
        sounds.write(MenuSound::Move);
        commands.insert_resource(ResultsPage::Leaving);
        next.set(back_from(outcome.mode));
        return;
    }
    if !(request.confirm || request.start) {
        return;
    }
    sounds.write(MenuSound::Confirm);
    // Going on from the race's results makes them final, and the
    // championship scores them.
    outcome.settle(&mut championship);
    // On the way out the results go at once: they are not built again
    // while the next screen (or the next track) is set up.
    match next_after(
        outcome.mode,
        page == ResultsPage::Standings,
        &championship,
        circuits.len(),
    ) {
        Next::Standings => commands.insert_resource(ResultsPage::Standings),
        Next::Race(round) => {
            commands.insert_resource(ResultsPage::Leaving);
            current.0 = round;
            next.set(GameState::Race);
        }
        Next::Menu => {
            commands.insert_resource(ResultsPage::Leaving);
            next.set(GameState::Welcome);
        }
        Next::Again => {
            commands.insert_resource(ResultsPage::Leaving);
            next.set(GameState::Race);
        }
    }
}

#[derive(Component)]
struct ResultsScreen;

#[allow(clippy::too_many_arguments)]
fn show_results(
    mut commands: Commands,
    outcome: Res<Outcome>,
    page: Option<Res<ResultsPage>>,
    championship: Res<Championship>,
    (circuits, roster): (Res<Circuits>, Res<Roster>),
    settings: Res<Settings>,
    device: Res<InputDevice>,
    active_controller: Option<Res<ActiveController>>,
    font: Res<UiFont>,
    icons: Res<UiIcons>,
    shown: Query<Entity, With<ResultsScreen>>,
    mut last: Local<Option<String>>,
) {
    let lang = settings.language;
    let page = page.map_or(ResultsPage::Race, |page| *page);
    let active_dev = current_active_device(*device, active_controller.as_deref());
    let wanted = format!(
        "{page:?} {lang:?} {active_dev:?} {:?} {} {}",
        outcome.places, outcome.settled, settings.centre_results
    );
    if last.as_ref() == Some(&wanted) && !shown.is_empty() {
        return;
    }
    *last = Some(wanted);
    for entity in &shown {
        commands.entity(entity).despawn();
    }
    if page == ResultsPage::Leaving {
        return;
    }
    let standings = page == ResultsPage::Standings;

    let circuit = &circuits[outcome.track];
    let track = circuit.name(lang);
    let title = if standings {
        let round = championship.round.min(circuits.len());
        let over = championship.is_over(circuits.len());
        match lang {
            Language::Spanish if over => "CLASIFICACIÓN FINAL".to_string(),
            Language::English if over => "FINAL STANDINGS".to_string(),
            Language::Spanish => format!("CLASIFICACIÓN  ·  {round}/{}", circuits.len()),
            Language::English => format!("STANDINGS  ·  {round}/{}", circuits.len()),
        }
    } else if outcome.mode == Mode::Checkpoint {
        match lang {
            Language::Spanish => "¡TIEMPO AGOTADO!".to_string(),
            Language::English => "TIME UP!".to_string(),
        }
    } else {
        match lang {
            Language::Spanish => format!("RESULTADOS  ·  {track}"),
            Language::English => format!("RESULTS  ·  {track}"),
        }
    };
    let lines = if standings {
        standing_lines(&roster, &outcome, &championship, lang)
    } else if outcome.mode == Mode::Checkpoint {
        checkpoint_lines(&outcome, &settings, circuit.name, track, lang)
    } else {
        race_lines(&roster, &outcome, lang)
    };
    let next = next_after(outcome.mode, standings, &championship, circuits.len());
    let go_on = match (next, lang) {
        (Next::Standings, Language::Spanish) => "Continuar",
        (Next::Standings, Language::English) => "Continue",
        (Next::Race(_), Language::Spanish) => "Siguiente carrera",
        (Next::Race(_), Language::English) => "Next race",
        (Next::Menu, Language::Spanish) => "Terminar",
        (Next::Menu, Language::English) => "Finish",
        (Next::Again, Language::Spanish) => "Repetir",
        (Next::Again, Language::English) => "Race again",
    };
    let prompts = menu_prompts(
        active_dev,
        &[
            (Keys::Confirm, go_on),
            (
                Keys::Back,
                match (back_from(outcome.mode), lang) {
                    (GameState::Modes, Language::Spanish) => "Modos de juego",
                    (GameState::Modes, Language::English) => "Game modes",
                    (_, Language::Spanish) => "Volver",
                    (_, Language::English) => "Back",
                },
            ),
        ],
    );

    // The race's results stand at the top right, over the race that goes
    // on behind them; if the options say so, once everybody is in, in the
    // middle, above the bike the camera shows. The standings, in the
    // middle, over a darker screen.
    let stand = if standings {
        Stand::Middle
    } else if settings.centre_results && everyone_in(&outcome) {
        Stand::Above
    } else {
        Stand::Corner
    };
    commands
        .spawn((
            ResultsScreen,
            DespawnOnExit(GameState::Race),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: match stand {
                    Stand::Corner => JustifyContent::FlexStart,
                    _ => JustifyContent::Center,
                },
                align_items: match stand {
                    Stand::Corner => AlignItems::FlexEnd,
                    _ => AlignItems::Center,
                },
                padding: match stand {
                    Stand::Middle => UiRect::bottom(px(56)),
                    // (A share of the height: one in per cent is of the width.)
                    Stand::Above => UiRect::bottom(Val::Vh(26.0)),
                    Stand::Corner => UiRect {
                        top: px(40),
                        right: px(28),
                        ..default()
                    },
                },
                ..default()
            },
            BackgroundColor(if standings {
                Color::srgba(0.0, 0.02, 0.05, 0.6)
            } else {
                Color::NONE
            }),
            GlobalZIndex(90),
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: px(if standings { 640 } else { 520 }),
                        max_width: percent(94),
                        padding: UiRect::axes(px(26), px(20)),
                        row_gap: px(6),
                        border_radius: BorderRadius::all(px(18)),
                        border: UiRect::all(px(2)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.02, 0.04, 0.1, 0.86)),
                    BorderColor::all(GOLD.with_alpha(0.6)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new(title),
                        text_font(&font, 28.0),
                        TextColor(GOLD),
                        TextShadow::default(),
                        Node {
                            align_self: AlignSelf::Center,
                            margin: UiRect::bottom(px(10)),
                            ..default()
                        },
                    ));
                    for line in lines {
                        panel
                            .spawn((
                                Node {
                                    justify_content: JustifyContent::SpaceBetween,
                                    column_gap: px(18),
                                    padding: UiRect::axes(px(12), px(4)),
                                    border_radius: BorderRadius::all(px(8)),
                                    ..default()
                                },
                                BackgroundColor(if line.player {
                                    PLAYER.with_alpha(0.18)
                                } else {
                                    Color::NONE
                                }),
                            ))
                            .with_children(|row| {
                                let color = if line.player { PLAYER } else { Color::WHITE };
                                row.spawn((
                                    Text::new(line.left),
                                    text_font(&font, 19.0),
                                    TextColor(color),
                                ));
                                row.spawn((
                                    Text::new(line.right),
                                    text_font(&font, 19.0),
                                    TextColor(if line.pending {
                                        color.with_alpha(0.55)
                                    } else {
                                        color
                                    }),
                                ));
                            });
                    }
                });
            spawn_legend(screen, &font, &icons, active_dev, &prompts);
        });
}

/// Where the results stand on the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stand {
    /// At the top right, while the others come in.
    Corner,
    /// In the middle, above the bike the camera shows, once all are in (if
    /// the options say so).
    Above,
    /// In the middle: the standings.
    Middle,
}

/// Whether every bike of the race is in: crossed the line or out (against
/// the clock, nobody comes in: it is over when the time is).
fn everyone_in(outcome: &Outcome) -> bool {
    outcome.mode == Mode::Checkpoint
        || outcome
            .places
            .iter()
            .all(|place| place.out || (place.time.is_some() && !place.estimated))
}

/// A line of the results: what goes on the left and on the right, whether
/// it is a player's, and whether it is still to come in (shown fainter).
struct Line {
    left: String,
    right: String,
    player: bool,
    pending: bool,
}

/// A bike's name, marked if it is a player's.
fn rider_name(roster: &Roster, rider: usize, outcome: &Outcome, lang: Language) -> (String, bool) {
    let name = roster[rider].name;
    match outcome.players.iter().position(|&p| p == rider) {
        Some(index) if outcome.players.len() > 1 => (format!("{name}  ·  {}P", index + 1), true),
        Some(_) => (
            format!(
                "{name}  ·  {}",
                match lang {
                    Language::Spanish => "TÚ",
                    Language::English => "YOU",
                }
            ),
            true,
        ),
        None => (name.to_string(), false),
    }
}

/// Each bike's place, time (or gap, or out) and, in a championship, the
/// points it gets. Those still racing are on their way; once the results
/// are final, they have the time they would have taken (marked "~").
fn race_lines(roster: &Roster, outcome: &Outcome, lang: Language) -> Vec<Line> {
    let winner = outcome.places.first().and_then(|place| place.time);
    outcome
        .places
        .iter()
        .enumerate()
        .map(|(index, place)| {
            let (name, player) = rider_name(roster, place.rider, outcome, lang);
            let pending = place.estimated && !outcome.settled;
            let result = if place.disqualified {
                match lang {
                    Language::Spanish => "descalificado".to_string(),
                    Language::English => "disqualified".to_string(),
                }
            } else if place.out {
                match lang {
                    Language::Spanish => "eliminado".to_string(),
                    Language::English => "out".to_string(),
                }
            } else {
                match (place.time, winner) {
                    _ if pending => match lang {
                        Language::Spanish => "en carrera".to_string(),
                        Language::English => "racing".to_string(),
                    },
                    (Some(time), first) => {
                        let mark = if place.estimated { "~" } else { "" };
                        match first {
                            Some(first) if index > 0 => format!("{mark}+{:.1} s", time - first),
                            _ => format!("{mark}{}", clock(time)),
                        }
                    }
                    (None, _) => match lang {
                        Language::Spanish => "en carrera".to_string(),
                        Language::English => "racing".to_string(),
                    },
                }
            };
            let points = if outcome.mode.is_championship() {
                let points = if place.disqualified {
                    0
                } else {
                    POINTS.get(index).copied().unwrap_or(0)
                };
                format!("   +{points}")
            } else {
                String::new()
            };
            Line {
                left: format!("{}   {name}", ordinal(index + 1)),
                right: format!("{result}{points}"),
                player,
                pending: pending || (place.time.is_none() && !place.out),
            }
        })
        .collect()
}

/// The championship's standings: each bike's place and points.
fn standing_lines(
    roster: &Roster,
    outcome: &Outcome,
    championship: &Championship,
    lang: Language,
) -> Vec<Line> {
    championship
        .standings()
        .into_iter()
        .enumerate()
        .map(|(index, rider)| {
            let (name, player) = rider_name(roster, rider, outcome, lang);
            Line {
                left: format!("{}   {name}", ordinal(index + 1)),
                right: format!("{} pts", championship.points[rider]),
                player,
                pending: false,
            }
        })
        .collect()
}

/// Against the clock: the laps done and the best on this track.
fn checkpoint_lines(
    outcome: &Outcome,
    settings: &Settings,
    name: &str,
    track: &str,
    lang: Language,
) -> Vec<Line> {
    let best = settings.records.checkpoint_best(name).unwrap_or(0);
    let lap = settings.records.best_lap(name);
    let mut lines = vec![
        Line {
            left: track.to_string(),
            right: String::new(),
            player: false,
            pending: false,
        },
        Line {
            left: match lang {
                Language::Spanish => "Vueltas".to_string(),
                Language::English => "Laps".to_string(),
            },
            right: if outcome.record {
                match lang {
                    Language::Spanish => format!("{}   ¡RÉCORD!", outcome.laps),
                    Language::English => format!("{}   RECORD!", outcome.laps),
                }
            } else {
                outcome.laps.to_string()
            },
            player: true,
            pending: false,
        },
        Line {
            left: match lang {
                Language::Spanish => "Récord de vueltas".to_string(),
                Language::English => "Most laps".to_string(),
            },
            right: best.to_string(),
            player: false,
            pending: false,
        },
    ];
    if let Some(lap) = lap {
        lines.push(Line {
            left: match lang {
                Language::Spanish => "Mejor vuelta".to_string(),
                Language::English => "Best lap".to_string(),
            },
            right: clock(lap),
            player: false,
            pending: false,
        });
    }
    lines
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    #[test]
    fn the_results_go_to_the_middle_once_everybody_is_in() {
        let place = |rider: usize, time: Option<f32>, estimated: bool| super::super::Place {
            rider,
            time,
            out: false,
            disqualified: false,
            estimated,
        };
        let mut outcome = Outcome {
            mode: Mode::Race,
            track: 0,
            places: vec![place(0, Some(90.0), false), place(7, Some(95.0), true)],
            players: vec![0],
            laps: 0,
            record: false,
            settled: false,
        };
        // One still racing (its time a guess): not yet.
        assert!(!everyone_in(&outcome));
        outcome.places[1] = place(7, None, false);
        assert!(!everyone_in(&outcome));
        // In, or out.
        outcome.places[1] = place(7, Some(96.0), false);
        assert!(everyone_in(&outcome));
        outcome.places[1].time = None;
        outcome.places[1].out = true;
        assert!(everyone_in(&outcome));
    }

    pub fn a_player_disqualified_says_so_in_the_results_on(roster: &'static crate::bike::Roster) {
        let place = |rider: usize, time: Option<f32>, out: bool| super::super::Place {
            rider,
            time,
            out,
            disqualified: out,
            estimated: false,
        };
        let mut outcome = Outcome {
            mode: Mode::Race,
            track: 0,
            places: vec![place(0, Some(90.0), false), place(7, None, true)],
            players: vec![7],
            laps: 0,
            record: false,
            settled: true,
        };
        let lines = race_lines(roster, &outcome, Language::Spanish);
        assert!(
            lines[1].right.starts_with("descalificado"),
            "{}",
            lines[1].right
        );
        assert!(lines[1].player && !lines[1].pending);
        let lines = race_lines(roster, &outcome, Language::English);
        assert!(lines[1].right.starts_with("disqualified"));
        // In the arcade, with no points.
        outcome.mode = Mode::Arcade;
        let lines = race_lines(roster, &outcome, Language::English);
        assert!(lines[1].right.ends_with("+0"), "{}", lines[1].right);
    }

    #[test]
    fn a_player_disqualified_says_so_in_the_results() {
        a_player_disqualified_says_so_in_the_results_on(&crate::testing::DEMO_ROSTER);
    }

    pub fn after_the_results_comes_what_the_mode_says_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut championship = Championship::default();
        // Out of a championship: the same race again, or back to the
        // tracks.
        assert_eq!(
            next_after(Mode::Race, false, &championship, circuits.len()),
            Next::Again
        );
        assert_eq!(
            next_after(Mode::Checkpoint, false, &championship, circuits.len()),
            Next::Again
        );
        assert_eq!(back_from(Mode::Race), GameState::Tracks);
        assert_eq!(back_from(Mode::Arcade), GameState::Modes);
        // A championship: the standings, then its next race...
        assert_eq!(
            next_after(Mode::Arcade, false, &championship, circuits.len()),
            Next::Standings
        );
        championship.score(&[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(
            next_after(Mode::Arcade, true, &championship, circuits.len()),
            Next::Race(1)
        );
        // ...and after the last one, the menu.
        championship.round = circuits.len();
        assert_eq!(
            next_after(Mode::Arcade, true, &championship, circuits.len()),
            Next::Menu
        );
        // An elimination is on the track chosen, like a race.
        assert_eq!(
            next_after(Mode::Elimination, false, &championship, circuits.len()),
            Next::Again
        );
    }

    /// The results of an arcade race, on the screen.
    fn results() -> App {
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
            .init_resource::<Championship>()
            .init_resource::<CurrentCircuit>()
            .insert_resource(*crate::testing::DEMO)
            .insert_resource(*crate::testing::DEMO_ROSTER)
            .add_message::<MenuSound>()
            .add_plugins((ResultsPlugin, crate::gamepads::GamepadsPlugin))
            .add_systems(OnExit(GameState::Race), |mut commands: Commands| {
                commands.remove_resource::<Outcome>();
            });
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Race);
        app.update();
        let place = |rider: usize, time: f32| super::super::Place {
            rider,
            time: Some(time),
            out: false,
            disqualified: false,
            estimated: false,
        };
        app.insert_resource(Outcome {
            mode: Mode::Arcade,
            track: 0,
            places: (0..8)
                .map(|rider| place(rider, 100.0 + rider as f32))
                .collect(),
            players: vec![7],
            laps: 0,
            record: false,
            settled: false,
        });
        app.update();
        app
    }

    fn press_enter(app: &mut App) {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        }
    }

    fn screens(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<Entity, With<ResultsScreen>>()
            .iter(app.world())
            .count()
    }

    #[test]
    fn the_results_go_away_for_the_next_race() {
        let mut app = results();
        assert_eq!(screens(&mut app), 1);
        // Continue: the race is scored and the standings are shown.
        press_enter(&mut app);
        assert_eq!(app.world().resource::<Championship>().round, 1);
        assert!(app.world().resource::<Outcome>().settled);
        assert_eq!(screens(&mut app), 1);
        // Next race: the results go at once, and do not come back.
        press_enter(&mut app);
        assert_eq!(screens(&mut app), 0);
        // The next circuit (round the engine's one, back to it).
        let circuits = app.world().resource::<Circuits>().len();
        assert_eq!(app.world().resource::<CurrentCircuit>().0, 1 % circuits);
        app.update();
        app.update();
        assert_eq!(screens(&mut app), 0);
        assert_eq!(app.world().resource::<Championship>().round, 1);
    }
}
