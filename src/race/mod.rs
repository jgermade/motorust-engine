//! The race: what is being played ([`Mode`]), each bike's progress round
//! its laps ([`Progress`]), the order they are in, the clock, the
//! checkpoints, who is eliminated and when it is over; and the arcade
//! championship, in which the tracks are raced one after another
//! ([`Championship`]).
//!
//! When the players have crossed the line at the end, the machine rides
//! their bikes ([`crate::bike::Autopilot`]) and the camera watches the
//! others come in ([`Watching`]), while the results fill in.
//!
//! What it shows: the start lights ([`lights`]), the laps, places and times
//! on the screen ([`hud`]) and the results when the race is over
//! ([`results`]).

pub mod hud;
pub mod lights;
pub mod results;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::bike::{
    Autopilot, BikeState, LocalPlayer, Player, RIDERS, Race, RaceControls, Retired, Rider,
    autopilot,
};
use crate::circuits::{Circuit, Circuits, CurrentCircuit};
use crate::menu::GameState;
use crate::pause;
use crate::settings::{Action, Difficulty, Settings};
use crate::track::{Nearest, Track};

/// How many laps a race of the arcade championship has.
pub const CHAMPIONSHIP_LAPS: u32 = 5;
/// How many laps an elimination has: one fewer than the bikes, so that one
/// goes out each lap but the last, which the last two race for.
pub const ELIMINATION_LAPS: u32 = RIDERS as u32 - 1;
/// The laps a race can be set to, and how many it has at first.
pub const LAPS: std::ops::RangeInclusive<u32> = 1..=9;
pub const DEFAULT_LAPS: u32 = 5;
/// The points for each place in a race of a championship.
pub const POINTS: [u32; 8] = [10, 8, 6, 5, 4, 3, 2, 1];
/// Against the clock: the seconds the start and each checkpoint give are
/// those the stretch to the next checkpoint takes at a pace (m/s), and a
/// couple more to spare; the start gives a few more, as the bike sets off
/// from a standstill. On medium the pace is about 220 km/h, a little slower
/// than a good rival rides it: such a ride earns one to four seconds at
/// each checkpoint, and a fall costs more than that. Easy gives more time,
/// hard less.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CheckpointTimes {
    pub pace: f32,
    pub spare: f32,
    pub start: f32,
}

impl CheckpointTimes {
    pub fn on(difficulty: Difficulty) -> Self {
        let (pace, spare, start) = match difficulty {
            Difficulty::Easy => (54.0, 3.0, 5.0),
            Difficulty::Medium => (62.0, 2.0, 3.0),
            Difficulty::Hard => (66.0, 1.5, 2.0),
            Difficulty::VeryHard => (69.0, 1.0, 2.5),
        };
        Self { pace, spare, start }
    }

    /// The seconds the clock gives for a stretch of `stretch` m to the next
    /// checkpoint.
    pub fn bonus(self, stretch: f32) -> f32 {
        stretch / self.pace + self.spare
    }

    /// The seconds on the clock at the start, for the stretch to the first
    /// checkpoint of `points` (see [`checkpoints`]) from a standstill.
    pub fn starting(self, points: &[f32]) -> f32 {
        self.bonus(stretch_after(points, usize::MAX)) + self.start
    }

    /// The same on a track ridden `pace` times as fast as most (see
    /// [`crate::circuits::Circuit::pace`]): the seconds of each stretch at
    /// that much of the pace, and as many to spare.
    pub fn at(self, pace: f32) -> Self {
        Self {
            pace: self.pace * pace,
            ..self
        }
    }
}

/// When nothing else is known of a bike, the pace it is thought to keep
/// (m/s).
const USUAL_PACE: f32 = 62.0;
/// How far past a checkpoint a bike still goes through it (m), in case a
/// step jumps it.
const CHECKPOINT_REACH: f32 = 150.0;
/// Seconds between the last player crossing the line (or being out) and
/// the results.
const RESULTS_AFTER: f32 = 2.5;
/// Seconds the camera stays on a bike it watched come in before it goes to
/// the next one.
const WATCH_ARRIVAL: f32 = 2.5;
/// Once everybody is in, seconds the camera stays on a bike before it cuts
/// to another (see [`spot_worth`]).
const SHOT: f32 = 6.0;
/// How far ahead of a bike (m) its spot is judged.
const SPOT_AHEAD: f32 = 120.0;
/// Bikes this close together (m) ride in a group, worth watching.
const TOGETHER: f32 = 20.0;
/// The last bikes shown, which the camera does not go back to while there
/// are others.
const RECENT_SHOTS: usize = 3;

/// What is being played.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Mode {
    /// The championship: the tracks one after another, five laps each,
    /// with points for each place.
    Arcade,
    /// Riding round a track with no end.
    #[default]
    Free,
    /// A race of the laps chosen, on the track chosen.
    Race,
    /// A race on the track chosen in which, each lap, the last one is out,
    /// until two are left to race the last lap.
    Elimination,
    /// Against the clock: from a few seconds, each checkpoint passed gives
    /// more; at nought it is over.
    Checkpoint,
}

impl Mode {
    /// Whether the tracks are raced one after another.
    pub fn is_championship(self) -> bool {
        self == Mode::Arcade
    }

    /// How many laps a race has, if it ends after some.
    pub fn laps(self, chosen: u32) -> Option<u32> {
        match self {
            Mode::Free | Mode::Checkpoint => None,
            Mode::Race => Some(chosen),
            Mode::Arcade => Some(CHAMPIONSHIP_LAPS),
            Mode::Elimination => Some(ELIMINATION_LAPS),
        }
    }
}

/// What is being played, and the laps chosen for a race.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    pub mode: Mode,
    pub laps: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            mode: Mode::Free,
            laps: DEFAULT_LAPS,
        }
    }
}

/// The championship under way: the race it is on (the index of its track
/// in [`Circuits`]) and the points of each bike of the [`crate::bike::Roster`] so far.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Championship {
    pub round: usize,
    pub points: [u32; 8],
}

impl Championship {
    /// Whether it is over: every one of the `tracks` raced.
    pub fn is_over(&self, tracks: usize) -> bool {
        self.round >= tracks
    }

    /// Scores a race finished in `order` (bikes of the roster, first to
    /// last) and goes on to the next.
    pub fn score(&mut self, order: &[usize]) {
        for (place, &rider) in order.iter().enumerate() {
            self.points[rider] += POINTS.get(place).copied().unwrap_or(0);
        }
        self.round += 1;
    }

    /// The bikes of the roster from most points to fewest (ties: the
    /// order of the roster).
    pub fn standings(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..RIDERS).collect();
        order.sort_by_key(|&rider| std::cmp::Reverse(self.points[rider]));
        order
    }
}

/// What is kept between sessions of the races: the best of each track. (A
/// championship is not: the arcade starts from the first track each time,
/// and files that still keep one load without it.)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Records {
    /// Best lap on each track (s), by its name.
    pub best_laps: Vec<(String, f32)>,
    /// Most laps against the clock on each track, by its name.
    pub checkpoint_laps: Vec<(String, u32)>,
}
impl Records {
    pub fn best_lap(&self, track: &str) -> Option<f32> {
        self.best_laps
            .iter()
            .find(|(name, _)| name == track)
            .map(|(_, time)| *time)
    }

    /// Keeps `time` as the best lap of `track` if it is. Whether it was.
    pub fn lap(&mut self, track: &str, time: f32) -> bool {
        match self.best_laps.iter_mut().find(|(name, _)| name == track) {
            Some((_, best)) if *best <= time => false,
            Some((_, best)) => {
                *best = time;
                true
            }
            None => {
                self.best_laps.push((track.to_string(), time));
                true
            }
        }
    }

    pub fn checkpoint_best(&self, track: &str) -> Option<u32> {
        self.checkpoint_laps
            .iter()
            .find(|(name, _)| name == track)
            .map(|(_, laps)| *laps)
    }

    /// Keeps `laps` as the most against the clock on `track` if it is.
    /// Whether it was.
    pub fn checkpoint(&mut self, track: &str, laps: u32) -> bool {
        match self
            .checkpoint_laps
            .iter_mut()
            .find(|(name, _)| name == track)
        {
            Some((_, best)) if *best >= laps => false,
            Some((_, best)) => {
                *best = laps;
                true
            }
            None => {
                self.checkpoint_laps.push((track.to_string(), laps));
                true
            }
        }
    }

    /// Keeps the records of a track that was `from` under its new name,
    /// `to` (the best of both, if it has some under that name already).
    pub fn rename(&mut self, from: &str, to: &str) {
        let laps: Vec<_> = self
            .best_laps
            .extract_if(.., |(name, _)| name == from)
            .collect();
        for (_, time) in laps {
            self.lap(to, time);
        }
        let clock: Vec<_> = self
            .checkpoint_laps
            .extract_if(.., |(name, _)| name == from)
            .collect();
        for (_, laps) in clock {
            self.checkpoint(to, laps);
        }
    }

    /// Keeps the records saved under the former names of each of the
    /// `circuits` (see [`crate::circuits::Circuit::formerly`]) under its own.
    pub fn take_former(&mut self, circuits: &crate::circuits::Circuits) {
        for circuit in circuits.iter() {
            for former in circuit.formerly {
                self.rename(former, circuit.name);
            }
        }
    }
}

/// The gates round a lap, as fractions of it: a lap only counts once a bike
/// has gone through both (so riding back and forth over the line does
/// not).
const GATES: [f32; 2] = [1.0 / 3.0, 2.0 / 3.0];

/// Against the clock, the checkpoints of a lap `lap` m long, in m from the
/// line: the track's checkpoint gantries (`gantries`), in order, and the
/// line itself, last, at the end of the lap.
pub fn checkpoints(gantries: &[f32], lap: f32) -> Vec<f32> {
    let mut points: Vec<f32> = gantries
        .iter()
        .copied()
        .filter(|&along| along > 0.0 && along < lap)
        .collect();
    points.sort_by(f32::total_cmp);
    points.push(lap);
    points
}

/// The length of the stretch (m) from checkpoint `index` of `points` (see
/// [`checkpoints`]) to the next one: from the line, the first.
pub fn stretch_after(points: &[f32], index: usize) -> f32 {
    let next = index.checked_add(1).and_then(|next| points.get(next));
    match (points.get(index), next) {
        (Some(from), Some(to)) => to - from,
        _ => points.first().copied().unwrap_or(0.0),
    }
}

/// How a bike is getting on in the race.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// Laps done.
    pub laps: u32,
    /// How far into this lap it is (m from the finish line).
    pub along: f32,
    /// The gates of this lap gone through (0, 1 or 2).
    gates: usize,
    /// When it crossed the line at the end of its race (s on the clock).
    pub finished: Option<f32>,
    /// When the lap under way began (s on the clock), the best lap and the
    /// last one (s).
    lap_start: f32,
    pub best_lap: Option<f32>,
    pub last_lap: Option<f32>,
    /// Against the clock: checkpoints gone through, and the next one of
    /// the lap (an index of [`checkpoints`]).
    pub checkpoints: u32,
    next_checkpoint: usize,
}

/// What a bike went through in a step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Passed {
    /// A checkpoint gantry on the way round (against the clock), or the line.
    pub checkpoint: bool,
    /// The finish line, at the end of a lap.
    pub lap: bool,
}

impl Progress {
    /// The bike is now `along` m into a lap `lap` m long, whose checkpoints
    /// are `points` (see [`checkpoints`]): what it went through.
    pub fn pass(&mut self, along: f32, lap: f32, points: &[f32]) -> Passed {
        let before = self.along;
        self.along = along;
        let mut passed = Passed::default();
        // A gate counts within a stretch after it, in case a step jumps it.
        if let Some(&gate) = GATES.get(self.gates) {
            let at = gate * lap;
            if (at..at + lap / 6.0).contains(&along) {
                self.gates += 1;
            }
        }
        // So does a checkpoint gantry; the line is the lap's.
        if self.next_checkpoint + 1 < points.len() {
            let at = points[self.next_checkpoint];
            if (at..at + CHECKPOINT_REACH.min(lap / 6.0)).contains(&along) {
                self.next_checkpoint += 1;
                self.checkpoints += 1;
                passed.checkpoint = true;
            }
        }
        if self.gates == GATES.len() && before > lap * 5.0 / 6.0 && along < lap / 6.0 {
            self.gates = 0;
            self.laps += 1;
            self.next_checkpoint = 0;
            self.checkpoints += 1;
            passed.lap = true;
            passed.checkpoint = true;
        }
        passed
    }

    /// How long the lap under way has taken at `clock` (s on the clock).
    pub fn lap_time(&self, clock: f32) -> f32 {
        (clock - self.lap_start).max(0.0)
    }

    /// The checkpoint just gone through (an index of [`checkpoints`]).
    pub fn last_checkpoint(&self, points: &[f32]) -> usize {
        match self.next_checkpoint {
            0 => points.len().saturating_sub(1),
            next => next - 1,
        }
    }

    /// How far it has gone in the race (m): laps done and how far into
    /// this one. On the grid, before the line, it is short of nought.
    pub fn distance(&self, lap: f32) -> f32 {
        let along = if self.gates == 0 && self.along > lap / 2.0 {
            self.along - lap
        } else {
            self.along
        };
        self.laps as f32 * lap + along
    }
}

/// The order of the bikes of a race, first to last, given each one's
/// progress (`racing`: an id and its progress) and those out of it, in the
/// order they went out: those that finished by when, then those racing by
/// how far they have gone, then those out, the last out first.
pub fn order<T: Copy>(racing: &[(T, &Progress)], out: &[T], lap: f32) -> Vec<T> {
    let mut finished: Vec<(T, f32)> = racing
        .iter()
        .filter_map(|(id, progress)| progress.finished.map(|time| (*id, time)))
        .collect();
    finished.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut running: Vec<(T, f32)> = racing
        .iter()
        .filter(|(_, progress)| progress.finished.is_none())
        .map(|(id, progress)| (*id, progress.distance(lap)))
        .collect();
    running.sort_by(|a, b| b.1.total_cmp(&a.1));
    finished
        .into_iter()
        .chain(running)
        .map(|(id, _)| id)
        .chain(out.iter().rev().copied())
        .collect()
}

/// When a bike still racing at `clock` (s), having gone `distance` m of a
/// race `total` m long, would cross the line at the pace it has kept.
pub fn estimate_finish(clock: f32, distance: f32, total: f32) -> f32 {
    let pace = if clock > 0.0 && distance > 0.0 {
        distance / clock
    } else {
        USUAL_PACE
    };
    clock + (total - distance).max(0.0) / pace.max(1.0)
}

/// The state of the race under way.
#[derive(Resource, Clone, Debug, Default)]
pub struct Standing {
    /// Seconds on the clock since the start.
    pub clock: f32,
    /// Against the clock: the seconds left, and the last bonus and when it
    /// came (s on the clock).
    pub time_left: f32,
    pub bonus: Option<(f32, f32)>,
    /// The bikes out, in the order they went out (bikes of the roster).
    pub out: Vec<usize>,
    /// Of those, the players disqualified for jumping the start (see
    /// [`lights`]).
    pub disqualified: Vec<usize>,
    /// The players' false starts let off so far, one for each (bikes of
    /// the roster).
    pub false_starts: Vec<usize>,
    /// The order of the race now (bikes of the roster), first to last.
    pub order: Vec<usize>,
    /// The laps whose last one has already gone out.
    eliminations: u32,
    /// The last one put out and when (s on the clock), to show it.
    pub last_out: Option<(usize, f32)>,
    /// When all the players were done (s on the clock).
    done_at: Option<f32>,
    /// The last lap of this race that beat the track's record, and when it
    /// did (s on the clock), to say so for a moment.
    pub record: Option<(f32, f32)>,
}

/// The race is over for the players: what is shown on the results. Until
/// they go on from them, the places fill in as the others come in.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Outcome {
    pub mode: Mode,
    pub track: usize,
    /// The bikes of the roster in the order they finished, with each one's
    /// time (if it finished) and whether it was out.
    pub places: Vec<Place>,
    /// The bikes of the players (of the roster), player 1 first.
    pub players: Vec<usize>,
    /// Against the clock: the laps done and whether they are a record.
    pub laps: u32,
    pub record: bool,
    /// Whether the places are final: the players went on from them (those
    /// still racing then are placed where they were, with the time they
    /// would have taken), and a championship has scored them.
    pub settled: bool,
}

impl Outcome {
    /// The places as they are, final: a championship scores them (but
    /// those disqualified, last, who get nothing).
    pub fn settle(&mut self, championship: &mut Championship) {
        if self.settled {
            return;
        }
        self.settled = true;
        if self.mode.is_championship() {
            let order: Vec<usize> = self
                .places
                .iter()
                .filter(|place| !place.disqualified)
                .map(|place| place.rider)
                .collect();
            championship.score(&order);
        }
    }
}

/// A bike's place in a race.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub rider: usize,
    pub time: Option<f32>,
    pub out: bool,
    /// Out for jumping the start.
    pub disqualified: bool,
    /// Whether `time` is when it would have crossed the line: it was still
    /// racing.
    pub estimated: bool,
}

/// After the players' race is over, the bike the camera watches come in,
/// and since when (s on the clock). In split screen the first camera takes
/// the whole screen for it.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Watching {
    pub bike: Entity,
    pub rider: usize,
    pub since: f32,
}

pub struct RacePlugin;

impl Plugin for RacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Rules>()
            .init_resource::<Championship>()
            .init_resource::<Standing>()
            .add_message::<RaceCue>()
            .add_plugins((
                hud::RaceHudPlugin,
                lights::StartLightsPlugin,
                results::ResultsPlugin,
            ))
            // Over, the players' controls no longer drive the bikes nor
            // restart the race: they work the results.
            .configure_sets(
                RunFixedMainLoop,
                RaceControls.run_if(not(resource_exists::<Outcome>)),
            )
            .add_systems(OnEnter(GameState::Race), start_race)
            .add_systems(
                Update,
                restart_with_some_out.run_if(
                    in_state(GameState::Race)
                        .and_then(pause::running)
                        .and_then(not(resource_exists::<Outcome>)),
                ),
            )
            // The race goes on behind the results, until the players go on.
            .add_systems(
                Update,
                (
                    attach_progress,
                    follow_the_race,
                    eliminate,
                    autopilot_the_finished,
                    finish_the_race.run_if(not(resource_exists::<Outcome>)),
                    cue_the_moments,
                    (fill_in_the_results, watch_them_come_in)
                        .chain()
                        .run_if(resource_exists::<Outcome>),
                )
                    .chain()
                    .run_if(in_state(GameState::Race)),
            )
            .add_systems(OnExit(GameState::Race), clear_outcome);
    }
}

/// The checkpoints of the track under way (see [`checkpoints`]).
fn track_checkpoints(circuit: &Circuit, track: &Track) -> Vec<f32> {
    checkpoints(&circuit.checkpoints(), track.lap_length())
}

/// A new race: the clock at nought and nobody out.
fn start_race(
    mut commands: Commands,
    rules: Res<Rules>,
    track: Res<Track>,
    current: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    settings: Res<Settings>,
) {
    let mut standing = Standing::default();
    if rules.mode == Mode::Checkpoint {
        let times = CheckpointTimes::on(settings.difficulty).at(circuits.get(*current).pace);
        standing.time_left = times.starting(&track_checkpoints(circuits.get(*current), &track));
    }
    commands.insert_resource(standing);
    commands.remove_resource::<Outcome>();
    commands.remove_resource::<Watching>();
}

fn clear_outcome(mut commands: Commands) {
    commands.remove_resource::<Outcome>();
    commands.remove_resource::<Watching>();
}

/// Restarting a race once someone is out (an elimination, or a player
/// disqualified at the start) sets it up again from the start, those out
/// back on the grid: the rivals out have left it, and the race keeps who
/// is out, so parking the bikes there (as [`crate::bike`] does) is not
/// enough.
fn restart_with_some_out(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    settings: Res<Settings>,
    standing: Res<Standing>,
    mut next: ResMut<NextState<GameState>>,
) {
    if !standing.out.is_empty()
        && settings
            .controls
            .just_pressed(Action::Restart, &keyboard, gamepads.iter())
    {
        next.set(GameState::Race);
    }
}

/// Every bike of the race keeps count of its laps.
#[allow(clippy::type_complexity)]
fn attach_progress(
    mut commands: Commands,
    bikes: Query<Entity, (With<BikeState>, With<Rider>, Without<Progress>)>,
) {
    for bike in &bikes {
        commands.entity(bike).insert(Progress::default());
    }
}

/// The clock, each bike's laps, the order of the race and, against the
/// clock, the time left. Restarting the race starts it all again.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn follow_the_race(
    time: Res<Time>,
    race: Res<Race>,
    rules: Res<Rules>,
    track: Res<Track>,
    current: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    mut settings: ResMut<Settings>,
    mut standing: ResMut<Standing>,
    mut was_started: Local<bool>,
    mut bikes: Query<(
        &BikeState,
        &Rider,
        &mut Progress,
        Has<Player>,
        Has<LocalPlayer>,
    )>,
) {
    let lap = track.lap_length();
    let points = track_checkpoints(circuits.get(*current), &track);
    let times = CheckpointTimes::on(settings.difficulty).at(circuits.get(*current).pace);
    // Restarted: everyone back to the grid.
    if *was_started && !race.started {
        let left = (rules.mode == Mode::Checkpoint).then(|| times.starting(&points));
        *standing = Standing {
            time_left: left.unwrap_or(0.0),
            ..default()
        };
        for (_, _, mut progress, ..) in &mut bikes {
            *progress = Progress::default();
        }
    }
    *was_started = race.started;
    if !race.started {
        // On the grid, waiting: the order they start in, those
        // disqualified last.
        for (state, _, mut progress, ..) in &mut bikes {
            let nearest = track.nearest_at(state.position, state.elevation);
            progress.pass(track.along(&nearest), lap, &points);
        }
        let grid: Vec<(usize, &Progress)> = bikes
            .iter()
            .filter(|(_, rider, ..)| !standing.out.contains(&rider.0))
            .map(|(_, rider, progress, ..)| (rider.0, progress))
            .collect();
        let order = order(&grid, &standing.out, lap);
        standing.order = order;
        return;
    }
    standing.clock += time.delta_secs();
    let clock = standing.clock;
    let target = rules.mode.laps(rules.laps);
    let name = circuits.get(*current).name;
    let mut record = None;
    for (state, _, mut progress, player, local) in &mut bikes {
        let nearest = track.nearest_at(state.position, state.elevation);
        let passed = progress.pass(track.along(&nearest), lap, &points);
        let human = player || local;
        if passed.lap {
            let lap_time = clock - progress.lap_start;
            progress.lap_start = clock;
            if progress.finished.is_none() {
                progress.last_lap = Some(lap_time);
                if progress.best_lap.is_none_or(|best| lap_time < best) {
                    progress.best_lap = Some(lap_time);
                }
                // A record if it beats the one kept (the first lap ever
                // ridden on a track is kept, but beats nothing).
                let kept = settings.records.best_lap(name).is_some();
                if human && settings.records.lap(name, lap_time) && kept {
                    record = Some((lap_time, clock));
                }
            }
            if target.is_some_and(|laps| progress.laps >= laps) && progress.finished.is_none() {
                progress.finished = Some(clock);
            }
        }
        if human && passed.checkpoint && rules.mode == Mode::Checkpoint {
            let reached = progress.last_checkpoint(&points);
            let bonus = times.bonus(stretch_after(&points, reached));
            standing.time_left += bonus;
            standing.bonus = Some((bonus, clock));
        }
    }
    if record.is_some() {
        standing.record = record;
    }
    if rules.mode == Mode::Checkpoint {
        standing.time_left = (standing.time_left - time.delta_secs()).max(0.0);
    }
    // A player who is out is in the race still, but in the order of those
    // out.
    let racing: Vec<(usize, &Progress)> = bikes
        .iter()
        .filter(|(_, rider, ..)| !standing.out.contains(&rider.0))
        .map(|(_, rider, progress, ..)| (rider.0, progress))
        .collect();
    standing.order = order(&racing, &standing.out, lap);
}

/// In elimination, each lap the last one is out, as soon as all but the
/// last two still in have crossed the line at its end: of the two left
/// behind, the one further back goes. A rival leaves the race; a player
/// stops where they are. With two left, they race the last lap for the win.
#[allow(clippy::type_complexity)]
fn eliminate(
    mut commands: Commands,
    rules: Res<Rules>,
    mut standing: ResMut<Standing>,
    bikes: Query<(Entity, &Rider, &Progress, Has<Player>, Has<LocalPlayer>), Without<Retired>>,
) {
    if rules.mode != Mode::Elimination {
        return;
    }
    let target = rules.mode.laps(rules.laps).unwrap_or(u32::MAX);
    let still_in: Vec<(Entity, usize, u32, bool)> = bikes
        .iter()
        .filter(|(_, rider, ..)| !standing.out.contains(&rider.0))
        .map(|(bike, rider, progress, player, local)| {
            (bike, rider.0, progress.laps, player || local)
        })
        .collect();
    let lap = standing.eliminations + 1;
    if still_in.len() < 3 || lap >= target {
        return;
    }
    // Has the third from last crossed the line at the end of this lap?
    let crossed = still_in
        .iter()
        .filter(|(_, _, laps, _)| *laps >= lap)
        .count();
    if crossed < still_in.len() - 2 {
        return;
    }
    standing.eliminations = lap;
    let Some(&last) = standing
        .order
        .iter()
        .rev()
        .find(|rider| still_in.iter().any(|(_, r, ..)| r == *rider))
    else {
        return;
    };
    let Some(&(bike, _, _, human)) = still_in.iter().find(|(_, rider, ..)| *rider == last) else {
        return;
    };
    if human {
        commands.entity(bike).insert(Retired);
    } else {
        commands.entity(bike).despawn();
    }
    standing.out.push(last);
    standing.last_out = Some((last, standing.clock));
}

/// A moment of the race worth a sound (see [`crate::sound`]).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaceCue {
    /// A player crossed the line at the end, first.
    Won,
    /// A player crossed the line at the end, not first.
    Finished,
    /// A player's lap beat the track's record, or against the clock their
    /// laps are the most.
    Record,
    /// A rival went out of an elimination.
    RivalOut,
    /// A player went out (eliminated or disqualified), or against the clock
    /// their time is up.
    Lost,
}

/// What the moments of a race are cued from, as last seen.
#[derive(Clone, Debug, Default, PartialEq)]
struct Moments {
    /// The players who have crossed the line at the end (bikes of the
    /// roster), with their places.
    finished: Vec<(usize, usize)>,
    /// Those out, in the order they went out (bikes of the roster), and
    /// whether each is a player.
    out: Vec<(usize, bool)>,
    /// The last record lap and when it was ridden (see [`Standing::record`]).
    record: Option<(f32, f32)>,
    /// Against the clock: whether the time is up, and whether the laps done
    /// are the most on the track.
    time_up: bool,
    laps_record: bool,
}

/// The sounds for what has happened between two looks at a race: a player
/// in, first or not; a record; someone out, a player or a rival; the time
/// up. Going back (the race set up again) cues nothing.
fn cues(before: &Moments, now: &Moments) -> Vec<RaceCue> {
    let mut cues = Vec::new();
    for &(rider, place) in &now.finished {
        if !before.finished.iter().any(|&(r, _)| r == rider) {
            cues.push(if place == 1 {
                RaceCue::Won
            } else {
                RaceCue::Finished
            });
        }
    }
    for &(rider, player) in &now.out {
        if !before.out.iter().any(|&(r, _)| r == rider) {
            cues.push(if player {
                RaceCue::Lost
            } else {
                RaceCue::RivalOut
            });
        }
    }
    if now.time_up && !before.time_up {
        cues.push(RaceCue::Lost);
    }
    if (now.record.is_some() && now.record != before.record)
        || (now.laps_record && !before.laps_record)
    {
        cues.push(RaceCue::Record);
    }
    cues
}

/// Cues the moments of the race as they happen (see [`cues`]).
#[allow(clippy::type_complexity)]
fn cue_the_moments(
    rules: Res<Rules>,
    standing: Res<Standing>,
    outcome: Option<Res<Outcome>>,
    bikes: Query<(&Rider, &Progress, Has<Player>, Has<LocalPlayer>)>,
    mut last: Local<Moments>,
    mut sounds: MessageWriter<RaceCue>,
) {
    let players: Vec<(usize, &Progress)> = bikes
        .iter()
        .filter(|(_, _, player, local)| *player || *local)
        .map(|(rider, progress, ..)| (rider.0, progress))
        .collect();
    let place = |rider: usize| {
        standing
            .order
            .iter()
            .position(|&r| r == rider)
            .map_or(standing.order.len(), |p| p + 1)
    };
    let now = Moments {
        finished: players
            .iter()
            .filter(|(_, progress)| progress.finished.is_some())
            .map(|&(rider, _)| (rider, place(rider)))
            .collect(),
        out: standing
            .out
            .iter()
            .map(|&rider| (rider, players.iter().any(|&(r, _)| r == rider)))
            .collect(),
        record: standing.record,
        time_up: rules.mode == Mode::Checkpoint
            && standing.clock > 0.0
            && standing.time_left <= 0.0,
        laps_record: outcome.is_some_and(|outcome| outcome.record),
    };
    if *last != now {
        sounds.write_batch(cues(&last, &now));
        *last = now;
    }
}

/// A player who crosses the line at the end of the race hands the bike to
/// the machine, which rides it on without crashing.
#[allow(clippy::type_complexity)]
fn autopilot_the_finished(
    mut commands: Commands,
    track: Res<Track>,
    bikes: Query<
        (Entity, &BikeState, &Progress),
        (
            Or<(With<Player>, With<LocalPlayer>)>,
            Without<Autopilot>,
            Without<Retired>,
        ),
    >,
) {
    for (bike, state, progress) in &bikes {
        if progress.finished.is_some() {
            let lateral = track.nearest_at(state.position, state.elevation).lateral;
            autopilot(&mut commands.entity(bike), lateral);
        }
    }
}

/// The places of the race now: those in by when, those still racing by how
/// far they have gone, with the time they would take at their pace (in a
/// race of laps, but for an elimination, where they may still go out),
/// and those out.
fn places_now(
    standing: &Standing,
    rules: &Rules,
    lap: f32,
    progress_of: impl Fn(usize) -> Option<Progress>,
) -> Vec<Place> {
    let total = rules
        .mode
        .laps(rules.laps)
        .filter(|_| rules.mode != Mode::Elimination)
        .map(|laps| laps as f32 * lap);
    let mut latest = 0.0f32;
    standing
        .order
        .iter()
        .map(|&rider| {
            let progress = progress_of(rider);
            let out = standing.out.contains(&rider);
            let finished = progress.as_ref().and_then(|p| p.finished);
            let estimate = match (&progress, total) {
                (Some(progress), Some(total)) if finished.is_none() && !out => Some(
                    estimate_finish(standing.clock, progress.distance(lap), total)
                        .max(latest + 0.1),
                ),
                _ => None,
            };
            let time = finished.or(estimate);
            if let Some(time) = time {
                latest = latest.max(time);
            }
            Place {
                rider,
                time,
                out,
                disqualified: standing.disqualified.contains(&rider),
                estimated: estimate.is_some(),
            }
        })
        .collect()
}

/// The race is over for the players when every one has crossed the line at
/// the end of it, or is out, or against the clock has no time left; a
/// moment later the results are shown, and a player whose time is up rolls
/// to a stop.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn finish_the_race(
    mut commands: Commands,
    rules: Res<Rules>,
    track: Res<Track>,
    current: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    mut settings: ResMut<Settings>,
    mut standing: ResMut<Standing>,
    bikes: Query<(
        Entity,
        &Rider,
        &Progress,
        Has<Player>,
        Has<LocalPlayer>,
        Has<Retired>,
    )>,
) {
    if rules.mode == Mode::Free {
        return;
    }
    // (player 1 or 2, their bike, whether they are done)
    let mut players: Vec<(usize, Entity, usize, bool)> = bikes
        .iter()
        .filter(|(_, _, _, player, local, _)| *player || *local)
        .map(|(bike, rider, progress, player, _, retired)| {
            let index = if player { 0 } else { 1 };
            (index, bike, rider.0, progress.finished.is_some() || retired)
        })
        .collect();
    players.sort_by_key(|(index, ..)| *index);
    let done = match rules.mode {
        Mode::Checkpoint => standing.time_left <= 0.0,
        _ => !players.is_empty() && players.iter().all(|(.., done)| *done),
    };
    if !done {
        return;
    }
    let clock = standing.clock;
    let done_at = *standing.done_at.get_or_insert(clock);
    if clock - done_at < RESULTS_AFTER {
        return;
    }
    let lap = track.lap_length();
    let places = places_now(&standing, &rules, lap, |rider| {
        bikes
            .iter()
            .find(|(_, r, ..)| r.0 == rider)
            .map(|(_, _, progress, ..)| progress.clone())
    });
    let name = circuits.get(*current).name;
    let laps = bikes
        .iter()
        .find(|(_, _, _, player, ..)| *player)
        .map_or(0, |(_, _, progress, ..)| progress.laps);
    let record =
        rules.mode == Mode::Checkpoint && laps > 0 && settings.records.checkpoint(name, laps);
    // Those who did not cross the line (their time is up) roll to a stop;
    // those who did ride on, on autopilot.
    for (_, bike, ..) in &players {
        let finished = bikes
            .get(*bike)
            .is_ok_and(|(_, _, progress, ..)| progress.finished.is_some());
        if !finished {
            commands.entity(*bike).insert(Retired);
        }
    }
    commands.insert_resource(Outcome {
        mode: rules.mode,
        track: current.0,
        places,
        players: players.iter().map(|(_, _, rider, _)| *rider).collect(),
        laps,
        record,
        settled: false,
    });
}

/// Until the players go on from the results, the places fill in as the
/// others come in.
fn fill_in_the_results(
    rules: Res<Rules>,
    track: Res<Track>,
    standing: Res<Standing>,
    mut outcome: ResMut<Outcome>,
    bikes: Query<(&Rider, &Progress)>,
) {
    if outcome.settled || rules.mode == Mode::Checkpoint {
        return;
    }
    let places = places_now(&standing, &rules, track.lap_length(), |rider| {
        bikes
            .iter()
            .find(|(r, _)| r.0 == rider)
            .map(|(_, progress)| progress.clone())
    });
    if outcome.places != places {
        outcome.places = places;
    }
}

/// After the players' race, the camera watches the others come in: the one
/// nearest to the line of those still racing; once it is in, it stays on it
/// a moment before going to the next. Once everybody is in, it shows them
/// riding on, a shot of each in turn: the one whose spot is most worth
/// watching (see [`spot_worth`]) of those not shown lately.
#[allow(clippy::type_complexity)]
fn watch_them_come_in(
    mut commands: Commands,
    rules: Res<Rules>,
    track: Res<Track>,
    standing: Res<Standing>,
    watching: Option<Res<Watching>>,
    bikes: Query<(Entity, &Rider, &Progress, &BikeState)>,
    mut shown: Local<Vec<usize>>,
) {
    if rules.mode == Mode::Checkpoint {
        return;
    }
    let clock = standing.clock;
    let watched = watching.as_deref().filter(|w| bikes.contains(w.bike));
    // Still on its way, or just in: the camera stays on it.
    if let Some(current) = watched
        && let Ok((_, _, progress, _)) = bikes.get(current.bike)
    {
        match progress.finished {
            None if !standing.out.contains(&current.rider) => return,
            Some(at) if clock - at < WATCH_ARRIVAL => return,
            _ => {}
        }
    }
    let next = standing.order.iter().find_map(|&rider| {
        if standing.out.contains(&rider) {
            return None;
        }
        bikes
            .iter()
            .find(|(_, r, progress, _)| r.0 == rider && progress.finished.is_none())
    });
    if let Some((bike, rider, ..)) = next {
        shown.clear();
        commands.insert_resource(Watching {
            bike,
            rider: rider.0,
            since: clock,
        });
        return;
    }
    // Everybody in: the shots of them riding on.
    if watched.is_some_and(|current| clock - current.since < SHOT) {
        return;
    }
    let riding: Vec<(Entity, usize, &BikeState)> = bikes
        .iter()
        .filter(|(_, rider, ..)| !standing.out.contains(&rider.0))
        .map(|(bike, rider, _, state)| (bike, rider.0, state))
        .collect();
    let worth: Vec<(usize, f32)> = riding
        .iter()
        .map(|&(_, rider, state)| {
            let together = riding
                .iter()
                .filter(|(_, other, them)| {
                    *other != rider
                        && them.position.distance(state.position) < TOGETHER
                        && (them.elevation - state.elevation).abs() < 5.0
                })
                .count();
            (rider, spot_worth(&track, state, together))
        })
        .collect();
    let current = watched.map(|current| current.rider);
    if let Some(rider) = next_shot(&worth, current, &shown)
        && let Some(&(bike, ..)) = riding.iter().find(|(_, r, _)| *r == rider)
    {
        shown.push(rider);
        commands.insert_resource(Watching {
            bike,
            rider,
            since: clock,
        });
    } else if watched.is_none() && watching.is_some() {
        commands.remove_resource::<Watching>();
    }
}

/// How worth watching the spot of a bike riding as `state` is, with
/// `together` others close by: round a loop most, then on a road off the
/// lap, the bends and crests coming up in the next [`SPOT_AHEAD`] m, and
/// riding in a group. A bike hardly moving is not.
pub fn spot_worth(track: &Track, state: &BikeState, together: usize) -> f32 {
    let nearest = track.nearest_at(state.position, state.elevation);
    let road = track.road(nearest.road);
    let ahead = (SPOT_AHEAD / road.step_length()).ceil() as usize;
    let (mut bend, mut crest) = (0.0f32, 0.0f32);
    for step in 0..ahead {
        let index = nearest.index + step;
        if !road.is_closed() && index >= road.sample_count() {
            break;
        }
        let index = index % road.sample_count();
        bend = bend.max(road.smoothed_curvature(index, 3));
        // Asked of the whole track, which finds the road from `nearest`: a
        // road off the lap has no roads of its own to find it among.
        crest = crest.max(track.crest(&Nearest { index, ..nearest }));
    }
    let mut worth = (bend * 40.0).min(1.5) + (crest * 60.0).min(1.5);
    worth += (together as f32 * 0.5).min(1.5);
    if nearest.road != 0 {
        worth += 1.5;
    }
    if state.ride.is_some() {
        worth += 3.0;
    }
    if state.speed.abs() < 5.0 {
        worth *= 0.2;
    }
    worth
}

/// The bike for the next shot, of those riding (`worth`: each one and how
/// worth watching its spot is): the most worth watching but the one
/// `current`ly watched, and but the last ones `shown` if there are others.
pub fn next_shot(worth: &[(usize, f32)], current: Option<usize>, shown: &[usize]) -> Option<usize> {
    let lately = &shown[shown.len().saturating_sub(RECENT_SHOTS)..];
    let best = |skip: &dyn Fn(usize) -> bool| {
        worth
            .iter()
            .filter(|(rider, _)| Some(*rider) != current && !skip(*rider))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(rider, _)| *rider)
    };
    best(&|rider| lately.contains(&rider)).or_else(|| best(&|_| false))
}

/// Puts the arcade championship on its first race.
pub fn start_championship(championship: &mut Championship, current: &mut CurrentCircuit) {
    *championship = Championship::default();
    current.0 = 0;
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    const LAP: f32 = 3000.0;
    /// Checkpoint gantries at 1000 and 2000 m, and the line.
    const POINTS: [f32; 3] = [1000.0, 2000.0, LAP];

    /// Rides `progress` from `from` to `to` m along (forward, round the
    /// line if need be) in steps of 10 m.
    fn ride(progress: &mut Progress, from: f32, to: f32) -> Vec<Passed> {
        let length = (to - from).rem_euclid(LAP);
        (0..=(length / 10.0) as usize)
            .map(|k| progress.pass((from + k as f32 * 10.0).rem_euclid(LAP), LAP, &POINTS))
            .filter(|passed| passed.checkpoint || passed.lap)
            .collect()
    }

    #[test]
    fn the_moments_of_a_race_are_cued_once_as_they_happen() {
        let start = Moments::default();
        assert!(cues(&start, &start).is_empty());
        // A rival goes out, then player 1 (rider 0): a rival out, then lost.
        let rival_out = Moments {
            out: vec![(5, false)],
            ..start.clone()
        };
        assert_eq!(cues(&start, &rival_out), [RaceCue::RivalOut]);
        assert!(cues(&rival_out, &rival_out).is_empty(), "once");
        let player_out = Moments {
            out: vec![(5, false), (0, true)],
            ..rival_out.clone()
        };
        assert_eq!(cues(&rival_out, &player_out), [RaceCue::Lost]);
        // Player 2 wins, and later player 1 comes in third.
        let won = Moments {
            finished: vec![(1, 1)],
            ..start.clone()
        };
        assert_eq!(cues(&start, &won), [RaceCue::Won]);
        let both = Moments {
            finished: vec![(1, 1), (0, 3)],
            ..won.clone()
        };
        assert_eq!(cues(&won, &both), [RaceCue::Finished]);
        // A lap record, and a later one; and the most laps against the clock.
        let record = Moments {
            record: Some((61.0, 70.0)),
            ..start.clone()
        };
        assert_eq!(cues(&start, &record), [RaceCue::Record]);
        let better = Moments {
            record: Some((60.5, 131.0)),
            ..record.clone()
        };
        assert_eq!(cues(&record, &better), [RaceCue::Record]);
        let time_up = Moments {
            time_up: true,
            ..start.clone()
        };
        assert_eq!(cues(&start, &time_up), [RaceCue::Lost]);
        let most_laps = Moments {
            laps_record: true,
            ..time_up.clone()
        };
        assert_eq!(cues(&time_up, &most_laps), [RaceCue::Record]);
        // The race set up again: nothing.
        assert!(cues(&both, &start).is_empty());
        assert!(cues(&player_out, &start).is_empty());
    }

    #[test]
    fn a_lap_counts_after_going_all_the_way_round() {
        let mut progress = Progress::default();
        // From the grid, behind the line: crossing it does not count.
        let passed = ride(&mut progress, LAP - 40.0, 100.0);
        assert!(passed.is_empty());
        assert_eq!(progress.laps, 0);
        // Round to the line: the two checkpoints and the line.
        let passed = ride(&mut progress, 100.0, 20.0);
        assert_eq!(passed.len(), 3);
        assert!(!passed[0].lap && !passed[1].lap && passed[2].lap);
        assert_eq!(progress.laps, 1);
        assert_eq!(progress.checkpoints, 3);
        assert_eq!(progress.last_checkpoint(&POINTS), 2);
    }

    #[test]
    fn riding_back_and_forth_over_the_line_does_not_count() {
        let mut progress = Progress::default();
        ride(&mut progress, 100.0, LAP - 10.0);
        // Now back and forth over the line.
        for _ in 0..5 {
            progress.pass(LAP - 20.0, LAP, &POINTS);
            progress.pass(15.0, LAP, &POINTS);
        }
        assert_eq!(progress.laps, 1, "only the first time");
        progress.pass(LAP - 20.0, LAP, &POINTS);
        progress.pass(15.0, LAP, &POINTS);
        assert_eq!(progress.laps, 1);
    }

    #[test]
    fn the_checkpoints_are_the_gantries_and_the_line() {
        let points = checkpoints(&[1980.0, 0.0, 960.0], 3068.0);
        assert_eq!(points, [960.0, 1980.0, 3068.0]);
        // Each gives the seconds of the stretch to the next; the line, of
        // the stretch to the first.
        assert_eq!(stretch_after(&points, 0), 1020.0);
        assert_eq!(stretch_after(&points, 1), 1088.0);
        assert_eq!(stretch_after(&points, 2), 960.0);
        assert_eq!(stretch_after(&points, usize::MAX), 960.0);
        // A track with no gantries: the line, once a lap.
        assert_eq!(checkpoints(&[], 1600.0), [1600.0]);
        assert_eq!(stretch_after(&[1600.0], 0), 1600.0);
    }

    pub fn every_track_has_its_checkpoints_evenly_spread_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let lap = Track::new(&circuit.layout).lap_length();
            let points = checkpoints(&circuit.checkpoints(), lap);
            assert!(points.len() >= 2, "{}: {points:?}", circuit.name);
            for index in 0..points.len() {
                let stretch = stretch_after(&points, index);
                // Some 30 s apart at race pace, never much more.
                assert!(
                    (550.0..=1500.0).contains(&stretch),
                    "{}: {stretch} m after checkpoint {index}",
                    circuit.name
                );
                // On medium, the seconds given are a little more than a
                // ride at 60 m/s takes, and a lot less than one at 30 m/s;
                // more on easy, fewer on hard.
                let [easy, medium, hard, very_hard] = [
                    Difficulty::Easy,
                    Difficulty::Medium,
                    Difficulty::Hard,
                    Difficulty::VeryHard,
                ]
                .map(CheckpointTimes::on);
                let bonus = medium.bonus(stretch);
                assert!(bonus > stretch / 60.0 && bonus < stretch / 30.0);
                assert!(easy.bonus(stretch) > bonus && hard.bonus(stretch) < bonus);
                assert!(very_hard.bonus(stretch) < hard.bonus(stretch));
                assert!(medium.starting(&points) > medium.bonus(points[0]));
                assert!(easy.starting(&points) > medium.starting(&points));
                assert!(hard.starting(&points) < medium.starting(&points));
                assert!(very_hard.starting(&points) < hard.starting(&points));
            }
        }
    }

    #[test]
    fn every_track_has_its_checkpoints_evenly_spread() {
        every_track_has_its_checkpoints_evenly_spread_on(&crate::testing::DEMO);
    }

    #[test]
    fn the_order_is_by_finish_then_distance_then_who_went_out_last() {
        let lap = LAP;
        let at = |laps: u32, along: f32, finished: Option<f32>| Progress {
            laps,
            along,
            gates: 1,
            finished,
            ..default()
        };
        let (a, b, c, d) = (
            at(2, 500.0, None),
            at(3, 10.0, Some(200.0)),
            at(2, 900.0, None),
            at(3, 30.0, Some(190.0)),
        );
        let racing = [(0, &a), (1, &b), (2, &c), (3, &d)];
        assert_eq!(order(&racing, &[5, 4], lap), [3, 1, 2, 0, 4, 5]);
        // On the grid, behind the line, they are not a lap ahead.
        let grid = Progress {
            along: lap - 20.0,
            ..default()
        };
        let moving = Progress {
            along: 30.0,
            ..default()
        };
        assert_eq!(order(&[(0, &grid), (1, &moving)], &[], lap), [1, 0]);
    }

    #[test]
    fn those_still_racing_are_placed_with_the_time_they_would_take() {
        // Half way round the second of two laps after 90 s: 1.5 laps at
        // its pace, 30 s more.
        assert!((estimate_finish(90.0, 1.5 * LAP, 2.0 * LAP) - 120.0).abs() < 1e-3);
        let standing = Standing {
            clock: 90.0,
            order: vec![2, 0, 1, 3],
            out: vec![3],
            ..default()
        };
        let rules = Rules {
            mode: Mode::Race,
            laps: 2,
        };
        let progress = |rider: usize| {
            Some(match rider {
                2 => Progress {
                    laps: 2,
                    finished: Some(85.0),
                    ..default()
                },
                0 => Progress {
                    laps: 1,
                    along: LAP / 2.0,
                    gates: 1,
                    ..default()
                },
                _ => Progress {
                    laps: 1,
                    along: LAP / 4.0,
                    ..default()
                },
            })
        };
        let places = places_now(&standing, &rules, LAP, progress);
        assert_eq!(places[0].time, Some(85.0));
        assert!(!places[0].estimated);
        assert!(places[1].estimated && places[2].estimated);
        // Behind, never in before the one ahead.
        assert!(places[2].time > places[1].time);
        assert!(places[3].out && places[3].time.is_none());
        // In an elimination, nobody racing is given a time: they may still
        // go out.
        let rules = Rules {
            mode: Mode::Elimination,
            ..rules
        };
        let places = places_now(&standing, &rules, LAP, progress);
        assert!(places[1].time.is_none() && !places[1].estimated);
    }

    pub fn a_championship_adds_up_points_race_after_race_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut championship = Championship::default();
        championship.score(&[3, 1, 0, 2, 4, 5, 6, 7]);
        championship.score(&[1, 3, 0, 2, 4, 5, 6, 7]);
        assert_eq!(championship.points[3], 10 + 8);
        assert_eq!(championship.points[1], 8 + 10);
        assert_eq!(championship.round, 2);
        assert_eq!(&championship.standings()[..3], &[1, 3, 0]);
        championship.round = circuits.len();
        assert!(championship.is_over(circuits.len()));
    }

    #[test]
    fn a_championship_adds_up_points_race_after_race() {
        a_championship_adds_up_points_race_after_race_on(&crate::testing::DEMO);
    }

    #[test]
    fn the_camera_goes_where_there_is_something_to_see() {
        let track = Track::default();
        let ahead = (SPOT_AHEAD / track.step_length()) as usize;
        let count = track.sample_count();
        let bendiest_ahead = |index: usize| {
            (0..ahead)
                .map(|step| track.smoothed_curvature((index + step) % count, 3))
                .fold(0.0, f32::max)
        };
        // The straightest stretch, and a little before the tightest bend.
        let straight = (0..count)
            .min_by(|a, b| bendiest_ahead(*a).total_cmp(&bendiest_ahead(*b)))
            .unwrap();
        let tightest = (0..count)
            .max_by(|a, b| {
                track
                    .smoothed_curvature(*a, 3)
                    .total_cmp(&track.smoothed_curvature(*b, 3))
            })
            .unwrap();
        let before_bend = (tightest + count - ahead / 2) % count;
        let riding = |index: usize, speed: f32| BikeState {
            speed,
            ..BikeState::parked_on(&track, track.pose_at(index))
        };
        let on_straight = spot_worth(&track, &riding(straight, 40.0), 0);
        let into_bend = spot_worth(&track, &riding(before_bend, 40.0), 0);
        assert!(into_bend > on_straight, "{into_bend} > {on_straight}");
        // In a group, more; hardly moving, hardly anything.
        assert!(spot_worth(&track, &riding(straight, 40.0), 2) > on_straight);
        assert!(spot_worth(&track, &riding(before_bend, 1.0), 0) < into_bend * 0.5);
    }

    pub fn a_bike_on_a_road_off_the_lap_is_worth_watching_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // Once everybody is in, the camera weighs every bike's spot; one on
        // a branch is weighed on that branch.
        let mut weighed = 0;
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            weighed += track.side_roads().len();
            for road in 1..=track.side_roads().len() {
                let line = track.road(road);
                let index = line.sample_count() / 2;
                let at = track.surface_point_on(road, index, 0.0);
                let state = BikeState {
                    elevation: at.y,
                    speed: 40.0,
                    ..BikeState::parked_at((Vec2::new(at.x, at.z), 0.0))
                };
                let nearest = track.nearest_at(state.position, state.elevation);
                if nearest.road == 0 {
                    // Where the branch still shares the lap's road.
                    continue;
                }
                let worth = spot_worth(&track, &state, 0);
                assert!(
                    worth.is_finite() && worth >= 1.5,
                    "{} road {road}: {worth}",
                    circuit.name
                );
            }
        }
        assert!(weighed > 0, "some track has roads off the lap");
    }

    #[test]
    fn the_shots_go_round_the_bikes_most_worth_watching_first() {
        let worth = [(0, 1.0), (1, 3.0), (2, 2.0), (3, 0.5), (4, 0.2)];
        // The best, but not the one watched.
        assert_eq!(next_shot(&worth, None, &[]), Some(1));
        assert_eq!(next_shot(&worth, Some(1), &[1]), Some(2));
        // Not the last ones shown while there are others.
        assert_eq!(next_shot(&worth, Some(2), &[1, 2]), Some(0));
        assert_eq!(next_shot(&worth, Some(0), &[1, 2, 0]), Some(3));
        // Only those shown left: the best of them again.
        assert_eq!(next_shot(&worth[..2], Some(0), &[1, 0]), Some(1));
        assert_eq!(next_shot(&[], None, &[]), None);
    }

    #[test]
    fn the_results_are_scored_once_when_the_players_go_on() {
        let place = |rider: usize, time: f32, estimated: bool| Place {
            rider,
            time: Some(time),
            out: false,
            disqualified: false,
            estimated,
        };
        let mut outcome = Outcome {
            mode: Mode::Arcade,
            track: 0,
            places: vec![place(4, 100.0, false), place(7, 104.0, true)],
            players: vec![7],
            laps: 0,
            record: false,
            settled: false,
        };
        let mut championship = Championship::default();
        outcome.settle(&mut championship);
        outcome.settle(&mut championship);
        assert_eq!(championship.round, 1);
        assert_eq!((championship.points[4], championship.points[7]), (10, 8));
        // Disqualified, last: no points.
        let mut outcome = Outcome {
            places: vec![
                place(4, 100.0, false),
                Place {
                    out: true,
                    disqualified: true,
                    ..place(7, 0.0, false)
                },
            ],
            settled: false,
            ..outcome
        };
        outcome.settle(&mut championship);
        assert_eq!((championship.points[4], championship.points[7]), (20, 8));
    }

    #[test]
    fn the_arcade_always_starts_from_the_first_track() {
        let mut championship = Championship {
            round: 2,
            points: [5; 8],
        };
        let mut current = CurrentCircuit(3);
        start_championship(&mut championship, &mut current);
        assert_eq!(championship, Championship::default());
        assert_eq!(current.0, 0);
    }

    #[test]
    fn records_keep_the_best() {
        let mut records = Records::default();
        assert!(records.lap("Bay", 70.0));
        assert!(!records.lap("Bay", 75.0));
        assert!(records.lap("Bay", 65.0));
        assert_eq!(records.best_lap("Bay"), Some(65.0));
        assert!(records.checkpoint("Bay", 2));
        assert!(!records.checkpoint("Bay", 1));
        assert_eq!(records.checkpoint_best("Bay"), Some(2));
        // Renamed, a track keeps its records, the best of both names.
        records.lap("Isle", 80.0);
        records.checkpoint("Isle", 4);
        records.lap("Island", 85.0);
        records.rename("Isle", "Island");
        records.rename("Nowhere", "Bay");
        assert_eq!(records.best_lap("Isle"), None);
        assert_eq!(records.checkpoint_best("Isle"), None);
        assert_eq!(records.best_lap("Island"), Some(80.0));
        assert_eq!(records.checkpoint_best("Island"), Some(4));
        assert_eq!(records.best_lap("Bay"), Some(65.0));
        assert_eq!(records.best_laps.len(), 2);
    }

    #[test]
    fn a_renamed_circuit_keeps_the_records_of_its_former_names() {
        // The demo circuit, once called otherwise.
        let path = "circuits/bahia.ron";
        let text = crate::testing::demo_file(path);
        let renamed = &*String::leak(text.replacen(
            "pace:",
            "formerly: [\"La Bahía\", \"Bay\"],\n    pace:",
            1,
        ));
        let circuits = crate::circuits::Circuits::parse(&[(path.to_string(), renamed)]).unwrap();
        let name = circuits[0].name;
        assert_eq!(circuits[0].formerly, ["La Bahía", "Bay"]);
        let mut records = Records::default();
        records.lap("La Bahía", 70.0);
        records.lap("Bay", 68.0);
        records.checkpoint("Bay", 3);
        records.lap("Elsewhere", 90.0);
        records.take_former(&circuits);
        assert_eq!(records.best_lap(name), Some(68.0));
        assert_eq!(records.checkpoint_best(name), Some(3));
        assert_eq!(records.best_lap("La Bahía"), None);
        assert_eq!(records.best_lap("Elsewhere"), Some(90.0));
        // A circuit that was never renamed has no former names.
        assert!(crate::testing::DEMO[0].formerly.is_empty());
    }

    pub fn each_mode_has_its_laps_on(roster: &'static crate::bike::Roster) {
        assert_eq!(Mode::Free.laps(3), None);
        assert_eq!(Mode::Checkpoint.laps(3), None);
        assert_eq!(Mode::Race.laps(7), Some(7));
        assert_eq!(Mode::Arcade.laps(7), Some(CHAMPIONSHIP_LAPS));
        // An elimination: one lap for each bike but the winner.
        assert_eq!(Mode::Elimination.laps(2), Some(7));
        assert_eq!(ELIMINATION_LAPS as usize, roster.len() - 1);
        assert_eq!(DEFAULT_LAPS, 5);
        assert!(Mode::Arcade.is_championship());
        assert!(!Mode::Race.is_championship() && !Mode::Elimination.is_championship());
        // The time against the clock grows with the stretch.
        let times = CheckpointTimes::on(Difficulty::Medium);
        assert!(times.bonus(1000.0) > times.bonus(500.0));
    }

    #[test]
    fn each_mode_has_its_laps() {
        each_mode_has_its_laps_on(&crate::testing::DEMO_ROSTER);
    }

    /// An elimination with four bikes of the roster (0, 1, 2 and 3), the
    /// player on 2, in the order 0, 1, 2, 3, having done `laps`.
    fn elimination(laps: [u32; 4]) -> (App, Vec<Entity>) {
        let mut app = App::new();
        app.insert_resource(Rules {
            mode: Mode::Elimination,
            laps: DEFAULT_LAPS,
        })
        .insert_resource(Standing {
            order: vec![0, 1, 2, 3],
            ..default()
        })
        .add_systems(Update, eliminate);
        let bikes = (0..4)
            .map(|rider| {
                let mut bike = app.world_mut().spawn((
                    Rider(rider),
                    Progress {
                        laps: laps[rider],
                        ..default()
                    },
                ));
                if rider == 2 {
                    bike.insert(Player);
                }
                bike.id()
            })
            .collect();
        (app, bikes)
    }

    #[test]
    fn the_last_one_is_out_when_the_third_from_last_crosses_the_line() {
        // Only the leader has finished the lap: nobody out yet.
        let (mut app, _) = elimination([1, 0, 0, 0]);
        app.update();
        assert!(app.world().resource::<Standing>().out.is_empty());
        // The third from last (of four, the second) crosses it: of the two
        // behind, the one further back leaves the race...
        let (mut app, bikes) = elimination([1, 1, 0, 0]);
        app.update();
        let standing = app.world().resource::<Standing>();
        assert_eq!(standing.out, [3]);
        assert_eq!(standing.last_out.map(|(rider, _)| rider), Some(3));
        assert!(app.world().get_entity(bikes[3]).is_err());
        // ...and only once for that lap.
        app.update();
        assert_eq!(app.world().resource::<Standing>().out, [3]);
        // Next lap, three left: the leader crossing the line is the third
        // from last, and the player, last, is out, but the bike stays,
        // rolling to a stop.
        app.world_mut().get_mut::<Progress>(bikes[0]).unwrap().laps = 2;
        app.update();
        assert_eq!(app.world().resource::<Standing>().out, [3, 2]);
        assert!(app.world().get::<Retired>(bikes[2]).is_some());
        // Two left: they race the last lap, and the finish decides it.
        app.world_mut().get_mut::<Progress>(bikes[0]).unwrap().laps = 6;
        app.world_mut().get_mut::<Progress>(bikes[1]).unwrap().laps = 6;
        app.update();
        assert_eq!(app.world().resource::<Standing>().out, [3, 2]);
    }
}
