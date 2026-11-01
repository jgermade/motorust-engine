//! The performance log: every few seconds, how fast the frames came (their
//! average rate, the slowest ones, the times at a few percentiles), how
//! long the simulation took of each (the CPU's share: the rest is drawing,
//! or waiting for the GPU), the CPU and memory the game and the system use,
//! how many entities there are, with what was being played and the graphics
//! options in use, written as one line of `key=value` pairs to a file of its
//! own for each session, beside the settings (on macOS,
//! `~/Library/Application Support/motorust/logs`). A stretch ends early when
//! anything it is noted with changes (the screen, the mode, an option, the
//! resolution), so no line mixes two of them. On the desktop only, and while
//! Options > Graphics > "Performance log" is on.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use bevy::diagnostic::{
    DiagnosticPath, DiagnosticsStore, SystemInfo, SystemInformationDiagnosticsPlugin,
};
use bevy::ecs::entity::Entities;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::PrimaryWindow;

use crate::circuits::{Circuits, CurrentCircuit};
use crate::menu::GameState;
use crate::pause::Pause;
use crate::race::Rules;
use crate::render_scale::{NativeShare, drawn_size, render_percent};
use crate::settings::Settings;
use crate::welcome::GameMode;
use crate::world::Dressed;
use crate::world::detail::{Detail, TwoSamples};

/// How long each line of the log covers (s), at most.
const STRETCH: f32 = 5.0;

pub struct PerfLogPlugin;

impl Plugin for PerfLogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PerfLog>()
            .init_resource::<FrameStart>()
            .add_systems(First, mark_frame_start)
            .add_systems(Last, log_performance);
    }
}

/// What a stretch of the log is noted with: when any of it changes, a new
/// stretch begins.
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    pub state: String,
    pub mode: String,
    pub players: u32,
    pub paused: bool,
    pub track: String,
    pub trees: &'static str,
    pub far_trees: bool,
    pub split_detail: bool,
    pub shadows: String,
    pub antialiasing: String,
    /// The relief of the scenery's textures.
    pub relief: String,
    /// Whether the road has relief too, and whether the relief is dropped
    /// at speed.
    pub road_relief: bool,
    pub flat_at_speed: bool,
    /// How the lawn's repetition is broken up.
    pub grass_tiling: String,
    /// The performance forced: off, or the frame rate it keeps.
    pub performance: String,
    /// The sea: mirroring or the cartoon one.
    pub water: &'static str,
    /// The render scale (%) and the pixels the world is drawn at.
    pub render_scale: u32,
    pub render: String,
    pub fps_limit: String,
    pub resolution: String,
    pub fullscreen: String,
    pub pipelined: bool,
    pub frame_latency: u32,
    pub window: String,
    pub ui_scale: f32,
}

impl Context {
    fn pairs(&self) -> String {
        format!(
            "state={} mode={} players={} paused={} track={:?} trees={} lighter_far_trees={} \
             lighter_split_screen={} force_performance={} water={} shadows={} antialiasing={} relief={} road_relief={} flat_at_speed={} grass_tiling={} render_scale={} render={} fps_limit={} \
             resolution={} fullscreen={} pipelined_rendering={} frame_latency={} window={} ui_scale={:.2}",
            self.state,
            self.mode,
            self.players,
            self.paused,
            self.track,
            self.trees,
            self.far_trees,
            self.split_detail,
            self.performance,
            self.water,
            self.shadows,
            self.antialiasing,
            self.relief,
            self.road_relief,
            self.flat_at_speed,
            self.grass_tiling,
            self.render_scale,
            self.render,
            self.fps_limit,
            self.resolution,
            self.fullscreen,
            self.pipelined,
            self.frame_latency,
            self.window,
            self.ui_scale,
        )
    }
}

/// How a stretch went: the frames' times (s), and how long the simulation
/// took of each (s, from the start of its update to the end).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stretch {
    pub frames: Vec<f32>,
    pub updates: Vec<f32>,
}

/// The time at `share` (0 to 1) of `times` (s), sorted, in ms.
fn percentile(sorted: &[f32], share: f32) -> f32 {
    let index = ((sorted.len() as f32 - 1.0) * share).round() as usize;
    sorted[index.min(sorted.len() - 1)] * 1000.0
}

/// What a stretch comes to: its average rate, the rate of its slowest 1 %
/// of frames, its frames' times (ms) at the middle and at 95 %, and the
/// simulation's (ms) at the middle, if it was timed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    pub fps: f32,
    pub low: f32,
    pub p50: f32,
    pub p95: f32,
    pub update: Option<f32>,
}

impl Stretch {
    pub fn summary(&self) -> Option<Summary> {
        if self.frames.is_empty() {
            return None;
        }
        let mut sorted = self.frames.clone();
        sorted.sort_by(f32::total_cmp);
        let total: f32 = sorted.iter().sum();
        let slowest = (sorted.len() / 100).max(1);
        let low: f32 = sorted.iter().rev().take(slowest).sum::<f32>() / slowest as f32;
        let mut updates = self.updates.clone();
        updates.sort_by(f32::total_cmp);
        Some(Summary {
            fps: sorted.len() as f32 / total.max(1e-6),
            low: 1.0 / low.max(1e-6),
            p50: percentile(&sorted, 0.5),
            p95: percentile(&sorted, 0.95),
            update: (!updates.is_empty()).then(|| percentile(&updates, 0.5)),
        })
    }

    /// Its figures: frames, how long it lasted (s), the average rate, the
    /// rate of the slowest 1 % of the frames, the times (ms) at the middle,
    /// at 95 % and 99 %, and the longest; and the simulation's (ms) at the
    /// middle and at 95 %.
    pub fn figures(&self) -> Option<String> {
        if self.frames.is_empty() {
            return None;
        }
        let mut sorted = self.frames.clone();
        sorted.sort_by(f32::total_cmp);
        let total: f32 = sorted.iter().sum();
        let at = |share: f32| percentile(&sorted, share);
        let mut updates = self.updates.clone();
        updates.sort_by(f32::total_cmp);
        let update = if updates.is_empty() {
            String::new()
        } else {
            format!(
                " update_ms_p50={:.2} update_ms_p95={:.2}",
                percentile(&updates, 0.5),
                percentile(&updates, 0.95)
            )
        };
        // The slowest 1 % (at least one frame), as a rate.
        let slowest = (sorted.len() / 100).max(1);
        let low: f32 = sorted.iter().rev().take(slowest).sum::<f32>() / slowest as f32;
        Some(format!(
            "frames={} seconds={:.2} fps={:.1} fps_1pct_low={:.1} ms_p50={:.2} ms_p95={:.2} \
             ms_p99={:.2} ms_max={:.2}{update}",
            sorted.len(),
            total,
            sorted.len() as f32 / total.max(1e-6),
            1.0 / low.max(1e-6),
            at(0.5),
            at(0.95),
            at(0.99),
            sorted[sorted.len() - 1] * 1000.0,
        ))
    }
}

/// The log of this session: its file (opened when the first line is
/// written), when the stretch under way began and what it is noted with.
#[derive(Resource, Default)]
pub struct PerfLog {
    /// Where it goes, if not to a file of its own beside the settings; and
    /// whether it waits to be told where (dropping what comes before).
    path: Option<PathBuf>,
    waiting: bool,
    file: Option<File>,
    failed: bool,
    started: Option<f32>,
    context: Option<Context>,
    stretch: Stretch,
    /// The game's CPU time when the stretch began.
    cpu: Option<CpuClock>,
}

/// When this frame's update began.
#[derive(Resource, Default)]
pub struct FrameStart(Option<Instant>);

impl FrameStart {
    /// How long this frame's update has taken so far (s).
    pub fn elapsed(&self) -> Option<f32> {
        self.0.map(|start| start.elapsed().as_secs_f32())
    }
}

fn mark_frame_start(mut start: ResMut<FrameStart>) {
    start.0 = Some(Instant::now());
}

/// The CPU time the game has had so far (s), all its threads together.
#[cfg(unix)]
fn cpu_time() -> Option<f64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: `getrusage` fills in the struct it is given, and says whether
    // it did.
    let usage = unsafe {
        if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) != 0 {
            return None;
        }
        usage.assume_init()
    };
    let seconds = |time: libc::timeval| time.tv_sec as f64 + time.tv_usec as f64 / 1e6;
    Some(seconds(usage.ru_utime) + seconds(usage.ru_stime))
}

#[cfg(not(unix))]
fn cpu_time() -> Option<f64> {
    None
}

/// The memory the game takes (GiB), as Activity Monitor shows it: Bevy's
/// reading of it is left out on macOS (its `sysinfo` is built for the App
/// Store, which reads no processes), and so is the CPU's.
#[cfg(target_os = "macos")]
fn memory() -> Option<f64> {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: `proc_pid_rusage` fills in the struct of the kind asked for
    // (`RUSAGE_INFO_V2`), and says whether it did.
    let info = unsafe {
        let pid = std::process::id() as libc::c_int;
        if libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, info.as_mut_ptr().cast()) != 0 {
            return None;
        }
        info.assume_init()
    };
    Some(info.ri_phys_footprint as f64 / (1u64 << 30) as f64)
}

#[cfg(not(target_os = "macos"))]
fn memory() -> Option<f64> {
    None
}

/// The game's share of all the cores together (%), having had `cpu` s of
/// CPU time over `wall` s on `cores` of them.
fn cpu_share(cpu: f64, wall: f64, cores: usize) -> Option<f32> {
    (wall > 0.0 && cores > 0).then(|| (cpu / wall / cores as f64 * 100.0) as f32)
}

/// The game's CPU time when a stretch began, to tell its share of the
/// cores over the stretch.
#[derive(Clone, Copy, Debug)]
pub struct CpuClock {
    wall: Instant,
    cpu: f64,
}

impl CpuClock {
    /// From now, where the game's CPU time can be read.
    pub fn start() -> Option<Self> {
        let cpu = cpu_time()?;
        Some(Self {
            wall: Instant::now(),
            cpu,
        })
    }

    /// The game's share of all the cores since it started (%).
    pub fn share(&self) -> Option<f32> {
        let cores = std::thread::available_parallelism().ok()?.get();
        cpu_share(
            cpu_time()? - self.cpu,
            self.wall.elapsed().as_secs_f64(),
            cores,
        )
    }
}

/// The CPU and memory the game and the whole system use: the game's CPU
/// over the stretch (`cpu`), as a share of all the cores together, where it
/// can be read (not on Windows), and its memory now (on macOS); the rest
/// averaged over the last few readings (taken about once a second) if they
/// are measured (see [`SystemInformationDiagnosticsPlugin`]).
fn usage(store: Option<&DiagnosticsStore>, cpu: Option<f32>) -> String {
    let read = |path: &DiagnosticPath| {
        store
            .and_then(|store| store.get(path))
            .and_then(|diagnostic| diagnostic.average())
    };
    let mut pairs = String::new();
    for (key, value) in [
        (
            "cpu_process_pct",
            cpu.map(f64::from)
                .or_else(|| read(&SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE)),
        ),
        (
            "cpu_system_pct",
            read(&SystemInformationDiagnosticsPlugin::SYSTEM_CPU_USAGE),
        ),
        (
            "mem_process_gib",
            memory().or_else(|| read(&SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE)),
        ),
        (
            "mem_system_pct",
            read(&SystemInformationDiagnosticsPlugin::SYSTEM_MEM_USAGE),
        ),
    ] {
        if let Some(value) = value {
            pairs.push_str(&format!(" {key}={value:.2}"));
        }
    }
    pairs
}

/// The game's share of all the cores (%) over the last few readings, as
/// Bevy reads it (where the game's own reading of it cannot be had).
pub fn process_cpu(store: Option<&DiagnosticsStore>) -> Option<f32> {
    store?
        .get(&SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE)?
        .average()
        .map(|cpu| cpu as f32)
}

/// `seconds` since 1970 as a date and time (UTC) for a file name:
/// `2026-10-16_1830`.
pub fn stamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let (hour, minute) = (seconds % 86_400 / 3600, seconds % 3600 / 60);
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}_{hour:02}{minute:02}")
}

/// `name` as it can go in a file name: lowercase letters and digits, a
/// dash between words, at most 40 characters (cut between words).
pub fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut slug = slug.trim_end_matches('-').to_string();
    if slug.len() > 40 {
        slug.truncate(40);
        if let Some(cut) = slug.rfind('-') {
            slug.truncate(cut);
        }
    }
    slug
}

/// A CPU's or GPU's name as it can go in a file name, without what says
/// nothing of it: trademarks, its clock, its driver (in brackets or after
/// a slash), "CPU", "Processor" and how many cores.
fn chip(name: &str) -> String {
    let mut name = name.to_string();
    for mark in ["(R)", "(r)", "(TM)", "(tm)", "®", "™"] {
        name = name.replace(mark, " ");
    }
    for cut in [" @", " (", "/"] {
        if let Some(at) = name.find(cut) {
            name.truncate(at);
        }
    }
    let words: Vec<&str> = name
        .split_whitespace()
        .filter(|word| {
            let word = word.to_lowercase();
            word != "cpu" && word != "processor" && !word.ends_with("-core")
        })
        .collect();
    slug(&words.join(" "))
}

/// The name of a log (without its ending): when it began (UTC), the system,
/// the CPU and the GPU (once, where they are one chip), as
/// `2026-10-16_1830_macos-26-3-1_apple-m1-pro`.
pub fn log_name(seconds: u64, system: Option<&SystemInfo>, gpu: Option<&str>) -> String {
    let os = slug(system.map_or(std::env::consts::OS, |system| system.os.as_str()));
    let cpu = system.map(|system| chip(&system.cpu));
    let gpu = gpu.map(chip).filter(|gpu| Some(gpu) != cpu.as_ref());
    [Some(stamp(seconds)), Some(os), cpu, gpu]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

/// The folder the logs go in: beside the settings.
pub fn folder() -> Option<PathBuf> {
    let var = |name| std::env::var_os(name).map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        var("HOME")?.join("Library/Application Support")
    } else if cfg!(windows) {
        var("APPDATA")?
    } else {
        match var("XDG_CONFIG_HOME").filter(|path| path.is_absolute()) {
            Some(folder) => folder,
            None => var("HOME")?.join(".config"),
        }
    };
    Some(base.join(crate::game_info::info().folder).join("logs"))
}

impl PerfLog {
    /// A log that waits to be told where to go ([`PerfLog::set_path`]):
    /// the benchmark's, beside its results once it names them.
    pub fn waiting() -> Self {
        Self {
            waiting: true,
            ..default()
        }
    }

    /// Where it goes, if it has not been written to yet.
    pub fn set_path(&mut self, path: PathBuf) {
        if self.file.is_none() {
            self.path = Some(path);
            self.waiting = false;
        }
    }

    /// Writes `line` to the file, opening it first (a new one for each
    /// session, named by `name` from when it began, see [`log_name`]), with
    /// a heading.
    fn write(
        &mut self,
        line: &str,
        name: impl FnOnce(u64) -> String,
        heading: impl FnOnce() -> String,
    ) {
        if self.failed || self.waiting || cfg!(target_arch = "wasm32") {
            return;
        }
        if self.file.is_none() {
            let since = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            let path = self
                .path
                .clone()
                .or_else(|| Some(folder()?.join(format!("{}.perf.log", name(since)))));
            let opened = path.and_then(|path| {
                std::fs::create_dir_all(path.parent()?).ok()?;
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .ok()?;
                info!("Performance log: {}", path.display());
                Some((file, since))
            });
            let Some((mut file, since)) = opened else {
                warn!("The performance log cannot be written");
                self.failed = true;
                return;
            };
            let _ = writeln!(
                file,
                "# {} {} perf log, unix time {since}",
                crate::game_info::info().folder,
                crate::build()
            );
            let _ = writeln!(file, "# {}", heading());
            self.file = Some(file);
        }
        if let Some(file) = &mut self.file
            && writeln!(file, "{line}").is_err()
        {
            self.failed = true;
        }
    }
}

/// Notes each frame's time, and every [`STRETCH`] s (or as soon as what it
/// is noted with changes) writes a line for the stretch.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn log_performance(
    time: Res<Time<Real>>,
    settings: Res<Settings>,
    state: Res<State<GameState>>,
    rules: Res<Rules>,
    game_mode: Res<GameMode>,
    pause: Res<Pause>,
    current: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    (window, ui_scale, adapter, system, trees): (
        Query<&Window, With<PrimaryWindow>>,
        Option<Res<UiScale>>,
        Option<Res<RenderAdapterInfo>>,
        Option<Res<SystemInfo>>,
        Query<(&Dressed, &InheritedVisibility)>,
    ),
    (start, store, entities, detail, native, two_samples): (
        Res<FrameStart>,
        Option<Res<DiagnosticsStore>>,
        &Entities,
        Option<Res<Detail>>,
        Option<Res<NativeShare>>,
        Option<Res<TwoSamples>>,
    ),
    mut log: ResMut<PerfLog>,
) {
    if !settings.performance_log {
        // Off: what was under way is dropped, and a new stretch begins
        // when it is on again.
        log.started = None;
        log.context = None;
        log.stretch = Stretch::default();
        return;
    }
    let now = time.elapsed_secs();
    let window = window.iter().next();
    let context = Context {
        state: format!("{:?}", state.get()),
        mode: format!("{:?}", rules.mode),
        players: if *game_mode == GameMode::SplitScreen {
            2
        } else {
            1
        },
        paused: pause.is_open(),
        track: circuits.get(*current).name.to_string(),
        trees: if settings.modelled_trees {
            "new"
        } else {
            "classic"
        },
        far_trees: settings.lighter_far_trees,
        split_detail: settings.lighter_split_screen,
        performance: format!("{:?}", settings.force_performance),
        water: if settings.cartoon_water {
            "cartoon"
        } else {
            "mirror"
        },
        // The quality chosen, and the level the shadows are drawn at now.
        shadows: format!(
            "{:?}/{:?}",
            settings.shadows,
            detail.as_deref().map(|detail| detail.level)
        ),
        // As chosen, and as drawn now (the performance forced may have
        // given up the multisampling, or the parallax).
        antialiasing: format!(
            "{:?}/{:?}",
            settings.antialiasing,
            crate::world::detail::in_use(
                settings.antialiasing,
                detail.as_deref().is_none_or(|detail| detail.msaa),
                TwoSamples::of(two_samples.as_deref())
            )
        ),
        relief: format!(
            "{:?}/{:?}",
            settings.relief,
            crate::world::relief::capped(
                settings.relief,
                detail.as_deref().is_none_or(|detail| detail.parallax)
            )
        ),
        road_relief: settings.road_relief,
        flat_at_speed: settings.flat_at_speed,
        grass_tiling: format!("{:?}", settings.grass_tiling),
        // As the options say, or as far as the performance forced has come.
        render_scale: render_percent(detail.as_deref()),
        render: window.map_or_else(
            || "none".into(),
            |window| {
                let size = drawn_size(
                    window.physical_size(),
                    render_percent(detail.as_deref()),
                    native.as_deref().copied().unwrap_or_default(),
                );
                format!("{}x{}", size.x, size.y)
            },
        ),
        fps_limit: format!("{:?}", settings.fps),
        resolution: format!("{:?}", settings.resolution),
        fullscreen: if settings.fullscreen {
            format!("{:?}", settings.fullscreen_kind)
        } else {
            "no".into()
        },
        pipelined: settings.pipelined_rendering,
        frame_latency: settings.frame_latency,
        window: window.map_or_else(
            || "none".into(),
            |window| {
                format!(
                    "{}x{}@{}",
                    window.physical_width(),
                    window.physical_height(),
                    window.scale_factor()
                )
            },
        ),
        ui_scale: ui_scale.map_or(1.0, |scale| scale.0),
    };
    let changed = log.context.as_ref() != Some(&context);
    let due = log.started.is_some_and(|started| now - started >= STRETCH);
    if changed || due {
        if let (Some(figures), Some(noted)) = (log.stretch.figures(), log.context.clone()) {
            // The trees in sight now, by the mesh they are drawn with:
            // full, lighter, lightest.
            let mut levels = [0; 3];
            for (dressed, visible) in &trees {
                if visible.get() {
                    levels[dressed.level().min(2)] += 1;
                }
            }
            let levels = format!(" tree_levels={}/{}/{}", levels[0], levels[1], levels[2]);
            let cpu = log.cpu.and_then(|clock| clock.share());
            let line = format!(
                "t={now:.1} {figures}{} entities={} {}{levels}",
                usage(store.as_deref(), cpu),
                entities.count_spawned(),
                noted.pairs()
            );
            let gpu = adapter.as_deref().map_or_else(
                || "GPU unknown".to_string(),
                |adapter| {
                    format!(
                        "GPU {} ({:?}, {})",
                        adapter.name, adapter.backend, adapter.driver
                    )
                },
            );
            let system = system.as_deref();
            let gpu_name = adapter.as_deref().map(|adapter| adapter.name.as_str());
            log.write(
                &line,
                |since| log_name(since, system, gpu_name),
                || {
                    format!(
                        "{gpu}; CPU {}, {} CPUs; {}",
                        system.map_or("unknown", |system| system.cpu.as_str()),
                        std::thread::available_parallelism().map_or(0, |n| n.get()),
                        system.map_or(std::env::consts::OS, |system| system.os.as_str())
                    )
                },
            );
        }
        log.stretch = Stretch::default();
        log.started = Some(now);
        log.context = Some(context);
        log.cpu = CpuClock::start();
    }
    // This frame's time (the first after a stretch begins counts in it),
    // and its update's so far, nearly all of it.
    let delta = time.delta_secs();
    if delta > 0.0 {
        log.stretch.frames.push(delta);
    }
    if let Some(started) = start.0 {
        log.stretch.updates.push(started.elapsed().as_secs_f32());
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn a_log_is_named_after_when_and_on_what() {
        assert_eq!(stamp(0), "1970-01-01_0000");
        assert_eq!(stamp(1_792_083_939), "2026-10-15_1705");
        // The last day of a leap February.
        assert_eq!(
            stamp(1_709_164_800 + 3_600 * 23 + 59 * 60),
            "2024-02-29_2359"
        );
        let system = |os: &str, cpu: &str| SystemInfo {
            os: os.into(),
            kernel: String::new(),
            cpu: cpu.into(),
            core_count: "8".into(),
            memory: "16.0 GiB".into(),
        };
        // An Apple chip is its CPU and its GPU: named once.
        let mac = system("macOS 26.3.1", "Apple M1 Pro");
        assert_eq!(
            log_name(1_792_083_939, Some(&mac), Some("Apple M1 Pro")),
            "2026-10-15_1705_macos-26-3-1_apple-m1-pro"
        );
        // What says nothing of the chips left out.
        let pc = system("Windows 11 Pro", "Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz");
        assert_eq!(
            log_name(0, Some(&pc), Some("NVIDIA GeForce RTX 2060/PCIe/SSE2")),
            "1970-01-01_0000_windows-11-pro_intel-core-i7-9750h_nvidia-geforce-rtx-2060"
        );
        let linux = system("Linux 24.04 Ubuntu", "AMD Ryzen 7 5800X 8-Core Processor");
        assert_eq!(
            log_name(0, Some(&linux), Some("llvmpipe (LLVM 15.0.7, 256 bits)")),
            "1970-01-01_0000_linux-24-04-ubuntu_amd-ryzen-7-5800x_llvmpipe"
        );
        // Without them, the date and the system.
        assert_eq!(
            log_name(0, None, None),
            format!("1970-01-01_0000_{}", std::env::consts::OS)
        );
        assert_eq!(
            slug("  Joshua's Mac--mini.local "),
            "joshua-s-mac-mini-local"
        );
        assert!(slug(&"word ".repeat(20)).len() <= 40);
    }

    #[test]
    fn the_games_share_of_all_the_cores() {
        // 2 s of CPU time over 1 s on 8 cores: a quarter of them.
        assert_eq!(cpu_share(2.0, 1.0, 8), Some(25.0));
        assert_eq!(cpu_share(1.0, 0.0, 8), None);
        assert_eq!(cpu_share(1.0, 1.0, 0), None);
        // Read from the system where it can be.
        if cfg!(unix) {
            let clock = CpuClock::start().expect("the CPU time is read");
            let busy = (0..2_000_000u64).fold(0u64, |sum, n| sum.wrapping_add(n * n));
            std::hint::black_box(busy);
            assert!(clock.share().is_some_and(|share| share >= 0.0));
        }
    }

    #[test]
    fn a_stretch_reads_as_its_rate_and_its_slowest_frames() {
        assert_eq!(Stretch::default().figures(), None);
        // 99 frames at 60 fps and one of 100 ms.
        let mut frames = vec![1.0 / 60.0; 99];
        frames.push(0.1);
        let stretch = Stretch {
            frames,
            updates: vec![0.004; 100],
        };
        assert_eq!(stretch.summary().unwrap().update, Some(4.0));
        let figures = stretch.figures().unwrap();
        assert!(
            figures.starts_with("frames=100 seconds=1.75 fps=57.1"),
            "{figures}"
        );
        assert!(figures.contains("fps_1pct_low=10.0"), "{figures}");
        assert!(figures.contains("ms_p50=16.67"), "{figures}");
        assert!(figures.contains("ms_max=100.00"), "{figures}");
        assert!(
            figures.ends_with("update_ms_p50=4.00 update_ms_p95=4.00"),
            "{figures}"
        );
    }
}
