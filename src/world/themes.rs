//! Themes: scenery a game adds of its own to the engine's, as plugins (the
//! engine knows no theme; the game registers its own).
//!
//! A theme is a type with what a circuit's file says of it, registered by a
//! name ([`register`]); a circuit's file writes its themes in a map by
//! those names, each read by the theme registered under it:
//!
//! ```ron
//! scenery: (
//!     ...
//!     themes: {
//!         "desert": (gas_stations: [...], ...),
//!     },
//! ),
//! ```
//!
//! A name no theme was registered under is refused, as a field the game
//! does not know is. As the world is built, each theme of the circuit
//! puts its own in it ([`Theme::spawn`]); what moves in it afterwards (a
//! pumpjack nodding, a tumbleweed rolling) the theme's own systems move,
//! added by the game's plugin.

use std::any::{Any, TypeId};
use std::fmt;
use std::sync::RwLock;

use bevy::prelude::*;
use serde::de::{DeserializeOwned, DeserializeSeed, Deserializer, Error, MapAccess, Visitor};

use super::Assets3d;
use super::props::Footprint;
use super::scenery::{Scenery, Side, Span};
use super::terrain::Terrain;
use super::textures::WorldTextures;
use crate::track::Track;

/// When a theme's things are put in the world, among the engine's own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    /// Before the houses, which keep off what it takes (and the trees).
    #[default]
    BeforeHouses,
    /// After the houses and the landmarks: only the trees keep off it.
    AfterHouses,
}

/// Scenery a game adds of its own: what a circuit's file says of it, and
/// what it puts in the world from that.
pub trait Theme: Any + Send + Sync {
    /// When it is put in the world (see [`Stage`]).
    fn stage(&self) -> Stage {
        Stage::BeforeHouses
    }

    /// Puts it in the circuit's world as it is built, beside the road, and
    /// returns the ground each thing takes, so that nothing else (houses,
    /// trees) is put there. The circuit's `scenery` is there for what of the
    /// engine's it goes with (a portal, a hall).
    fn spawn(
        &self,
        world: &mut ChildSpawnerCommands,
        track: &Track,
        terrain: &Terrain,
        scenery: &Scenery,
        assets: &mut Assets3d,
    ) -> Vec<Footprint> {
        let _ = (world, track, terrain, scenery, assets);
        Vec::new()
    }

    /// What it builds of the circuit's structures, worked out with the rest
    /// of them on the thread that prepares the circuit (of its roads, those
    /// `within` a stretch of the lap, if only those are wanted): meshes
    /// each with its material, put in the world with the circuit's first
    /// part.
    fn pieces(
        &self,
        track: &Track,
        scenery: &Scenery,
        within: Option<Span>,
        textures: &WorldTextures,
    ) -> Vec<(Mesh, StandardMaterial)> {
        let _ = (track, scenery, within, textures);
        Vec::new()
    }

    /// What it puts round it all with the ground, in the circuit's first
    /// part (a room's walls and ceiling).
    fn spawn_around(&self, world: &mut ChildSpawnerCommands, assets: &mut Assets3d) {
        let _ = (world, assets);
    }

    /// Whether the road's wall at `along` (m along a lap `lap` long), on
    /// `side`, opens into a low fence for what it puts there, so that it is
    /// seen over it (a small prop on its plinth). Worked out on the thread
    /// that prepares the circuit.
    fn opens_wall(&self, lap: f32, along: f32, side: Side) -> bool {
        let _ = (lap, along, side);
        false
    }

    /// The packs of props it places, by their folders in `assets/models/`
    /// (see [`super::prop_pack::PropPacks`]), loaded as the game starts.
    fn packs() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &[]
    }
}

/// How a registered theme is read from a file.
type Reader = fn(&mut dyn erased_serde::Deserializer<'_>) -> Result<Box<dyn Theme>, String>;

/// A theme registered: its name, its type, how it is read and the packs of
/// props it places.
type Registered = (&'static str, TypeId, Reader, &'static [&'static str]);

/// The themes registered, in the order they were.
static REGISTERED: RwLock<Vec<Registered>> = RwLock::new(Vec::new());

/// Registers the theme `T` under `name`: a circuit's file then writes it in
/// its `themes` by that name. Registering it again does nothing; another
/// theme under a name taken stops the game, saying so.
pub fn register<T: Theme + DeserializeOwned>(name: &'static str) {
    fn read<T: Theme + DeserializeOwned>(
        deserializer: &mut dyn erased_serde::Deserializer<'_>,
    ) -> Result<Box<dyn Theme>, String> {
        erased_serde::deserialize::<T>(deserializer)
            .map(|theme| Box::new(theme) as Box<dyn Theme>)
            .map_err(|error| error.to_string())
    }
    let mut registered = REGISTERED
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match registered.iter().find(|(taken, ..)| *taken == name) {
        Some((_, other, ..)) if *other == TypeId::of::<T>() => {}
        Some(_) => panic!("two themes registered as `{name}`"),
        None => registered.push((name, TypeId::of::<T>(), read::<T>, T::packs())),
    }
}

/// How the theme registered under `name` is read, if one is.
fn reader(name: &str) -> Option<Reader> {
    REGISTERED
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .find(|(registered, ..)| *registered == name)
        .map(|&(_, _, reader, _)| reader)
}

/// The packs of props the themes registered place, each once.
pub fn packs() -> Vec<&'static str> {
    let mut packs: Vec<&'static str> = Vec::new();
    for &(.., named) in REGISTERED
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
    {
        for &pack in named {
            if !packs.contains(&pack) {
                packs.push(pack);
            }
        }
    }
    packs
}

/// The names of the themes registered, for the messages.
fn names() -> Vec<&'static str> {
    REGISTERED
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .map(|&(name, ..)| name)
        .collect()
}

/// A circuit's themes, in the order its file writes them, each by its
/// name.
#[derive(Default)]
pub struct Themes(&'static [(&'static str, Box<dyn Theme>)]);

impl Themes {
    /// Each theme, with its name.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &'static dyn Theme)> {
        self.0.iter().map(|(name, theme)| (*name, &**theme))
    }

    /// The theme of type `T`, if the circuit has it.
    pub fn get<T: Theme>(&self) -> Option<&'static T> {
        self.0
            .iter()
            .find_map(|(_, theme)| (&**theme as &dyn Any).downcast_ref::<T>())
    }
}

impl<'de> serde::Deserialize<'de> for Themes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(ThemesVisitor)
    }
}

struct ThemesVisitor;

impl<'de> Visitor<'de> for ThemesVisitor {
    type Value = Themes;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a map of themes by their names")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Themes, A::Error> {
        let mut themes: Vec<(&'static str, Box<dyn Theme>)> = Vec::new();
        while let Some(name) = map.next_key::<String>()? {
            let Some(reader) = reader(&name) else {
                return Err(A::Error::custom(format!(
                    "no theme called `{name}` (the themes: {:?})",
                    names()
                )));
            };
            if themes.iter().any(|(taken, _)| *taken == name) {
                return Err(A::Error::custom(format!("the theme `{name}` twice")));
            }
            let theme = map.next_value_seed(Seed(reader))?;
            themes.push((String::leak(name), theme));
        }
        Ok(Themes(Vec::leak(themes)))
    }
}

/// Reads a theme with the reader registered for its name.
struct Seed(Reader);

impl<'de> DeserializeSeed<'de> for Seed {
    type Value = Box<dyn Theme>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        let mut erased = <dyn erased_serde::Deserializer>::erase(deserializer);
        (self.0)(&mut erased).map_err(D::Error::custom)
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[derive(serde::Deserialize, PartialEq, Debug)]
    #[serde(deny_unknown_fields)]
    struct Statues {
        count: usize,
        side: super::super::scenery::Side,
    }

    impl Theme for Statues {}

    #[derive(serde::Deserialize)]
    struct Fountains {}

    impl Theme for Fountains {}

    #[test]
    fn a_file_writes_its_themes_by_the_names_they_were_registered_under() {
        register::<Statues>("statues");
        register::<Statues>("statues");
        register::<Fountains>("fountains");
        let themes: Themes =
            ron::from_str(r#"{ "statues": (count: 3, side: Left), "fountains": () }"#).unwrap();
        assert_eq!(
            themes.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            ["statues", "fountains"]
        );
        assert_eq!(
            themes.get::<Statues>(),
            Some(&Statues {
                count: 3,
                side: super::super::scenery::Side::Left
            })
        );
        assert!(Themes::default().get::<Statues>().is_none());
        // What it gets wrong is told: an unknown theme, one twice, and a
        // field its theme does not have.
        let wrong = |text| ron::from_str::<Themes>(text).err().unwrap().to_string();
        assert!(wrong(r#"{ "nowhere": () }"#).contains("nowhere"));
        assert!(wrong(r#"{ "fountains": (), "fountains": () }"#).contains("twice"));
        assert!(wrong(r#"{ "statues": (count: 3, side: Left, height: 2.0) }"#).contains("height"));
    }

    #[test]
    #[should_panic(expected = "two themes")]
    fn two_themes_cannot_take_one_name() {
        register::<Statues>("taken");
        register::<Fountains>("taken");
    }
}
