//! A grid over a road's centre line: which of its segments pass through
//! each square, so that the nearest one to a point is found by looking only
//! at the squares around it instead of going through them all. A lap of a
//! long track has thousands of segments, and the bikes ask where they are
//! on it many times a step.

use bevy::prelude::*;

/// The side of a square (m): a few segments long, and about a road wide,
/// so the nearest segment is mostly in the point's square or the next.
const CELL: f32 = 16.0;

/// How far a square's distance to a point, worked out on its corners, may
/// fall short of that of a segment inside it, worked out on the segment,
/// with rounding (m). Squares are skipped only past it, so rounding never
/// leaves out what a search through every segment would find.
const SLACK: f32 = 0.01;

/// The segments of a centre line by the squares of a grid over it.
#[derive(Clone, Debug, Default)]
pub(super) struct SegmentGrid {
    /// The corner of the first square, with the least x and z.
    origin: Vec2,
    columns: i32,
    rows: i32,
    /// Where each square's segments start in `segments` (and the next
    /// square's start, where they end), square by square along the rows.
    starts: Vec<u32>,
    segments: Vec<u32>,
}

impl SegmentGrid {
    /// The grid over the segments from `segment(index)` (its two ends) for
    /// every index below `count`: each is in every square its bounding box
    /// touches, and so its nearest point to anything is in one of them.
    pub(super) fn new(count: usize, segment: impl Fn(usize) -> (Vec2, Vec2)) -> Self {
        if count == 0 {
            return Self::default();
        }
        let (min, max) = (0..count).fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(min, max), index| {
                let (a, b) = segment(index);
                (min.min(a).min(b), max.max(a).max(b))
            },
        );
        let origin = min;
        let size = ((max - min) / CELL).floor().as_ivec2() + IVec2::ONE;
        let mut grid = Self {
            origin,
            columns: size.x,
            rows: size.y,
            starts: Vec::new(),
            segments: Vec::new(),
        };
        let squares = |index: usize| {
            let (a, b) = segment(index);
            let (from, to) = (grid.square_of(a.min(b)), grid.square_of(a.max(b)));
            (from.y..=to.y).flat_map(move |y| (from.x..=to.x).map(move |x| IVec2::new(x, y)))
        };
        let mut counts = vec![0u32; (grid.columns * grid.rows) as usize + 1];
        for index in 0..count {
            for square in squares(index) {
                counts[grid.slot(square) + 1] += 1;
            }
        }
        for slot in 1..counts.len() {
            counts[slot] += counts[slot - 1];
        }
        let mut filled = counts.clone();
        let mut segments = vec![0u32; counts[counts.len() - 1] as usize];
        for index in 0..count {
            for square in squares(index) {
                let slot = grid.slot(square);
                segments[filled[slot] as usize] = index as u32;
                filled[slot] += 1;
            }
        }
        grid.starts = counts;
        grid.segments = segments;
        grid
    }

    /// Whether the grid has no segments (one not made yet): then it cannot
    /// be searched.
    pub(super) fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// The square `point` is in, kept to the grid's.
    fn square_of(&self, point: Vec2) -> IVec2 {
        let square = ((point - self.origin) / CELL).floor().as_ivec2();
        square.clamp(IVec2::ZERO, IVec2::new(self.columns - 1, self.rows - 1))
    }

    fn slot(&self, square: IVec2) -> usize {
        (square.y * self.columns + square.x) as usize
    }

    /// The segment with the least `key` (worked out by `key(index)`, `None`
    /// to leave it out), the lowest index among equal keys, as going through
    /// every segment in order and keeping only one strictly less would find,
    /// among those that may be within `reach` of `point` (m). The key may
    /// be the distance to `point` or grow with it: `bound(distance)` is the
    /// least key of something that far from `point`.
    pub(super) fn search(
        &self,
        point: Vec2,
        reach: f32,
        bound: impl Fn(f32) -> f32,
        mut key: impl FnMut(usize) -> Option<f32>,
    ) -> Option<(f32, usize)> {
        let mut best: Option<(f32, usize)> = None;
        // A bound that some key to come may reach: the slack makes it
        // smaller, so nothing equal to the best is left out by rounding.
        let beyond = |best: Option<(f32, usize)>, distance: f32| {
            let distance = (distance - SLACK).max(0.0);
            distance > reach || best.is_some_and(|(least, _)| bound(distance) > least)
        };
        // The point's own square, off the grid if the point is: the rings
        // round it start at the first that reaches the grid. (Kept within a
        // million squares of it, so the rings' corners are numbers.)
        let last = IVec2::new(self.columns - 1, self.rows - 1);
        let centre = ((point - self.origin) / CELL)
            .floor()
            .as_ivec2()
            .clamp(IVec2::splat(-(1 << 20)), last + IVec2::splat(1 << 20));
        let first = (IVec2::ZERO - centre).max(centre - last).max(IVec2::ZERO);
        for ring in first.max_element().. {
            let (from, to) = (centre - IVec2::splat(ring), centre + IVec2::splat(ring));
            // The ring's squares on the grid: its top and bottom rows whole,
            // and its two sides between them.
            for y in from.y.max(0)..=to.y.min(last.y) {
                let (row, step) = if y == from.y || y == to.y {
                    (from.x.max(0)..=to.x.min(last.x), 1)
                } else {
                    (from.x..=to.x, (to.x - from.x) as usize)
                };
                for x in row.step_by(step.max(1)) {
                    if x < 0 || x > last.x {
                        continue;
                    }
                    let square = IVec2::new(x, y);
                    if beyond(best, self.distance_to(square, point)) {
                        continue;
                    }
                    let slot = self.slot(square);
                    let (start, end) = (self.starts[slot], self.starts[slot + 1]);
                    for &index in &self.segments[start as usize..end as usize] {
                        let index = index as usize;
                        if let Some(found) = key(index)
                            && best.is_none_or(|(least, at)| {
                                found < least || (found == least && index < at)
                            })
                        {
                            best = Some((found, index));
                        }
                    }
                }
            }
            let covered =
                from.x <= 0 && from.y <= 0 && to.x >= self.columns - 1 && to.y >= self.rows - 1;
            if covered {
                break;
            }
            // How far the point is from the squares not yet searched: they
            // are all outside this ring's.
            let low = self.origin + from.as_vec2() * CELL;
            let high = self.origin + (to + IVec2::ONE).as_vec2() * CELL;
            let inside = (point - low).min(high - point).min_element();
            if inside > 0.0 && beyond(best, inside) {
                break;
            }
        }
        best
    }

    /// How far `point` is from the nearest point of a square (m).
    fn distance_to(&self, square: IVec2, point: Vec2) -> f32 {
        let low = self.origin + square.as_vec2() * CELL;
        let high = low + Vec2::splat(CELL);
        (low - point).max(point - high).max(Vec2::ZERO).length()
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    /// A zigzag of segments, and every segment's distance to a point.
    fn zigzag() -> Vec<(Vec2, Vec2)> {
        (0..60)
            .map(|i| {
                let a = Vec2::new(i as f32 * 3.0, if i % 2 == 0 { 0.0 } else { 40.0 });
                let b = Vec2::new((i + 1) as f32 * 3.0, if i % 2 == 0 { 40.0 } else { 0.0 });
                (a, b)
            })
            .collect()
    }

    fn distance((a, b): (Vec2, Vec2), point: Vec2) -> f32 {
        let ab = b - a;
        let t = ((point - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        point.distance(a + ab * t)
    }

    /// What going through every segment in order finds.
    fn every(
        segments: &[(Vec2, Vec2)],
        point: Vec2,
        keep: impl Fn(usize) -> bool,
    ) -> Option<(f32, usize)> {
        let mut best: Option<(f32, usize)> = None;
        for (index, &segment) in segments.iter().enumerate() {
            let found = distance(segment, point);
            if keep(index) && best.is_none_or(|(least, _)| found < least) {
                best = Some((found, index));
            }
        }
        best
    }

    #[test]
    fn finds_what_going_through_every_segment_finds() {
        let segments = zigzag();
        let grid = SegmentGrid::new(segments.len(), |i| segments[i]);
        let odd = |index: usize| index % 7 == 3;
        for x in (-60..250).step_by(7) {
            for y in (-90..130).step_by(5) {
                let point = Vec2::new(x as f32 * 1.03, y as f32 * 0.97);
                let found = grid.search(
                    point,
                    f32::INFINITY,
                    |d| d,
                    |i| Some(distance(segments[i], point)),
                );
                assert_eq!(found, every(&segments, point, |_| true), "at {point}");
                // Leaving some out, as the search for the road at a level does.
                let found = grid.search(
                    point,
                    f32::INFINITY,
                    |d| d,
                    |i| odd(i).then(|| distance(segments[i], point)),
                );
                assert_eq!(found, every(&segments, point, odd), "at {point}");
            }
        }
    }

    #[test]
    fn ties_go_to_the_lowest_index() {
        // Two segments meeting at a corner: the corner's own point is as
        // near both, and the first one counts.
        let segments = [
            (Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0)),
            (Vec2::new(20.0, 0.0), Vec2::new(20.0, 20.0)),
        ];
        let grid = SegmentGrid::new(2, |i| segments[i]);
        let point = Vec2::new(30.0, -10.0);
        let found = grid.search(
            point,
            f32::INFINITY,
            |d| d,
            |i| Some(distance(segments[i], point)),
        );
        assert_eq!(found.map(|(_, index)| index), Some(0));
    }

    #[test]
    fn nothing_past_its_reach_is_needed() {
        let segments = zigzag();
        let grid = SegmentGrid::new(segments.len(), |i| segments[i]);
        let point = Vec2::new(90.0, 200.0);
        let near = |reach: f32| {
            grid.search(point, reach, |d| d, |i| Some(distance(segments[i], point)))
                .filter(|&(found, _)| found <= reach)
        };
        assert_eq!(near(50.0), None);
        assert_eq!(near(200.0), every(&segments, point, |_| true));
    }

    #[test]
    fn a_point_far_off_the_grid_finds_the_nearest_too() {
        let segments = zigzag();
        let grid = SegmentGrid::new(segments.len(), |i| segments[i]);
        for point in [Vec2::new(-5_000.0, 30.0), Vec2::new(1.0e6, -1.0e6)] {
            let found = grid.search(
                point,
                f32::INFINITY,
                |d| d,
                |i| Some(distance(segments[i], point)),
            );
            assert_eq!(found, every(&segments, point, |_| true), "at {point}");
        }
    }
}
