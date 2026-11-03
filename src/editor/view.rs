//! How the editor looks at a circuit, and what it does with the mouse: a
//! camera over a point on the ground, turned and tilted, as near or as far
//! as asked ([`View`]); picking the control point under the cursor
//! ([`nearest_on_screen`]) and moving it where the cursor's ray meets the
//! level it is at ([`on_level`]); a point added between two
//! ([`between`]).

use bevy::prelude::*;

/// The editor's camera: over `focus` (x, z) at `height`, `distance` from
/// it, looking down at `pitch` (rad, from level) from the side `yaw` says
/// (rad, about the vertical).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub focus: Vec2,
    pub height: f32,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
}

/// How near and far the camera may be (m), and how steeply it may look
/// down (rad).
pub const DISTANCE: (f32, f32) = (40.0, 5000.0);
pub const PITCH: (f32, f32) = (0.35, 1.5);

impl View {
    /// Looking down on all of `points` (x, z), from the south, steeply.
    pub fn over(points: &[Vec2], height: f32) -> Self {
        let (min, max) = points.iter().fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(min, max), &p| (min.min(p), max.max(p)),
        );
        let (focus, size) = if points.is_empty() {
            (Vec2::ZERO, 500.0)
        } else {
            ((min + max) / 2.0, (max - min).max_element())
        };
        Self {
            focus,
            height,
            distance: (size * 1.1).clamp(DISTANCE.0, DISTANCE.1),
            yaw: 0.0,
            pitch: 1.2,
        }
    }

    /// Where the camera is, looking at its focus.
    pub fn transform(&self) -> Transform {
        let target = Vec3::new(self.focus.x, self.height, self.focus.y);
        // From the south (+z) at yaw 0, turned about the vertical.
        let back = Quat::from_rotation_y(self.yaw) * Vec3::Z;
        let eye = target + (back * self.pitch.cos() + Vec3::Y * self.pitch.sin()) * self.distance;
        Transform::from_translation(eye).looking_at(target, Vec3::Y)
    }

    /// Moved `by` on the screen's axes (right, forward), in metres at the
    /// focus.
    pub fn panned(self, by: Vec2) -> Self {
        let right = Vec2::new(self.yaw.cos(), -self.yaw.sin());
        let forward = Vec2::new(-self.yaw.sin(), -self.yaw.cos());
        Self {
            focus: self.focus + right * by.x + forward * by.y,
            ..self
        }
    }

    /// Nearer (`steps` > 0) or further, a tenth each step.
    pub fn zoomed(self, steps: f32) -> Self {
        Self {
            distance: (self.distance * 0.9f32.powf(steps)).clamp(DISTANCE.0, DISTANCE.1),
            ..self
        }
    }

    /// Turned `yaw` and tilted `pitch` more (rad).
    pub fn turned(self, yaw: f32, pitch: f32) -> Self {
        Self {
            yaw: self.yaw + yaw,
            pitch: (self.pitch + pitch).clamp(PITCH.0, PITCH.1),
            ..self
        }
    }
}

/// Of the points seen at `on_screen` (px, `None` where not in view), the
/// one nearest `cursor`, if within `reach` px of it.
pub fn nearest_on_screen(on_screen: &[Option<Vec2>], cursor: Vec2, reach: f32) -> Option<usize> {
    on_screen
        .iter()
        .enumerate()
        .filter_map(|(index, at)| at.map(|at| (index, at.distance(cursor))))
        .filter(|&(_, distance)| distance <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// Where a ray from `origin` along `direction` meets the level `height`
/// (x, z), if it goes down (or up) to it ahead.
pub fn on_level(origin: Vec3, direction: Vec3, height: f32) -> Option<Vec2> {
    if direction.y.abs() < 1e-6 {
        return None;
    }
    let t = (height - origin.y) / direction.y;
    (t > 0.0).then(|| {
        let at = origin + direction * t;
        Vec2::new(at.x, at.z)
    })
}

/// Halfway from point `index` of the closed loop `points` to the next.
pub fn between(points: &[Vec2], index: usize) -> Vec2 {
    let next = points[(index + 1) % points.len()];
    (points[index] + next) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_looks_at_its_focus_from_above() {
        let view = View {
            focus: Vec2::new(100.0, -50.0),
            height: 10.0,
            distance: 200.0,
            yaw: 0.3,
            pitch: 1.0,
        };
        let transform = view.transform();
        let target = Vec3::new(100.0, 10.0, -50.0);
        assert!((transform.translation.distance(target) - 200.0).abs() < 1e-3);
        assert!(transform.translation.y > target.y);
        let looking = transform.forward().as_vec3();
        assert!(looking.dot((target - transform.translation).normalize()) > 0.9999);
    }

    #[test]
    fn panning_moves_the_focus_along_the_screen() {
        let view = View::over(&[Vec2::ZERO, Vec2::new(100.0, 100.0)], 0.0);
        // Looking north at yaw 0: forward is -z, right is +x.
        let moved = view.panned(Vec2::new(10.0, 20.0));
        assert!((moved.focus - view.focus - Vec2::new(10.0, -20.0)).length() < 1e-4);
        // Turned a quarter to the left, forward is -x.
        let turned = view.turned(std::f32::consts::FRAC_PI_2, 0.0);
        let moved = turned.panned(Vec2::new(0.0, 10.0));
        assert!((moved.focus - turned.focus - Vec2::new(-10.0, 0.0)).length() < 1e-4);
        // The camera seen to move the way the screen says.
        let eye = |view: View| view.transform().translation;
        assert!(eye(view.panned(Vec2::X)).x > eye(view).x);
    }

    #[test]
    fn zoom_and_tilt_keep_within_their_limits() {
        let view = View::over(&[Vec2::ZERO, Vec2::splat(1000.0)], 0.0);
        assert!(view.zoomed(1.0).distance < view.distance);
        assert_eq!(view.zoomed(-1000.0).distance, DISTANCE.1);
        assert_eq!(view.zoomed(1000.0).distance, DISTANCE.0);
        assert_eq!(view.turned(0.0, 10.0).pitch, PITCH.1);
        assert_eq!(view.turned(0.0, -10.0).pitch, PITCH.0);
    }

    #[test]
    fn the_nearest_point_within_reach_is_picked() {
        let on_screen = [Some(Vec2::new(0.0, 0.0)), None, Some(Vec2::new(10.0, 0.0))];
        assert_eq!(
            nearest_on_screen(&on_screen, Vec2::new(7.0, 1.0), 12.0),
            Some(2)
        );
        assert_eq!(
            nearest_on_screen(&on_screen, Vec2::new(3.0, 0.0), 12.0),
            Some(0)
        );
        assert_eq!(
            nearest_on_screen(&on_screen, Vec2::new(50.0, 0.0), 12.0),
            None
        );
    }

    #[test]
    fn a_ray_meets_the_level_below_it() {
        let at = on_level(Vec3::new(0.0, 100.0, 0.0), Vec3::new(1.0, -1.0, 0.0), 20.0);
        assert!((at.unwrap() - Vec2::new(80.0, 0.0)).length() < 1e-4);
        assert_eq!(on_level(Vec3::Y * 100.0, Vec3::Y, 20.0), None);
        assert_eq!(on_level(Vec3::Y * 100.0, Vec3::X, 20.0), None);
    }

    #[test]
    fn a_point_between_goes_halfway_and_round_the_end() {
        let points = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)];
        assert_eq!(between(&points, 0), Vec2::new(5.0, 0.0));
        assert_eq!(between(&points, 2), Vec2::new(5.0, 5.0));
    }
}
