//! The engine on its own: its demo circuit and roster, run as a game over
//! it runs (`motorust_engine::app`). This is what the engine's builds and
//! releases hold (see `.github/workflows/build.yml`).

// On Windows, a release build opens with no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    motorust_engine::crash::install();
    // On macOS, no App Nap, timers on time and no sleep while it runs.
    #[cfg(target_os = "macos")]
    motorust_engine::latency::begin();
    motorust_engine::app().run();
}
