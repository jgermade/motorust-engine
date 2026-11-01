//! La moto: controles, física arcade a paso fijo y su representación visual.
//!
//! La física no simula el equilibrio sobre dos ruedas: la moto se mueve como
//! un cuerpo con velocidad y orientación, y la inclinación es solo visual (la
//! que tendría una moto real tomando esa curva a esa velocidad).

pub mod ai;
mod chassis;
pub mod crash;
pub mod curves;
mod grass_kickup;
pub mod learning;
mod loop_ride;
pub mod model;
pub mod wheelie_halo;

#[cfg(any(test, feature = "testing"))]
use crash::{Body, carry, collide};
pub use crash::{CrashState, Knock};
pub use grass_kickup::{DustPuff, GrassKickup, GrassKickupPlugin, GrassParticle};
pub use loop_ride::Ride;

use bevy::input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest};
use bevy::{ecs::system::SystemParam, prelude::*};

use crate::geometry::{approach, forward, heading_of, smoothstep, to_world, wrap_angle};
use crate::lobby::{ControllerBinding, LocalPlayersConfig};
use crate::locale::Language;
use crate::menu::{Choice, GameState, SplitChoices};
use crate::pause::{self, DebugOptions, StickCurve, TurnLimit};
use crate::settings::{Action, Settings};
use crate::track::{SURFACE_Y, Surface, Track};
use crate::welcome::GameMode;
use ai::Rival;
use chassis::{Chassis, Forces};
#[cfg(any(test, feature = "testing"))]
use curves::KeyboardShapes;
use curves::{CurveLean, CurveProbe, KeyboardCurves, KeyboardMode};
use model::{
    BikeBlueprint, BikeModelPlugin, COVER, DIAGONAL, Design, FLECHA, Livery, ROSSO, TRICOLOR, VERDE,
};

const GRAVITY: f32 = 9.81;
/// En el aire la moto sube y cae con más gravedad que en la realidad (y aún
/// más al bajar): los saltos son cortos y secos, como en un arcade.
const RISE_GRAVITY: f32 = 2.0 * GRAVITY;
const FALL_GRAVITY: f32 = 4.0 * GRAVITY;
/// En el aire se puede corregir algo la dirección (fracción del giro normal).
const AIR_STEERING: f32 = 0.3;
/// Forma de la inclinación según el agarre usado: por debajo de 1 se tumba
/// algo más al principio de la curva que al final.
const LEAN_CURVE: f32 = 0.75;
/// Con teclado o cruceta (todo o nada), la dirección va hacia donde se pulsa
/// desde el primer instante, ya deprisa, y cada vez más cuanto más tiempo se
/// lleva pulsando: empieza a esta rapidez (1/s) y acelera tanto así (1/s²).
/// Al soltar vuelve al centro más suave: empieza despacio y va cada vez más
/// deprisa cuanto más tiempo se lleva sin pulsar.
const DIGITAL_STEER_START: f32 = 2.5;
const DIGITAL_STEER_ACCEL: f32 = 5.0;
const DIGITAL_RELEASE_START: f32 = 0.5;
const DIGITAL_RELEASE_ACCEL: f32 = 4.0;
/// Cuanto más deprisa va la moto, menos gira el teclado manteniéndolo
/// pulsado: a toda velocidad, hasta esta fracción del tope, casi sin entrar
/// en la zona en la que la moto pierde velocidad (ver `CORNER_LOSS_FROM`).
/// Despacio gira del todo. Entre estas dos velocidades (m/s) se pasa de uno
/// a otro. Responde igual de rápido a cualquier velocidad. Es el límite
/// «alto» de las opciones de depuración, que pueden quitarlo o dejar una
/// parte, para el teclado y para el stick.
const DIGITAL_STEER_FAST: f32 = 0.8;
const DIGITAL_STEER_SPEEDS: (f32, f32) = (15.0, 70.0);
/// With the keyboard's curves (see [`curves`]), the speed from which the
/// bike leans all they ask (m/s), and how closely the lean shown follows it
/// (1/s): within a few hundredths of a second, so that their times are
/// the ones seen.
const CURVE_LEAN_SPEED: f32 = 15.0;
const CURVE_LEAN_FOLLOW: f32 = 40.0;
/// The lean shown soft, about 80 ms behind (1/s); smooth, a critically
/// damped spring (1/s) whose lean is about three quarters there in 150 ms,
/// with no jolt starting or stopping (see [`curves::LeanLook`]).
const SOFT_LEAN_FOLLOW: f32 = 12.5;
const SMOOTH_LEAN_SPRING: f32 = 13.3;
/// Lo que tarda la hierba en frenar del todo (s), y la parte de ese tiempo
/// en la que apenas se nota: pisarla un momento casi no frena. Al volver al
/// asfalto se recupera el doble de rápido.
const GRASS_BITE_TIME: f32 = 1.0;
const GRASS_GRACE: f32 = 0.25;
/// Rapidez con la que el cabeceo sigue a la pendiente o a la trayectoria (1/s).
const PITCH_RESPONSE: f32 = 10.0;
/// Medio ancho de la moto con el piloto: lo que se puede arrimar a un muro.
const BIKE_HALF_WIDTH: f32 = 0.45;
/// Fracción de la velocidad que se pierde en un choque de frente.
const IMPACT_SPEED_LOSS: f32 = 0.75;
/// Qué parte del ángulo contra el muro se corrige en cada paso de contacto:
/// la moto se pone paralela al muro y sigue, en vez de quedarse clavada.
const WALL_ALIGN: f32 = 0.6;
/// En curva el motor empuja algo más (fracción, del todo desde
/// `CORNER_GRIP_FROM`): la moto sale lanzada de las curvas en vez de quedarse
/// en ellas.
const CORNER_DRIVE: f32 = 0.4;
/// Desde qué parte del tope de la dirección se apura la curva: la moto agarra
/// cada vez más, hasta tanto más (fracción) con la dirección a tope, despacio
/// .. deprisa.
const CORNER_GRIP_FROM: f32 = 0.7;
const CORNER_GRIP: std::ops::Range<f32> = 0.6..1.4;
/// Y solo desde aquí, casi a tope, el empuje de más se acaba, el motor empuja
/// menos (fracción) y la moto pierde velocidad sola (m/s², deprisa) lo justo
/// para cerrar las curvas más cerradas: en arcade, mantener el giro basta para
/// no salirse. Tumbarse sin llegar ahí no cuesta velocidad.
const CORNER_LOSS_FROM: f32 = 0.85;
const CORNER_POWER_CUT: f32 = 0.6;
const CORNER_SCRUB: f32 = 35.0;
/// How much more the front grips loaded by the braking (a share of the
/// grip, with the fork dived all it can; see [`Chassis::front_load`]).
const BRAKE_GRIP: f32 = 0.15;
/// With the gears on (Options > Game), the speed each gear runs up to
/// (m/s, the engine's sound has the same), how long changing up takes
/// with no drive (s), and how far below the gear under it the bike goes
/// down to it (a share of that gear's top).
pub const GEARS: [f32; 6] = [26.0, 38.0, 49.0, 60.0, 71.0, 94.0];
const GEAR_SHIFT_TIME: f32 = 0.12;
const GEAR_DOWN: f32 = 0.7;
/// Changing up from this share of a gear's top speed.
const GEAR_UP: f32 = 0.97;
/// How much more the engine drives at its strongest in a gear (at
/// [`TORQUE_PEAK`] of the gear's top), less on either side.
const TORQUE_BOOST: f32 = 0.2;
const TORQUE_PEAK: f32 = 0.8;
const TORQUE_SPREAD: f32 = 0.35;

/// How hard a booster drives a bike on (m/s²): through one at the speed
/// bikes take a toy track's bends, up to its speed or nearly.
const BOOSTER_ACCEL: f32 = 60.0;

/// Inclinación de la moto en parado con el pie derecho en el suelo (rad).
pub const STOPPED_LEAN: f32 = -0.12;
/// Por debajo de esta velocidad la moto se inclina en parado y el piloto baja el pie (m/s).
pub const STOPPED_SPEED: f32 = 2.5;

pub struct BikePlugin;

impl Plugin for BikePlugin {
    fn build(&self, app: &mut App) {
        // The bikes, read from their file once, as the game starts.
        app.init_resource::<Roster>();
        app.add_plugins((
            BikeModelPlugin,
            GrassKickupPlugin,
            wheelie_halo::WheelieHaloPlugin,
            learning::LearningPlugin,
        ))
        .init_resource::<BikeTuning>()
        .init_resource::<CurveProbe>()
        .init_resource::<Race>()
        .add_message::<RecallToGrid>()
        .init_resource::<SplitChoices>()
        .add_systems(OnEnter(GameState::Race), spawn_grid)
        .add_systems(Update, follow_gears)
        .add_systems(
            FixedUpdate,
            (
                ai::drive_rivals,
                simulate_bikes,
                separate_bikes,
                rumble_controllers,
            )
                .chain(),
        )
        .add_systems(
            RunFixedMainLoop,
            (
                (restart_race, read_player_input)
                    .run_if(in_state(GameState::Race).and_then(pause::running))
                    .in_set(RaceControls)
                    .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
                interpolate_bikes.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
            ),
        );
    }
}

/// Every bike back to its grid slot, as a restart does: after a false
/// start (see [`crate::race::lights`]).
#[derive(Message, Clone, Copy, Debug)]
pub struct RecallToGrid;

/// The systems that read the players' controls in the race (and restart
/// it): other plugins can hold them, as the results of a race do.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RaceControls;

/// Estado de la carrera: los rivales esperan en la parrilla hasta que el
/// jugador acelera por primera vez; or, with a start at the lights, until
/// they go out.
#[derive(Resource, Default)]
pub struct Race {
    pub started: bool,
    /// The race starts at the lights ([`crate::race::lights`]): the
    /// throttle does not start it.
    pub lights: bool,
    /// And until it starts every bike stays on the grid, though the
    /// players' engines rev. Unheld, a player who moves off before the
    /// lights go out is disqualified.
    pub held: bool,
}

/// A gamepad's throttle and brake only count once they have been seen at
/// rest since the race was set up. Cross/A confirms in the menus and also
/// accelerates, so the press that enters the race must not start the bike;
/// nor must a trigger that does not read zero while the player is not
/// touching it.
#[derive(Default)]
struct PadArming {
    /// Per gamepad, whether its throttle and brake have been seen at rest.
    armed: bevy::platform::collections::HashMap<Entity, [bool; 2]>,
    /// Whether the race had started, to start over when a new one is set up.
    started: bool,
}

/// Below this a gamepad control counts as released.
const AT_REST: f32 = 0.1;
const THROTTLE: usize = 0;
const BRAKE: usize = 1;

impl PadArming {
    /// Called every frame with whether the race has started: when a new race
    /// is set up (or restarted), every control has to be released again.
    fn track(&mut self, started: bool) {
        if self.started && !started {
            self.armed.clear();
        }
        self.started = started;
    }

    /// What control `slot` ([`THROTTLE`] or [`BRAKE`]) of `pad` reading
    /// `value` counts for: nothing until it has been seen at rest.
    fn filter(&mut self, pad: Entity, slot: usize, value: f32) -> f32 {
        let armed = &mut self.armed.entry(pad).or_default()[slot];
        *armed |= value < AT_REST;
        if *armed { value } else { 0.0 }
    }
}

/// Puesto de salida de cada moto ([`Track::grid_slot`]), para volver a él al
/// reiniciar la carrera.
#[derive(Component, Clone, Copy)]
struct GridPosition(usize);

/// Marca la moto que lleva el jugador.
#[derive(Component)]
pub struct Player;

/// Which bike of the [`ROSTER`] this is (also its rider): who it is in the
/// race's results.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rider(pub usize);

/// A player's bike out of the race (in elimination, or when the race is
/// over): it no longer answers the controls and rolls to a stop.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Retired;

/// A player's bike the machine rides once it has crossed the line at the
/// end of the race, as it would a rival's, so that it does not crash while
/// the others come in (see [`autopilot`]).
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Autopilot;

/// How much of its bike the autopilot uses: an easy pace, slower than any
/// rival, on a lap that no longer counts.
const AUTOPILOT_SKILL: f32 = 0.85;
/// How far from the middle of the road the autopilot keeps at most (m).
const AUTOPILOT_LANE: f32 = 4.0;

/// Hands `bike` over to the machine, riding on from `lateral` (m left of the
/// centre line) in the lane nearest to it, on the lap (no loops).
pub fn autopilot(bike: &mut EntityCommands, lateral: f32) {
    let lane = lateral.clamp(-AUTOPILOT_LANE, AUTOPILOT_LANE);
    bike.insert((
        Autopilot,
        Rival {
            skill: AUTOPILOT_SKILL,
            lane,
            loops: false,
        },
        ai::Line(lane),
    ));
}

/// Marca los jugadores locales en modo multijugador (0 para Player 1, 1 para Player 2).
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocalPlayer(pub usize);

/// Registra el impacto de un choque reciente con otra moto para feedback háptico.
#[derive(Component, Default, Clone, Copy, Debug, PartialEq)]
pub struct CollisionImpact {
    /// Tiempo restante de vibración tras el impacto (s).
    pub timer: f32,
    /// Intensidad normalizada del choque (de 0 a 1).
    pub intensity: f32,
    /// A new hit not yet felt in the controller: how hard (0 to 1) and
    /// whether the rider went down.
    pub unfelt: Option<(f32, bool)>,
}

impl CollisionImpact {
    /// Records a hit of `strength` (0 to 1); `falls` if the rider went down.
    /// Only a new hit is felt: bikes leaning on each other, or on a wall,
    /// would otherwise rumble on every step.
    pub fn record(&mut self, strength: f32, falls: bool) {
        let fresh = self.timer == 0.0;
        let (timer, intensity) = if falls {
            (0.5, 1.0)
        } else {
            (0.25, 0.35 + 0.65 * strength)
        };
        self.timer = self.timer.max(timer);
        self.intensity = if fresh {
            intensity
        } else {
            self.intensity.max(intensity)
        };
        if fresh || falls {
            let (was, fell) = self.unfelt.unwrap_or((0.0, false));
            self.unfelt = Some((was.max(strength), fell || falls));
        }
    }
}

/// Lo que pide el piloto en este momento. Al jugador lo rellena el teclado o
/// el mando; a los rivales, su IA.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct BikeInput {
    /// Acelerador, de 0 a 1.
    pub throttle: f32,
    /// Freno (y marcha atrás si está parada), de 0 a 1.
    pub brake: f32,
    /// Dirección, de -1 (derecha) a 1 (izquierda). La moto la sigue como un
    /// muelle, poco a poco.
    pub steer: f32,
    /// Dirección que entra tal cual, sin el muelle: la del teclado o la
    /// cruceta, que ya llega suavizada (ver `DigitalSteer`). Se suma a la
    /// otra.
    pub steer_direct: f32,
    /// Cuánto tira del manillar para hacer el caballito, de 0 a 1.
    pub wheelie: f32,
    /// How much slower than usual the bike and the rider go over to the
    /// other side when the turn changes sides (0, as always, to 1): from
    /// the keyboard or the d-pad, as Options > Control says (see
    /// [`pause::ReverseRate`]).
    pub pivot_slowing: f32,
    /// The lean the keyboard's curves ask for (a share of all the bike can,
    /// positive to the left), which the bike shows instead of the one the
    /// turn gives: with the keyboard's behaviour on Bézier (Options > Control >
    /// Advanced), while the
    /// keyboard or the d-pad steer (see [`curves`]).
    pub lean: Option<f32>,
    /// Without the front's extra grip braking (see [`BRAKE_GRIP`]): the
    /// rivals', whose steering does not reckon with it (with it they turn
    /// in tighter than they mean to off the brakes), and the players' while
    /// the classic controls steer (as they always have): it goes with the
    /// curves' handling.
    pub plain_grip: bool,
    /// How the lean shown follows `lean`, from the curves of the keys or
    /// the stick steering.
    pub look: curves::LeanLook,
}

/// Estado físico de la moto. Solo cambia en los pasos fijos de la simulación.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BikeState {
    /// Posición sobre el suelo (x, z).
    pub position: Vec2,
    /// Orientación (ver `geometry`). No se acota a [-π, π) para poder
    /// interpolar entre pasos sin saltos.
    pub heading: f32,
    /// Velocidad en m/s; negativa en marcha atrás.
    pub speed: f32,
    /// Dirección ya suavizada, de -1 a 1.
    pub steer: f32,
    /// Lo deprisa que cambia la parte de la dirección que va con muelle (1/s).
    pub steer_rate: f32,
    /// La parte de `steer` que entró directa en el último paso.
    pub steer_direct: f32,
    /// Cuánto se ha metido en la hierba, de 0 a 1: sube mientras la pisa y
    /// baja al volver al asfalto. Dirt counts as grass (see
    /// [`Surface::is_rough`]).
    pub grass: f32,
    /// Inclinación en radianes; positiva hacia la izquierda.
    pub lean: f32,
    /// Altura de referencia del chasis (m): la de las ruedas en el suelo con
    /// la suspensión en reposo. En el aire, las ruedas cuelgan por debajo lo
    /// que se extienda la suspensión.
    pub elevation: f32,
    /// Velocidad vertical (m/s).
    pub climb: f32,
    /// Si las ruedas tocan el suelo.
    pub grounded: bool,
    /// Cabeceo (rad): positivo con el morro hacia arriba. Sigue la pendiente
    /// en el suelo y la trayectoria en el aire.
    pub pitch: f32,
    /// Aceleración hacia delante que se nota en el último paso (m/s²): la del
    /// motor, el freno y las resistencias, sin la de la cuesta (la gravedad
    /// tira igual de la moto que del piloto). Es la que hunde la suspensión y
    /// echa atrás o adelante al piloto.
    pub accel: f32,
    /// Caballito y suspensión.
    pub chassis: Chassis,
    /// Where it is on a vertical loop, while it rides one (see
    /// [`loop_ride`]): then the position, the height and the pitch follow
    /// the running surface.
    pub ride: Option<Ride>,
    /// Whether its lean is going over to the other side (see [`swinging`]).
    pub swinging: bool,
    /// How fast the lean shown changes (rad/s), for the smooth look.
    pub lean_rate: f32,
    /// With the gears on, the gear it is in (0 the first) and how long the
    /// change up it is in still lasts (s).
    pub gear: usize,
    pub shift: f32,
}

impl BikeState {
    pub fn parked_at((position, heading): (Vec2, f32)) -> Self {
        Self {
            position,
            heading,
            speed: 0.0,
            steer: 0.0,
            steer_rate: 0.0,
            steer_direct: 0.0,
            grass: 0.0,
            lean: STOPPED_LEAN,
            elevation: 0.0,
            climb: 0.0,
            grounded: true,
            pitch: 0.0,
            accel: 0.0,
            chassis: Chassis::default(),
            ride: None,
            swinging: false,
            lean_rate: 0.0,
            gear: 0,
            shift: 0.0,
        }
    }

    /// Parada en una posición del circuito, sobre el asfalto.
    pub fn parked_on(track: &Track, pose: (Vec2, f32)) -> Self {
        Self {
            elevation: track.road_height(pose.0),
            ..Self::parked_at(pose)
        }
    }

    /// Like [`Self::parked_on`], on the stretch at about `height` where the
    /// track passes over itself (see [`Track::nearest_at`]).
    pub fn parked_at_level(track: &Track, pose: (Vec2, f32), height: f32) -> Self {
        Self {
            elevation: track.road_height_at(pose.0, height),
            ..Self::parked_at(pose)
        }
    }

    /// Which way the bike points, without its lean: its heading and pitch
    /// and, on a loop, how far it has turned on the running surface, about
    /// its own up (which is down at the top).
    pub fn orientation(&self) -> Quat {
        let yaw = self.ride.map_or(0.0, |ride| ride.yaw);
        Quat::from_rotation_y(self.heading)
            * Quat::from_rotation_x(self.pitch)
            * Quat::from_rotation_y(yaw)
    }
}

/// Lo que hay bajo las ruedas al empezar un paso.
#[derive(Clone, Copy, Debug)]
pub struct Ground {
    pub surface: Surface,
    /// Pendiente en la dirección de la moto: metros que sube por metro.
    pub grade: f32,
    /// Curvatura vertical (1/m): positiva en lo alto de una loma.
    pub crest: f32,
    /// The speed a booster under the bike drives it on to (m/s), or nought
    /// (see [`crate::track::Track::booster_beside`]).
    pub booster: f32,
    /// How much harder than usual the road grips across (see
    /// [`crate::track::Track::grip_beside`]): 1 as usual.
    pub grip: f32,
}

impl Ground {
    #[cfg(any(test, feature = "testing"))]
    pub fn flat(surface: Surface) -> Self {
        Self {
            surface,
            grade: 0.0,
            crest: 0.0,
            booster: 0.0,
            grip: 1.0,
        }
    }
}

/// Estado del paso anterior, para interpolar el render entre dos pasos.
#[derive(Component, Clone, Copy, Debug)]
struct PreviousBikeState(BikeState);

/// Pivote de la inclinación: hijo de la moto, en el punto de contacto con el suelo.
#[derive(Component)]
struct LeanPivot;

/// Parámetros de conducción. Es un recurso para poder ajustarlos en caliente.
#[derive(Resource, Clone, Copy, Debug)]
pub struct BikeTuning {
    /// Punta en asfalto (m/s).
    pub max_speed: f32,
    /// Aceleración del motor desde parado (m/s²); se va apagando cerca de la punta.
    pub engine_accel: f32,
    pub brake_decel: f32,
    pub reverse_accel: f32,
    pub reverse_max_speed: f32,
    /// Deceleración que siempre actúa: rodadura y aire, simplificados.
    pub rolling_resistance: f32,
    /// Deceleración extra al soltar acelerador y freno.
    pub engine_braking: f32,
    /// And this much more per m/s of speed (1/s): the engine brakes harder
    /// the higher it revs, and the air with it, so letting go of the
    /// throttle at speed slows the bike down much more than when slow.
    pub engine_braking_per_speed: f32,
    /// Punta sobre hierba (m/s).
    pub grass_max_speed: f32,
    /// Deceleración extra cuando se va más rápido que la punta del suelo que se pisa.
    pub overspeed_drag: f32,
    /// Radio de giro mínimo (m): limita el giro a poca velocidad.
    pub min_turn_radius: f32,
    /// Aceleración lateral máxima (m/s²): limita el giro a mucha velocidad.
    pub max_lateral_accel: f32,
    /// Rapidez con la que la dirección sigue al mando (1/s): va como un muelle
    /// sin rebote, así entra y sale de las curvas poco a poco y sin tirones.
    pub steer_response: f32,
    /// Inclinación máxima (rad).
    pub max_lean: f32,
    /// Rapidez con la que la moto se tumba al entrar en la curva (1/s): casi
    /// al momento.
    pub lean_in_response: f32,
    /// Rapidez con la que se levanta (1/s): más suave.
    pub lean_response: f32,
    /// Whether the bikes change gears (Options > Game): none while changing
    /// up, and each gear strongest at the revs it pulls best (see
    /// [`gearbox`]). Off, the engine drives as if it had none.
    pub gears: bool,
}

impl Default for BikeTuning {
    fn default() -> Self {
        Self {
            max_speed: 80.0,
            engine_accel: 14.5,
            brake_decel: 28.0,
            reverse_accel: 4.0,
            reverse_max_speed: 5.0,
            rolling_resistance: 0.5,
            engine_braking: 2.5,
            engine_braking_per_speed: 0.065,
            grass_max_speed: 36.0,
            overspeed_drag: 10.0,
            min_turn_radius: 3.5,
            max_lateral_accel: 65.0,
            steer_response: 11.0,
            max_lean: 55f32.to_radians(),
            lean_in_response: 16.0,
            lean_response: 7.0,
            gears: false,
        }
    }
}

impl BikeTuning {
    /// Lo más deprisa que puede girar la moto a esta velocidad (rad/s), con
    /// la dirección a fondo.
    pub fn max_yaw_rate(&self, speed: f32) -> f32 {
        self.max_yaw_rate_with_grip(speed, 1.0)
    }

    /// Igual, con el agarre multiplicado por `grip`.
    fn max_yaw_rate_with_grip(&self, speed: f32, grip: f32) -> f32 {
        let v = speed.abs();
        (v / self.min_turn_radius).min(self.max_lateral_accel * grip / v)
    }
}

/// The bikes change gears as Options > Game says.
fn follow_gears(settings: Res<Settings>, mut tuning: ResMut<BikeTuning>) {
    if tuning.gears != settings.gears {
        tuning.gears = settings.gears;
    }
}

/// The gears after `dt` seconds in gear `gear` (still changing up for
/// `shift` s) at `speed` (m/s) with `throttle`: up near the gear's top
/// with the throttle open (no drive for [`GEAR_SHIFT_TIME`]), down well
/// below the gear under it. Also how much the engine drives there: none
/// while changing up, and in a gear more at the revs it pulls best (see
/// [`TORQUE_BOOST`]), as much as without gears at either end.
pub fn gearbox(gear: usize, shift: f32, speed: f32, throttle: f32, dt: f32) -> (usize, f32, f32) {
    let speed = speed.max(0.0);
    let mut gear = gear.min(GEARS.len() - 1);
    let mut shift = (shift - dt).max(0.0);
    if gear + 1 < GEARS.len() && speed >= GEARS[gear] * GEAR_UP && throttle > 0.0 {
        gear += 1;
        shift = GEAR_SHIFT_TIME;
    }
    while gear > 0 && speed < GEARS[gear - 1] * GEAR_DOWN {
        gear -= 1;
    }
    let revs = speed / GEARS[gear];
    let pull = (1.0 - ((revs - TORQUE_PEAK) / TORQUE_SPREAD).powi(2)).max(0.0);
    let drive = if shift > 0.0 {
        0.0
    } else {
        1.0 + TORQUE_BOOST * pull
    };
    (gear, shift, drive)
}

/// Avanza la moto un paso de `dt` segundos. Es una función pura: no toca el
/// ECS, así se puede probar y reutilizar (repeticiones, IA, red...).
pub fn step(
    state: &BikeState,
    input: &BikeInput,
    ground: &Ground,
    tuning: &BikeTuning,
    dt: f32,
) -> BikeState {
    // En el aire ni el motor ni el freno ni la dirección hacen nada.
    let contact = if state.grounded { 1.0 } else { 0.0 };
    let throttle = input.throttle.clamp(0.0, 1.0) * contact;
    let brake = input.brake.clamp(0.0, 1.0) * contact;
    // La hierba frena poco a poco: la punta baja según lo que se lleva en ella.
    // Dirt does the same; the concrete or planks of a tunnel or a bridge
    // are as firm as the road.
    let on_grass = ground.surface.is_rough() && state.grounded;
    let grass = if on_grass {
        move_toward(state.grass, 1.0, dt / GRASS_BITE_TIME)
    } else {
        move_toward(state.grass, 0.0, 2.0 * dt / GRASS_BITE_TIME)
    };
    let top_speed = tuning
        .max_speed
        .lerp(tuning.grass_max_speed, smoothstep(GRASS_GRACE, 1.0, grass))
        * (1.0 + state.chassis.wheelie_boost());

    // Motor: empuja fuerte desde parado y se queda sin empuje al llegar a la punta.
    let speed_ratio = (state.speed.max(0.0) / top_speed).min(1.0);
    // En curva empuja algo más: tumbarse se premia, no se paga. Solo al
    // apurar, con la dirección a tope, el motor empuja menos y la moto frena
    // (más abajo) para cerrar la curva.
    let steer = state.steer.abs();
    let at_limit = smoothstep(CORNER_LOSS_FROM, 1.0, steer);
    let corner_power = 1.0 + CORNER_DRIVE * (steer / CORNER_GRIP_FROM).min(1.0) * (1.0 - at_limit)
        - CORNER_POWER_CUT * at_limit;
    // Con un caballito empuja algo más.
    // With the gears: none while changing up, and each gear stronger at the
    // revs it pulls best.
    let (gear, shift, drive) = if tuning.gears {
        gearbox(state.gear, state.shift, state.speed, throttle, dt)
    } else {
        (0, 0.0, 1.0)
    };
    let power = throttle
        * (1.0 - speed_ratio * speed_ratio)
        * corner_power
        * drive
        * (1.0 + state.chassis.wheelie_drive());
    let mut accel = power * tuning.engine_accel;
    // Freno: frena mientras avanza; parada (o casi), da marcha atrás.
    if state.speed > 0.5 {
        accel -= brake * tuning.brake_decel;
    } else {
        accel -= brake * tuning.reverse_accel;
    }
    // Cuesta arriba la gravedad frena; cuesta abajo, empuja.
    let slope_sine = ground.grade / (1.0 + ground.grade * ground.grade).sqrt();
    accel -= GRAVITY * slope_sine * contact;
    let mut speed = state.speed + accel * dt;

    // Resistencias: se oponen al movimiento, pero nunca lo invierten.
    // Con un caballito solo una rueda pisa el asfalto, reduciendo la resistencia.
    let mut resistance = tuning.rolling_resistance * (1.0 - 0.35 * state.chassis.wheelie_lift());
    if state.grounded && throttle == 0.0 && brake == 0.0 {
        resistance += tuning.engine_braking + tuning.engine_braking_per_speed * speed.abs();
    }
    if speed.abs() > top_speed {
        resistance += tuning.overspeed_drag;
    }
    resistance += CORNER_SCRUB * at_limit * smoothstep(25.0, 60.0, speed) * contact;
    speed = move_toward(speed, 0.0, resistance * dt);
    // A booster under it drives it on, hard, up to its speed; never slows it.
    if ground.booster > 0.0 && state.grounded {
        speed = speed.max(move_toward(speed, ground.booster, BOOSTER_ACCEL * dt));
    }
    let speed = speed.clamp(-tuning.reverse_max_speed, top_speed.max(tuning.max_speed));
    let accel = (speed - state.speed) / dt + GRAVITY * slope_sine * contact;

    // Giro: a poca velocidad lo limita el radio de giro; a mucha, el agarre.
    // El stick (y la IA) mueven la dirección con un muelle; el teclado entra
    // directo, que ya llega suave.
    let response = tuning.steer_response;
    let spring = state.steer - state.steer_direct;
    let steer_accel = response * response * (input.steer.clamp(-1.0, 1.0) - spring)
        - 2.0 * response * state.steer_rate;
    let steer_rate = state.steer_rate + steer_accel * dt;
    let steer_direct = input.steer_direct.clamp(-1.0, 1.0);
    let steer = (spring + steer_rate * dt + steer_direct).clamp(-1.0, 1.0);
    // Al apurar con la dirección a tope, la moto agarra más y cierra la curva.
    // Más cuanto más deprisa: despacio manda el radio de giro.
    let extra_grip = CORNER_GRIP
        .start
        .lerp(CORNER_GRIP.end, smoothstep(25.0, 60.0, speed.abs()));
    // Braking, and a moment after, the front is loaded and grips more: the
    // bike turns tighter turning in off the brakes.
    let corner_grip = (1.0 + extra_grip * smoothstep(CORNER_GRIP_FROM, 1.0, steer.abs()))
        * if input.plain_grip {
            1.0
        } else {
            1.0 + BRAKE_GRIP * state.chassis.front_load()
        }
        // Banked track that grips harder (the toy cars').
        * ground.grip;
    let max_yaw_rate = tuning.max_yaw_rate_with_grip(speed, corner_grip);
    // Con una rueda en el aire apenas se puede girar.
    let grip = state.chassis.steering_grip();
    let traction = if state.grounded { 1.0 } else { AIR_STEERING };
    let yaw_rate = steer * max_yaw_rate * speed.signum() * traction * grip;
    let heading = state.heading + yaw_rate * dt;

    // Inclinación según cuánto agarre pide la curva: poco giro, poca
    // inclinación, y del todo solo al apurar. (La de equilibrio frente a la
    // gravedad, con un agarre tan alto, tumbaría la moto con muy poco giro.)
    // En parado la moto apoya el pie derecho y se inclina un poco hacia ese lado.
    let stopped = if state.grounded {
        (1.0 - speed.abs() / STOPPED_SPEED).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let grip_used = (speed * yaw_rate / tuning.max_lateral_accel).clamp(-1.0, 1.0);
    // With the keyboard's curves, the lean they ask for, less of it slow
    // (walking pace, the bike turns on the bars rather than leaning).
    let dynamic_lean = match input.lean {
        Some(share) => {
            tuning.max_lean
                * share.clamp(-1.0, 1.0)
                * smoothstep(STOPPED_SPEED, CURVE_LEAN_SPEED, speed.abs())
        }
        None => tuning.max_lean * grip_used.abs().powf(LEAN_CURVE).copysign(grip_used),
    };
    let target_lean = dynamic_lean * (1.0 - stopped) + STOPPED_LEAN * stopped;
    // Se tumba deprisa y se levanta suave. Al cambiar de lado en una curva,
    // también se tumba deprisa hacia el lado contrario.
    let leaning_in = (target_lean.abs() > state.lean.abs() && target_lean * state.lean >= 0.0)
        || target_lean * state.lean < 0.0;
    // Going over to the other side, as fast as the controls say (as always
    // but with the keyboard asking for less).
    let swinging = swinging(state.swinging, state.lean, target_lean);
    let lean_rate = if input.lean.is_some() {
        // The curves already shape it: followed closely.
        CURVE_LEAN_FOLLOW
    } else if leaning_in {
        tuning.lean_in_response
    } else {
        tuning.lean_response
    } * if swinging {
        1.0 - input.pivot_slowing
    } else {
        1.0
    };

    // The lean shown: by the curves, as their look says; else as always.
    let (lean, new_lean_rate) = match (input.lean, input.look) {
        (Some(_), curves::LeanLook::Soft) => {
            (approach(state.lean, target_lean, SOFT_LEAN_FOLLOW, dt), 0.0)
        }
        (Some(_), curves::LeanLook::Smooth) => {
            let spring = SMOOTH_LEAN_SPRING;
            let accel =
                spring * spring * (target_lean - state.lean) - 2.0 * spring * state.lean_rate;
            let rate = state.lean_rate + accel * dt;
            (state.lean + rate * dt, rate)
        }
        _ => (approach(state.lean, target_lean, lean_rate, dt), 0.0),
    };

    let target_pitch = if state.grounded {
        ground.grade.atan()
    } else {
        state.climb.atan2(speed.abs().max(1.0))
    };

    let chassis = state.chassis.step(
        &Forces {
            pull: input.wheelie.clamp(0.0, 1.0),
            brake,
            throttle,
            power,
            speed,
            accel,
            // En los valles la carretera empuja hacia arriba y comprime la suspensión.
            lift: -ground.crest * speed * speed * contact,
            grounded: state.grounded,
        },
        dt,
    );

    BikeState {
        position: state.position + forward(heading) * speed * dt,
        heading,
        speed,
        steer,
        steer_rate,
        steer_direct,
        grass,
        lean,
        lean_rate: new_lean_rate,
        swinging,
        gear,
        shift,
        pitch: approach(state.pitch, target_pitch, PITCH_RESPONSE, dt),
        accel,
        chassis,
        ..*state
    }
}

/// Whether something leaning `now` (positive one way, negative the other)
/// towards `target` is going over to the other side, if it `was`: from when
/// the target goes over, while it is still on the side it leaves and on its
/// way out on the new one, until it is nearly there or the target comes
/// back towards the middle.
pub fn swinging(was: bool, now: f32, target: f32) -> bool {
    now * target < 0.0 || (was && now * target > 0.0 && now.abs() < 0.9 * target.abs())
}

/// Pega la moto al suelo o la deja volar. Sobre el suelo sigue su altura; si
/// en un cambio de rasante haría falta más fuerza hacia abajo de la que da la
/// gravedad para seguir la carretera, despega, y aterriza al volver a tocarla.
/// `ground` es lo que había bajo las ruedas al empezar el paso y
/// `ground_height`, la altura del suelo donde ha acabado.
pub fn settle(state: &mut BikeState, ground: &Ground, ground_height: f32, dt: f32) {
    let road_climb = ground.grade * state.speed;
    if state.grounded {
        // Se compara con la gravedad con la que volaría: así no da saltitos
        // que no llegan a nada.
        let gravity = if road_climb > 0.0 {
            RISE_GRAVITY
        } else {
            FALL_GRAVITY
        };
        if ground.crest * state.speed * state.speed > gravity {
            state.grounded = false;
            state.climb = road_climb;
            // Despega el chasis: las ruedas se quedan donde estaban y, a partir
            // de aquí, cuelgan lo que se extienda la suspensión.
            state.elevation = ground_height + state.chassis.heave;
        } else {
            state.climb = road_climb;
            state.elevation = ground_height;
            return;
        }
    }
    let gravity = if state.climb > 0.0 {
        RISE_GRAVITY
    } else {
        FALL_GRAVITY
    };
    state.elevation += state.climb * dt - 0.5 * gravity * dt * dt;
    state.climb -= gravity * dt;
    let wheels = state.elevation - state.chassis.heave;
    if wheels <= ground_height {
        // El golpe del aterrizaje lo absorbe la suspensión.
        state.chassis.land(state.climb - road_climb);
        state.elevation = ground_height;
        state.climb = road_climb;
        state.grounded = true;
    }
}

/// Un paso completo de una moto en el circuito: conducción, muros y suelo.
#[cfg(any(test, feature = "testing"))]
pub fn advance(
    state: &BikeState,
    input: &BikeInput,
    track: &Track,
    tuning: &BikeTuning,
    dt: f32,
) -> BikeState {
    if let Some(ride) = state.ride {
        return loop_ride::ride(state, input, &track.loops()[ride.index], tuning, dt).0;
    }
    advance_hitting_walls(state, input, track, tuning, dt).0
}

/// Like [`advance`], also telling whether the bike hit a wall in this step.
pub fn advance_hitting_walls(
    state: &BikeState,
    input: &BikeInput,
    track: &Track,
    tuning: &BikeTuning,
    dt: f32,
) -> (BikeState, Option<WallHit>) {
    let nearest = track.nearest_at(state.position, state.elevation);
    let ground = Ground {
        surface: track.surface_beside(&nearest),
        grade: track.grade(state.position, forward(state.heading), state.elevation),
        crest: track.crest(&nearest),
        booster: track.booster_beside(&nearest),
        grip: track.grip_beside(&nearest),
    };
    let mut next = step(state, input, &ground, tuning, dt);
    let wall = hit_walls(&mut next, track);
    let ground_height = track.road_height_at(next.position, next.elevation);
    settle(&mut next, &ground, ground_height, dt);
    loop_ride::enter(&mut next, track);
    (next, wall)
}

/// Speed into a wall below which hitting it is not felt in the controller
/// (m/s): scraping along it.
const WALL_FELT: f32 = 2.0;

/// Half the distance between the circles at the two ends of a bike that the
/// walls see (m); each is [`BIKE_HALF_WIDTH`] wide, so the bike reaches
/// about a metre ahead and behind its centre, like the model.
const WALL_REACH: f32 = 0.6;
/// How far to the side the rider's shoulders and helmet go when the bike
/// leans (m, times the sine of the lean: the helmet is over a metre up, and
/// the rider hangs off), and how wide they are (m). Fallen, the top of the
/// bike lying on its side reaches about as far.
const LEAN_REACH: f32 = 1.05;
const LEAN_RADIUS: f32 = 0.25;
/// The handlebar and fairing at the front and the tail at the back stick out
/// to the side the bike leans (m ahead, m out times the sine of the lean, and
/// radius): once fallen they are what touches a wall first.
const BARS: (f32, f32, f32) = (0.55, 0.75, 0.3);
const TAIL: (f32, f32, f32) = (-0.55, 0.7, 0.3);

/// The parts of a bike that can touch a wall, seen from above, as circles
/// (centre and radius): its two ends, the rider leaning out to one side (or
/// the top of the bike, once it lies on that side), and the handlebar and
/// tail, which also lean out with it.
pub fn footprint(state: &BikeState) -> [(Vec2, f32); 5] {
    let ahead = forward(state.heading);
    let out = -ahead.perp() * state.lean.sin();
    let leaning = |(along, side, radius): (f32, f32, f32)| {
        (state.position + ahead * along + out * side, radius)
    };
    [
        (state.position + ahead * WALL_REACH, BIKE_HALF_WIDTH),
        (state.position - ahead * WALL_REACH, BIKE_HALF_WIDTH),
        leaning((0.0, LEAN_REACH, LEAN_RADIUS)),
        leaning(BARS),
        leaning(TAIL),
    ]
}

/// The part of `parts` (circles of something centred at `center`, at
/// `height`) that goes deepest into the edge of the track (a wall, or the
/// earth where it rises before it: see [`Track::edge_beside`]), if any: the
/// normal out of the track and how deep it is (m).
pub fn wall_contact(
    track: &Track,
    center: Vec2,
    height: f32,
    parts: &[(Vec2, f32)],
) -> Option<(Vec2, f32)> {
    // Far from the edges, one look at the centre is enough.
    let reach = parts
        .iter()
        .map(|&(point, radius)| point.distance(center) + radius)
        .fold(0.0, f32::max);
    let nearest = track.nearest_at(center, height);
    let spread = (reach / track.step_length()).ceil() as usize + 1;
    if nearest.distance + reach <= track.closest_edge_near(&nearest, spread) {
        return None;
    }
    parts
        .iter()
        .filter_map(|&(point, radius)| {
            let nearest = track.nearest_at(point, height);
            let depth = nearest.distance + radius - track.edge_beside(&nearest);
            let out = (point - nearest.point).try_normalize()?;
            (depth > 0.0).then_some((out, depth))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// A bike hitting a wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallHit {
    /// Out of the track, perpendicular to the wall.
    pub normal: Vec2,
    /// Velocity before the hit (m/s).
    pub before: Vec2,
    /// Speed into the wall (m/s).
    pub into: f32,
    /// Where the bike touched it, from its centre.
    pub at: Vec2,
}

/// Mantiene la moto entre los muros que bordean la carretera. Si se mete en
/// uno, la saca y le quita velocidad según lo de frente que haya sido el golpe
/// (rozarlo apenas frena, darse de cara la frena mucho) y la pone paralela al
/// muro, para que siga en vez de quedarse clavada. No part of the bike (see
/// [`footprint`]) goes into the wall: not its front wheel when it hits at an
/// angle, nor the rider leaning towards it. Returns the hit, if there was one.
pub fn hit_walls(state: &mut BikeState, track: &Track) -> Option<WallHit> {
    let parts = footprint(state);
    let (normal, depth) = wall_contact(track, state.position, state.elevation, &parts)?;
    let at = parts
        .iter()
        .map(|&(point, radius)| point - state.position + normal * radius)
        .max_by(|a, b| a.dot(normal).total_cmp(&b.dot(normal)))
        .unwrap_or(Vec2::ZERO);
    state.position -= normal * depth;
    let before = forward(state.heading) * state.speed;
    let moving = forward(state.heading) * state.speed.signum();
    // Se pierde la velocidad contra el muro, no la que va a lo largo.
    let impact = moving.dot(normal).max(0.0);
    state.speed *= 1.0 - IMPACT_SPEED_LOSS * impact * impact;
    if state.speed > 0.0 {
        let along = Vec2::new(-normal.y, normal.x);
        let along = if along.dot(moving) >= 0.0 {
            along
        } else {
            -along
        };
        state.heading += wrap_angle(heading_of(along) - state.heading) * WALL_ALIGN;
    }
    Some(WallHit {
        normal,
        before,
        into: before.dot(normal).max(0.0),
        at,
    })
}

fn move_toward(value: f32, target: f32, max_delta: f32) -> f32 {
    value + (target - value).clamp(-max_delta, max_delta)
}

/// Una moto que se puede elegir: su diseño, su decoración (solo cuenta en las
/// hechas por código) y cómo se presenta en la pantalla de inicio. Read from
/// the roster's file (see [`Roster`]).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    #[serde(deserialize_with = "crate::circuits::data::text")]
    pub name: &'static str,
    #[serde(deserialize_with = "crate::circuits::data::text")]
    pub description: &'static str,
    #[serde(deserialize_with = "crate::circuits::data::text")]
    pub description_en: &'static str,
    /// Color de su nombre en la pantalla de inicio.
    #[serde(deserialize_with = "crate::circuits::data::srgba")]
    pub color: Srgba,
    design: Design,
    /// Its livery, by name (see [`livery`]).
    #[serde(deserialize_with = "livery")]
    livery: &'static Livery,
}

impl Entry {
    /// Its design: built in code, or a model.
    pub fn design(&self) -> model::Design {
        self.design
    }

    pub fn description(&self, lang: Language) -> &'static str {
        match lang {
            Language::Spanish => self.description,
            Language::English => self.description_en,
        }
    }
}

/// A livery the roster's file names: those painted in code, the engine's
/// own (see `src/bike/model/livery.rs`).
fn livery<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<&'static Livery, D::Error> {
    use serde::{Deserialize, de::Error};
    let name = String::deserialize(deserializer)?;
    Ok(match name.as_str() {
        "cover" => &COVER,
        "flecha" => &FLECHA,
        "diagonal" => &DIAGONAL,
        "verde" => &VERDE,
        "tricolor" => &TRICOLOR,
        "rosso" => &ROSSO,
        _ => return Err(D::Error::custom(format!("no livery called `{name}`"))),
    })
}

/// How many ride a race: the places on the grid ([`GRID`]), as many as the
/// bikes of the roster.
pub const RIDERS: usize = GRID.len();

/// The bikes' roster, in the content pack.
const ROSTER_FILE: &str = "bikes/roster.ron";

/// Las motos de la carrera, read from `assets/bikes/roster.ron` once, when
/// the game starts ([`Roster::load`]). La que elige el jugador sale desde su
/// puesto y las demás ocupan, por orden, los de los rivales: one for each
/// place on the grid ([`RIDERS`]).
#[derive(Resource, Clone, Copy)]
pub struct Roster(&'static [Entry]);

impl Roster {
    /// The bikes, read from their file in the content pack (see
    /// [`crate::circuits::data`] and [`crate::content`]).
    pub fn load() -> Self {
        Self::parse(ROSTER_FILE, crate::content::text(ROSTER_FILE))
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The bikes a roster's file, `name` (for the messages), holding `text`
    /// says, one for each place on the grid; or what is wrong.
    pub fn parse(name: &str, text: &'static str) -> Result<Self, String> {
        let entries: Vec<Entry> = crate::circuits::data::parse(name, text)?;
        if entries.len() != RIDERS {
            return Err(format!(
                "{name}: {} bikes, not one for each of the {RIDERS} places on the grid",
                entries.len()
            ));
        }
        Ok(Roster(Vec::leak(entries)))
    }
}

impl Default for Roster {
    fn default() -> Self {
        Self::load()
    }
}

impl std::ops::Deref for Roster {
    type Target = [Entry];

    fn deref(&self) -> &[Entry] {
        self.0
    }
}

/// Quién sale de cada puesto de la parrilla ([`Track::grid_slot`]), de la
/// pole hacia atrás: los rivales, del más hábil al menos, y el jugador el
/// último, para tener a quién adelantar. For each rival, its skill, its lane
/// in the race and whether it takes the branches with a loop.
const GRID: [Option<Rival>; 8] = [
    Some(Rival {
        skill: 0.95,
        lane: 2.0,
        loops: false,
    }),
    Some(Rival {
        skill: 0.94,
        lane: -2.0,
        loops: true,
    }),
    Some(Rival {
        skill: 0.92,
        lane: 4.0,
        loops: false,
    }),
    Some(Rival {
        skill: 0.91,
        lane: -4.0,
        loops: false,
    }),
    Some(Rival {
        skill: 0.9,
        lane: 0.0,
        loops: false,
    }),
    Some(Rival {
        skill: 0.89,
        lane: 1.0,
        loops: false,
    }),
    Some(Rival {
        skill: 0.88,
        lane: -1.0,
        loops: true,
    }),
    None,
];

/// El puesto del jugador en [`GRID`].
const PLAYER_SLOT: usize = GRID.len() - 1;

/// Qué moto de [`ROSTER`] va en cada puesto de [`GRID`] si el jugador elige
/// `choice`: la suya en el del jugador y las demás, por orden, en los de los
/// rivales.
fn lineup(choice: usize) -> [usize; GRID.len()] {
    let mut lineup = [choice; GRID.len()];
    let mut others = (0..RIDERS).filter(|&entry| entry != choice);
    for (slot, entry) in lineup.iter_mut().enumerate() {
        if slot != PLAYER_SLOT {
            *entry = others.next().expect("hay una moto para cada rival");
        }
    }
    lineup
}

/// En pantalla dividida: P1 (`p1_choice`) en PLAYER_SLOT, P2 (`p2_choice`) en PLAYER_SLOT - 1,
/// y los rivales en el resto.
fn split_lineup(p1_choice: usize, p2_choice: usize) -> [usize; GRID.len()] {
    let mut lineup = [0; GRID.len()];
    lineup[PLAYER_SLOT] = p1_choice;
    lineup[PLAYER_SLOT - 1] = p2_choice;
    let mut others = (0..RIDERS).filter(|&entry| entry != p1_choice && entry != p2_choice);
    for (slot, entry) in lineup.iter_mut().enumerate() {
        if slot != PLAYER_SLOT && slot != PLAYER_SLOT - 1 {
            *entry = others.next().unwrap_or(slot % RIDERS);
        }
    }
    lineup
}

/// El color de una moto, el de su nombre en [`ROSTER`]: el de su punto en el
/// minimapa.
#[derive(Component, Clone, Copy)]
pub struct BikeColor(pub Color);

/// Lo que hace falta para crear motos: las mallas compartidas y dónde guardar
/// las de cada moto.
#[derive(SystemParam)]
pub struct BikeAssets<'w> {
    blueprint: Res<'w, BikeBlueprint>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    images: ResMut<'w, Assets<Image>>,
}

impl BikeAssets<'_> {
    /// Crea una moto de [`ROSTER`] parada en `state`, con su piloto.
    pub fn spawn(&mut self, commands: &mut Commands, entry: &Entry, state: BikeState) -> Entity {
        let bike = commands
            .spawn((
                state,
                PreviousBikeState(state),
                BikeInput::default(),
                BikeColor(entry.color.into()),
                GrassKickup::default(),
                Knock::default(),
                CrashState::default(),
                Transform::from_translation(to_world(state.position, SURFACE_Y))
                    .with_rotation(Quat::from_rotation_y(state.heading)),
                Visibility::default(),
            ))
            .id();
        model::spawn(
            commands,
            bike,
            &self.blueprint,
            entry.design(),
            entry.livery,
            &mut self.meshes,
            &mut self.materials,
            &mut self.images,
        );
        bike
    }
}

/// Where the bike of grid slot `slot` starts: on the grid, or, asked for
/// it while developing, that slot moved along the lap (see
/// [`crate::debug_start`]).
fn start_pose(track: &Track, slot: usize) -> (Vec2, f32) {
    match crate::debug_start::along() {
        Some(along) => track.grid_slot_at(slot, along),
        None => track.grid_slot(slot),
    }
}

/// Al empezar la carrera: la parrilla con la moto elegida para el jugador.
/// Learning the keyboard's curves in free ride, the player alone on it
/// (see [`learning`]).
#[allow(clippy::too_many_arguments)]
fn spawn_grid(
    mut commands: Commands,
    roster: Res<Roster>,
    track: Res<Track>,
    choice: Res<Choice>,
    split_choices: Option<Res<SplitChoices>>,
    mut race: ResMut<Race>,
    mut assets: BikeAssets,
    game_mode: Res<GameMode>,
    settings: Res<Settings>,
    rules: Option<Res<crate::race::Rules>>,
) {
    race.started = false;
    let alone = learning::learning_line(&settings, rules.as_deref(), *game_mode).is_some();
    let roster_indices = if *game_mode == GameMode::SplitScreen {
        if let Some(split) = split_choices {
            split_lineup(split.p1, split.p2)
        } else {
            split_lineup(0, 1)
        }
    } else {
        lineup(choice.0)
    };
    for (slot, (rival, entry)) in GRID.iter().zip(roster_indices).enumerate() {
        if alone && rival.is_some() {
            continue;
        }
        let state = BikeState::parked_on(&track, start_pose(&track, slot));
        let bike = assets.spawn(&mut commands, &roster[entry], state);
        let mut bike = commands.entity(bike);
        bike.insert((
            GridPosition(slot),
            Rider(entry),
            DespawnOnExit(GameState::Race),
            CollisionImpact::default(),
        ));
        if *game_mode == GameMode::SplitScreen && slot == PLAYER_SLOT - 1 {
            bike.insert(LocalPlayer(1));
        } else {
            match *rival {
                Some(rival) => {
                    // As fast as the difficulty asks. It sets off on its
                    // slot's line, and goes over to its lane once there is
                    // room beside it.
                    let skill = settings.difficulty.rival_skill(rival.skill);
                    let line = ai::Line(Track::grid_lane(slot));
                    bike.insert((Rival { skill, ..rival }, line));
                }
                None => {
                    bike.insert(Player);
                    if *game_mode == GameMode::SplitScreen {
                        bike.insert(LocalPlayer(0));
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn read_controller_binding(
    binding: ControllerBinding,
    keyboard: &ButtonInput<KeyCode>,
    gamepads: &Query<(Entity, &Gamepad)>,
    options: &DebugOptions,
    curves: SteerCurves,
    speed: f32,
    dt: f32,
    digital: &mut KeySteer,
    analog: &mut StickSteer,
    arming: &mut PadArming,
) -> BikeInput {
    match binding {
        ControllerBinding::Wasd => {
            let throttle = if keyboard.pressed(KeyCode::KeyW) {
                1.0
            } else {
                0.0
            };
            let brake = if keyboard.pressed(KeyCode::KeyS) {
                1.0
            } else {
                0.0
            };
            let left = if keyboard.pressed(KeyCode::KeyA) {
                1.0f32
            } else {
                0.0f32
            };
            let right = if keyboard.pressed(KeyCode::KeyD) {
                1.0f32
            } else {
                0.0f32
            };
            let wheelie = if keyboard.pressed(KeyCode::Space) {
                1.0
            } else {
                0.0
            };
            let keys = digital.update(left - right, speed, dt, options, curves.keys());
            BikeInput {
                throttle,
                brake,
                steer: 0.0,
                steer_direct: keys.steer,
                wheelie,
                pivot_slowing: keys.pivot_slowing,
                lean: keys.lean,
                // The front's extra grip braking is the curves' handling's.
                plain_grip: keys.lean.is_none(),
                look: keys.look,
            }
        }
        ControllerBinding::Arrows => {
            let throttle = if keyboard.pressed(KeyCode::ArrowUp) {
                1.0
            } else {
                0.0
            };
            let brake = if keyboard.pressed(KeyCode::ArrowDown) {
                1.0
            } else {
                0.0
            };
            let left = if keyboard.pressed(KeyCode::ArrowLeft) {
                1.0f32
            } else {
                0.0f32
            };
            let right = if keyboard.pressed(KeyCode::ArrowRight) {
                1.0f32
            } else {
                0.0f32
            };
            let wheelie = if keyboard.any_pressed([
                KeyCode::ControlRight,
                KeyCode::ShiftRight,
                KeyCode::Numpad0,
                KeyCode::KeyL,
            ]) {
                1.0
            } else {
                0.0
            };
            let keys = digital.update(left - right, speed, dt, options, curves.keys());
            BikeInput {
                throttle,
                brake,
                steer: 0.0,
                steer_direct: keys.steer,
                wheelie,
                pivot_slowing: keys.pivot_slowing,
                lean: keys.lean,
                // The front's extra grip braking is the curves' handling's.
                plain_grip: keys.lean.is_none(),
                look: keys.look,
            }
        }
        ControllerBinding::Gamepad(entity) => {
            if let Ok((_, gamepad)) = gamepads.get(entity) {
                let trigger_r = gamepad.get(GamepadButton::RightTrigger2).unwrap_or(0.0);
                let trigger_l = gamepad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0);
                let south = if gamepad.pressed(GamepadButton::South) {
                    1.0
                } else {
                    0.0
                };
                let west = if gamepad.pressed(GamepadButton::West) {
                    1.0
                } else {
                    0.0
                };
                let throttle = arming.filter(entity, THROTTLE, squeezed(trigger_r.max(south)));
                let brake = arming.filter(entity, BRAKE, squeezed(trigger_l.max(west)));

                let stick = gamepad.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
                let classic = -stick_steer(stick, options.stick_curve);
                let stick = analog.update(
                    classic,
                    -stick_travel(stick),
                    speed,
                    dt,
                    options,
                    curves.stick(),
                );

                let dpad_left = if gamepad.pressed(GamepadButton::DPadLeft) {
                    1.0f32
                } else {
                    0.0f32
                };
                let dpad_right = if gamepad.pressed(GamepadButton::DPadRight) {
                    1.0f32
                } else {
                    0.0f32
                };
                let keys =
                    digital.update(dpad_left - dpad_right, speed, dt, options, curves.keys());

                // B, or the trigger and A at once. Not the stick pulled back:
                // steering, it is easy to pull it a little without meaning to.
                let wheelie =
                    if gamepad.pressed(GamepadButton::East) || (trigger_r > 0.2 && south > 0.5) {
                        1.0
                    } else {
                        0.0
                    };

                BikeInput {
                    throttle,
                    brake,
                    steer: stick.steer.clamp(-1.0, 1.0),
                    steer_direct: (keys.steer + stick.direct).clamp(-1.0, 1.0),
                    wheelie,
                    pivot_slowing: keys.pivot_slowing,
                    lean: keys.lean.or(stick.lean),
                    plain_grip: keys.lean.is_none() && stick.lean.is_none(),
                    look: if keys.lean.is_some() {
                        keys.look
                    } else {
                        stick.look
                    },
                }
            } else {
                BikeInput::default()
            }
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn read_player_input(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    settings: Res<Settings>,
    game_mode: Res<GameMode>,
    local_players: Res<LocalPlayersConfig>,
    mut active_controller: Option<ResMut<crate::gamepads::ActiveController>>,
    mut race: ResMut<Race>,
    options: Res<DebugOptions>,
    player: Single<(&mut BikeInput, &BikeState, Has<Autopilot>), With<Player>>,
    mut other_players: Query<
        (&mut BikeInput, &BikeState, &LocalPlayer),
        (Without<Player>, Without<Autopilot>),
    >,
    mut digital: Local<[KeySteer; 2]>,
    mut analog: Local<[StickSteer; 2]>,
    mut arming: Local<PadArming>,
    names: Query<&Name, With<Gamepad>>,
    (tuning, mut probe): (Res<BikeTuning>, ResMut<CurveProbe>),
) {
    let (input, state, autopilot) = player.into_inner();
    // The keyboard's and the stick's curves, when they are on, and the top
    // speed their limit with speed is measured against.
    let (keys_shaped, stick_shaped) = (
        settings.keyboard_shapes.curves(),
        settings.stick_shapes.curves(),
    );
    let curves = SteerCurves {
        keys: (settings.keyboard_mode == KeyboardMode::Bezier).then_some(&keys_shaped),
        stick: (settings.stick_mode == KeyboardMode::Bezier).then_some(&stick_shaped),
        top_speed: tuning.max_speed,
    };
    // On autopilot the machine rides the bike: what the player presses is
    // read (to keep the steering's state) but not used.
    let mut ignored = BikeInput::default();
    let input: &mut BikeInput = if autopilot {
        &mut ignored
    } else {
        input.into_inner()
    };
    let dt = time.delta_secs();
    arming.track(race.started);
    let [digital_p1, digital_p2] = &mut *digital;
    let [analog_p1, analog_p2] = &mut *analog;

    if *game_mode == GameMode::SplitScreen {
        if let Some(binding) = local_players.p1 {
            *input = read_controller_binding(
                binding,
                &keyboard,
                &gamepads,
                &options,
                curves,
                state.speed,
                dt,
                digital_p1,
                analog_p1,
                &mut arming,
            );
        }
        for (mut p2_input, p2_state, local_player) in &mut other_players {
            if let (1, Some(binding)) = (local_player.0, local_players.p2) {
                *p2_input = read_controller_binding(
                    binding,
                    &keyboard,
                    &gamepads,
                    &options,
                    curves,
                    p2_state.speed,
                    dt,
                    digital_p2,
                    analog_p2,
                    &mut arming,
                );
            }
        }
    } else {
        // Modo 1 jugador normal
        let controls = &settings.controls;
        let key = |action| controls.key_value(action, &keyboard);
        let mut throttle = key(Action::Throttle);
        let mut brake = key(Action::Brake);
        let mut pressed = key(Action::Left) - key(Action::Right);
        let mut steer = 0.0;
        // How far the sticks are pushed past their dead zone, to the left.
        let mut travel = 0.0;
        let mut wheelie = key(Action::Wheelie);

        let mut active_pad_entity: Option<Entity> = None;
        // What is pressing the throttle, to tell in the log what starts the race.
        let mut throttle_from = (throttle > 0.0).then(|| "teclado".to_string());

        for (entity, gamepad) in &gamepads {
            let button = |action| controls.button_value(action, gamepad);
            let p_throttle = arming.filter(entity, THROTTLE, squeezed(button(Action::Throttle)));
            let p_brake = arming.filter(entity, BRAKE, squeezed(button(Action::Brake)));
            let stick = gamepad.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
            let p_steer = stick_steer(stick, options.stick_curve);
            travel -= stick_travel(stick);
            let digital = |action| if button(action) > 0.5 { 1.0f32 } else { 0.0f32 };
            let p_pressed: f32 = digital(Action::Left) - digital(Action::Right);
            let trigger_r = gamepad
                .get(GamepadButton::RightTrigger2)
                .unwrap_or(0.0)
                .max(gamepad.get(GamepadButton::RightTrigger).unwrap_or(0.0));
            let south = if gamepad.pressed(GamepadButton::South) {
                1.0
            } else {
                0.0
            };
            let both_throttles: f32 = if trigger_r > 0.2 && south > 0.5 {
                1.0
            } else {
                0.0
            };
            // Not the stick pulled back: steering, it is easy to pull it a
            // little without meaning to.
            let p_wheelie = both_throttles.max(button(Action::Wheelie));

            let has_activity = p_throttle > 0.05
                || p_brake > 0.05
                || p_steer.abs() > 0.05
                || p_pressed.abs() > 0.1
                || p_wheelie > 0.1
                || gamepad.get_just_pressed().next().is_some();

            if has_activity {
                active_pad_entity = Some(entity);
            }

            if p_throttle > throttle {
                let name = names.get(entity).map_or("mando", |name| name.as_str());
                throttle_from = Some(format!(
                    "{name}: RT {:.2}, A {:.2}",
                    gamepad.get(GamepadButton::RightTrigger2).unwrap_or(0.0),
                    gamepad.get(GamepadButton::South).unwrap_or(0.0),
                ));
            }
            throttle = throttle.max(p_throttle);
            brake = brake.max(p_brake);
            steer -= p_steer;
            pressed += p_pressed;
            wheelie = wheelie.max(p_wheelie);
        }

        if let Some(ac) = active_controller.as_mut() {
            if let Some(pad_ent) = active_pad_entity {
                ac.gamepad = Some(pad_ent);
            } else if keyboard.get_pressed().next().is_some() {
                ac.gamepad = None;
            } else if ac.gamepad.is_none() && gamepads.iter().len() == 1 {
                ac.gamepad = gamepads.iter().next().map(|(e, _)| e);
            }
        }

        let stick = analog_p1.update(steer, travel, state.speed, dt, &options, curves.stick());
        let keys = digital_p1.update(pressed, state.speed, dt, &options, curves.keys());

        *input = BikeInput {
            throttle,
            brake,
            steer: stick.steer.clamp(-1.0, 1.0),
            steer_direct: (keys.steer + stick.direct).clamp(-1.0, 1.0),
            wheelie,
            pivot_slowing: keys.pivot_slowing,
            lean: keys.lean.or(stick.lean),
            // The front's extra grip braking is the curves' handling's: in
            // the classic one the bike turns as it always has.
            plain_grip: keys.lean.is_none() && stick.lean.is_none(),
            look: if keys.lean.is_some() {
                keys.look
            } else {
                stick.look
            },
        };
        if throttle > 0.0 && !race.started && !race.lights {
            info!(
                "Empieza la carrera: acelerador {throttle:.2} ({})",
                throttle_from.as_deref().unwrap_or("?")
            );
        }
    }

    // Where the first player's bike is on the curves, for the panel that
    // draws them: the stick's while it leans the bike, else the keyboard's
    // if it steers by them, else the stick's if it does.
    // Whichever leans the bike; upright, the device last used (a gamepad
    // in use: its stick), so that the panel and the learning do not
    // flick to the other's while the stick rests in the middle.
    let stick_leans = analog_p1.curves.lean != 0.0 || analog_p1.travel != 0.0;
    let keys_lean = digital_p1.curves.lean != 0.0;
    let pad_in_use = active_controller
        .as_ref()
        .is_some_and(|controller| controller.gamepad.is_some());
    let on = match (curves.keys, curves.stick) {
        (_, Some(stick)) if stick_leans => Some((stick, true)),
        (Some(keys), _) if keys_lean => Some((keys, false)),
        (_, Some(stick)) if pad_in_use => Some((stick, true)),
        (Some(keys), _) => Some((keys, false)),
        (None, Some(stick)) => Some((stick, true)),
        (None, None) => None,
    };
    let next = match on {
        Some((shaped, stick)) => {
            let speed_share = (state.speed / curves.top_speed).abs();
            let lean = if stick {
                analog_p1.curves
            } else {
                digital_p1.curves
            };
            let travel = if stick { analog_p1.travel.abs() } else { 0.0 };
            CurveProbe {
                live: true,
                stick,
                travel,
                asked: if stick {
                    shaped.lean_asked(travel)
                } else {
                    1.0
                },
                wanted: if stick {
                    shaped.turn_at(shaped.lean_asked(travel), speed_share)
                } else if matches!(
                    digital_p1.curves.phase,
                    curves::Phase::LeaningIn | curves::Phase::Unwinding
                ) {
                    // A key held: all the turn.
                    shaped.turn_at(1.0, speed_share)
                } else {
                    0.0
                },
                lean,
                speed_share,
                limit: shaped.lean_limit(speed_share),
                turn: shaped.turn_at(lean.lean, speed_share),
                turn_limit: shaped.turn_limit(speed_share),
            }
        }
        None => CurveProbe::default(),
    };
    probe.set_if_neq(next);

    let p2_throttles = other_players.iter().any(|(inp, _, _)| inp.throttle > 0.0);
    if (input.throttle > 0.0 || p2_throttles) && !race.started && !race.lights {
        race.started = true;
    }
}

/// Lo más que gira el teclado manteniéndolo pulsado yendo a `speed` (m/s).
fn digital_reach(speed: f32) -> f32 {
    let (slow, fast) = DIGITAL_STEER_SPEEDS;
    1.0 - (1.0 - DIGITAL_STEER_FAST) * smoothstep(slow, fast, speed.abs())
}

/// Lo más que se gira yendo a `speed` (m/s) con este límite de depuración:
/// todo, sin límite; lo del teclado de siempre, con el alto.
fn reach_with(limit: TurnLimit, speed: f32) -> f32 {
    1.0 - (1.0 - digital_reach(speed)) * limit.strength()
}

/// Dirección del teclado o la cruceta: su valor (de -1 a 1), lo que se pulsa
/// y cuánto tiempo lleva así.
#[derive(Default)]
struct DigitalSteer {
    value: f32,
    pressed: f32,
    time: f32,
}

impl DigitalSteer {
    /// Avanza `dt` segundos con `pressed` pulsado (-1, 0 o 1), girando como
    /// mucho `reach` (de 0 a 1) y con una rapidez multiplicada por `rate_factor`.
    /// Al soltar (sin pulsar), vuelve al centro con rapidez multiplicada por `return_factor`.
    fn update(
        &mut self,
        pressed: f32,
        reach: f32,
        rate_factor: f32,
        return_factor: f32,
        dt: f32,
    ) -> f32 {
        if pressed != self.pressed {
            // Si se pulsa hacia el otro lado estando inclinado o recuperando,
            // aborta la recuperación para empezar de inmediato hacia el nuevo
            // lado (la misma respuesta que pulsando desde el centro).
            if pressed != 0.0 && self.value * pressed < 0.0 {
                self.value = 0.0;
            }
            (self.pressed, self.time) = (pressed, 0.0);
        }
        // En pasos de un milisegundo: igual a cualquier tasa de fotogramas.
        let steps = (dt * 1000.0).ceil().max(1.0);
        let dt = dt / steps;
        for _ in 0..steps as usize {
            if pressed != 0.0 && self.value * pressed < 0.0 {
                self.value = 0.0;
            }
            let returning = pressed == 0.0;
            let (start, accel, target) = if returning {
                (
                    DIGITAL_RELEASE_START * return_factor,
                    DIGITAL_RELEASE_ACCEL * return_factor,
                    0.0,
                )
            } else {
                (
                    DIGITAL_STEER_START * rate_factor,
                    DIGITAL_STEER_ACCEL * rate_factor,
                    pressed * reach,
                )
            };
            let rate = start + accel * (self.time + 0.5 * dt);
            self.value = move_toward(self.value, target, rate * dt);
            self.time += dt;
        }
        self.value
    }
}

/// How the keyboard or the d-pad steer a bike: as always (see
/// [`DigitalSteer`]), or leaning it over by the keyboard's curves when they
/// are on (see [`curves`]).
#[derive(Default)]
struct KeySteer {
    digital: DigitalSteer,
    curves: CurveLean,
}

/// What the keys ask of the bike: the turn, how much slower than usual the
/// lean goes over to the other side, and the lean, with the curves.
struct KeyTurn {
    steer: f32,
    pivot_slowing: f32,
    lean: Option<f32>,
    look: curves::LeanLook,
}

impl KeySteer {
    /// Goes on `dt` seconds with `pressed` held (left less right) at `speed`
    /// (m/s): as Options > Control > Keyboard says, or by `curves` (with the
    /// bike's top speed) when they are on.
    fn update(
        &mut self,
        pressed: f32,
        speed: f32,
        dt: f32,
        options: &DebugOptions,
        curves: Option<(&KeyboardCurves, f32)>,
    ) -> KeyTurn {
        let pressed = pressed.clamp(-1.0, 1.0);
        match curves {
            Some((curves, top_speed)) => {
                self.digital = DigitalSteer::default();
                let lean = self.curves.update(pressed, curves, speed / top_speed, dt);
                KeyTurn {
                    steer: curves.turn_at(lean, speed / top_speed),
                    pivot_slowing: 0.0,
                    // Upright with no key held, the stick (if any) leans it.
                    lean: (pressed != 0.0 || lean != 0.0).then_some(lean),
                    look: curves.look,
                }
            }
            None => {
                self.curves = CurveLean::default();
                let steer = self.digital.update(
                    pressed,
                    reach_with(options.keyboard_limit, speed),
                    options.keyboard_rate.factor(),
                    options.keyboard_return.factor(),
                    dt,
                );
                KeyTurn {
                    steer,
                    pivot_slowing: options.keyboard_reverse.slowing(steer),
                    lean: None,
                    look: curves::LeanLook::Real,
                }
            }
        }
    }
}

/// The curves the keys and the stick steer by, each when it does, and the
/// bike's top speed their limit with speed is measured against.
#[derive(Clone, Copy)]
struct SteerCurves<'a> {
    keys: Option<&'a KeyboardCurves>,
    stick: Option<&'a KeyboardCurves>,
    top_speed: f32,
}

impl<'a> SteerCurves<'a> {
    fn keys(self) -> Option<(&'a KeyboardCurves, f32)> {
        self.keys.map(|curves| (curves, self.top_speed))
    }

    fn stick(self) -> Option<(&'a KeyboardCurves, f32)> {
        self.stick.map(|curves| (curves, self.top_speed))
    }
}

/// How the stick steers a bike: as always (its response, then
/// [`AnalogSteer`] with the spring of the steering), or leaning it over by
/// the stick's curves when they are on (see [`curves`]): R gives the lean
/// asked for how far it is pushed, and the lean follows it as A to E say.
/// How far it was pushed past its dead zone last (to the left).
#[derive(Default)]
struct StickSteer {
    analog: AnalogSteer,
    curves: CurveLean,
    travel: f32,
}

/// What the stick asks of the bike: the turn through the steering's
/// spring (as always), the turn straight in and the lean (by the curves).
struct StickTurn {
    steer: f32,
    direct: f32,
    lean: Option<f32>,
    look: curves::LeanLook,
}

impl StickSteer {
    /// Goes on `dt` seconds with the stick giving `classic` (its response
    /// as Options > Control > Stick says, to the left) and pushed `travel`
    /// past its dead zone (to the left), at `speed` (m/s): as always, or by
    /// `curves` (with the bike's top speed) when they are on.
    fn update(
        &mut self,
        classic: f32,
        travel: f32,
        speed: f32,
        dt: f32,
        options: &DebugOptions,
        curves: Option<(&KeyboardCurves, f32)>,
    ) -> StickTurn {
        self.travel = travel.clamp(-1.0, 1.0);
        match curves {
            Some((curves, top_speed)) => {
                self.analog = AnalogSteer::default();
                let asked = curves.lean_asked(self.travel);
                let lean = self.curves.update(asked, curves, speed / top_speed, dt);
                StickTurn {
                    steer: 0.0,
                    direct: curves.turn_at(lean, speed / top_speed),
                    lean: (asked != 0.0 || lean != 0.0).then_some(lean),
                    look: curves.look,
                }
            }
            None => {
                self.curves = CurveLean::default();
                let raw = classic * reach_with(options.stick_limit, speed);
                StickTurn {
                    steer: self.analog.update(raw, dt),
                    direct: 0.0,
                    lean: None,
                    look: curves::LeanLook::Real,
                }
            }
        }
    }
}

/// Dirección suavizada del stick: al tumbar hacia la curva (`leaning_in`), avanza
/// a un ritmo progresivo parecido a la cruceta o el teclado; al soltar o enderezar
/// hacia el centro, recupera de inmediato para mantener toda la agilidad.
#[derive(Default)]
struct AnalogSteer {
    value: f32,
}

impl AnalogSteer {
    fn update(&mut self, target: f32, dt: f32) -> f32 {
        // Al girar hacia el otro lado, aborta el giro anterior de inmediato
        // (igual que hace DigitalSteer en teclado/cruceta) para que la respuesta
        // sea instantánea.
        if target != 0.0 && self.value * target < 0.0 {
            self.value = 0.0;
        }
        let leaning_in = target.abs() > self.value.abs();
        let rate = if leaning_in { 3.8 } else { 12.0 };
        self.value = move_toward(self.value, target, rate * dt);
        self.value
    }
}

/// How far a trigger at rest may read pressed: some (the DualSense, with
/// its adaptive resistance engaged) never quite read zero, and the least
/// throttle takes away the engine braking, so the bike would run on by
/// itself.
pub const TRIGGER_REST: f32 = 0.15;

/// How much a gamepad's throttle or brake counts, from how far it reads
/// pressed: nothing up to [`TRIGGER_REST`], and from there all the way to
/// fully pressed. Buttons, pressed or not, count fully or not at all.
fn squeezed(value: f32) -> f32 {
    ((value - TRIGGER_REST) / (1.0 - TRIGGER_REST)).clamp(0.0, 1.0)
}

/// Curva de respuesta del stick: nada en su zona muerta, y a fondo gira
/// todo. In between, as the curve chosen in Options has it: normal turns
/// a little less than the stick near the middle, to correct the line
/// finely (see [`StickCurve`]).
fn stick_steer(x: f32, curve: StickCurve) -> f32 {
    stick_travel(x).abs().powf(curve.exponent()).copysign(x)
}

/// How far the stick reading `x` is pushed past its dead zone (0 to 1, on
/// its side): what its response, classic or R, is a curve of.
fn stick_travel(x: f32) -> f32 {
    const DEAD_ZONE: f32 = 0.08;
    ((x.abs() - DEAD_ZONE) / (1.0 - DEAD_ZONE))
        .clamp(0.0, 1.0)
        .copysign(x)
}

/// Con la tecla de reiniciar (R o Select en el mando, si no se han cambiado)
/// todas las motos vuelven a la parrilla y la carrera espera a que el jugador
/// acelere.
/// What the wheels throw up: blades of grass and puffs of dust.
type KickedUp = (With<GrassParticle>, With<DustPuff>);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn restart_race(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    settings: Res<Settings>,
    track: Res<Track>,
    mut race: ResMut<Race>,
    mut bikes: Query<(
        &GridPosition,
        &mut BikeState,
        &mut PreviousBikeState,
        &mut Knock,
        &mut CrashState,
        Option<&mut ai::Line>,
    )>,
    particles: Query<Entity, Or<KickedUp>>,
    autopiloted: Query<Entity, With<Autopilot>>,
    mut recalls: MessageReader<RecallToGrid>,
    mut commands: Commands,
) {
    let recalled = recalls.read().count() > 0;
    if !recalled
        && !settings
            .controls
            .just_pressed(Action::Restart, &keyboard, gamepads.iter())
    {
        return;
    }
    race.started = false;
    // The players ride their bikes again.
    for bike in &autopiloted {
        commands
            .entity(bike)
            .remove::<(Autopilot, Rival, ai::Line)>();
    }
    for (grid, mut state, mut previous, mut knock, mut crash, line) in &mut bikes {
        *state = BikeState::parked_on(&track, start_pose(&track, grid.0));
        if let Some(mut line) = line {
            line.0 = Track::grid_lane(grid.0);
        }
        // Sin estado anterior distinto, la interpolación no dibuja un "viaje".
        previous.0 = *state;
        *knock = Knock::default();
        *crash = CrashState::default();
    }
    for particle in &particles {
        commands.entity(particle).despawn();
    }
}

#[allow(clippy::type_complexity)]
fn simulate_bikes(
    time: Res<Time>,
    tuning: Res<BikeTuning>,
    track: Res<Track>,
    race: Res<Race>,
    mut bikes: Query<(
        Entity,
        &BikeInput,
        &mut BikeState,
        &mut PreviousBikeState,
        &mut Knock,
        &mut CrashState,
        Option<&mut ai::Line>,
        Option<&mut CollisionImpact>,
        Has<Retired>,
    )>,
) {
    let dt = time.delta_secs();
    let positions: Vec<(Entity, Vec2)> = bikes
        .iter()
        .map(|(entity, _, state, ..)| (entity, state.position))
        .collect();
    let idle = BikeInput::default();
    for (entity, input, mut state, mut previous, mut knock, mut crash, line, impact, retired) in
        &mut bikes
    {
        // Out of the race, a player's bike rolls to a stop.
        let input = if retired { &idle } else { input };
        previous.0 = *state;
        // Waiting for the lights, nobody moves.
        if race.held && !race.started {
            continue;
        }
        if let Some(ride) = state.ride.filter(|_| !crash.is_down()) {
            let outcome;
            let looped = &track.loops()[ride.index];
            (*state, outcome) = loop_ride::ride(&state, input, looped, &tuning, dt);
            match outcome {
                loop_ride::Outcome::Fell(velocity) => {
                    *crash = crash::fall_off(&mut state, &mut knock, velocity, &track);
                    if let Some(mut impact) = impact {
                        impact.record(1.0, true);
                    }
                }
                // Felt as against the walls of a road, though nobody falls
                // against these.
                loop_ride::Outcome::HitWall(into) => {
                    if let Some(mut impact) = impact.filter(|_| into > WALL_FELT) {
                        impact.record((into / crash::WALL_FALL).min(1.0), false);
                    }
                }
                loop_ride::Outcome::Riding | loop_ride::Outcome::Off => {}
            }
            continue;
        }
        if !crash.is_down() {
            let wall;
            (*state, wall) = advance_hitting_walls(&state, input, &track, &tuning, dt);
            if let Some(hit) = wall {
                let falls = crash::wall_knocks_down(&hit);
                if falls {
                    *crash = crash::fall_against_wall(&mut state, &mut knock, &hit);
                }
                // A scrape is not felt; a bump is, more the harder it was.
                if let Some(mut impact) = impact.filter(|_| hit.into > WALL_FELT) {
                    impact.record((hit.into / crash::WALL_FALL).min(1.0), falls);
                }
            }
            crash::carry(&mut state, &mut knock, &track, dt);
            continue;
        }
        crash::slide(&mut state, &mut knock, &mut crash, &track, dt);
        if crash.is_over() {
            // Back on the track at once, next to where the bike stopped and
            // clear of the others, facing along the track.
            let others: Vec<Vec2> = positions
                .iter()
                .filter(|(other, _)| *other != entity)
                .map(|&(_, position)| position)
                .collect();
            let (position, heading, lateral) =
                crash::rejoin_pose(&track, state.position, state.elevation, &others);
            *state = BikeState::parked_at_level(&track, (position, heading), state.elevation);
            // Without a different previous state, interpolation draws no trip.
            previous.0 = *state;
            *knock = Knock::default();
            *crash = CrashState::default();
            if let Some(mut line) = line {
                line.0 = lateral;
            }
        }
    }
}

/// Resolves the contacts between bikes (see [`crash`]): they push each other,
/// and the riders that cannot ride the hit out go down. A bike pushed into a
/// wall stays out of it.
#[allow(clippy::type_complexity)]
fn separate_bikes(
    time: Res<Time>,
    track: Res<Track>,
    mut bikes: Query<(
        &mut BikeState,
        &mut Knock,
        &mut CrashState,
        Option<&mut CollisionImpact>,
    )>,
) {
    let dt = time.delta_secs();
    for (.., impact) in &mut bikes {
        if let Some(mut impact) = impact {
            impact.timer = (impact.timer - dt).max(0.0);
        }
    }

    let mut pairs = bikes.iter_combinations_mut();
    while let Some(
        [
            (mut state_a, mut knock_a, mut crash_a, impact_a),
            (mut state_b, mut knock_b, mut crash_b, impact_b),
        ],
    ) = pairs.fetch_next()
    {
        // A bike on a loop is clear of the others.
        if state_a.ride.is_some() || state_b.ride.is_some() {
            continue;
        }
        let (down_a, down_b) = (crash_a.is_down(), crash_b.is_down());
        let (from_a, from_b) = (state_a.position, state_b.position);
        let contact = crash::collide(
            crash::Body {
                state: &mut state_a,
                knock: &mut knock_a,
                down: down_a,
            },
            crash::Body {
                state: &mut state_b,
                knock: &mut knock_b,
                down: down_b,
            },
        );
        for (state, from) in [(&mut state_a, from_a), (&mut state_b, from_b)] {
            if state.position != from
                && let Some((out, depth)) =
                    wall_contact(&track, state.position, state.elevation, &footprint(state))
            {
                state.position -= out * depth;
            }
        }
        let Some(contact) = contact else {
            continue;
        };
        if contact.a.falls {
            *crash_a = crash::fall(&mut state_a, &mut knock_a, &contact.a);
        }
        if contact.b.falls {
            *crash_b = crash::fall(&mut state_b, &mut knock_b, &contact.b);
        }
        let severity = contact.severity();
        for (impact, falls) in [(impact_a, contact.a.falls), (impact_b, contact.b.falls)] {
            if let Some(mut impact) = impact {
                impact.record(severity, falls);
            }
        }
    }
}

/// The players feel their new hits, against bikes or walls, in their
/// controllers: harder the harder the hit, and hardest when they go down.
#[allow(clippy::type_complexity)]
fn rumble_controllers(
    mut bikes: Query<(&mut CollisionImpact, Has<Player>, Option<&LocalPlayer>)>,
    mut rumble: MessageWriter<GamepadRumbleRequest>,
    game_mode: Option<Res<GameMode>>,
    local_players: Option<Res<LocalPlayersConfig>>,
    active_controller: Option<Res<crate::gamepads::ActiveController>>,
) {
    let pads = if game_mode.is_some_and(|mode| *mode == GameMode::SplitScreen) {
        let pad = |binding: Option<ControllerBinding>| match binding {
            Some(ControllerBinding::Gamepad(pad)) => Some(pad),
            _ => None,
        };
        local_players.map_or([None, None], |config| [pad(config.p1), pad(config.p2)])
    } else {
        // With one player, only the controller in use rumbles.
        [active_controller.and_then(|active| active.gamepad), None]
    };
    for (mut impact, player, local) in &mut bikes {
        let Some((strength, fell)) = impact.unfelt.take() else {
            continue;
        };
        let slot = match local {
            Some(local) => local.0,
            None if player => 0,
            None => continue,
        };
        let Some(&Some(gamepad)) = pads.get(slot) else {
            continue;
        };
        let (milliseconds, strong, weak) = if fell {
            (500, 0.75, 0.95)
        } else {
            let strength = 0.4 + 0.6 * strength;
            (250, 0.35 * strength, 0.65 * strength)
        };
        rumble.write(GamepadRumbleRequest::Add {
            duration: std::time::Duration::from_millis(milliseconds),
            intensity: GamepadRumbleIntensity {
                strong_motor: strong,
                weak_motor: weak,
            },
            gamepad,
        });
    }
}

/// Coloca cada moto entre su estado anterior y el actual según cuánto falta
/// para el siguiente paso fijo, para que el movimiento se vea suave a
/// cualquier tasa de fotogramas.
fn interpolate_bikes(
    fixed_time: Res<Time<Fixed>>,
    mut bikes: Query<(&BikeState, &PreviousBikeState, &mut Transform), Without<LeanPivot>>,
    mut pivots: Query<(&ChildOf, &mut Transform), With<LeanPivot>>,
) {
    let alpha = fixed_time.overstep_fraction();
    for (state, previous, mut transform) in &mut bikes {
        let (from, to) = (&previous.0, state);
        let position = from.position.lerp(to.position, alpha);
        let elevation = from.elevation.lerp(to.elevation, alpha);
        transform.translation = to_world(position, elevation + SURFACE_Y);
        // As a whole: round a loop the bike turns about its own up, and out
        // of it the pitch goes from a whole turn back to about zero.
        transform.rotation = from.orientation().slerp(to.orientation(), alpha);
    }
    for (child_of, mut transform) in &mut pivots {
        if let Ok((state, previous, _)) = bikes.get(child_of.parent()) {
            let lean = previous.0.lean.lerp(state.lean, alpha);
            transform.rotation = Quat::from_rotation_z(lean);
        }
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use std::f32::consts::FRAC_PI_2;

    use bevy::ecs::system::RunSystemOnce;

    use crate::pause::{ReturnRate, ReverseRate, SteerRate};
    use crate::track::{Dust, WALL_DISTANCE};

    use super::*;

    /// The roster's file reads, with a bike for each place on the grid;
    /// what it gets wrong is told.
    pub fn the_roster_reads_a_bike_for_each_place_on(roster: &'static crate::bike::Roster) {
        assert_eq!(roster.len(), RIDERS);
        // The engine's demo roster's file.
        let text = crate::testing::demo_file("bikes/roster.ron");
        let changed = |from, to| &*String::leak(text.replacen(from, to, 1));
        let parse = |text| crate::circuits::data::parse::<Vec<Entry>>("roster.ron", text);
        assert!(parse(text).is_ok());
        let livery = changed("\"cover\"", "\"nowhere\"");
        assert!(parse(livery).err().unwrap().contains("nowhere"));
        let unknown = changed("design:", "model:");
        assert!(parse(unknown).err().unwrap().contains("model"));
        // A model by a folder with none in it, or not a folder.
        let nowhere = changed("design: Afilada", "design: Model(\"nowhere\")");
        let error = parse(nowhere).map(|entries| entries.len());
        assert!(
            error
                .as_ref()
                .err()
                .is_some_and(|e| e.contains("models/nowhere/model.ron")),
            "{error:?}"
        );
        let path = changed("design: Afilada", "design: Model(\"../racer\")");
        assert!(parse(path).err().unwrap().contains("../racer"));
    }

    #[test]
    fn the_roster_reads_a_bike_for_each_place() {
        the_roster_reads_a_bike_for_each_place_on(&crate::testing::DEMO_ROSTER);
    }

    const DT: f32 = 1.0 / 120.0;

    fn drive(input: BikeInput, surface: Surface, seconds: f32, from: BikeState) -> BikeState {
        let tuning = BikeTuning::default();
        let mut state = from;
        for _ in 0..(seconds / DT) as usize {
            state = step(&state, &input, &Ground::flat(surface), &tuning, DT);
        }
        state
    }

    fn parked() -> BikeState {
        BikeState::parked_at((Vec2::ZERO, 0.0))
    }

    fn full_throttle() -> BikeInput {
        BikeInput {
            throttle: 1.0,
            ..default()
        }
    }

    #[test]
    fn throttle_moves_forward_up_to_top_speed() {
        let tuning = BikeTuning::default();
        let state = drive(full_throttle(), Surface::Asphalt, 3.0, parked());
        assert!(state.speed > 20.0, "acelera con brío: {}", state.speed);
        assert!(state.position.y < 0.0, "avanza hacia -Z");

        let state = drive(full_throttle(), Surface::Asphalt, 120.0, parked());
        assert!(state.speed <= tuning.max_speed);
        assert!(state.speed > 0.9 * tuning.max_speed);
    }

    #[test]
    fn grass_slows_the_bike_down() {
        let tuning = BikeTuning::default();
        let fast = drive(full_throttle(), Surface::Asphalt, 30.0, parked());
        let on_grass = drive(full_throttle(), Surface::Grass, 10.0, fast);
        assert!(on_grass.speed <= tuning.grass_max_speed + 0.5);
    }

    #[test]
    fn dirt_slows_the_bike_down_and_concrete_does_not() {
        let tuning = BikeTuning::default();
        let fast = drive(full_throttle(), Surface::Asphalt, 30.0, parked());
        let on_dirt = drive(full_throttle(), Surface::Dirt(Dust::Earth), 10.0, fast);
        assert!(on_dirt.speed <= tuning.grass_max_speed + 0.5);
        let on_concrete = drive(full_throttle(), Surface::Paved, 10.0, fast);
        assert!(on_concrete.speed > tuning.grass_max_speed + 20.0);
        assert_eq!(on_concrete.grass, 0.0);
    }

    pub fn the_chosen_bike_takes_the_player_slot_and_the_rest_the_rivals_on(
        roster: &'static crate::bike::Roster,
    ) {
        assert!(GRID[PLAYER_SLOT].is_none(), "el jugador no es un rival");
        assert_eq!(GRID.iter().filter(|rival| rival.is_none()).count(), 1);
        for choice in 0..roster.len() {
            let lineup = lineup(choice);
            assert_eq!(lineup[PLAYER_SLOT], choice);
            let mut sorted = lineup;
            sorted.sort();
            assert_eq!(sorted, [0, 1, 2, 3, 4, 5, 6, 7], "cada moto una vez");
        }
        // Sin cambiar de moto: las demás, por orden, delante del jugador.
        assert_eq!(lineup(0), [1, 2, 3, 4, 5, 6, 7, 0]);
    }

    #[test]
    fn the_chosen_bike_takes_the_player_slot_and_the_rest_the_rivals() {
        the_chosen_bike_takes_the_player_slot_and_the_rest_the_rivals_on(
            &crate::testing::DEMO_ROSTER,
        );
    }

    #[test]
    fn brushing_the_grass_hardly_slows_down() {
        let fast = drive(full_throttle(), Surface::Asphalt, 30.0, parked());
        let brushed = drive(full_throttle(), Surface::Grass, 0.3, fast);
        assert!(
            fast.speed - brushed.speed < 1.0,
            "pierde {}",
            fast.speed - brushed.speed
        );
        // Un segundo dentro sí frena, y al volver al asfalto se recupera.
        let deep = drive(full_throttle(), Surface::Grass, 1.0, fast);
        assert!(
            fast.speed - deep.speed > 5.0,
            "pierde {}",
            fast.speed - deep.speed
        );
        let back = drive(full_throttle(), Surface::Asphalt, 0.6, deep);
        assert_eq!(back.grass, 0.0);
    }

    #[test]
    fn brake_stops_then_reverses() {
        let tuning = BikeTuning::default();
        let moving = drive(full_throttle(), Surface::Asphalt, 5.0, parked());
        let brake = BikeInput {
            brake: 1.0,
            ..default()
        };
        let reversing = drive(brake, Surface::Asphalt, 8.0, moving);
        assert!(reversing.speed < 0.0);
        assert!(reversing.speed >= -tuning.reverse_max_speed);
    }

    #[test]
    fn letting_go_of_the_throttle_slows_down_harder_the_faster() {
        let tuning = BikeTuning::default();
        // Speed lost in the first second with nothing pressed (m/s).
        let coast = |speed: f32| {
            let from = BikeState { speed, ..parked() };
            speed - drive(BikeInput::default(), Surface::Asphalt, 1.0, from).speed
        };
        let (slow, fast) = (coast(15.0), coast(70.0));
        assert!(slow > 3.5 && slow < 4.5, "at 54 km/h: {slow} m/s²");
        assert!(fast > 7.0 && fast < 8.5, "at 250 km/h: {fast} m/s²");
        // Still far from what the brake does.
        assert!(fast < tuning.brake_decel / 3.0);
    }

    #[test]
    fn coasting_comes_to_a_stop() {
        let moving = drive(full_throttle(), Surface::Asphalt, 5.0, parked());
        let stopped = drive(BikeInput::default(), Surface::Asphalt, 60.0, moving);
        assert_eq!(stopped.speed, 0.0);
    }

    #[test]
    fn cannot_turn_while_stopped() {
        let input = BikeInput {
            steer: 1.0,
            ..default()
        };
        let state = drive(input, Surface::Asphalt, 2.0, parked());
        assert_eq!(state.heading, 0.0);
        assert_eq!(state.position, Vec2::ZERO);
    }

    #[test]
    fn steering_builds_up_and_settles_without_jerks() {
        let tuning = BikeTuning::default();
        let moving = drive(full_throttle(), Surface::Asphalt, 3.0, parked());
        let input = BikeInput {
            throttle: 1.0,
            steer: 1.0,
            ..default()
        };
        let mut state = moving;
        let mut previous_rate = 0.0;
        let mut steps = 0;
        while state.steer < 0.99 {
            state = step(&state, &input, &Ground::flat(Surface::Asphalt), &tuning, DT);
            // Empieza despacio: el cambio de la dirección crece de paso a paso.
            let rate_change = (state.steer_rate - previous_rate).abs();
            assert!(rate_change * DT < 0.01, "tirón de {rate_change}");
            previous_rate = state.steer_rate;
            steps += 1;
        }
        let seconds = steps as f32 * DT;
        assert!((0.3..0.9).contains(&seconds), "tarda {seconds} s");
        // Y no se pasa: llega sin rebotar.
        let settled = drive(input, Surface::Asphalt, 1.0, state);
        assert!(settled.steer <= 1.0 && settled.steer > 0.99);
    }

    /// Valores de la dirección del teclado, fotograma a fotograma, pulsando
    /// lo que diga `pressed` en cada instante.
    fn digital_steer(seconds: f32, dt: f32, pressed: impl Fn(f32) -> f32) -> Vec<f32> {
        digital_steer_at(0.0, seconds, dt, pressed)
    }

    /// Igual, yendo a `speed` (m/s).
    fn digital_steer_at(
        speed: f32,
        seconds: f32,
        dt: f32,
        pressed: impl Fn(f32) -> f32,
    ) -> Vec<f32> {
        digital_steer_with_rate(speed, 1.0, seconds, dt, pressed)
    }

    /// Igual, con un multiplicador de rapidez `rate_factor`.
    fn digital_steer_with_rate(
        speed: f32,
        rate_factor: f32,
        seconds: f32,
        dt: f32,
        pressed: impl Fn(f32) -> f32,
    ) -> Vec<f32> {
        digital_steer_with_rates(speed, rate_factor, 1.0, seconds, dt, pressed)
    }

    /// Igual, con multiplicadores de rapidez al pulsar y al soltar.
    fn digital_steer_with_rates(
        speed: f32,
        rate_factor: f32,
        return_factor: f32,
        seconds: f32,
        dt: f32,
        pressed: impl Fn(f32) -> f32,
    ) -> Vec<f32> {
        let mut digital = DigitalSteer::default();
        (0..(seconds / dt).round() as usize)
            .map(|frame| {
                digital.update(
                    pressed(frame as f32 * dt),
                    digital_reach(speed),
                    rate_factor,
                    return_factor,
                    dt,
                )
            })
            .collect()
    }

    #[test]
    fn analog_steer_leans_in_progressively_and_returns_fast() {
        let mut analog = AnalogSteer::default();
        let dt = 1.0 / 120.0;
        let mut frames_in = 0;
        while analog.value < 0.99 {
            analog.update(1.0, dt);
            frames_in += 1;
        }
        let time_in = frames_in as f32 * dt;
        assert!((0.2..0.4).contains(&time_in), "tumbar tarda {time_in} s");

        let mut frames_out = 0;
        while analog.value > 0.01 {
            analog.update(0.0, dt);
            frames_out += 1;
        }
        let time_out = frames_out as f32 * dt;
        assert!(
            (0.05..0.15).contains(&time_out),
            "volver al centro tarda {time_out} s"
        );
        assert!(
            time_out < time_in * 0.5,
            "vuelve mucho más rápido de lo que tumba"
        );

        // Al cambiar de lado, aborta el lado anterior y empieza de inmediato
        analog.update(1.0, dt);
        assert!(analog.value > 0.0);
        analog.update(-1.0, dt);
        assert!(
            analog.value < 0.0,
            "empieza de inmediato hacia el otro lado"
        );
    }

    #[test]
    fn going_over_to_the_other_side_is_told_from_leaning_in() {
        // The target over to the other side: going over.
        assert!(swinging(false, 0.5, -0.5));
        // On its way out on the new side, if it was going over.
        assert!(swinging(true, -0.2, -0.5));
        assert!(!swinging(false, -0.2, -0.5), "leaning in from upright");
        // Nearly there, or the target back towards the middle: done.
        assert!(!swinging(true, -0.48, -0.5));
        assert!(!swinging(true, -0.2, -0.1));
        assert!(!swinging(true, 0.0, 0.0));
    }

    #[test]
    fn the_keyboard_turning_the_other_way_swings_the_bike_over_as_fast_as_asked() {
        let tuning = BikeTuning::default();
        // Turning flat out to the left at speed, then to the right: how long
        // until the bike leans most of the way to the right, and where it
        // goes (the turn changes at once whatever the option).
        let over = |rate: ReverseRate| {
            let mut state = BikeState {
                speed: 50.0,
                ..parked()
            };
            let input = |steer_direct: f32| BikeInput {
                throttle: 1.0,
                steer_direct,
                pivot_slowing: rate.slowing(steer_direct),
                ..default()
            };
            for _ in 0..(1.0 / DT) as usize {
                state = step(
                    &state,
                    &input(1.0),
                    &Ground::flat(Surface::Asphalt),
                    &tuning,
                    DT,
                );
            }
            let leaned = state.lean;
            let mut over = None;
            for frame in 1..=(1.0 / DT) as usize {
                state = step(
                    &state,
                    &input(-1.0),
                    &Ground::flat(Surface::Asphalt),
                    &tuning,
                    DT,
                );
                if over.is_none() && state.lean < -0.8 * leaned {
                    over = Some(frame as f32 * DT);
                }
            }
            (over.unwrap(), state.heading)
        };
        let (fast, heading) = over(ReverseRate::Fast);
        let (medium, medium_heading) = over(ReverseRate::Medium);
        let (slow, slow_heading) = over(ReverseRate::Slow);
        assert!(fast < 0.2, "fast, as always: {fast} s");
        assert!(
            medium > 1.4 * fast && slow > 1.4 * medium,
            "{fast} {medium} {slow}"
        );
        assert!(slow < 0.6, "slow: {slow} s");
        // Fast is as always: no slowing at all.
        assert_eq!(ReverseRate::Fast.slowing(1.0), 0.0);
        // The bike's course is the same: only the lean is slower.
        assert_eq!(heading, medium_heading);
        assert_eq!(heading, slow_heading);
        // Nothing slower with the stick (no keyboard turn).
        assert_eq!(ReverseRate::Slow.slowing(0.0), 0.0);
    }

    #[test]
    fn with_the_curves_the_bike_shows_the_lean_they_ask_for() {
        let tuning = BikeTuning::default();
        let mut state = BikeState {
            speed: 50.0,
            ..parked()
        };
        let input = BikeInput {
            throttle: 1.0,
            steer_direct: 0.5,
            lean: Some(0.5),
            ..default()
        };
        for _ in 0..(0.2 / DT) as usize {
            state = step(&state, &input, &Ground::flat(Surface::Asphalt), &tuning, DT);
        }
        // Half the bike's lean, whatever the turn would give.
        assert!(
            (state.lean - 0.5 * tuning.max_lean).abs() < 0.01,
            "{}",
            state.lean
        );
        // To the other side, at once (within a few hundredths).
        let other = BikeInput {
            steer_direct: -0.5,
            lean: Some(-0.5),
            ..input
        };
        for _ in 0..(0.1 / DT) as usize {
            state = step(&state, &other, &Ground::flat(Surface::Asphalt), &tuning, DT);
        }
        assert!(
            (state.lean + 0.5 * tuning.max_lean).abs() < 0.03,
            "{}",
            state.lean
        );
        // At walking pace, hardly any: the bars turn it.
        let mut slow = BikeState {
            speed: 3.0,
            ..parked()
        };
        let coasting = BikeInput {
            throttle: 0.0,
            ..input
        };
        for _ in 0..(0.2 / DT) as usize {
            slow = step(
                &slow,
                &coasting,
                &Ground::flat(Surface::Asphalt),
                &tuning,
                DT,
            );
        }
        assert!(slow.lean.abs() < 0.05 * tuning.max_lean, "{}", slow.lean);
    }

    #[test]
    fn the_keys_steer_as_always_without_the_curves_and_by_them_with_them() {
        let options = DebugOptions::default();
        let mut keys = KeySteer::default();
        let mut digital = DigitalSteer::default();
        let reach = reach_with(options.keyboard_limit, 30.0);
        let (rate, back) = (
            options.keyboard_rate.factor(),
            options.keyboard_return.factor(),
        );
        for frame in 0..90 {
            let pressed = if frame < 50 { 1.0 } else { 0.0 };
            let turn = keys.update(pressed, 30.0, DT, &options, None);
            assert_eq!(turn.steer, digital.update(pressed, reach, rate, back, DT));
            assert_eq!(turn.lean, None);
        }
        // With the classic curves, nearly the same turn, and the lean.
        let curves = KeyboardCurves::default();
        let mut by_curves = KeySteer::default();
        let mut digital = DigitalSteer::default();
        for frame in 0..90 {
            let pressed = if frame < 50 { 1.0 } else { 0.0 };
            let turn = by_curves.update(pressed, 30.0, DT, &options, Some((&curves, 80.0)));
            let classic = digital.update(pressed, reach, rate, back, DT);
            // Let go short of all the way over (the limit at speed), the
            // curve goes on from where it is that high, a little ahead of
            // the classic keyboard, which starts back slowly from anywhere.
            let near = if frame < 50 { 0.03 } else { 0.08 };
            assert!(
                (turn.steer - classic).abs() < near,
                "{frame}: {} {classic}",
                turn.steer
            );
            // E is a straight line in the classic curves: as much turn as lean.
            assert!((turn.lean.unwrap() - turn.steer).abs() < 1e-4);
            assert_eq!(turn.pivot_slowing, 0.0);
        }
        // Upright with no key held, the lean is left to the turn.
        let mut idle = KeySteer::default();
        let turn = idle.update(0.0, 30.0, DT, &options, Some((&curves, 80.0)));
        assert_eq!((turn.steer, turn.lean), (0.0, None));
    }

    #[test]
    fn the_stick_steers_as_always_without_its_curves_and_leans_by_them_with_them() {
        let options = DebugOptions::default();
        let mut stick = StickSteer::default();
        let mut analog = AnalogSteer::default();
        let reach = reach_with(options.stick_limit, 30.0);
        for frame in 0..60 {
            let x = if frame < 30 { -0.7 } else { 0.0 };
            let classic = -stick_steer(x, options.stick_curve);
            let turn = stick.update(classic, -stick_travel(x), 30.0, DT, &options, None);
            assert_eq!(turn.steer, analog.update(classic * reach, DT));
            assert_eq!((turn.direct, turn.lean), (0.0, None));
        }
        // By the classic curves: the lean the stick asks for, turned
        // straight in, near the classic turn.
        let curves = KeyboardShapes::classic(true).curves();
        let mut stick = StickSteer::default();
        let mut analog = AnalogSteer::default();
        for _ in 0..60 {
            let x = -0.7;
            let classic = -stick_steer(x, options.stick_curve);
            let turn = stick.update(
                classic,
                -stick_travel(x),
                30.0,
                DT,
                &options,
                Some((&curves, 80.0)),
            );
            let was = analog.update(classic * reach, DT);
            assert_eq!(turn.steer, 0.0);
            assert!((turn.direct - was).abs() < 0.03, "{} {was}", turn.direct);
            assert!((turn.lean.unwrap() - turn.direct).abs() < 1e-4);
        }
    }

    #[test]
    fn braking_loads_the_front_and_the_bike_turns_tighter_for_a_moment() {
        let tuning = BikeTuning::default();
        let ground = Ground::flat(Surface::Asphalt);
        let turning = |state: &BikeState| {
            let input = BikeInput {
                steer_direct: 1.0,
                ..default()
            };
            let next = step(state, &input, &ground, &tuning, DT);
            wrap_angle(next.heading - state.heading) / DT
        };
        let cruising = BikeState {
            speed: 40.0,
            ..parked()
        };
        // Braking hard for half a second: the fork dives.
        let mut braked = cruising;
        for _ in 0..60 {
            braked = step(
                &braked,
                &BikeInput {
                    brake: 1.0,
                    ..default()
                },
                &ground,
                &tuning,
                DT,
            );
        }
        assert!(
            braked.chassis.front_load() > 0.5,
            "{}",
            braked.chassis.front_load()
        );
        // At the same speed, nose down as it is, it turns tighter loaded
        // than with the fork up.
        let same = BikeState {
            chassis: Chassis {
                squat: 0.0,
                squat_rate: 0.0,
                ..braked.chassis
            },
            ..braked
        };
        assert!(turning(&braked) > 1.05 * turning(&same));
        assert!(turning(&braked) <= (1.0 + BRAKE_GRIP) * 1.001 * turning(&same));
        // A moment after letting go, most of the load is gone (the engine
        // braking keeps a little).
        let mut after = braked;
        for _ in 0..120 {
            after = step(&after, &BikeInput::default(), &ground, &tuning, DT);
        }
        let (was, now) = (braked.chassis.front_load(), after.chassis.front_load());
        assert!(now < 0.5 * was, "{was} -> {now}");
        // With the classic controls (plain grip), as it always has: loaded
        // or not, the same turn.
        let plain = |state: &BikeState| {
            let input = BikeInput {
                steer_direct: 1.0,
                plain_grip: true,
                ..default()
            };
            let next = step(state, &input, &ground, &tuning, DT);
            wrap_angle(next.heading - state.heading) / DT
        };
        assert_eq!(plain(&braked), plain(&same));
    }

    #[test]
    fn the_gears_change_up_near_their_top_and_drive_most_at_their_best_revs() {
        // Up at the first gear's top, with a moment with no drive.
        let (gear, shift, pulling) = gearbox(0, 0.0, GEARS[0] * 0.98, 1.0, DT);
        assert_eq!((gear, pulling), (1, 0.0));
        assert!(shift > 0.0);
        // Not with the throttle shut.
        assert_eq!(gearbox(0, 0.0, GEARS[0] * 0.98, 0.0, DT).0, 0);
        // Down well below the gear under it, all the way.
        assert_eq!(gearbox(4, 0.0, 10.0, 0.0, DT).0, 0);
        // Strongest at its best revs, as without gears at either end.
        let at = |share: f32| gearbox(2, 0.0, GEARS[2] * share, 1.0, DT).2;
        assert!((at(TORQUE_PEAK) - (1.0 + TORQUE_BOOST)).abs() < 1e-4);
        assert!(at(0.6) < at(0.8) && at(0.95) < at(0.8));
        assert_eq!(at(0.2), 1.0);
        // From standing, flat out, the bike goes up through them.
        let tuning = BikeTuning {
            gears: true,
            ..default()
        };
        let mut state = parked();
        let mut gears = Vec::new();
        for _ in 0..(20.0 / DT) as usize {
            state = step(
                &state,
                &full_throttle(),
                &Ground::flat(Surface::Asphalt),
                &tuning,
                DT,
            );
            if gears.last() != Some(&state.gear) {
                gears.push(state.gear);
            }
        }
        assert!(
            gears.len() >= 5 && gears.windows(2).all(|w| w[1] == w[0] + 1),
            "{gears:?}"
        );
        assert_eq!(gears[0], 0);
        // About as quick to top speed as without them.
        let plain = drive(full_throttle(), Surface::Asphalt, 20.0, parked());
        assert!(
            (state.speed - plain.speed).abs() < 3.0,
            "{} {}",
            state.speed,
            plain.speed
        );
    }

    #[test]
    fn the_lean_shown_follows_the_one_felt_as_its_look_says() {
        let tuning = BikeTuning::default();
        let ground = Ground::flat(Surface::Asphalt);
        // Upright, asked half the lean at once: how far the lean shown is
        // after `seconds`, as a share of it.
        let shown = |look: curves::LeanLook, seconds: f32| {
            let mut state = BikeState {
                speed: 40.0,
                lean: 0.0,
                ..parked()
            };
            let input = BikeInput {
                throttle: 1.0,
                steer_direct: 0.5,
                lean: Some(0.5),
                look,
                ..default()
            };
            for _ in 0..(seconds / DT).round() as usize {
                state = step(&state, &input, &ground, &tuning, DT);
            }
            state.lean / (0.5 * tuning.max_lean)
        };
        use curves::LeanLook::{Real, Smooth, Soft};
        // Real: there in a few hundredths; soft, about 80 ms behind (two
        // thirds there by then); smooth, slower still at first, about three
        // quarters there by 150 ms, and never past it.
        assert!(shown(Real, 0.08) > 0.95);
        let soft = shown(Soft, 0.08);
        assert!((0.55..0.72).contains(&soft), "{soft}");
        assert!(shown(Smooth, 0.04) < shown(Soft, 0.04));
        let smooth = shown(Smooth, 0.15);
        assert!((0.6..0.85).contains(&smooth), "{smooth}");
        for seconds in [0.3, 0.6, 1.0] {
            assert!(shown(Smooth, seconds) <= 1.0 + 1e-3);
        }
        assert!(shown(Smooth, 1.0) > 0.99 && shown(Soft, 1.0) > 0.99);
    }

    pub fn a_banked_helix_is_taken_flat_out_on(
        circuits: &'static crate::circuits::Circuits,
        name: &str,
    ) {
        // Round every point where the toy cars' banked track grips harder, at
        // top speed, the turn its bend needs is within what the bike gives
        // short of where it scrubs speed off (`CORNER_LOSS_FROM`).
        let circuit = circuits
            .iter()
            .find(|circuit| circuit.name == name)
            .unwrap();
        let track = Track::new(&circuit.layout);
        let tuning = BikeTuning::default();
        let speed = tuning.max_speed;
        let steer = CORNER_LOSS_FROM;
        let mut grippy = 0;
        for index in 0..track.sample_count() {
            let grip = track.grip_at(0, index);
            if grip <= 1.0 {
                continue;
            }
            grippy += 1;
            let extra = CORNER_GRIP.end * smoothstep(CORNER_GRIP_FROM, 1.0, steer);
            let most = steer * tuning.max_yaw_rate_with_grip(speed, (1.0 + extra) * grip);
            let needed = track.smoothed_curvature(index, 4) * speed;
            assert!(
                needed <= most,
                "{} m: {needed} > {most}",
                index as f32 * track.step_length()
            );
        }
        // All the helix: over 250 m of it.
        assert!(grippy as f32 * track.step_length() > 250.0, "{grippy}");
    }

    #[test]
    fn keyboard_leans_the_bike_at_once_and_lets_it_up_softly() {
        let tuning = BikeTuning::default();
        let mut digital = DigitalSteer::default();
        let mut state = BikeState {
            speed: 60.0,
            ..parked()
        };
        let (hold, release) = (0.8, (0.8 / DT).round() as usize);
        let mut leans = Vec::new();
        for frame in 0..(1.6 / DT).round() as usize {
            let pressed = if frame < release { 1.0 } else { 0.0 };
            let input = BikeInput {
                throttle: 1.0,
                steer_direct: digital.update(pressed, digital_reach(state.speed), 1.0, 1.0, DT),
                ..default()
            };
            state = step(&state, &input, &Ground::flat(Surface::Asphalt), &tuning, DT);
            leans.push(state.lean);
        }
        let held = leans[release - 1];
        let seconds = |frames: usize| frames as f32 * DT;
        let half_in = seconds(leans.iter().position(|&l| l >= 0.5 * held).unwrap());
        let half_out = seconds(
            leans[release..]
                .iter()
                .position(|&l| l <= 0.5 * held)
                .unwrap(),
        );
        println!(
            "tumbada {held:.3} rad en {hold} s: la mitad en {half_in:.3} s, se levanta a la mitad en {half_out:.3} s"
        );
        assert!(half_in < 0.2, "tarda {half_in} s en tumbarse a medias");
        assert!(half_out > 1.5 * half_in, "se levanta en {half_out} s");
    }

    #[test]
    fn the_debug_limits_take_all_part_or_none_of_the_turn_limit() {
        let flat_out = 80.0;
        assert_eq!(reach_with(TurnLimit::Off, flat_out), 1.0);
        assert_eq!(
            reach_with(TurnLimit::High, flat_out),
            digital_reach(flat_out)
        );
        let low = reach_with(TurnLimit::Low, flat_out);
        let medium = reach_with(TurnLimit::Medium, flat_out);
        assert!(1.0 > low && low > medium && medium > digital_reach(flat_out));
        // Despacio no limita ninguno.
        assert_eq!(reach_with(TurnLimit::High, 5.0), 1.0);
    }

    #[test]
    fn keyboard_steering_turns_less_the_faster_the_bike_goes() {
        let held = |speed: f32| digital_steer_at(speed, 1.5, DT, |_| 1.0);
        let at = |values: &[f32], seconds: f32| values[(seconds / DT).round() as usize - 1];
        let (parked, cruising, flat_out) = (held(5.0), held(45.0), held(80.0));
        // Despacio gira del todo; deprisa, mantenerlo se queda a las puertas de
        // apurar.
        assert_eq!(*parked.last().unwrap(), 1.0);
        let most = *flat_out.last().unwrap();
        assert!((0.75..0.85).contains(&most), "a fondo: {most}");
        let middle = *cruising.last().unwrap();
        assert!(most < middle && middle < 1.0, "a media: {middle}");
        // Responde igual de rápido a cualquier velocidad.
        for seconds in [0.05, 0.1, 0.2] {
            assert_eq!(at(&flat_out, seconds), at(&parked, seconds));
        }
        // Y llega a lo más que gira en algo menos de medio segundo.
        assert!(at(&flat_out, 0.5) > most - 0.01);
    }

    #[test]
    fn keyboard_steering_answers_at_once_and_speeds_up_while_held() {
        let values = digital_steer(1.0, DT, |_| 1.0);
        let at = |seconds: f32| values[(seconds / DT).round() as usize - 1];
        // Sin retraso: ya en el primer fotograma se mueve.
        assert!(values[0] > 0.0);
        // Un toque corto gira un poco; mantenerlo un poco, bastante; algo más
        // de un cuarto de segundo, del todo.
        assert!((0.2..0.35).contains(&at(0.1)), "toque: {}", at(0.1));
        assert!((0.5..0.7).contains(&at(0.2)), "{}", at(0.2));
        assert_eq!(at(0.35), 1.0);
        // Cada vez más deprisa: gira más en la segunda décima que en la primera.
        assert!(at(0.2) - at(0.1) > at(0.1));
        assert!(values.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn keyboard_steering_returns_faster_the_longer_it_is_released() {
        let values = digital_steer(1.5, DT, |t| if t < 0.7 { 1.0 } else { 0.0 });
        let at = |seconds: f32| values[(seconds / DT).round() as usize - 1];
        assert_eq!(at(0.7), 1.0);
        // Al soltar empieza a volver enseguida, poco a poco y cada vez más
        // deprisa: más suave que al pulsar.
        let first = at(0.7) - at(0.8);
        let second = at(0.8) - at(0.9);
        assert!(first > 0.0 && second > first * 1.3, "{first} y {second}");
        let pressing = digital_steer(0.1, DT, |_| 1.0);
        assert!(first < 0.5 * pressing.last().unwrap());
        // En algo más de medio segundo está en el centro, sin cruzar al otro
        // lado.
        assert!(at(1.2) > 0.0);
        assert_eq!(at(1.35), 0.0);
        assert!(values.iter().all(|&v| v >= 0.0));
    }

    #[test]
    fn keyboard_steering_aborts_recovery_to_turn_the_other_way() {
        // Al cambiar de lado aborta la recuperación y empieza de inmediato
        // hacia el otro lado, como si se pulsara desde el centro.
        let values = digital_steer(1.5, DT, |t| if t < 0.7 { 1.0 } else { -1.0 });
        let at = |seconds: f32| values[(seconds / DT).round() as usize - 1];
        let from_center = digital_steer(0.2, DT, |_| -1.0);
        let at_center = |seconds: f32| from_center[(seconds / DT).round() as usize - 1];
        // En 0.1 s tras cambiar de lado está exactamente en la misma posición
        // que pulsando brevemente sin estar inclinado.
        assert_eq!(at(0.8), at_center(0.1));
        assert!(at(0.8) < 0.0);
        assert_eq!(*values.last().unwrap(), -1.0);
    }

    #[test]
    fn keyboard_steering_aborts_active_recovery_when_tapping_opposite() {
        // Si se suelta (empieza a recuperar) y se da un toque al lado opuesto,
        // aborta la recuperación y va a la misma posición que desde el centro.
        let values = digital_steer(1.5, DT, |t| {
            if t < 0.4 {
                1.0
            } else if t < 0.6 {
                0.0
            } else {
                -1.0
            }
        });
        let at = |seconds: f32| values[(seconds / DT).round() as usize - 1];
        let from_center = digital_steer(0.2, DT, |_| -1.0);
        let at_center = |seconds: f32| from_center[(seconds / DT).round() as usize - 1];
        // En 0.6 s estaba recuperando hacia el centro (todavía positivo).
        assert!(at(0.6) > 0.0);
        // En 0.7 s (0.1 s de toque opuesto), está exactamente en la posición del
        // toque desde el centro.
        assert_eq!(at(0.7), at_center(0.1));
        assert!(at(0.7) < 0.0);
    }

    #[test]
    fn keyboard_steering_is_the_same_at_any_frame_rate() {
        let pressed = |t: f32| if t < 0.4 { 1.0 } else { 0.0 };
        let fast = digital_steer(0.6, DT, pressed);
        let slow = digital_steer(0.6, 0.05, pressed);
        for (frame, value) in slow.iter().enumerate() {
            let same_moment = fast[(frame + 1) * 6 - 1];
            assert!(
                (value - same_moment).abs() < 0.02,
                "{value} y {same_moment}"
            );
        }
    }

    #[test]
    fn keyboard_steering_rate_option_changes_how_fast_it_turns_while_held() {
        let at = |values: &[f32], seconds: f32| values[(seconds / DT).round() as usize - 1];
        let held = |rate: SteerRate| digital_steer_with_rate(0.0, rate.factor(), 1.0, DT, |_| 1.0);
        let slow = held(SteerRate::Slow);
        let medium = held(SteerRate::Medium);
        let fast = held(SteerRate::Fast);

        // Al dejar pulsado, lento gira menos que medio, y medio menos que alto.
        assert!(at(&slow, 0.2) < at(&medium, 0.2));
        assert!(at(&medium, 0.2) < at(&fast, 0.2));

        // Alto llega al tope en unos 0.35 s; lento tarda más de medio segundo.
        assert_eq!(at(&fast, 0.35), 1.0);
        assert!(at(&slow, 0.35) < 0.8);
        assert_eq!(at(&slow, 0.8), 1.0);
    }

    #[test]
    fn keyboard_steering_return_option_changes_how_fast_it_returns_when_released() {
        let at = |values: &[f32], seconds: f32| values[(seconds / DT).round() as usize - 1];
        let released = |rate: ReturnRate| {
            digital_steer_with_rates(0.0, 1.0, rate.factor(), 1.5, DT, |t| {
                if t < 0.5 { 1.0 } else { 0.0 }
            })
        };
        let slow = released(ReturnRate::Slow);
        let medium = released(ReturnRate::Medium);
        let fast = released(ReturnRate::Fast);

        // A los 0.5 s todos han llegado al tope.
        assert_eq!(at(&slow, 0.5), 1.0);
        assert_eq!(at(&medium, 0.5), 1.0);
        assert_eq!(at(&fast, 0.5), 1.0);

        // Al soltar (a los 0.8 s, 0.3 s tras soltar), rápido ha recuperado más que medio, y medio más que lento.
        assert!(at(&fast, 0.8) < at(&medium, 0.8));
        assert!(at(&medium, 0.8) < at(&slow, 0.8));

        // Rápido (actual) vuelve al centro antes que lento.
        assert_eq!(at(&fast, 1.15), 0.0);
        assert!(at(&slow, 1.15) > 0.0);
    }

    #[test]
    fn lean_follows_how_hard_the_bike_turns() {
        let tuning = BikeTuning::default();
        let lean_at = |steer: f32| {
            let mut state = BikeState {
                speed: 50.0,
                steer,
                ..parked()
            };
            let input = BikeInput {
                throttle: 1.0,
                steer,
                ..default()
            };
            for _ in 0..240 {
                state = step(&state, &input, &Ground::flat(Surface::Asphalt), &tuning, DT);
            }
            state.lean
        };
        let (gentle, full) = (lean_at(0.15), lean_at(1.0));
        assert!(
            gentle < 0.45 * tuning.max_lean,
            "poco giro, poca inclinación: {gentle}"
        );
        assert!(full > 0.9 * tuning.max_lean);
        // Y se tumba deprisa, pero no de golpe.
        let mut state = BikeState {
            speed: 50.0,
            steer: 1.0,
            ..parked()
        };
        let input = BikeInput {
            throttle: 1.0,
            steer: 1.0,
            ..default()
        };
        for _ in 0..6 {
            state = step(&state, &input, &Ground::flat(Surface::Asphalt), &tuning, DT);
        }
        assert!(
            state.lean < 0.6 * tuning.max_lean,
            "a los 0,05 s: {}",
            state.lean
        );
    }

    #[test]
    fn stick_turns_little_near_the_center_and_fully_at_the_edge() {
        let stick_steer = |x| stick_steer(x, StickCurve::Normal);
        assert_eq!(stick_steer(0.05), 0.0);
        assert!(stick_steer(0.3) < 0.2);
        assert!(stick_steer(-0.3) > -0.2 && stick_steer(-0.3) < 0.0);
        assert_eq!(stick_steer(1.0), 1.0);
        assert_eq!(stick_steer(-1.0), -1.0);
        let mut last = 0.0;
        for i in 0..=20 {
            let value = stick_steer(i as f32 / 20.0);
            assert!(value >= last, "crece sin saltos");
            last = value;
        }
    }

    #[test]
    fn every_stick_curve_turns_fully_at_the_edge_and_they_differ_near_the_middle() {
        let curves = [
            StickCurve::SquareRoot,
            StickCurve::Linear,
            StickCurve::Normal,
            StickCurve::Quadratic,
            StickCurve::Cubic,
        ];
        for curve in curves {
            assert_eq!(stick_steer(0.05, curve), 0.0, "{curve:?}: its dead zone");
            assert_eq!(stick_steer(1.0, curve), 1.0, "{curve:?}");
            assert_eq!(stick_steer(-1.0, curve), -1.0, "{curve:?}");
            assert_eq!(stick_steer(-0.5, curve), -stick_steer(0.5, curve));
            let mut last = 0.0;
            for i in 0..=20 {
                let value = stick_steer(i as f32 / 20.0, curve);
                assert!(value >= last, "{curve:?}: grows without jumps");
                last = value;
            }
        }
        // A third of the way over, from liveliest to finest.
        let at_a_third = curves.map(|curve| stick_steer(0.3, curve));
        assert!(at_a_third.windows(2).all(|w| w[0] > w[1]), "{at_a_third:?}");
        // Linear: as far as the stick, past its dead zone.
        let linear = stick_steer(0.54, StickCurve::Linear);
        assert!((linear - 0.5).abs() < 1e-5, "{linear}");
        // Normal at first: as always.
        assert_eq!(DebugOptions::default().stick_curve, StickCurve::Normal);
    }

    #[test]
    fn steering_left_turns_and_leans_left() {
        let moving = drive(full_throttle(), Surface::Asphalt, 3.0, parked());
        let input = BikeInput {
            throttle: 1.0,
            steer: 1.0,
            ..default()
        };
        let turning = drive(input, Surface::Asphalt, 0.5, moving);
        assert!(turning.heading > moving.heading);
        assert!(turning.lean > 0.0);
        assert!(turning.lean <= BikeTuning::default().max_lean);
    }

    #[test]
    fn turning_radius_grows_with_speed() {
        let tuning = BikeTuning::default();
        let radius_at = |speed: f32| {
            let state = BikeState {
                speed,
                steer: 1.0,
                ..parked()
            };
            let input = BikeInput {
                steer: 1.0,
                ..default()
            };
            let next = step(&state, &input, &Ground::flat(Surface::Asphalt), &tuning, DT);
            next.speed / ((next.heading - state.heading) / DT)
        };
        assert!(radius_at(40.0) > radius_at(20.0));
        assert!(radius_at(20.0) > radius_at(5.0));
    }

    /// Una moto en el borde del muro izquierdo de la recta de meta, con la
    /// orientación dada.
    fn against_left_wall(heading: f32) -> (Track, BikeState) {
        let track = Track::default();
        // 100 m después de la meta, en plena recta hacia -Z.
        let position = track.offset_point(50, WALL_DISTANCE + 1.0);
        let state = BikeState {
            position,
            heading,
            speed: 40.0,
            elevation: track.road_height(position),
            ..parked()
        };
        (track, state)
    }

    #[test]
    fn head_on_wall_crash_pushes_back_and_kills_speed() {
        // Mirando a la izquierda (-X), de cara contra el muro.
        let (track, mut state) = against_left_wall(FRAC_PI_2);
        hit_walls(&mut state, &track);
        let distance = track.distance_to_centerline(state.position);
        assert!(distance <= WALL_DISTANCE - BIKE_HALF_WIDTH + 1e-3);
        assert!(state.speed < 40.0 * (1.0 - IMPACT_SPEED_LOSS) + 0.5);
    }

    #[test]
    fn a_button_held_from_the_menu_does_not_start_the_bike() {
        let pad = Entity::from_raw_u32(1).unwrap();
        let mut arming = PadArming::default();
        arming.track(false);
        // Cross confirmed the bike and is still held when the race appears.
        assert_eq!(arming.filter(pad, THROTTLE, 1.0), 0.0);
        assert_eq!(arming.filter(pad, THROTTLE, 1.0), 0.0);
        // Once released, the next press accelerates.
        assert_eq!(arming.filter(pad, THROTTLE, 0.0), 0.0);
        assert_eq!(arming.filter(pad, THROTTLE, 0.7), 0.7);
        // The brake is armed on its own.
        assert_eq!(arming.filter(pad, BRAKE, 0.0), 0.0);
        assert_eq!(arming.filter(pad, BRAKE, 0.4), 0.4);
    }

    #[test]
    fn a_trigger_that_does_not_read_zero_at_rest_does_not_start_the_bike() {
        let pad = Entity::from_raw_u32(1).unwrap();
        let mut arming = PadArming::default();
        arming.track(false);
        for _ in 0..100 {
            assert_eq!(arming.filter(pad, THROTTLE, 0.2), 0.0);
        }
    }

    #[test]
    fn a_trigger_resting_a_little_pressed_lets_go_of_the_throttle() {
        assert_eq!(squeezed(0.0), 0.0);
        assert_eq!(squeezed(TRIGGER_REST), 0.0);
        assert_eq!(squeezed(0.1), 0.0);
        assert!((squeezed(1.0) - 1.0).abs() < 1e-6);
        // Half way along what counts, half throttle.
        assert!((squeezed(TRIGGER_REST + (1.0 - TRIGGER_REST) / 2.0) - 0.5).abs() < 1e-6);
        // A button counts fully.
        assert_eq!(squeezed(1.0), 1.0);
        // Released like that, the engine brakes the bike.
        let tuning = BikeTuning::default();
        let rolling = BikeState {
            speed: 30.0,
            ..parked()
        };
        let input = BikeInput {
            throttle: squeezed(0.1),
            ..default()
        };
        let next = step(
            &rolling,
            &input,
            &Ground::flat(Surface::Asphalt),
            &tuning,
            DT,
        );
        assert!(next.speed < 30.0 - tuning.engine_braking * DT);
    }

    #[test]
    fn controls_must_be_released_again_for_a_new_race() {
        let pad = Entity::from_raw_u32(1).unwrap();
        let mut arming = PadArming::default();
        arming.track(false);
        assert_eq!(arming.filter(pad, THROTTLE, 0.0), 0.0);
        assert_eq!(arming.filter(pad, THROTTLE, 1.0), 1.0);
        arming.track(true);
        assert_eq!(arming.filter(pad, THROTTLE, 1.0), 1.0, "racing, it counts");
        // Restart (or a new race) while holding the throttle.
        arming.track(false);
        assert_eq!(arming.filter(pad, THROTTLE, 1.0), 0.0);
        assert_eq!(arming.filter(pad, THROTTLE, 0.0), 0.0);
        assert_eq!(arming.filter(pad, THROTTLE, 1.0), 1.0);
    }

    #[test]
    fn uphill_slows_and_downhill_speeds_up() {
        let tuning = BikeTuning::default();
        let moving = drive(full_throttle(), Surface::Asphalt, 3.0, parked());
        let coast = BikeInput::default();
        let on = |grade| {
            let ground = Ground {
                grade,
                ..Ground::flat(Surface::Asphalt)
            };
            step(&moving, &coast, &ground, &tuning, DT).speed
        };
        assert!(on(0.1) < on(0.0));
        assert!(on(-0.1) > on(0.0));
    }

    #[test]
    fn fast_over_a_crest_the_bike_flies_and_lands() {
        // Subiendo a una loma.
        let crest = Ground {
            crest: 0.005,
            grade: 0.02,
            ..Ground::flat(Surface::Asphalt)
        };
        // Despacio, sigue pegada al suelo.
        let mut slow = BikeState {
            speed: 30.0,
            ..parked()
        };
        settle(&mut slow, &crest, 0.0, DT);
        assert!(slow.grounded);

        // A 250 km/h hace falta más que la gravedad para seguir la loma.
        let mut fast = BikeState {
            speed: 70.0,
            ..parked()
        };
        settle(&mut fast, &crest, -0.01, DT);
        assert!(!fast.grounded);
        let mut steps = 0;
        while !fast.grounded {
            settle(&mut fast, &Ground::flat(Surface::Asphalt), -1.0, DT);
            steps += 1;
            assert!(steps < 1000, "no aterriza nunca");
        }
        assert_eq!(fast.elevation, -1.0);
    }

    #[test]
    fn in_the_air_the_bike_barely_steers_and_cannot_accelerate() {
        let tuning = BikeTuning::default();
        let input = BikeInput {
            throttle: 1.0,
            steer: 1.0,
            ..default()
        };
        let turn = |grounded: bool| {
            let mut state = BikeState {
                speed: 50.0,
                grounded,
                ..parked()
            };
            for _ in 0..60 {
                state = step(&state, &input, &Ground::flat(Surface::Asphalt), &tuning, DT);
            }
            state
        };
        let (flying, riding) = (turn(false), turn(true));
        assert!(flying.heading > 0.0, "algo se corrige");
        assert!(flying.heading < 0.4 * riding.heading);
        assert!(flying.speed <= 50.0);
    }

    #[test]
    fn a_jump_does_not_hang_in_the_air() {
        // A 230 km/h por una loma que hace despegar, y luego cuesta abajo.
        let mut state = BikeState {
            speed: 64.0,
            ..parked()
        };
        let crest = Ground {
            crest: 0.005,
            grade: 0.03,
            ..Ground::flat(Surface::Asphalt)
        };
        settle(&mut state, &crest, 0.0, DT);
        assert!(!state.grounded);
        let downhill = Ground {
            grade: -0.08,
            ..Ground::flat(Surface::Asphalt)
        };
        let mut steps = 0;
        while !state.grounded {
            let road = -0.08 * 64.0 * steps as f32 * DT;
            settle(&mut state, &downhill, road, DT);
            steps += 1;
        }
        let seconds = steps as f32 * DT;
        assert!(seconds < 0.7, "vuela {seconds} s");
    }

    #[test]
    fn a_wheelie_accelerates_a_little_more() {
        let tuning = BikeTuning::default();
        let wheelie = BikeInput {
            wheelie: 1.0,
            ..full_throttle()
        };
        let lifted = drive(wheelie, Surface::Asphalt, 60.0, parked());
        // A la misma velocidad, con la rueda en el aire gana velocidad algo
        // más deprisa que con las dos en el suelo.
        let accel = |chassis: Chassis| {
            let state = BikeState {
                speed: 40.0,
                chassis,
                ..parked()
            };
            let next = step(
                &state,
                &wheelie,
                &Ground::flat(Surface::Asphalt),
                &tuning,
                DT,
            );
            (next.speed - state.speed) / DT
        };
        let (up, down) = (accel(lifted.chassis), accel(Chassis::default()));
        assert!(
            up > down * 1.25 && up < down * 1.8,
            "{up} frente a {down} m/s²"
        );
    }

    #[test]
    fn a_wheelie_on_the_straight_is_faster_but_hardly_turns() {
        let tuning = BikeTuning::default();
        let flat_out = drive(full_throttle(), Surface::Asphalt, 60.0, parked());
        let wheelie = BikeInput {
            wheelie: 1.0,
            ..full_throttle()
        };
        let lifted = drive(wheelie, Surface::Asphalt, 60.0, parked());
        assert!(lifted.chassis.wheelie > 0.25, "rueda en el aire");
        assert!(
            lifted.speed > flat_out.speed + 1.5,
            "{} frente a {}",
            lifted.speed,
            flat_out.speed
        );

        // A la misma velocidad, con la rueda en el aire se gira mucho menos.
        let turn = BikeInput {
            steer: 1.0,
            ..wheelie
        };
        let yaw_rate = |chassis: Chassis| {
            let state = BikeState {
                speed: 40.0,
                steer: 1.0,
                chassis,
                ..parked()
            };
            let next = step(&state, &turn, &Ground::flat(Surface::Asphalt), &tuning, DT);
            (next.heading - state.heading) / DT
        };
        assert!(yaw_rate(lifted.chassis) < 0.3 * yaw_rate(Chassis::default()));
    }

    #[test]
    fn braking_dives_and_accelerating_squats() {
        let moving = drive(full_throttle(), Surface::Asphalt, 4.0, parked());
        assert!(moving.accel > 2.0 && moving.chassis.squat > 0.0);
        let braking = BikeInput {
            brake: 1.0,
            ..default()
        };
        let braked = drive(braking, Surface::Asphalt, 0.5, moving);
        assert!(braked.accel < -20.0);
        assert!(braked.chassis.squat < -0.04, "morro hundido");
    }

    #[test]
    fn a_booster_drives_the_bike_on_up_to_its_speed_on_the_ground() {
        let tuning = BikeTuning::default();
        let boosted = Ground {
            booster: 75.0,
            ..Ground::flat(Surface::Asphalt)
        };
        let flat = Ground::flat(Surface::Asphalt);
        let moving = BikeState {
            speed: 30.0,
            ..parked()
        };
        let ride = |ground: &Ground, input: BikeInput, state: BikeState, seconds: f32| {
            (0..(seconds / DT) as usize)
                .fold(state, |state, _| step(&state, &input, ground, &tuning, DT))
        };
        let plain = ride(&flat, full_throttle(), moving, 0.2).speed;
        let pushed = ride(&boosted, full_throttle(), moving, 0.2).speed;
        assert!(pushed > plain + 0.15 * BOOSTER_ACCEL, "{pushed} {plain}");
        // Coasting on it, it holds the bike at its speed, no faster.
        let held = ride(&boosted, BikeInput::default(), moving, 3.0).speed;
        assert!((74.0..=75.01).contains(&held), "{held}");
        // What goes faster it does not slow.
        let fast = BikeState {
            speed: 78.0,
            ..parked()
        };
        let through = ride(&boosted, full_throttle(), fast, 0.2).speed;
        assert!(through >= ride(&flat, full_throttle(), fast, 0.2).speed - 1e-3);
        // In the air it does nothing.
        let flying = BikeState {
            grounded: false,
            ..moving
        };
        let next = step(&flying, &full_throttle(), &boosted, &tuning, DT);
        assert!(next.speed <= flying.speed);
    }

    #[test]
    fn rolling_downhill_is_not_felt_as_acceleration() {
        let tuning = BikeTuning::default();
        let moving = drive(full_throttle(), Surface::Asphalt, 3.0, parked());
        // Bajando un 80 %: la gravedad empuja más de lo que frena el motor.
        let downhill = Ground {
            grade: -0.8,
            ..Ground::flat(Surface::Asphalt)
        };
        let next = step(&moving, &BikeInput::default(), &downhill, &tuning, DT);
        assert!(next.speed > moving.speed, "la cuesta empuja");
        assert!(next.accel < 0.0, "pero el piloto solo nota el freno motor");
    }

    #[test]
    fn landing_a_jump_compresses_the_suspension() {
        let tuning = BikeTuning::default();
        let mut state = BikeState {
            speed: 40.0,
            elevation: 3.0,
            grounded: false,
            ..parked()
        };
        let ground = Ground::flat(Surface::Asphalt);
        let mut deepest = 0.0f32;
        for _ in 0..240 {
            state = step(&state, &BikeInput::default(), &ground, &tuning, DT);
            settle(&mut state, &ground, 0.0, DT);
            deepest = deepest.min(state.chassis.heave);
        }
        assert!(state.grounded);
        assert!(deepest < -0.06, "comprime {deepest}");
        assert!(state.chassis.heave.abs() < 0.01, "y se recupera");
    }

    #[test]
    fn hitting_a_wall_at_an_angle_turns_the_bike_along_it() {
        // 30° contra el muro izquierdo (que queda hacia -X).
        let (track, mut state) = against_left_wall(0.5);
        let before = state.heading;
        for _ in 0..12 {
            hit_walls(&mut state, &track);
            state.position += forward(state.heading) * state.speed * DT;
        }
        assert!(
            state.heading < before * 0.2,
            "paralela al muro: {}",
            state.heading
        );
        assert!(state.speed > 15.0, "y sigue: {}", state.speed);
    }

    #[test]
    fn holding_full_lock_fast_sheds_speed_to_make_the_corner() {
        let tuning = BikeTuning::default();
        let fast = BikeState {
            speed: 70.0,
            ..parked()
        };
        let input = |steer| BikeInput {
            throttle: 1.0,
            steer,
            ..default()
        };
        let straight = drive(input(0.0), Surface::Asphalt, 2.0, fast);
        let turning = drive(input(1.0), Surface::Asphalt, 2.0, fast);
        assert!(turning.speed < straight.speed - 10.0);
        // Despacio no frena: solo gira.
        let slow = BikeState {
            speed: 15.0,
            ..parked()
        };
        let next = step(
            &slow,
            &input(1.0),
            &Ground::flat(Surface::Asphalt),
            &tuning,
            DT,
        );
        assert!(next.speed >= slow.speed);
    }

    /// Lo que pide un juego arcade: a fondo, sin frenar nunca y solo girando
    /// hacia la carretera, se da la vuelta sin tocar los muros.
    #[test]
    fn moderate_corners_flat_out_gain_speed() {
        let fast = BikeState {
            speed: 60.0,
            ..parked()
        };
        let input = |steer| BikeInput {
            throttle: 1.0,
            steer,
            ..default()
        };
        let straight = drive(input(0.0), Surface::Asphalt, 2.0, fast);
        // También al 80 % al que llega el teclado yendo deprisa.
        for steer in [0.3, 0.5, 0.65, 0.8] {
            let turning = drive(input(steer), Surface::Asphalt, 2.0, fast);
            assert!(
                turning.speed > straight.speed,
                "girando {steer}: {} frente a {} en recta",
                turning.speed,
                straight.speed
            );
        }
    }

    pub fn flat_out_steering_only_laps_without_touching_the_walls_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            flat_out_lap(&Track::new(&circuit.layout), circuit.name);
        }
    }

    #[test]
    fn flat_out_steering_only_laps_without_touching_the_walls() {
        flat_out_steering_only_laps_without_touching_the_walls_on(&crate::testing::DEMO);
    }

    fn flat_out_lap(track: &Track, name: &str) {
        let tuning = BikeTuning::default();
        let mut state = BikeState::parked_on(track, track.grid_pose(0, 0.0));
        let n = track.sample_count();
        let here = |state: &BikeState| track.nearest_index_at(state.position, state.elevation);
        let (mut progress, mut last) = (0i64, here(&state) as i64);
        let mut on_grass = 0;
        // Long enough for a lap of the longest track.
        let steps = (100.0 / DT) as usize;
        for _ in 0..steps {
            // Mira 18 m por delante de la línea central y gira hacia allí.
            let ahead = track.offset_point(here(&state) + 9, 0.0);
            let angle = wrap_angle(heading_of(ahead - state.position) - state.heading);
            let input = BikeInput {
                throttle: 1.0,
                steer: (angle * 3.0).clamp(-1.0, 1.0),
                ..default()
            };
            state = advance(&state, &input, track, &tuning, DT);
            let nearest = track.nearest_at(state.position, state.elevation);
            let distance = nearest.distance;
            assert!(
                distance < track.edge_beside(&nearest) - BIKE_HALF_WIDTH - 0.01,
                "toca el muro de {name} en {:?} (muestra {}) a {:.0} km/h",
                state.position,
                here(&state),
                state.speed * 3.6
            );
            if distance > track.half_width() {
                on_grass += 1;
            }
            let index = here(&state) as i64;
            progress += (index - last + n as i64 * 3 / 2) % n as i64 - n as i64 / 2;
            last = index;
        }
        assert!(
            progress > n as i64,
            "da la vuelta a {name}: {progress} de {n}"
        );
        assert!(
            on_grass < steps / 20,
            "fuera del asfalto de {name} {on_grass} pasos"
        );
    }

    /// How far the part of the bike deepest into a wall is into it (m).
    fn into_wall(track: &Track, state: &BikeState) -> f32 {
        footprint(state)
            .iter()
            .map(|&(point, radius)| track.distance_to_centerline(point) + radius - WALL_DISTANCE)
            .fold(f32::MIN, f32::max)
    }

    #[test]
    fn no_part_of_the_bike_goes_into_a_wall_it_hits_at_an_angle() {
        for heading in [0.3, 0.8, FRAC_PI_2] {
            // Fast enough to cross most of a metre in a step.
            let (track, mut state) = against_left_wall(heading);
            state.position = track.offset_point(50, WALL_DISTANCE - 1.0);
            state.speed = 80.0;
            for _ in 0..30 {
                state.position += forward(state.heading) * state.speed * DT;
                hit_walls(&mut state, &track);
                assert!(into_wall(&track, &state) < 1e-3, "{heading}: {state:?}");
            }
        }
    }

    #[test]
    fn a_rider_leaning_towards_a_wall_does_not_go_into_it() {
        // Along the left wall, leaning left at full lean.
        let (track, mut state) = against_left_wall(0.0);
        state.position = track.offset_point(50, WALL_DISTANCE - BIKE_HALF_WIDTH);
        state.lean = BikeTuning::default().max_lean;
        let hit = hit_walls(&mut state, &track).expect("the rider touches the wall");
        assert!(into_wall(&track, &state) < 1e-3);
        assert!(
            hit.into.abs() < 1e-3,
            "riding along it is not a hit: {hit:?}"
        );
        let gap = WALL_DISTANCE - track.distance_to_centerline(state.position);
        assert!(gap > 0.8, "the bike keeps the rider off the wall: {gap} m");
    }

    #[test]
    fn upright_along_a_wall_the_bike_gets_as_close_as_before() {
        let (track, mut state) = against_left_wall(0.0);
        state.position = track.offset_point(50, WALL_DISTANCE - 0.3);
        state.lean = 0.0;
        hit_walls(&mut state, &track);
        let gap = WALL_DISTANCE - track.distance_to_centerline(state.position);
        assert!((gap - BIKE_HALF_WIDTH).abs() < 1e-3, "{gap}");
    }

    #[test]
    fn a_bike_pushed_by_another_into_a_wall_stays_out_of_it() {
        let track = Track::default();
        let (_, heading) = track.pose_at(50);
        let left = -forward(heading).perp();
        let wall_side = track.offset_point(50, WALL_DISTANCE - BIKE_HALF_WIDTH);
        let mut against = BikeState {
            position: wall_side,
            heading,
            speed: 30.0,
            lean: 0.0,
            elevation: track.road_height(wall_side),
            ..parked()
        };
        let mut pusher = BikeState {
            position: wall_side - left * 0.6,
            ..against
        };
        let (mut ka, mut kb) = (Knock::default(), Knock::default());
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        world.insert_resource(Track::default());
        let a = world.spawn((against, ka, CrashState::default())).id();
        let b = world.spawn((pusher, kb, CrashState::default())).id();
        world.run_system_once(separate_bikes).unwrap();
        (against, pusher) = (
            *world.get::<BikeState>(a).unwrap(),
            *world.get::<BikeState>(b).unwrap(),
        );
        (ka, kb) = (
            *world.get::<Knock>(a).unwrap(),
            *world.get::<Knock>(b).unwrap(),
        );
        assert!(into_wall(&track, &against) < 1e-3, "{against:?}");
        assert!(against.position.distance(pusher.position) > 0.6);
        assert_eq!((ka, kb), (Knock::default(), Knock::default()));
    }

    #[test]
    fn earth_rising_before_the_wall_stops_the_bike() {
        let (mut track, mut state) = against_left_wall(FRAC_PI_2);
        // Earth 2 m before the left wall around the 50th point.
        let mut edges = vec![[WALL_DISTANCE; 2]; track.sample_count()];
        for sides in &mut edges[45..=55] {
            sides[0] = WALL_DISTANCE - 2.0;
        }
        track.set_edges(vec![edges]);
        let hit = hit_walls(&mut state, &track).expect("hits the earth");
        assert!(hit.into > 30.0, "head on: {hit:?}");
        for &(point, radius) in &footprint(&state) {
            let reach = track.distance_to_centerline(point) + radius;
            assert!(reach < WALL_DISTANCE - 2.0 + 1e-3, "{reach}");
        }
        // Elsewhere the wall is where it was.
        let (_, mut far) = against_left_wall(FRAC_PI_2);
        far.position = track.offset_point(150, WALL_DISTANCE + 1.0);
        hit_walls(&mut far, &track);
        let gap = WALL_DISTANCE - track.distance_to_centerline(far.position);
        assert!(gap < 1.1, "{gap}");
    }

    #[test]
    fn scraping_along_a_wall_keeps_most_of_the_speed() {
        // Casi paralelo al muro, apuntando un poco hacia él.
        let (track, mut state) = against_left_wall(0.1);
        hit_walls(&mut state, &track);
        assert!(state.speed > 35.0);
    }

    #[test]
    fn stopped_bike_leans_and_straightens_with_speed() {
        let tuning = BikeTuning::default();
        let parked = BikeState::parked_at((Vec2::ZERO, 0.0));
        assert_eq!(parked.lean, STOPPED_LEAN);

        let still = step(
            &parked,
            &BikeInput::default(),
            &Ground::flat(Surface::Asphalt),
            &tuning,
            1.0,
        );
        assert!((still.lean - STOPPED_LEAN).abs() < 1e-4);

        let mut moving = parked;
        let input = BikeInput {
            throttle: 1.0,
            ..default()
        };
        for _ in 0..(2.0 / DT) as usize {
            moving = step(
                &moving,
                &input,
                &Ground::flat(Surface::Asphalt),
                &tuning,
                DT,
            );
        }
        assert!(moving.speed > STOPPED_SPEED);
        assert!(moving.lean.abs() < 0.02);
    }
}
