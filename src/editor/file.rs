//! Writing what the editor changed back into a circuit's file, the rest of
//! it as it was: its comments (a circuit's file is written by hand, and
//! says why it is as it is), its layout and its order.
//!
//! The editor moves, adds and removes the lap's control points. Each point
//! it kept is written over where it was in the file's `control_points`
//! list, keeping the comments round it; a point it removed is taken out,
//! with its line if nothing else is on it; a point it added goes on a line
//! of its own after the point it follows (see [`write_control_points`]).

use bevy::prelude::*;

/// A control point as the editor has it: where it is now and, if it was in
/// the file, which of the file's points it was (by its place in the list).
/// The editor never changes their order, so those it kept are in the
/// file's order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditedPoint {
    pub at: Vec2,
    pub was: Option<usize>,
}

/// `value` (m) to the centimetre, as the circuits' files write it: `12.5`,
/// `-3.25`, `0.0`.
pub fn centimetres(value: f32) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0');
    let text = if text.ends_with('.') {
        format!("{text}0")
    } else {
        text.to_string()
    };
    if text == "-0.0" { "0.0".into() } else { text }
}

/// A point as the circuits' files write it: `(x, z)`.
pub fn point_text(point: Vec2) -> String {
    format!("({}, {})", centimetres(point.x), centimetres(point.y))
}

/// Where the code of `text` is, outside its comments: for each byte,
/// whether it is in a `//` or `/* */` comment (or a string, where a comment
/// would not start).
fn in_comment(text: &str) -> Vec<bool> {
    let bytes = text.as_bytes();
    let mut comment = vec![false; bytes.len()];
    let mut i = 0;
    let mut string = false;
    while i < bytes.len() {
        if string {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            string = bytes[i] != b'"';
            i += 1;
        } else if bytes[i] == b'"' {
            string = true;
            i += 1;
        } else if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                comment[i] = true;
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            let end = text[i + 2..].find("*/").map_or(bytes.len(), |e| i + 2 + e + 2);
            comment[i..end].iter_mut().for_each(|c| *c = true);
            i = end;
        } else {
            i += 1;
        }
    }
    comment
}

/// Where the list `key: [ ... ]` is in `text`, outside comments: the
/// index of its `[` and of its `]`.
fn list(text: &str, key: &str, comment: &[bool]) -> Result<(usize, usize), String> {
    let pattern = format!("{key}:");
    let start = text
        .match_indices(&pattern)
        .map(|(at, _)| at)
        .find(|&at| {
            !comment[at]
                && text[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
        })
        .ok_or_else(|| format!("no `{key}` in the file"))?;
    let open = text[start..]
        .find('[')
        .map(|at| start + at)
        .ok_or_else(|| format!("`{key}` is not a list"))?;
    let mut depth = 0usize;
    for (at, byte) in text.bytes().enumerate().skip(open) {
        if comment[at] {
            continue;
        }
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((open, at));
                }
            }
            _ => {}
        }
    }
    Err(format!("`{key}`'s list is not closed"))
}

/// The tuples `( ... )` directly in the list between `open` and `close`,
/// outside comments: where each starts and ends (past its `)`).
fn tuples(text: &str, open: usize, close: usize, comment: &[bool]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (at, byte) in text.bytes().enumerate().take(close).skip(open + 1) {
        if comment[at] {
            continue;
        }
        match byte {
            b'(' => {
                if depth == 0 {
                    start = at;
                }
                depth += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    found.push((start, at + 1));
                }
            }
            _ => {}
        }
    }
    found
}

/// The start of the line `at` is on, and its end (its `\n`, or the end of
/// the text).
fn line_of(text: &str, at: usize) -> (usize, usize) {
    let start = text[..at].rfind('\n').map_or(0, |n| n + 1);
    let end = text[at..].find('\n').map_or(text.len(), |n| at + n);
    (start, end)
}

/// `text`, a circuit's file, with its `control_points` as `points` are:
/// each kept point written over its old self, those removed taken out,
/// those added each on a line of its own after the one before it. The
/// rest of the file (its comments too) is as it was. It fails, changing
/// nothing, where the file's list is not as the points say it was.
pub fn write_control_points(text: &str, points: &[EditedPoint]) -> Result<String, String> {
    let comment = in_comment(text);
    let (open, close) = list(text, "control_points", &comment)?;
    let spans = tuples(text, open, close, &comment);
    let kept: Vec<usize> = points.iter().filter_map(|point| point.was).collect();
    if kept.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("the points kept are out of the file's order".into());
    }
    if kept.last().is_some_and(|&last| last >= spans.len()) {
        return Err(format!(
            "the file has {} control points, fewer than the editor read",
            spans.len()
        ));
    }
    // How the file's points are laid out: the indent of the first one's
    // line (with none, the list's own and a step more).
    let indent = |at: usize| {
        let (line, _) = line_of(text, at);
        let rest = &text[line..];
        rest[..rest.len() - rest.trim_start_matches([' ', '\t']).len()].to_string()
    };
    let indent = match spans.first() {
        Some(&(start, _)) => indent(start),
        None => format!("{}    ", indent(open)),
    };
    // The changes, each a range of the text and what goes there.
    let mut changes: Vec<(usize, usize, String)> = Vec::new();
    for (index, &(start, end)) in spans.iter().enumerate() {
        match points.iter().find(|point| point.was == Some(index)) {
            Some(point) => changes.push((start, end, point_text(point.at))),
            None => {
                // Out, with its comma; and its line, if nothing is left on it.
                let mut cut = end;
                let rest = &text[end..];
                let after = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                if rest[after..].starts_with(',') {
                    cut = end + after + 1;
                }
                let (line, line_end) = line_of(text, start);
                let left = format!("{}{}", &text[line..start], &text[cut..line_end]);
                if left.trim().is_empty() {
                    changes.push((line, (line_end + 1).min(text.len()), String::new()));
                } else {
                    changes.push((start, cut, String::new()));
                }
            }
        }
    }
    // The new points, after the file's point before them (or first).
    let mut after: Option<usize> = None;
    let mut added: Vec<(Option<usize>, Vec2)> = Vec::new();
    for point in points {
        match point.was {
            Some(was) => after = Some(was),
            None => added.push((after, point.at)),
        }
    }
    for (anchor, at) in added {
        let line = format!("{indent}{},", point_text(at));
        match anchor.and_then(|was| spans.get(was)) {
            Some(&(_, end)) => {
                let (_, line_end) = line_of(text, end);
                changes.push((line_end, line_end, format!("\n{line}")));
            }
            None => {
                let (_, line_end) = line_of(text, open);
                let at = (line_end + 1).min(close);
                changes.push((at, at, format!("{line}\n")));
            }
        }
    }
    // From the last to the first, so that each range is where it was found;
    // several at one place in the order they were made.
    let mut order: Vec<usize> = (0..changes.len()).collect();
    order.sort_by_key(|&k| (std::cmp::Reverse(changes[k].0), std::cmp::Reverse(k)));
    let mut out = text.to_string();
    for k in order {
        let (start, end, ref with) = changes[k];
        out.replace_range(start..end, with);
    }
    Ok(out)
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    const FILE: &str = "(
    name: \"Test\",
    layout: (
        // The centre line's points (control_points: not this one).
        control_points: [
            (0.0, 0.0),
            // A comment about the next point.
            (10.0, -20.0), // and one after it
            (30.5, -40.25),
            (50.0, 0.0),
        ],
        elevation: [(0.0, 0.0)],
    ),
)
";

    fn kept(points: &[(f32, f32)]) -> Vec<EditedPoint> {
        points
            .iter()
            .enumerate()
            .map(|(was, &(x, z))| EditedPoint {
                at: Vec2::new(x, z),
                was: Some(was),
            })
            .collect()
    }

    #[test]
    fn numbers_are_written_to_the_centimetre() {
        assert_eq!(centimetres(12.5), "12.5");
        assert_eq!(centimetres(-3.254), "-3.25");
        assert_eq!(centimetres(7.0), "7.0");
        assert_eq!(centimetres(-0.001), "0.0");
        assert_eq!(point_text(Vec2::new(1.0, -2.5)), "(1.0, -2.5)");
    }

    #[test]
    fn unchanged_points_leave_the_file_as_it_was() {
        let points = kept(&[(0.0, 0.0), (10.0, -20.0), (30.5, -40.25), (50.0, 0.0)]);
        assert_eq!(write_control_points(FILE, &points).unwrap(), FILE);
    }

    #[test]
    fn a_moved_point_is_written_in_its_place_with_its_comments() {
        let points = kept(&[(0.0, 0.0), (12.0, -21.5), (30.5, -40.25), (50.0, 0.0)]);
        let out = write_control_points(FILE, &points).unwrap();
        assert_eq!(out, FILE.replace("(10.0, -20.0)", "(12.0, -21.5)"));
    }

    #[test]
    fn a_removed_point_goes_with_its_line() {
        let mut points = kept(&[(0.0, 0.0), (10.0, -20.0), (30.5, -40.25), (50.0, 0.0)]);
        points.remove(2);
        let out = write_control_points(FILE, &points).unwrap();
        assert_eq!(out, FILE.replace("            (30.5, -40.25),\n", ""));
        // One with a comment on its line leaves the comment.
        let mut points = kept(&[(0.0, 0.0), (10.0, -20.0), (30.5, -40.25), (50.0, 0.0)]);
        points.remove(1);
        let out = write_control_points(FILE, &points).unwrap();
        assert_eq!(out, FILE.replace("(10.0, -20.0), // and", " // and"));
    }

    #[test]
    fn an_added_point_goes_on_a_line_of_its_own_after_the_one_before_it() {
        let mut points = kept(&[(0.0, 0.0), (10.0, -20.0), (30.5, -40.25), (50.0, 0.0)]);
        points.insert(
            2,
            EditedPoint {
                at: Vec2::new(20.0, -30.0),
                was: None,
            },
        );
        points.insert(
            0,
            EditedPoint {
                at: Vec2::new(-5.0, 5.0),
                was: None,
            },
        );
        let out = write_control_points(FILE, &points).unwrap();
        let expected = FILE
            .replace(
                "// and one after it\n",
                "// and one after it\n            (20.0, -30.0),\n",
            )
            .replace(
                "control_points: [\n",
                "control_points: [\n            (-5.0, 5.0),\n",
            );
        assert_eq!(out, expected);
    }

    #[test]
    fn what_the_file_has_not_is_refused() {
        let mut points = kept(&[(0.0, 0.0), (10.0, -20.0), (30.5, -40.25), (50.0, 0.0)]);
        points[3].was = Some(7);
        assert!(write_control_points(FILE, &points).is_err());
        points.swap(1, 2);
        assert!(write_control_points(FILE, &points).is_err());
        assert!(write_control_points("(layout: ())", &[]).is_err());
    }

    /// Every circuit of `files` (each its path and its text), written with
    /// its own points, unchanged, is as it was; with every point moved a
    /// metre east, it reads back so, the rest of it as it was.
    pub fn every_circuit_s_points_write_back_as_they_were_on(files: &[(String, &'static str)]) {
        for (path, text) in files {
            let circuit: crate::circuits::Circuit =
                crate::circuits::data::parse(path, text).unwrap();
            let points = circuit.layout.control_points;
            let same: Vec<EditedPoint> = points
                .iter()
                .enumerate()
                .map(|(was, &at)| EditedPoint { at, was: Some(was) })
                .collect();
            assert_eq!(write_control_points(text, &same).unwrap(), *text, "{path}");
            let moved: Vec<EditedPoint> = same
                .iter()
                .map(|point| EditedPoint {
                    at: point.at + Vec2::X,
                    ..*point
                })
                .collect();
            let out: &'static str = String::leak(write_control_points(text, &moved).unwrap());
            let read: crate::circuits::Circuit = crate::circuits::data::parse(path, out).unwrap();
            assert_eq!(read.layout.control_points.len(), points.len(), "{path}");
            for (a, b) in read.layout.control_points.iter().zip(points) {
                assert!(a.distance(*b + Vec2::X) < 0.01, "{path}: {a} {b}");
            }
            assert_eq!(read.layout.elevation, circuit.layout.elevation, "{path}");
        }
    }

    #[test]
    fn every_circuit_s_points_write_back_as_they_were() {
        let path = "circuits/bahia.ron".to_string();
        let text = crate::testing::demo_file(&path);
        every_circuit_s_points_write_back_as_they_were_on(&[(path, text)]);
    }
}
