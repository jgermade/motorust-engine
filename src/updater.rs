//! The game updating itself from its releases, where its `game.ron` says
//! they are published ([`crate::game_info::Updates`]), on the desktop.
//!
//! As the game starts, a thread reads the latest release's version. If it
//! is newer than the game's, the version on the start menu says so, and the
//! key U updates it:
//!
//! 1. the release's archive for this platform is downloaded, with the
//!    release's SHA-256 sums, and must match its sum, or nothing is touched;
//! 2. it is unpacked into a folder beside the game ([`STAGING`]);
//! 3. the game's executable (on macOS, its whole app, if it runs from one)
//!    is renamed aside with [`OLD`] after its name, which a running
//!    executable may be on every system, Windows too, and the new one is
//!    moved into its place;
//! 4. the new one is started, and this one ends.
//!
//! What was put aside is removed the next time the game starts. Only a
//! release build updates itself; a development build is left as it is.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use sha2::{Digest, Sha256};

use crate::game_info::Updates;
use crate::locale::Language;
use crate::menu::GameState;
use crate::settings::Settings;
use crate::ui::VersionLabel;

pub struct UpdaterPlugin;

impl Plugin for UpdaterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Updater>()
            .add_systems(Startup, (remove_what_was_put_aside, check_for_updates))
            .add_systems(
                Update,
                (update_on_request, show_the_update, restart_once_installed),
            );
    }
}

/// The folder beside the game an update is unpacked in.
pub const STAGING: &str = ".update";
/// What is put after the name of what an update replaces.
pub const OLD: &str = ".old";
/// The most a release's archive may be (bytes).
const MOST: u64 = 400 * 1024 * 1024;
/// The environment variable that keeps the game from looking for updates.
const NO_UPDATES: &str = "MOTORUST_NO_UPDATES";

/// Where the updating is.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Update {
    /// Not looked for (no `updates` in `game.ron`, or not asked to).
    #[default]
    Idle,
    Checking,
    /// The game is the latest release.
    Latest,
    /// A newer release, by its version.
    Available(String),
    /// Downloading and installing it.
    Installing(String),
    /// Installed: the new executable, to start.
    Installed(PathBuf),
    /// Something went wrong, said in a few words.
    Failed(String),
}

/// The updating, shared with the threads that do it.
#[derive(Resource, Clone, Default)]
pub struct Updater(Arc<Mutex<Update>>);

impl Updater {
    pub fn now(&self) -> Update {
        self.0
            .lock()
            .map(|update| update.clone())
            .unwrap_or_default()
    }

    fn set(&self, update: Update) {
        if let Ok(mut now) = self.0.lock() {
            *now = update;
        }
    }
}

/// A version as three numbers ("0.0.18", or "v0.0.18", and what follows a
/// space or a dash left out).
pub fn parse_version(text: &str) -> Option<[u32; 3]> {
    let text = text.trim().trim_start_matches('v');
    let text = text.split([' ', '-', '+']).next()?;
    let mut parts = text.split('.').map(|part| part.parse::<u32>().ok());
    let version = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(version)
}

/// Whether `latest` is newer than `running`.
pub fn is_newer(latest: &str, running: &str) -> bool {
    match (parse_version(latest), parse_version(running)) {
        (Some(latest), Some(running)) => latest > running,
        _ => false,
    }
}

/// The platform's target, as the releases' archives are named for it.
pub fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        _ => None,
    }
}

/// How a release's archive is packed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Packing {
    /// A zip of a folder with the executable (Windows).
    Zip,
    /// A gzipped tar of a folder with the executable.
    TarGz,
    /// A zip with the app at its top (macOS, for a game run from its app).
    App,
}

impl Packing {
    /// How it is packed for the game at `executable` on the system `os`.
    pub fn of(os: &str, executable: &Path) -> Self {
        match os {
            "windows" => Packing::Zip,
            "macos" if app_of(executable).is_some() => Packing::App,
            _ => Packing::TarGz,
        }
    }

    /// What its archive's name ends with.
    pub fn ending(self) -> &'static str {
        match self {
            Packing::Zip => ".zip",
            Packing::TarGz => ".tar.gz",
            Packing::App => "-app.zip",
        }
    }
}

/// The app (`Name.app`) the executable at `executable` is in, if it is.
pub fn app_of(executable: &Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|ending| ending == "app"))
        .map(Path::to_path_buf)
}

/// `template` with its `{version}` and `{target}` filled in.
pub fn fill(template: &str, version: &str, target: &str) -> String {
    template
        .replace("{version}", version)
        .replace("{target}", target)
}

/// The sum `sums` (as `sha256sum` writes them: a hash, a space or two, an
/// asterisk for a binary file, its name) give the file `name`, in lower case.
pub fn sum_of(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        let file = file.rsplit('/').next().unwrap_or(file);
        (file == name).then(|| hash.to_ascii_lowercase())
    })
}

/// The SHA-256 of `bytes`, in lower-case hexadecimal.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The body of `url`, at most `most` bytes.
fn fetch(url: &str, most: u64) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url)
        .call()
        .map_err(|error| format!("{url}: {error}"))?;
    response
        .body_mut()
        .with_config()
        .limit(most)
        .read_to_vec()
        .map_err(|error| format!("{url}: {error}"))
}

/// Looks for a newer release than `running`.
fn latest_release(updates: &Updates, running: &str) -> Update {
    match fetch(updates.latest, 4096) {
        Ok(body) => {
            let latest = String::from_utf8_lossy(&body).trim().to_string();
            if parse_version(&latest).is_none() {
                Update::Failed(format!("{}: {latest:?}", updates.latest))
            } else if is_newer(&latest, running) {
                Update::Available(latest)
            } else {
                Update::Latest
            }
        }
        Err(error) => Update::Failed(error),
    }
}

/// Unpacks `archive`, packed as `packing`, into `into`.
pub fn unpack(archive: &[u8], packing: Packing, into: &Path) -> Result<(), String> {
    match packing {
        Packing::TarGz => {
            let gz = flate2::read::GzDecoder::new(Cursor::new(archive));
            tar::Archive::new(gz)
                .unpack(into)
                .map_err(|error| error.to_string())
        }
        Packing::Zip | Packing::App => zip::ZipArchive::new(Cursor::new(archive))
            .and_then(|mut zip| zip.extract(into))
            .map_err(|error| error.to_string()),
    }
}

/// The first file or folder under `folder` named `name`.
fn find(folder: &Path, name: &std::ffi::OsStr) -> Option<PathBuf> {
    let mut queue = vec![folder.to_path_buf()];
    while let Some(here) = queue.pop() {
        for entry in std::fs::read_dir(&here).ok()?.flatten() {
            let path = entry.path();
            if path.file_name() == Some(name) {
                return Some(path);
            }
            if path.is_dir() {
                queue.push(path);
            }
        }
    }
    None
}

/// `path` with [`OLD`] after its name.
pub fn put_aside_name(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(OLD);
    path.with_file_name(name)
}

/// Puts what an update unpacked in `staging` in the place of the game at
/// `executable`, packed as `packing`: its executable, or its app, renamed
/// aside first. Returns the new executable, to start.
pub fn replace(executable: &Path, packing: Packing, staging: &Path) -> Result<PathBuf, String> {
    let (current, new) = match packing {
        Packing::App => {
            let app = app_of(executable).ok_or("not in an app")?;
            let name = app.file_name().ok_or("an app with no name")?;
            let new = find(staging, name)
                .ok_or_else(|| format!("no {} in the archive", name.to_string_lossy()))?;
            (app, new)
        }
        _ => {
            let name = executable.file_name().ok_or("an executable with no name")?;
            let new = find(staging, name)
                .ok_or_else(|| format!("no {} in the archive", name.to_string_lossy()))?;
            (executable.to_path_buf(), new)
        }
    };
    let aside = put_aside_name(&current);
    let _ = remove(&aside);
    std::fs::rename(&current, &aside).map_err(|error| format!("{}: {error}", current.display()))?;
    if let Err(error) = std::fs::rename(&new, &current) {
        // Put back as it was.
        let _ = std::fs::rename(&aside, &current);
        return Err(format!("{}: {error}", current.display()));
    }
    #[cfg(unix)]
    if packing != Packing::App {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&current, std::fs::Permissions::from_mode(0o755));
    }
    let _ = remove(staging);
    Ok(match packing {
        Packing::App => current.join(
            executable
                .strip_prefix(app_of(executable).unwrap_or_default())
                .unwrap_or(executable),
        ),
        _ => current,
    })
}

/// Removes a file or a folder.
fn remove(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Downloads release `version`, checks it and puts it in the place of the
/// running game. Returns the new executable.
fn install(updates: &Updates, version: &str) -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let target = target().ok_or("no releases for this platform")?;
    let packing = Packing::of(std::env::consts::OS, &executable);
    let url = format!(
        "{}{}",
        fill(updates.archive, version, target),
        packing.ending()
    );
    let name = url.rsplit('/').next().unwrap_or(&url).to_string();
    let sums = fetch(&fill(updates.sums, version, target), 1024 * 1024)?;
    let expected = sum_of(&String::from_utf8_lossy(&sums), &name)
        .ok_or_else(|| format!("no sum for {name}"))?;
    let archive = fetch(&url, MOST)?;
    let got = sha256(&archive);
    if got != expected {
        return Err(format!("{name}: its sum is {got}, not {expected}"));
    }
    let beside = match packing {
        Packing::App => app_of(&executable).and_then(|app| app.parent().map(Path::to_path_buf)),
        _ => executable.parent().map(Path::to_path_buf),
    }
    .ok_or("the game is in no folder")?;
    let staging = beside.join(STAGING);
    let _ = remove(&staging);
    std::fs::create_dir_all(&staging).map_err(|error| format!("{}: {error}", staging.display()))?;
    unpack(&archive, packing, &staging)?;
    replace(&executable, packing, &staging)
}

/// Whether this game looks for updates: a release build, with `updates`
/// in its `game.ron`, not told not to.
fn looks_for_updates() -> Option<&'static Updates> {
    if cfg!(debug_assertions) || std::env::var_os(NO_UPDATES).is_some() {
        return None;
    }
    crate::game_info::info().updates.as_ref()
}

/// Removes what the last update put aside, and the folder it was unpacked
/// in, beside the game.
fn remove_what_was_put_aside() {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let mut aside = vec![put_aside_name(&executable)];
    if let Some(app) = app_of(&executable) {
        aside.push(put_aside_name(&app));
        if let Some(folder) = app.parent() {
            aside.push(folder.join(STAGING));
        }
    }
    if let Some(folder) = executable.parent() {
        aside.push(folder.join(STAGING));
    }
    for path in aside.iter().filter(|path| path.exists()) {
        let _ = remove(path);
    }
}

/// Looks for a newer release, on a thread of its own.
fn check_for_updates(updater: Res<Updater>) {
    let Some(updates) = looks_for_updates() else {
        return;
    };
    updater.set(Update::Checking);
    let updater = updater.clone();
    let running = crate::version().to_string();
    std::thread::spawn(move || updater.set(latest_release(updates, &running)));
}

/// U on the start menu, with a newer release: downloads and installs it, on
/// a thread of its own.
fn update_on_request(
    keys: Res<ButtonInput<KeyCode>>,
    state: Option<Res<State<GameState>>>,
    updater: Res<Updater>,
) {
    if !keys.just_pressed(KeyCode::KeyU)
        || state.is_none_or(|state| *state.get() != GameState::Welcome)
    {
        return;
    }
    let (Update::Available(version), Some(updates)) = (updater.now(), looks_for_updates()) else {
        return;
    };
    updater.set(Update::Installing(version.clone()));
    let updater = updater.clone();
    std::thread::spawn(move || {
        updater.set(match install(updates, &version) {
            Ok(executable) => Update::Installed(executable),
            Err(error) => Update::Failed(error),
        });
    });
}

/// What the version on the start menu says, in `language`: the game's
/// version and, where there is any, how its update is going.
pub fn version_text(language: Language, running: &str, update: &Update) -> String {
    let spanish = language == Language::Spanish;
    let what = match update {
        Update::Available(version) if spanish => {
            format!("v{version} disponible: pulsa U para actualizar")
        }
        Update::Available(version) => format!("v{version} available: press U to update"),
        Update::Installing(version) if spanish => format!("Instalando v{version}…"),
        Update::Installing(version) => format!("Installing v{version}…"),
        Update::Installed(_) if spanish => "Actualizado: reiniciando…".into(),
        Update::Installed(_) => "Updated: restarting…".into(),
        Update::Failed(_) if spanish => "No se pudo actualizar".into(),
        Update::Failed(_) => "Could not update".into(),
        _ => return format!("v{running}"),
    };
    format!("v{running} · {what}")
}

/// Shows how the updating goes on the version of the start menu.
fn show_the_update(
    updater: Res<Updater>,
    settings: Option<Res<Settings>>,
    mut labels: Query<&mut Text, With<VersionLabel>>,
    mut said: Local<Option<Update>>,
) {
    let update = updater.now();
    if said.as_ref() == Some(&update) && !updater.is_changed() {
        return;
    }
    if let Update::Failed(error) = &update {
        warn!("Update: {error}");
    }
    let language = settings.map_or_else(Language::default, |settings| settings.language);
    let text = version_text(language, crate::version(), &update);
    for mut label in &mut labels {
        if label.0 != text {
            label.0.clone_from(&text);
        }
    }
    *said = Some(update);
}

/// Once installed, starts the new game, as this one was started, and ends
/// this one.
fn restart_once_installed(updater: Res<Updater>, mut exit: MessageWriter<AppExit>) {
    let Update::Installed(executable) = updater.now() else {
        return;
    };
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match std::process::Command::new(&executable)
        .args(arguments)
        .spawn()
    {
        Ok(_) => {
            exit.write(AppExit::Success);
        }
        Err(error) => updater.set(Update::Failed(format!("{}: {error}", executable.display()))),
    }
}

/// The bytes of a reader, for the tests.
#[cfg(any(test, feature = "testing"))]
fn read_all(mut reader: impl std::io::Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    bytes
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn versions_are_read_and_compared_as_three_numbers() {
        assert_eq!(parse_version("0.0.18"), Some([0, 0, 18]));
        assert_eq!(parse_version(" v1.2.3\n"), Some([1, 2, 3]));
        assert_eq!(parse_version("0.0.17 (1a2b3c4d5)"), Some([0, 0, 17]));
        assert_eq!(parse_version("0.0.17-dirty"), Some([0, 0, 17]));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("<html>"), None);
        assert!(is_newer("0.0.18", "0.0.17"));
        assert!(is_newer("0.1.0", "0.0.99"));
        assert!(is_newer("0.0.10", "0.0.9"));
        assert!(!is_newer("0.0.17", "0.0.17"));
        assert!(!is_newer("0.0.16", "0.0.17"));
        assert!(!is_newer("nonsense", "0.0.17"));
    }

    #[test]
    fn each_platform_has_its_archive() {
        let template = "https://example.org/downloads/game-{version}-{target}";
        assert_eq!(
            fill(template, "0.0.18", "x86_64-unknown-linux-gnu"),
            "https://example.org/downloads/game-0.0.18-x86_64-unknown-linux-gnu"
        );
        let exe = Path::new("/home/someone/game/motorust");
        assert_eq!(Packing::of("linux", exe), Packing::TarGz);
        assert_eq!(
            Packing::of("windows", Path::new("C:/Games/motorust.exe")),
            Packing::Zip
        );
        let app = Path::new("/Applications/Motorust.app/Contents/MacOS/motorust");
        assert_eq!(Packing::of("macos", app), Packing::App);
        assert_eq!(
            app_of(app),
            Some(PathBuf::from("/Applications/Motorust.app"))
        );
        assert_eq!(Packing::of("macos", exe), Packing::TarGz);
        assert_eq!(
            [Packing::Zip, Packing::TarGz, Packing::App].map(Packing::ending),
            [".zip", ".tar.gz", "-app.zip"]
        );
        assert!(
            target().is_some() || !cfg!(any(windows, target_os = "linux", target_os = "macos"))
        );
    }

    #[test]
    fn an_archive_must_match_its_sum() {
        // The SHA-256 of "abc".
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let sums = "ABCDEF  motorust-0.0.18-x86_64-unknown-linux-gnu.tar.gz\n\
                    123456 *motorust-0.0.18-x86_64-pc-windows-msvc.zip\n\
                    999999  dist/motorust-0.0.18-aarch64-apple-darwin-app.zip\n";
        assert_eq!(
            sum_of(sums, "motorust-0.0.18-x86_64-unknown-linux-gnu.tar.gz").as_deref(),
            Some("abcdef")
        );
        assert_eq!(
            sum_of(sums, "motorust-0.0.18-x86_64-pc-windows-msvc.zip").as_deref(),
            Some("123456")
        );
        assert_eq!(
            sum_of(sums, "motorust-0.0.18-aarch64-apple-darwin-app.zip").as_deref(),
            Some("999999")
        );
        assert_eq!(
            sum_of(sums, "motorust-0.0.18-aarch64-apple-darwin.tar.gz"),
            None
        );
    }

    #[test]
    fn the_start_menu_says_how_the_update_goes() {
        let latest = version_text(Language::English, "0.0.17", &Update::Latest);
        assert_eq!(latest, "v0.0.17");
        let available = Update::Available("0.0.18".into());
        assert_eq!(
            version_text(Language::Spanish, "0.0.17", &available),
            "v0.0.17 · v0.0.18 disponible: pulsa U para actualizar"
        );
        assert!(
            version_text(Language::English, "0.0.17", &Update::Failed("x".into()))
                .contains("Could not")
        );
    }

    /// A folder of its own for a test, empty.
    fn scratch(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("motorust-updater-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn an_update_takes_the_place_of_the_executable_and_puts_the_old_one_aside() {
        let folder = scratch("tar");
        let executable = folder.join("motorust");
        std::fs::write(&executable, b"old").unwrap();
        // A release packed as the releases are: a folder with the executable.
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(
            &mut header,
            "motorust-0.0.18-x86_64-unknown-linux-gnu/motorust",
            &b"new"[..],
        )
        .unwrap();
        let archive = tar.into_inner().unwrap().finish().unwrap();
        let staging = folder.join(STAGING);
        std::fs::create_dir_all(&staging).unwrap();
        unpack(&archive, Packing::TarGz, &staging).unwrap();
        let new = replace(&executable, Packing::TarGz, &staging).unwrap();
        assert_eq!(new, executable);
        assert_eq!(std::fs::read(&executable).unwrap(), b"new");
        assert_eq!(std::fs::read(put_aside_name(&executable)).unwrap(), b"old");
        assert!(!staging.exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_update_takes_the_place_of_the_whole_app() {
        let folder = scratch("app");
        let app = folder.join("Game.app");
        let executable = app.join("Contents/MacOS/game");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"old").unwrap();
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
        zip.start_file("Game.app/Contents/MacOS/game", options)
            .unwrap();
        std::io::Write::write_all(&mut zip, b"new").unwrap();
        zip.start_file("README.md", options).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let staging = folder.join(STAGING);
        std::fs::create_dir_all(&staging).unwrap();
        unpack(&archive, Packing::App, &staging).unwrap();
        let new = replace(&executable, Packing::App, &staging).unwrap();
        assert_eq!(new, executable);
        assert_eq!(read_all(std::fs::File::open(&executable).unwrap()), b"new");
        assert!(put_aside_name(&app).join("Contents/MacOS/game").exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_game_with_no_updates_reads_as_ever() {
        let info: crate::game_info::GameInfo = crate::circuits::data::parse(
            "game.ron",
            r#"(title: "T", folder: "t", logo: (plain: "A", accent: "B"))"#,
        )
        .unwrap();
        assert!(info.updates.is_none());
        let info: crate::game_info::GameInfo = crate::circuits::data::parse(
            "game.ron",
            r#"(title: "T", folder: "t", logo: (plain: "A", accent: "B"), updates: Some((
                latest: "https://example.org/version.txt",
                archive: "https://example.org/downloads/t-{version}-{target}",
                sums: "https://example.org/v{version}/SHA256SUMS.txt",
            )))"#,
        )
        .unwrap();
        assert_eq!(
            info.updates.unwrap().latest,
            "https://example.org/version.txt"
        );
    }
}
