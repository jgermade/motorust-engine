//! The lap's height profile and banking as the editor edits them: the
//! marks the road's height goes through ([`Mark`], the file's `elevation`)
//! and the stretches banked more or less than their bends ask
//! ([`Span`], the file's `banking`), with how each is written in a
//! circuit's file, and the panel that shows them along the lap
//! ([`Panel`]).

use bevy::prelude::*;

use super::file::{Item, centimetres};
use crate::track::Banking;

/// A mark of the height profile: `height` m over the finish line's level,
/// `along` m from the line. `was`: which of the file's marks it was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub along: f32,
    pub height: f32,
    pub was: Option<usize>,
}

/// A stretch of banking (see [`Banking`]): from `from` to `to` m along the
/// lap, its bends banked `factor` times as much, raised on supports or
/// not, gripping `grip` times as hard. `was`: which of the file's it was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub from: f32,
    pub to: f32,
    pub factor: f32,
    pub raised: bool,
    pub grip: f32,
    pub was: Option<usize>,
}

/// A stretch that runs to the end of the lap, however long it is, ends
/// here (as the files write it: `3.4028235e38`).
pub const TO_THE_END: f32 = f32::MAX;

/// The fewest marks a profile can have, and the shortest a stretch of
/// banking can be (m).
pub const FEWEST_MARKS: usize = 2;
pub const SHORTEST_SPAN: f32 = 4.0;
/// How far apart two marks must be (m).
pub const MARKS_APART: f32 = 2.0;
/// How much a key changes a stretch's factor and grip, and their bounds.
pub const FACTOR_STEP: f32 = 0.1;
pub const FACTOR: (f32, f32) = (0.0, 3.0);
pub const GRIP_STEP: f32 = 0.05;
pub const GRIP: (f32, f32) = (1.0, 3.0);
/// A new stretch's length (m) and its factor.
pub const NEW_SPAN: f32 = 60.0;
pub const NEW_FACTOR: f32 = 1.5;

impl Mark {
    /// The marks of a profile as read from the file.
    pub fn read(elevation: &[(f32, f32)]) -> Vec<Self> {
        elevation
            .iter()
            .enumerate()
            .map(|(was, &(along, height))| Self {
                along,
                height,
                was: Some(was),
            })
            .collect()
    }

    /// As the file writes it: `(along, height)`.
    pub fn text(&self) -> String {
        format!(
            "({}, {})",
            centimetres(self.along),
            centimetres(self.height)
        )
    }
}

impl Span {
    /// The stretches of banking as read from the file.
    pub fn read(banking: &[Banking]) -> Vec<Self> {
        banking
            .iter()
            .enumerate()
            .map(|(was, banking)| Self {
                from: banking.from,
                to: banking.to,
                factor: banking.factor,
                raised: banking.raised,
                grip: banking.grip,
                was: Some(was),
            })
            .collect()
    }

    /// As the game takes it.
    pub fn banking(&self) -> Banking {
        Banking {
            from: self.from,
            to: self.to,
            factor: self.factor,
            raised: self.raised,
            grip: self.grip,
        }
    }

    /// As the file writes it: what is as usual left out (not raised,
    /// gripping as any road).
    pub fn text(&self) -> String {
        let to = if self.to >= TO_THE_END {
            "3.4028235e38".to_string()
        } else {
            centimetres(self.to)
        };
        let mut text = format!(
            "(from: {}, to: {to}, factor: {}",
            centimetres(self.from),
            centimetres(self.factor)
        );
        if self.raised {
            text.push_str(", raised: true");
        }
        if self.grip != 1.0 {
            text.push_str(&format!(", grip: {}", centimetres(self.grip)));
        }
        text.push(')');
        text
    }

    /// Where it ends on a lap `lap` m long.
    pub fn end(&self, lap: f32) -> f32 {
        self.to.min(lap)
    }

    /// Whether it covers `along`.
    pub fn covers(&self, along: f32) -> bool {
        (self.from..self.to).contains(&along)
    }
}

/// The items of a list as the file is to have them: `read`, the values as
/// read from the file, to tell which changed.
pub fn items<T: Copy + PartialEq>(
    values: &[T],
    was: impl Fn(&T) -> Option<usize>,
    text: impl Fn(&T) -> String,
    read: &[T],
    same: impl Fn(&T, &T) -> bool,
) -> Vec<Item> {
    values
        .iter()
        .map(|value| Item {
            text: text(value),
            was: was(value),
            changed: was(value).is_none_or(|w| read.get(w).is_none_or(|r| !same(r, value))),
        })
        .collect()
}

/// Mark `index` moved to `along` and `height`, kept between the marks
/// either side of it (and on the lap, `lap` m long).
pub fn move_mark(marks: &mut [Mark], index: usize, along: f32, height: f32, lap: f32) {
    let low = if index == 0 {
        0.0
    } else {
        marks[index - 1].along + MARKS_APART
    };
    let high = marks
        .get(index + 1)
        .map_or(lap - MARKS_APART, |next| next.along - MARKS_APART);
    if let Some(mark) = marks.get_mut(index) {
        mark.along = along.clamp(low, high.max(low));
        mark.height = height;
    }
}

/// A mark halfway from mark `index` to the next (or to the end of the lap
/// for the last, at its height): where it goes in the list.
pub fn mark_after(marks: &[Mark], index: usize, lap: f32) -> Option<(usize, Mark)> {
    let mark = marks.get(index)?;
    let (along, height) = match marks.get(index + 1) {
        Some(next) => (
            (mark.along + next.along) / 2.0,
            (mark.height + next.height) / 2.0,
        ),
        None => ((mark.along + lap) / 2.0, mark.height),
    };
    let apart = marks.get(index + 1).map_or(lap, |next| next.along) - mark.along;
    (apart >= 2.0 * MARKS_APART).then_some((
        index + 1,
        Mark {
            along,
            height,
            was: None,
        },
    ))
}

/// A new stretch of banking at `along` on a lap `lap` m long, in the gap
/// between the stretches there (none if a stretch covers it): where it
/// goes in the list, and itself.
pub fn span_at(spans: &[Span], along: f32, lap: f32) -> Option<(usize, Span)> {
    if spans.iter().any(|span| span.covers(along)) || !(0.0..lap).contains(&along) {
        return None;
    }
    let index = spans.iter().take_while(|span| span.from <= along).count();
    let gap_from = index.checked_sub(1).map_or(0.0, |i| spans[i].end(lap));
    let gap_to = spans.get(index).map_or(lap, |span| span.from);
    let from = (along - NEW_SPAN / 2.0).max(gap_from);
    let to = (from + NEW_SPAN).min(gap_to);
    (to - from >= SHORTEST_SPAN).then_some((
        index,
        Span {
            from,
            to,
            factor: NEW_FACTOR,
            raised: false,
            grip: 1.0,
            was: None,
        },
    ))
}

/// Stretch `index` cut in two halfway, the second half new.
pub fn split_span(spans: &mut Vec<Span>, index: usize, lap: f32) -> bool {
    let Some(&span) = spans.get(index) else {
        return false;
    };
    let middle = (span.from + span.end(lap)) / 2.0;
    if middle - span.from < SHORTEST_SPAN {
        return false;
    }
    spans[index].to = middle;
    spans.insert(
        index + 1,
        Span {
            from: middle,
            was: None,
            ..span
        },
    );
    true
}

/// An end of stretch `index` (its start, or its end) moved to `along`,
/// kept off the stretches either side and no shorter than
/// [`SHORTEST_SPAN`]. A stretch's end moved where the next one starts
/// moves that one's start with it (and the other way).
pub fn move_span_end(spans: &mut [Span], index: usize, start: bool, along: f32, lap: f32) {
    let Some(&span) = spans.get(index) else {
        return;
    };
    if start {
        let joined = index > 0 && (spans[index - 1].to - span.from).abs() < 0.01;
        let low = if index == 0 {
            0.0
        } else if joined {
            spans[index - 1].from + SHORTEST_SPAN
        } else {
            spans[index - 1].to
        };
        let high = span.end(lap) - SHORTEST_SPAN;
        let at = along.clamp(low, high.max(low));
        spans[index].from = at;
        if joined {
            spans[index - 1].to = at;
        }
    } else {
        let joined = spans
            .get(index + 1)
            .is_some_and(|next| (next.from - span.to).abs() < 0.01);
        let low = span.from + SHORTEST_SPAN;
        let high = match spans.get(index + 1) {
            Some(next) if joined => next.end(lap) - SHORTEST_SPAN,
            Some(next) => next.from,
            None => lap,
        };
        let at = along.clamp(low, high.max(low));
        spans[index].to = at;
        if joined {
            spans[index + 1].from = at;
        }
    }
}

/// Where the panel along the bottom of the screen is and what it shows:
/// the lap from end to end across it, heights from `low` to `high` m up it
/// over the band of the banking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    /// Its corners on the screen (px, from the top left): the profile's
    /// box, and under it the banking's band.
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub band: f32,
    pub lap: f32,
    pub low: f32,
    pub high: f32,
}

/// How much of the window's height the panel takes, and its margins (px).
pub const PANEL_SHARE: f32 = 0.3;
pub const PANEL_MARGIN: f32 = 16.0;
pub const BAND_SHARE: f32 = 0.22;

impl Panel {
    /// The panel at the bottom of a window `size` px, for a lap `lap` m
    /// long whose road is `heights` m high.
    pub fn new(size: Vec2, lap: f32, heights: impl Iterator<Item = f32>) -> Self {
        let (low, high) = heights.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), h| {
            (lo.min(h), hi.max(h))
        });
        let (low, high) = if low.is_finite() {
            (low, high)
        } else {
            (0.0, 0.0)
        };
        // Never flatter than 20 m of height, a little over and under.
        let pad = ((20.0 - (high - low)) / 2.0).max(0.0) + 2.0;
        let top = size.y * (1.0 - PANEL_SHARE) + PANEL_MARGIN;
        let bottom = size.y - PANEL_MARGIN;
        Self {
            left: PANEL_MARGIN * 3.0,
            right: size.x - PANEL_MARGIN,
            top,
            bottom,
            band: bottom - (bottom - top) * BAND_SHARE,
            lap: lap.max(1.0),
            low: low - pad,
            high: high + pad,
        }
    }

    /// Whether the point `at` of the screen is on the panel (and on its
    /// band of banking).
    pub fn contains(&self, at: Vec2) -> bool {
        (self.left - PANEL_MARGIN..=self.right + PANEL_MARGIN).contains(&at.x)
            && (self.top - PANEL_MARGIN..=self.bottom + PANEL_MARGIN).contains(&at.y)
    }

    pub fn on_band(&self, at: Vec2) -> bool {
        self.contains(at) && at.y >= self.band
    }

    /// Where on the screen (px) `along` and `height` are.
    pub fn x(&self, along: f32) -> f32 {
        self.left + (self.right - self.left) * along / self.lap
    }

    pub fn y(&self, height: f32) -> f32 {
        self.band - (self.band - self.top) * (height - self.low) / (self.high - self.low)
    }

    /// How far along, and how high, the point `at` of the screen is.
    pub fn along(&self, x: f32) -> f32 {
        ((x - self.left) / (self.right - self.left) * self.lap).clamp(0.0, self.lap)
    }

    pub fn height(&self, y: f32) -> f32 {
        self.low + (self.band - y) / (self.band - self.top) * (self.high - self.low)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(points: &[(f32, f32)]) -> Vec<Mark> {
        Mark::read(points)
    }

    fn span(from: f32, to: f32) -> Span {
        Span {
            from,
            to,
            factor: 1.0,
            raised: false,
            grip: 1.0,
            was: None,
        }
    }

    #[test]
    fn marks_and_spans_are_written_as_the_files_write_them() {
        assert_eq!(marks(&[(120.0, 5.4)])[0].text(), "(120.0, 5.4)");
        let mut banked = span(0.0, 410.0);
        banked.factor = 1.5;
        banked.raised = true;
        assert_eq!(
            banked.text(),
            "(from: 0.0, to: 410.0, factor: 1.5, raised: true)"
        );
        banked.to = TO_THE_END;
        banked.raised = false;
        banked.grip = 1.25;
        assert_eq!(
            banked.text(),
            "(from: 0.0, to: 3.4028235e38, factor: 1.5, grip: 1.25)"
        );
        // Read back as written.
        let read: Banking = ron::from_str(&banked.text()).unwrap();
        assert_eq!(read.to, TO_THE_END);
        assert_eq!(read.grip, 1.25);
    }

    #[test]
    fn a_mark_moves_between_its_neighbours() {
        let mut list = marks(&[(0.0, 0.0), (100.0, 5.0), (200.0, 0.0)]);
        move_mark(&mut list, 1, 500.0, 8.0, 1000.0);
        assert_eq!((list[1].along, list[1].height), (198.0, 8.0));
        move_mark(&mut list, 1, -50.0, 8.0, 1000.0);
        assert_eq!(list[1].along, 2.0);
        move_mark(&mut list, 2, 2000.0, 1.0, 1000.0);
        assert_eq!(list[2].along, 998.0);
    }

    #[test]
    fn a_mark_added_goes_halfway_to_the_next() {
        let list = marks(&[(0.0, 0.0), (100.0, 6.0)]);
        let (index, mark) = mark_after(&list, 0, 1000.0).unwrap();
        assert_eq!((index, mark.along, mark.height), (1, 50.0, 3.0));
        let (index, mark) = mark_after(&list, 1, 1000.0).unwrap();
        assert_eq!((index, mark.along, mark.height), (2, 550.0, 6.0));
        let close = marks(&[(0.0, 0.0), (3.0, 1.0)]);
        assert_eq!(mark_after(&close, 0, 1000.0), None);
    }

    #[test]
    fn a_new_span_fills_the_gap_where_it_is_put() {
        let spans = vec![span(100.0, 200.0), span(300.0, TO_THE_END)];
        assert_eq!(span_at(&spans, 150.0, 1000.0), None, "already banked");
        assert_eq!(span_at(&spans, 500.0, 1000.0), None, "to the end");
        let (index, new) = span_at(&spans, 250.0, 1000.0).unwrap();
        assert_eq!((index, new.from, new.to), (1, 220.0, 280.0));
        let (index, new) = span_at(&spans, 210.0, 1000.0).unwrap();
        assert_eq!((index, new.from, new.to), (1, 200.0, 260.0));
        let (index, new) = span_at(&spans, 10.0, 1000.0).unwrap();
        assert_eq!((index, new.from, new.to), (0, 0.0, 60.0));
        assert_eq!(span_at(&[], 10.0, 1000.0).unwrap().0, 0);
    }

    #[test]
    fn a_span_splits_in_two_halves() {
        let mut spans = vec![span(100.0, 200.0), span(300.0, TO_THE_END)];
        assert!(split_span(&mut spans, 1, 1000.0));
        assert_eq!(spans.len(), 3);
        assert_eq!((spans[1].from, spans[1].to), (300.0, 650.0));
        assert_eq!((spans[2].from, spans[2].to), (650.0, TO_THE_END));
        assert_eq!(spans[2].was, None);
    }

    #[test]
    fn a_span_s_ends_move_within_their_room_and_with_the_next() {
        let mut spans = vec![span(100.0, 200.0), span(200.0, 400.0), span(500.0, 600.0)];
        // Joined: the next one's start goes with it.
        move_span_end(&mut spans, 0, false, 250.0, 1000.0);
        assert_eq!((spans[0].to, spans[1].from), (250.0, 250.0));
        // Not joined: no further than the next one.
        move_span_end(&mut spans, 1, false, 900.0, 1000.0);
        assert_eq!(spans[1].to, 500.0);
        // Never shorter than the shortest.
        move_span_end(&mut spans, 2, true, 700.0, 1000.0);
        assert_eq!(spans[2].from, 600.0 - SHORTEST_SPAN);
        move_span_end(&mut spans, 0, true, -10.0, 1000.0);
        assert_eq!(spans[0].from, 0.0);
    }

    #[test]
    fn the_panel_maps_the_lap_and_its_heights_both_ways() {
        let panel = Panel::new(Vec2::new(1000.0, 800.0), 2000.0, [0.0, 40.0].into_iter());
        assert!(panel.contains(Vec2::new(500.0, 700.0)));
        assert!(!panel.contains(Vec2::new(500.0, 100.0)));
        assert!(panel.on_band(Vec2::new(500.0, panel.bottom - 1.0)));
        assert!(!panel.on_band(Vec2::new(500.0, panel.top + 1.0)));
        for (along, height) in [(0.0, 0.0), (1000.0, 20.0), (2000.0, 40.0)] {
            assert!((panel.along(panel.x(along)) - along).abs() < 1e-2);
            assert!((panel.height(panel.y(height)) - height).abs() < 1e-3);
        }
        // Higher is higher on the screen.
        assert!(panel.y(40.0) < panel.y(0.0));
        assert!(panel.y(40.0) >= panel.top && panel.y(0.0) <= panel.band);
    }
}
