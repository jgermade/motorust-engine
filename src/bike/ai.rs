//! Pilotos rivales: siguen su carril a lo largo del circuito, frenan antes de
//! las curvas y aceleran a la salida. Si alcanzan a otra moto que va por su
//! trazada, se abren hacia el lado con sitio para adelantarla, y si no caben,
//! levantan el gas en vez de embestirla; luego vuelven a su carril. Conducen
//! con los mismos mandos ([`BikeInput`]) y la misma física que el jugador.
//! Some take the branches off the lap (see [`crate::track::Branch`]) and go
//! round their loops, flat out, keeping to their line across the running
//! surface; a rival pushed onto a branch it does not take follows it too,
//! back to the lap. Where the road splits without a loop, each rival rides
//! the branch on the side of its lane, or the lap.

use bevy::prelude::*;

use super::{BikeInput, BikeState, BikeTuning, Race, Ride, move_toward};
use crate::geometry::{heading_of, wrap_angle};
use crate::track::{Fork, SAMPLE_SPACING, Track};

/// Distancia a la que mira por delante para apuntar: fija más la que recorre
/// en este tiempo. Mirar más lejos a más velocidad evita los eses.
const LOOK_AHEAD_DISTANCE: f32 = 8.0;
const LOOK_AHEAD_TIME: f32 = 0.4;
/// Where the road grips harder (see [`Track::grip_at`]), the most of it a
/// rival plans its speed round a bend with.
const PLANNED_GRIP: f32 = 1.6;
/// Fracción del freno con la que planea las frenadas, para llegar con margen.
const PLANNED_BRAKING: f32 = 0.6;
/// How much more of the brakes a rival plans with for each bit of skill
/// past 1 (only on very hard): it brakes later.
const DARING_BRAKING: f32 = 2.0;
/// Metros extra que revisa por delante además de su distancia de frenado.
const EXTRA_HORIZON: f32 = 25.0;
/// Puntos a cada lado con los que suaviza la curvatura del trazado.
const CURVATURE_WINDOW: usize = 3;
/// Ve a las motos de delante a esta distancia más lo que recorre en este
/// tiempo (m, s).
const TRAFFIC_DISTANCE: f32 = 6.0;
const TRAFFIC_TIME: f32 = 1.6;
/// Separación de lado a lado para pasar junto a otra moto (m), y lo que deja
/// hasta el borde del asfalto.
const CLEARANCE: f32 = 2.1;
const EDGE_MARGIN: f32 = 1.0;
/// Lo deprisa que cambia de trazada (m/s), más cuanto más deprisa va.
const LINE_RATE: f32 = 2.0;
const LINE_RATE_PER_SPEED: f32 = 0.03;
/// Si no puede pasar, se queda a esta distancia de la de delante (m).
const FOLLOW_GAP: f32 = 4.0;
/// A bike from this far behind to this far ahead (m) is beside a rival: it
/// does not move across into it. Off the grid, where the bikes set off side
/// by side, it keeps to its side of the road until there is room.
const BESIDE: f32 = 6.0;
/// Over the last this many metres of a branch without a loop (m), a rival
/// gives way to a bike on the lap from this far behind to this far ahead
/// (m), from where it is now to where it will be by the time the rival
/// gets to the lap (so that one catching it up from further behind counts
/// too), and within this much more than the clearance across (m): going
/// this much slower than it (m/s), or slower still for it to be that far
/// ahead by then, though no more than twice as much slower.
const MERGE_ZONE: f32 = 120.0;
const MERGE_BEHIND: f32 = 4.0;
const MERGE_AHEAD: f32 = 15.0;
const MERGE_MARGIN: f32 = 1.5;
const MERGE_YIELD: f32 = 8.0;
/// Round a loop, how long a rival takes to get back to its line across the
/// running surface (s), how far it turns for that at most (rad), and how
/// long it takes to point the way it wants to (s).
const LOOP_LINE_TIME: f32 = 1.0;
const LOOP_MAX_YAW: f32 = 0.15;
const LOOP_TURN_TIME: f32 = 0.3;

/// Un rival controlado por la máquina.
#[derive(Component, Clone, Copy, Debug)]
pub struct Rival {
    /// Fracción de la punta y del agarre de la moto que se atreve a usar (0..1];
    /// past 1 only on very hard (see [`decide`]).
    pub skill: f32,
    /// Distancia a la izquierda de la línea central por la que va (m).
    pub lane: f32,
    /// Whether it takes the branches off the lap and rides their loops (and
    /// every branch without one).
    pub loops: bool,
}

/// How far to the side of the middle of the lap a rival's lane must be for
/// it to take a branch without a loop on that side (m).
const FORK_LANE: f32 = 2.0;

impl Rival {
    /// Whether it rides `fork` rather than the lap beside it: if it takes
    /// the branches, or if its lane is on that side.
    pub fn takes(&self, fork: &Fork) -> bool {
        self.loops || self.lane * fork.side >= FORK_LANE
    }

    /// Whether it rides road `road` (see [`crate::track::Nearest::road`])
    /// where it shares the lap's.
    fn rides(&self, track: &Track, road: usize) -> bool {
        match track.forks().iter().find(|fork| fork.road == road) {
            Some(fork) => self.takes(fork),
            None => self.loops,
        }
    }
}

/// A point of a rival's way ahead.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Waypoint {
    /// Point `index` of the centre line of road `road` (see
    /// [`crate::track::Nearest::road`]).
    Road(usize, usize),
    /// Straight on into loop `loop` (see [`Track::loops`]), this many metres
    /// past its mouth: round it, it keeps to its line (see [`loop_steer`]).
    Into(usize, f32),
}

impl Waypoint {
    /// Where a rival riding `state` is on its way: the point of the road it
    /// rides nearest to it. A rival that does not take the branches keeps
    /// to the lap where they share its road.
    fn start(track: &Track, state: &BikeState, rival: &Rival) -> Self {
        let lap = track.nearest_index_at(state.position, state.elevation);
        let nearest = track.nearest_at(state.position, state.elevation);
        let off_lap = state.position.distance(track.centerline()[lap]) > track.wall_at(lap);
        if nearest.road != 0 && (rival.rides(track, nearest.road) || off_lap) {
            Waypoint::Road(nearest.road, nearest.index)
        } else {
            Waypoint::Road(0, lap)
        }
    }

    /// The next point on the way, about [`SAMPLE_SPACING`] further on.
    fn next(self, track: &Track, rival: &Rival) -> Self {
        match self {
            Waypoint::Road(0, index) => {
                let index = (index + 1) % track.sample_count();
                let looped = track.loops().iter().find(|looped| looped.fork == index);
                let fork = track
                    .forks()
                    .iter()
                    .find(|fork| fork.fork == index && rival.takes(fork));
                match (looped, fork) {
                    (Some(looped), _) if rival.loops => Waypoint::Road(looped.approach, 0),
                    (_, Some(fork)) => Waypoint::Road(fork.road, 0),
                    _ => Waypoint::Road(0, index),
                }
            }
            Waypoint::Road(road, index) if index + 1 < track.road(road).sample_count() => {
                Waypoint::Road(road, index + 1)
            }
            Waypoint::Road(road, _) => {
                let loops = track.loops();
                match loops.iter().position(|looped| looped.approach == road) {
                    Some(looped) => Waypoint::Into(looped, SAMPLE_SPACING),
                    None => {
                        let join = loops
                            .iter()
                            .find(|looped| looped.exit == road)
                            .map(|looped| looped.join)
                            .or_else(|| {
                                let forks = track.forks();
                                forks.iter().find(|fork| fork.road == road).map(|f| f.join)
                            });
                        Waypoint::Road(0, join.unwrap_or(0))
                    }
                }
            }
            Waypoint::Into(looped, past) => Waypoint::Into(looped, past + SAMPLE_SPACING),
        }
    }

    /// The point `lateral` metres to the left of the middle of the road here.
    fn point(self, track: &Track, lateral: f32) -> Vec2 {
        match self {
            Waypoint::Road(road, index) => track.road(road).offset_point(index, lateral),
            Waypoint::Into(looped, past) => {
                let looped = &track.loops()[looped];
                looped.mouth + looped.ahead * past + looped.left() * lateral
            }
        }
    }

    /// How sharp the road is here (1/m, see [`Track::smoothed_curvature`]).
    fn curvature(self, track: &Track) -> f32 {
        match self {
            Waypoint::Road(road, index) => {
                track.road(road).smoothed_curvature(index, CURVATURE_WINDOW)
            }
            Waypoint::Into(..) => 0.0,
        }
    }

    /// How much harder than usual the road grips across here (see
    /// [`Track::grip_at`]).
    fn grip(self, track: &Track) -> f32 {
        match self {
            Waypoint::Road(road, index) => track.grip_at(road, index),
            Waypoint::Into(..) => 1.0,
        }
    }

    /// Half the width of the asphalt here (m), short of the walls where they
    /// close in on it.
    fn half_width(self, track: &Track) -> f32 {
        match self {
            Waypoint::Road(road, index) => track.road(road).half_width_at(index),
            Waypoint::Into(looped, _) => track.road(track.loops()[looped].approach).half_width(),
        }
    }

    /// The way ahead of a rival riding `state`, `count` points of it.
    fn ahead(track: &Track, state: &BikeState, rival: &Rival, count: usize) -> Vec<Self> {
        let mut way = Vec::with_capacity(count);
        let mut here = Self::start(track, state, rival);
        for _ in 0..count {
            way.push(here);
            here = here.next(track, rival);
        }
        way
    }
}

/// Por dónde va un rival ahora mismo (m a la izquierda de la línea central):
/// su carril o, para adelantar, a un lado.
#[derive(Component, Clone, Copy, Debug)]
pub struct Line(pub f32);

/// Otra moto de la carrera, como la ve un rival.
#[derive(Clone, Copy, Debug)]
pub struct Traffic {
    pub position: Vec2,
    /// How high it is (m): where the track passes over itself, a bike on
    /// the other stretch is not in the way.
    pub elevation: f32,
    pub speed: f32,
}

#[allow(clippy::type_complexity)]
pub fn drive_rivals(
    time: Res<Time>,
    race: Res<Race>,
    tuning: Res<BikeTuning>,
    track: Res<Track>,
    bikes: Query<(Entity, &BikeState)>,
    mut rivals: Query<(
        Entity,
        &Rival,
        &BikeState,
        &mut Line,
        &mut BikeInput,
        &super::CrashState,
    )>,
) {
    let traffic: Vec<(Entity, Traffic)> = bikes
        .iter()
        .map(|(entity, state)| {
            let traffic = Traffic {
                position: state.position,
                elevation: state.elevation,
                speed: state.speed,
            };
            (entity, traffic)
        })
        .collect();
    for (entity, rival, state, mut line, mut input, crash) in &mut rivals {
        // Down on the track, the rider lets go of everything.
        if !race.started || crash.is_down() {
            *input = BikeInput::default();
            continue;
        }
        let others: Vec<Traffic> = traffic
            .iter()
            .filter(|(other, _)| *other != entity)
            .map(|(_, traffic)| *traffic)
            .collect();
        let (target, limit) = plan_line(rival, line.0, state, &track, &others);
        let rate = LINE_RATE + LINE_RATE_PER_SPEED * state.speed.abs();
        line.0 = move_toward(line.0, target, rate * time.delta_secs());
        *input = decide(rival, line.0, limit, state, &track, &tuning);
    }
}

/// Hacia qué trazada ir con el tráfico que tiene delante, y a qué velocidad
/// como mucho. Si su carril está libre, vuelve a él; si no, sigue por donde
/// va mientras esté libre; si no, se abre al hueco libre más cercano junto a
/// las motos que le estorban. Si no hay hueco, sigue a la de delante.
pub fn plan_line(
    rival: &Rival,
    line: f32,
    state: &BikeState,
    track: &Track,
    traffic: &[Traffic],
) -> (f32, f32) {
    let n = track.sample_count() as i64;
    let here = track.nearest_index_at(state.position, state.elevation) as i64;
    let speed = state.speed.max(0.0);
    let horizon = TRAFFIC_DISTANCE + speed * TRAFFIC_TIME;
    // Las motos que tiene delante (o al lado) y a las que se acerca: cuánto
    // por delante, por dónde van y a qué velocidad.
    // Where they are across the road it rides, so that one coming off
    // another road where the two meet is seen where it is.
    let way = Waypoint::start(track, state, rival);
    let across = |other: &Traffic| match way {
        Waypoint::Road(road, index) => {
            let line = track.road(road);
            let reach = (horizon / line.step_length()).ceil() as usize + 2;
            // From a little behind, for the bikes beside it.
            let behind = (BESIDE / line.step_length()).ceil() as usize + 1;
            let (from, count) = if line.is_closed() {
                (
                    (index + line.sample_count() - behind) % line.sample_count(),
                    reach + behind,
                )
            } else {
                let from = index.saturating_sub(behind).min(line.segment_count() - 1);
                (from, (reach + behind).min(line.segment_count() - from))
            };
            line.nearest_among(other.position, from, count).lateral
        }
        Waypoint::Into(..) => track.nearest_at(other.position, other.elevation).lateral,
    };
    let gap_to = |other: &Traffic| {
        let index = track.nearest_index_at(other.position, other.elevation) as i64;
        ((index - here + n + n / 2) % n - n / 2) as f32 * SAMPLE_SPACING
    };
    let ahead: Vec<(f32, f32, f32)> = traffic
        .iter()
        .filter_map(|other| {
            let gap = gap_to(other);
            let closing = speed - other.speed;
            (gap > -SAMPLE_SPACING && gap < horizon && (closing > 0.5 || gap < TRAFFIC_DISTANCE))
                .then(|| (gap, across(other), other.speed))
        })
        .collect();
    // Where the bikes beside it are across the road.
    let alongside: Vec<f32> = traffic
        .iter()
        .filter(|other| gap_to(other).abs() <= BESIDE)
        .map(across)
        .collect();
    // Coming off a branch without a loop, it joins the lap on its line
    // there: it gives way to the bikes on the lap about level with it and
    // near that line, dropping in behind them.
    if let Waypoint::Road(road, index) = way
        && track.forks().iter().any(|fork| fork.road == road)
    {
        let branch = track.road(road);
        let last = branch.sample_count() - 1;
        let to_join = (last - index.min(last)) as f32 * branch.step_length();
        // How far ahead of it (m, behind if negative) a bike on the lap
        // `gap` ahead now will be when it gets to the lap.
        let joining = to_join / speed.max(1.0);
        let at_join = |gap: f32, other: &Traffic| gap + (other.speed - speed) * joining;
        let quickest = traffic
            .iter()
            .map(|other| other.speed)
            .fold(speed, f32::max);
        let back = MERGE_BEHIND + (quickest - speed) * joining.min(MERGE_ZONE / speed.max(1.0));
        let before = (back / track.step_length()).ceil() as usize + 4;
        let reach = (horizon / track.step_length()).ceil() as usize + before;
        let on_lap = |other: &Traffic| {
            let from = (here as usize + track.sample_count() - before) % track.sample_count();
            track.nearest_among(other.position, from, reach).lateral
        };
        let yielding = traffic
            .iter()
            .filter_map(|other| {
                let index = track.nearest_index_at(other.position, other.elevation) as i64;
                let gap = ((index - here + n + n / 2) % n - n / 2) as f32 * SAMPLE_SPACING;
                // Level with it at any time until then, on the lap (one on
                // this branch with it is traffic like any other).
                let (near, far) = (gap.min(at_join(gap, other)), gap.max(at_join(gap, other)));
                let level = far > -MERGE_BEHIND
                    && near < MERGE_AHEAD
                    && track.nearest_at(other.position, other.elevation).road != road
                    && (on_lap(other) - line).abs() < CLEARANCE + MERGE_MARGIN;
                // Slow enough that it is far enough ahead by then, though
                // no more than twice as much slower as by default.
                let clear = other.speed * to_join / (MERGE_AHEAD + to_join - gap).max(1.0);
                level.then(|| {
                    (other.speed - MERGE_YIELD)
                        .min(clear)
                        .max(other.speed - 2.0 * MERGE_YIELD)
                })
            })
            .reduce(f32::min);
        if let (true, Some(yielding)) = (to_join < MERGE_ZONE, yielding) {
            return (line, yielding.max(0.0));
        }
    }
    let limit = way.half_width(track) - EDGE_MARGIN;
    let free = |lateral: f32| {
        lateral.abs() <= limit
            && ahead
                .iter()
                .all(|&(_, other, _)| (other - lateral).abs() >= CLEARANCE)
    };
    let wanted = if free(rival.lane) {
        rival.lane
    } else if free(line) {
        line
    } else {
        ahead
            .iter()
            .flat_map(|&(_, other, _)| [other + CLEARANCE, other - CLEARANCE])
            .filter(|&lateral| free(lateral))
            .min_by(|a, b| (a - line).abs().total_cmp(&(b - line).abs()))
            .unwrap_or(line)
    };
    // Only as far across as the bikes beside it leave room for.
    let target = across_to(line, wanted, &alongside);
    if free(target) {
        return (target, f32::INFINITY);
    }
    // Sin hueco: se pone a la velocidad de la que tiene delante.
    let leader = ahead
        .iter()
        .filter(|&&(gap, other, _)| gap > 0.0 && (other - target).abs() < CLEARANCE)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    match leader {
        Some(&(gap, _, leader_speed)) => {
            (target, leader_speed.max(0.0) + (gap - FOLLOW_GAP).max(0.0))
        }
        None => (target, f32::INFINITY),
    }
}

/// How far a rival on `line` gets towards `wanted` (m to the left of the
/// middle of the road) with bikes beside it at `alongside`: up to the
/// clearance short of the first one on that side, and never back.
fn across_to(line: f32, wanted: f32, alongside: &[f32]) -> f32 {
    let way = wanted - line;
    alongside.iter().fold(wanted, |to, &other| {
        let off = other - line;
        if off * way <= 0.0 || off.abs() >= way.abs() + CLEARANCE {
            return to;
        }
        let short = other - CLEARANCE * way.signum();
        if way > 0.0 {
            to.min(short.max(line))
        } else {
            to.max(short.min(line))
        }
    })
}

/// Lo que hace un rival en este paso, yendo por `line` (m a la izquierda de
/// la línea central) sin pasar de `limit` (m/s). Es una función pura: se
/// puede probar sin ECS y da siempre lo mismo para la misma situación.
pub fn decide(
    rival: &Rival,
    line: f32,
    limit: f32,
    state: &BikeState,
    track: &Track,
    tuning: &BikeTuning,
) -> BikeInput {
    // Round a loop, flat out, keeping to its line.
    if let Some(ride) = state.ride {
        return BikeInput {
            throttle: 1.0,
            steer: loop_steer(&ride, line, state.speed, track, tuning),
            ..default()
        };
    }
    let speed = state.speed.max(0.0);
    // Past 1 (on very hard), the corners harder (the bike grips more with
    // the steering near its stop) and the brakes later; never faster than
    // the bike goes.
    let grip = tuning.max_lateral_accel * rival.skill;
    let daring = (rival.skill - 1.0).max(0.0);
    let braking = tuning.brake_decel * (PLANNED_BRAKING + DARING_BRAKING * daring);
    let horizon = speed * speed / (2.0 * braking) + EXTRA_HORIZON;
    let ahead = ((LOOK_AHEAD_DISTANCE + speed * LOOK_AHEAD_TIME) / SAMPLE_SPACING).round() as usize;
    let steps = (horizon / SAMPLE_SPACING) as usize;
    let way = Waypoint::ahead(track, state, rival, ahead.max(steps) + 1);

    // Dirección: persecución pura de un punto de su carril por delante. La
    // curvatura que lleva hasta él, dividida entre la máxima que permite la
    // física a esta velocidad, es la dirección a aplicar.
    let target = way[ahead];
    let line = line.clamp(
        EDGE_MARGIN - target.half_width(track),
        target.half_width(track) - EDGE_MARGIN,
    );
    let to_target = target.point(track, line) - state.position;
    let angle = wrap_angle(heading_of(to_target) - state.heading);
    let wanted_curvature = 2.0 * angle.sin() / to_target.length().max(1.0);
    // Where the road grips harder (the toy cars' banked track), the same
    // steering turns it more, as the physics has it.
    let held = way[0].grip(track);
    let max_curvature = (1.0 / tuning.min_turn_radius)
        .min(tuning.max_lateral_accel * held / speed.max(0.1).powi(2));
    let steer = (wanted_curvature / max_curvature).clamp(-1.0, 1.0);

    // Velocidad: la más baja que exijan las curvas de delante, contando con
    // que le dé tiempo a frenar hasta ellas.
    let mut target_speed = (tuning.max_speed * rival.skill.min(1.0)).min(limit);
    for (step, waypoint) in way.iter().enumerate().take(steps) {
        let curvature = waypoint.curvature(track);
        if curvature < 1e-4 {
            continue;
        }
        // Where the road grips harder, faster, but with room: their
        // steering turns in late into a tight bend taken flat out.
        let held = waypoint.grip(track).min(PLANNED_GRIP);
        let corner_speed = (grip * held / curvature).sqrt();
        let distance = step as f32 * SAMPLE_SPACING;
        target_speed = target_speed.min((corner_speed.powi(2) + 2.0 * braking * distance).sqrt());
    }

    BikeInput {
        throttle: ((target_speed - speed) / 2.0).clamp(0.0, 1.0),
        brake: ((speed - target_speed - 1.0) / 3.0).clamp(0.0, 1.0),
        steer,
        steer_direct: 0.0,
        // Los rivales no hacen caballitos: van a por el tiempo.
        wheelie: 0.0,
        pivot_slowing: 0.0,
        lean: None,
        // Its steering does not reckon with the front's extra grip braking.
        plain_grip: true,
        look: super::curves::LeanLook::Real,
    }
}

/// How a rival riding round a loop as `ride` says at `speed` steers to get
/// to its `line` (m to the left of the middle of the running surface, as on
/// the road in): it points towards it, gently, and straight on again as it
/// gets there.
fn loop_steer(ride: &Ride, line: f32, speed: f32, track: &Track, tuning: &BikeTuning) -> f32 {
    let half = Waypoint::Into(ride.index, 0.0).half_width(track) - EDGE_MARGIN;
    let speed = speed.max(1.0);
    let wanted = ((line.clamp(-half, half) - ride.lateral) / (speed * LOOP_LINE_TIME))
        .clamp(-LOOP_MAX_YAW, LOOP_MAX_YAW);
    ((wanted - ride.yaw) / (LOOP_TURN_TIME * tuning.max_yaw_rate(speed))).clamp(-1.0, 1.0)
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;
    use crate::bike::crash::{
        CrashState, fall, fall_against_wall, rejoin_pose, slide, wall_knocks_down,
    };
    use crate::bike::{BikeState, Body, Knock, advance, advance_hitting_walls};
    use crate::settings::Difficulty;

    pub fn rival_laps_the_track_without_leaving_it_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let tuning = BikeTuning::default();
            let rival = Rival {
                skill: 0.95,
                lane: 2.0,
                loops: false,
            };
            let mut state = BikeState::parked_on(&track, track.grid_pose(0, rival.lane));
            let dt = 1.0 / 120.0;
            let n = track.sample_count() as i64;
            let mut progress = 0;
            let mut last_index = track.nearest_index_at(state.position, state.elevation) as i64;
            for _ in 0..(90.0 / dt) as usize {
                let input = decide(&rival, rival.lane, f32::INFINITY, &state, &track, &tuning);
                state = advance(&state, &input, &track, &tuning, dt);
                assert!(
                    track.nearest_at(state.position, state.elevation).distance < 8.5,
                    "se ha salido de {} en {:?}",
                    circuit.name,
                    state.position
                );
                // Avance con signo, teniendo en cuenta que el índice da la vuelta.
                let index = track.nearest_index_at(state.position, state.elevation) as i64;
                progress += (index - last_index + n + n / 2) % n - n / 2;
                last_index = index;
            }
            assert!(
                progress > n,
                "debería dar al menos una vuelta a {} en 90 s",
                circuit.name
            );
        }
    }

    #[test]
    fn rival_laps_the_track_without_leaving_it() {
        rival_laps_the_track_without_leaving_it_on(&crate::testing::DEMO);
    }

    /// Rides a rival of `skill` round `circuit` from the grid for `laps`
    /// laps: the seconds it takes over each stretch between checkpoints
    /// (see [`crate::race::checkpoints`]), the first from the start.
    fn checkpoint_times(circuit: &crate::circuits::Circuit, skill: f32, laps: usize) -> Vec<f32> {
        let track = Track::new(&circuit.layout);
        let lap = track.lap_length();
        let points = crate::race::checkpoints(&circuit.checkpoints(), lap);
        let tuning = BikeTuning::default();
        let rival = Rival {
            skill,
            lane: 0.0,
            loops: false,
        };
        let mut state = BikeState::parked_on(&track, track.grid_pose(0, rival.lane));
        let mut progress = crate::race::Progress::default();
        let dt = 1.0 / 120.0;
        // From a standing start on the grid, as against the clock.
        let (mut clock, mut since) = (0.0, 0.0);
        let mut times = Vec::new();
        while times.len() < laps * points.len() && clock < 600.0 {
            let input = decide(&rival, rival.lane, f32::INFINITY, &state, &track, &tuning);
            state = advance(&state, &input, &track, &tuning, dt);
            clock += dt;
            let nearest = track.nearest_at(state.position, state.elevation);
            if progress
                .pass(track.along(&nearest), lap, &points)
                .checkpoint
            {
                times.push(clock - since);
                since = clock;
            }
        }
        times
    }

    pub fn against_the_clock_a_good_ride_earns_a_little_time_at_each_checkpoint_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        use crate::race::{CheckpointTimes, checkpoints, stretch_after};
        for circuit in circuits.iter() {
            let lap = Track::new(&circuit.layout).lap_length();
            let points = checkpoints(&circuit.checkpoints(), lap);
            let times = checkpoint_times(circuit, 0.9, 1);
            assert_eq!(times.len(), points.len(), "{}", circuit.name);
            // On very hard, a ride as good as its rivals'.
            let very_hard = checkpoint_times(circuit, Difficulty::VeryHard.rival_skill(0.9), 1);
            for (difficulty, earns) in [
                (Difficulty::Medium, 1.0..5.0),
                (Difficulty::Hard, 0.3..3.5),
                (Difficulty::Easy, 2.5..9.0),
                (Difficulty::VeryHard, 0.0..3.0),
            ] {
                let times = if difficulty == Difficulty::VeryHard {
                    &very_hard
                } else {
                    &times
                };
                let given = CheckpointTimes::on(difficulty).at(circuit.pace);
                let mut left = given.starting(&points);
                for (index, time) in times.iter().enumerate() {
                    let name = circuit.name;
                    // A rival's ride (a good one) never runs short of time,
                    // even on hard...
                    left -= time;
                    assert!(
                        left > 0.5,
                        "{name}, {difficulty:?}: {left} s left at checkpoint {index}"
                    );
                    // ...but it earns only a few seconds at each checkpoint:
                    // what the stretch to the next one takes it, and a
                    // little more.
                    let bonus = given.bonus(stretch_after(&points, index));
                    left += bonus;
                    if let Some(next) = times.get(index + 1) {
                        let earned = bonus - next;
                        assert!(
                            earns.contains(&earned),
                            "{name}, {difficulty:?}: {earned} s earned at checkpoint {index}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn against_the_clock_a_good_ride_earns_a_little_time_at_each_checkpoint() {
        against_the_clock_a_good_ride_earns_a_little_time_at_each_checkpoint_on(
            &crate::testing::DEMO,
        );
    }

    pub fn rivals_that_take_the_branches_ride_round_the_loops_and_back_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            if track.loops().is_empty() {
                continue;
            }
            let tuning = BikeTuning::default();
            let rival = Rival {
                skill: 0.9,
                lane: -1.0,
                loops: true,
            };
            let mut state = BikeState::parked_on(&track, track.grid_pose(0, rival.lane));
            let dt = 1.0 / 120.0;
            let n = track.sample_count() as i64;
            let (mut progress, mut last) = (
                0,
                track.nearest_index_at(state.position, state.elevation) as i64,
            );
            let (mut rounds, mut riding) = (0, false);
            for _ in 0..(90.0 / dt) as usize {
                let input = decide(&rival, rival.lane, f32::INFINITY, &state, &track, &tuning);
                state = advance(&state, &input, &track, &tuning, dt);
                if riding && state.ride.is_none() {
                    // Out of the way out, on the road back.
                    let looped = &track.loops()[0];
                    assert!(
                        state.position.distance(looped.way_out()) < 2.0,
                        "{}: came off the loop at {:?}",
                        circuit.name,
                        state.position
                    );
                    rounds += 1;
                }
                riding = state.ride.is_some();
                if !riding {
                    let nearest = track.nearest_at(state.position, state.elevation);
                    assert!(
                        nearest.distance < track.edge_beside(&nearest) - 0.5,
                        "{}: off the road at {:?}",
                        circuit.name,
                        state.position
                    );
                }
                let index = track.nearest_index_at(state.position, state.elevation) as i64;
                progress += (index - last + n + n / 2) % n - n / 2;
                last = index;
            }
            assert!(rounds >= 1, "{}: never went round the loop", circuit.name);
            assert!(progress > n, "{}: {progress} of {n}", circuit.name);
        }
    }

    #[test]
    fn rivals_that_take_the_branches_ride_round_the_loops_and_back() {
        rivals_that_take_the_branches_ride_round_the_loops_and_back_on(&crate::testing::DEMO);
    }

    pub fn rivals_ride_the_forks_on_their_side_and_back_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let tuning = BikeTuning::default();
            let dt = 1.0 / 120.0;
            let name = circuit.name;
            for fork in track.forks() {
                for lane in [3.0 * fork.side, 0.0] {
                    let rival = Rival {
                        skill: 0.9,
                        lane,
                        loops: false,
                    };
                    // From a little before the fork, at speed.
                    let start = (fork.fork + track.sample_count() - 40) % track.sample_count();
                    let mut state = BikeState {
                        speed: 40.0,
                        ..BikeState::parked_on(&track, track.pose_at(start))
                    };
                    let mut rode = false;
                    // Until it is back on the lap a little way past the
                    // join (before the next split, where the splits come one
                    // after another).
                    let past = fork.join as f32 * track.step_length() + 50.0;
                    let mut back = false;
                    for _ in 0..(20.0 / dt) as usize {
                        let input = decide(&rival, lane, f32::INFINITY, &state, &track, &tuning);
                        state = advance(&state, &input, &track, &tuning, dt);
                        let nearest = track.nearest_at(state.position, state.elevation);
                        assert!(
                            nearest.distance < track.edge_beside(&nearest) - 0.5,
                            "{name}: off the road at {:?}",
                            state.position
                        );
                        // Well away from the lap: past where their walls part.
                        let lap = track.centerline()[track.nearest_index(state.position)];
                        let parted = state.position.distance(lap) > 2.0 * track.wall() + 2.0;
                        rode |= nearest.road == fork.road && parted;
                        let along = track.along(&nearest);
                        if (past..past + 100.0).contains(&along) {
                            assert_eq!(nearest.road, 0, "{name}: not back on the lap");
                            back = true;
                            break;
                        }
                    }
                    assert_eq!(rode, rival.takes(fork), "{name}: lane {lane}");
                    assert!(back, "{name}: never got past the join, lane {lane}");
                }
            }
        }
    }

    #[test]
    fn rivals_ride_the_forks_on_their_side_and_back() {
        rivals_ride_the_forks_on_their_side_and_back_on(&crate::testing::DEMO);
    }

    pub fn no_way_through_a_split_is_clearly_the_quickest_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // Where the road splits three ways, a rival on each way (the one to
        // the left, the lap, the one to the right) takes about as long from
        // before the split to after it.
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let forks = track.forks();
            let step = track.step_length();
            let tuning = BikeTuning::default();
            let dt = 1.0 / 120.0;
            for left in forks.iter().filter(|fork| fork.side > 0.0) {
                let Some(right) = forks
                    .iter()
                    .find(|fork| fork.side < 0.0 && fork.fork.abs_diff(left.fork) < 20)
                else {
                    continue;
                };
                let from = left.fork.min(right.fork) as f32 * step - 60.0;
                let to = left.join.max(right.join) as f32 * step + 50.0;
                let times: Vec<f32> = [3.0, 0.0, -3.0]
                    .into_iter()
                    .map(|lane| {
                        let rival = Rival {
                            skill: 1.0,
                            lane,
                            loops: false,
                        };
                        let start = track.index_at(from - 150.0);
                        let mut state = BikeState::parked_on(&track, track.pose_at(start));
                        let (mut time, mut started) = (0.0, None);
                        for _ in 0..(40.0 / dt) as usize {
                            let input =
                                decide(&rival, lane, f32::INFINITY, &state, &track, &tuning);
                            state = advance(&state, &input, &track, &tuning, dt);
                            time += dt;
                            let nearest = track.nearest_at(state.position, state.elevation);
                            let along = track.along(&nearest);
                            if started.is_none() && (from..from + 50.0).contains(&along) {
                                started = Some(time);
                            }
                            if let Some(start) = started
                                && (to..to + 50.0).contains(&along)
                            {
                                return time - start;
                            }
                        }
                        panic!("{}: lane {lane} never got through", circuit.name)
                    })
                    .collect();
                let quickest = times.iter().copied().fold(f32::MAX, f32::min);
                let slowest = times.iter().copied().fold(0.0, f32::max);
                assert!(
                    slowest < 1.05 * quickest,
                    "{}: {times:?} s through the split at {from} m",
                    circuit.name
                );
            }
        }
    }

    #[test]
    fn no_way_through_a_split_is_clearly_the_quickest() {
        no_way_through_a_split_is_clearly_the_quickest_on(&crate::testing::DEMO);
    }

    pub fn round_a_loop_is_about_as_quick_as_the_lap_beside_it_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // A rival round the loop and one on the lap beside it take about as
        // long from before the fork to after the join: the way round the
        // loop goes straight to it, the lap swings away, and each has a
        // booster, if they have any.
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let Some(&looped) = track.loops().first() else {
                continue;
            };
            let step = track.step_length();
            let tuning = BikeTuning::default();
            let dt = 1.0 / 120.0;
            let from = looped.fork as f32 * step - 60.0;
            let to = looped.join as f32 * step + 50.0;
            let times: Vec<f32> = [true, false]
                .into_iter()
                .map(|loops| {
                    let rival = Rival {
                        skill: 1.0,
                        lane: 0.0,
                        loops,
                    };
                    let start = track.index_at(from - 150.0);
                    let mut state = BikeState::parked_on(&track, track.pose_at(start));
                    let (mut time, mut started) = (0.0, None);
                    for _ in 0..(60.0 / dt) as usize {
                        let input = decide(&rival, 0.0, f32::INFINITY, &state, &track, &tuning);
                        state = advance(&state, &input, &track, &tuning, dt);
                        time += dt;
                        if state.ride.is_some() {
                            continue;
                        }
                        let nearest = track.nearest_at(state.position, state.elevation);
                        let along = track.along(&nearest);
                        if started.is_none() && (from..from + 50.0).contains(&along) {
                            started = Some(time);
                        }
                        if let Some(start) = started
                            && nearest.road == 0
                            && (to..to + 50.0).contains(&along)
                        {
                            return time - start;
                        }
                    }
                    panic!("{}: never got through (loops {loops})", circuit.name)
                })
                .collect();
            assert!(
                times[0] < 1.05 * times[1] && times[1] < 1.05 * times[0],
                "{}: round the loop {} s, the lap {} s",
                circuit.name,
                times[0],
                times[1]
            );
        }
    }

    #[test]
    fn round_a_loop_is_about_as_quick_as_the_lap_beside_it() {
        round_a_loop_is_about_as_quick_as_the_lap_beside_it_on(&crate::testing::DEMO);
    }

    pub fn out_of_a_loop_the_road_back_keeps_the_wheels_on_the_ground_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let Some(&looped) = track.loops().first() else {
                continue;
            };
            let tuning = BikeTuning::default();
            let rival = Rival {
                skill: 1.0,
                lane: 0.0,
                loops: true,
            };
            let dt = 1.0 / 120.0;
            for speed in [30.0, 50.0, 70.0] {
                // About to come out of the way out, flat out, with the
                // suspension pressed down by the loop.
                let mut state = BikeState {
                    speed,
                    ride: Some(crate::bike::Ride {
                        index: 0,
                        angle: std::f32::consts::TAU - 0.05,
                        lateral: 0.0,
                        yaw: 0.0,
                    }),
                    ..BikeState::parked_at((looped.way_out(), looped.heading))
                };
                state.chassis.heave = -0.12;
                let name = circuit.name;
                // Until a little way along the lap past the join.
                let beyond = looped.join as f32 * track.step_length() + 30.0;
                let mut out = 0.0;
                for _ in 0..(10.0 / dt) as usize {
                    let input = decide(&rival, rival.lane, f32::INFINITY, &state, &track, &tuning);
                    state = advance(&state, &input, &track, &tuning, dt);
                    if state.ride.is_some() {
                        continue;
                    }
                    out += dt;
                    let nearest = track.nearest_at(state.position, state.elevation);
                    if nearest.road == 0 && track.along(&nearest) > beyond {
                        break;
                    }
                    // No jumps and no dips that bottom out the suspension,
                    // once it has come back up from the loop.
                    assert!(state.grounded, "{name}: flies at {speed} m/s, {out} s out");
                    if out > 0.5 {
                        assert!(
                            state.chassis.heave > -0.08,
                            "{name}: bottoms out at {speed} m/s, {out} s out"
                        );
                    }
                }
                let nearest = track.nearest_at(state.position, state.elevation);
                assert_eq!(nearest.road, 0, "{name}: not back on the lap");
            }
        }
    }

    #[test]
    fn out_of_a_loop_the_road_back_keeps_the_wheels_on_the_ground() {
        out_of_a_loop_the_road_back_keeps_the_wheels_on_the_ground_on(&crate::testing::DEMO);
    }

    pub fn round_a_loop_a_rival_gets_to_its_line_clear_of_the_walls_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let Some(&looped) = track.loops().first() else {
                continue;
            };
            let tuning = BikeTuning::default();
            let rival = Rival {
                skill: 1.0,
                lane: -2.0,
                loops: true,
            };
            let dt = 1.0 / 120.0;
            // Just in at the mouth, on the other side of the running surface
            // and turned away from its line.
            let mut state = BikeState {
                speed: 35.0,
                ride: Some(crate::bike::Ride {
                    index: 0,
                    angle: 0.0,
                    lateral: 3.0,
                    yaw: 0.1,
                }),
                ..BikeState::parked_at((looped.mouth, looped.heading))
            };
            let name = circuit.name;
            let mut out = false;
            for _ in 0..(10.0 / dt) as usize {
                let input = decide(&rival, rival.lane, f32::INFINITY, &state, &track, &tuning);
                let before = state;
                state = advance(&state, &input, &track, &tuning, dt);
                let Some(ride) = state.ride else {
                    // Out of the way out, on its line and straight on.
                    let lateral = (state.position - looped.way_out()).dot(looped.left());
                    assert!((lateral - rival.lane).abs() < 0.3, "{name}: {lateral} m");
                    let yaw = before.ride.unwrap().yaw;
                    assert!(yaw.abs() < 0.02, "{name}: turned {yaw} rad");
                    out = true;
                    break;
                };
                assert!(
                    ride.lateral.abs() < 4.5,
                    "{name}: near the wall at {ride:?}"
                );
            }
            assert!(out, "{name}: never came out of the loop");
        }
    }

    #[test]
    fn round_a_loop_a_rival_gets_to_its_line_clear_of_the_walls() {
        round_a_loop_a_rival_gets_to_its_line_clear_of_the_walls_on(&crate::testing::DEMO);
    }

    #[test]
    fn rival_goes_round_a_stopped_bike_in_its_lane() {
        let track = Track::default();
        let tuning = BikeTuning::default();
        let rival = Rival {
            skill: 0.95,
            lane: 0.0,
            loops: false,
        };
        let dt = 1.0 / 120.0;
        let mut state = BikeState::parked_on(&track, track.grid_pose(5, rival.lane));
        // Una moto parada en su carril, 50 m por delante.
        let mut stopped = BikeState::parked_on(&track, track.grid_pose(0, rival.lane));
        let mut line = Line(rival.lane);
        let (mut knock, mut stopped_knock) = (Knock::default(), Knock::default());
        let n = track.sample_count() as i64;
        let gap = |a: &BikeState, b: &BikeState| {
            let (a, b) = (
                track.nearest_index(a.position) as i64,
                track.nearest_index(b.position) as i64,
            );
            (b - a + n + n / 2) % n - n / 2
        };
        let mut closest = f32::MAX;
        for _ in 0..(8.0 / dt) as usize {
            let traffic = [Traffic {
                position: stopped.position,
                elevation: stopped.elevation,
                speed: stopped.speed,
            }];
            let (target, limit) = plan_line(&rival, line.0, &state, &track, &traffic);
            let rate = LINE_RATE + LINE_RATE_PER_SPEED * state.speed;
            line.0 = move_toward(line.0, target, rate * dt);
            let input = decide(&rival, line.0, limit, &state, &track, &tuning);
            state = advance(&state, &input, &track, &tuning, dt);
            crate::bike::carry(&mut state, &mut knock, &track, dt);
            let contact = crate::bike::collide(
                Body {
                    state: &mut state,
                    knock: &mut knock,
                    down: false,
                },
                Body {
                    state: &mut stopped,
                    knock: &mut stopped_knock,
                    down: false,
                },
            );
            assert!(contact.is_none(), "no la toca: {contact:?}");
            closest = closest.min(state.position.distance(stopped.position));
        }
        assert!(gap(&state, &stopped) < -10, "la ha adelantado");
        assert!(closest > 1.5, "sin tocarla: {closest} m");
        assert!(state.speed > 30.0, "y sigue lanzado: {}", state.speed);
    }

    /// One rider of the pack in [`pack_race`] and
    /// [`the_grid_gets_away_without_a_touch`].
    struct Racer {
        rival: Rival,
        line: Line,
        state: BikeState,
        knock: Knock,
        crash: CrashState,
    }

    impl Racer {
        /// On grid slot `slot` of `track`, on its line.
        fn on_grid(track: &Track, slot: usize, rival: Rival) -> Self {
            Racer {
                rival,
                line: Line(Track::grid_lane(slot)),
                state: BikeState::parked_on(track, track.grid_slot(slot)),
                knock: Knock::default(),
                crash: CrashState::default(),
            }
        }
    }

    /// What happened riding a pack for a while.
    #[derive(Default)]
    struct Ridden {
        falls: usize,
        touches: usize,
    }

    /// Rides `pack` round `track` for `seconds` as the game does, step by
    /// step: each rider plans its line, rides it against the walls and the
    /// others, and gets up again after a fall; `after` sees the pack after
    /// every step.
    fn ride_pack(
        track: &Track,
        pack: &mut [Racer],
        seconds: f32,
        mut after: impl FnMut(&[Racer]),
    ) -> Ridden {
        let tuning = BikeTuning::default();
        let dt = 1.0 / 120.0;
        let mut ridden = Ridden::default();
        for _ in 0..(seconds / dt) as usize {
            let traffic: Vec<Traffic> = pack
                .iter()
                .map(|racer| Traffic {
                    position: racer.state.position,
                    elevation: racer.state.elevation,
                    speed: racer.state.speed,
                })
                .collect();
            for (i, racer) in pack.iter_mut().enumerate() {
                if racer.crash.is_down() {
                    slide(
                        &mut racer.state,
                        &mut racer.knock,
                        &mut racer.crash,
                        track,
                        dt,
                    );
                    if racer.crash.is_over() {
                        let (position, heading, lateral) =
                            rejoin_pose(track, racer.state.position, racer.state.elevation, &[]);
                        racer.state = BikeState::parked_at_level(
                            track,
                            (position, heading),
                            racer.state.elevation,
                        );
                        racer.knock = Knock::default();
                        racer.crash = CrashState::default();
                        racer.line.0 = lateral;
                    }
                    continue;
                }
                let others: Vec<Traffic> = traffic
                    .iter()
                    .enumerate()
                    .filter(|&(j, _)| j != i)
                    .map(|(_, other)| *other)
                    .collect();
                let (target, limit) =
                    plan_line(&racer.rival, racer.line.0, &racer.state, track, &others);
                let rate = LINE_RATE + LINE_RATE_PER_SPEED * racer.state.speed.abs();
                racer.line.0 = move_toward(racer.line.0, target, rate * dt);
                let input = decide(
                    &racer.rival,
                    racer.line.0,
                    limit,
                    &racer.state,
                    track,
                    &tuning,
                );
                // Round a loop, along its running surface.
                if racer.state.ride.is_some() {
                    racer.state = advance(&racer.state, &input, track, &tuning, dt);
                    continue;
                }
                let wall;
                (racer.state, wall) =
                    advance_hitting_walls(&racer.state, &input, track, &tuning, dt);
                if let Some(hit) = wall.filter(wall_knocks_down) {
                    racer.crash = fall_against_wall(&mut racer.state, &mut racer.knock, &hit);
                    ridden.falls += 1;
                }
                crate::bike::carry(&mut racer.state, &mut racer.knock, track, dt);
            }
            for j in 1..pack.len() {
                let (before, after) = pack.split_at_mut(j);
                let b = &mut after[0];
                for a in before.iter_mut() {
                    if a.state.ride.is_some() || b.state.ride.is_some() {
                        continue;
                    }
                    let contact = crate::bike::collide(
                        Body {
                            state: &mut a.state,
                            knock: &mut a.knock,
                            down: a.crash.is_down(),
                        },
                        Body {
                            state: &mut b.state,
                            knock: &mut b.knock,
                            down: b.crash.is_down(),
                        },
                    );
                    let Some(contact) = contact else {
                        continue;
                    };
                    ridden.touches += 1;
                    if contact.a.falls {
                        a.crash = fall(&mut a.state, &mut a.knock, &contact.a);
                        ridden.falls += 1;
                    }
                    if contact.b.falls {
                        b.crash = fall(&mut b.state, &mut b.knock, &contact.b);
                        ridden.falls += 1;
                    }
                }
            }
            after(pack);
        }
        ridden
    }

    pub fn a_pack_of_rivals_races_without_wrecking_each_other_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // With the rivals that take the branches going round the loops, and
        // through the forks.
        for circuit in circuits.iter() {
            pack_race(
                &Track::new(&circuit.layout),
                circuit.name,
                Difficulty::Medium,
                0,
            );
        }
    }

    #[test]
    fn a_pack_of_rivals_races_without_wrecking_each_other() {
        a_pack_of_rivals_races_without_wrecking_each_other_on(&crate::testing::DEMO);
    }

    pub fn a_very_hard_pack_races_without_wrecking_each_other_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // Braking later and riding the corners harder, near enough clean:
        // a touch at most a lap (two down), where a rival riding the lap on
        // the side a branch leaves from is crossed by one taking it.
        for circuit in circuits.iter() {
            pack_race(
                &Track::new(&circuit.layout),
                circuit.name,
                Difficulty::VeryHard,
                2,
            );
        }
    }

    #[test]
    fn a_very_hard_pack_races_without_wrecking_each_other() {
        a_very_hard_pack_races_without_wrecking_each_other_on(&crate::testing::DEMO);
    }

    pub fn on_very_hard_the_rivals_are_quicker_than_on_hard_and_keep_to_the_road_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // The quickest rival of the grid, a lap from the grid, each way.
        for circuit in circuits.iter() {
            let times = |difficulty: Difficulty| -> f32 {
                checkpoint_times(circuit, difficulty.rival_skill(0.95), 1)
                    .iter()
                    .sum()
            };
            let (hard, very_hard) = (times(Difficulty::Hard), times(Difficulty::VeryHard));
            assert!(
                very_hard < 0.995 * hard && very_hard > 0.95 * hard,
                "{}: {very_hard} s against {hard} s",
                circuit.name
            );
        }
        // Up to 1 nothing changes: hard's rivals ride as they did.
        assert_eq!(Difficulty::Hard.rival_skill(0.99), 1.0);
    }

    #[test]
    fn on_very_hard_the_rivals_are_quicker_than_on_hard_and_keep_to_the_road() {
        on_very_hard_the_rivals_are_quicker_than_on_hard_and_keep_to_the_road_on(
            &crate::testing::DEMO,
        );
    }

    /// The rivals of [`crate::bike::GRID`], as fast as `difficulty` asks,
    /// race a lap of `track` from the grid: they all get round, no more
    /// than `most_falls` falls, and nobody leaves the road. Those that take
    /// the branches go round the loops.
    fn pack_race(track: &Track, name: &str, difficulty: Difficulty, most_falls: usize) {
        let mut pack: Vec<Racer> = crate::bike::GRID
            .iter()
            .enumerate()
            .filter_map(|(slot, rival)| {
                rival.map(|rival| {
                    let skill = difficulty.rival_skill(rival.skill);
                    Racer::on_grid(track, slot, Rival { skill, ..rival })
                })
            })
            .collect();
        let n = track.sample_count() as i64;
        let mut last: Vec<i64> = pack
            .iter()
            .map(|racer| track.nearest_index(racer.state.position) as i64)
            .collect();
        let mut progress = vec![0; pack.len()];
        // Long enough for a lap of the longest track.
        let ridden = ride_pack(track, &mut pack, 100.0, |pack| {
            for (i, racer) in pack.iter().enumerate() {
                assert!(
                    racer.state.ride.is_some()
                        || track.distance_to_centerline(racer.state.position) < 11.5,
                    "el rival {i} se ha salido de {name} en {:?}",
                    racer.state.position
                );
                let index = track.nearest_index(racer.state.position) as i64;
                progress[i] += (index - last[i] + n + n / 2) % n - n / 2;
                last[i] = index;
            }
        });
        assert!(
            ridden.falls <= most_falls,
            "se caen {} veces en {name}",
            ridden.falls
        );
        for (i, progress) in progress.iter().enumerate() {
            assert!(
                *progress > n,
                "el rival {i} no da la vuelta a {name}: {progress} de {n}"
            );
        }
    }

    pub fn the_grid_gets_away_without_a_touch_on(circuits: &'static crate::circuits::Circuits) {
        // The rivals set off side by side, three to a row, most of them
        // wanting a lane on the other side of the road. With the player on
        // its slot too, keeping to its line and not riding into anyone, on
        // every difficulty: nobody so much as touches another bike.
        use crate::settings::DIFFICULTIES;
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            for difficulty in DIFFICULTIES {
                let mut pack: Vec<Racer> = crate::bike::GRID
                    .iter()
                    .enumerate()
                    .map(|(slot, rival)| {
                        let rival = match rival {
                            Some(rival) => Rival {
                                skill: difficulty.rival_skill(rival.skill),
                                ..*rival
                            },
                            None => Rival {
                                skill: 0.97,
                                lane: Track::grid_lane(slot),
                                loops: false,
                            },
                        };
                        Racer::on_grid(&track, slot, rival)
                    })
                    .collect();
                let ridden = ride_pack(&track, &mut pack, 10.0, |_| {});
                assert_eq!(
                    ridden.touches, 0,
                    "{}, {difficulty:?}: {} touches",
                    circuit.name, ridden.touches
                );
            }
        }
    }

    #[test]
    fn the_grid_gets_away_without_a_touch() {
        the_grid_gets_away_without_a_touch_on(&crate::testing::DEMO);
    }

    #[test]
    fn a_rival_moves_across_only_as_far_as_the_bikes_beside_it_leave_room() {
        // Nobody beside: all the way.
        assert_eq!(across_to(-4.5, 4.0, &[]), 4.0);
        // One beside, between it and where it goes: up to the clearance
        // short of it, and no further while it is there.
        assert_eq!(across_to(-4.5, 4.0, &[0.0]), -CLEARANCE);
        assert_eq!(across_to(-CLEARANCE, 4.0, &[0.0]), -CLEARANCE);
        // Too close already: it stays where it is, never back.
        assert_eq!(across_to(-1.0, 4.0, &[0.0]), -1.0);
        // One on the other side, or past where it goes and the clearance:
        // no matter.
        assert_eq!(across_to(-4.5, 0.0, &[-6.0, 3.0]), 0.0);
        // The nearest of those in the way, either way across.
        assert_eq!(across_to(4.5, -4.0, &[0.0, 2.0]), 2.0 + CLEARANCE);
    }

    pub fn jumps_on_the_track_are_short_on(circuits: &'static crate::circuits::Circuits) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let tuning = BikeTuning::default();
            let rival = Rival {
                skill: 1.0,
                lane: 0.0,
                loops: false,
            };
            let mut state = BikeState::parked_on(&track, track.grid_pose(0, rival.lane));
            let dt = 1.0 / 120.0;
            let (mut flights, mut air) = (0, 0usize);
            let name = circuit.name;
            // Long enough for a jump on two laps of the longest track, whose
            // only jump flat out may be on a single stretch.
            for _ in 0..(90.0 / dt) as usize {
                let input = decide(&rival, rival.lane, f32::INFINITY, &state, &track, &tuning);
                state = advance(&state, &input, &track, &tuning, dt);
                if !state.grounded {
                    air += 1;
                    let height =
                        state.elevation - track.road_height_at(state.position, state.elevation);
                    assert!(height < 1.3, "vuela muy alto en {name}: {height} m");
                } else if air > 0 {
                    flights += 1;
                    assert!(
                        air as f32 * dt < 0.75,
                        "vuela {} s en {name}",
                        air as f32 * dt
                    );
                    air = 0;
                }
            }
            assert!(
                flights >= 2,
                "a fondo, en los cambios de rasante de {name} se vuela"
            );
        }
    }

    #[test]
    fn jumps_on_the_track_are_short() {
        jumps_on_the_track_are_short_on(&crate::testing::DEMO);
    }

    pub fn a_seafront_runs_into_its_first_bend_with_no_crest_on(
        circuits: &'static crate::circuits::Circuits,
        name: &str,
    ) {
        // Flat out to the end of the seafront and braking for the banked
        // right-hander at the water's edge, the bike stays on the road: no
        // crest throws it off, even at top speed.
        let circuit = circuits
            .iter()
            .find(|circuit| circuit.name == name)
            .unwrap_or_else(|| panic!("{name} is a circuit"));
        let track = Track::new(&circuit.layout);
        let top = BikeTuning::default().max_speed;
        for along in (100..480).step_by(2) {
            let index = track.index_at(along as f32);
            let crest = track.crest(&crate::track::Nearest {
                point: track.centerline()[index],
                distance: 0.0,
                index,
                t: 0.0,
                lateral: 0.0,
                road: 0,
            });
            assert!(
                crest * top * top < crate::bike::GRAVITY,
                "a crest of {:.0} m at {along} m",
                1.0 / crest
            );
        }
    }

    pub fn crests_only_throw_the_bikes_into_the_air_on_the_straights_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // Flying, a bike neither turns nor leans: a crest in a bend, or one
        // that lands in a bend, would straighten it up (as a mountain road
        // once did over its top into a dive, a forest road over a hump, a
        // rock pass over a bump and a start straight where a branch turns
        // off it).
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let tuning = BikeTuning::default();
            for (skill, lane) in [(0.9, 4.0), (0.95, 2.0), (1.0, -3.0)] {
                let rival = Rival {
                    skill,
                    lane,
                    loops: false,
                };
                let mut state = BikeState::parked_on(&track, track.grid_pose(0, lane));
                let dt = 1.0 / 120.0;
                for _ in 0..(70.0 / dt) as usize {
                    let input = decide(&rival, lane, f32::INFINITY, &state, &track, &tuning);
                    let before = state;
                    state = advance(&state, &input, &track, &tuning, dt);
                    if before.grounded != state.grounded {
                        let index = track.nearest_index_at(state.position, state.elevation);
                        let radius = 1.0 / track.smoothed_curvature(index, 6).abs().max(1e-6);
                        let along = index as f32 * track.step_length();
                        assert!(
                            radius > 600.0,
                            "{} takes off or lands in a bend of {radius:.0} m at {along:.0} m",
                            circuit.name,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn crests_only_throw_the_bikes_into_the_air_on_the_straights() {
        crests_only_throw_the_bikes_into_the_air_on_the_straights_on(&crate::testing::DEMO);
    }
}
