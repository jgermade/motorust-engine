//! What the race shows on the screen for each player: their place, their
//! lap, the clock, the time of the lap under way and their best lap; against the clock, the time left and
//! what each checkpoint gives; in elimination, who is out (and how many are
//! still in, which pops as it changes); and their place when they cross the
//! line at the end. Once the players' race is over,
//! which bike the camera watches come in, and its place when it does.

use bevy::prelude::*;

use super::{Mode, Outcome, Progress, Rules, Standing, Watching};
use crate::bike::{LocalPlayer, Player, Race, Retired, Rider, Roster};
use crate::geometry::smoothstep;
use crate::hud::UiFont;
use crate::locale::Language;
use crate::menu::GameState;
use crate::settings::Settings;
use crate::ui::DESIGN_WINDOW;
use crate::welcome::GameMode;

/// Seconds a checkpoint's bonus and who is out are shown for.
const NOTICE: f32 = 2.5;
const GOLD: Color = Color::srgb(1.0, 0.82, 0.2);
const RED: Color = Color::srgb(1.0, 0.3, 0.25);
/// How big the place and the lap are (px), the place the biggest, and the
/// smaller words and totals round them ("1 / 8", "VUELTA 2 / 5"); and the
/// times under them.
const PLACE_SIZE: f32 = 88.0;
const LAP_SIZE: f32 = 40.0;
const TOTAL_SIZE: f32 = 24.0;
/// How big the total after the place is (" / 8"), up at its top.
const PLACE_TOTAL_SIZE: f32 = 30.0;
const TIMES_SIZE: f32 = 17.0;
/// The times' colour; and how big the last lap's is when it beat the
/// track's record (in gold).
const TIMES_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.85);
const RECORD_SIZE: f32 = 21.0;
/// How opaque what is not a figure is at the top left ("º", "/", "VUELTA",
/// "TOTAL"), as a share of the figures'.
const WORDS: f32 = 0.55;
/// How far above the bottom of its line the HUD's font (Russo One) sits on
/// its baseline, as a share of its size: a number and its total, side by
/// side at the bottom, sit on the same baseline lifting the total by the
/// difference. And how far below the top of its line its figures reach up
/// to (0.926 of its size over the baseline in a line 1.2 tall, figures 0.71
/// tall): a total at the top of its number is lowered by the difference.
const BASELINE: f32 = 0.28;
const FIGURES_TOP: f32 = 0.21;
const FIGURES_HEIGHT: f32 = 0.71;
/// How much of the room under the place's figures (their font's descent)
/// the lap under it takes up.
const UNDER_PLACE: f32 = 0.2;
/// The total after the place: faint; in an elimination, once someone is out,
/// yellow, going orange as fewer are left.
const FAINT: Color = Color::srgba(1.0, 1.0, 1.0, WORDS);
const YELLOW: Color = Color::srgb(1.0, 0.86, 0.2);
const ORANGE: Color = Color::srgb(1.0, 0.5, 0.12);
/// Where the line of the notices in the middle starts, down the player's
/// half of the screen (or all of it), in percent.
const NOTICE_TOP: f32 = 30.0;
/// Where the middle of the false start notice's letters goes instead, down
/// the player's half (or all of it), in percent: between the start lights
/// (their bottom about 22.6 % down the screen) and the top of the start's
/// arch on the grid (about 30 % down), which it would hide at the notices'
/// own place.
const FALSE_START_MIDDLE: f32 = 26.1;
/// Seconds the total pops for when it changes, and how much bigger it gets.
const POP: f32 = 0.8;
const POP_GROWTH: f32 = 0.6;

pub struct RaceHudPlugin;

impl Plugin for RaceHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Race), spawn_race_info)
            .add_systems(
                Update,
                (
                    (show_race_info, show_place_total, show_figures).chain(),
                    show_watched,
                    show_arrivals,
                )
                    .run_if(in_state(GameState::Race)),
            );
    }
}

/// A text of the race for a player (0 or 1).
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
struct Info {
    player: usize,
    kind: InfoKind,
}

/// How far up the false start notice goes from the notices' place (see
/// [`FALSE_START_MIDDLE`]): its node's offset then.
#[derive(Component)]
struct FalseStartLift(Val);

/// How far up (in percent of the window's height) the false start notice,
/// in letters `size` big, goes in a half of the screen `height` percent of
/// the window tall: its letters' middle from where it is at the notices'
/// place to [`FALSE_START_MIDDLE`], at the window's design size (the
/// interface grows and shrinks with the window).
fn false_start_lift(size: f32, height: f32) -> f32 {
    let half = DESIGN_WINDOW.y * height / 100.0;
    let middle = (FIGURES_TOP + FIGURES_HEIGHT / 2.0) * size / half * 100.0;
    (NOTICE_TOP + middle - FALSE_START_MIDDLE) * height / 100.0
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum InfoKind {
    Place,
    /// The word before the lap ("VUELTA"), the lap, big, and how many laps
    /// there are after it ("/ 5").
    LapWord,
    Lap,
    Laps,
    /// The race's whole time, if the options say so.
    Clock,
    /// The time of the lap under way, and of the last one.
    LapTime,
    /// Against the clock, the time left; or a big notice in the middle.
    TimeLeft,
    Notice,
}

/// A text about the bike the camera watches come in: who it is and how it
/// is getting on, or, as it crosses the line, its place.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum WatchInfo {
    Caption,
    Notice,
}

/// Top right, under the performance HUD: who has crossed the line at the
/// end, in order, and when.
#[derive(Component)]
struct Arrivals;

/// A text at the top left whose figures stand out: what is not a figure is
/// drawn translucent. Its text and colour are written here, and
/// [`show_figures`] lays them out in spans under it, with its font.
#[derive(Component, Clone, Default, PartialEq)]
struct Figured {
    text: String,
    color: Color,
    /// The colour of what is not a figure, where it is not the figures'
    /// own, translucent.
    words: Option<Color>,
}

/// The runs of `text` that are figures (digits, and a colon or a point
/// between two of them, as in "1:23.4") and those that are not, in order.
fn figure_runs(text: &str) -> Vec<(String, bool)> {
    let chars: Vec<char> = text.chars().collect();
    let digit = |index: Option<usize>| {
        index
            .and_then(|i| chars.get(i))
            .is_some_and(char::is_ascii_digit)
    };
    let mut runs: Vec<(String, bool)> = Vec::new();
    for (index, &c) in chars.iter().enumerate() {
        let figure = c.is_ascii_digit()
            || (matches!(c, ':' | '.') && digit(index.checked_sub(1)) && digit(Some(index + 1)));
        match runs.last_mut() {
            Some((run, was)) if *was == figure => run.push(c),
            _ => runs.push((c.to_string(), figure)),
        }
    }
    runs
}

/// Each [`Figured`] text laid out in spans, one for each run of figures or
/// of what is not, the latter translucent, all in the text's font.
#[allow(clippy::type_complexity)]
fn show_figures(
    mut commands: Commands,
    mut texts: Query<
        (Entity, &Figured, &TextFont, Option<&Children>, &mut Text),
        Or<(Changed<Figured>, Changed<TextFont>)>,
    >,
    mut spans: Query<(&mut TextSpan, &mut TextColor, &mut TextFont), Without<Figured>>,
) {
    for (text, figured, font, children, mut root) in &mut texts {
        let runs = figure_runs(&figured.text);
        let count = runs.len();
        let children: Vec<Entity> = children.map_or(Vec::new(), |c| c.iter().collect());
        for (index, (run, figure)) in runs.into_iter().enumerate() {
            let color = match (figure, figured.words) {
                (true, _) => figured.color,
                (false, Some(words)) => words,
                (false, None) => figured.color.with_alpha(figured.color.alpha() * WORDS),
            };
            match children
                .get(index)
                .and_then(|&span| spans.get_mut(span).ok())
            {
                Some((mut span, mut tint, mut span_font)) => {
                    if span.0 != run {
                        span.0 = run;
                    }
                    tint.set_if_neq(TextColor(color));
                    if *span_font != *font {
                        *span_font = font.clone();
                    }
                }
                None => {
                    commands.entity(text).with_child((
                        TextSpan::new(run),
                        font.clone(),
                        TextColor(color),
                    ));
                }
            }
        }
        for &extra in children.iter().skip(count) {
            commands.entity(extra).despawn();
        }
        // With its last span gone the text has no children to say it
        // changed, and the interface would go on drawing what it said
        // (the place and the lap stayed up after the race, under the name
        // of the bike the camera watches): the text itself says so.
        if count == 0 && !children.is_empty() {
            root.set_changed();
        }
    }
}

/// After a player's place, how many there are ("/ 8"), and since when (s,
/// real time) the total it shows changed, to pop.
#[derive(Component, Debug, Default)]
struct PlaceTotal {
    player: usize,
    shown: Option<usize>,
    changed: Option<f32>,
}

fn spawn_race_info(
    mut commands: Commands,
    font: Res<UiFont>,
    game_mode: Res<GameMode>,
    rules: Res<Rules>,
) {
    let players = if *game_mode == GameMode::SplitScreen {
        2
    } else {
        1
    };
    let text = |size: f32| TextFont {
        font: font.0.clone().into(),
        font_size: FontSize::Px(size),
        ..default()
    };
    // Top right, under the frame rate, with no panel behind: those in.
    commands.spawn((
        DespawnOnExit(GameState::Race),
        Arrivals,
        Text::new(""),
        text(17.0),
        TextColor(Color::WHITE),
        TextLayout::justify(Justify::Right),
        TextShadow {
            offset: Vec2::splat(1.5),
            color: Color::srgba(0.0, 0.0, 0.0, 0.85),
        },
        Node {
            position_type: PositionType::Absolute,
            top: px(40),
            right: px(16),
            ..default()
        },
    ));
    // Over the whole screen, left of the results: the bike watched.
    commands
        .spawn((
            DespawnOnExit(GameState::Race),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(60),
                height: percent(100),
                ..default()
            },
        ))
        .with_children(|left| {
            left.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(18),
                top: px(14),
                ..default()
            })
            .with_child((
                WatchInfo::Caption,
                Text::new(""),
                text(26.0),
                TextColor(Color::WHITE),
                TextShadow::default(),
            ));
            left.spawn(Node {
                position_type: PositionType::Absolute,
                top: percent(30),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_child((
                WatchInfo::Notice,
                Text::new(""),
                text(44.0),
                TextColor(GOLD),
                TextLayout::justify(Justify::Center),
                TextShadow {
                    offset: Vec2::splat(3.0),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.7),
                },
            ));
        });
    for player in 0..players {
        // Each player's half of the screen (or all of it).
        let (top, height) = if players == 1 {
            (0.0, 100.0)
        } else {
            (50.0 * player as f32, 50.0)
        };
        commands
            .spawn((
                DespawnOnExit(GameState::Race),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: percent(top),
                    width: percent(100),
                    height: percent(height),
                    ..default()
                },
            ))
            .with_children(|half| {
                // Top left: place, lap, the lap's times and the race's.
                half.spawn(Node {
                    position_type: PositionType::Absolute,
                    left: px(18),
                    top: px(12),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    ..default()
                })
                .with_children(|column| {
                    let info = |kind, size, color| {
                        (
                            Info { player, kind },
                            Text::new(""),
                            Figured { color, ..default() },
                            text(size),
                            TextColor(color),
                            TextShadow::default(),
                        )
                    };
                    // The place, and how many there are after it, apart,
                    // to pop without moving the rest. None against the
                    // clock, nor the room for it.
                    if rules.mode != Mode::Checkpoint {
                        let mut row = on_top();
                        row.margin.bottom = px(-UNDER_PLACE * PLACE_SIZE);
                        column.spawn(row).with_children(|row| {
                            row.spawn(info(InfoKind::Place, PLACE_SIZE, Color::WHITE));
                            row.spawn((
                                PlaceTotal {
                                    player,
                                    ..default()
                                },
                                up_at(PLACE_SIZE, PLACE_TOTAL_SIZE),
                                Text::new(""),
                                Figured::default(),
                                text(PLACE_TOTAL_SIZE),
                                TextColor(Color::WHITE),
                                TextShadow::default(),
                                UiTransform::IDENTITY,
                            ));
                        });
                    }
                    // The lap, smaller than the place, between its word and
                    // how many laps there are.
                    column.spawn(beside()).with_children(|row| {
                        let mut word = after(LAP_SIZE, TOTAL_SIZE);
                        word.margin.right = px(TOTAL_SIZE / 4.0);
                        row.spawn((info(InfoKind::LapWord, TOTAL_SIZE, Color::WHITE), word));
                        row.spawn(info(InfoKind::Lap, LAP_SIZE, Color::WHITE));
                        row.spawn((
                            info(InfoKind::Laps, TOTAL_SIZE, Color::WHITE),
                            after(LAP_SIZE, TOTAL_SIZE),
                        ));
                    });
                    for kind in [InfoKind::LapTime, InfoKind::Clock] {
                        column.spawn(info(kind, TIMES_SIZE, TIMES_COLOR));
                    }
                });
                // Top middle: the time left against the clock.
                half.spawn(Node {
                    position_type: PositionType::Absolute,
                    top: px(10),
                    width: percent(100),
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_child((
                    Info {
                        player,
                        kind: InfoKind::TimeLeft,
                    },
                    Text::new(""),
                    text(46.0),
                    TextLayout::justify(Justify::Center),
                    TextShadow::default(),
                ));
                // In the middle: notices.
                let size = if players == 1 { 52.0 } else { 36.0 };
                half.spawn(Node {
                    position_type: PositionType::Absolute,
                    top: percent(NOTICE_TOP),
                    width: percent(100),
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_child((
                    Info {
                        player,
                        kind: InfoKind::Notice,
                    },
                    FalseStartLift(Val::Vh(-false_start_lift(size, height))),
                    Text::new(""),
                    text(size),
                    TextLayout::justify(Justify::Center),
                    TextShadow {
                        offset: Vec2::splat(3.0),
                        color: Color::srgba(0.0, 0.0, 0.0, 0.7),
                    },
                ));
            });
    }
}

/// A row of a number and, after it, a smaller total ("/ 8"), on the same
/// baseline.
fn beside() -> Node {
    Node {
        align_items: AlignItems::FlexEnd,
        ..default()
    }
}

/// The node of a total `size` px big after a number `number` px big:
/// lifted onto the number's baseline.
fn after(number: f32, size: f32) -> Node {
    Node {
        margin: UiRect::bottom(px(BASELINE * (number - size))),
        ..default()
    }
}

/// A row of a number and, after it, a smaller total (" / 8"), their tops
/// in line.
fn on_top() -> Node {
    Node {
        align_items: AlignItems::FlexStart,
        ..default()
    }
}

/// The node of a total `size` px big after a number `number` px big:
/// lowered for the tops of their figures to be in line.
fn up_at(number: f32, size: f32) -> Node {
    Node {
        margin: UiRect::top(px(FIGURES_TOP * (number - size))),
        ..default()
    }
}

/// The colour of the total after a player's place, among `racers`, `out`
/// of them out (the player among them or not, `player_out`): faint; but in
/// an elimination, once someone is out and while the player is still in,
/// yellow, going orange as fewer are left, all orange with the last two.
fn total_color(mode: Mode, racers: usize, out: usize, player_out: bool) -> Color {
    if mode != Mode::Elimination || out == 0 || player_out {
        return FAINT;
    }
    // Out with the last two left.
    let last = racers.saturating_sub(2).max(1);
    let t = if last <= 1 {
        1.0
    } else {
        (out - 1) as f32 / (last - 1) as f32
    };
    YELLOW.mix(&ORANGE, t.clamp(0.0, 1.0))
}

/// A time as minutes, seconds and tenths: 1:23.4.
pub fn clock(seconds: f32) -> String {
    let tenths = (seconds.max(0.0) * 10.0).floor() as u32;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

/// The lap's time: on the first lap, the time of the lap under way, with
/// no word (`seconds`, "0:31.2"); from the second on, the last lap's
/// instead, said so ("ÚLTIMA 1:13.9", "LAST 1:13.9").
fn lap_times(seconds: f32, last: Option<f32>, lang: Language) -> String {
    match last {
        Some(last) => {
            let word = match lang {
                Language::Spanish => "ÚLTIMA",
                Language::English => "LAST",
            };
            format!("{word} {}", clock(last))
        }
        None => clock(seconds),
    }
}

/// A place, in either language: 1º, 2º, 8º...
pub fn ordinal(place: usize) -> String {
    format!("{place}º")
}

/// Whether the last lap of a bike that has made `progress` is the one that
/// beat the track's record in this race.
fn last_is_record(standing: &Standing, progress: &Progress) -> bool {
    standing
        .record
        .is_some_and(|(time, _)| progress.last_lap == Some(time))
}

/// A bike's place in the race (`rider`, of the roster) and how many bikes
/// it is among: all of them; but in elimination, while it is still in,
/// only those still in, so that the count comes down as they go out
/// ("2º / 6" with two of eight out). One out keeps its place among them
/// all.
fn place_among(standing: &Standing, mode: Mode, rider: usize) -> (usize, usize) {
    let racers = standing.order.len();
    let place = standing
        .order
        .iter()
        .position(|&r| r == rider)
        .map_or(racers, |p| p + 1);
    let among = if mode == Mode::Elimination && !standing.out.contains(&rider) {
        racers.saturating_sub(standing.out.len())
    } else {
        racers
    };
    (place, among)
}

#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
fn show_race_info(
    rules: Res<Rules>,
    roster: Res<Roster>,
    race: Res<Race>,
    standing: Res<Standing>,
    settings: Res<Settings>,
    outcome: Option<Res<Outcome>>,
    bikes: Query<(
        &Rider,
        &Progress,
        Has<Player>,
        Option<&LocalPlayer>,
        Has<Retired>,
    )>,
    mut texts: Query<(
        &Info,
        &mut Text,
        &mut TextColor,
        &mut TextFont,
        Option<&mut Figured>,
        Option<(&mut Node, &FalseStartLift)>,
    )>,
) {
    let lang = settings.language;
    let target = rules.mode.laps(rules.laps);
    for (info, mut text, mut color, mut font, figured, lift) in &mut texts {
        let Some((rider, progress, _, _, retired)) =
            bikes.iter().find(|(_, _, player, local, _)| {
                let index = local.map_or(usize::from(!*player), |local| local.0);
                index == info.player
            })
        else {
            continue;
        };
        let (place, _) = place_among(&standing, rules.mode, rider.0);
        let (wanted, tint) = match info.kind {
            // Over for the players: the results say it all.
            _ if outcome.is_some() => (String::new(), color.0),
            InfoKind::Place if rules.mode != Mode::Checkpoint => (place.to_string(), color.0),
            InfoKind::LapWord => (
                match lang {
                    Language::Spanish => "VUELTA".into(),
                    Language::English => "LAP".into(),
                },
                color.0,
            ),
            InfoKind::Lap => {
                let now = progress.laps + 1;
                let now = target.map_or(now, |laps| now.min(laps));
                (now.to_string(), color.0)
            }
            InfoKind::Laps => (
                target.map_or(String::new(), |laps| format!(" / {laps}")),
                color.0,
            ),
            // "TOTAL" in either language, if the options say so.
            InfoKind::Clock if settings.show_total_time => {
                (format!("TOTAL {}", clock(standing.clock)), color.0)
            }
            // The lap under way, until the line at the end.
            // The last lap in gold, bigger, if it beat the track's record.
            InfoKind::LapTime => {
                let record = last_is_record(&standing, progress);
                let size = FontSize::Px(if record { RECORD_SIZE } else { TIMES_SIZE });
                if font.font_size != size {
                    font.font_size = size;
                }
                (
                    if progress.finished.is_some() {
                        String::new()
                    } else {
                        lap_times(progress.lap_time(standing.clock), progress.last_lap, lang)
                    },
                    if record { GOLD } else { TIMES_COLOR },
                )
            }
            InfoKind::TimeLeft if rules.mode == Mode::Checkpoint => {
                let left = standing.time_left;
                let bonus = standing
                    .bonus
                    .filter(|(_, at)| standing.clock - at < NOTICE)
                    .map_or(String::new(), |(bonus, _)| format!("   +{bonus:.1}"));
                (
                    format!("{:.1}{bonus}", left),
                    if left < 10.0 { RED } else { GOLD },
                )
            }
            InfoKind::Notice => {
                let out = match (retired, standing.disqualified.contains(&rider.0)) {
                    (_, true) => Out::Disqualified,
                    (true, false) => Out::Retired,
                    (false, false) => Out::In,
                };
                // Back on the grid after a false start: how many so far.
                let made = standing
                    .false_starts
                    .iter()
                    .filter(|&&r| r == rider.0)
                    .count() as u32;
                let false_starts =
                    (!race.started && made > 0).then_some((made, settings.false_starts));
                // Back on the grid, up between the lights and the arch.
                if let Some((mut node, lift)) = lift {
                    let top = if false_starts.is_some() && out == Out::In {
                        lift.0
                    } else {
                        Val::ZERO
                    };
                    if node.top != top {
                        node.top = top;
                    }
                }
                notice(
                    &rules,
                    &roster,
                    &standing,
                    progress,
                    out,
                    false_starts,
                    place,
                    lang,
                )
            }
            _ => (String::new(), color.0),
        };
        // At the top left, laid out by `show_figures`.
        if let Some(mut figured) = figured {
            figured.set_if_neq(Figured {
                text: wanted,
                color: tint,
                words: None,
            });
            continue;
        }
        if text.0 != wanted {
            text.0 = wanted;
        }
        if color.0 != tint {
            color.0 = tint;
        }
    }
}

/// How much the total after a place pops (0 to 1) `elapsed` s after it
/// changed: at once all the way, and back over [`POP`].
pub fn pop(elapsed: f32) -> f32 {
    if !(0.0..POP).contains(&elapsed) {
        return 0.0;
    }
    let back = 1.0 - elapsed / POP;
    smoothstep(0.0, 0.06, elapsed) * back * back
}

/// After each player's place, how many there are (" / 8"; in elimination,
/// how many are still in), faint, or in elimination in the colour of how
/// few are left (see [`total_color`]), popping bigger and red as it
/// changes: scaled from its top left corner, so nothing round it moves.
#[allow(clippy::type_complexity)]
fn show_place_total(
    time: Res<Time<Real>>,
    rules: Res<Rules>,
    standing: Res<Standing>,
    outcome: Option<Res<Outcome>>,
    bikes: Query<(&Rider, Has<Player>, Option<&LocalPlayer>)>,
    mut totals: Query<(
        &mut PlaceTotal,
        &mut Figured,
        &mut UiTransform,
        &ComputedNode,
    )>,
) {
    let now = time.elapsed_secs();
    for (mut total, mut figured, mut transform, node) in &mut totals {
        let rider = bikes.iter().find_map(|(rider, player, local)| {
            let index = local.map_or(usize::from(!player), |local| local.0);
            (index == total.player).then_some(rider.0)
        });
        let among = rider
            .filter(|_| outcome.is_none() && rules.mode != Mode::Checkpoint)
            .map(|rider| place_among(&standing, rules.mode, rider).1);
        // A change of the total pops; its first showing does not.
        if among != total.shown {
            if total.shown.is_some_and(|shown| shown > 0) && among.is_some() {
                total.changed = Some(now);
            }
            total.shown = among;
        }
        let text = among.map_or(String::new(), |among| format!(" / {among}"));
        let popped = total.changed.map_or(0.0, |at| pop(now - at));
        let scale = 1.0 + POP_GROWTH * popped;
        let size = node.size() * node.inverse_scale_factor;
        let popping = UiTransform {
            translation: Val2::px(size.x * (scale - 1.0) / 2.0, size.y * (scale - 1.0) / 2.0),
            scale: Vec2::splat(scale),
            ..UiTransform::IDENTITY
        };
        if *transform != popping {
            *transform = popping;
        }
        let color = total_color(
            rules.mode,
            standing.order.len(),
            standing.out.len(),
            rider.is_some_and(|rider| standing.out.contains(&rider)),
        );
        figured.set_if_neq(Figured {
            text,
            color: color.mix(&RED, popped),
            words: Some(FAINT.mix(&RED.with_alpha(WORDS), popped)),
        });
    }
}

/// Who the camera watches come in: their place and lap, and as they cross
/// the line, their place and how far behind the winner.
#[allow(clippy::type_complexity)]
fn show_watched(
    rules: Res<Rules>,
    roster: Res<Roster>,
    standing: Res<Standing>,
    settings: Res<Settings>,
    watching: Option<Res<Watching>>,
    bikes: Query<&Progress>,
    mut texts: Query<(&WatchInfo, &mut Text)>,
) {
    let lang = settings.language;
    let watched = watching
        .as_deref()
        .and_then(|watching| Some((watching, bikes.get(watching.bike).ok()?)));
    let place = watched.and_then(|(watching, _)| {
        standing
            .order
            .iter()
            .position(|&rider| rider == watching.rider)
            .map(|index| index + 1)
    });
    // When the first one crossed the line.
    let winner = bikes
        .iter()
        .filter_map(|progress| progress.finished)
        .min_by(f32::total_cmp);
    for (info, mut text) in &mut texts {
        let wanted = match (info, watched, place) {
            (WatchInfo::Caption, Some((watching, progress)), Some(place)) => {
                let name = roster[watching.rider].name;
                let lap = match lang {
                    Language::Spanish => "VUELTA",
                    Language::English => "LAP",
                };
                match rules.mode.laps(rules.laps) {
                    Some(laps) if progress.finished.is_none() => format!(
                        "{name}  ·  {}  ·  {lap} {}/{laps}",
                        ordinal(place),
                        (progress.laps + 1).min(laps)
                    ),
                    _ => format!("{name}  ·  {}", ordinal(place)),
                }
            }
            (WatchInfo::Notice, Some((watching, progress)), Some(place)) => {
                match progress.finished {
                    Some(at) if standing.clock - at < NOTICE => {
                        let gap = winner
                            .filter(|first| at - first > 0.05)
                            .map_or(String::new(), |first| format!("  +{:.1} s", at - first));
                        let finish = match lang {
                            Language::Spanish => "¡META!",
                            Language::English => "FINISH!",
                        };
                        format!(
                            "{finish}  {}  {}{gap}",
                            roster[watching.rider].name,
                            ordinal(place)
                        )
                    }
                    _ => String::new(),
                }
            }
            _ => String::new(),
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// In a race or the arcade, those who have crossed the line at the end, as
/// they do: their place, name and time. Once the results are up, they say
/// it instead.
fn show_arrivals(
    rules: Res<Rules>,
    roster: Res<Roster>,
    standing: Res<Standing>,
    outcome: Option<Res<Outcome>>,
    bikes: Query<(&Rider, &Progress)>,
    mut text: Query<&mut Text, With<Arrivals>>,
) {
    let wanted = if matches!(rules.mode, Mode::Race | Mode::Arcade) && outcome.is_none() {
        arrival_lines(&roster, &standing.order, |rider| {
            bikes
                .iter()
                .find(|(r, _)| r.0 == rider)
                .and_then(|(_, progress)| progress.finished)
        })
    } else {
        String::new()
    };
    for mut text in &mut text {
        if text.0 != wanted {
            text.0.clone_from(&wanted);
        }
    }
}

/// A line for each bike of `order` (bikes of the roster) that has crossed
/// the line at the end (`finished`: when), in order: "1º  Name  1:23.4".
fn arrival_lines(
    roster: &Roster,
    order: &[usize],
    finished: impl Fn(usize) -> Option<f32>,
) -> String {
    let mut arrived: Vec<(usize, f32)> = order
        .iter()
        .filter_map(|&rider| finished(rider).map(|time| (rider, time)))
        .collect();
    arrived.sort_by(|a, b| a.1.total_cmp(&b.1));
    arrived
        .iter()
        .enumerate()
        .map(|(index, (rider, time))| {
            format!(
                "{}   {}   {}",
                ordinal(index + 1),
                roster[*rider].name,
                clock(*time)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a player is still in the race.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Out {
    In,
    Retired,
    /// For jumping the start.
    Disqualified,
}

/// The big notice in the middle for a player at `place`: their place when
/// they cross the line at the end, that their time is up, they are out or
/// disqualified, back on the grid after a false start (`false_starts`: how
/// many they have made and are let off), who has just gone out, or a lap
/// record.
#[allow(clippy::too_many_arguments)]
fn notice(
    rules: &Rules,
    roster: &Roster,
    standing: &Standing,
    progress: &Progress,
    out: Out,
    false_starts: Option<(u32, u32)>,
    place: usize,
    lang: Language,
) -> (String, Color) {
    if let Some((made, allowed)) = false_starts.filter(|_| out == Out::In) {
        return (
            match lang {
                Language::Spanish => format!("¡SALIDA NULA!  {made} de {allowed}"),
                Language::English => format!("FALSE START!  {made} of {allowed}"),
            },
            GOLD,
        );
    }
    if out == Out::Disqualified {
        return (
            match lang {
                Language::Spanish => "¡SALIDA ANTICIPADA!  DESCALIFICADO".into(),
                Language::English => "JUMP START!  DISQUALIFIED".into(),
            },
            RED,
        );
    }
    if progress.finished.is_some() {
        return (
            match lang {
                Language::Spanish => format!("¡META!  {}", ordinal(place)),
                Language::English => format!("FINISH!  {}", ordinal(place)),
            },
            GOLD,
        );
    }
    if rules.mode == Mode::Checkpoint && standing.time_left <= 0.0 {
        return (
            match lang {
                Language::Spanish => "¡TIEMPO!".into(),
                Language::English => "TIME UP!".into(),
            },
            RED,
        );
    }
    if out == Out::Retired {
        return (
            match lang {
                Language::Spanish => "¡ELIMINADO!".into(),
                Language::English => "ELIMINATED!".into(),
            },
            RED,
        );
    }
    if let Some((rider, at)) = standing.last_out
        && standing.clock - at < NOTICE
    {
        let name = roster[rider].name;
        return (
            match lang {
                Language::Spanish => format!("Eliminado: {name}"),
                Language::English => format!("Out: {name}"),
            },
            RED,
        );
    }
    // Just after the lap that beat it, to the player who rode it.
    if let Some((best, at)) = standing.record
        && progress.best_lap == Some(best)
        && standing.clock - at < NOTICE
    {
        return (
            match lang {
                Language::Spanish => format!("¡RÉCORD DE VUELTA!  {}", clock(best)),
                Language::English => format!("LAP RECORD!  {}", clock(best)),
            },
            GOLD,
        );
    }
    (String::new(), Color::WHITE)
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    /// What a player's texts at the top left say, put together from their
    /// spans.
    fn top_left(app: &mut App) -> String {
        let world = app.world_mut();
        let mut said = Vec::new();
        let mut texts = world.query::<(&Info, &Figured, Option<&Children>)>();
        let texts: Vec<(Info, Vec<Entity>)> = texts
            .iter(world)
            .map(|(info, _, children)| (*info, children.map_or(Vec::new(), |c| c.iter().collect())))
            .collect();
        for (info, spans) in texts {
            let text: String = spans
                .iter()
                .filter_map(|&span| world.get::<TextSpan>(span).map(|span| span.0.clone()))
                .collect();
            if !text.is_empty() {
                said.push(format!("{:?}={text}", info.kind));
            }
        }
        said.sort();
        said.join(" ")
    }

    #[test]
    fn a_false_start_notice_goes_up_between_the_lights_and_the_arch() {
        // The top of the start's arch on the grid, down the screen (or the
        // half): about 29.6 % on Bahía GP's grid, and a little more on others.
        const ARCH_TOP: f32 = 29.6;
        for (size, height) in [(52.0, 100.0), (36.0, 50.0)] {
            // Its letters, down the half (or the screen), in percent.
            let half = DESIGN_WINDOW.y * height / 100.0;
            let top = NOTICE_TOP - false_start_lift(size, height) * 100.0 / height;
            let letters = |share: f32| top + share * size / half * 100.0;
            let (first, last) = (letters(FIGURES_TOP), letters(FIGURES_TOP + FIGURES_HEIGHT));
            assert!(
                (letters(FIGURES_TOP + FIGURES_HEIGHT / 2.0) - FALSE_START_MIDDLE).abs() < 1e-3
            );
            assert!(last < ARCH_TOP, "{size}: {first}-{last} %");
            // On a screen of its own, under the lights (split, they are
            // across the middle).
            if height == 100.0 {
                assert!(
                    first > super::super::lights::panel_bottom() + 0.5,
                    "{first} %"
                );
            }
            // Up from the notices' own place, where the arch hid it.
            assert!(false_start_lift(size, height) > 0.0);
        }
    }

    pub fn the_top_left_clears_once_the_race_is_over_for_the_players_on(
        roster: &'static crate::bike::Roster,
    ) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Font>()
            .init_state::<GameState>()
            .init_resource::<Settings>()
            .init_resource::<UiFont>()
            .init_resource::<GameMode>()
            .insert_resource(*roster)
            .insert_resource(Rules {
                mode: Mode::Race,
                laps: 1,
            })
            .init_resource::<Race>()
            .insert_resource(Standing {
                order: (0..8).collect(),
                ..default()
            })
            .add_plugins(RaceHudPlugin);
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Race);
        app.update();
        let bike = app
            .world_mut()
            .spawn((Rider(7), Progress::default(), Player))
            .id();
        app.update();
        app.update();
        let racing = top_left(&mut app);
        assert!(racing.split(' ').any(|said| said == "Place=8"), "{racing}");
        assert!(racing.contains("LapTime="), "{racing}");
        // Over the line: the lap's time goes.
        app.world_mut().get_mut::<Progress>(bike).unwrap().finished = Some(28.8);
        app.update();
        app.update();
        let finished = top_left(&mut app);
        assert!(!finished.contains("LapTime="), "{finished}");
        // The results up: nothing left at the top left, for the bike the
        // camera watches to be named there.
        app.insert_resource(super::super::Outcome {
            mode: Mode::Race,
            track: 0,
            places: Vec::new(),
            players: vec![7],
            laps: 1,
            record: false,
            settled: false,
        });
        let before = app.world_mut().change_tick();
        app.update();
        app.update();
        assert_eq!(top_left(&mut app), "");
        // And each text emptied is to be drawn again, with nothing left in
        // it to say so.
        let world = app.world_mut();
        let now = world.change_tick();
        let mut texts = world.query_filtered::<(&Info, Ref<Text>), With<Figured>>();
        for (info, text) in texts.iter(world) {
            if matches!(
                info.kind,
                InfoKind::Place | InfoKind::Lap | InfoKind::LapWord
            ) {
                assert!(text.last_changed().is_newer_than(before, now), "{info:?}");
            }
        }
    }

    #[test]
    fn the_top_left_clears_once_the_race_is_over_for_the_players() {
        the_top_left_clears_once_the_race_is_over_for_the_players_on(&crate::testing::DEMO_ROSTER);
    }

    #[test]
    fn the_lap_under_way_reads_with_its_time_and_then_the_last_one() {
        assert_eq!(lap_times(31.25, None, Language::Spanish), "0:31.2");
        // Once a lap is done, the last one's instead.
        assert_eq!(
            lap_times(50.2, Some(73.9), Language::Spanish),
            "ÚLTIMA 1:13.9"
        );
        assert_eq!(
            lap_times(50.2, Some(73.9), Language::English),
            "LAST 1:13.9"
        );
        // Since the lap began, at the line.
        let progress = Progress {
            lap_start: 40.0,
            ..default()
        };
        assert_eq!(progress.lap_time(71.5), 31.5);
        assert_eq!(progress.lap_time(30.0), 0.0);
    }

    #[test]
    fn what_is_not_a_figure_is_told_apart() {
        let runs = figure_runs;
        let part = |run: &str, figure| (run.to_string(), figure);
        assert_eq!(runs("1º"), [part("1", true), part("º", false)]);
        assert_eq!(runs(" / 8"), [part(" / ", false), part("8", true)]);
        assert_eq!(runs("VUELTA"), [part("VUELTA", false)]);
        // A time is all figures, its colon and point too.
        assert_eq!(
            runs("TOTAL 5:50.2"),
            [part("TOTAL ", false), part("5:50.2", true)]
        );
        assert_eq!(runs("12º"), [part("12", true), part("º", false)]);
        assert!(runs("").is_empty());
    }

    #[test]
    fn times_read_as_minutes_seconds_and_tenths() {
        assert_eq!(clock(0.0), "0:00.0");
        assert_eq!(clock(83.47), "1:23.4");
        assert_eq!(clock(600.0), "10:00.0");
    }

    pub fn those_in_are_listed_by_when_they_crossed_the_line_on(
        roster: &'static crate::bike::Roster,
    ) {
        let finished = |rider: usize| match rider {
            3 => Some(83.4),
            1 => Some(81.2),
            _ => None,
        };
        let lines = arrival_lines(roster, &[1, 3, 0, 2], finished);
        assert_eq!(
            lines,
            format!(
                "1º   {}   1:21.2\n2º   {}   1:23.4",
                roster[1].name, roster[3].name
            )
        );
        assert_eq!(arrival_lines(roster, &[0, 2], finished), "");
    }

    #[test]
    fn those_in_are_listed_by_when_they_crossed_the_line() {
        those_in_are_listed_by_when_they_crossed_the_line_on(&crate::testing::DEMO_ROSTER);
    }

    #[test]
    fn the_last_lap_is_the_record_while_it_is_the_one_that_beat_it() {
        let standing = Standing {
            record: Some((70.0, 71.0)),
            ..default()
        };
        let lap = |last| Progress {
            laps: 2,
            last_lap: last,
            ..default()
        };
        assert!(last_is_record(&standing, &lap(Some(70.0))));
        // Another lap since, or another player's.
        assert!(!last_is_record(&standing, &lap(Some(72.5))));
        assert!(!last_is_record(&standing, &lap(None)));
        // No record beaten in this race.
        assert!(!last_is_record(&Standing::default(), &lap(Some(70.0))));
    }

    pub fn a_lap_record_is_told_once_just_after_the_lap_that_beat_it_on(
        roster: &'static crate::bike::Roster,
    ) {
        let rules = Rules {
            mode: Mode::Race,
            laps: 5,
        };
        let mut standing = Standing {
            clock: 72.0,
            record: Some((70.0, 71.0)),
            ..default()
        };
        let progress = Progress {
            laps: 1,
            best_lap: Some(70.0),
            ..default()
        };
        let told = |standing: &Standing, progress: &Progress| {
            notice(
                &rules,
                roster,
                standing,
                progress,
                Out::In,
                None,
                8,
                Language::English,
            )
            .0
            .starts_with("LAP RECORD")
        };
        assert!(told(&standing, &progress));
        // Not to the other player, whose best is another.
        let other = Progress {
            best_lap: Some(74.0),
            ..progress.clone()
        };
        assert!(!told(&standing, &other));
        // Laps later, slower ones: no more.
        standing.clock = 150.0;
        let later = Progress {
            laps: 2,
            ..progress.clone()
        };
        assert!(!told(&standing, &later));
    }

    #[test]
    fn a_lap_record_is_told_once_just_after_the_lap_that_beat_it() {
        a_lap_record_is_told_once_just_after_the_lap_that_beat_it_on(&crate::testing::DEMO_ROSTER);
    }

    pub fn back_on_the_grid_after_a_false_start_it_says_how_many_on(
        roster: &'static crate::bike::Roster,
    ) {
        let rules = Rules {
            mode: Mode::Race,
            laps: 5,
        };
        let standing = Standing::default();
        let progress = Progress::default();
        let said = |out, false_starts| {
            notice(
                &rules,
                roster,
                &standing,
                &progress,
                out,
                false_starts,
                8,
                Language::Spanish,
            )
            .0
        };
        assert_eq!(said(Out::In, Some((1, 2))), "¡SALIDA NULA!  1 de 2");
        assert_eq!(said(Out::In, None), "");
        // Disqualified says so instead.
        assert!(said(Out::Disqualified, Some((2, 2))).contains("DESCALIFICADO"));
    }

    #[test]
    fn back_on_the_grid_after_a_false_start_it_says_how_many() {
        back_on_the_grid_after_a_false_start_it_says_how_many_on(&crate::testing::DEMO_ROSTER);
    }

    #[test]
    fn in_elimination_a_place_is_among_those_still_in() {
        // Eight bikes, two of them out: 7 first, then 5.
        let standing = Standing {
            order: vec![3, 0, 1, 2, 4, 6, 5, 7],
            out: vec![7, 5],
            ..default()
        };
        assert_eq!(place_among(&standing, Mode::Elimination, 0), (2, 6));
        assert_eq!(place_among(&standing, Mode::Elimination, 6), (6, 6));
        // Those out keep their place among them all.
        assert_eq!(place_among(&standing, Mode::Elimination, 5), (7, 8));
        assert_eq!(place_among(&standing, Mode::Elimination, 7), (8, 8));
        // Nobody out yet: among them all.
        let start = Standing {
            out: Vec::new(),
            ..standing.clone()
        };
        assert_eq!(place_among(&start, Mode::Elimination, 0), (2, 8));
        // In a race, a bike out (disqualified) still counts.
        assert_eq!(place_among(&standing, Mode::Race, 0), (2, 8));
    }

    #[test]
    fn the_total_is_faint_but_goes_yellow_then_orange_as_an_elimination_goes_on() {
        // Faint in a race, and in an elimination with nobody out yet.
        assert_eq!(total_color(Mode::Race, 8, 0, false), FAINT);
        assert_eq!(total_color(Mode::Race, 8, 3, false), FAINT);
        assert_eq!(total_color(Mode::Elimination, 8, 0, false), FAINT);
        assert!(FAINT.alpha() < 1.0);
        // The first out: yellow, not faint; the last two left: orange; in
        // between, more orange the fewer are left.
        assert_eq!(total_color(Mode::Elimination, 8, 1, false), YELLOW);
        assert_eq!(total_color(Mode::Elimination, 8, 6, false), ORANGE);
        let green = |out| {
            total_color(Mode::Elimination, 8, out, false)
                .to_srgba()
                .green
        };
        for out in 1..6 {
            assert!(green(out + 1) < green(out), "{out}");
            assert_eq!(total_color(Mode::Elimination, 8, out, false).alpha(), 1.0);
        }
        // A player out counts among them all, faint.
        assert_eq!(total_color(Mode::Elimination, 8, 4, true), FAINT);
        // Three racing: the first out leaves the last two.
        assert_eq!(total_color(Mode::Elimination, 3, 1, false), ORANGE);
    }

    #[test]
    fn the_total_pops_at_once_and_settles_back() {
        assert_eq!(pop(-0.1), 0.0);
        assert!(pop(0.08) > 0.8, "at once");
        // Smaller and smaller, and gone after a moment.
        let mut last = pop(0.08);
        for k in 2..16 {
            let now = pop(0.05 * k as f32);
            assert!(now < last, "{k}");
            last = now;
        }
        assert_eq!(pop(POP), 0.0);
        assert_eq!(pop(5.0), 0.0);
    }

    #[test]
    fn places_read_alike_in_both_languages() {
        assert_eq!(ordinal(1), "1º");
        assert_eq!(ordinal(2), "2º");
        assert_eq!(ordinal(11), "11º");
    }
}
