//! Riding round a vertical loop ([`Loop`]), upside down at the top.
//!
//! On the loop the bike follows the running surface: its position, height and
//! pitch come from how far round it is and how far to the side ([`Ride`]).
//! Steering turns it on the surface as on the road, and then it goes across
//! the running surface as it goes round; the walls along its edges keep it on
//! it, as the walls of a road do, though nobody falls against them. The
//! running surface carries the bike sideways as it goes round, so that it
//! comes out beside the mouth: going straight on follows it. The other bikes
//! are left out. Its speed comes from the same engine, brake and resistances
//! as on the road ([`step`]), plus the part of gravity along its way, which
//! slows it on the way up and speeds it up on the way down. The surface holds
//! the bike as long as it presses on it: past the side, too slow to be pressed
//! against the surface (its speed round the loop squared over the radius is
//! less than the part of gravity pulling it away), it falls off; the less it
//! presses, the less the tyres grip to turn. Too slow to get past the side, it
//! rolls back down and out of the mouth, backwards.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use super::{
    BIKE_HALF_WIDTH, BikeInput, BikeState, BikeTuning, GRAVITY, Ground, IMPACT_SPEED_LOSS,
    WALL_ALIGN, step,
};
use crate::geometry::wrap_angle;
use crate::track::{LOOP_WALL, Loop, Surface, Track};

/// A bike riding round a loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ride {
    /// Which loop of the track (see [`Track::loops`]).
    pub index: usize,
    /// How far round it is (rad): 0 at the mouth, a full turn at the way out.
    pub angle: f32,
    /// How far to the left of the middle of the running surface (m).
    pub lateral: f32,
    /// Which way it points on the running surface (rad, in [-π, π)): 0
    /// straight round, positive to the left, like the heading. Off the loop
    /// it is part of the heading again.
    pub yaw: f32,
}

/// How a step on a loop ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// Still on the loop.
    Riding,
    /// Still on the loop, against one of the walls along its edges, into
    /// which it went at this speed (m/s).
    HitWall(f32),
    /// Back on the road: out of the way out, or rolled back out of the mouth.
    Off,
    /// Fell off the running surface, with this velocity (x, up, z; m/s).
    Fell(Vec3),
}

#[cfg(any(test, feature = "testing"))]
impl Outcome {
    /// Whether the bike is still on the loop.
    pub fn riding(self) -> bool {
        matches!(self, Outcome::Riding | Outcome::HitWall(_))
    }
}

/// How far from the middle of the running surface a bike can ride (m): up
/// to the walls along its edges.
const RIDE_HALF_WIDTH: f32 = LOOP_WALL - BIKE_HALF_WIDTH;

/// If a bike that has just taken a step on the road has gone in at the mouth
/// of a loop, puts it on the loop, pointing on the running surface the way
/// it pointed on the road.
pub fn enter(state: &mut BikeState, track: &Track) {
    if state.ride.is_some() || state.speed <= 0.0 {
        return;
    }
    for (index, looped) in track.loops().iter().enumerate() {
        let Some((into, lateral)) = looped.entered(state.position, state.elevation) else {
            continue;
        };
        let yaw = wrap_angle(state.heading - looped.heading);
        if yaw.cos() <= 0.5 {
            continue;
        }
        let ride = Ride {
            index,
            angle: into / looped.arc(),
            lateral: lateral.clamp(-RIDE_HALF_WIDTH, RIDE_HALF_WIDTH),
            yaw,
        };
        state.grass = 0.0;
        // On the loop the heading is the loop's, and the yaw the rest.
        state.heading -= yaw;
        place(state, looped, ride);
        return;
    }
}

/// One step of `dt` seconds of a bike on a loop.
pub fn ride(
    state: &BikeState,
    input: &BikeInput,
    looped: &Loop,
    tuning: &BikeTuning,
    dt: f32,
) -> (BikeState, Outcome) {
    let Some(mut ride) = state.ride else {
        return (*state, Outcome::Off);
    };
    // The engine, the brake, the resistances and the steering as on a flat
    // road; the running surface presses the suspension down, the harder the
    // faster the bike goes round it.
    let input = BikeInput {
        wheelie: 0.0,
        ..*input
    };
    let round = ride.yaw.cos();
    let ground = Ground {
        surface: Surface::Asphalt,
        grade: 0.0,
        crest: -round * round / looped.radius,
        booster: 0.0,
        grip: 1.0,
    };
    let mut next = step(state, &input, &ground, tuning, dt);
    // The tyres grip as hard as the surface presses on them, up to as hard
    // as the road does: slow near the top, the bike hardly turns.
    let grip = (pressed(state.speed, ride, looped) / GRAVITY).clamp(0.0, 1.0);
    let turn = (next.heading - state.heading) * grip;
    next.heading = state.heading;
    next.speed -= GRAVITY * ride.angle.sin() * round * dt;
    // Round and across the running surface, pointing half way through the
    // turn.
    let travel = 0.5 * (state.speed + next.speed) * dt;
    let pointing = ride.yaw + 0.5 * turn;
    ride.yaw = wrap_angle(ride.yaw + turn);
    ride.angle += travel * pointing.cos() / looped.arc();
    ride.lateral += travel * pointing.sin();
    let wall = hit_walls(&mut next, &mut ride);
    next.ride = Some(ride);
    if pressed(next.speed, ride, looped) < 0.0 {
        let left = looped.left();
        let velocity = (looped.tangent(ride.angle) * ride.yaw.cos()
            + Vec3::new(left.x, 0.0, left.y) * ride.yaw.sin())
            * next.speed;
        place(&mut next, looped, ride);
        // Seen from above, upside down, a bike turned to its left points
        // to the right of the way round.
        next.heading += ride.yaw * ride.angle.cos().signum();
        next.ride = None;
        return (next, Outcome::Fell(velocity));
    }
    if ride.angle >= TAU {
        // Out of the way out, as far along the road as it went past it.
        let past = (ride.angle - TAU) * looped.arc();
        leave(
            &mut next,
            looped,
            looped.way_out() + looped.ahead * past,
            ride,
        );
        return (next, Outcome::Off);
    }
    if ride.angle < 0.0 {
        // Rolled back out of the mouth.
        let past = ride.angle * looped.arc();
        leave(&mut next, looped, looped.mouth + looped.ahead * past, ride);
        return (next, Outcome::Off);
    }
    place(&mut next, looped, ride);
    (next, wall.map_or(Outcome::Riding, Outcome::HitWall))
}

/// How hard the running surface presses on a bike going at `speed` where
/// `ride` says, per unit of mass (m/s²): what it takes to keep it going
/// round, less the part of gravity pulling it off. Only the speed round the
/// loop counts: across it, the running surface is straight.
fn pressed(speed: f32, ride: Ride, looped: &Loop) -> f32 {
    let round = speed * ride.yaw.cos();
    round * round / looped.radius + GRAVITY * ride.angle.cos()
}

/// Keeps a bike on a loop between the walls along the edges of its running
/// surface, as [`super::hit_walls`] does on the road: one that goes into a
/// wall stays against it, loses speed the more head on it went into it
/// (scraping along it hardly costs any) and turns along it. Returns the speed
/// into the wall (m/s), if it touched one.
fn hit_walls(state: &mut BikeState, ride: &mut Ride) -> Option<f32> {
    if ride.lateral.abs() <= RIDE_HALF_WIDTH {
        return None;
    }
    let side = ride.lateral.signum();
    ride.lateral = side * RIDE_HALF_WIDTH;
    // How head on it was going (forwards or backwards) into the wall.
    let impact = (ride.yaw.sin() * state.speed.signum() * side).max(0.0);
    let into = state.speed.abs() * impact;
    state.speed *= 1.0 - IMPACT_SPEED_LOSS * impact * impact;
    if state.speed > 0.0 {
        let along = if ride.yaw.cos() >= 0.0 { 0.0 } else { PI };
        ride.yaw = wrap_angle(ride.yaw + wrap_angle(along - ride.yaw) * WALL_ALIGN);
    }
    Some(into)
}

/// Puts a bike on the running surface where `ride` says, facing round it.
fn place(state: &mut BikeState, looped: &Loop, ride: Ride) {
    let point = looped.surface_point(ride.angle, ride.lateral);
    state.position = Vec2::new(point.x, point.z);
    state.elevation = point.y;
    state.pitch = ride.angle;
    state.climb = state.speed * ride.yaw.cos() * ride.angle.sin();
    state.grounded = true;
    state.ride = Some(ride);
}

/// Takes a bike off the loop onto the road at `point` (the middle of the
/// road there), as far to the side as it rode and pointing the way it did.
fn leave(state: &mut BikeState, looped: &Loop, point: Vec2, ride: Ride) {
    state.position = point + looped.left() * ride.lateral;
    state.elevation = looped.height;
    state.heading += ride.yaw;
    state.pitch = wrap_angle(ride.angle);
    state.climb = 0.0;
    state.grounded = true;
    state.ride = None;
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;
    use crate::geometry::forward;
    use crate::track::{LOOP_HALF_WIDTH, WALL_DISTANCE};

    const DT: f32 = 1.0 / 120.0;

    fn looped() -> Loop {
        Loop {
            mouth: Vec2::ZERO,
            ahead: Vec2::NEG_Y,
            heading: 0.0,
            height: 2.0,
            radius: 16.0,
            shift: -16.0,
            approach: 1,
            exit: 2,
            fork: 0,
            join: 0,
            boosters: false,
        }
    }

    /// A bike going in at the mouth at `speed`, with `throttle`; how it
    /// came off the loop and when (s).
    fn go_round(speed: f32, throttle: f32) -> (BikeState, Outcome, f32) {
        let state = BikeState {
            speed,
            ride: Some(Ride {
                index: 0,
                angle: 0.0,
                lateral: 1.0,
                yaw: 0.0,
            }),
            ..BikeState::parked_at((Vec2::ZERO, 0.0))
        };
        let input = BikeInput {
            throttle,
            ..default()
        };
        let (state, outcome, seconds) = keep_riding(state, input, 20.0);
        assert!(!outcome.riding(), "still on the loop");
        (state, outcome, seconds)
    }

    /// A bike on the loop riding with `input` for up to `seconds`, or until
    /// it comes off.
    fn keep_riding(
        mut state: BikeState,
        input: BikeInput,
        seconds: f32,
    ) -> (BikeState, Outcome, f32) {
        let (looped, tuning) = (looped(), BikeTuning::default());
        for k in 0..(seconds / DT) as usize {
            let outcome;
            (state, outcome) = ride(&state, &input, &looped, &tuning, DT);
            if !outcome.riding() {
                return (state, outcome, k as f32 * DT);
            }
        }
        (state, Outcome::Riding, seconds)
    }

    #[test]
    fn the_running_surface_goes_round_and_comes_out_beside_the_mouth() {
        let looped = looped();
        let start = looped.surface_point(0.0, 0.0);
        let top = looped.surface_point(TAU / 2.0, 0.0);
        let end = looped.surface_point(TAU, 0.0);
        assert!((top.y - start.y - 2.0 * looped.radius).abs() < 1e-3);
        assert!((Vec2::new(end.x, end.z) - looped.way_out()).length() < 1e-3);
        // Side by side at the bottom, clear of each other.
        assert!(looped.shift.abs() > 2.0 * LOOP_HALF_WIDTH);
        assert!(looped.shift.abs() < 2.0 * WALL_DISTANCE);
        // At the top, upside down: up is down.
        assert!((looped.up(TAU / 2.0) - Vec3::NEG_Y).length() < 1e-4);
        assert!((looped.tangent(TAU / 4.0) - Vec3::Y).length() < 1e-4);
    }

    #[test]
    fn fast_enough_the_bike_goes_round_and_out_of_the_way_out() {
        let looped = looped();
        let (state, outcome, seconds) = go_round(40.0, 1.0);
        assert_eq!(outcome, Outcome::Off);
        assert!(state.ride.is_none());
        let out = state.position - looped.way_out();
        assert!(out.dot(looped.ahead) >= 0.0 && out.dot(looped.ahead) < 1.0);
        assert!(
            (out.dot(looped.left()) - 1.0).abs() < 1e-3,
            "keeps its side"
        );
        assert!((state.elevation - looped.height).abs() < 1e-3);
        assert!(state.speed > 30.0, "{}", state.speed);
        assert!(seconds < 3.0, "{seconds} s");
    }

    #[test]
    fn too_slow_to_reach_the_side_the_bike_comes_back_down_and_out() {
        let looped = looped();
        let state = BikeState {
            speed: 12.0,
            ride: Some(Ride {
                index: 0,
                angle: 0.0,
                lateral: 0.0,
                yaw: 0.0,
            }),
            ..BikeState::parked_at((Vec2::ZERO, 0.0))
        };
        // It stops short of the side and comes back down, where the slope
        // is gentle enough for the engine braking to hold it.
        let (state, outcome, _) = keep_riding(state, BikeInput::default(), 6.0);
        if outcome.riding() {
            let ride = state.ride.unwrap();
            assert!(ride.angle < 0.6 && state.speed.abs() < 0.5, "{ride:?}");
        }
        // Reversing, it rolls out of the mouth backwards.
        let reverse = BikeInput {
            brake: 1.0,
            ..default()
        };
        let (state, outcome, _) = keep_riding(state, reverse, 10.0);
        assert_eq!(outcome, Outcome::Off);
        assert!(state.speed < 0.0, "backwards: {}", state.speed);
        let out = state.position - looped.mouth;
        assert!(out.dot(looped.ahead) <= 0.0);
        assert!((state.elevation - looped.height).abs() < 1e-3);
    }

    #[test]
    fn too_slow_to_go_over_the_top_the_bike_falls_off() {
        let (_, outcome, _) = go_round(28.0, 0.0);
        let Outcome::Fell(velocity) = outcome else {
            panic!("{outcome:?}");
        };
        // Past the side, still going up and round.
        assert!(velocity.length() > 1.0, "{velocity}");
    }

    #[test]
    fn a_bike_at_the_mouth_goes_onto_the_loop() {
        let mut track = Track::default();
        let looped = looped();
        track.add_loop_for_tests(looped);
        let mut state = BikeState {
            speed: 30.0,
            elevation: looped.height,
            ..BikeState::parked_at((looped.mouth + looped.ahead * 0.5 + looped.left() * 2.0, 0.0))
        };
        enter(&mut state, &track);
        let ride = state.ride.expect("on the loop");
        assert!((ride.lateral - 2.0).abs() < 1e-3);
        assert!(ride.angle > 0.0 && ride.angle < 0.05);
        assert_eq!(ride.yaw, 0.0);
        // Turned on the road, it is as turned on the running surface, and
        // its heading is the loop's.
        let mut state = BikeState {
            speed: 30.0,
            elevation: looped.height,
            ..BikeState::parked_at((looped.mouth + looped.ahead * 0.5, 0.3))
        };
        enter(&mut state, &track);
        let ride = state.ride.expect("on the loop");
        assert!((ride.yaw - 0.3).abs() < 1e-5, "{ride:?}");
        assert!((state.heading - looped.heading).abs() < 1e-5);
        // Backwards, not yet at the mouth or not facing into it, it does not.
        for (speed, along, heading) in [(-3.0, 0.5, 0.0), (30.0, -0.5, 0.0), (30.0, 0.5, 1.2)] {
            let mut state = BikeState {
                speed,
                elevation: looped.height,
                ..BikeState::parked_at((looped.mouth + looped.ahead * along, heading))
            };
            enter(&mut state, &track);
            assert!(state.ride.is_none());
        }
    }

    /// A bike on the loop at `speed`, `angle` round, `lateral` to the left of
    /// the middle and turned `yaw` on the running surface.
    fn on_loop(speed: f32, angle: f32, lateral: f32, yaw: f32) -> BikeState {
        let mut state = BikeState {
            speed,
            ..BikeState::parked_at((Vec2::ZERO, 0.0))
        };
        let ride = Ride {
            index: 0,
            angle,
            lateral,
            yaw,
        };
        place(&mut state, &looped(), ride);
        state
    }

    #[test]
    fn steering_takes_the_bike_across_the_running_surface() {
        for side in [1.0, -1.0] {
            let steering = BikeInput {
                throttle: 1.0,
                steer: 0.3 * side,
                ..default()
            };
            let (state, outcome, _) = keep_riding(on_loop(35.0, 0.0, 0.0, 0.0), steering, 0.4);
            assert!(outcome.riding(), "{outcome:?}");
            let ride = state.ride.unwrap();
            assert!(ride.yaw * side > 0.1, "turns that way: {ride:?}");
            assert!(ride.lateral * side > 0.5, "goes that way: {ride:?}");
            assert!(
                state.lean * side > 0.1,
                "leans into the turn: {}",
                state.lean
            );
            // Let go, it keeps going that way, without turning back.
            let straight = BikeInput {
                throttle: 1.0,
                ..default()
            };
            let (later, outcome, _) = keep_riding(state, straight, 0.3);
            assert!(outcome.riding(), "{outcome:?}");
            let later = later.ride.unwrap();
            assert!(later.yaw * side >= ride.yaw * side, "{later:?}");
            assert!(
                later.lateral * side > ride.lateral * side + 0.5,
                "{later:?}"
            );
        }
    }

    #[test]
    fn the_bike_turns_about_its_own_up_on_the_running_surface() {
        let looped = looped();
        let left = looped.left();
        let left = Vec3::new(left.x, 0.0, left.y);
        for angle in [0.0, 1.0, TAU / 2.0, 4.0] {
            let orientation = on_loop(30.0, angle, 0.0, 0.3).orientation();
            let way = looped.tangent(angle) * 0.3f32.cos() + left * 0.3f32.sin();
            assert!((orientation * Vec3::Y - looped.up(angle)).length() < 1e-4);
            assert!((orientation * Vec3::NEG_Z - way).length() < 1e-4);
        }
    }

    #[test]
    fn the_walls_along_the_edges_keep_the_bike_on_the_running_surface() {
        let (looped, tuning) = (looped(), BikeTuning::default());
        let input = BikeInput {
            throttle: 1.0,
            ..default()
        };
        // Straight on, beside the left wall: the speed it keeps.
        let ride_for = |yaw: f32| {
            let mut state = on_loop(35.0, 0.0, RIDE_HALF_WIDTH - 0.5, yaw);
            let mut hardest: f32 = 0.0;
            for _ in 0..(1.0 / DT) as usize {
                let outcome;
                (state, outcome) = ride(&state, &input, &looped, &tuning, DT);
                if let Outcome::HitWall(into) = outcome {
                    hardest = hardest.max(into);
                }
                assert!(outcome.riding(), "{outcome:?}");
                let lateral = state.ride.unwrap().lateral;
                assert!(
                    lateral.abs() <= RIDE_HALF_WIDTH + 1e-4,
                    "through the wall: {lateral}"
                );
            }
            (state, hardest)
        };
        let (along, _) = ride_for(0.0);
        // Turned into the wall, it hits it as hard as it was going into
        // it, stays against it and turns along it, slower the more head on
        // it went.
        for (yaw, kept) in [(0.05, 0.99..1.0), (0.4, 0.8..0.95)] {
            let (state, hardest) = ride_for(yaw);
            let ride = state.ride.unwrap();
            assert!(
                (hardest - 35.0 * yaw.sin()).abs() < 0.5,
                "{yaw}: {hardest} m/s"
            );
            assert!((ride.lateral - RIDE_HALF_WIDTH).abs() < 1e-3, "{ride:?}");
            assert!(ride.yaw.abs() < 1e-3, "along the wall: {ride:?}");
            let kept_speed = state.speed / along.speed;
            assert!(kept.contains(&kept_speed), "{yaw}: kept {kept_speed}");
        }
    }

    #[test]
    fn turned_on_the_loop_the_bike_comes_out_pointing_that_way() {
        let looped = looped();
        // Turned a little to the left from the right of the running
        // surface: across it all the way round, clear of the walls.
        let (yaw, from) = (0.04, -3.0);
        let input = BikeInput {
            throttle: 1.0,
            ..default()
        };
        let (state, outcome, _) = keep_riding(on_loop(40.0, 0.0, from, yaw), input, 20.0);
        assert_eq!(outcome, Outcome::Off);
        assert!(state.ride.is_none());
        assert!((state.heading - (looped.heading + yaw)).abs() < 1e-4);
        let lateral = (state.position - looped.way_out()).dot(looped.left());
        let across = from + looped.length() * yaw.tan();
        assert!(
            (lateral - across).abs() < 0.1,
            "{lateral} m, not {across} m"
        );
    }

    #[test]
    fn the_less_the_surface_presses_the_less_the_bike_turns() {
        // Steering hard, at the bottom and at the top, upside down, barely
        // fast enough to stay on: there the surface presses a fifth as hard
        // as the road.
        let turn = |angle: f32, speed: f32| {
            let state = BikeState {
                steer: 1.0,
                ..on_loop(speed, angle, 0.0, 0.0)
            };
            let input = BikeInput {
                steer: 1.0,
                ..default()
            };
            let (next, outcome) = ride(&state, &input, &looped(), &BikeTuning::default(), DT);
            assert!(outcome.riding(), "{outcome:?}");
            next.ride.unwrap().yaw
        };
        let slow = (1.2 * GRAVITY * looped().radius).sqrt();
        let (bottom, top) = (turn(0.0, slow), turn(TAU / 2.0, slow));
        assert!(bottom > 0.0, "{bottom}");
        assert!(
            (top / bottom - 0.2).abs() < 0.02,
            "{top} rad, against {bottom}"
        );
    }

    #[test]
    fn turned_upside_down_the_bike_falls_off_the_way_it_was_going() {
        // At the top, too slow to stay on, turned to its left.
        let state = on_loop(3.0, TAU / 2.0, 0.0, 0.3);
        let (looped, tuning) = (looped(), BikeTuning::default());
        let (next, outcome) = ride(&state, &BikeInput::default(), &looped, &tuning, DT);
        let Outcome::Fell(velocity) = outcome else {
            panic!("{outcome:?}");
        };
        assert!(next.ride.is_none());
        // Back over the mouth and, as seen from above, to its right: the way
        // the bike pointed, and the way its rider faces as they fall (see
        // `crash::fall_off`).
        let across = Vec2::new(velocity.x, velocity.z).normalize();
        let pointed = state.orientation() * Vec3::NEG_Z;
        assert!(across.dot(Vec2::new(pointed.x, pointed.z).normalize()) > 0.999);
        assert!(across.dot(-looped.ahead) > 0.9 && across.dot(looped.left()) > 0.2);
        assert!(
            across.dot(forward(next.heading + PI)) > 0.999,
            "{}",
            next.heading
        );
    }
}
