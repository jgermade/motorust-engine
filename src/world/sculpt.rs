//! A circuit's ground as shaped by hand, over the land its scenery makes
//! (`super::terrain`): how much higher or lower it is at each point of a
//! regular grid ([`Sculpt`]), as the map editor's brushes leave it
//! ([`Brush`]: raise, lower, smooth, flatten, restore, as a terrain editor's).
//! Near the roads the road still has its way: the ground beside them
//! follows them as ever, and is shaped only past their walls (see
//! `Shaper::height_at`).
//!
//! It is kept in a file of its own beside the circuit's (its companion,
//! `circuits/NAME.terrain` beside `circuits/NAME.ron`, see [`companion`]),
//! binary: a header and a height a point of the grid, in centimetres
//! ([`Sculpt::encode`]).

use bevy::prelude::*;

use crate::geometry::smoothstep;

/// The companion file of a circuit's file, where its ground's shaping is.
pub fn companion(circuit_file: &str) -> String {
    match circuit_file.strip_suffix(".ron") {
        Some(stem) => format!("{stem}.terrain"),
        None => format!("{circuit_file}.terrain"),
    }
}

/// How far apart the points of a new grid are (m).
pub const CELL: f32 = 2.0;
/// The file's first bytes, and its version.
const MAGIC: &[u8; 4] = b"MRSC";
const VERSION: u32 = 2;
/// The most a point is raised or lowered (m): what a file's centimetres
/// hold.
pub const MOST: f32 = 327.0;

/// How much higher (or lower) the ground is at each point of a regular
/// grid, from `origin` (x, z) `cell` m apart, `width` points across (x) and
/// `depth` down (z), by rows; and what ground each point was painted as, if
/// any (see [`Paint`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Sculpt {
    pub origin: Vec2,
    pub cell: f32,
    pub width: usize,
    pub depth: usize,
    pub heights: Vec<f32>,
    pub paint: Vec<Paint>,
}

/// What the ground at a point was painted as: as the land makes it (its
/// slope, its height, by the sea), or grass, sand or rock whatever it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Paint {
    #[default]
    Land,
    Grass,
    Sand,
    Rock,
}

impl Paint {
    fn byte(self) -> u8 {
        match self {
            Paint::Land => 0,
            Paint::Grass => 1,
            Paint::Sand => 2,
            Paint::Rock => 3,
        }
    }

    fn of(byte: u8) -> Self {
        match byte {
            1 => Paint::Grass,
            2 => Paint::Sand,
            3 => Paint::Rock,
            _ => Paint::Land,
        }
    }

    /// The ground it paints, if any.
    pub fn ground(self) -> Option<super::terrain::Ground> {
        use super::terrain::Ground;
        match self {
            Paint::Land => None,
            Paint::Grass => Some(Ground::Grass),
            Paint::Sand => Some(Ground::Sand),
            Paint::Rock => Some(Ground::Rock),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Paint::Land => "unpaint",
            Paint::Grass => "grass",
            Paint::Sand => "sand",
            Paint::Rock => "rock",
        }
    }
}

impl Sculpt {
    /// A grid with nothing shaped yet over the box from `min` to `max`.
    pub fn flat(min: Vec2, max: Vec2, cell: f32) -> Self {
        let size = ((max - min) / cell).ceil().max(Vec2::ONE);
        let (width, depth) = (size.x as usize + 1, size.y as usize + 1);
        Self {
            origin: min,
            cell,
            width,
            depth,
            heights: vec![0.0; width * depth],
            paint: vec![Paint::Land; width * depth],
        }
    }

    /// Where point (`i`, `j`) of the grid is.
    pub fn point(&self, i: usize, j: usize) -> Vec2 {
        self.origin + Vec2::new(i as f32, j as f32) * self.cell
    }

    /// How much higher the ground is at `point` (interpolated between the
    /// points round it; nothing off the grid).
    pub fn at(&self, point: Vec2) -> f32 {
        let local = (point - self.origin) / self.cell;
        if local.x < 0.0 || local.y < 0.0 {
            return 0.0;
        }
        let (i, j) = (local.x.floor() as usize, local.y.floor() as usize);
        if i + 1 >= self.width || j + 1 >= self.depth {
            return 0.0;
        }
        let (tx, tz) = (local.x.fract(), local.y.fract());
        let h = |i: usize, j: usize| self.heights[j * self.width + i];
        let near = h(i, j).lerp(h(i + 1, j), tx);
        let far = h(i, j + 1).lerp(h(i + 1, j + 1), tx);
        near.lerp(far, tz)
    }

    /// Whether anything is shaped or painted.
    pub fn is_flat(&self) -> bool {
        self.heights.iter().all(|&h| h == 0.0) && self.paint.iter().all(|&p| p == Paint::Land)
    }

    /// What the ground at `point` was painted as: the nearest point of the
    /// grid's (as the land makes it off the grid).
    pub fn paint_at(&self, point: Vec2) -> Paint {
        let local = ((point - self.origin) / self.cell).round();
        if local.x < 0.0 || local.y < 0.0 {
            return Paint::Land;
        }
        let (i, j) = (local.x as usize, local.y as usize);
        if i >= self.width || j >= self.depth {
            return Paint::Land;
        }
        self.paint[j * self.width + i]
    }

    /// The grid's points within `radius` of `center`: their indices in
    /// `heights` and where they are.
    fn within(&self, center: Vec2, radius: f32) -> impl Iterator<Item = (usize, Vec2)> + '_ {
        let low = ((center - radius - self.origin) / self.cell)
            .floor()
            .max(Vec2::ZERO);
        let high = ((center + radius - self.origin) / self.cell).ceil();
        let (i0, j0) = (low.x as usize, low.y as usize);
        let i1 = (high.x.max(0.0) as usize).min(self.width.saturating_sub(1));
        let j1 = (high.y.max(0.0) as usize).min(self.depth.saturating_sub(1));
        (j0..=j1)
            .flat_map(move |j| (i0..=i1).map(move |i| (i, j)))
            .map(|(i, j)| (j * self.width + i, self.point(i, j)))
            .filter(move |&(_, p)| p.distance(center) <= radius)
    }

    /// The brush applied at `center` for `dt` seconds. `ground` is how high
    /// the ground is at a point with nothing shaped (to smooth and flatten
    /// the ground as it stands, shaped and all); `level`, for flattening,
    /// the height it flattens to.
    pub fn apply(
        &mut self,
        brush: &Brush,
        center: Vec2,
        dt: f32,
        ground: impl Fn(Vec2) -> f32,
        level: f32,
    ) {
        let points: Vec<(usize, Vec2)> = self.within(center, brush.radius).collect();
        // Each point's weight, and how much of the way it goes this time.
        let weight = |p: Vec2| brush.weight(p.distance(center));
        let rate = |p: Vec2| (brush.strength * dt * weight(p) / 4.0).min(1.0);
        let now = |sculpt: &Self, index: usize, p: Vec2| ground(p) + sculpt.heights[index];
        // Painting: the points the brush covers over its inner part and a
        // little more, painted.
        if let Tool::Paint(paint) = brush.tool {
            for &(index, p) in &points {
                if weight(p) > 0.3 {
                    self.paint[index] = paint;
                }
            }
            return;
        }
        let changes: Vec<(usize, f32)> = points
            .iter()
            .map(|&(index, p)| {
                let h = self.heights[index];
                let next = match brush.tool {
                    Tool::Paint(_) => h,
                    Tool::Raise => h + brush.strength * dt * weight(p),
                    Tool::Lower => h - brush.strength * dt * weight(p),
                    Tool::Restore => h * (1.0 - rate(p)),
                    Tool::Flatten => h + (level - now(self, index, p)) * rate(p),
                    Tool::Smooth => {
                        // Toward the average of the ground round it, a fifth
                        // of the brush across.
                        let reach = (brush.radius / 5.0).max(self.cell);
                        let around = [
                            Vec2::new(reach, 0.0),
                            Vec2::new(-reach, 0.0),
                            Vec2::new(0.0, reach),
                            Vec2::new(0.0, -reach),
                        ];
                        let average = around
                            .iter()
                            .map(|&offset| ground(p + offset) + self.at(p + offset))
                            .sum::<f32>()
                            / 4.0;
                        h + (average - now(self, index, p)) * rate(p)
                    }
                };
                (index, next.clamp(-MOST, MOST))
            })
            .collect();
        for (index, height) in changes {
            self.heights[index] = height;
        }
    }

    /// As the file has it: its header (the magic, the version, the origin,
    /// the cell, the width and the depth), a height a point, in
    /// centimetres, all little-endian, and a byte a point for its paint
    /// (since version 2).
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(28 + 2 * self.heights.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        for value in [self.origin.x, self.origin.y, self.cell] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for count in [self.width as u32, self.depth as u32] {
            bytes.extend_from_slice(&count.to_le_bytes());
        }
        for &height in &self.heights {
            let centimetres = (height * 100.0)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32);
            bytes.extend_from_slice(&(centimetres as i16).to_le_bytes());
        }
        bytes.extend(self.paint.iter().map(|paint| paint.byte()));
        bytes
    }

    /// Read back from what [`Self::encode`] wrote, or what is wrong.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let word = |at: usize| -> Result<[u8; 4], String> {
            bytes
                .get(at..at + 4)
                .and_then(|slice| slice.try_into().ok())
                .ok_or_else(|| "too short".to_string())
        };
        if bytes.get(0..4) != Some(MAGIC.as_slice()) {
            return Err("not a terrain's shaping".into());
        }
        let version = u32::from_le_bytes(word(4)?);
        if !(1..=VERSION).contains(&version) {
            return Err(format!("version {version}, not {VERSION}"));
        }
        let float = |at: usize| word(at).map(f32::from_le_bytes);
        let (x, z, cell) = (float(8)?, float(12)?, float(16)?);
        let width = u32::from_le_bytes(word(20)?) as usize;
        let depth = u32::from_le_bytes(word(24)?) as usize;
        let body = &bytes[28..];
        let count = width * depth;
        // Version 1 had no paint.
        let paint_bytes = if version >= 2 { count } else { 0 };
        if body.len() != 2 * count + paint_bytes || cell.is_nan() || cell <= 0.0 {
            return Err(format!(
                "{width} by {depth} points, {} bytes of them",
                body.len()
            ));
        }
        let heights = body[..2 * count]
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 100.0)
            .collect();
        let paint = if version >= 2 {
            body[2 * count..]
                .iter()
                .map(|&byte| Paint::of(byte))
                .collect()
        } else {
            vec![Paint::Land; count]
        };
        Ok(Self {
            origin: Vec2::new(x, z),
            cell,
            width,
            depth,
            heights,
            paint,
        })
    }
}

/// What a brush does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Raise,
    Lower,
    Smooth,
    Flatten,
    /// Back toward the land as its scenery makes it.
    Restore,
    /// The ground painted (see [`Paint`]).
    Paint(Paint),
}

impl Tool {
    pub const ALL: [Tool; 9] = [
        Tool::Raise,
        Tool::Lower,
        Tool::Smooth,
        Tool::Flatten,
        Tool::Restore,
        Tool::Paint(Paint::Grass),
        Tool::Paint(Paint::Sand),
        Tool::Paint(Paint::Rock),
        Tool::Paint(Paint::Land),
    ];

    /// Whether it paints rather than shapes.
    pub fn paints(self) -> bool {
        matches!(self, Tool::Paint(_))
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Raise => "raise",
            Tool::Lower => "lower",
            Tool::Smooth => "smooth",
            Tool::Flatten => "flatten",
            Tool::Restore => "restore",
            Tool::Paint(paint) => paint.name(),
        }
    }
}

/// A brush: its tool, how far it reaches (m) and how strong it is (m a
/// second at its middle, raising or lowering; how fast it smooths, flattens
/// or restores).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub tool: Tool,
    pub radius: f32,
    pub strength: f32,
}

/// How big and how strong a brush may be.
pub const RADIUS: (f32, f32) = (4.0, 200.0);
pub const STRENGTH: (f32, f32) = (0.5, 40.0);

impl Default for Brush {
    fn default() -> Self {
        Self {
            tool: Tool::Raise,
            radius: 30.0,
            strength: 6.0,
        }
    }
}

impl Brush {
    /// How much it does `distance` from its middle: all of it over its
    /// inner half, fading out to nothing at its edge.
    pub fn weight(&self, distance: f32) -> f32 {
        1.0 - smoothstep(self.radius * 0.5, self.radius, distance)
    }

    /// Bigger (`steps` > 0) or smaller, a tenth each step.
    pub fn resized(self, steps: f32) -> Self {
        Self {
            radius: (self.radius * 1.1f32.powf(steps)).clamp(RADIUS.0, RADIUS.1),
            ..self
        }
    }

    /// Stronger (`steps` > 0) or weaker, a fifth each step.
    pub fn strengthened(self, steps: f32) -> Self {
        Self {
            strength: (self.strength * 1.2f32.powf(steps)).clamp(STRENGTH.0, STRENGTH.1),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Sculpt {
        Sculpt::flat(Vec2::new(-100.0, -100.0), Vec2::new(100.0, 100.0), CELL)
    }

    fn brush(tool: Tool) -> Brush {
        Brush {
            tool,
            radius: 20.0,
            strength: 10.0,
        }
    }

    #[test]
    fn a_companion_sits_beside_its_circuit() {
        assert_eq!(companion("circuits/bahia.ron"), "circuits/bahia.terrain");
        assert_eq!(companion("odd"), "odd.terrain");
    }

    #[test]
    fn raising_lifts_the_middle_most_and_nothing_past_the_edge() {
        let mut sculpt = grid();
        assert!(sculpt.is_flat());
        sculpt.apply(&brush(Tool::Raise), Vec2::ZERO, 0.5, |_| 0.0, 0.0);
        assert!((sculpt.at(Vec2::ZERO) - 5.0).abs() < 1e-4);
        assert!(sculpt.at(Vec2::new(15.0, 0.0)) < 5.0);
        assert!(sculpt.at(Vec2::new(15.0, 0.0)) > 0.0);
        assert_eq!(sculpt.at(Vec2::new(25.0, 0.0)), 0.0);
        assert!(!sculpt.is_flat());
        sculpt.apply(&brush(Tool::Lower), Vec2::ZERO, 1.0, |_| 0.0, 0.0);
        assert!((sculpt.at(Vec2::ZERO) + 5.0).abs() < 1e-4);
    }

    #[test]
    fn off_the_grid_nothing_is_shaped() {
        let mut sculpt = grid();
        sculpt.apply(
            &brush(Tool::Raise),
            Vec2::new(95.0, 95.0),
            1.0,
            |_| 0.0,
            0.0,
        );
        assert_eq!(sculpt.at(Vec2::new(150.0, 0.0)), 0.0);
        assert_eq!(sculpt.at(Vec2::new(-150.0, 0.0)), 0.0);
        assert!(sculpt.at(Vec2::new(95.0, 95.0)) > 0.0);
    }

    #[test]
    fn flattening_brings_the_ground_to_the_level() {
        let mut sculpt = grid();
        // A slope, flattened to 3 m round the middle.
        let slope = |p: Vec2| p.x * 0.5;
        for _ in 0..60 {
            sculpt.apply(&brush(Tool::Flatten), Vec2::ZERO, 0.1, slope, 3.0);
        }
        for x in [-6.0, 0.0, 6.0] {
            let p = Vec2::new(x, 0.0);
            assert!((slope(p) + sculpt.at(p) - 3.0).abs() < 0.1, "{x}");
        }
    }

    #[test]
    fn smoothing_takes_a_spike_down_and_restoring_takes_it_all() {
        let mut sculpt = grid();
        let spike = Brush {
            radius: 6.0,
            ..brush(Tool::Raise)
        };
        sculpt.apply(&spike, Vec2::ZERO, 1.0, |_| 0.0, 0.0);
        let top = sculpt.at(Vec2::ZERO);
        for _ in 0..30 {
            sculpt.apply(&brush(Tool::Smooth), Vec2::ZERO, 0.1, |_| 0.0, 0.0);
        }
        assert!(
            sculpt.at(Vec2::ZERO) < 0.6 * top,
            "{} of {top}",
            sculpt.at(Vec2::ZERO)
        );
        for _ in 0..200 {
            sculpt.apply(&brush(Tool::Restore), Vec2::ZERO, 0.1, |_| 0.0, 0.0);
        }
        assert!(sculpt.at(Vec2::ZERO).abs() < 0.01);
    }

    #[test]
    fn painting_covers_the_brush_and_unpainting_takes_it_off() {
        let mut sculpt = grid();
        let paint = |paint| Brush {
            tool: Tool::Paint(paint),
            ..brush(Tool::Raise)
        };
        sculpt.apply(&paint(Paint::Sand), Vec2::ZERO, 0.1, |_| 0.0, 0.0);
        assert_eq!(sculpt.paint_at(Vec2::ZERO), Paint::Sand);
        assert_eq!(sculpt.paint_at(Vec2::new(10.0, 0.0)), Paint::Sand);
        assert_eq!(sculpt.paint_at(Vec2::new(30.0, 0.0)), Paint::Land);
        assert_eq!(sculpt.paint_at(Vec2::new(500.0, 0.0)), Paint::Land);
        assert_eq!(sculpt.at(Vec2::ZERO), 0.0, "painting shapes nothing");
        assert!(!sculpt.is_flat());
        sculpt.apply(&paint(Paint::Land), Vec2::ZERO, 0.1, |_| 0.0, 0.0);
        assert!(sculpt.is_flat());
        assert_eq!(
            Paint::Rock.ground(),
            Some(super::super::terrain::Ground::Rock)
        );
        assert_eq!(Paint::Land.ground(), None);
    }

    #[test]
    fn a_file_of_the_first_version_reads_with_no_paint() {
        let mut sculpt = grid();
        sculpt.heights[5] = 1.5;
        let mut bytes = sculpt.encode();
        bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
        bytes.truncate(bytes.len() - sculpt.width * sculpt.depth);
        let read = Sculpt::decode(&bytes).unwrap();
        assert_eq!(read.heights[5], 1.5);
        assert!(read.paint.iter().all(|&p| p == Paint::Land));
    }

    #[test]
    fn a_file_reads_back_as_it_was_written_to_the_centimetre() {
        let mut sculpt = grid();
        sculpt.apply(
            &brush(Tool::Raise),
            Vec2::new(10.0, -20.0),
            0.37,
            |_| 0.0,
            0.0,
        );
        let read = Sculpt::decode(&sculpt.encode()).unwrap();
        assert_eq!(
            (read.origin, read.cell, read.width, read.depth),
            (sculpt.origin, sculpt.cell, sculpt.width, sculpt.depth)
        );
        for (a, b) in read.heights.iter().zip(&sculpt.heights) {
            assert!((a - b).abs() <= 0.005);
        }
        assert!(Sculpt::decode(b"nonsense").is_err());
        let mut short = sculpt.encode();
        short.pop();
        assert!(Sculpt::decode(&short).is_err());
    }

    #[test]
    fn a_brush_grows_and_strengthens_within_its_limits() {
        let brush = Brush::default();
        assert!(brush.resized(1.0).radius > brush.radius);
        assert_eq!(brush.resized(-100.0).radius, RADIUS.0);
        assert_eq!(brush.strengthened(100.0).strength, STRENGTH.1);
        assert_eq!(brush.weight(0.0), 1.0);
        assert_eq!(brush.weight(brush.radius), 0.0);
    }
}
