//! The limits a lap's centre line and height profile must keep, as the
//! editor shows them while a circuit is laid out and as the tests check
//! them (`curves_are_drivable`, `slopes_and_banking_stay_rideable`): no bend
//! tighter than [`MIN_RADIUS_IN_WALLS`] times as far as the walls stand from
//! the middle, no grade of [`MAX_GRADE`] or more.
//!
//! [`Preview`] lays a lap out from its control points and height profile as
//! [`crate::track::Track`] does (the same smooth closed curve, its points
//! [`SAMPLE_SPACING`] apart, the same profile), without the rest of what a
//! track works out (banking, walls, the roads off it): cheap enough to do
//! again each time a point is moved.

use bevy::prelude::*;

use crate::geometry::{heading_of, wrap_angle};
use crate::track::{BASE_ELEVATION, SAMPLE_SPACING, closed_curve, profile_height};

/// Under this many times as far as the walls stand from the middle, a bend's
/// inside wall would fold over itself, and no one could take it.
pub const MIN_RADIUS_IN_WALLS: f32 = 2.5;
/// The steepest grade a road may have, up or down (rise over run).
pub const MAX_GRADE: f32 = 0.13;

/// The tightest a bend may be (m) on a road whose walls stand `wall` from
/// its middle.
pub fn min_radius(wall: f32) -> f32 {
    MIN_RADIUS_IN_WALLS * wall
}

/// The curvature at point `index` of a closed line (1/radius, positive
/// turning left), as [`crate::track::Track::curvature`] works it out: the
/// turn from the way in to the way out over the length between them.
pub fn curvature(line: &[Vec2], index: usize) -> f32 {
    let n = line.len();
    if n < 3 {
        return 0.0;
    }
    let (prev, point, next) = (
        line[(index + n - 1) % n],
        line[index],
        line[(index + 1) % n],
    );
    let turn = wrap_angle(heading_of(next - point) - heading_of(point - prev));
    turn / (0.5 * (prev.distance(point) + point.distance(next)))
}

/// The grade from point `index` of a closed profile to the next (rise over
/// run), the points `step` apart.
pub fn grade(heights: &[f32], step: f32, index: usize) -> f32 {
    let n = heights.len();
    (heights[(index + 1) % n] - heights[index]) / step
}

/// Whether a bend of `curvature` is too tight for walls `wall` from the
/// middle.
pub fn too_tight(curvature: f32, wall: f32) -> bool {
    curvature.abs() * min_radius(wall) >= 1.0
}

/// Whether a `grade` is too steep.
pub fn too_steep(grade: f32) -> bool {
    grade.abs() >= MAX_GRADE
}

/// A lap laid out from its control points and height profile, as the game
/// would lay it, and where it breaks the limits.
#[derive(Clone, Debug, Default)]
pub struct Preview {
    /// The centre line, points about [`SAMPLE_SPACING`] apart, the last
    /// running on into the first.
    pub line: Vec<Vec2>,
    /// The road's height at each point (m, as the game's: over the sea).
    pub heights: Vec<f32>,
    /// How far apart the points are (m).
    pub step: f32,
    /// Which points are in a bend too tight, and which on a grade too
    /// steep (to the next).
    pub tight: Vec<bool>,
    pub steep: Vec<bool>,
    /// The tightest bend's radius (m) and the steepest grade (signed).
    pub tightest: f32,
    pub steepest: f32,
}

impl Preview {
    /// The lap through `points` with the height profile `elevation` (m
    /// along the lap, m high), its walls `wall` from the middle.
    pub fn new(points: &[Vec2], elevation: &[(f32, f32)], wall: f32) -> Self {
        if points.len() < 3 {
            return Self::default();
        }
        let line = closed_curve(points, SAMPLE_SPACING);
        let n = line.len();
        let step = (0..n)
            .map(|i| line[i].distance(line[(i + 1) % n]))
            .sum::<f32>()
            / n as f32;
        let lap = n as f32 * step;
        let heights: Vec<f32> = (0..n)
            .map(|i| {
                BASE_ELEVATION
                    + if elevation.is_empty() {
                        0.0
                    } else {
                        profile_height(elevation, lap, i as f32 * step)
                    }
            })
            .collect();
        let curvatures: Vec<f32> = (0..n).map(|i| curvature(&line, i)).collect();
        let grades: Vec<f32> = (0..n).map(|i| grade(&heights, step, i)).collect();
        let tightest = curvatures
            .iter()
            .map(|c| 1.0 / c.abs().max(1e-6))
            .fold(f32::INFINITY, f32::min);
        let steepest =
            grades.iter().copied().fold(
                0.0,
                |most: f32, g| if g.abs() > most.abs() { g } else { most },
            );
        Self {
            tight: curvatures.iter().map(|&c| too_tight(c, wall)).collect(),
            steep: grades.iter().map(|&g| too_steep(g)).collect(),
            line,
            heights,
            step,
            tightest,
            steepest,
        }
    }

    /// How long the lap is (m).
    pub fn length(&self) -> f32 {
        self.line.len() as f32 * self.step
    }

    /// Whether it keeps every limit.
    pub fn keeps_the_limits(&self) -> bool {
        !self.tight.contains(&true) && !self.steep.contains(&true)
    }

    /// The curvature at point `index` of the centre line (see
    /// [`curvature`]).
    pub fn curvature_at(&self, index: usize) -> f32 {
        curvature(&self.line, index)
    }

    /// How far along the lap (m) point `index` of its centre line is.
    pub fn along(&self, index: usize) -> f32 {
        index as f32 * self.step
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// A circle of `radius` round the origin through `count` points.
    fn circle(radius: f32, count: usize) -> Vec<Vec2> {
        (0..count)
            .map(|k| {
                let a = TAU * k as f32 / count as f32;
                Vec2::new(radius * a.cos(), radius * a.sin())
            })
            .collect()
    }

    #[test]
    fn a_wide_flat_circle_keeps_the_limits() {
        let preview = Preview::new(&circle(100.0, 24), &[(0.0, 0.0)], 7.5);
        assert!(preview.keeps_the_limits());
        // (A curve through 24 points of a circle, not quite a circle.)
        assert!(
            (preview.tightest - 100.0).abs() < 6.0,
            "{}",
            preview.tightest
        );
        assert!(preview.steepest.abs() < 1e-3);
        assert!((preview.length() - TAU * 100.0).abs() < 2.0);
    }

    #[test]
    fn a_circle_too_small_for_its_walls_is_too_tight_all_round() {
        // 2.5 times 11.5 m is 28.75 m: a circle of 25 m is too tight.
        let preview = Preview::new(&circle(25.0, 16), &[(0.0, 0.0)], 11.5);
        assert!(preview.tight.iter().all(|&tight| tight));
        let wider = Preview::new(&circle(25.0, 16), &[(0.0, 0.0)], 7.5);
        assert!(!wider.tight.contains(&true));
    }

    #[test]
    fn a_steep_climb_and_drop_are_marked_where_they_are() {
        // Round a circle 628 m long: up 40 m over 200 m (20 %) and back.
        let preview = Preview::new(
            &circle(100.0, 24),
            &[(0.0, 0.0), (100.0, 0.0), (300.0, 40.0), (400.0, 40.0)],
            7.5,
        );
        assert!(preview.steep.contains(&true));
        assert!(preview.steepest.abs() >= MAX_GRADE);
        let index = preview.steep.iter().position(|&steep| steep).unwrap();
        let along = preview.along(index);
        // (The profile eases into the climb a little before its foot.)
        assert!((80.0..300.0).contains(&along) || along > 380.0, "{along}");
        // Gently, it keeps them.
        let gentle = Preview::new(
            &circle(100.0, 24),
            &[(0.0, 0.0), (100.0, 0.0), (300.0, 10.0), (400.0, 10.0)],
            7.5,
        );
        assert!(gentle.keeps_the_limits());
    }

    #[test]
    fn the_limits_are_those_of_the_tests() {
        assert_eq!(min_radius(7.5), 18.75);
        assert!(too_tight(1.0 / 18.0, 7.5) && !too_tight(1.0 / 19.0, 7.5));
        assert!(too_steep(-0.13) && !too_steep(0.129));
    }

    #[test]
    fn the_preview_is_the_lap_the_game_lays() {
        // The demo circuit's lap, as the game lays it.
        let circuit = &crate::testing::DEMO[0];
        let layout = &circuit.layout;
        let track = crate::track::Track::new(layout);
        let preview = Preview::new(layout.control_points, layout.elevation, layout.wall);
        assert_eq!(preview.line.len(), track.sample_count());
        assert!((preview.step - track.step_length()).abs() < 1e-4);
        for index in (0..preview.line.len()).step_by(37) {
            let (point, _) = track.pose_at(index);
            assert!(preview.line[index].distance(point) < 1e-3);
            assert!((preview.curvature_at(index) - track.curvature(index)).abs() < 1e-4);
        }
    }
}
