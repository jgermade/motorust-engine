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
            let end = text[i + 2..]
                .find("*/")
                .map_or(bytes.len(), |e| i + 2 + e + 2);
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

/// An item of a list as the editor has it: its text, which of the file's
/// items it was (by its place in the list), and whether it changed (an
/// unchanged one keeps its text in the file as it was, comments inside it
/// too).
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub text: String,
    pub was: Option<usize>,
    pub changed: bool,
}

/// `text`, a circuit's file, with its list `key` as `items` are: each kept
/// item that changed written over its old self, those removed taken out
/// (with their line if nothing else is on it), those added each on a line
/// of its own after the one before it. The rest of the file (its comments
/// too) is as it was. Where the file has no such list, it is added after
/// the list `after` (if given), on lines of its own. It fails, changing
/// nothing, where the file's list is not as the items say it was.
pub fn write_list(
    text: &str,
    key: &str,
    items: &[Item],
    after: Option<&str>,
) -> Result<String, String> {
    let comment = in_comment(text);
    let (open, close) = match list(text, key, &comment) {
        Ok(found) => found,
        Err(error) => {
            let Some(after) = after else {
                return Err(error);
            };
            if items.iter().any(|item| item.was.is_some()) {
                return Err(error);
            }
            if items.is_empty() {
                return Ok(text.to_string());
            }
            // A new list after the other, on lines of its own.
            let (open, close) = list(text, after, &comment)?;
            let (line, _) = line_of(text, open);
            let indent = leading(text, line);
            let rest = &text[close + 1..];
            let comma = rest.starts_with(',') as usize;
            let at = close + 1 + comma;
            let lines: String = items
                .iter()
                .map(|item| format!("{indent}    {},\n", item.text))
                .collect();
            let mut out = text.to_string();
            out.insert_str(
                at,
                &format!(
                    "{}\n{indent}{key}: [\n{lines}{indent}],",
                    if comma == 1 { "" } else { "," }
                ),
            );
            return Ok(out);
        }
    };
    let spans = tuples(text, open, close, &comment);
    let kept: Vec<usize> = items.iter().filter_map(|item| item.was).collect();
    if kept.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(format!("the {key} kept are out of the file's order"));
    }
    if kept.last().is_some_and(|&last| last >= spans.len()) {
        return Err(format!(
            "the file has {} {key}, fewer than the editor read",
            spans.len()
        ));
    }
    let (list_line, _) = line_of(text, open);
    let base = leading(text, list_line);
    // On one line: the whole list written again, an item a line.
    if !text[open..close].contains('\n') {
        let lines: String = items
            .iter()
            .map(|item| {
                let item_text = match item.was {
                    Some(was) if !item.changed => &text[spans[was].0..spans[was].1],
                    _ => item.text.as_str(),
                };
                format!("{base}    {item_text},\n")
            })
            .collect();
        let mut out = text.to_string();
        out.replace_range(open..=close, &format!("[\n{lines}{base}]"));
        return Ok(out);
    }
    // How the file's items are laid out: the indent of the first one's
    // line (with none, the list's own and a step more).
    let indent = match spans.first() {
        Some(&(start, _)) => leading(text, line_of(text, start).0),
        None => format!("{base}    "),
    };
    // The changes, each a range of the text and what goes there.
    let mut changes: Vec<(usize, usize, String)> = Vec::new();
    for (index, &(start, end)) in spans.iter().enumerate() {
        match items.iter().find(|item| item.was == Some(index)) {
            Some(item) if item.changed => changes.push((start, end, item.text.clone())),
            Some(_) => {}
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
    // The new items, after the file's item before them (or first).
    let mut anchor: Option<usize> = None;
    let mut added: Vec<(Option<usize>, &str)> = Vec::new();
    for item in items {
        match item.was {
            Some(was) => anchor = Some(was),
            None => added.push((anchor, &item.text)),
        }
    }
    let kept_after = |was: usize| items.iter().any(|item| item.was.is_some_and(|w| w > was));
    for (anchor, item_text) in added {
        let line = format!("{indent}{item_text},");
        match anchor.and_then(|was| spans.get(was).map(|span| (was, span))) {
            Some((was, &(_, end))) => {
                // After it, on a line of its own; with a comma after it, if
                // it had none (the last of the list).
                let rest = &text[end..];
                let after = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                if !rest[after..].starts_with(',') && !kept_after(was) {
                    changes.push((end, end, ",".into()));
                }
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

/// The spaces and tabs a line starts with, the line starting at `line`.
fn leading(text: &str, line: usize) -> String {
    let rest = &text[line..];
    rest[..rest.len() - rest.trim_start_matches([' ', '\t']).len()].to_string()
}

/// `text`, a circuit's file, with its `control_points` as `points` are
/// (see [`write_list`]); `read` the points as read from the file, to tell
/// which moved.
pub fn write_control_points(
    text: &str,
    points: &[EditedPoint],
    read: &[Vec2],
) -> Result<String, String> {
    let items: Vec<Item> = points
        .iter()
        .map(|point| Item {
            text: point_text(point.at),
            was: point.was,
            changed: point
                .was
                .is_none_or(|was| read.get(was).is_none_or(|&r| r.distance(point.at) > 0.004)),
        })
        .collect();
    write_list(text, "control_points", &items, None)
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

    const READ: [Vec2; 4] = [
        Vec2::new(0.0, 0.0),
        Vec2::new(10.0, -20.0),
        Vec2::new(30.5, -40.25),
        Vec2::new(50.0, 0.0),
    ];

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
        assert_eq!(write_control_points(FILE, &points, &READ).unwrap(), FILE);
    }

    #[test]
    fn a_moved_point_is_written_in_its_place_with_its_comments() {
        let points = kept(&[(0.0, 0.0), (12.0, -21.5), (30.5, -40.25), (50.0, 0.0)]);
        let out = write_control_points(FILE, &points, &READ).unwrap();
        assert_eq!(out, FILE.replace("(10.0, -20.0)", "(12.0, -21.5)"));
    }

    #[test]
    fn a_removed_point_goes_with_its_line() {
        let mut points = kept(&[(0.0, 0.0), (10.0, -20.0), (30.5, -40.25), (50.0, 0.0)]);
        points.remove(2);
        let out = write_control_points(FILE, &points, &READ).unwrap();
        assert_eq!(out, FILE.replace("            (30.5, -40.25),\n", ""));
        // One with a comment on its line leaves the comment.
        let mut points = kept(&[(0.0, 0.0), (10.0, -20.0), (30.5, -40.25), (50.0, 0.0)]);
        points.remove(1);
        let out = write_control_points(FILE, &points, &READ).unwrap();
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
        let out = write_control_points(FILE, &points, &READ).unwrap();
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
        assert!(write_control_points(FILE, &points, &READ).is_err());
        points.swap(1, 2);
        assert!(write_control_points(FILE, &points, &READ).is_err());
        assert!(write_control_points("(layout: ())", &[], &[]).is_err());
    }

    fn item(text: &str, was: Option<usize>, changed: bool) -> Item {
        Item {
            text: text.into(),
            was,
            changed,
        }
    }

    #[test]
    fn a_list_on_one_line_is_written_an_item_a_line() {
        let items = [
            item("(0.0, 0.0)", Some(0), false),
            item("(120.0, 4.5)", None, true),
        ];
        let out = write_list(FILE, "elevation", &items, None).unwrap();
        assert_eq!(
            out,
            FILE.replace(
                "elevation: [(0.0, 0.0)],",
                "elevation: [\n            (0.0, 0.0),\n            (120.0, 4.5),\n        ],"
            )
        );
    }

    #[test]
    fn an_item_added_after_the_last_gives_it_its_comma() {
        let text =
            "(\n    elevation: [\n        (0.0, 0.0),\n        (50.0, 2.0) // the top\n    ],\n)\n";
        let items = [
            item("(0.0, 0.0)", Some(0), false),
            item("(50.0, 2.0)", Some(1), false),
            item("(80.0, 1.0)", None, true),
        ];
        let out = write_list(text, "elevation", &items, None).unwrap();
        assert_eq!(
            out,
            "(\n    elevation: [\n        (0.0, 0.0),\n        (50.0, 2.0), // the top\n        (80.0, 1.0),\n    ],\n)\n"
        );
    }

    #[test]
    fn a_list_the_file_has_not_is_added_after_another() {
        let items = [item("(from: 10.0, to: 20.0, factor: 1.5)", None, true)];
        let out = write_list(FILE, "banking", &items, Some("elevation")).unwrap();
        assert_eq!(
            out,
            FILE.replace(
                "elevation: [(0.0, 0.0)],",
                "elevation: [(0.0, 0.0)],\n        banking: [\n            (from: 10.0, to: 20.0, factor: 1.5),\n        ],"
            )
        );
        // Nothing to add, nothing added; and none where no other is.
        assert_eq!(
            write_list(FILE, "banking", &[], Some("elevation")).unwrap(),
            FILE
        );
        assert!(write_list(FILE, "banking", &items, None).is_err());
    }

    #[test]
    fn an_unchanged_item_keeps_its_text_and_comments_inside_it() {
        let text = "(banking: [\n    (\n        from: 1.0,\n        // why\n        to: 2.0,\n        factor: 2.5,\n    ),\n    (from: 2.0, to: 3.0, factor: 0.0),\n])";
        let items = [
            item("(from: 1.0, to: 2.0, factor: 2.5)", Some(0), false),
            item("(from: 2.0, to: 4.0, factor: 0.0)", Some(1), true),
        ];
        let out = write_list(text, "banking", &items, None).unwrap();
        assert_eq!(out, text.replace("to: 3.0", "to: 4.0"));
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
            assert_eq!(
                write_control_points(text, &same, points).unwrap(),
                *text,
                "{path}"
            );
            let moved: Vec<EditedPoint> = same
                .iter()
                .map(|point| EditedPoint {
                    at: point.at + Vec2::X,
                    ..*point
                })
                .collect();
            let out: &'static str =
                String::leak(write_control_points(text, &moved, points).unwrap());
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
