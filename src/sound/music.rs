//! Music: each track has its own song ([`crate::circuits::Song`]), if any,
//! which loops for the whole race. Every song is turned up or down so that they
//! all sound as loud over the engines: to the same loudness, [`LOUDNESS`],
//! above 500 Hz, where the engines' drone does not cover it. Below that,
//! songs with heavy bass carry more, and a song measured over all of it
//! would be heard less. It plays well below the engines, so the bike (revs,
//! gear changes, tyres, impacts) is always heard over it: its loudest second
//! at least 10 dB under the player's engine flat out.
//!
//! The menus (the welcome screen, the lobby, the modes and the tracks) have
//! a song of their own, [`Songs::menu`], low; choosing the bike has none:
//! the sounds of the track's surroundings ([`super::ambience`]) instead.
//! The songs that are not a track's are named in `assets/music/music.ron`
//! ([`Songs`]); where there is none, nothing plays.
//!
//! In free ride, Y (on a gamepad, the top face button) changes what plays
//! ([`FreeMusic`]): the track's song, the songs of every track one after
//! another from this one's, or the chill songs ([`Songs::chill`]), a little
//! quieter ([`CHILL_LOUDNESS`]); only what has songs to play.
//!
//! It fades in when the race starts and out when leaving it, drops in the
//! pause menu, and M mutes it with everything else.
//!
//! Options > Sound has a music player ([`Jukebox`]): every song the game
//! has ([`player_tunes`]), each tracks' with its track, the menus' and the
//! chill ones, and those no screen plays ([`Songs::loose`]), played one at a
//! time while its page is open, in place of the screen's.

use std::io::Cursor;
use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
#[cfg(not(target_arch = "wasm32"))]
use bevy::audio::AudioSinkPlayback;
use bevy::audio::{Decodable, Volume};

/// What a song's volume is set through: Bevy's sink, or on the web the
/// sound's own (see `super::output`).
#[cfg(not(target_arch = "wasm32"))]
type Sink = AudioSink;
#[cfg(target_arch = "wasm32")]
type Sink = super::output::web::WebSink;
use bevy::prelude::*;

use super::{MASTER, SoundControls, music_gain};
use crate::circuits::{Circuits, CurrentCircuit, Song};
use crate::content;
use crate::locale::Language;
use crate::menu::GameState;
use crate::pause::Pause;
use crate::race::{Mode, Rules};
use crate::settings::{DEFAULT_VOLUME, Settings};

/// Loudness every song plays at above 500 Hz (LUFS), with the sound on (see
/// [`Song::loudness`]). The songs come mastered at -13 to -17 LUFS there:
/// they are turned down by 9 to 12 dB.
const LOUDNESS: f32 = -25.4;
/// The chill songs, a little quieter.
const CHILL_LOUDNESS: f32 = LOUDNESS - 4.0;
/// The menus' song, low: in the background.
const MENU_LOUDNESS: f32 = LOUDNESS - 8.0;
/// Fader in the pause menu: about 6 dB lower.
const PAUSED: f32 = 0.5;
/// Time to fade in from silence to full volume, and out (s).
const FADE_IN: f32 = 2.0;
const FADE_OUT: f32 = 1.0;

/// The key and the gamepad button that change free ride's music.
pub const MUSIC_KEY: KeyCode = KeyCode::KeyY;
pub const MUSIC_BUTTON: GamepadButton = GamepadButton::North;

/// A song with a name of its own (the menus', a chill one).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedSong {
    #[serde(deserialize_with = "crate::circuits::data::text")]
    pub name: &'static str,
    pub song: Song,
}

/// The music that is not a track's own, read from `assets/music/music.ron`
/// in the content pack as the game starts: the menus' song, if any, the
/// chill songs, in the order they play, and the loose ones, which only the
/// music player plays.
#[derive(Resource, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "'de: 'static"))]
pub struct Songs {
    pub menu: Option<NamedSong>,
    pub chill: Vec<NamedSong>,
    #[serde(default)]
    pub loose: Vec<NamedSong>,
}

/// The file it is read from, in the content pack.
const SONGS_FILE: &str = "music/music.ron";

impl Default for Songs {
    /// The game's, or none if it has no `music/music.ron` (the engine's
    /// demo has none).
    fn default() -> Self {
        match content::try_text(SONGS_FILE) {
            Ok(text) => crate::circuits::data::parse(SONGS_FILE, text)
                .unwrap_or_else(|error| panic!("{error}")),
            Err(_) => Songs {
                menu: None,
                chill: Vec::new(),
                loose: Vec::new(),
            },
        }
    }
}

/// A song that can play: a track's (an index of [`Circuits`]), a chill one
/// (of [`Songs::chill`]), the menus', or a loose one (of [`Songs::loose`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tune {
    Track(usize),
    Chill(usize),
    Menu,
    Loose(usize),
}

impl Tune {
    /// Its song, if there is one (a track may have none, and the menus).
    fn song(self, circuits: &Circuits, songs: &Songs) -> Option<Song> {
        match self {
            Tune::Track(index) => circuits.at(index).song,
            Tune::Chill(index) => songs.chill.get(index).map(|named| named.song),
            Tune::Menu => songs.menu.as_ref().map(|named| named.song),
            Tune::Loose(index) => songs.loose.get(index).map(|named| named.song),
        }
    }

    /// The loudness it plays at above 500 Hz (LUFS).
    fn loudness(self) -> f32 {
        match self {
            Tune::Track(_) | Tune::Loose(_) => LOUDNESS,
            Tune::Chill(_) => CHILL_LOUDNESS,
            Tune::Menu => MENU_LOUDNESS,
        }
    }

    /// Its name: the track's, or the song's.
    pub fn name(self, circuits: &Circuits, songs: &Songs, lang: Language) -> &'static str {
        match self {
            Tune::Track(index) => circuits.at(index).name(lang),
            Tune::Chill(index) => songs.chill.get(index).map_or("", |named| named.name),
            Tune::Menu => songs.menu.as_ref().map_or("", |named| named.name),
            Tune::Loose(index) => songs.loose.get(index).map_or("", |named| named.name),
        }
    }

    /// The song's own name, as the music player shows it: a track's song's
    /// title, if its file gives one, or else its file's name in words
    /// (`victory_in_the_rearview.mp3`, "Victory in the Rearview"); the
    /// others', their names.
    pub fn title(self, circuits: &Circuits, songs: &Songs) -> String {
        match self {
            Tune::Track(index) => {
                circuits
                    .at(index)
                    .song
                    .map_or_else(String::new, |song| match song.title {
                        Some(title) => title.to_string(),
                        None => title_of(song.file),
                    })
            }
            _ => self.name(circuits, songs, Language::English).to_string(),
        }
    }

    /// What it is played for, as the music player shows it: a track's song,
    /// its track; the others, the menus, chill, or none (loose).
    pub fn played_for(self, circuits: &Circuits, lang: Language) -> &'static str {
        match (self, lang) {
            (Tune::Track(index), _) => circuits.at(index).name(lang),
            (Tune::Menu, Language::Spanish) => "Menús",
            (Tune::Menu, Language::English) => "Menus",
            (Tune::Chill(_), _) => "Chill",
            (Tune::Loose(_), Language::Spanish) => "Suelta",
            (Tune::Loose(_), Language::English) => "Loose",
        }
    }
}

/// A song's file's name in words, as a title: its words capitalised but the
/// small ones inside it (`music/crown_of_the_track.mp3`, "Crown of the
/// Track").
fn title_of(file: &str) -> String {
    const SMALL: [&str; 9] = ["a", "an", "and", "at", "in", "of", "on", "the", "to"];
    let stem = file.rsplit('/').next().unwrap_or(file);
    let stem = stem.strip_suffix(".mp3").unwrap_or(stem);
    stem.split('_')
        .filter(|word| !word.is_empty())
        .enumerate()
        .map(|(at, word)| {
            if at > 0 && SMALL.contains(&word) {
                word.to_string()
            } else {
                let mut letters = word.chars();
                letters.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(letters).collect()
                })
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every song the music player lists, in its order: each track's that has
/// one (a song two tracks share, once, under the first), the menus', the
/// chill ones and the loose ones.
pub fn player_tunes(circuits: &Circuits, songs: &Songs) -> Vec<Tune> {
    let mut files = Vec::new();
    let tracks = (0..circuits.len()).filter(|&index| {
        circuits.at(index).song.is_some_and(|song| {
            let new = !files.contains(&song.file);
            files.push(song.file);
            new
        })
    });
    let tracks: Vec<Tune> = tracks.map(Tune::Track).collect();
    tracks
        .into_iter()
        .chain(songs.menu.as_ref().map(|_| Tune::Menu))
        .chain((0..songs.chill.len()).map(Tune::Chill))
        .chain((0..songs.loose.len()).map(Tune::Loose))
        .collect()
}

/// The music player of Options > Sound: the song it plays, if any, in
/// place of the screen's, whatever the screen, while its page is open.
#[derive(Resource, Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Jukebox {
    pub playing: Option<Tune>,
}

/// What free ride plays.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum MusicMode {
    /// The track's song, over and over.
    #[default]
    Track,
    /// The songs of every track, one after another, from this one's.
    AllTracks,
    /// The chill songs, one after another.
    Chill,
}

const MUSIC_MODES: [MusicMode; 3] = [MusicMode::Track, MusicMode::AllTracks, MusicMode::Chill];

impl MusicMode {
    /// The songs it plays on the track `circuit`, in order: the last one is
    /// followed by the first. Only songs there are: none, if there are none
    /// (a track with no song, no chill songs).
    pub fn tunes(self, circuit: usize, circuits: &Circuits, songs: &Songs) -> Vec<Tune> {
        let tracks = circuits.len();
        let tunes: Vec<Tune> = match self {
            MusicMode::Track => vec![Tune::Track(circuit)],
            MusicMode::AllTracks => (0..tracks)
                .map(|step| Tune::Track((circuit + step) % tracks))
                .collect(),
            MusicMode::Chill => (0..songs.chill.len()).map(Tune::Chill).collect(),
        };
        tunes
            .into_iter()
            .filter(|tune| tune.song(circuits, songs).is_some())
            .collect()
    }

    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (MusicMode::Track, Language::Spanish) => "la del circuito",
            (MusicMode::Track, Language::English) => "the track's",
            (MusicMode::AllTracks, Language::Spanish) => "todos los circuitos",
            (MusicMode::AllTracks, Language::English) => "every track's",
            (MusicMode::Chill, _) => "chill",
        }
    }
}

/// Free ride's music: what it plays and which of its songs is playing (an
/// index of [`MusicMode::tunes`]). Every free ride starts with the track's
/// song.
#[derive(Resource, Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct FreeMusic {
    pub mode: MusicMode,
    pub at: usize,
}

impl FreeMusic {
    /// The song playing on the track `circuit`, if it has any to play.
    pub fn tune(&self, circuit: usize, circuits: &Circuits, songs: &Songs) -> Option<Tune> {
        let tunes = self.mode.tunes(circuit, circuits, songs);
        (!tunes.is_empty()).then(|| tunes[self.at % tunes.len()])
    }

    /// The next of what it plays, from its first song: one that has songs
    /// to play on the track `circuit`, if any does.
    fn next_mode(&mut self, circuit: usize, circuits: &Circuits, songs: &Songs) {
        let index = MUSIC_MODES
            .iter()
            .position(|&m| m == self.mode)
            .unwrap_or(0);
        self.mode = (1..=MUSIC_MODES.len())
            .map(|step| MUSIC_MODES[(index + step) % MUSIC_MODES.len()])
            .find(|mode| !mode.tunes(circuit, circuits, songs).is_empty())
            .unwrap_or(MusicMode::Track);
        self.at = 0;
    }

    /// What the line at the bottom of free ride says: what plays and, in a
    /// list, the song playing.
    pub fn describe(
        &self,
        circuit: usize,
        circuits: &Circuits,
        songs: &Songs,
        lang: Language,
    ) -> String {
        let music = match lang {
            Language::Spanish => "Música",
            Language::English => "Music",
        };
        match (self.mode, self.tune(circuit, circuits, songs)) {
            (MusicMode::AllTracks | MusicMode::Chill, Some(tune)) => format!(
                "{music}: {}  ·  {}",
                self.mode.name(lang),
                tune.name(circuits, songs, lang)
            ),
            _ => format!("{music}: {}", self.mode.name(lang)),
        }
    }
}

/// A song as a Bevy audio source: its MP3, loaded from the content pack
/// ([`SongLoader`]). It loops by decoding the MP3 again from the start:
/// Bevy's own looping keeps every decoded sample in memory (about 60 MB for
/// a 3-minute song).
#[derive(Asset, TypePath, Clone)]
pub struct SongSource {
    mp3: Arc<[u8]>,
}

impl Decodable for SongSource {
    type Decoder = rodio::decoder::LoopedDecoder<Cursor<Arc<[u8]>>>;

    fn decoder(&self) -> Self::Decoder {
        rodio::Decoder::builder()
            .with_data(Cursor::new(self.mp3.clone()))
            .with_byte_len(self.mp3.len() as u64)
            .with_hint("mp3")
            .with_gapless(true)
            .build_looped()
            .expect("the songs are valid MP3")
    }
}

/// Loads an MP3 as a [`SongSource`]: its bytes, decoded as it plays. (Bevy's
/// own audio does not read MP3s here: the game decodes them with rodio.)
#[derive(Default, TypePath)]
pub struct SongLoader;

impl AssetLoader for SongLoader {
    type Asset = SongSource;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _context: &mut LoadContext<'_>,
    ) -> Result<SongSource, Self::Error> {
        let mut mp3 = Vec::new();
        reader.read_to_end(&mut mp3).await?;
        Ok(SongSource { mp3: mp3.into() })
    }

    fn extensions(&self) -> &[&str] {
        &["mp3"]
    }
}

/// A song while it plays.
#[derive(Component)]
pub struct Music {
    tune: Tune,
    song: Song,
    /// From 0 (silent) to 1 (full volume).
    fader: f32,
    /// How long it has played (s), to go on to the next song of a list.
    played: f32,
}

/// How much `tune`'s `song` is turned up or down to play at its loudness.
fn gain(tune: Tune, song: Song) -> f32 {
    10f32.powf((tune.loudness() - song.loudness) / 20.0)
}

/// Where the fader goes: up while racing on the song's track (lower in the
/// pause menu), down to silence otherwise.
fn fader_target(racing: bool, paused: bool) -> f32 {
    match (racing, paused) {
        (false, _) => 0.0,
        (true, true) => PAUSED,
        (true, false) => 1.0,
    }
}

/// Moves the fader towards `target`, at the speed of [`FADE_IN`] going up and
/// [`FADE_OUT`] going down.
fn fade(fader: f32, target: f32, dt: f32) -> f32 {
    if target > fader {
        (fader + dt / FADE_IN).min(target)
    } else {
        (fader - dt / FADE_OUT).max(target)
    }
}

/// A new free ride starts with the track's song.
pub(super) fn reset_free_music(mut free: ResMut<FreeMusic>) {
    *free = FreeMusic::default();
}

/// In the menus with the music's key in their legend (the welcome screen,
/// the modes and the tracks), Y (or the top face button) turns the music on
/// or off, as Options > Sound does; not with the options open.
pub(super) fn toggle_menu_music(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    options: Option<Res<crate::options::OptionsMenu>>,
    mut settings: ResMut<Settings>,
) {
    let pressed = keyboard.just_pressed(MUSIC_KEY)
        || gamepads.iter().any(|pad| pad.just_pressed(MUSIC_BUTTON));
    if pressed && !options.is_some_and(|options| options.is_open) {
        settings.music_enabled = !settings.music_enabled;
    }
}

/// In free ride, Y (or the top face button) changes what plays.
pub(super) fn change_free_music(
    rules: Option<Res<Rules>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    pause: Res<Pause>,
    (circuit, circuits, songs): (Res<CurrentCircuit>, Res<Circuits>, Res<Songs>),
    mut free: ResMut<FreeMusic>,
) {
    let free_ride = rules.is_some_and(|rules| rules.mode == Mode::Free);
    let pressed = keyboard.just_pressed(MUSIC_KEY)
        || gamepads.iter().any(|pad| pad.just_pressed(MUSIC_BUTTON));
    if free_ride && pressed && !pause.is_open() {
        free.next_mode(circuit.0, &circuits, &songs);
    }
}

/// Starts the song of the screen: the menus', the track's in the race (in
/// free ride, the one [`FreeMusic`] says), none choosing the bike, nor
/// where there is no song. Moves its
/// fader, goes on to the next song of a list once one is over, and lets
/// each go once it has faded out. Real time, which keeps running in the
/// pause menu.
#[allow(clippy::too_many_arguments)]
pub(super) fn play_music(
    mut commands: Commands,
    time: Res<Time<Real>>,
    state: Res<State<GameState>>,
    circuit: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    songs: Res<Songs>,
    rules: Option<Res<Rules>>,
    mut free: Option<ResMut<FreeMusic>>,
    pause: Res<Pause>,
    controls: Res<SoundControls>,
    settings: Option<Res<Settings>>,
    assets: Res<AssetServer>,
    jukebox: Option<Res<Jukebox>>,
    mut playing: Query<(Entity, &mut Music, Option<&mut Sink>)>,
) {
    let jukebox = jukebox.and_then(|jukebox| jukebox.playing);
    let dt = time.delta_secs();
    let free_ride = rules.is_some_and(|rules| rules.mode == Mode::Free);
    let wanted = |free: Option<&FreeMusic>| match free.filter(|_| free_ride) {
        Some(free) => free.tune(circuit.0, &circuits, &songs),
        None => Some(Tune::Track(circuit.0)),
    };
    let racing = *state.get() == GameState::Race;
    // M mutes the music too.
    let master = controls.0.master.get() / MASTER;
    // The music player plays even with the music off, at its volume.
    let music_factor = settings
        .as_ref()
        .map_or(music_gain(DEFAULT_VOLUME), |settings| {
            if settings.music_enabled || jukebox.is_some() {
                music_gain(settings.music_volume)
            } else {
                0.0
            }
        });
    // The song of a list played to its end goes (it would start over), and
    // the next one comes.
    let mut over = None;
    if let Some(free) = free
        .as_deref_mut()
        .filter(|free| free.mode != MusicMode::Track)
        && free_ride
        && racing
    {
        let current = free.tune(circuit.0, &circuits, &songs);
        over = playing
            .iter()
            .find(|(_, music, _)| Some(music.tune) == current && music.played >= music.song.length)
            .map(|(entity, ..)| entity);
        if let Some(entity) = over {
            commands.entity(entity).despawn();
            free.at += 1;
        }
    }
    let tune = match (jukebox, state.get()) {
        (Some(tune), _) => Some(tune),
        (None, GameState::Race) => wanted(free.as_deref()),
        (None, GameState::Select) => None,
        (None, _) => Some(Tune::Menu),
    };
    // Not lowered by the pause menu, the player's song.
    let paused = pause.is_open() && jukebox.is_none();
    let mut started = false;
    for (entity, mut music, sink) in &mut playing {
        if over == Some(entity) {
            continue;
        }
        let current = tune == Some(music.tune);
        started |= current;
        music.fader = fade(music.fader, fader_target(current, paused), dt);
        if music.fader == 0.0 && !current {
            commands.entity(entity).despawn();
        } else if let Some(mut sink) = sink {
            music.played += dt;
            sink.set_volume(Volume::Linear(
                music.fader * gain(music.tune, music.song) * master * music_factor,
            ));
        }
    }
    if let Some(tune) = tune
        && !started
        && let Some(song) = tune.song(&circuits, &songs)
    {
        // Loaded from the pack as it starts (it fades in meanwhile), and let
        // go once it has faded out.
        let source: Handle<SongSource> = assets.load(content::path(song.file));
        commands.spawn((
            AudioPlayer(source),
            PlaybackSettings::ONCE.with_volume(Volume::Linear(0.0)),
            Music {
                tune,
                song,
                fader: 0.0,
                played: 0.0,
            },
        ));
    }
}

/// The songs to start on the web: loaded or not, with no voice yet.
#[cfg(target_arch = "wasm32")]
type Waiting<'w, 's> =
    Query<'w, 's, (Entity, &'static AudioPlayer<SongSource>), (With<Music>, Without<Sink>)>;

/// On the web, each song as soon as it is loaded, through the sound's own
/// way out (silent: [`play_music`] fades it in), with its voice on its
/// entity.
#[cfg(target_arch = "wasm32")]
pub fn start_web_songs(
    mut commands: Commands,
    songs: Res<Assets<SongSource>>,
    waiting: Waiting,
    mut audio: NonSendMut<super::output::web::WebAudio>,
) {
    use std::num::NonZero;

    for (entity, player) in &waiting {
        let Some(song) = songs.get(&player.0) else {
            continue;
        };
        let samples = rodio::source::UniformSourceIterator::new(
            song.decoder(),
            NonZero::new(2).unwrap(),
            NonZero::new(super::output::RATE).unwrap(),
        );
        let voice = super::output::Voice::new(0.0);
        audio.voices.play(samples, voice.clone());
        commands
            .entity(entity)
            .insert(super::output::web::WebSink(voice));
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use bevy::audio::AddAudioSource;
    use bevy::time::TimeUpdateStrategy;
    use rodio::Source;

    use super::super::synth::{Controls, Mixer, SAMPLE_RATE, pan};
    use super::*;

    /// Root mean square of the samples, in dB.
    fn rms_db(samples: &[f32]) -> f32 {
        let power = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
        10.0 * power.log10()
    }

    /// The songs not a track's, read once.
    static SONGS: std::sync::LazyLock<Songs> = std::sync::LazyLock::new(Songs::default);

    /// Every song of the game's `circuits` and its other songs, with its
    /// tune.
    fn every_tune(circuits: &Circuits) -> Vec<(Tune, Song)> {
        let tunes: Vec<Tune> = (0..circuits.len())
            .map(Tune::Track)
            .chain((0..SONGS.chill.len()).map(Tune::Chill))
            .chain([Tune::Menu])
            .chain((0..SONGS.loose.len()).map(Tune::Loose))
            .collect();
        tunes
            .into_iter()
            .filter_map(|tune| Some((tune, tune.song(circuits, &SONGS)?)))
            .collect()
    }

    /// A song's MP3, read from the disk.
    fn mp3(song: &Song) -> Arc<[u8]> {
        content::read(song.file).into()
    }

    pub fn every_song_is_in_the_pack_on(circuits: &'static Circuits) {
        for (tune, song) in every_tune(circuits) {
            let file = song.file;
            assert!(
                content::is_listed(file),
                "{tune:?}: {file} in no content pack"
            );
        }
    }

    pub fn every_song_is_turned_to_its_loudness_on(circuits: &'static Circuits) {
        for (tune, song) in every_tune(circuits) {
            let played = song.loudness + 20.0 * gain(tune, song).log10();
            assert!((played - tune.loudness()).abs() < 1e-3, "{tune:?}");
            assert!(gain(tune, song) < 1.0, "{tune:?}: turned down");
        }
        // The chill songs a little quieter than the tracks', the menus'
        // quieter still.
        const { assert!(CHILL_LOUDNESS < LOUDNESS - 3.0 && MENU_LOUDNESS < CHILL_LOUDNESS) };
    }

    pub fn songs_last_as_long_as_they_say_and_loop_back_to_the_start_on(
        circuits: &'static Circuits,
    ) {
        for (tune, song) in every_tune(circuits) {
            let mp3 = mp3(&song);
            let mut looped = SongSource { mp3: mp3.clone() }.decoder();
            assert_eq!(looped.channels().get(), 2, "{tune:?}");
            assert_eq!(looped.sample_rate().get(), 44_100, "{tune:?}");

            // The song once through, then the looped one past its end.
            let once = rodio::Decoder::builder()
                .with_data(Cursor::new(mp3.clone()))
                .with_byte_len(mp3.len() as u64)
                .with_hint("mp3")
                .with_gapless(true)
                .build()
                .unwrap();
            let length = once.count();
            let seconds = length as f32 / (44_100.0 * 2.0);
            assert!(
                (seconds - song.length).abs() < 0.3,
                "{tune:?}: {seconds} s, not {}",
                song.length
            );
            let start: Vec<f32> = looped.by_ref().take(44_100 * 2).collect();
            assert!(start.iter().any(|s| s.abs() > 0.01), "{tune:?}: not silent");
            let again: Vec<f32> = looped
                .by_ref()
                .skip(length - start.len())
                .take(start.len())
                .collect();
            assert_eq!(again, start, "{tune:?}: starts over");
        }
    }

    pub fn the_music_stays_well_under_the_player_s_engine_on(circuits: &'static Circuits) {
        // The player's engine flat out, in the middle, with the wind, as
        // `place_sounds` and `player_sounds` leave it.
        let controls = Arc::new(Controls::default());
        controls.master.set(MASTER);
        let engine = &controls.engines[0];
        engine.rpm.set(14_000.0);
        engine.load.set(1.0);
        let (left, right) = pan(1.0, 0.0);
        engine.left.set(left);
        engine.right.set(right);
        controls.wind.set(0.4);
        let mix: Vec<f32> = Mixer::new(controls)
            .skip(SAMPLE_RATE as usize)
            .take(SAMPLE_RATE as usize * 2 * 4)
            .collect();
        let engine = rms_db(&mix);

        // Not the loose songs: only the music player plays them, never
        // over a race's engines.
        let raced = every_tune(circuits)
            .into_iter()
            .filter(|(tune, _)| !matches!(tune, Tune::Loose(_)));
        for (tune, song) in raced {
            let samples: Vec<f32> = SongSource { mp3: mp3(&song) }
                .decoder()
                .take(SAMPLE_RATE as usize * 2 * 60)
                .map(|s| s * gain(tune, song))
                .collect();
            // Its loudest second.
            let loudest = samples
                .chunks(SAMPLE_RATE as usize * 2)
                .map(rms_db)
                .fold(f32::MIN, f32::max);
            assert!(
                loudest < engine - 10.0,
                "{tune:?}: {loudest:.1} dB against the engine's {engine:.1} dB"
            );
        }
    }

    #[test]
    fn fades_in_dips_in_the_pause_and_fades_out() {
        let mut fader = 0.0;
        let mut time = 0.0;
        while fader < 1.0 {
            fader = fade(fader, fader_target(true, false), 0.1);
            time += 0.1;
        }
        assert!((time - FADE_IN).abs() < 0.15, "fades in: {time}");
        for _ in 0..20 {
            fader = fade(fader, fader_target(true, true), 0.1);
        }
        assert_eq!(fader, PAUSED, "lower in the pause menu");
        let mut time = 0.0;
        while fader > 0.0 {
            fader = fade(fader, fader_target(false, false), 0.1);
            time += 0.1;
        }
        assert!(time <= FADE_OUT + 0.05, "fades out: {time}");
    }

    fn music_app(circuits: &Circuits) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(content::ContentPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .add_plugins(bevy::audio::AudioPlugin::default())
            .add_audio_source::<SongSource>()
            .init_asset_loader::<SongLoader>()
            .init_state::<GameState>()
            .init_resource::<Pause>()
            .insert_resource(CurrentCircuit(circuits.len().min(3) - 1))
            .insert_resource(*circuits)
            .init_resource::<Songs>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                100,
            )))
            .add_systems(Update, play_music);
        let controls = Arc::new(Controls::default());
        controls.master.set(MASTER);
        app.insert_resource(SoundControls(controls));
        app
    }

    fn songs_playing(app: &mut App) -> Vec<(Tune, f32)> {
        let mut query = app.world_mut().query::<&Music>();
        query
            .iter(app.world())
            .map(|music| (music.tune, music.fader))
            .collect()
    }

    fn go_to(app: &mut App, state: GameState) {
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(state);
    }

    pub fn the_menus_have_their_song_the_race_the_track_s_and_the_bikes_none_on(
        circuits: &'static Circuits,
    ) {
        let mut app = music_app(circuits);
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(songs_playing(&mut app), [(Tune::Menu, 1.0)], "the menus'");

        go_to(&mut app, GameState::Select);
        for _ in 0..15 {
            app.update();
        }
        assert!(songs_playing(&mut app).is_empty(), "none choosing the bike");

        go_to(&mut app, GameState::Race);
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(
            songs_playing(&mut app),
            [(Tune::Track(2), 1.0)],
            "the track's song"
        );

        go_to(&mut app, GameState::Welcome);
        app.update();
        app.update();
        let fading = songs_playing(&mut app);
        assert!(
            fading
                .iter()
                .any(|(tune, fader)| *tune == Tune::Track(2) && *fader < 1.0),
            "fades out: {fading:?}"
        );
        for _ in 0..15 {
            app.update();
        }
        let playing: Vec<Tune> = songs_playing(&mut app).iter().map(|(t, _)| *t).collect();
        assert_eq!(playing, [Tune::Menu], "and gives way to the menus'");
    }

    pub fn free_ride_plays_the_track_s_song_then_every_track_s_then_chill_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let (circuits, songs) = (circuits, &*SONGS);
        let mut free = FreeMusic::default();
        assert_eq!(free.tune(2, circuits, songs), Some(Tune::Track(2)));
        free.next_mode(2, circuits, songs);
        assert_eq!(free.mode, MusicMode::AllTracks);
        // From this track's, round them all.
        let tunes = free.mode.tunes(2, circuits, songs);
        assert_eq!(tunes.len(), circuits.len());
        assert_eq!(tunes[0], Tune::Track(2));
        assert_eq!(tunes[circuits.len() - 1], Tune::Track(1));
        free.next_mode(2, circuits, songs);
        assert_eq!(
            free.mode.tunes(2, circuits, songs),
            [Tune::Chill(0), Tune::Chill(1), Tune::Chill(2)]
        );
        free.at = 4;
        assert_eq!(free.tune(2, circuits, songs), Some(Tune::Chill(1)));
        assert!(
            free.describe(2, circuits, songs, Language::English)
                .contains("Sunlight on Still Water")
        );
        // And back to the track's, from the start.
        free.next_mode(2, circuits, songs);
        assert_eq!((free.mode, free.at), (MusicMode::Track, 0));
        assert_eq!(
            free.describe(2, circuits, songs, Language::Spanish),
            "Música: la del circuito"
        );
    }

    #[test]
    fn the_engine_s_demo_plays_no_song_and_free_ride_keeps_to_the_track_s() {
        let circuits = &*crate::testing::DEMO;
        let songs = Songs {
            menu: None,
            chill: Vec::new(),
            loose: Vec::new(),
        };
        assert!(circuits.at(0).song.is_none());
        let mut free = FreeMusic::default();
        assert_eq!(free.tune(0, circuits, &songs), None);
        free.next_mode(0, circuits, &songs);
        assert_eq!(free.mode, MusicMode::Track, "none has songs to play");
        let mut app = music_app(circuits);
        app.insert_resource(songs)
            .insert_resource(CurrentCircuit(0));
        for _ in 0..10 {
            app.update();
        }
        go_to(&mut app, GameState::Race);
        for _ in 0..10 {
            app.update();
        }
        assert!(songs_playing(&mut app).is_empty());
    }

    pub fn with_no_songs_nothing_plays_and_free_ride_keeps_to_what_has_some_on(
        circuits: &'static Circuits,
    ) {
        // A track with no song, and no chill songs.
        let _ = circuits;
        let mut files = crate::circuits::files().unwrap();
        files[0].1 = String::leak(
            files[0]
                .1
                .lines()
                .filter(|line| !line.trim_start().starts_with("song:"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let circuits = Circuits::parse(&files).unwrap();
        assert!(circuits.at(0).song.is_none());
        let songs = Songs {
            menu: None,
            chill: Vec::new(),
            loose: Vec::new(),
        };
        let mut free = FreeMusic::default();
        assert_eq!(free.tune(0, &circuits, &songs), None, "the track has none");
        // Every track's: the others'.
        free.next_mode(0, &circuits, &songs);
        assert_eq!(free.mode, MusicMode::AllTracks);
        assert_eq!(free.tune(0, &circuits, &songs), Some(Tune::Track(1)));
        // No chill, and the track's has none: every track's still, the
        // only one with songs to play here.
        free.next_mode(0, &circuits, &songs);
        assert_eq!(free.mode, MusicMode::AllTracks);
        // On a track with a song, the track's again after it.
        free.next_mode(1, &circuits, &songs);
        assert_eq!(free.mode, MusicMode::Track);

        // In the menus and on that track, nothing plays.
        let mut app = music_app(&circuits);
        app.insert_resource(circuits)
            .insert_resource(songs)
            .insert_resource(CurrentCircuit(0));
        for _ in 0..10 {
            app.update();
        }
        assert!(songs_playing(&mut app).is_empty(), "no menus' song");
        go_to(&mut app, GameState::Race);
        for _ in 0..10 {
            app.update();
        }
        assert!(songs_playing(&mut app).is_empty(), "no track's song");
    }

    pub fn a_song_of_a_list_played_through_gives_way_to_the_next_on(circuits: &'static Circuits) {
        let mut app = music_app(circuits);
        app.insert_resource(Rules {
            mode: Mode::Free,
            laps: 1,
        })
        .insert_resource(FreeMusic {
            mode: MusicMode::Chill,
            at: 0,
        });
        go_to(&mut app, GameState::Race);
        for _ in 0..30 {
            app.update();
        }
        let playing: Vec<Tune> = songs_playing(&mut app).iter().map(|(t, _)| *t).collect();
        assert_eq!(playing, [Tune::Chill(0)]);
        // Played through: the next one, at once.
        let mut query = app.world_mut().query::<&mut Music>();
        for mut music in query.iter_mut(app.world_mut()) {
            music.played = SONGS.chill[0].song.length;
        }
        app.update();
        app.update();
        let playing: Vec<Tune> = songs_playing(&mut app).iter().map(|(t, _)| *t).collect();
        assert_eq!(playing, [Tune::Chill(1)]);
        assert_eq!(app.world().resource::<FreeMusic>().at, 1);
    }

    #[test]
    fn songs_are_titled_by_their_files() {
        assert_eq!(
            title_of("music/crown_of_the_track.mp3"),
            "Crown of the Track"
        );
        assert_eq!(title_of("music/the_iron_paddock.mp3"), "The Iron Paddock");
        assert_eq!(title_of("music/a_lap_in_the_sun.mp3"), "A Lap in the Sun");
        assert_eq!(title_of("bahia.mp3"), "Bahia");
    }

    pub fn the_music_player_lists_every_song_once_and_plays_it_on(circuits: &'static Circuits) {
        let songs = &*SONGS;
        let tunes = player_tunes(circuits, songs);
        // Every song once: by its file, none twice.
        let files: Vec<&str> = tunes
            .iter()
            .map(|tune| tune.song(circuits, songs).unwrap().file)
            .collect();
        let mut distinct = files.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), files.len(), "{files:?}");
        let every: Vec<&str> = every_tune(circuits)
            .iter()
            .map(|(_, song)| song.file)
            .collect();
        assert!(every.iter().all(|file| files.contains(file)));
        // The tracks' first, then the menus', the chill ones and the loose.
        let order = |tune: &Tune| match tune {
            Tune::Track(_) => 0,
            Tune::Menu => 1,
            Tune::Chill(_) => 2,
            Tune::Loose(_) => 3,
        };
        assert!(
            tunes
                .windows(2)
                .all(|pair| order(&pair[0]) <= order(&pair[1]))
        );
        for &tune in &tunes {
            assert!(!tune.title(circuits, songs).is_empty(), "{tune:?}");
            assert!(!tune.played_for(circuits, Language::Spanish).is_empty());
        }

        // Choosing the bike, where nothing plays, the player's song, whole.
        let tune = *tunes.last().unwrap();
        let mut app = music_app(circuits);
        app.insert_resource(Jukebox {
            playing: Some(tune),
        });
        go_to(&mut app, GameState::Select);
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(songs_playing(&mut app), [(tune, 1.0)]);
        // Stopped, it fades out, and the screen's (none) is back.
        app.world_mut().resource_mut::<Jukebox>().playing = None;
        for _ in 0..15 {
            app.update();
        }
        assert!(songs_playing(&mut app).is_empty());
    }

    #[test]
    fn the_engine_s_demo_has_no_song_for_the_player() {
        let songs = Songs {
            menu: None,
            chill: Vec::new(),
            loose: Vec::new(),
        };
        assert!(player_tunes(&crate::testing::DEMO, &songs).is_empty());
    }
}
