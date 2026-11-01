//! Sonido: motores con caja de cambios, viento, neumáticos, hierba, golpes,
//! aterrizajes y el menú. Todo se sintetiza en tiempo real ([`synth`]); aquí
//! se decide qué suena, cuánto y en qué oído, desde la cámara.
//!
//! Cada moto lleva un [`Engine`]: sus revoluciones salen de la velocidad y la
//! marcha, que sube al llegar arriba y baja al caer. Los motores de los
//! rivales se oyen menos cuanto más lejos, a un lado o al otro, y más agudos
//! al acercarse que al alejarse (efecto Doppler). Con M se silencia todo.
//!
//! Each track also has its music ([`music`]), which plays during the race
//! well below the engines.

pub mod ambience;
pub mod music;
mod synth;

use std::sync::Arc;

use bevy::audio::AddAudioSource;
use bevy::prelude::*;

use crate::bike::{BikeInput, BikeState, LocalPlayer, Player};
use crate::circuits::{Ambience, Circuits, CurrentCircuit};
use crate::geometry::{approach, forward, smoothstep};
use crate::menu::{Choice, GameState};
use crate::pause::Pause;
use crate::race::RaceCue;
use crate::race::lights::StartBeep;
use crate::settings::{ControlsTest, DEFAULT_VOLUME, Settings, ToggleMute};
use crate::ui::MenuSound;
use crate::world::realm::PortalView;
pub use synth::Sound;
use synth::{Controls, Mix, REDLINE, VOICES, pan};

/// Volumen general.
const MASTER: f32 = 0.8;
/// Velocidad (m/s) a la que cada marcha llega al corte de inyección: the
/// bikes' own, which they change through with the gears on.
use crate::bike::GEARS;
/// Ralentí (alto, como el de una moto de carreras), cambio a la marcha de
/// arriba y a la de abajo (rpm): en carrera el motor va siempre alto.
const IDLE: f32 = 3000.0;
const UPSHIFT: f32 = 16_800.0;
const DOWNSHIFT: f32 = 10_500.0;
/// Revoluciones a las que patina el embrague al salir parado a fondo.
const LAUNCH: f32 = 9500.0;
/// Lo que se tarda en subir de marcha, sin gas (s): cambio casi instantáneo.
const SHIFT_TIME: f32 = 0.04;
/// Golpe de gas automático al reducir (s) y cuánto gas da.
const BLIP_TIME: f32 = 0.07;
const BLIP: f32 = 0.6;
/// Rapidez con la que las revoluciones siguen a la rueda (1/s).
const REV_RESPONSE: f32 = 14.0;
/// Distancia hasta la que un motor suena a todo volumen (m), y cuánto menos
/// suenan los rivales que el jugador.
const NEAR: f32 = 5.0;
const RIVAL_VOLUME: f32 = 0.7;
/// Velocidad del sonido (m/s), para el efecto Doppler, y lo que tarda la
/// velocidad de la cámara en seguir a la medida (s).
const SOUND_SPEED: f32 = 343.0;
const LISTENER_SMOOTHING: f32 = 0.25;
/// Caída de velocidad que ya es un golpe (m/s²) y golpe más fuerte (m/s).
const IMPACT_DECEL: f32 = 45.0;
const HARDEST_IMPACT: f32 = 20.0;
/// Velocidad de caída del aterrizaje más fuerte (m/s).
const HARDEST_LANDING: f32 = 9.0;

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        let controls = Arc::new(Controls::default());
        controls.master.set(MASTER);
        app.add_audio_source::<Mix>()
            .add_audio_source::<music::SongSource>()
            .init_asset_loader::<music::SongLoader>()
            .insert_resource(SoundControls(controls))
            .add_message::<StartBeep>()
            .add_message::<RaceCue>()
            .add_message::<ToggleMute>()
            .init_resource::<ControlsTest>()
            .init_resource::<Blip>()
            .init_resource::<music::FreeMusic>()
            .init_resource::<music::Songs>()
            .add_systems(OnEnter(GameState::Race), music::reset_free_music)
            .add_systems(Startup, start_mixer)
            .add_systems(
                Update,
                (
                    sync_settings_sound,
                    attach_engines,
                    (menu_blip.run_if(in_state(GameState::Select)), run_engines).chain(),
                    place_sounds,
                    player_sounds,
                    pause_sounds,
                    surroundings,
                    menu_sounds,
                    toggle_mute,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    music::change_free_music.run_if(in_state(GameState::Race)),
                    music::toggle_menu_music.run_if(
                        in_state(GameState::Welcome)
                            .or_else(in_state(GameState::Modes))
                            .or_else(in_state(GameState::Tracks)),
                    ),
                    music::play_music,
                )
                    .chain()
                    .after(toggle_mute),
            );
    }
}

#[derive(Resource)]
struct SoundControls(Arc<Controls>);

/// How loud the effects and the music play at [`DEFAULT_VOLUME`]: the
/// balance found by ear, with the effects at a fifth of the old scale and
/// the music at nine tenths.
const FX_AT_DEFAULT: f32 = 0.2;
const MUSIC_AT_DEFAULT: f32 = 0.9;

/// Gain of the sound effects at a volume of `level` %.
pub fn fx_gain(level: u32) -> f32 {
    level as f32 / DEFAULT_VOLUME as f32 * FX_AT_DEFAULT
}

/// Gain of the music at a volume of `level` %.
pub fn music_gain(level: u32) -> f32 {
    level as f32 / DEFAULT_VOLUME as f32 * MUSIC_AT_DEFAULT
}

fn start_mixer(
    mut commands: Commands,
    controls: Res<SoundControls>,
    settings: Option<Res<Settings>>,
    mut mixes: ResMut<Assets<Mix>>,
) {
    if let Some(settings) = settings {
        controls.0.fx.set(fx_gain(settings.fx_volume));
    }
    let mix = mixes.add(Mix {
        controls: controls.0.clone(),
    });
    commands.spawn(AudioPlayer(mix));
}

fn sync_settings_sound(settings: Option<Res<Settings>>, controls: Res<SoundControls>) {
    if let Some(settings) = settings
        && settings.is_changed()
    {
        controls.0.fx.set(fx_gain(settings.fx_volume));
    }
}

/// El motor de una moto.
#[derive(Component, Clone, Copy, Debug)]
pub struct Engine {
    /// Marcha, de 0 (primera) a 5 (sexta).
    gear: usize,
    rpm: f32,
    /// Lo que queda del cambio de marcha y del golpe de gas al reducir (s).
    shifting: f32,
    blipping: f32,
    /// Gas efectivo: sin gas mientras cambia o al cortar la inyección.
    load: f32,
    /// Para notar golpes y aterrizajes: dónde estaba, su velocidad, si
    /// tocaba el suelo y a qué velocidad caía en el fotograma anterior.
    last_position: Vec2,
    last_speed: f32,
    last_grounded: bool,
    last_climb: f32,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            gear: 0,
            rpm: IDLE,
            shifting: 0.0,
            blipping: 0.0,
            load: 0.0,
            last_position: Vec2::ZERO,
            last_speed: 0.0,
            last_grounded: true,
            last_climb: 0.0,
        }
    }
}

impl Engine {
    /// Avanza el motor `dt` segundos. En punto muerto (`neutral`) las
    /// revoluciones solo dependen del gas; si no, de la rueda y la marcha, y
    /// en el aire o arrancando parado el gas las sube más.
    pub fn run(
        &mut self,
        speed: f32,
        throttle: f32,
        grounded: bool,
        neutral: bool,
        time: f32,
        dt: f32,
    ) {
        let throttle = throttle.clamp(0.0, 1.0);
        let speed = speed.abs();
        self.shifting = (self.shifting - dt).max(0.0);
        self.blipping = (self.blipping - dt).max(0.0);
        let wheel = |gear: usize| speed / GEARS[gear] * REDLINE;

        let target = if neutral {
            IDLE + throttle * (0.75 * REDLINE - IDLE)
        } else {
            if self.shifting == 0.0 {
                if self.gear + 1 < GEARS.len() && wheel(self.gear) > UPSHIFT {
                    self.gear += 1;
                    self.shifting = SHIFT_TIME;
                } else if self.gear > 0 && wheel(self.gear) < DOWNSHIFT {
                    self.gear -= 1;
                    self.blipping = BLIP_TIME;
                }
            }
            let mut target = wheel(self.gear).max(IDLE);
            if self.gear == 0 {
                // El embrague patina: a fondo y despacio, el motor sube.
                target = target.max(IDLE + throttle * (LAUNCH - IDLE));
            }
            if !grounded {
                // En el aire la rueda gira libre.
                target = target.max(throttle * 0.95 * REDLINE);
            }
            target
        };
        self.rpm = approach(self.rpm, target.min(REDLINE), REV_RESPONSE, dt);

        // Corte de inyección: el gas se corta a golpes contra el tope.
        let limiter = self.rpm > REDLINE - 150.0 && (time * 18.0).fract() < 0.5;
        self.load = if self.shifting > 0.0 || limiter {
            0.0
        } else if self.blipping > 0.0 {
            throttle.max(BLIP)
        } else {
            throttle
        };
    }
}

/// Toda moto nueva lleva su motor.
fn attach_engines(
    mut commands: Commands,
    bikes: Query<Entity, (With<BikeState>, Without<Engine>)>,
) {
    for bike in &bikes {
        commands.entity(bike).insert(Engine::default());
    }
}

/// En el menú, al cambiar de moto, la elegida da un acelerón.
#[derive(Resource, Default)]
struct Blip(f32);

fn menu_blip(
    time: Res<Time>,
    choice: Res<Choice>,
    controls: Res<SoundControls>,
    mut blip: ResMut<Blip>,
) {
    blip.0 = (blip.0 - time.delta_secs() * 2.5).max(0.0);
    if choice.is_changed() && !choice.is_added() {
        blip.0 = 1.0;
        controls.0.play(Sound::Click);
    }
}

/// Lo que suena al moverse por los menús, the beeps of the start lights,
/// and the moments of a race: a fanfare for a win, a chord for coming in, a
/// sparkle for a record, a sweep for a rival out and falling notes for a
/// player out or their time up.
fn menu_sounds(
    mut sounds: MessageReader<MenuSound>,
    mut beeps: MessageReader<StartBeep>,
    mut cues: MessageReader<RaceCue>,
    controls: Res<SoundControls>,
) {
    for sound in sounds.read() {
        controls.0.play(match sound {
            MenuSound::Move => Sound::Click,
            MenuSound::Confirm => Sound::Confirm,
        });
    }
    for beep in beeps.read() {
        controls.0.play(match beep {
            StartBeep::Light => Sound::StartLight,
            StartBeep::Go => Sound::StartGo,
        });
    }
    for cue in cues.read() {
        controls.0.play(match cue {
            RaceCue::Won => Sound::Victory,
            RaceCue::Finished => Sound::Finished,
            RaceCue::Record => Sound::Record,
            RaceCue::RivalOut => Sound::KnockedOut,
            RaceCue::Lost => Sound::Defeat,
        });
    }
}

fn run_engines(
    time: Res<Time>,
    state: Res<State<GameState>>,
    blip: Res<Blip>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<PortalView>)>,
    mut bikes: Query<(&BikeState, &BikeInput, &GlobalTransform, &mut Engine)>,
) {
    let dt = time.delta_secs();
    let in_menu = *state.get() != GameState::Race;
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    // En el menú, la moto elegida es la que está más cerca de la cámara.
    let nearest = bikes
        .iter()
        .map(|(_, _, transform, _)| transform.translation().distance(camera.translation()))
        .fold(f32::MAX, f32::min);
    for (state, input, transform, mut engine) in &mut bikes {
        let throttle = if in_menu {
            let chosen = transform.translation().distance(camera.translation()) <= nearest;
            if chosen { blip.0 } else { 0.0 }
        } else {
            input.throttle
        };
        engine.run(
            state.speed,
            throttle,
            state.grounded,
            in_menu,
            time.elapsed_secs(),
            dt,
        );
    }
}

/// Coloca cada motor respecto a la cámara (volumen, oído y Doppler) y suelta
/// los golpes y los aterrizajes. With two players there is a camera each:
/// every bike is heard from the nearer one, and both players' bikes sound
/// as the player's (full volume, no Doppler).
#[allow(clippy::type_complexity)]
fn place_sounds(
    time: Res<Time>,
    controls: Res<SoundControls>,
    cameras: Query<(Entity, &GlobalTransform), (With<Camera3d>, Without<PortalView>)>,
    mut listener_motion: Local<bevy::platform::collections::HashMap<Entity, (Vec3, Vec3)>>,
    mut bikes: Query<(
        Entity,
        &BikeState,
        &GlobalTransform,
        &mut Engine,
        Has<Player>,
        Has<LocalPlayer>,
    )>,
) {
    let dt = time.delta_secs().max(1e-4);
    // Each camera with its velocity, smoothed: measured from one frame to
    // the next it shakes, and the shaking would be heard in the Doppler of
    // every engine.
    let mut listeners = Vec::new();
    for (camera, transform) in &cameras {
        let at = transform.translation();
        let (last, velocity) = listener_motion.entry(camera).or_insert((at, Vec3::ZERO));
        let measured = (at - *last) / dt;
        *velocity = velocity.lerp(measured, 1.0 - (-dt / LISTENER_SMOOTHING).exp());
        *last = at;
        listeners.push((at, *velocity, transform.right().as_vec3()));
    }
    listener_motion.retain(|camera, _| cameras.contains(*camera));
    if listeners.is_empty() {
        return;
    }

    // Cada moto, siempre con la misma voz: por orden de creación, no de
    // distancia, que cambia cuando dos motos se cruzan.
    let mut voices: Vec<_> = bikes.iter_mut().collect();
    voices.sort_by_key(|voice| voice.0);
    for (index, params) in controls.0.engines.iter().enumerate() {
        let Some((_, state, transform, engine, player, other_player)) = voices.get_mut(index)
        else {
            params.left.set(0.0);
            params.right.set(0.0);
            continue;
        };
        let player = *player || *other_player;
        let at = transform.translation();
        let (listener, listener_velocity, right) = *nearest_listener(&listeners, at);
        let offset = at - listener;
        let distance = offset.length().max(0.1);
        let direction = offset / distance;
        let volume = (NEAR / distance).min(1.0) * if player { 1.0 } else { RIVAL_VOLUME };
        let (left, right_gain) = pan(volume, direction.dot(right));
        params.left.set(left);
        params.right.set(right_gain);
        // Doppler: más agudo si se acerca, más grave si se aleja. La moto del
        // jugador va con la cámara: la suya no cambia.
        let doppler = if player {
            1.0
        } else {
            let along = forward(state.heading) * state.speed;
            let velocity = Vec3::new(along.x, state.climb, along.y);
            let away = (velocity - listener_velocity).dot(direction);
            (SOUND_SPEED / (SOUND_SPEED + away)).clamp(0.8, 1.25)
        };
        params.rpm.set(engine.rpm * doppler);
        params.load.set(engine.load);
        params.wheelie.set(state.chassis.wheelie_lift());

        // Al reiniciar la carrera las motos saltan a la parrilla: eso no es
        // un golpe.
        let moved = state.position.distance(engine.last_position);
        let teleported = moved > engine.last_speed.abs() * dt * 2.0 + 1.0;
        let (left, right) = pan(volume.max(0.15), direction.dot(right));
        // Golpes: la moto pierde velocidad de repente.
        let lost = engine.last_speed.abs() - state.speed.abs();
        if lost / dt > IMPACT_DECEL && lost > 1.0 && !teleported {
            controls.0.play(Sound::Impact {
                strength: (lost / HARDEST_IMPACT).min(1.0),
                left,
                right,
            });
        }
        // Aterrizajes: vuelve a tocar el suelo después de volar.
        if state.grounded && !engine.last_grounded && !teleported {
            controls.0.play(Sound::Landing {
                strength: (-engine.last_climb / HARDEST_LANDING).clamp(0.2, 1.0),
                left,
                right,
            });
        }
        remember(state, engine);
    }
    // Las motos sin voz también se apuntan lo último, por si la recuperan.
    for (_, state, _, engine, _, _) in voices.iter_mut().skip(VOICES) {
        remember(state, engine);
    }
}

fn remember(state: &BikeState, engine: &mut Engine) {
    engine.last_position = state.position;
    engine.last_speed = state.speed;
    engine.last_grounded = state.grounded;
    engine.last_climb = state.climb;
}

/// The bikes the players ride: one, or two in split screen.
type PlayerBikes = Or<(With<Player>, With<LocalPlayer>)>;

/// Viento, chirrido de neumáticos y hierba: los de la moto del jugador, or
/// of both players' bikes with two (whichever sounds louder).
fn player_sounds(
    controls: Res<SoundControls>,
    tuning: Res<crate::bike::BikeTuning>,
    players: Query<(&BikeState, &BikeInput), PlayerBikes>,
) {
    let controls = &controls.0;
    let [wind, skid, grass] = players
        .iter()
        .map(|(state, input)| rider_noise(state, input, tuning.max_speed))
        .fold([0.0f32; 3], |loudest, noise| {
            std::array::from_fn(|i| loudest[i].max(noise[i]))
        });
    controls.wind.set(wind);
    controls.skid.set(skid);
    controls.grass.set(grass);
}

/// How loud a player's wind, tyre squeal and grass are.
fn rider_noise(state: &BikeState, input: &BikeInput, max_speed: f32) -> [f32; 3] {
    let speed = state.speed.abs();
    let fast = speed / max_speed;
    let wind = 0.6 * fast * fast;
    // Chirría al apurar el agarre en las curvas o al frenar fuerte deprisa.
    let skid = if state.grounded {
        let cornering = smoothstep(0.8, 1.0, state.steer.abs()) * smoothstep(15.0, 30.0, speed);
        let braking = smoothstep(0.6, 1.0, input.brake) * smoothstep(12.0, 30.0, speed);
        cornering.max(0.7 * braking)
    } else {
        0.0
    };
    let grass = (state.grass * 4.0).min(1.0) * (speed / 20.0).min(1.0);
    [
        wind,
        0.25 * skid,
        if state.grounded { 0.35 * grass } else { 0.0 },
    ]
}

/// The camera nearest to `at`, of those listening: their position,
/// velocity and right.
fn nearest_listener(listeners: &[(Vec3, Vec3, Vec3)], at: Vec3) -> &(Vec3, Vec3, Vec3) {
    listeners
        .iter()
        .min_by(|a, b| {
            a.0.distance_squared(at)
                .total_cmp(&b.0.distance_squared(at))
        })
        .expect("there is at least one listener")
}

/// Choosing the bike, the sounds of the chosen track's surroundings;
/// elsewhere, none (they fade in and out in the synthesiser).
fn surroundings(
    state: Res<State<GameState>>,
    circuit: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    controls: Res<SoundControls>,
) {
    let ambience = if *state.get() == GameState::Select {
        circuits.get(*circuit).ambience
    } else {
        Ambience::default()
    };
    let params = &controls.0.ambience;
    params.sea.set(ambience.sea);
    params.gulls.set(ambience.gulls);
    params.birds.set(ambience.birds);
    params.cicadas.set(ambience.cicadas);
    params.wind.set(ambience.wind);
}

/// En pausa se apaga el ruido de la carrera, y moverse por el menú suena.
fn pause_sounds(pause: Res<Pause>, controls: Res<SoundControls>) {
    if !pause.is_changed() || pause.is_added() {
        return;
    }
    controls
        .0
        .muffle
        .set(if pause.is_open() { 1.0 } else { 0.0 });
    controls.0.play(Sound::Click);
}

/// M (or Select and Y on a gamepad) turns the sound off or back on; not
/// while the controls are tried out.
fn toggle_mute(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut shortcut: MessageReader<ToggleMute>,
    test: Res<ControlsTest>,
    controls: Res<SoundControls>,
) {
    let pad = shortcut.read().count() % 2 == 1;
    let key = keyboard.just_pressed(KeyCode::KeyM) && !test.on;
    if key != pad {
        let master = &controls.0.master;
        master.set(if master.get() > 0.0 { 0.0 } else { MASTER });
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn each_bike_is_heard_from_the_nearer_camera() {
        let listeners = [
            (Vec3::ZERO, Vec3::ZERO, Vec3::X),
            (Vec3::new(100.0, 0.0, 0.0), Vec3::ZERO, Vec3::X),
        ];
        let near = |at: Vec3| nearest_listener(&listeners, at).0;
        assert_eq!(near(Vec3::new(90.0, 0.0, 5.0)), listeners[1].0);
        assert_eq!(near(Vec3::new(10.0, 0.0, -5.0)), listeners[0].0);
    }

    #[test]
    fn with_two_players_both_bikes_sound_as_the_player() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        world.insert_resource(SoundControls(Arc::new(Controls::default())));
        // A camera behind each player, far apart.
        for x in [0.0, 400.0] {
            world.spawn((
                Camera3d::default(),
                GlobalTransform::from(
                    Transform::from_xyz(x, 2.0, 5.0).looking_at(Vec3::new(x, 0.0, 0.0), Vec3::Y),
                ),
            ));
        }
        let bike = |x: f32| {
            (
                BikeState::parked_at((Vec2::new(x, 0.0), 0.0)),
                GlobalTransform::from(Transform::from_xyz(x, 0.0, 0.0)),
                Engine::default(),
            )
        };
        world.spawn((bike(0.0), Player));
        world.spawn((bike(400.0), LocalPlayer(1)));
        world.run_system_once(place_sounds).unwrap();
        let controls = &world.resource::<SoundControls>().0;
        let loudness = |voice: usize| {
            let params = &controls.engines[voice];
            params.left.get() + params.right.get()
        };
        // Player 2 is as loud as player 1, though 400 m from player 1's camera.
        assert!(loudness(1) > 0.0);
        assert!(
            (loudness(1) - loudness(0)).abs() < 1e-3,
            "{} {}",
            loudness(0),
            loudness(1)
        );
    }

    #[test]
    fn the_default_volume_is_the_balance_found_by_ear() {
        // Effects at 20 % and music at 90 % of the old scale sounded right:
        // that is now 80 % of both.
        assert_eq!(fx_gain(DEFAULT_VOLUME), 0.2);
        assert_eq!(music_gain(DEFAULT_VOLUME), 0.9);
        assert_eq!(fx_gain(0), 0.0);
        assert_eq!(music_gain(0), 0.0);
        // Linear: each step of 10 % is an eighth of the default.
        assert!((fx_gain(40) - 0.1).abs() < 1e-6);
        assert!((music_gain(100) - 1.125).abs() < 1e-6);
    }

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn gears_go_up_while_accelerating_and_down_when_braking() {
        let mut engine = Engine::default();
        let mut gears = Vec::new();
        for step in 0..=800 {
            let speed = step as f32 * 0.1;
            engine.run(speed, 1.0, true, false, step as f32 * DT, DT);
            assert!(
                (IDLE - 1.0..=REDLINE).contains(&engine.rpm),
                "{}",
                engine.rpm
            );
            if gears.last() != Some(&engine.gear) {
                gears.push(engine.gear);
            }
        }
        assert_eq!(gears, [0, 1, 2, 3, 4, 5], "sube las seis marchas");
        for step in (0..=800).rev() {
            engine.run(step as f32 * 0.1, 0.0, true, false, 0.0, DT);
        }
        assert_eq!(engine.gear, 0, "y las baja al frenar");
        assert!(
            (engine.rpm - IDLE).abs() < 200.0,
            "al ralentí: {}",
            engine.rpm
        );
    }

    #[test]
    fn launching_and_flying_rev_the_engine_up() {
        let mut engine = Engine::default();
        for _ in 0..60 {
            engine.run(0.0, 1.0, true, false, 0.0, DT);
        }
        assert!(
            engine.rpm > 0.9 * LAUNCH,
            "sale con el embrague: {}",
            engine.rpm
        );
        let mut flying = Engine::default();
        for _ in 0..60 {
            flying.run(30.0, 1.0, false, false, 0.0, DT);
        }
        assert!(flying.rpm > 0.9 * REDLINE, "en el aire se dispara");
    }

    #[test]
    fn shifting_cuts_the_throttle_for_a_moment() {
        let mut engine = Engine {
            gear: 0,
            rpm: UPSHIFT,
            ..default()
        };
        engine.run(
            GEARS[0] * UPSHIFT / REDLINE + 0.5,
            1.0,
            true,
            false,
            0.0,
            DT,
        );
        assert_eq!(engine.gear, 1);
        assert_eq!(engine.load, 0.0);
        for _ in 0..10 {
            engine.run(
                GEARS[0] * UPSHIFT / REDLINE + 0.5,
                1.0,
                true,
                false,
                0.0,
                DT,
            );
        }
        assert_eq!(engine.load, 1.0, "y vuelve a acelerar");
    }
}
