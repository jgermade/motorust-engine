//! Las pistas del juego: el trazado y el escenario de cada una, y cuál se
//! está usando.

pub mod data;
pub mod reload;

use bevy::prelude::*;
use serde::Deserialize;

use crate::content;
use crate::locale::Language;
use crate::track::Layout;
use crate::world::scenery::Scenery;

/// Una pista para elegir.
///
/// Written in Rust, or read from a RON file (see [`data`]).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Circuit {
    #[serde(deserialize_with = "data::text")]
    pub name: &'static str,
    #[serde(deserialize_with = "data::text")]
    pub name_en: &'static str,
    #[serde(deserialize_with = "data::text")]
    pub description: &'static str,
    #[serde(deserialize_with = "data::text")]
    pub description_en: &'static str,
    /// The music that plays during the race, if it has any.
    #[serde(default, deserialize_with = "data::some_song")]
    pub song: Option<Song>,
    /// What its surroundings sound like, choosing the bike.
    pub ambience: Ambience,
    /// How fast it is ridden against most tracks (1): against the clock,
    /// the checkpoints of a slower one, narrow and twisting, give their
    /// seconds at a pace this much slower (see
    /// [`crate::race::CheckpointTimes::at`]).
    pub pace: f32,
    /// The names it had before, if it was renamed: the records saved under
    /// them are moved to its name as the circuits are read.
    #[serde(default, deserialize_with = "data::texts")]
    pub formerly: &'static [&'static str],
    pub layout: Layout,
    pub scenery: Scenery,
}

/// The sounds of a track's surroundings, heard while choosing the bike
/// (see [`crate::sound::ambience`]): how much of each, from 0 to 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ambience {
    /// Waves breaking on the shore.
    pub sea: f32,
    /// Seagulls calling now and then.
    pub gulls: f32,
    /// Small birds chirping in the trees.
    pub birds: f32,
    /// Cicadas buzzing in the heat.
    pub cicadas: f32,
    pub wind: f32,
}

/// A song: an MP3 of the content pack ([`crate::content`]), which
/// [`crate::sound`] loops, or, in a list, plays through and goes on to the
/// next.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "data::SongFile")]
pub struct Song {
    /// Its file in `assets/` (`music/bahia.mp3`).
    pub file: &'static str,
    /// How long it lasts (s), as `ffprobe` gives it.
    pub length: f32,
    /// How loud it sounds over the engines, as mastered: the integrated
    /// loudness (LUFS) of its first two minutes, as long as a race, above
    /// 500 Hz, as `ffmpeg -t 120 -i song.mp3 -af highpass=f=500,ebur128 -f
    /// null -` measures it. The game turns each song up or down by the
    /// difference, so they all sound equally loud.
    pub loudness: f32,
}

impl Circuit {
    pub fn name(&self, lang: Language) -> &'static str {
        match lang {
            Language::Spanish => self.name,
            Language::English => self.name_en,
        }
    }

    pub fn description(&self, lang: Language) -> &'static str {
        match lang {
            Language::Spanish => self.description,
            Language::English => self.description_en,
        }
    }

    /// Puntos a lo largo de la vuelta (m desde la meta) donde hay un checkpoint.
    pub fn checkpoints(&self) -> Vec<f32> {
        self.scenery
            .gantries
            .iter()
            .filter(|g| g.text == Some("CHECKPOINT"))
            .map(|g| g.along)
            .collect()
    }
}

/// The list of the circuits, in the content pack: their files in
/// `assets/circuits/`, in the order of the track select screen.
const LIST: &str = "circuits/circuits.ron";

/// The circuits' files, each its path in the content pack and its text, in
/// the order of the track select screen.
pub fn files() -> Result<Vec<(String, &'static str)>, String> {
    let names: Vec<String> = data::parse(LIST, content::try_text(LIST)?)?;
    if names.is_empty() {
        return Err(format!("{LIST}: no circuits"));
    }
    names
        .into_iter()
        .map(|name| {
            let path = format!("circuits/{name}");
            let text = content::try_text(&path)?;
            Ok((path, text))
        })
        .collect()
}

/// The terrain's functions a game registered, by their names.
static TERRAIN_FUNCTIONS: std::sync::RwLock<Vec<(&str, data::TerrainFunction)>> =
    std::sync::RwLock::new(Vec::new());

/// Registers `function` under `name`, for the circuits' files to name it
/// (a volcano's gullies: what a game adds of its own to the shapes of the
/// terrain). Registering it again does nothing.
pub fn register_terrain_function(name: &'static str, function: data::TerrainFunction) {
    let mut registered = TERRAIN_FUNCTIONS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !registered.iter().any(|(taken, _)| *taken == name) {
        registered.push((name, function));
    }
}

/// The terrain's function the circuits' files name `name` (see [`data`]):
/// `flat`, or one a game registered.
fn terrain_function(name: &str) -> Option<data::TerrainFunction> {
    if name == "flat" {
        return Some(|_| 0.0);
    }
    TERRAIN_FUNCTIONS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .find(|(registered, _)| *registered == name)
        .map(|&(_, function)| function)
}

/// Las pistas, en el orden de la pantalla de elegir pista: read from their
/// files once, when the game starts ([`Circuits::load`]), and kept for as
/// long as it runs.
#[derive(Resource, Clone, Copy)]
pub struct Circuits(&'static [Circuit]);

impl Circuits {
    /// Every circuit, read from its file in the content pack (see [`data`]
    /// and [`crate::content`]). It stops the game, saying why, if one of
    /// them is wrong.
    pub fn load() -> Self {
        files()
            .and_then(|files| Self::parse(&files))
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The circuits the `files` hold (each its path and its text, in the
    /// menu's order), or what is wrong with them.
    pub fn parse(files: &[(String, &'static str)]) -> Result<Self, String> {
        let circuits = files
            .iter()
            .map(|(path, text)| data::parse(path, text))
            .collect::<Result<Vec<Circuit>, String>>()?;
        Ok(Circuits(Vec::leak(circuits)))
    }

    /// The one chosen.
    pub fn get(&self, current: CurrentCircuit) -> &'static Circuit {
        self.at(current.0)
    }

    /// The one at `index`, for as long as the game runs.
    pub fn at(&self, index: usize) -> &'static Circuit {
        &self.0[index]
    }
}

impl Default for Circuits {
    fn default() -> Self {
        Self::load()
    }
}

impl std::ops::Deref for Circuits {
    type Target = [Circuit];

    fn deref(&self) -> &[Circuit] {
        self.0
    }
}

/// La pista elegida: su índice en [`Circuits`]. Al cambiarla, el mundo se
/// vuelve a construir ([`crate::world`]).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct CurrentCircuit(pub usize);

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    pub fn circuits_have_checkpoints_on(circuits: &'static crate::circuits::Circuits) {
        assert_eq!(circuits[0].checkpoints(), vec![780.0]);
        assert_eq!(circuits[1].checkpoints(), vec![960.0, 1980.0, 2980.0]);
        assert_eq!(circuits[2].checkpoints(), vec![700.0, 1560.0]);
        assert_eq!(
            circuits[3].checkpoints(),
            vec![1200.0, 2640.0, 3540.0, 4440.0, 5250.0]
        );
        assert_eq!(
            circuits[4].checkpoints(),
            vec![1000.0, 2200.0, 3150.0, 4230.0, 5100.0]
        );
        assert_eq!(circuits[5].checkpoints(), vec![1090.0, 2240.0, 3450.0]);
        assert_eq!(
            circuits[6].checkpoints(),
            vec![868.0, 1905.1, 2955.1, 3585.1, 4565.1]
        );
        assert_eq!(circuits[7].checkpoints(), vec![635.0, 1381.0, 2303.0]);
    }
}
