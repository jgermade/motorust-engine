//! A racing line round the lap: how far to the side of the centre line to
//! ride at each of its points, for the keyboard's self-learning to follow
//! (see [`crate::bike::learning`]). It is the line that bends least within
//! the asphalt (its edges kept a margin away): out wide before a corner,
//! in at its apex and out wide again after it, straight through gentle
//! bends. Found by easing each point towards where the neighbours either
//! side of it, and theirs beyond, would have it to bend least (the fairing
//! of a curve: (-P₋₂ + 4 P₋₁ + 4 P₁ - P₂) / 6), a long reach first (the
//! line through a whole corner) and shorter after (its finer shape),
//! within the asphalt at every step. (Easing towards the middle of the
//! neighbours alone would give the shortest way round instead, hugging
//! the inside from well before the corner.)

use bevy::math::Vec2;

/// How far the line keeps from the edge of the asphalt (m).
const EDGE_MARGIN: f32 = 1.5;
/// The neighbours each point is eased towards, how far along (in points of
/// the centre line, about 2 m apart), and how many times each.
const PASSES: [(usize, usize); 5] = [(32, 300), (16, 300), (8, 300), (4, 300), (2, 300)];
/// How much of the way to where its neighbours would have it each point
/// goes each time.
const EASE: f32 = 0.3;

/// The racing line of a closed lap through `centerline` (points in order),
/// with `half_width(index)` the half width of the asphalt there: how far to
/// the left of each point to ride (m, negative to the right), left as
/// [`crate::track::Track::offset_point`] has it.
pub fn racing_line(centerline: &[Vec2], half_width: impl Fn(usize) -> f32) -> Vec<f32> {
    let n = centerline.len();
    if n < 8 {
        return vec![0.0; n];
    }
    let at = |index: isize| index.rem_euclid(n as isize) as usize;
    let normals: Vec<Vec2> = (0..n as isize)
        .map(|i| {
            let tangent = (centerline[at(i + 1)] - centerline[at(i - 1)]).normalize_or_zero();
            Vec2::new(tangent.y, -tangent.x)
        })
        .collect();
    let room: Vec<f32> = (0..n)
        .map(|i| (half_width(i) - EDGE_MARGIN).max(0.0))
        .collect();
    let mut lateral = vec![0.0f32; n];
    for (reach, times) in PASSES {
        let reach = reach.min(n / 8).max(1) as isize;
        for _ in 0..times {
            let point = |lateral: &[f32], i: usize| centerline[i] + normals[i] * lateral[i];
            let eased: Vec<f32> = (0..n)
                .map(|i| {
                    let near = |k: isize| point(&lateral, at(i as isize + k * reach));
                    let fair = (4.0 * (near(-1) + near(1)) - near(-2) - near(2)) / 6.0;
                    let toward = (fair - point(&lateral, i)).dot(normals[i]);
                    (lateral[i] + EASE * toward).clamp(-room[i], room[i])
                })
                .collect();
            lateral = eased;
        }
    }
    lateral
}

/// How sharp a closed path through `points` bends at each (1/m, without
/// its sign), from the turn between one point and the next, averaged over
/// `window` points either side (one point's alone is noisy).
pub fn path_curvature(points: &[Vec2], window: usize) -> Vec<f32> {
    let n = points.len();
    if n < 3 {
        return vec![0.0; n];
    }
    let at = |index: isize| points[index.rem_euclid(n as isize) as usize];
    let bend: Vec<f32> = (0..n as isize)
        .map(|i| {
            let (a, b) = (at(i) - at(i - 1), at(i + 1) - at(i));
            let length = 0.5 * (a.length() + b.length());
            if length < 1e-4 {
                0.0
            } else {
                a.angle_to(b).abs() / length
            }
        })
        .collect();
    let window = window as isize;
    (0..n as isize)
        .map(|i| {
            (i - window..=i + window)
                .map(|k| bend[k.rem_euclid(n as isize) as usize])
                .sum::<f32>()
                / (2 * window + 1) as f32
        })
        .collect()
}

/// The fastest a bike can ride each point of a closed lap bending
/// `curvature` (1/m), `step` m apart: no faster than `top` (m/s), than
/// `grip` (m/s² across) lets it round the bend there, nor than it can brake
/// down from (`braking`, m/s²) to the next point's. And whether each point
/// is in a braking zone: slower than its own bend allows, for one ahead.
pub fn speed_plan(
    curvature: &[f32],
    step: f32,
    top: f32,
    grip: f32,
    braking: f32,
) -> (Vec<f32>, Vec<bool>) {
    let n = curvature.len();
    let own: Vec<f32> = curvature
        .iter()
        .map(|&bend| {
            if bend > 1e-5 {
                (grip / bend).sqrt().min(top)
            } else {
                top
            }
        })
        .collect();
    let mut speed = own.clone();
    // Backwards round the lap, twice: the braking for a corner reaches back
    // past the line.
    for _ in 0..2 {
        for i in (0..n).rev() {
            let next = speed[(i + 1) % n];
            speed[i] = speed[i].min((next * next + 2.0 * braking * step).sqrt());
        }
    }
    let zone = (0..n).map(|i| speed[i] < own[i] - 0.5).collect();
    (speed, zone)
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// A lap round a stadium: two straights `straight` m long joined by
    /// half circles of `radius` m round (0, ±straight / 2), about 2 m
    /// between points.
    fn stadium(straight: f32, radius: f32) -> Vec<Vec2> {
        let mut points = Vec::new();
        let steps = |length: f32| (length / 2.0).round() as usize;
        for k in 0..steps(straight) {
            points.push(Vec2::new(radius, -straight / 2.0 + k as f32 * 2.0));
        }
        let arc = steps(TAU / 2.0 * radius);
        for k in 0..arc {
            let angle = k as f32 / arc as f32 * TAU / 2.0;
            points.push(Vec2::new(
                radius * angle.cos(),
                straight / 2.0 + radius * angle.sin(),
            ));
        }
        for k in 0..steps(straight) {
            points.push(Vec2::new(-radius, straight / 2.0 - k as f32 * 2.0));
        }
        for k in 0..arc {
            let angle = TAU / 2.0 + k as f32 / arc as f32 * TAU / 2.0;
            points.push(Vec2::new(
                radius * angle.cos(),
                -straight / 2.0 + radius * angle.sin(),
            ));
        }
        points
    }

    #[test]
    fn the_line_stays_on_the_asphalt_short_of_its_edges() {
        let lap = stadium(200.0, 40.0);
        let line = racing_line(&lap, |_| 7.0);
        assert_eq!(line.len(), lap.len());
        assert!(line.iter().all(|l| l.abs() <= 7.0 - EDGE_MARGIN + 1e-4));
    }

    /// Where the line is at point `index` of `lap`.
    fn on_line(lap: &[Vec2], line: &[f32], index: usize) -> Vec2 {
        let n = lap.len();
        let tangent = (lap[(index + 1) % n] - lap[(index + n - 1) % n]).normalize();
        lap[index] + Vec2::new(tangent.y, -tangent.x) * line[index]
    }

    #[test]
    fn round_a_corner_it_goes_in_at_the_apex_and_out_wide_on_either_side() {
        let (straight, radius) = (200.0, 40.0);
        let lap = stadium(straight, radius);
        let line = racing_line(&lap, |_| 7.0);
        let room = 7.0 - EDGE_MARGIN;
        // How much nearer the corner's middle than the centre line it is.
        let middle = Vec2::new(0.0, straight / 2.0);
        let inwards = |index: usize| {
            lap[index].distance(middle) - on_line(&lap, &line, index).distance(middle)
        };
        let steps = (straight / 2.0).round() as usize;
        let arc = (TAU / 2.0 * radius / 2.0).round() as usize;
        let apex = steps + arc / 2;
        assert!(inwards(apex) > 0.8 * room, "apex: {}", inwards(apex));
        // Out wide on the straight 40 m before and after it, turning in
        // from there.
        let (before, after) = (steps - 20, steps + arc + 20);
        assert!(inwards(before) < -0.4 * room, "before: {}", inwards(before));
        assert!(inwards(after) < -0.4 * room, "after: {}", inwards(after));
    }

    #[test]
    fn it_bends_less_than_the_centre_line() {
        let lap = stadium(120.0, 30.0);
        let line = racing_line(&lap, |_| 7.0);
        let n = lap.len();
        let path: Vec<Vec2> = (0..n).map(|i| on_line(&lap, &line, i)).collect();
        // How much it bends all round: the squares of its turns from one
        // point to the next.
        let bending = |points: &[Vec2]| -> f32 {
            (0..n)
                .map(|i| {
                    let a = points[(i + 1) % n] - points[i];
                    let b = points[(i + 2) % n] - points[(i + 1) % n];
                    a.angle_to(b).powi(2)
                })
                .sum()
        };
        assert!(bending(&path) < 0.85 * bending(&lap));
    }

    pub fn on_every_track_it_stays_on_the_asphalt_and_takes_the_sharpest_bend_inside_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for circuit in circuits.iter() {
            let track = crate::track::Track::new(&circuit.layout);
            let line = racing_line(track.centerline(), |i| track.half_width_at(i));
            for (index, &left) in line.iter().enumerate() {
                assert!(left.abs() <= track.half_width_at(index), "{}", circuit.name);
            }
            // The sharpest bend (left positive): the line on its inside.
            let sharpest = (0..line.len())
                .max_by(|&a, &b| {
                    let bend = |i| track.smoothed_curvature(i, 4);
                    bend(a).total_cmp(&bend(b))
                })
                .unwrap();
            let bend: f32 = (-4..=4)
                .map(|k| track.curvature((sharpest as isize + k).max(0) as usize))
                .sum();
            assert!(
                line[sharpest] * bend > 0.0,
                "{}: {}",
                circuit.name,
                line[sharpest]
            );
        }
    }

    #[test]
    fn on_every_track_it_stays_on_the_asphalt_and_takes_the_sharpest_bend_inside() {
        on_every_track_it_stays_on_the_asphalt_and_takes_the_sharpest_bend_inside_on(
            &crate::testing::DEMO,
        );
    }

    #[test]
    fn the_speed_plan_brakes_before_each_corner_down_to_what_it_allows() {
        let lap = stadium(200.0, 40.0);
        let bend = path_curvature(&lap, 3);
        // Round the half circles, about one over the radius; nothing on the
        // straights' middle.
        let steps = 100;
        let arc = (TAU / 2.0 * 40.0 / 2.0).round() as usize;
        assert!((bend[steps + arc / 2] - 1.0 / 40.0).abs() < 0.003);
        assert!(bend[steps / 2] < 1e-4);
        let (top, grip, braking) = (80.0, 65.0, 16.8);
        let (speed, zone) = speed_plan(&bend, 2.0, top, grip, braking);
        let corner = (grip * 40.0f32).sqrt();
        assert!((speed[steps + arc / 2] - corner).abs() < 1.5);
        // On the straight before it, braking down to it: in the zone, and
        // never faster than it can brake from.
        assert!(zone[steps - 5] && speed[steps - 5] < top);
        for i in 0..lap.len() {
            let next = speed[(i + 1) % lap.len()];
            assert!(speed[i] * speed[i] <= next * next + 2.0 * braking * 2.0 + 1e-2);
        }
        // Out of the corner, flat out again: not in it.
        assert!(!zone[steps + arc + 30]);
        assert!(!zone[steps + arc / 2]);
    }

    #[test]
    fn a_short_or_empty_lap_has_the_centre_line() {
        assert!(racing_line(&[], |_| 7.0).is_empty());
        assert_eq!(racing_line(&[Vec2::ZERO; 4], |_| 7.0), vec![0.0; 4]);
    }
}
