//! What the engine's tests run on: its own demo content
//! (`motorust-engine/assets/`), not the game's. The engine's demo circuit,
//! Bahía GP with no song ([`DEMO`]), and its demo roster, the bikes built in
//! code ([`DEMO_ROSTER`]). A test of the engine that needs a circuit or a
//! bike takes them from here, and an app of the engine's plugins is given
//! them before its plugins are added ([`demo_content`]), so that it reads
//! nothing of a game's.

use std::path::Path;
use std::sync::LazyLock;

use bevy::prelude::*;

use crate::bike::Roster;
use crate::circuits::{Circuit, Circuits};

/// A file of the engine's demo content, by its path in its `assets/`.
pub fn demo_file(path: &str) -> &'static str {
    let on_disk = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(path);
    let text = std::fs::read_to_string(&on_disk)
        .unwrap_or_else(|error| panic!("{}: {error}", on_disk.display()));
    String::leak(text)
}

/// The engine's demo circuits, read once, as the game reads its own.
pub static DEMO: LazyLock<Circuits> = LazyLock::new(|| {
    let names: Vec<String> =
        crate::circuits::data::parse("circuits.ron", demo_file("circuits/circuits.ron"))
            .unwrap_or_else(|error| panic!("{error}"));
    let files: Vec<(String, &'static str)> = names
        .into_iter()
        .map(|name| {
            let path = format!("circuits/{name}");
            let text = demo_file(&path);
            (path, text)
        })
        .collect();
    Circuits::parse(&files).unwrap_or_else(|error| panic!("{error}"))
});

/// The engine's demo circuit, the first of [`DEMO`].
pub fn demo_circuit() -> &'static Circuit {
    DEMO.at(0)
}

/// The engine's demo roster: the bikes built in code.
pub static DEMO_ROSTER: LazyLock<Roster> = LazyLock::new(|| {
    Roster::parse("roster.ron", demo_file("bikes/roster.ron"))
        .unwrap_or_else(|error| panic!("{error}"))
});

/// Gives `app` the engine's demo circuits and roster, before its plugins
/// read the game's.
pub fn demo_content(app: &mut App) -> &mut App {
    app.insert_resource(*DEMO).insert_resource(*DEMO_ROSTER)
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn the_engine_has_a_circuit_and_a_bike_for_each_place_of_its_own() {
        assert_eq!(demo_circuit().name, "Bahía GP");
        assert!(demo_circuit().song.is_none(), "no song");
        assert!(
            demo_circuit().scenery.themes.iter().next().is_none(),
            "no theme"
        );
        assert_eq!(DEMO_ROSTER.len(), crate::bike::RIDERS);
        assert!(
            DEMO_ROSTER
                .iter()
                .all(|entry| entry.design().index()
                    < crate::bike::model::Design::BUILT_IN_CODE.len()),
            "every bike built in code"
        );
    }
}
