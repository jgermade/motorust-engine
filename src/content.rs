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
//! - **A desktop release build** has them built into the executable: the
//!   files each pack's `pack.txt` lists are embedded by `build.rs` and served
//!   from memory, so the game stays a single file.
//! - **A web build** has none built in, so that the `.wasm` stays small
//!   (the browser compiles it and keeps it in memory whole, and with every
//!   file in it, it was over 200 MB and froze the tab). The page downloads
//!   the packs' files apart, in one `content.pack` (written by
//!   `tools/web_pack.py`, the packs laid one over the other already, see
//!   [`unpack`]), and hands its bytes over before the game starts, in the
//!   global `motorustContent` ([`PAGE_GLOBAL`]), which the game takes
//!   ([`try_file`]) the first time it needs a file. What only the asset
//!   server reads (the songs) is left out of it and served loose beside the
//!   page, in `content/` ([`WEB_FOLDER`]), and fetched when it is needed.
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

/// The global the web page leaves the bytes of its `content.pack` in, for
/// the game to take as it starts (see the module's documentation).
pub const PAGE_GLOBAL: &str = "motorustContent";

/// The folder beside the web page the files left out of its `content.pack`
/// are served from, each at its path in its pack (`content/music/bahia.mp3`).
pub const WEB_FOLDER: &str = "content";

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
        app.register_asset_source(
            SOURCE,
            AssetSourceBuilder::new(|| Box::new(PackReader::new())),
        );
    }
}

/// Reads the asset server's files from the packs, as [`file()`] finds
/// them: from the disk in a development build, each time (a song is big,
/// and kept only while it plays), from what is built in otherwise; on the
/// web, what the page's `content.pack` leaves out from beside the page
/// ([`WEB_FOLDER`]).
struct PackReader {
    #[cfg(target_arch = "wasm32")]
    web: bevy::asset::io::wasm::HttpWasmAssetReader,
}

impl PackReader {
    fn new() -> Self {
        Self {
            #[cfg(target_arch = "wasm32")]
            web: bevy::asset::io::wasm::HttpWasmAssetReader::new(WEB_FOLDER),
        }
    }
}

impl AssetReader for PackReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let not_found = || AssetReaderError::NotFound(path.to_owned());
        let name = path.to_str().ok_or_else(not_found)?;
        let reader: Box<dyn Reader + 'a> = if FROM_DISK {
            let on_disk = on_disk_path(name).ok_or_else(not_found)?;
            let bytes =
                std::fs::read(on_disk).map_err(|error| AssetReaderError::Io(error.into()))?;
            Box::new(VecReader::new(bytes))
        } else if let Ok(bytes) = try_file(name) {
            Box::new(SliceReader::new(bytes))
        } else {
            #[cfg(target_arch = "wasm32")]
            {
                Box::new(self.web.read(path).await?)
            }
            #[cfg(not(target_arch = "wasm32"))]
            return Err(not_found());
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
        return on_disk::file(path);
    }
    let built_in = packs().iter().find_map(|pack| {
        pack.built_in
            .iter()
            .find(|(file, _)| *file == path)
            .map(|&(_, bytes)| bytes)
    });
    #[cfg(target_arch = "wasm32")]
    let built_in = built_in.or_else(|| from_page::file(path));
    built_in.ok_or_else(|| format!("{path}: in no content pack (pack.txt)"))
}

/// The files of a `content.pack`, as `tools/web_pack.py` writes it for the
/// web page (and that tool's header says): `MRPK`, its version (1) and how
/// many files it has, each a little-endian `u32`, then each file: the
/// length of its path, its path (UTF-8), the length of its bytes (each a
/// `u32`) and its bytes. A path is the file's in its pack, once, from the
/// first pack that lists it.
pub fn unpack(pack: &[u8]) -> Result<Vec<(&str, &[u8])>, String> {
    let mut rest = pack;
    let mut take = |length: usize| -> Result<&[u8], String> {
        if rest.len() < length {
            return Err("content.pack: cut short".to_string());
        }
        let (taken, after) = rest.split_at(length);
        rest = after;
        Ok(taken)
    };
    let number = |bytes: &[u8]| u32::from_le_bytes(bytes.try_into().expect("4 bytes")) as usize;
    if take(4)? != b"MRPK" {
        return Err("content.pack: not a content pack".to_string());
    }
    let version = number(take(4)?);
    if version != 1 {
        return Err(format!("content.pack: version {version}, not 1"));
    }
    let count = number(take(4)?);
    let mut files = Vec::with_capacity(count);
    for _ in 0..count {
        let length = number(take(4)?);
        let path = std::str::from_utf8(take(length)?)
            .map_err(|error| format!("content.pack: a path: {error}"))?;
        let length = number(take(4)?);
        files.push((path, take(length)?));
    }
    if !rest.is_empty() {
        return Err("content.pack: bytes after its last file".to_string());
    }
    Ok(files)
}

/// The packs' files the web page handed over (see the module's
/// documentation), taken from it the first time one is asked for, and kept
/// for as long as the game runs.
#[cfg(target_arch = "wasm32")]
mod from_page {
    use std::collections::HashMap;
    use std::sync::LazyLock;

    use bevy::log::error;

    static FILES: LazyLock<HashMap<&'static str, &'static [u8]>> = LazyLock::new(|| {
        let global = js_sys::global();
        let key = js_sys::JsString::from(super::PAGE_GLOBAL);
        let Ok(value) = js_sys::Reflect::get(&global, &key) else {
            return HashMap::new();
        };
        if value.is_undefined() || value.is_null() {
            error!(
                "The page has handed over no content pack (`{}`)",
                super::PAGE_GLOBAL
            );
            return HashMap::new();
        }
        // Copied into the game's memory, and let go of in the page's.
        let bytes: &'static [u8] = Vec::leak(js_sys::Uint8Array::new(&value).to_vec());
        let _ = js_sys::Reflect::delete_property(&global, &key);
        match super::unpack(bytes) {
            Ok(files) => files.into_iter().collect(),
            Err(error) => {
                error!("{error}");
                HashMap::new()
            }
        }
    });

    pub fn file(path: &str) -> Option<&'static [u8]> {
        FILES.get(path).copied()
    }
}

/// A text file of the packs, as [`try_file`].
pub fn try_text(path: &str) -> Result<&'static str, String> {
    std::str::from_utf8(try_file(path)?).map_err(|error| format!("{path}: {error}"))
}

/// The files each pack's `pack.txt` lists, by the pack's folder, as read.
static LISTED: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<&'static str, &'static [&'static str]>>,
> = std::sync::LazyLock::new(Default::default);

/// `path` listed in the `pack.txt` of the pack that lists `beside`, on the
/// line after it (by the map editor, for a file it makes: a circuit's
/// ground's shaping, see [`crate::editor`]): from now on read from that
/// pack, and built into a release. Nothing if a pack lists it already.
#[cfg_attr(
    not(all(debug_assertions, not(target_arch = "wasm32"))),
    expect(dead_code, reason = "a development build's")
)]
pub fn list_beside(beside: &str, path: &str) -> Result<(), String> {
    let packs = packs();
    if packs.iter().any(|pack| listed(pack).contains(&path)) {
        return Ok(());
    }
    let pack = packs
        .iter()
        .find(|pack| listed(pack).contains(&beside))
        .ok_or_else(|| format!("{beside}: in no content pack (pack.txt)"))?;
    let list = Path::new(pack.folder).join("pack.txt");
    let text =
        std::fs::read_to_string(&list).map_err(|error| format!("{}: {error}", list.display()))?;
    let mut lines: Vec<&str> = text.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.trim() == beside)
        .map_or(lines.len(), |at| at + 1);
    lines.insert(at, path);
    let mut written = lines.join("\n");
    written.push('\n');
    std::fs::write(&list, written).map_err(|error| format!("{}: {error}", list.display()))?;
    LISTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(pack.folder);
    Ok(())
}

/// The files `pack`'s `pack.txt` lists, each a path in the pack: what a
/// release build has built in. Read from the disk once.
pub fn listed(pack: &Pack) -> &'static [&'static str] {
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
pub use on_disk::{modified, reread, write, write_bytes};

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

    /// A file of the packs written on the disk, where the first pack that
    /// lists it has it (by the map editor, see [`crate::editor`]): the
    /// circuits' watch reads it again (`crate::circuits::reload`).
    #[cfg_attr(
        not(all(debug_assertions, not(target_arch = "wasm32"))),
        expect(dead_code, reason = "a development build's")
    )]
    pub fn write(path: &str, text: &str) -> Result<(), String> {
        write_bytes(path, text.as_bytes())
    }

    /// The same, of any bytes (a circuit's ground's shaping).
    #[cfg_attr(
        not(all(debug_assertions, not(target_arch = "wasm32"))),
        expect(dead_code, reason = "a development build's")
    )]
    pub fn write_bytes(path: &str, bytes: &[u8]) -> Result<(), String> {
        let on_disk = super::on_disk_path(path)
            .ok_or_else(|| format!("{path}: in no content pack (pack.txt)"))?;
        std::fs::write(&on_disk, bytes).map_err(|error| format!("{}: {error}", on_disk.display()))
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

        let packs = PackReader::new();
        let mut reader = block_on(packs.read(Path::new("circuits/bahia.ron"))).unwrap();
        let mut bytes = Vec::new();
        block_on(reader.read_to_end(&mut bytes)).unwrap();
        assert_eq!(bytes, read("circuits/bahia.ron"));
        assert!(block_on(packs.read(Path::new("nowhere.ron"))).is_err());
    }

    /// A `content.pack` as `tools/web_pack.py` writes it.
    fn packed(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut pack = b"MRPK".to_vec();
        pack.extend(1u32.to_le_bytes());
        pack.extend((files.len() as u32).to_le_bytes());
        for (path, bytes) in files {
            pack.extend((path.len() as u32).to_le_bytes());
            pack.extend(path.as_bytes());
            pack.extend((bytes.len() as u32).to_le_bytes());
            pack.extend(*bytes);
        }
        pack
    }

    #[test]
    fn a_content_pack_gives_back_its_files() {
        let files: &[(&str, &[u8])] = &[
            ("game.ron", b"(name: \"Motorust\")"),
            ("textures/grass.jpg", &[0xff, 0xd8, 0, 1, 2]),
            ("empty.txt", b""),
        ];
        let pack = packed(files);
        assert_eq!(unpack(&pack).unwrap(), files);
        assert_eq!(unpack(&packed(&[])).unwrap(), []);
    }

    #[test]
    fn a_broken_content_pack_is_refused() {
        let pack = packed(&[("game.ron", b"()")]);
        assert!(unpack(&pack[..pack.len() - 1]).is_err(), "cut short");
        assert!(
            unpack(&[pack.as_slice(), b"!"].concat()).is_err(),
            "too long"
        );
        assert!(unpack(b"PK\x03\x04").is_err(), "not a pack");
        let mut later = pack.clone();
        later[4] = 2;
        assert!(unpack(&later).is_err(), "another version");
    }

    /// `tools/web_pack.py` writes the packs as [`unpack`] reads them,
    /// where there is a Python to run it.
    #[test]
    fn the_web_pack_tool_writes_what_is_read() {
        let Some(python) = python() else {
            return;
        };
        let out = std::env::temp_dir().join(format!("motorust-web-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let status = std::process::Command::new(python)
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/web_pack.py"))
            .arg(&out)
            .arg(ENGINE.folder)
            .status()
            .unwrap();
        assert!(status.success());
        let pack = std::fs::read(out.join("content.pack")).unwrap();
        let files = unpack(&pack).unwrap();
        let listed = listed(&ENGINE);
        assert_eq!(files.len(), listed.len());
        for (path, bytes) in files {
            assert!(listed.contains(&path), "{path}");
            assert_eq!(bytes, read(path), "{path}");
        }
        std::fs::remove_dir_all(&out).unwrap();
    }

    /// The Python there is to run the tools with, if there is one.
    fn python() -> Option<&'static str> {
        ["python3", "python"].into_iter().find(|python| {
            std::process::Command::new(python)
                .arg("--version")
                .output()
                .is_ok_and(|output| output.status.success())
        })
    }
}
