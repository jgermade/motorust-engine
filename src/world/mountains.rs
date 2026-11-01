//! The mountains far off round a circuit, closing its horizon, as shapes a
//! circuit's file gives with their parameters (see [`Mountains`]): ranges
//! of rolling peaks rising round it, or past a line, and lone cones with a
//! crater at their tops, the highest of them at each point.

use bevy::prelude::*;
use serde::Deserialize;

use crate::geometry::{fbm, smoothstep};

/// A circuit's mountains: the highest of its shapes at each point, and none
/// where it has none (indoors, say).
#[derive(Deserialize, Default, Clone, Copy)]
#[serde(transparent)]
pub struct Mountains(
    #[serde(deserialize_with = "crate::circuits::data::leak")] pub &'static [Mountain],
);

impl Mountains {
    /// How high the mountains stand at `point` (m).
    pub fn height(&self, point: Vec2) -> f32 {
        self.0
            .iter()
            .map(|mountain| mountain.height(point))
            .reduce(f32::max)
            .unwrap_or(0.0)
    }
}

/// One of a circuit's mountains.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub enum Mountain {
    /// A range of rolling peaks round a circuit: none within `near` metres
    /// of its `center`, rising to their full height at `far`, all round it;
    /// or, with `toward`, past a line through the centre square to that
    /// way, as far into the land that way.
    Range {
        center: Vec2,
        near: f32,
        far: f32,
        #[serde(default)]
        toward: Option<Vec2>,
        /// Its edge wandering in and out, not a circle nor a straight line.
        #[serde(default)]
        ragged: Option<Ragged>,
        /// Only the way it faces: none the other way, open sea, say.
        #[serde(default)]
        facing: Option<Facing>,
        /// None near a point, round a peak standing alone.
        #[serde(default)]
        clear_of: Option<Clear>,
        /// Flat-topped mesas and buttes, not rolling peaks.
        #[serde(default)]
        mesas: Option<Mesas>,
        /// How high its peaks stand.
        relief: Relief,
    },
    /// A lone mountain: a cone `height` metres high and `radius` across at
    /// its foot, its flanks sweeping up ever steeper (`steepness`, over 1)
    /// to a crater `crater` metres deep at its top, as wide as `rim` of its
    /// radius; and, if it has one, the relief of its flanks, a function by
    /// name (see `terrain_function` in `src/circuits/mod.rs`), added to it:
    /// gullies, say.
    Cone {
        center: Vec2,
        radius: f32,
        height: f32,
        steepness: f32,
        crater: f32,
        rim: f32,
        #[serde(default, deserialize_with = "crate::circuits::data::optional_function")]
        relief: Option<fn(Vec2) -> f32>,
    },
}

impl Mountain {
    fn height(&self, point: Vec2) -> f32 {
        match *self {
            Mountain::Range {
                center,
                near,
                far,
                toward,
                ragged,
                facing,
                clear_of,
                mesas,
                relief,
            } => {
                let away = point - center;
                let mut distance = match toward {
                    Some(toward) => away.dot(toward.normalize()),
                    None => away.length(),
                };
                if let Some(ragged) = ragged {
                    distance += ragged.amount * (fbm(point / ragged.scale, ragged.octaves) - 0.5);
                }
                let mut rise = smoothstep(near, far, distance);
                if let Some(facing) = facing {
                    let way = away.normalize_or_zero().dot(facing.direction);
                    rise *= smoothstep(facing.from, facing.to, way);
                }
                if let Some(clear) = clear_of {
                    let reach = point.distance(clear.center) / clear.radius;
                    rise *= smoothstep(clear.from, clear.to, reach);
                }
                if let Some(mesas) = mesas {
                    let noise = fbm(point / mesas.scale, mesas.octaves);
                    rise *= smoothstep(mesas.from, mesas.to, noise);
                }
                rise * relief.at(point)
            }
            Mountain::Cone {
                center,
                radius,
                height,
                steepness,
                crater,
                rim,
                relief,
            } => {
                let reach = point.distance(center) / radius;
                let cone = height * (1.0 - reach).max(0.0).powf(steepness);
                let crater = crater * (1.0 - smoothstep(0.0, rim, reach));
                cone - crater + relief.map_or(0.0, |relief| relief(point))
            }
        }
    }
}

/// An edge wandering in and out by up to `amount` metres, in waves about
/// `scale` metres across.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Ragged {
    pub amount: f32,
    pub scale: f32,
    pub octaves: u32,
}

/// Which way a range faces: none where the way from its centre is further
/// from `direction` than `from` (the product of the two, the way as a unit
/// vector), all of it from `to` on.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Facing {
    pub direction: Vec2,
    pub from: f32,
    pub to: f32,
}

/// Clear of a point: none within `from` times `radius` of `center`, all of
/// it from `to` times on.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Clear {
    pub center: Vec2,
    pub radius: f32,
    pub from: f32,
    pub to: f32,
}

/// Mesas: where noise about `scale` metres across goes over `from`, rising
/// sheer, flat on top from `to` up.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Mesas {
    pub from: f32,
    pub to: f32,
    pub scale: f32,
    pub octaves: u32,
}

/// How high a range's peaks stand: from `low` to `high` metres, by noise
/// about `scale` metres across (shifted by `offset`, so that two ranges are
/// not alike), raised to `power` (over 1, sharper peaks over lower land).
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Relief {
    pub low: f32,
    pub high: f32,
    pub scale: f32,
    pub octaves: u32,
    #[serde(default)]
    pub offset: Vec2,
    #[serde(default = "one")]
    pub power: f32,
}

fn one() -> f32 {
    1.0
}

impl Relief {
    fn at(&self, point: Vec2) -> f32 {
        let noise = fbm(point / self.scale + self.offset, self.octaves);
        self.low + (self.high - self.low) * noise.powf(self.power)
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    fn relief(low: f32, high: f32) -> Relief {
        Relief {
            low,
            high,
            scale: 300.0,
            octaves: 4,
            offset: Vec2::ZERO,
            power: 1.0,
        }
    }

    fn range(
        near: f32,
        far: f32,
        toward: Option<Vec2>,
        facing: Option<Facing>,
        relief: Relief,
    ) -> Mountain {
        Mountain::Range {
            center: Vec2::ZERO,
            near,
            far,
            toward,
            ragged: None,
            facing,
            clear_of: None,
            mesas: None,
            relief,
        }
    }

    fn cone(center: Vec2, height: f32) -> Mountain {
        Mountain::Cone {
            center,
            radius: 1000.0,
            height,
            steepness: 1.5,
            crater: 50.0,
            rim: 0.1,
            relief: None,
        }
    }

    #[test]
    fn a_range_rises_from_near_to_far_and_only_the_way_it_faces() {
        let facing = Facing {
            direction: Vec2::X,
            from: 0.0,
            to: 0.5,
        };
        let range = range(1000.0, 2000.0, None, Some(facing), relief(100.0, 100.0));
        assert_eq!(range.height(Vec2::X * 900.0), 0.0);
        assert_eq!(range.height(Vec2::X * 2100.0), 100.0);
        assert!((range.height(Vec2::X * 1500.0) - 50.0).abs() < 1e-3);
        // The other way, none.
        assert_eq!(range.height(Vec2::NEG_X * 2100.0), 0.0);
    }

    #[test]
    fn a_range_past_a_line_and_its_peaks_between_low_and_high() {
        let range = range(
            0.0,
            100.0,
            Some(Vec2::new(0.0, -2.0)),
            None,
            relief(40.0, 100.0),
        );
        for x in [-5000.0, 0.0, 5000.0] {
            assert_eq!(range.height(Vec2::new(x, 50.0)), 0.0);
            let peak = range.height(Vec2::new(x, -500.0));
            assert!((40.0..=100.0).contains(&peak), "{peak}");
        }
    }

    #[test]
    fn a_cone_peaks_round_its_crater_and_the_highest_shape_stands() {
        let lone = cone(Vec2::ZERO, 500.0);
        assert_eq!(lone.height(Vec2::ZERO), 450.0);
        // At its rim, the cone with no crater in it.
        assert!((lone.height(Vec2::X * 100.0) - 500.0 * 0.9f32.powf(1.5)).abs() < 1e-3);
        assert_eq!(lone.height(Vec2::X * 1000.0), 0.0);
        let mountains = Mountains(Vec::leak(vec![lone, cone(Vec2::X * 3000.0, 800.0)]));
        assert_eq!(mountains.height(Vec2::ZERO), 450.0);
        assert_eq!(mountains.height(Vec2::X * 3000.0), 750.0);
        assert_eq!(Mountains::default().height(Vec2::ZERO), 0.0);
    }

    #[test]
    fn a_file_writes_them_as_their_fields() {
        let mountains: Mountains = ron::from_str(
            "[Range(center: (0.0, 0.0), near: 10.0, far: 20.0, relief: (low: 5.0, high: 5.0, \
             scale: 100.0, octaves: 2)), Cone(center: (0.0, 0.0), radius: 10.0, height: 9.0, \
             steepness: 1.0, crater: 0.0, rim: 0.1)]",
        )
        .unwrap();
        assert_eq!(mountains.height(Vec2::ZERO), 9.0);
        assert_eq!(mountains.height(Vec2::X * 30.0), 5.0);
    }
}
