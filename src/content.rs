//! The content packs: the files the engine and the game run on (circuits,
//! bikes, models, textures, music, fonts and art), each asked for by its
//! path in its pack, `textures/grass.jpg`, and none built into the code.
//!
//! There are two packs, laid one over the other: the engine's own
//! ([`ENGINE`], the engine crate's `assets/`: its demo circuit and bikes, the
//! textures, the font, the gamepads' art) and the game's, which it
//! registers as it starts ([`register_pack`], `assets/`: its circuits,
//! bikes, models, music and art). A file is read from the first pack that
//! lists it, the game's before the engine's: the game's `circuits/bahia.ron`
//! (with its song) in place of the engine's (with none), its `game.ron` in
//! place of the engine's.
//!
//! Where a pack's files are read from depends on the build:
//!
//! - **A desktop development build** (`cargo run`, `cargo test`) reads them
//!   from the disk, from the pack's folder, so a file can be changed with
//!   no compiling (and none is built in, which keeps such builds quicker to
//!   link). It still reads none its pack's `pack.txt` leaves out, so it
//!   fails where a release would.
//! - **A release build, and every web build**, has them built into the
//!   executable: the files each pack's `pack.txt` lists are embedded by
//!   `build.rs` and served from memory, so the game stays a single file.
//!
//! Two ways to have one:
//!
//! - [`file()`] and [`text`], at once, for what is needed as the game starts
//!   (the circuits and the roster, the textures, the models, the fonts);
//! - through Bevy's asset server, from the asset source [`SOURCE`]
//!   (`content://music/bahia.mp3`, see [`path`]), loaded in the background,
//!   for what can come a little later (the songs, as they start playing).

use std::path::Path;
use std::sync::RwLock;

use bevy::asset::io::{
    AssetReader, AssetReaderError, AssetSourceBuilder, PathStream, Reader, SliceReader, VecReader,
};
use bevy::prelude::*;

/// A pack of content: its folder, whose `pack.txt` lists its files, and
/// those files built in (in a release build and on the web; none in a
/// development build, which reads them from the folder).
#[derive(Clone, Copy, Debug)]
pub struct Pack {
    pub folder: &'static str,
    pub built_in: &'static [(&'static str, &'static [u8])],
}

/// Whether this build reads the packs from their folders.
const FROM_DISK: bool = cfg!(all(debug_assertions, not(target_arch = "wasm32")));

/// The engine's own pack, under all the others.
pub const ENGINE: Pack = Pack {
    folder: concat!(env!("CARGO_MANIFEST_DIR"), "/assets"),
    built_in: engine_built_in::FILES,
};

/// The engine's pack's files, built in, as `build.rs` lists them from its
/// `pack.txt`.
mod engine_built_in {
    #[cfg(not(all(debug_assertions, not(target_arch = "wasm32"))))]
    include!(concat!(env!("OUT_DIR"), "/engine_pack.rs"));
    #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
    pub const FILES: &[(&str, &[u8])] = &[];
}

/// The packs registered over the engine's, the last registered first.
static REGISTERED: RwLock<Vec<Pack>> = RwLock::new(Vec::new());

/// Lays `pack` over those there are: a game's, before any file is read.
/// Registering it again does nothing.
pub fn register_pack(pack: Pack) {
    let mut registered = REGISTERED
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !registered.iter().any(|there| there.folder == pack.folder) {
        registered.insert(0, pack);
    }
}

/// The packs, in the order a file is looked for in them: those registered,
/// the last first, then the engine's. (The engine's own tests have the
/// engine's alone; a game's register its pack first.)
pub fn packs() -> Vec<Pack> {
    let mut packs = REGISTERED
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    packs.push(ENGINE);
    packs
}

/// The asset source the packs are read from.
pub const SOURCE: &str = "content";

/// The path the asset server knows a file of the packs by: `path` is where
/// it is in its pack (`music/bahia.mp3`).
pub fn path(file: &str) -> String {
    format!("{SOURCE}://{file}")
}

/// Registers the packs' asset source. It must be added before Bevy's
/// `AssetPlugin` (`DefaultPlugins`), which builds the sources it finds.
pub struct ContentPlugin;

impl Plugin for ContentPlugin {
    fn build(&self, app: &mut App) {
        app.register_asset_source(SOURCE, AssetSourceBuilder::new(|| Box::new(PackReader)));
    }
}

/// Reads the asset server's files from the packs, as [`file()`] finds
/// them: from the disk in a development build, each time (a song is big,
/// and kept only while it plays), from what is built in otherwise.
struct PackReader;

impl AssetReader for PackReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let not_found = || AssetReaderError::NotFound(path.to_owned());
        let name = path.to_str().ok_or_else(not_found)?;
        let reader: Box<dyn Reader + 'a> = if FROM_DISK {
            let on_disk = on_disk_path(name).ok_or_else(not_found)?;
            let bytes =
                std::fs::read(on_disk).map_err(|error| AssetReaderError::Io(error.into()))?;
            Box::new(VecReader::new(bytes))
        } else {
            Box::new(SliceReader::new(try_file(name).map_err(|_| not_found())?))
        };
        Ok(reader)
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Err::<VecReader, _>(AssetReaderError::NotFound(path.to_owned()))
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(path.to_owned()))
    }

    async fn is_directory<'a>(&'a self, _path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
}

/// A file of the packs, by its path in its pack (`textures/grass.jpg`),
/// for as long as the game runs. It stops the game, saying why, if no pack
/// has such a file.
pub fn file(path: &str) -> &'static [u8] {
    try_file(path).unwrap_or_else(|error| panic!("{error}"))
}

/// A text file of the packs (a circuit, the roster, an SVG), as [`file()`].
pub fn text(path: &str) -> &'static str {
    try_text(path).unwrap_or_else(|error| panic!("{error}"))
}

/// A file of the packs, as [`file()`], or what is wrong: for what is read
/// again while the game runs (a circuit changed on the disk), and for what
/// a pack may not have (the modelled trees, the menu's helmet), where the
/// game goes on without it.
pub fn try_file(path: &str) -> Result<&'static [u8], String> {
    if FROM_DISK {
        on_disk::file(path)
    } else {
        packs()
            .iter()
            .find_map(|pack| {
                pack.built_in
                    .iter()
                    .find(|(file, _)| *file == path)
                    .map(|&(_, bytes)| bytes)
            })
            .ok_or_else(|| format!("{path}: in no content pack (pack.txt)"))
    }
}

/// A text file of the packs, as [`try_file`].
pub fn try_text(path: &str) -> Result<&'static str, String> {
    std::str::from_utf8(try_file(path)?).map_err(|error| format!("{path}: {error}"))
}

/// The files `pack`'s `pack.txt` lists, each a path in the pack: what a
/// release build has built in. Read from the disk once.
pub fn listed(pack: &Pack) -> &'static [&'static str] {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    static LISTED: LazyLock<Mutex<HashMap<&str, &[&str]>>> = LazyLock::new(Default::default);
    let mut listed = LISTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    listed.entry(pack.folder).or_insert_with(|| {
        let path = Path::new(pack.folder).join("pack.txt");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        Vec::leak(
            String::leak(text)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .collect(),
        )
    })
}

/// Whether some pack lists `path`.
#[cfg(any(test, feature = "testing"))]
pub fn is_listed(path: &str) -> bool {
    packs().iter().any(|pack| listed(pack).contains(&path))
}

/// Where the file `path` is on the disk: in the first pack that lists it.
fn on_disk_path(path: &str) -> Option<std::path::PathBuf> {
    packs()
        .iter()
        .find(|pack| listed(pack).contains(&path))
        .map(|pack| Path::new(pack.folder).join(path))
}

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
pub use on_disk::{modified, reread};

/// The packs' files read from the disk, in a desktop development build:
/// each once, kept for as long as the game runs, unless it is read again
/// ([`reread`], a file changed on the disk).
mod on_disk {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex, MutexGuard};

    static READ: LazyLock<Mutex<HashMap<String, &'static [u8]>>> = LazyLock::new(Default::default);

    fn read_so_far() -> MutexGuard<'static, HashMap<String, &'static [u8]>> {
        READ.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A file a pack lists, from the disk.
    fn read(path: &str) -> Result<&'static [u8], String> {
        let on_disk = super::on_disk_path(path).ok_or_else(|| {
            format!("{path}: in no content pack (pack.txt), so a release build would not have it")
        })?;
        let bytes =
            std::fs::read(&on_disk).map_err(|error| format!("{}: {error}", on_disk.display()))?;
        Ok(Vec::leak(bytes))
    }

    pub fn file(path: &str) -> Result<&'static [u8], String> {
        if let Some(&bytes) = read_so_far().get(path) {
            return Ok(bytes);
        }
        let bytes = read(path)?;
        Ok(*read_so_far().entry(path.to_string()).or_insert(bytes))
    }

    /// A file of the packs read from the disk again, in place of what was
    /// read before (which stays, for whatever still holds it), from now on.
    #[cfg_attr(
        not(all(debug_assertions, not(target_arch = "wasm32"))),
        expect(dead_code, reason = "a development build's")
    )]
    pub fn reread(path: &str) -> Result<&'static [u8], String> {
        let bytes = read(path)?;
        read_so_far().insert(path.to_string(), bytes);
        Ok(bytes)
    }

    /// When a file of the packs was last changed on the disk, if it is
    /// there.
    #[cfg_attr(
        not(all(debug_assertions, not(target_arch = "wasm32"))),
        expect(dead_code, reason = "a development build's")
    )]
    pub fn modified(path: &str) -> Option<std::time::SystemTime> {
        std::fs::metadata(super::on_disk_path(path)?)
            .and_then(|metadata| metadata.modified())
            .ok()
    }
}

/// A file of the packs, read from the disk as the tests read it.
#[cfg(any(test, feature = "testing"))]
pub fn read(file: &str) -> Vec<u8> {
    let path = on_disk_path(file).unwrap_or_else(|| panic!("{file}: in no content pack"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn each_pack_lists_files_that_are_there_once_each() {
        for pack in packs() {
            let files = listed(&pack);
            assert!(!files.is_empty(), "{}", pack.folder);
            for (index, file) in files.iter().enumerate() {
                assert!(
                    Path::new(pack.folder).join(file).is_file(),
                    "{file} is not in {}",
                    pack.folder
                );
                assert!(!files[..index].contains(file), "{file} twice");
            }
        }
    }

    #[test]
    fn the_engine_s_pack_has_what_the_engine_needs_of_its_own() {
        for file in [
            "circuits/circuits.ron",
            "circuits/bahia.ron",
            "bikes/roster.ron",
            "game.ron",
            "textures/grass.jpg",
            "fonts/RussoOne-Regular.ttf",
            "ui/pads/xbox.svg",
        ] {
            assert!(listed(&ENGINE).contains(&file), "{file}");
        }
    }

    #[test]
    fn a_file_is_read_once_and_kept() {
        let font = file("fonts/RussoOne-Regular.ttf");
        assert_eq!(font, read("fonts/RussoOne-Regular.ttf"));
        assert!(
            std::ptr::eq(font, file("fonts/RussoOne-Regular.ttf")),
            "read once"
        );
        assert!(text("circuits/circuits.ron").contains("\"bahia.ron\""));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_file_read_again_takes_the_place_of_the_one_before() {
        let path = "game.ron";
        let before = file(path);
        let again = reread(path).unwrap();
        assert_eq!(again, before, "the same bytes, as nothing changed");
        assert!(std::ptr::eq(file(path), again), "from now on");
        assert!(modified(path).is_some());
        assert!(reread("nowhere.ron").is_err(), "in no pack");
        assert!(try_file("nowhere.ron").is_err());
    }

    #[test]
    #[should_panic(expected = "in no content pack")]
    fn a_file_no_pack_lists_is_refused_as_a_release_would() {
        // There, but not listed: the engine's pack's own list.
        file("pack.txt");
    }

    #[test]
    fn a_file_of_the_packs_is_known_by_its_source() {
        assert_eq!(path("music/bahia.mp3"), "content://music/bahia.mp3");
    }

    #[test]
    fn the_asset_server_reads_a_file_from_its_pack() {
        use bevy::tasks::block_on;

        let mut reader = block_on(PackReader.read(Path::new("circuits/bahia.ron"))).unwrap();
        let mut bytes = Vec::new();
        block_on(reader.read_to_end(&mut bytes)).unwrap();
        assert_eq!(bytes, read("circuits/bahia.ron"));
        assert!(block_on(PackReader.read(Path::new("nowhere.ron"))).is_err());
    }
}
