//! The game's identity, read from `assets/game.ron` in the content pack:
//! its title, the folder it keeps its files in, its logo and its icon. The
//! engine names no game of its own: whatever game it runs says what it is
//! called.

use std::sync::OnceLock;

use serde::Deserialize;

/// What a game is called and where it keeps its files.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameInfo {
    /// The window's title, and the crash report's.
    #[serde(deserialize_with = "crate::circuits::data::text")]
    pub title: &'static str,
    /// The name of its folder for the settings and the logs, and of its
    /// crash report.
    #[serde(deserialize_with = "crate::circuits::data::text")]
    pub folder: &'static str,
    /// The title on the start menu.
    pub logo: Logo,
    /// The window's icon, a PNG in the content pack, if it has one.
    #[serde(default, deserialize_with = "crate::circuits::data::optional_text")]
    pub icon: Option<&'static str>,
}

/// The title on the start menu: its first part in white, the rest in the
/// accent colour.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logo {
    #[serde(deserialize_with = "crate::circuits::data::text")]
    pub plain: &'static str,
    #[serde(deserialize_with = "crate::circuits::data::text")]
    pub accent: &'static str,
}

impl Logo {
    /// The whole of it, as it reads.
    pub fn text(&self) -> String {
        format!("{}{}", self.plain, self.accent)
    }
}

/// Its file in the content pack.
const FILE: &str = "game.ron";

static INFO: OnceLock<GameInfo> = OnceLock::new();

/// The game's identity, read the first time it is asked for. It stops the
/// game, saying why, if its file is wrong.
pub fn info() -> &'static GameInfo {
    INFO.get_or_init(|| {
        crate::circuits::data::parse(FILE, crate::content::text(FILE))
            .unwrap_or_else(|error| panic!("{error}"))
    })
}

/// The game's identity if it has been read already: for the crash report,
/// which must not read files as the game falls.
pub fn read_already() -> Option<&'static GameInfo> {
    INFO.get()
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    #[test]
    fn whatever_runs_says_what_it_is_called() {
        let info = info();
        assert!(!info.title.is_empty() && !info.folder.is_empty());
        assert!(!info.logo.text().is_empty());
        if let Some(icon) = info.icon {
            assert!(crate::content::try_file(icon).is_ok(), "{icon}");
        }
        assert!(read_already().is_some());
    }

    pub fn the_game_says_what_it_is_called_on() {
        let info = info();
        assert_eq!(info.title, "Motorust");
        assert_eq!(info.folder, "motorust");
        assert_eq!(info.logo.text(), "MOTORUST");
        assert!(crate::content::try_file(info.icon.unwrap()).is_ok());
        assert!(read_already().is_some());
    }
}
