//! The benchmark (`make bench`, or the game started with `--bench` or
//! `MOTORUST_BENCH=1`): with no one at the controls, it rides every track
//! (or those in `MOTORUST_BENCH_TRACKS`, as `0,2`) in free ride on
//! autopilot, all round the lap with one player and then with two, trying
//! each setting worth comparing (the shadows' levels, the ground left out of
//! them, the ways of smoothing the edges, the lighter far trees, the relief
//! of the textures, less detail for two players), with the frame rate
//! unlimited.
//!
//! All round the lap the settings take turns of a couple of seconds (let
//! settle half a second, then measured), each round in a different order,
//! so that every one of them is measured all over the track, on the same
//! roads as the rest; it rides on until it has gone all round and each
//! setting has been measured for `MOTORUST_BENCH_SECONDS` (10 by default).
//! Each lap begins with every setting tried once, unmeasured, while its
//! shaders compile. The game's share of the CPU is read over each turn, and
//! where on the lap the frames came slowest and fastest is noted.
//!
//! It says what it is doing in the terminal as it goes, and on the screen
//! under the frame rate: the setting on, and how far round the lap. The
//! results are written as they are measured to `bench-results/` (in the
//! folder it is run from: the repository, with `make bench`), one file for
//! each run named after when, the system, the CPU and the GPU
//! (`2026-10-16_1830_macos-26-3-1_apple-m1-pro.txt`, see
//! [`crate::perf_log::log_name`]), headed with the machine and the commit,
//! with the performance log of the run beside it (`.perf.log`); so runs on
//! several machines can be kept side by side, committed and compared.
//! Stopping it (Esc, which also prints the table so far, closing the window,
//! or Ctrl+C in the terminal) keeps the laps measured. The settings tried
//! are never saved.

use std::fs::File;
use std::io::Write;
use std::ops::Range;
use std::path::PathBuf;

use bevy::diagnostic::{DiagnosticsStore, SystemInfo};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::PrimaryWindow;

use crate::bike::{LocalPlayer, Player, Race, autopilot};
use crate::circuits::{Circuit, Circuits, CurrentCircuit};
use crate::hud::UiFont;
use crate::menu::GameState;
use crate::perf_log::{
    CpuClock, FrameStart, PerfLog, Stretch, Summary, folder, log_name, process_cpu,
};
use crate::race::{Mode, Progress, Rules};
use crate::render_scale::{NativeShare, drawn_size};
use crate::settings::{
    Antialiasing, ForcePerformance, FpsLimit, GrassTiling, Relief, Settings, ShadowQuality,
};
use crate::track::Track;
use crate::ui::text_font;
use crate::welcome::GameMode;
use crate::world::GroundPiece;
use crate::world::detail::TwoSamples;

/// Each setting's turn on the lap (s): let settle first (the setting just
/// put on: its pipelines, shadow maps, trees), then measured.
const TURN_SETTLE: f32 = 0.5;
const TURN_MEASURE: f32 = 2.0;
/// Each setting's turn as a lap begins, unmeasured (s).
const WARM_TURN: f32 = 1.5;
/// How long each setting is measured at least on each lap (s), by default.
const MEASURE: f32 = 10.0;
/// About how long setting up a race takes (s), and how fast the autopilot
/// rides round (m/s): for how long a run will take.
const SETTING_UP: f32 = 3.0;
const RIDING: f32 = 40.0;
/// Slower than this round the lap (m/s), and a minute more, the lap is
/// given up as it is: the bike is stuck.
const STUCK: f32 = 10.0;
/// The folder the results go in, in the folder the game is run from.
const RESULTS: &str = "bench-results";

/// Whether the game was started for the benchmark.
pub fn requested() -> bool {
    std::env::args().any(|arg| arg == "--bench")
        || std::env::var("MOTORUST_BENCH").is_ok_and(|value| value != "0")
}

/// The settings for the benchmark: the player's, never saved, with the
/// frame rate unlimited (to see how fast it can go) and the log on.
pub fn settings(mut settings: Settings) -> Settings {
    settings.transient = true;
    settings.fps = FpsLimit::Unlimited;
    // Each scenario's settings as they are, not changed by the frame rate.
    settings.force_performance = ForcePerformance::Off;
    settings.performance_log = true;
    // The frame rate shown, and its own label under it (with the pixels the
    // world is drawn at) instead of the HUD's second line.
    settings.show_fps = true;
    settings.show_resolution = false;
    settings
}

/// A scenario: a track, one player or two, and the settings that matter.
#[derive(Clone, Debug, PartialEq)]
pub struct Scenario {
    pub name: &'static str,
    pub track: usize,
    pub players: u32,
    pub shadows: ShadowQuality,
    pub antialiasing: Antialiasing,
    pub far_trees: bool,
    pub split_detail: bool,
    /// The render scale (% of the window's resolution).
    pub render_scale: u32,
    /// The relief of the scenery's textures.
    pub relief: Relief,
    /// Whether the road has relief too.
    pub road_relief: bool,
    /// Whether the relief is dropped at speed (but the road's).
    pub flat_at_speed: bool,
    /// How the lawn's repetition is broken up.
    pub grass_tiling: GrassTiling,
    /// Whether the ground casts the sun's shadows, as in the game. Each of
    /// its meshes covers the whole circuit, so every cascade draws all of
    /// it: without, how much cutting it into pieces the cascades could
    /// leave out would save at most (see [`GroundPiece`]).
    pub ground_shadows: bool,
}

/// Every scenario on each of `tracks`: one player at a fixed reference,
/// not as the game starts (shadows high, not automatic, so that runs
/// compare; multisampled, whole trees, the world at the window's
/// resolution, normal maps but on the road and not at speed, the wide
/// lawn; the game starts with FXAA, no relief and the shadows automatic),
/// then one thing changed at a time (parallax, kept at speed too); then two
/// players, with less detail, with all of it, with less detail drawn
/// smaller and without relief.
pub fn scenarios(tracks: &[usize]) -> Vec<Scenario> {
    let base = |name, track| Scenario {
        name,
        track,
        players: 1,
        shadows: ShadowQuality::High,
        antialiasing: Antialiasing::Msaa,
        far_trees: false,
        split_detail: true,
        render_scale: 100,
        relief: Relief::Normal,
        road_relief: false,
        flat_at_speed: true,
        grass_tiling: GrassTiling::Wide,
        ground_shadows: true,
    };
    tracks
        .iter()
        .flat_map(|&track| {
            [
                base("1p", track),
                Scenario {
                    shadows: ShadowQuality::Medium,
                    ..base("1p-shadows-medium", track)
                },
                Scenario {
                    shadows: ShadowQuality::Low,
                    ..base("1p-shadows-low", track)
                },
                Scenario {
                    ground_shadows: false,
                    ..base("1p-no-ground-shadows", track)
                },
                Scenario {
                    antialiasing: Antialiasing::Fxaa,
                    ..base("1p-fxaa", track)
                },
                Scenario {
                    antialiasing: Antialiasing::Off,
                    ..base("1p-no-antialiasing", track)
                },
                Scenario {
                    antialiasing: Antialiasing::Msaa2,
                    ..base("1p-msaa-2x", track)
                },
                Scenario {
                    antialiasing: Antialiasing::Smaa,
                    ..base("1p-smaa", track)
                },
                Scenario {
                    far_trees: true,
                    ..base("1p-lighter-far-trees", track)
                },
                Scenario {
                    render_scale: 70,
                    ..base("1p-render-70", track)
                },
                Scenario {
                    render_scale: 50,
                    ..base("1p-render-50", track)
                },
                Scenario {
                    relief: Relief::Off,
                    ..base("1p-relief-off", track)
                },
                Scenario {
                    flat_at_speed: false,
                    ..base("1p-relief-at-speed", track)
                },
                Scenario {
                    relief: Relief::Parallax,
                    flat_at_speed: false,
                    ..base("1p-parallax", track)
                },
                Scenario {
                    road_relief: true,
                    ..base("1p-road-relief", track)
                },
                Scenario {
                    grass_tiling: GrassTiling::Plain,
                    ..base("1p-grass-plain", track)
                },
                Scenario {
                    grass_tiling: GrassTiling::Stochastic,
                    ..base("1p-grass-stochastic", track)
                },
                Scenario {
                    players: 2,
                    ..base("2p", track)
                },
                Scenario {
                    players: 2,
                    split_detail: false,
                    ..base("2p-full-detail", track)
                },
                Scenario {
                    players: 2,
                    render_scale: 70,
                    ..base("2p-render-70", track)
                },
                Scenario {
                    players: 2,
                    relief: Relief::Off,
                    ..base("2p-relief-off", track)
                },
                Scenario {
                    players: 2,
                    ground_shadows: false,
                    ..base("2p-no-ground-shadows", track)
                },
            ]
        })
        .collect()
}

/// The tracks asked for in `value` (as `0,2`), or all `count` of them.
pub fn tracks(value: Option<&str>, count: usize) -> Vec<usize> {
    let asked: Vec<usize> = value
        .unwrap_or_default()
        .split(',')
        .filter_map(|track| track.trim().parse().ok())
        .filter(|&track| track < count)
        .collect();
    if asked.is_empty() {
        (0..count).collect()
    } else {
        asked
    }
}

/// The laps ridden: the scenarios of one track and players, one after
/// another, as ranges of `scenarios`, their settings taking turns.
pub fn laps(scenarios: &[Scenario]) -> Vec<Range<usize>> {
    let mut laps: Vec<Range<usize>> = Vec::new();
    for (index, scenario) in scenarios.iter().enumerate() {
        match laps.last_mut() {
            Some(lap)
                if scenarios[lap.start].track == scenario.track
                    && scenarios[lap.start].players == scenario.players =>
            {
                lap.end = index + 1;
            }
            _ => laps.push(index..index + 1),
        }
    }
    laps
}

/// Whose the `turn`th turn of a lap is, of `settings` taking turns: each of
/// them once a round, each round one place on, so that each one takes every
/// place of a round in turn.
pub fn turn_of(turn: usize, settings: usize) -> usize {
    (turn + turn / settings) % settings
}

/// How far a bike went (m) from `before` to `now` m into a lap `lap` m
/// long: over the line too, and back if it went back.
pub fn advanced(before: f32, now: f32, lap: f32) -> f32 {
    let step = now - before;
    if step > lap / 2.0 {
        step - lap
    } else if step < -lap / 2.0 {
        step + lap
    } else {
        step
    }
}

/// Where a lap was slowest and fastest to draw: of the `turns` measured
/// (whose, where on the lap (m), at what rate), the one furthest under its
/// setting's average rate (`averages`) and the one furthest over it, each
/// as where (m) and by how much (a share of the average).
pub fn extremes(turns: &[(usize, f32, f32)], averages: &[f32]) -> Option<((f32, f32), (f32, f32))> {
    let against: Vec<(f32, f32)> = turns
        .iter()
        .filter_map(|&(setting, along, fps)| {
            let average = *averages.get(setting)?;
            (average > 0.0).then(|| (along, fps / average - 1.0))
        })
        .collect();
    let slowest = against.iter().copied().min_by(|a, b| a.1.total_cmp(&b.1))?;
    let fastest = against.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1))?;
    Some((slowest, fastest))
}

/// About how long a track's lap is (m): round its control points, a little
/// shorter than its curve.
fn rough_length(track: &Circuit) -> f32 {
    let points = track.layout.control_points;
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a.distance(*b))
        .sum()
}

/// "1 player", "2 players".
fn players_said(players: u32) -> String {
    if players == 1 {
        "1 player".into()
    } else {
        format!("{players} players")
    }
}

/// The pixels the world is drawn at in a window of `window` physical
/// pixels at the render scale of `settings`, at most the screen's own (see
/// [`NativeShare`]), as shown on the screen.
pub fn resolution_said(settings: &Settings, window: UVec2, native: NativeShare) -> String {
    let size = drawn_size(window, settings.render_scale, native);
    format!(
        "3D {} × {} · {} %",
        size.x,
        size.y,
        settings.render_scale.min(100)
    )
}

/// The settings on, as they are shown on the screen: the shadows, the
/// smoothing of the edges, the far trees, the relief, the lawn's tiling
/// and, with two players, the detail.
pub fn settings_said(settings: &Settings, players: u32) -> String {
    let antialiasing = match settings.antialiasing {
        Antialiasing::Msaa => "MSAA 4x",
        Antialiasing::Msaa2 => "MSAA 2x",
        Antialiasing::Smaa => "SMAA",
        Antialiasing::Fxaa => "FXAA",
        Antialiasing::Off => "no antialiasing",
    };
    let relief = match settings.relief {
        Relief::Off => "no relief",
        Relief::Normal => "normal maps",
        Relief::Parallax => "parallax",
    };
    let grass = match settings.grass_tiling {
        GrassTiling::Plain => "grass tiled",
        GrassTiling::Wide => "grass wide",
        GrassTiling::Stochastic => "grass stochastic",
    };
    let road = if settings.road_relief {
        " (road too)"
    } else {
        ""
    };
    let speed = if settings.flat_at_speed {
        ""
    } else {
        " at speed too"
    };
    let mut line = format!(
        "shadows {} · {antialiasing} · far trees {} · {relief}{road}{speed} · {grass}",
        format!("{:?}", settings.shadows).to_lowercase(),
        if settings.lighter_far_trees {
            "lighter"
        } else {
            "whole"
        }
    );
    if players == 2 {
        line.push_str(if settings.lighter_split_screen {
            " · less detail for two"
        } else {
            " · all the detail"
        });
    }
    line
}

/// This machine's name, as the system gives it.
fn machine() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_default()
}

/// Where the benchmark is at in its lap.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// About to begin, the game started: its first lap is set up as the
    /// menus would (not while the first world is being built).
    Start,
    /// The race being set up for a lap, since when.
    Entering(f32),
    /// Each setting tried once as the lap begins, unmeasured: whose turn,
    /// since when.
    Warming { setting: usize, since: f32 },
    /// A setting's turn on the lap: whose, since when.
    Turn { setting: usize, since: f32 },
}

#[derive(Resource)]
pub struct Bench {
    scenarios: Vec<Scenario>,
    laps: Vec<Range<usize>>,
    /// The lap under way (an index of `laps`), and where it is at.
    lap: usize,
    phase: Phase,
    measure: f32,
    /// What the lap under way has measured of each of its settings: their
    /// frames, and their share of the CPU (%) by the seconds it was read
    /// over, and those seconds.
    stretches: Vec<Stretch>,
    cpu: Vec<(f32, f32)>,
    /// How many turns of the lap have been measured; the turn under way
    /// once it is measured: its frames, the game's CPU time and how far
    /// round it was (m) when its measuring began.
    turns: usize,
    turn_frames: Vec<f32>,
    turn_cpu: Option<CpuClock>,
    turn_from: Option<f32>,
    /// How long the lap is (m), where on it the measuring began and the
    /// first player was last (m from the line), and how far round it has
    /// been ridden since (m).
    length: f32,
    start_along: f32,
    along: Option<f32>,
    ridden: f32,
    /// When its measuring began (s), and how many quarters of it have been
    /// told in the terminal.
    measuring_since: f32,
    quarters: u32,
    /// Each turn measured: whose, where on the lap (m, its middle), at what
    /// rate.
    spots: Vec<(usize, f32, f32)>,
    results: Vec<(Scenario, Option<Summary>, Option<f32>)>,
    /// The circuits its scenarios' tracks are of.
    circuits: Circuits,
    /// This machine's name, the folder the results go in, and the file they
    /// are written to as they come (named as the run begins).
    machine: String,
    folder: PathBuf,
    path: Option<PathBuf>,
    file: Option<File>,
}

impl Bench {
    /// A run of `scenarios` of `circuits`, each measured `measure` s a lap,
    /// on `machine`, its results written to `folder`; not begun.
    fn new(
        circuits: Circuits,
        scenarios: Vec<Scenario>,
        measure: f32,
        machine: String,
        folder: PathBuf,
    ) -> Self {
        Bench {
            circuits,
            laps: laps(&scenarios),
            scenarios,
            lap: 0,
            phase: Phase::Start,
            measure,
            stretches: Vec::new(),
            cpu: Vec::new(),
            turns: 0,
            turn_frames: Vec::new(),
            turn_cpu: None,
            turn_from: None,
            length: 1.0,
            start_along: 0.0,
            along: None,
            ridden: 0.0,
            measuring_since: 0.0,
            quarters: 0,
            spots: Vec::new(),
            results: Vec::new(),
            machine,
            folder,
            path: None,
            file: None,
        }
    }

    /// Leaves out the scenarios `out` says, before the run begins (its laps
    /// made again from those left): how many.
    fn leave_out(&mut self, out: impl Fn(&Scenario) -> bool) -> usize {
        let before = self.scenarios.len();
        self.scenarios.retain(|scenario| !out(scenario));
        self.laps = laps(&self.scenarios);
        before - self.scenarios.len()
    }

    /// The scenarios of the lap under way.
    fn tried(&self) -> &[Scenario] {
        &self.scenarios[self.laps[self.lap].clone()]
    }

    /// The scenario whose setting is on: the lap's first while its race is
    /// set up, then each in its turn; none before the first lap.
    fn setting_on(&self) -> Option<&Scenario> {
        match self.phase {
            Phase::Start => None,
            Phase::Entering(_) => self.tried().first(),
            Phase::Warming { setting, .. } | Phase::Turn { setting, .. } => {
                self.tried().get(setting)
            }
        }
    }

    /// The lap under way, as the terminal says it: `[3/10] Bahía GP · 1
    /// player`.
    fn label(&self) -> String {
        let first = &self.tried()[0];
        format!(
            "[{:>2}/{}] {} · {}",
            self.lap + 1,
            self.laps.len(),
            self.circuits[first.track].name,
            players_said(first.players)
        )
    }

    /// A new lap, nothing of it measured yet.
    fn begin_lap(&mut self, length: f32) {
        let settings = self.tried().len();
        self.stretches = vec![Stretch::default(); settings];
        self.cpu = vec![(0.0, 0.0); settings];
        self.turns = 0;
        self.turn_frames.clear();
        self.turn_cpu = None;
        self.turn_from = None;
        self.length = length.max(1.0);
        self.along = None;
        self.ridden = 0.0;
        self.quarters = 0;
        self.spots.clear();
    }

    /// The turn of `setting` over: its CPU, and where on the lap it was and
    /// how fast.
    fn end_turn(&mut self, setting: usize) {
        let frames = std::mem::take(&mut self.turn_frames);
        let seconds: f32 = frames.iter().sum();
        if let Some(share) = self.turn_cpu.take().and_then(|clock| clock.share()) {
            let (sum, over) = &mut self.cpu[setting];
            *sum += share * seconds;
            *over += seconds;
        }
        if let Some(from) = self.turn_from.take()
            && seconds > 0.0
        {
            let middle = (self.start_along + (from + self.ridden) / 2.0).rem_euclid(self.length);
            self.spots
                .push((setting, middle, frames.len() as f32 / seconds));
        }
    }

    /// The lap measured: each setting's figures, in the terminal and in the
    /// file, and where it was slowest and fastest.
    fn finish_lap(&mut self, store: Option<&DiagnosticsStore>, all_round: bool) {
        let label = self.label();
        let tried = self.tried().to_vec();
        let first = &tried[0];
        let lap = format!(
            "{} with {}",
            self.circuits[first.track].name,
            players_said(first.players)
        );
        let summaries: Vec<Option<Summary>> = self.stretches.iter().map(Stretch::summary).collect();
        let fallback = process_cpu(store);
        println!(
            "{label}: {:.1} km ridden{}",
            self.ridden / 1000.0,
            if all_round {
                ""
            } else {
                " (stuck: not all round)"
            }
        );
        let mut lines = Vec::new();
        for ((scenario, summary), &(sum, over)) in tried.iter().zip(&summaries).zip(&self.cpu) {
            let cpu = (over > 0.0).then(|| sum / over).or(fallback);
            println!("    {:<22} {}", scenario.name, said(summary.as_ref(), cpu));
            lines.push(row(&self.circuits, scenario, summary.as_ref(), cpu));
            self.results.push((scenario.clone(), *summary, cpu));
        }
        let averages: Vec<f32> = summaries
            .iter()
            .map(|summary| summary.map_or(0.0, |summary| summary.fps))
            .collect();
        if let Some(((slow_at, slow), (fast_at, fast))) = extremes(&self.spots, &averages) {
            let line = format!(
                "{lap}: slowest around {:.1} km ({:+.0} %), fastest around {:.1} km ({:+.0} %)",
                slow_at / 1000.0,
                slow * 100.0,
                fast_at / 1000.0,
                fast * 100.0
            );
            println!("    {line}");
            lines.push(format!("# {line}"));
        }
        if !all_round {
            lines.push(format!(
                "# {lap}: not ridden all round, {:.1} of {:.1} km",
                self.ridden / 1000.0,
                self.length / 1000.0
            ));
        }
        if let Some(file) = &mut self.file {
            for line in &lines {
                let _ = writeln!(file, "{line}");
            }
            let _ = file.flush();
        }
    }

    /// What it is doing, shown on the screen under the frame rate: the lap,
    /// the setting on and how far round.
    fn on_screen(&self, settings: &Settings, window: UVec2, native: NativeShare) -> String {
        let Some(range) = self.laps.get(self.lap) else {
            return String::new();
        };
        let first = &self.scenarios[range.start];
        let heading = format!(
            "BENCH {}/{} · {} · {}",
            self.lap + 1,
            self.laps.len(),
            self.circuits[first.track].name,
            players_said(first.players)
        );
        let (setting, doing) = match self.phase {
            Phase::Start | Phase::Entering(_) => (None, "setting up the race".to_string()),
            Phase::Warming { setting, .. } => (Some(setting), "warming up, not measured".into()),
            Phase::Turn { setting, .. } => (
                Some(setting),
                format!(
                    "{} · {:.1} of {:.1} km",
                    if self.turn_from.is_some() {
                        "measuring"
                    } else {
                        "settling"
                    },
                    self.ridden.max(0.0) / 1000.0,
                    self.length / 1000.0
                ),
            ),
        };
        let mut lines = vec![heading];
        if let Some(scenario) =
            setting.and_then(|setting| self.scenarios.get(range.start + setting))
        {
            lines.push(scenario.name.to_string());
            lines.push(resolution_said(settings, window, native));
            lines.push(settings_said(settings, first.players));
        }
        lines.push(doing);
        lines.join("\n")
    }
}

pub struct BenchPlugin;

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        let measure = std::env::var("MOTORUST_BENCH_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|&seconds: &f32| seconds > 0.0)
            .unwrap_or(MEASURE);
        // The world's circuits, or else (without it) read here.
        let circuits = *app
            .world_mut()
            .get_resource_or_insert_with(Circuits::load)
            .into_inner();
        let tracks = tracks(
            std::env::var("MOTORUST_BENCH_TRACKS").ok().as_deref(),
            circuits.len(),
        );
        // In the folder it is run from, or else beside the performance log.
        let results = std::env::current_dir()
            .map(|here| here.join(RESULTS))
            .ok()
            .filter(|folder| std::fs::create_dir_all(folder).is_ok())
            .or_else(folder)
            .unwrap_or_else(|| PathBuf::from(RESULTS));
        let scenarios = scenarios(&tracks);
        // The performance log of the run goes beside its results, once they
        // are named.
        app.insert_resource(PerfLog::waiting())
            .insert_resource(Bench::new(
                circuits,
                scenarios,
                measure,
                machine().trim().to_string(),
                results,
            ))
            .add_systems(Startup, (start_bench, spawn_bench_label))
            .add_systems(
                Update,
                (run_bench, (show_bench, cast_ground_shadows)).chain(),
            );
    }
}

/// The track and players of `scenario`, for the race to be set up again
/// with, and its settings.
fn apply(
    scenario: &Scenario,
    settings: &mut Settings,
    current: &mut CurrentCircuit,
    game_mode: &mut GameMode,
) {
    use_setting(scenario, settings);
    let mode = if scenario.players == 2 {
        GameMode::SplitScreen
    } else {
        GameMode::SinglePlayer
    };
    if current.0 != scenario.track {
        current.0 = scenario.track;
    }
    if *game_mode != mode {
        *game_mode = mode;
    }
}

/// The settings `scenario` tries.
fn use_setting(scenario: &Scenario, settings: &mut Settings) {
    settings.shadows = scenario.shadows;
    settings.antialiasing = scenario.antialiasing;
    settings.lighter_far_trees = scenario.far_trees;
    settings.lighter_split_screen = scenario.split_detail;
    settings.render_scale = scenario.render_scale;
    settings.relief = scenario.relief;
    settings.road_relief = scenario.road_relief;
    settings.flat_at_speed = scenario.flat_at_speed;
    settings.grass_tiling = scenario.grass_tiling;
}

/// The ground in the sun's shadows or out of them, as the setting on says,
/// as it comes too (a race set up again builds it again).
fn cast_ground_shadows(
    mut commands: Commands,
    bench: Res<Bench>,
    ground: Query<(Entity, Has<NotShadowCaster>), With<GroundPiece>>,
) {
    let cast = bench
        .setting_on()
        .is_none_or(|scenario| scenario.ground_shadows);
    for (piece, left_out) in &ground {
        if cast && left_out {
            commands.entity(piece).remove::<NotShadowCaster>();
        } else if !cast && !left_out {
            commands.entity(piece).insert(NotShadowCaster);
        }
    }
}

fn start_bench(mut rules: ResMut<Rules>) {
    rules.mode = Mode::Free;
}

/// What the benchmark is doing, on the screen under the frame rate.
#[derive(Component)]
struct BenchLabel;

fn spawn_bench_label(mut commands: Commands, font: Res<UiFont>) {
    commands.spawn((
        BenchLabel,
        Text::new(""),
        // The game's font: Bevy's has no accents, nor the dots between.
        text_font(&font, 14.0),
        TextLayout::justify(Justify::Right),
        TextShadow::default(),
        Node {
            position_type: PositionType::Absolute,
            top: px(36),
            right: px(16),
            ..default()
        },
        GlobalZIndex(30),
    ));
}

fn show_bench(
    bench: Res<Bench>,
    settings: Res<Settings>,
    native: Option<Res<NativeShare>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut label: Single<&mut Text, With<BenchLabel>>,
) {
    let window = window
        .single()
        .map_or(UVec2::ZERO, |window| window.physical_size());
    let text = bench.on_screen(
        &settings,
        window,
        native.as_deref().copied().unwrap_or_default(),
    );
    if label.0 != text {
        label.0 = text;
    }
}

/// What the benchmark runs on: the machine, its GPU and system, the
/// window, and the settings it starts from, as the lines of a heading.
fn machine_lines(
    bench: &Bench,
    system: Option<&SystemInfo>,
    adapter: Option<&RenderAdapterInfo>,
    window: Option<&Window>,
    settings: &Settings,
) -> Vec<String> {
    let mut lines = vec![format!(
        "{} {} benchmark on {}",
        crate::game_info::info().folder,
        crate::build(),
        if bench.machine.is_empty() {
            "this machine"
        } else {
            &bench.machine
        }
    )];
    if let Some(system) = system {
        lines.push(format!(
            "system: {}; CPU {} ({} cores); memory {}",
            system.os, system.cpu, system.core_count, system.memory
        ));
    }
    if let Some(adapter) = adapter {
        lines.push(format!(
            "GPU: {} ({:?}, {})",
            adapter.name, adapter.backend, adapter.driver
        ));
    }
    if let Some(window) = window {
        lines.push(format!(
            "window: {} x {} pixels at {}x; resolution {:?}, fullscreen {}",
            window.physical_width(),
            window.physical_height(),
            window.scale_factor(),
            settings.resolution,
            if settings.fullscreen {
                format!("{:?}", settings.fullscreen_kind)
            } else {
                "no".into()
            }
        ));
    }
    lines.push(format!(
        "trees {}; pipelined rendering {}; frames queued {}; frame rate unlimited",
        if settings.modelled_trees {
            "new"
        } else {
            "classic"
        },
        settings.pipelined_rendering,
        settings.frame_latency
    ));
    lines.push(format!(
        "{} laps, all round each track with 1 player and with 2, the settings taking turns of \
         {TURN_MEASURE} s, each measured {} s at least; about {} min",
        bench.laps.len(),
        bench.measure,
        minutes(
            &bench.circuits,
            &bench.scenarios,
            &bench.laps,
            bench.measure
        )
    ));
    lines
}

/// About how many minutes `laps` of `scenarios` take, each setting measured
/// `measure` s at least.
fn minutes(
    circuits: &Circuits,
    scenarios: &[Scenario],
    laps: &[Range<usize>],
    measure: f32,
) -> u32 {
    let seconds: f32 = laps
        .iter()
        .map(|lap| {
            let settings = lap.len() as f32;
            let turns = settings * (measure / TURN_MEASURE).ceil();
            let round = rough_length(&circuits[scenarios[lap.start].track]) / RIDING;
            SETTING_UP + settings * WARM_TURN + round.max(turns * (TURN_SETTLE + TURN_MEASURE))
        })
        .sum();
    (seconds / 60.0).ceil() as u32
}

/// The players' bikes, and how far round the lap they are.
type Riders<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        Has<Player>,
        Option<&'static LocalPlayer>,
        Option<&'static Progress>,
    ),
    Or<(With<Player>, With<LocalPlayer>)>,
>;

/// Where on the lap the first player is (m from the line).
fn lead(riders: &Riders) -> Option<f32> {
    riders
        .iter()
        .find(|(_, player, local, _)| *player || local.is_some_and(|local| local.0 == 0))
        .and_then(|(.., progress)| progress.map(|progress| progress.along))
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_bench(
    mut commands: Commands,
    time: Res<Time<Real>>,
    state: Res<State<GameState>>,
    mut bench: ResMut<Bench>,
    mut settings: ResMut<Settings>,
    (mut current, mut game_mode, mut next, mut race, mut perf_log): (
        ResMut<CurrentCircuit>,
        ResMut<GameMode>,
        ResMut<NextState<GameState>>,
        ResMut<Race>,
        ResMut<PerfLog>,
    ),
    (riders, track): (Riders, Res<Track>),
    keyboard: Res<ButtonInput<KeyCode>>,
    (frame_start, store, system, adapter, window, two_samples): (
        Res<FrameStart>,
        Option<Res<DiagnosticsStore>>,
        Option<Res<SystemInfo>>,
        Option<Res<RenderAdapterInfo>>,
        Query<&Window, With<PrimaryWindow>>,
        Option<Res<TwoSamples>>,
    ),
    mut exit: MessageWriter<AppExit>,
) {
    // Stopped: the laps measured are kept.
    if keyboard.just_pressed(KeyCode::Escape) {
        let (done, total) = (bench.results.len(), bench.scenarios.len());
        if let Some(file) = &mut bench.file {
            let _ = writeln!(file, "# stopped after {done} of {total}");
        }
        println!("\nStopped after {done} of {total} (the lap under way left out).");
        report(&bench);
        exit.write(AppExit::Success);
        return;
    }
    let now = time.elapsed_secs();
    // The race on as soon as there is one: no lights, the players' bikes
    // ridden by the machine.
    if *state.get() == GameState::Race && !race.started {
        race.started = true;
    }
    match bench.phase {
        Phase::Start => {
            // Two samples a pixel only where the GPU has them: elsewhere
            // they would be drawn as four, and measured again as such.
            let two_samples = TwoSamples::of(two_samples.as_deref());
            let left_out = bench
                .leave_out(|scenario| scenario.antialiasing == Antialiasing::Msaa2 && !two_samples);
            if bench.laps.is_empty() {
                exit.write(AppExit::Success);
                return;
            }
            // The heading, in the terminal and in the file, named after
            // when, the system, the CPU and the GPU, with the performance
            // log beside it.
            let mut heading = machine_lines(
                &bench,
                system.as_deref(),
                adapter.as_deref(),
                window.iter().next(),
                &settings,
            );
            if left_out > 0 {
                heading.push("MSAA 2x left out: this GPU draws one or four samples a pixel".into());
            }
            println!();
            for line in &heading {
                println!("{line}");
            }
            let since = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            let name = log_name(
                since,
                system.as_deref(),
                adapter.as_deref().map(|adapter| adapter.name.as_str()),
            );
            let path = bench.folder.join(format!("{name}.txt"));
            perf_log.set_path(bench.folder.join(format!("{name}.perf.log")));
            bench.file = File::create(&path).ok().and_then(|mut file| {
                for line in &heading {
                    writeln!(file, "# {line}").ok()?;
                }
                writeln!(file, "{}", header()).ok()?;
                Some(file)
            });
            match &bench.file {
                Some(_) => println!("results: {}", path.display()),
                None => println!("the results cannot be written to {}", path.display()),
            }
            bench.path = Some(path);
            println!("Esc stops it, keeping the laps measured.\n");
            let first = bench.tried()[0].clone();
            apply(&first, &mut settings, &mut current, &mut game_mode);
            next.set(GameState::Race);
            println!("{}: setting up the race", bench.label());
            bench.phase = Phase::Entering(now);
        }
        Phase::Entering(since) => {
            if *state.get() != GameState::Race || riders.is_empty() || now - since < 0.5 {
                return;
            }
            for (bike, ..) in &riders {
                autopilot(&mut commands.entity(bike), 0.0);
            }
            bench.begin_lap(track.lap_length());
            let first = bench.tried()[0].clone();
            use_setting(&first, &mut settings);
            println!(
                "{}: warming up, each of its {} settings {WARM_TURN} s",
                bench.label(),
                bench.tried().len()
            );
            bench.phase = Phase::Warming {
                setting: 0,
                since: now,
            };
        }
        Phase::Warming { setting, since } => {
            if now - since < WARM_TURN {
                return;
            }
            let count = bench.tried().len();
            if setting + 1 < count {
                let next_setting = bench.tried()[setting + 1].clone();
                use_setting(&next_setting, &mut settings);
                bench.phase = Phase::Warming {
                    setting: setting + 1,
                    since: now,
                };
                return;
            }
            // Measured from here, all round.
            bench.along = lead(&riders);
            bench.start_along = bench.along.unwrap_or(0.0);
            bench.measuring_since = now;
            let first = turn_of(0, count);
            let scenario = bench.tried()[first].clone();
            use_setting(&scenario, &mut settings);
            println!(
                "{}: measuring all round the lap ({:.1} km), its settings taking turns of \
                 {TURN_MEASURE} s, each {} s at least",
                bench.label(),
                bench.length / 1000.0,
                bench.measure
            );
            bench.phase = Phase::Turn {
                setting: first,
                since: now,
            };
        }
        Phase::Turn { setting, since } => {
            // How far round the lap.
            if let Some(along) = lead(&riders) {
                if let Some(before) = bench.along {
                    bench.ridden += advanced(before, along, bench.length);
                }
                bench.along = Some(along);
            }
            let quarters = (bench.ridden / bench.length * 4.0).floor().clamp(0.0, 4.0) as u32;
            if quarters > bench.quarters {
                bench.quarters = quarters;
                println!(
                    "{}: {:.1} of {:.1} km",
                    bench.label(),
                    bench.ridden / 1000.0,
                    bench.length / 1000.0
                );
            }
            let elapsed = now - since;
            if elapsed >= TURN_SETTLE {
                if bench.turn_from.is_none() {
                    bench.turn_from = Some(bench.ridden);
                    bench.turn_cpu = CpuClock::start();
                }
                let delta = time.delta_secs();
                bench.stretches[setting].frames.push(delta);
                bench.turn_frames.push(delta);
                if let Some(update) = frame_start.elapsed() {
                    bench.stretches[setting].updates.push(update);
                }
            }
            if elapsed < TURN_SETTLE + TURN_MEASURE {
                return;
            }
            bench.end_turn(setting);
            let measure = bench.measure;
            let measured = bench
                .stretches
                .iter()
                .all(|stretch| stretch.frames.iter().sum::<f32>() >= measure);
            let all_round = bench.ridden >= bench.length;
            let stuck = now - bench.measuring_since > bench.length / STUCK + 60.0;
            if !(all_round && measured || stuck) {
                bench.turns += 1;
                let next_setting = turn_of(bench.turns, bench.tried().len());
                let scenario = bench.tried()[next_setting].clone();
                use_setting(&scenario, &mut settings);
                bench.phase = Phase::Turn {
                    setting: next_setting,
                    since: now,
                };
                return;
            }
            bench.finish_lap(store.as_deref(), all_round);
            // The next lap, or the end.
            bench.lap += 1;
            if bench.lap >= bench.laps.len() {
                bench.lap = bench.laps.len() - 1;
                if let Some(file) = &mut bench.file {
                    let _ = writeln!(file, "# finished");
                }
                report(&bench);
                exit.write(AppExit::Success);
                return;
            }
            let first = bench.tried()[0].clone();
            apply(&first, &mut settings, &mut current, &mut game_mode);
            next.set(GameState::Race);
            println!("{}: setting up the race", bench.label());
            bench.phase = Phase::Entering(now);
        }
    }
}

/// What a scenario measured, said in a line of the terminal.
fn said(summary: Option<&Summary>, cpu: Option<f32>) -> String {
    let Some(s) = summary else {
        return "no frames".into();
    };
    let mut line = format!(
        "{:.1} fps · 1% low {:.1} · {:.2} ms (95%: {:.2} ms)",
        s.fps, s.low, s.p50, s.p95
    );
    if let Some(update) = s.update {
        line.push_str(&format!(" · simulation {update:.2} ms"));
    }
    if let Some(cpu) = cpu {
        line.push_str(&format!(" · CPU {cpu:.0} %"));
    }
    line
}

/// The heading of the table of results.
fn header() -> String {
    format!(
        "{:<16} {:<22} {:>8} {:>10} {:>8} {:>8} {:>8} {:>6}",
        "track", "scenario", "fps", "1% low", "p50 ms", "p95 ms", "sim ms", "cpu %"
    )
}

/// A line of the table: a scenario and what it measured.
fn row(
    circuits: &Circuits,
    scenario: &Scenario,
    summary: Option<&Summary>,
    cpu: Option<f32>,
) -> String {
    let track = circuits[scenario.track].name;
    let or_dash = |value: Option<f32>, decimals: usize| {
        value.map_or("-".to_string(), |value| format!("{value:.decimals$}"))
    };
    match summary {
        Some(s) => format!(
            "{track:<16} {:<22} {:>8.1} {:>10.1} {:>8.2} {:>8.2} {:>8} {:>6}",
            scenario.name,
            s.fps,
            s.low,
            s.p50,
            s.p95,
            or_dash(s.update, 2),
            or_dash(cpu, 0)
        ),
        None => format!("{track:<16} {:<22} {:>8}", scenario.name, "-"),
    }
}

/// The results as a table: a line for each scenario.
pub fn table(circuits: &Circuits, results: &[(Scenario, Option<Summary>, Option<f32>)]) -> String {
    std::iter::once(header())
        .chain(
            results
                .iter()
                .map(|(scenario, summary, cpu)| row(circuits, scenario, summary.as_ref(), *cpu)),
        )
        .collect::<Vec<_>>()
        .join("\n")
}

/// Prints the results (they are in their file already).
fn report(bench: &Bench) {
    println!("\n{}\n", table(&bench.circuits, &bench.results));
    if let (Some(_), Some(path)) = (&bench.file, &bench.path) {
        println!("Written to {}", path.display());
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    pub fn every_track_is_ridden_under_each_setting_with_one_change_at_a_time_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let all = scenarios(&tracks(None, circuits.len()));
        assert_eq!(all.len(), 22 * circuits.len());
        let first = &all[..22];
        assert!(first.iter().all(|scenario| scenario.track == 0));
        // The first the reference; the others one change from it.
        let base = &first[0];
        assert_eq!(
            (
                base.players,
                base.shadows,
                base.antialiasing,
                base.far_trees,
                base.render_scale,
                base.relief,
                base.road_relief,
                base.flat_at_speed,
                base.grass_tiling,
                base.ground_shadows
            ),
            (
                1,
                ShadowQuality::High,
                Antialiasing::Msaa,
                false,
                100,
                Relief::Normal,
                false,
                true,
                GrassTiling::Wide,
                true
            )
        );
        for scenario in &first[1..] {
            let changes = [
                scenario.players != base.players,
                scenario.shadows != base.shadows,
                scenario.antialiasing != base.antialiasing,
                scenario.far_trees != base.far_trees,
                scenario.split_detail != base.split_detail && scenario.players == 2,
                scenario.render_scale != base.render_scale,
                scenario.relief != base.relief,
                scenario.road_relief != base.road_relief,
                scenario.flat_at_speed != base.flat_at_speed,
                scenario.grass_tiling != base.grass_tiling,
                scenario.ground_shadows != base.ground_shadows,
            ]
            .iter()
            .filter(|&&changed| changed)
            .count();
            assert!((1..=2).contains(&changes), "{}", scenario.name);
        }
        // The players change once a track, after all of one player's.
        let two = first
            .iter()
            .position(|scenario| scenario.players == 2)
            .unwrap();
        assert!(first[two..].iter().all(|scenario| scenario.players == 2));
    }

    #[test]
    fn every_track_is_ridden_under_each_setting_with_one_change_at_a_time() {
        every_track_is_ridden_under_each_setting_with_one_change_at_a_time_on(
            &crate::testing::DEMO,
        );
    }

    pub fn the_ground_is_left_out_of_the_shadows_only_in_its_turn_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut app = App::new();
        app.insert_resource(Bench::new(
            *circuits,
            scenarios(&[0]),
            MEASURE,
            String::new(),
            PathBuf::new(),
        ))
        .add_systems(Update, cast_ground_shadows);
        let ground = app.world_mut().spawn(GroundPiece).id();
        let unshadowed = |app: &App| app.world().get::<NotShadowCaster>(ground).is_some();
        let turn = |app: &mut App, name: &str| {
            let mut bench = app.world_mut().resource_mut::<Bench>();
            let setting = bench
                .tried()
                .iter()
                .position(|scenario| scenario.name == name)
                .unwrap();
            bench.phase = Phase::Turn {
                setting,
                since: 0.0,
            };
            app.update();
        };
        // Before the run and while a race is set up: in the shadows.
        app.update();
        assert!(!unshadowed(&app));
        app.world_mut().resource_mut::<Bench>().phase = Phase::Entering(0.0);
        app.update();
        assert!(!unshadowed(&app));
        turn(&mut app, "1p-no-ground-shadows");
        assert!(unshadowed(&app));
        // Ground built again in that turn is left out too.
        let rebuilt = app.world_mut().spawn(GroundPiece).id();
        app.update();
        assert!(app.world().get::<NotShadowCaster>(rebuilt).is_some());
        turn(&mut app, "1p-shadows-low");
        assert!(!unshadowed(&app));
        assert!(app.world().get::<NotShadowCaster>(rebuilt).is_none());
    }

    #[test]
    fn the_ground_is_left_out_of_the_shadows_only_in_its_turn() {
        the_ground_is_left_out_of_the_shadows_only_in_its_turn_on(&crate::testing::DEMO);
    }

    pub fn two_samples_are_left_out_where_the_gpu_has_not_got_them_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut bench = Bench::new(
            *circuits,
            scenarios(&[0]),
            MEASURE,
            String::new(),
            PathBuf::new(),
        );
        assert_eq!(bench.leave_out(|_| false), 0);
        let left_out = bench.leave_out(|scenario| scenario.antialiasing == Antialiasing::Msaa2);
        assert_eq!(left_out, 1);
        assert!(
            bench
                .scenarios
                .iter()
                .all(|scenario| scenario.name != "1p-msaa-2x")
        );
        // The laps made again: one player's one fewer, two players' as many.
        assert_eq!(
            bench.laps.iter().map(|lap| lap.len()).collect::<Vec<_>>(),
            [16, 5]
        );
    }

    #[test]
    fn two_samples_are_left_out_where_the_gpu_has_not_got_them() {
        two_samples_are_left_out_where_the_gpu_has_not_got_them_on(&crate::testing::DEMO);
    }

    pub fn the_tracks_asked_for_or_all_on(circuits: &'static crate::circuits::Circuits) {
        assert_eq!(
            tracks(None, circuits.len()),
            (0..circuits.len()).collect::<Vec<_>>()
        );
        assert_eq!(tracks(Some("0, 2"), circuits.len()), [0, 2]);
        assert_eq!(
            tracks(Some("99,x"), circuits.len()),
            (0..circuits.len()).collect::<Vec<_>>()
        );
    }

    pub fn the_results_read_as_a_table_on(circuits: &'static crate::circuits::Circuits) {
        let scenario = scenarios(&[0])[0].clone();
        let summary = Summary {
            fps: 72.25,
            low: 41.0,
            p50: 13.5,
            p95: 18.25,
            update: Some(2.5),
        };
        let table = table(
            circuits,
            &[
                (scenario.clone(), Some(summary), Some(34.0)),
                (scenario, None, None),
            ],
        );
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("track"));
        assert!(
            lines[1].contains("72.2") || lines[1].contains("72.3"),
            "{table}"
        );
        assert!(
            lines[1].contains("18.25") && lines[1].contains("2.50"),
            "{table}"
        );
        assert!(lines[1].trim_end().ends_with("34"), "{table}");
        assert!(lines[2].trim_end().ends_with('-'));
        assert!(said(Some(&summary), Some(34.0)).contains("simulation 2.50 ms · CPU 34 %"));
    }

    #[test]
    fn the_results_read_as_a_table() {
        the_results_read_as_a_table_on(&crate::testing::DEMO);
    }

    #[test]
    fn each_track_is_one_lap_with_one_player_and_one_with_two() {
        let all = scenarios(&[0, 3]);
        let laps = laps(&all);
        assert_eq!(laps.len(), 4);
        for lap in &laps {
            let first = &all[lap.start];
            assert!(all[lap.clone()].iter().all(|scenario| {
                scenario.track == first.track && scenario.players == first.players
            }));
        }
        assert_eq!(laps[0].len(), 17);
        assert_eq!(laps[1].len(), 5);
        assert_eq!(all[laps[2].start].track, 3);
    }

    #[test]
    fn the_settings_take_every_place_of_a_round_in_turn() {
        for settings in [2, 3, 6] {
            let order: Vec<usize> = (0..settings * settings)
                .map(|turn| turn_of(turn, settings))
                .collect();
            // Each round has each of them once...
            for round in order.chunks(settings) {
                let mut sorted = round.to_vec();
                sorted.sort_unstable();
                assert_eq!(sorted, (0..settings).collect::<Vec<_>>(), "{order:?}");
            }
            // ...and over as many rounds, each one in each place once.
            for place in 0..settings {
                let mut at: Vec<usize> = order.chunks(settings).map(|round| round[place]).collect();
                at.sort_unstable();
                assert_eq!(at, (0..settings).collect::<Vec<_>>(), "{order:?}");
            }
        }
    }

    #[test]
    fn how_far_round_the_lap_over_the_line() {
        assert_eq!(advanced(100.0, 130.0, 1600.0), 30.0);
        // Over the line.
        assert_eq!(advanced(1590.0, 20.0, 1600.0), 30.0);
        // Back over it.
        assert_eq!(advanced(10.0, 1590.0, 1600.0), -20.0);
    }

    #[test]
    fn where_the_lap_is_slowest_and_fastest_against_each_settings_average() {
        // Setting 1 is slower everywhere: its slow spot is against its own
        // average, not the other's.
        let turns = [
            (0, 100.0, 120.0),
            (1, 400.0, 85.0),
            (0, 700.0, 90.0),
            (1, 1000.0, 100.0),
        ];
        let ((slow_at, slow), (fast_at, fast)) = extremes(&turns, &[100.0, 90.0]).unwrap();
        assert_eq!(slow_at, 700.0);
        assert!((slow + 0.1).abs() < 1e-6);
        assert_eq!(fast_at, 100.0);
        assert!((fast - 0.2).abs() < 1e-6);
        assert_eq!(extremes(&[], &[100.0]), None);
    }

    #[test]
    fn the_settings_on_read_on_the_screen() {
        let mut settings = Settings {
            shadows: ShadowQuality::Medium,
            antialiasing: Antialiasing::Fxaa,
            lighter_far_trees: true,
            ..default()
        };
        assert_eq!(
            settings_said(&settings, 1),
            "shadows medium · FXAA · far trees lighter · no relief · grass wide"
        );
        settings.lighter_split_screen = false;
        assert!(settings_said(&settings, 2).ends_with(" · all the detail"));
        settings.relief = Relief::Normal;
        settings.road_relief = true;
        settings.flat_at_speed = false;
        assert!(settings_said(&settings, 1).contains("normal maps (road too) at speed too"));
        // The pixels the world is drawn at.
        let retina = UVec2::new(3024, 1898);
        let whole = NativeShare::default();
        assert_eq!(
            resolution_said(&settings, retina, whole),
            "3D 3024 × 1898 · 100 %"
        );
        settings.render_scale = 70;
        assert_eq!(
            resolution_said(&settings, retina, whole),
            "3D 2117 × 1329 · 70 %"
        );
        // On a 1440p monitor in HiDPI, never more than its own pixels.
        settings.render_scale = 100;
        assert_eq!(
            resolution_said(&settings, UVec2::new(4096, 2304), NativeShare(0.625)),
            "3D 2560 × 1440 · 100 %"
        );
    }

    pub fn a_run_takes_about_as_long_as_its_laps_on(circuits: &'static crate::circuits::Circuits) {
        let all = scenarios(&[0]);
        let laps = laps(&all);
        let minutes = minutes(circuits, &all, &laps, MEASURE);
        // Bahía's two laps: a couple of minutes, not ten.
        assert!((2..=6).contains(&minutes), "{minutes}");
        assert!(rough_length(&circuits[0]) > 1000.0 && rough_length(&circuits[0]) < 2000.0);
    }

    #[test]
    fn a_run_takes_about_as_long_as_its_laps() {
        a_run_takes_about_as_long_as_its_laps_on(&crate::testing::DEMO);
    }
}
