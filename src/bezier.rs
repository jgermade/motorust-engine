//! Cubic Bézier easing curves, as CSS's `cubic-bezier(x1, y1, x2, y2)`:
//! from (0, 0) to (1, 1), shaped by two control points in between. With
//! both control points' x between 0 and 1 the curve never turns back on
//! itself across, so each x has one y: a function, which is what the
//! keyboard's curves need (see [`crate::bike::curves`]).

use serde::{Deserialize, Serialize};

/// A curve from (0, 0) to (1, 1) through the pull of its control points
/// P1 (`x1`, `y1`) and P2 (`x2`, `y2`).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Bezier {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

/// How many halvings find the point of the curve at a given x or y: well
/// under a millionth of the way along it.
const HALVINGS: usize = 24;
/// How many stretches the curve is looked at in, to find where it first
/// comes to a height (it may come to it more than once).
const STRETCHES: usize = 64;

impl Bezier {
    /// The straight line: y is x.
    pub const LINEAR: Bezier = Bezier::new(1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);

    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    /// One coordinate of the curve at `t` (0 to 1 along it), for control
    /// points at `a` and `b` along that coordinate.
    fn coordinate(a: f32, b: f32, t: f32) -> f32 {
        let s = 1.0 - t;
        3.0 * s * s * t * a + 3.0 * s * t * t * b + t * t * t
    }

    /// The point of the curve `t` of the way along it.
    pub fn point(self, t: f32) -> (f32, f32) {
        (
            Self::coordinate(self.x1, self.x2, t),
            Self::coordinate(self.y1, self.y2, t),
        )
    }

    /// How far along the curve (`t`) it is at `x`, which grows with `t`.
    fn along(self, x: f32) -> f32 {
        let (mut low, mut high) = (0.0f32, 1.0f32);
        for _ in 0..HALVINGS {
            let middle = 0.5 * (low + high);
            if Self::coordinate(self.x1, self.x2, middle) < x {
                low = middle;
            } else {
                high = middle;
            }
        }
        0.5 * (low + high)
    }

    /// The curve's height at `x` (0 to 1, kept to it).
    pub fn ease(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        Self::coordinate(self.y1, self.y2, self.along(x))
    }

    /// The first x at which the curve comes up to `y` (0 to 1, kept to it):
    /// where something following it from the start would be on reaching
    /// that height. The curve need not rise all the way, so it is looked at
    /// stretch by stretch from the start and the first that gets there is
    /// halved down to the point.
    pub fn first_reaching(self, y: f32) -> f32 {
        let y = y.clamp(0.0, 1.0);
        let height = |t| Self::coordinate(self.y1, self.y2, t);
        if height(0.0) >= y {
            return 0.0;
        }
        let mut before = 0.0;
        for stretch in 1..=STRETCHES {
            let t = stretch as f32 / STRETCHES as f32;
            if height(t) >= y {
                let (mut low, mut high) = (before, t);
                for _ in 0..HALVINGS {
                    let middle = 0.5 * (low + high);
                    if height(middle) < y {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                return Self::coordinate(self.x1, self.x2, high);
            }
            before = t;
        }
        1.0
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn the_linear_curve_is_the_identity() {
        for k in 0..=20 {
            let x = k as f32 / 20.0;
            assert!((Bezier::LINEAR.ease(x) - x).abs() < 1e-5, "{x}");
            assert!((Bezier::LINEAR.first_reaching(x) - x).abs() < 1e-5, "{x}");
        }
    }

    #[test]
    fn a_curve_goes_from_the_origin_to_the_far_corner() {
        let curve = Bezier::new(0.1, 0.6, 0.7, 0.2);
        assert!(curve.ease(0.0).abs() < 1e-6);
        assert!((curve.ease(1.0) - 1.0).abs() < 1e-6);
        // Outside the square, held at its edges.
        assert_eq!(curve.ease(-1.0), curve.ease(0.0));
        assert_eq!(curve.ease(2.0), curve.ease(1.0));
    }

    #[test]
    fn ease_matches_the_curve_drawn_through_its_points() {
        let curve = Bezier::new(0.1, 0.6, 0.7, 0.2);
        for k in 0..=50 {
            let (x, y) = curve.point(k as f32 / 50.0);
            assert!((curve.ease(x) - y).abs() < 1e-4, "at {x}");
        }
    }

    #[test]
    fn with_the_control_points_on_the_thirds_y_is_a_polynomial_of_x() {
        // x(t) = t, so y = 3 y1 x (1 - x)² + 3 y2 x² (1 - x) + x³.
        let (y1, y2) = (0.24, 0.57);
        let curve = Bezier::new(1.0 / 3.0, y1, 2.0 / 3.0, y2);
        for k in 0..=10 {
            let x = k as f32 / 10.0;
            let s = 1.0 - x;
            let y = 3.0 * y1 * x * s * s + 3.0 * y2 * x * x * s + x * x * x;
            assert!((curve.ease(x) - y).abs() < 1e-5, "{x}");
        }
    }

    #[test]
    fn first_reaching_undoes_ease_on_a_rising_curve() {
        let curve = Bezier::new(0.4, 0.0, 0.2, 1.0);
        for k in 0..=20 {
            let x = k as f32 / 20.0;
            let back = curve.first_reaching(curve.ease(x));
            assert!((back - x).abs() < 1e-3, "{x}: {back}");
        }
    }

    #[test]
    fn first_reaching_finds_the_first_time_on_a_curve_that_overshoots() {
        // Up past 0.5 early, back down, and up again to the end: 0.5 is
        // first reached early on.
        let curve = Bezier::new(0.1, 1.5, 0.5, -0.5);
        let x = curve.first_reaching(0.5);
        assert!(x < 0.2, "{x}");
        assert!((curve.ease(x) - 0.5).abs() < 1e-3);
    }
}
