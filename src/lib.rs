//! Motorust's engine: everything that makes a motorbike racing game in
//! Bevy, knowing nothing of the content it runs. A game builds its app with
//! [`app`], lays its content pack over the engine's
//! ([`content::register_pack`]), registers its themes
//! ([`world::themes::register`]) and adds its own plugins.

// The checks of the engine's tests are written to take the content they
// check (`NAME_on`), for a game's tests to run them on its own (feature
// `testing`): in the engine's own tests, those of what its demo has none of
// are not called; with the feature, its `#[test]`s are not compiled, and
// what only they used goes unused.
#![cfg_attr(any(test, feature = "testing"), allow(dead_code, unused_imports))]

#[cfg(not(target_arch = "wasm32"))]
pub mod bench;
pub mod bezier;
pub mod bike;
pub mod camera;
pub mod circuits;
pub mod content;
pub mod curves_hud;
pub mod debug_start;
pub mod game_info;
pub mod gamepads;
pub mod geometry;
pub mod hud;
#[cfg(target_os = "macos")]
pub mod latency;
pub mod legend;
pub mod lobby;
pub mod locale;
pub mod mapping;
pub mod menu;
pub mod minimap;
pub mod modes;
pub mod options;
pub mod pad_art;
pub mod pause;
pub mod perf_log;
pub mod race;
pub mod render_scale;
pub mod settings;
pub mod shapes;
pub mod sound;
pub mod speedometer;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod track;
pub mod track_select;
pub mod ui;
pub mod vector_art;
#[cfg(target_arch = "wasm32")]
pub mod web;
pub mod welcome;
pub mod world;

use std::sync::OnceLock;

use bevy::prelude::*;

/// Bevy, as the engine uses it: a game takes it from the same version.
pub use bevy;

/// Pasos por segundo de la simulación. La física avanza siempre a este ritmo
/// y el render interpola entre pasos, así la moto se conduce igual a 60 que a
/// 240 FPS y la simulación es reproducible.
pub const SIMULATION_HZ: f64 = 120.0;

/// At most this many steps of the simulation in a frame. After a frame that
/// came late (a hitch, a circuit put in the world), Bevy catches up with as
/// many steps as the time it took, up to a quarter of a second's (30 at
/// 120 Hz): the race jumps on at once, and that frame takes longer still.
/// Past these the game's time slows down for that frame instead: it keeps
/// to real time down to 15 frames a second.
pub const MAX_STEPS_PER_FRAME: u32 = 8;

/// The simulation's clocks: its fixed step, and the game's time, which goes
/// on at most [`MAX_STEPS_PER_FRAME`] of those steps in a frame.
pub fn simulation_clocks() -> (Time<Fixed>, Time<Virtual>) {
    let fixed = Time::<Fixed>::from_hz(SIMULATION_HZ);
    let game = Time::<Virtual>::from_max_delta(fixed.timestep() * MAX_STEPS_PER_FRAME);
    (fixed, game)
}

/// What a game's build is called in the logs: its version and the commit
/// it is built from (see [`set_build`]).
static BUILD: OnceLock<&'static str> = OnceLock::new();

/// Says what the game's build is called in the logs ("0.0.10 (1a2b3c4d5)"),
/// before the app is built.
pub fn set_build(build: &'static str) {
    let _ = BUILD.set(build);
}

/// What the game's build is called in the logs: as the game said, or the
/// engine's version.
pub fn build() -> &'static str {
    BUILD
        .get()
        .copied()
        .unwrap_or(concat!(env!("CARGO_PKG_VERSION"), " (engine)"))
}

/// The game's version ("0.0.10"), what [`build`] says before the commit.
pub fn version() -> &'static str {
    build().split(" (").next().unwrap_or_default()
}

/// The engine's app: its window as the settings saved say (or the
/// benchmark's, if it was asked for), Bevy's plugins and the engine's. The
/// game's content pack and themes are registered before it is built; the
/// game adds its own plugins to it, and runs it.
pub fn app() -> App {
    // What the game is called (its folder holds the settings), read now.
    game_info::info();
    let settings = settings::Settings::load();
    // The benchmark rides the tracks on its own, with the player's settings
    // changed as it goes and never saved.
    #[cfg(not(target_arch = "wasm32"))]
    let bench = bench::requested();
    #[cfg(not(target_arch = "wasm32"))]
    let settings = if bench {
        bench::settings(settings)
    } else {
        settings
    };
    let mut app = App::new();
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(settings.window()),
            ..default()
        })
        // Los mandos los lee `gamepads`, que además los vuelve a buscar.
        .disable::<bevy::gilrs::GilrsPlugin>();
    // Pipelined rendering draws each frame while the next one is
    // simulated: more frames per second when the CPU holds them back, but
    // each one a frame later after the controls. Options can turn it off
    // (less lag), from the next start.
    #[cfg(not(target_arch = "wasm32"))]
    let plugins = if settings.pipelined_rendering {
        plugins
    } else {
        plugins.disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
    };
    // The CPU and memory in use, for the performance log (read on another
    // thread about once a second), only when it is written.
    #[cfg(not(target_arch = "wasm32"))]
    if settings.performance_log {
        app.add_plugins(bevy::diagnostic::SystemInformationDiagnosticsPlugin);
    }
    let (fixed, game) = simulation_clocks();
    // The content pack's source, before the asset server is built.
    app.add_plugins(content::ContentPlugin)
        .add_plugins(plugins)
        .insert_resource(fixed)
        .insert_resource(game)
        .insert_resource(settings::StartedWith::from(&settings))
        .insert_resource(settings)
        .add_plugins((
            world::WorldPlugin,
            bike::BikePlugin,
            camera::ChaseCameraPlugin,
            gamepads::GamepadsPlugin,
            hud::HudPlugin,
            menu::MenuPlugin,
            lobby::LobbyPlugin,
            minimap::MinimapPlugin,
            welcome::WelcomePlugin,
            track_select::TrackSelectPlugin,
            pause::PausePlugin,
            options::OptionsPlugin,
            settings::SettingsPlugin,
            sound::SoundPlugin,
            ui::UiPlugin,
        ))
        .add_plugins((
            race::RacePlugin,
            modes::ModesPlugin,
            perf_log::PerfLogPlugin,
            render_scale::RenderScalePlugin,
            curves_hud::CurvesHudPlugin,
        ));
    #[cfg(target_arch = "wasm32")]
    app.add_plugins(web::WebPlugin);
    #[cfg(not(target_arch = "wasm32"))]
    if bench {
        app.add_plugins(bench::BenchPlugin);
    }
    app
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;

    #[derive(Resource, Default)]
    struct Steps(u32);

    #[test]
    fn a_late_frame_catches_up_by_a_few_steps_at_most() {
        let (fixed, game) = simulation_clocks();
        let step = fixed.timestep();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(fixed)
            .insert_resource(game)
            .init_resource::<Steps>()
            .add_systems(FixedUpdate, |mut steps: ResMut<Steps>| steps.0 += 1);
        let frame = |app: &mut App, millis: u64| {
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                millis,
            )));
            app.world_mut().resource_mut::<Steps>().0 = 0;
            app.update();
            app.world().resource::<Steps>().0
        };
        // The clocks start.
        frame(&mut app, 0);
        // A frame of 25 ms: three steps of 1/120 s.
        assert_eq!(frame(&mut app, 25), 3);
        // Half a second late (a hitch): eight steps, not Bevy's thirty (a
        // quarter of a second's), and the game's time on by as much.
        assert_eq!(frame(&mut app, 500), MAX_STEPS_PER_FRAME);
        let moved = app.world().resource::<Time<Virtual>>().delta();
        assert_eq!(moved, step * MAX_STEPS_PER_FRAME);
        // And back to keeping time.
        assert_eq!(frame(&mut app, 25), 3);
    }
}
