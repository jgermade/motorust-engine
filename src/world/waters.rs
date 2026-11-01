//! The water of a circuit, as shapes its file gives with their parameters
//! (see [`Waters`]): a sea beyond straight shores with waves in them, lakes
//! whose shores wander, rivers between their banks, and islets in them.

use bevy::prelude::*;
use serde::Deserialize;

use crate::geometry::fbm;

/// How far from the water a circuit with none is (m): far enough that
/// nothing of it shows.
pub const NO_WATER: f32 = 1000.0;

/// A circuit's water: how far a point is from its shores, positive on the
/// land and negative in the water, the nearest of its seas, lakes and rivers,
/// but for its islets, land in them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Waters {
    pub shores: &'static [Shore],
    pub lakes: &'static [Lake],
    pub rivers: &'static [River],
    pub islets: &'static [Islet],
}

impl Waters {
    /// How far `point` is from the water (m): negative in it.
    pub fn distance(&self, point: Vec2) -> f32 {
        let water = (self.shores.iter().map(|shore| shore.distance(point)))
            .chain(self.lakes.iter().map(|lake| lake.distance(point)))
            .chain(self.rivers.iter().map(|river| river.distance(point)))
            .reduce(f32::min)
            .unwrap_or(NO_WATER);
        self.islets.iter().fold(water, |water, islet| {
            water.max(islet.radius - point.distance(islet.center))
        })
    }
}

/// A shore with the sea beyond it: a straight line `offset` metres back
/// from the origin along `normal` (the way inland), bent by waves.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Shore {
    pub normal: Vec2,
    pub offset: f32,
    #[serde(default, deserialize_with = "crate::circuits::data::leak")]
    pub waves: &'static [Wave],
}

impl Shore {
    fn distance(&self, point: Vec2) -> f32 {
        self.waves
            .iter()
            .fold(point.dot(self.normal) + self.offset, |distance, wave| {
                distance + wave.amplitude * (point.dot(wave.along) / wave.length + wave.phase).sin()
            })
    }
}

/// A wave in a shore: `amplitude` metres in (out, if negative), repeating
/// every `length` times τ metres along `along`, shifted by `phase` radians.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Wave {
    pub along: Vec2,
    pub amplitude: f32,
    pub length: f32,
    #[serde(default)]
    pub phase: f32,
}

/// A lake: round, `radius` metres from its `center`, its shore wandering
/// in and out if it does.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Lake {
    pub center: Vec2,
    pub radius: f32,
    #[serde(default)]
    pub wander: Option<Wander>,
}

impl Lake {
    fn distance(&self, point: Vec2) -> f32 {
        let distance = point.distance(self.center) - self.radius;
        distance + self.wander.map_or(0.0, |wander| wander.at(point))
    }
}

/// A river: through `points` (x, z), each with how far its banks are from
/// its middle there (m), from one to the next in straight reaches, the
/// width changing evenly along them; its banks wandering if they do.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct River {
    #[serde(deserialize_with = "crate::circuits::data::leak")]
    pub points: &'static [(Vec2, f32)],
    #[serde(default)]
    pub wander: Option<Wander>,
}

impl River {
    fn distance(&self, point: Vec2) -> f32 {
        let banks = self
            .points
            .windows(2)
            .map(|pair| {
                let ((a, wide_a), (b, wide_b)) = (pair[0], pair[1]);
                let ab = b - a;
                let t = ((point - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
                point.distance(a + ab * t) - wide_a.lerp(wide_b, t)
            })
            .fold(f32::MAX, f32::min);
        banks + self.wander.map_or(0.0, |wander| wander.at(point))
    }
}

/// An islet: land `radius` metres round its `center`, in the water.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Islet {
    pub center: Vec2,
    pub radius: f32,
}

/// A shore wandering up to `amount` metres in and out, in bends about
/// `scale` metres across (noise shifted by `offset`, so that two waters
/// wander apart).
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Wander {
    pub amount: f32,
    pub scale: f32,
    pub octaves: u32,
    #[serde(default)]
    pub offset: Vec2,
}

impl Wander {
    fn at(&self, point: Vec2) -> f32 {
        self.amount * (2.0 * fbm(point / self.scale + self.offset, self.octaves) - 1.0)
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn a_shore_with_waves_the_sea_beyond_it() {
        let wave = Wave {
            along: Vec2::Y,
            amplitude: 10.0,
            length: 100.0,
            phase: 0.0,
        };
        let shore = Shore {
            normal: Vec2::X,
            offset: 50.0,
            waves: Vec::leak(vec![wave]),
        };
        // Inland to the east of x = -50, its shore bent by the wave.
        assert_eq!(shore.distance(Vec2::new(-50.0, 0.0)), 0.0);
        assert!(shore.distance(Vec2::new(0.0, 0.0)) > 0.0);
        assert!(shore.distance(Vec2::new(-100.0, 0.0)) < 0.0);
        let bent = shore.distance(Vec2::new(-50.0, 100.0 * std::f32::consts::FRAC_PI_2));
        assert!((bent - 10.0).abs() < 1e-3, "{bent}");
    }

    #[test]
    fn the_nearest_water_but_for_the_islets_and_none_far_off() {
        let lake = Lake {
            center: Vec2::ZERO,
            radius: 100.0,
            wander: None,
        };
        let river = River {
            points: Vec::leak(vec![
                (Vec2::new(300.0, 0.0), 10.0),
                (Vec2::new(300.0, 1000.0), 30.0),
            ]),
            wander: None,
        };
        let waters = Waters {
            lakes: Vec::leak(vec![lake]),
            rivers: Vec::leak(vec![river]),
            islets: Vec::leak(vec![Islet {
                center: Vec2::ZERO,
                radius: 20.0,
            }]),
            ..default()
        };
        assert_eq!(waters.distance(Vec2::new(0.0, 50.0)), -30.0);
        assert_eq!(waters.distance(Vec2::ZERO), 20.0, "on the islet");
        assert_eq!(
            waters.distance(Vec2::new(200.0, 0.0)),
            90.0,
            "the river's nearer"
        );
        // The river wider downstream.
        assert_eq!(waters.distance(Vec2::new(400.0, 500.0)), 80.0);
        assert_eq!(Waters::default().distance(Vec2::ZERO), NO_WATER);
    }

    #[test]
    fn a_shore_wanders_as_far_as_it_says() {
        let lake = Lake {
            center: Vec2::ZERO,
            radius: 100.0,
            wander: Some(Wander {
                amount: 8.0,
                scale: 50.0,
                octaves: 2,
                offset: Vec2::splat(11.0),
            }),
        };
        for k in 0..360 {
            let point = Vec2::from_angle(k as f32 / 360.0 * std::f32::consts::TAU) * 100.0;
            assert!(lake.distance(point).abs() <= 8.0);
        }
    }
}
