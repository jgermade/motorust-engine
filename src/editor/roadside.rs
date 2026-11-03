//! The editor's roadside mode: what stands by the road at a distance along
//! the lap, placed by hand ([`Kind`]: arches over the road, checkpoints
//! among them; billboards; chevron signs before a bend; brake markers;
//! grandstands). Each is drawn on the map where it stands, picked and
//! dragged along the road with the mouse (to the side of the road the
//! cursor is on), added where the cursor is, taken out, turned to the other
//! side; every change can be undone. A moment after a change the circuit is
//! built anew with them (see [`super::terrain::rebuild_live`]); saved, each
//! list is written into the circuit's file in place of its own, its items
//! as they were but for where they stand (their texts, colours and
//! comments kept), new ones as [`Kind::template`] makes them.
//!
//! | | |
//! |---|---|
//! | M | (from the terrain mode) the roadside mode |
//! | 1 to 5 | what N adds: an arch, a billboard, a chevron sign, a brake marker, a grandstand |
//! | left button | pick one, and drag it along the road |
//! | N | one added where the cursor is |
//! | X | the one picked to the other side |
//! | Delete | the one picked, out |
//! | Tab | the next one |
//! | Ctrl+Z / Ctrl+Y | undo / redo |

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::file::{self, Item, centimetres, set_field};
use super::terrain::{Shaping, ground_now, ray_to_ground};
use super::{Editor, Mode};
use crate::camera::CameraPlayer;
use crate::circuits::Circuit;
use crate::track::Track;
use crate::world::BuiltTerrains;
use crate::world::scenery::Side;

/// What stands by the road at a distance along the lap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An arch over the road (a checkpoint, if it says so).
    Gantry,
    Billboard,
    Chevron,
    BrakeMarker,
    Grandstand,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Gantry,
        Kind::Billboard,
        Kind::Chevron,
        Kind::BrakeMarker,
        Kind::Grandstand,
    ];

    /// The scenery's list of them, in a circuit's file.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Gantry => "gantries",
            Kind::Billboard => "billboards",
            Kind::Chevron => "chevrons",
            Kind::BrakeMarker => "brake_markers",
            Kind::Grandstand => "grandstands",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Gantry => "arch",
            Kind::Billboard => "billboard",
            Kind::Chevron => "chevron",
            Kind::BrakeMarker => "brake marker",
            Kind::Grandstand => "grandstand",
        }
    }

    /// Whether it stands to one side of the road (an arch spans it).
    pub fn sided(self) -> bool {
        self != Kind::Gantry
    }

    /// A new one, as a circuit's file writes it, `along` m from the line on
    /// `side`: a checkpoint's arch, a billboard of the game's name, a
    /// chevron pointing round the bend (`bend`: the side it turns to), a
    /// brake marker 100 m before it, a grandstand.
    pub fn template(self, along: f32, side: Side, bend: Side) -> String {
        let along = centimetres(along);
        let side = side_text(side);
        match self {
            Kind::Gantry => {
                format!("(along: {along}, color: (1.0, 0.76, 0.05), text: Some(\"CHECKPOINT\"))")
            }
            Kind::Billboard => format!(
                "(along: {along}, side: {side}, text: \"MOTORUST\", color: (0.1, 0.1, 0.12))"
            ),
            Kind::Chevron => {
                format!(
                    "(along: {along}, side: {side}, direction: {})",
                    side_text(bend)
                )
            }
            Kind::BrakeMarker => format!("(along: {along}, side: {side}, distance: 100)"),
            Kind::Grandstand => {
                format!("(along: {along}, side: {side}, lateral: 16.5, length: 30.0)")
            }
        }
    }
}

fn side_text(side: Side) -> &'static str {
    match side {
        Side::Left => "Left",
        Side::Right => "Right",
    }
}

/// One of them, as the editor has it: where it stands, its text as the
/// file is to have it, which of the file's it was, and whether it changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub kind: Kind,
    pub along: f32,
    pub side: Side,
    pub text: String,
    pub was: Option<usize>,
    pub changed: bool,
}

impl Placed {
    /// Moved to `along` on `side`, its text said so.
    fn place(&mut self, along: f32, side: Side) {
        self.along = along;
        self.text = set_field(&self.text, "along", &centimetres(along));
        if self.kind.sided() {
            self.side = side;
            self.text = set_field(&self.text, "side", side_text(side));
        }
        self.changed = true;
    }
}

/// What stands by the road, being edited, with what was done to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Roadside {
    pub placed: Vec<Placed>,
    pub selected: Option<usize>,
    /// What N adds.
    pub adding: Option<Kind>,
    undo: Vec<(Vec<Placed>, Option<usize>)>,
    redo: Vec<(Vec<Placed>, Option<usize>)>,
    pub changed: bool,
    /// A drag under way.
    dragging: bool,
}

impl Roadside {
    /// What `circuit` has by its road, as its file `text` writes it.
    pub fn read(circuit: &Circuit, text: &str) -> Self {
        let scenery = &circuit.scenery;
        let mut placed = Vec::new();
        let mut add = |kind: Kind, at: &mut dyn Iterator<Item = (f32, Side)>| {
            let texts = file::list_items(text, kind.key()).unwrap_or_default();
            for (was, ((along, side), text)) in at.zip(texts).enumerate() {
                placed.push(Placed {
                    kind,
                    along,
                    side,
                    text,
                    was: Some(was),
                    changed: false,
                });
            }
        };
        add(
            Kind::Gantry,
            &mut scenery.gantries.iter().map(|g| (g.along, Side::Left)),
        );
        add(
            Kind::Billboard,
            &mut scenery.billboards.iter().map(|b| (b.along, b.side)),
        );
        add(
            Kind::Chevron,
            &mut scenery.chevrons.iter().map(|c| (c.along, c.side)),
        );
        add(
            Kind::BrakeMarker,
            &mut scenery.brake_markers.iter().map(|b| (b.along, b.side)),
        );
        add(
            Kind::Grandstand,
            &mut scenery.grandstands.iter().map(|g| (g.along, g.side)),
        );
        Self {
            placed,
            adding: Some(Kind::Billboard),
            ..default()
        }
    }

    pub fn remember(&mut self) {
        self.undo.push((self.placed.clone(), self.selected));
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some((placed, selected)) = self.undo.pop() {
            self.redo
                .push((std::mem::replace(&mut self.placed, placed), self.selected));
            self.selected = selected;
            self.changed = true;
        }
    }

    pub fn redo(&mut self) {
        if let Some((placed, selected)) = self.redo.pop() {
            self.undo
                .push((std::mem::replace(&mut self.placed, placed), self.selected));
            self.selected = selected;
            self.changed = true;
        }
    }

    /// One of `kind` added `along` m from the line on `side`, picked.
    pub fn add(&mut self, kind: Kind, along: f32, side: Side, bend: Side) {
        self.remember();
        let side = if kind.sided() { side } else { Side::Left };
        self.placed.push(Placed {
            kind,
            along,
            side,
            text: kind.template(along, side, bend),
            was: None,
            changed: true,
        });
        self.selected = Some(self.placed.len() - 1);
        self.changed = true;
    }

    /// The one picked taken out.
    pub fn remove_selected(&mut self) {
        if let Some(index) = self.selected.filter(|&i| i < self.placed.len()) {
            self.remember();
            self.placed.remove(index);
            self.selected = None;
            self.changed = true;
        }
    }

    /// The one picked to the other side of the road.
    pub fn flip_selected(&mut self) {
        if let Some(index) = self.selected.filter(|&i| i < self.placed.len()) {
            let placed = &self.placed[index];
            if !placed.kind.sided() {
                return;
            }
            let side = match placed.side {
                Side::Left => Side::Right,
                Side::Right => Side::Left,
            };
            let along = placed.along;
            self.remember();
            self.placed[index].place(along, side);
            self.changed = true;
        }
    }

    /// The one picked moved (remembered before, once for a whole drag).
    pub fn move_selected(&mut self, along: f32, side: Side) {
        if let Some(index) = self.selected.filter(|&i| i < self.placed.len()) {
            self.placed[index].place(along, side);
            self.changed = true;
        }
    }

    pub fn select_next(&mut self) {
        if !self.placed.is_empty() {
            self.selected = Some(self.selected.map_or(0, |i| (i + 1) % self.placed.len()));
        }
    }

    /// `text`, a circuit's file, with its lists of what stands by the road
    /// as these are: the kept ones in the file's order, the new ones after
    /// them (see [`file::write_list`]); a list the file has not added
    /// after its kerbs' colours.
    pub fn write(&self, text: &str) -> Result<String, String> {
        let mut text = text.to_string();
        for kind in Kind::ALL {
            let mut kept: Vec<&Placed> = self
                .placed
                .iter()
                .filter(|p| p.kind == kind && p.was.is_some())
                .collect();
            kept.sort_by_key(|p| p.was);
            let new = self
                .placed
                .iter()
                .filter(|p| p.kind == kind && p.was.is_none());
            let items: Vec<Item> = kept
                .into_iter()
                .chain(new)
                .map(|p| Item {
                    text: p.text.clone(),
                    was: p.was,
                    changed: p.changed,
                })
                .collect();
            text = file::write_list(&text, kind.key(), &items, Some("curbs"))?;
        }
        Ok(text)
    }
}

/// Where one stands on the map: `along` m from the line, a little past the
/// walls on `side` (in the middle, for an arch), on the ground.
fn spot(track: &Track, along: f32, side: Side, kind: Kind) -> Vec3 {
    let index = ((along / track.step_length()).round() as usize) % track.sample_count();
    let lateral = if kind.sided() {
        side.sign() * (track.wall_at(index) + 3.0)
    } else {
        0.0
    };
    track.surface_point(index, lateral)
}

/// How far along the lap, and on which side of it, the point `at` (x, z) is
/// beside the road: on the lap's own road only.
fn along_and_side(track: &Track, at: Vec2) -> Option<(f32, Side)> {
    let nearest = track.nearest(at);
    (nearest.road == 0).then(|| {
        let side = if nearest.lateral >= 0.0 {
            Side::Left
        } else {
            Side::Right
        };
        (track.along(&nearest), side)
    })
}

/// The side the lap turns to `along` m from the line (where a chevron
/// points).
fn bend_at(track: &Track, along: f32) -> Side {
    let index = ((along / track.step_length()).round() as usize) % track.sample_count();
    let n = track.sample_count();
    let turn: f32 = (0..=20)
        .map(|k| track.curvature((index + n + k - 10) % n))
        .sum();
    if turn >= 0.0 { Side::Left } else { Side::Right }
}

/// How near the cursor (px) one is picked.
const PICK_REACH: f32 = 18.0;
/// How high over the ground its post is drawn (m).
const POST: f32 = 10.0;

/// How high the posts are drawn (and picked) from `view`: as big on the
/// screen however far the camera is.
fn post_height(view: &super::view::View) -> f32 {
    (view.distance * 0.03).max(POST)
}

/// The mouse and the keys in the roadside mode.
#[allow(clippy::too_many_arguments)]
pub fn roadside_input(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<CameraPlayer>>,
    track: Res<Track>,
    built_terrains: Res<BuiltTerrains>,
    view: Res<super::view::View>,
    mut editor: ResMut<Editor>,
    mut shaping: ResMut<Shaping>,
) {
    if editor.mode != Mode::Roadside {
        editor.roadside.dragging = false;
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let before = editor.roadside.placed.clone();
    for (key, kind) in [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
    ]
    .into_iter()
    .zip(Kind::ALL)
    {
        if keys.just_pressed(key) {
            editor.roadside.adding = Some(kind);
        }
    }
    if keys.just_pressed(KeyCode::Tab) {
        editor.roadside.select_next();
    }
    if keys.any_just_pressed([KeyCode::Delete, KeyCode::Backspace]) {
        editor.roadside.remove_selected();
    }
    if keys.just_pressed(KeyCode::KeyX) {
        editor.roadside.flip_selected();
    }
    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        if shift {
            editor.roadside.redo();
        } else {
            editor.roadside.undo();
        }
    }
    if ctrl && keys.just_pressed(KeyCode::KeyY) {
        editor.roadside.redo();
    }
    // The cursor on the ground beside the road.
    let cursor = (|| {
        let window = window.single().ok()?;
        let (camera, camera_at) = camera.iter().next()?;
        let cursor = window.cursor_position()?;
        let ray = camera.viewport_to_world(camera_at, cursor).ok()?;
        let terrain = built_terrains.0.get(&editor.circuit)?;
        let ground = ray_to_ground(ray.origin, *ray.direction, 8000.0, |p| {
            ground_now(terrain, &shaping, p)
        })?;
        Some((cursor, Vec2::new(ground.x, ground.z), camera, camera_at))
    })();
    if let Some((_, ground, _, _)) = cursor
        && keys.just_pressed(KeyCode::KeyN)
        && let (Some(kind), Some((along, side))) =
            (editor.roadside.adding, along_and_side(&track, ground))
    {
        let bend = bend_at(&track, along);
        editor.roadside.add(kind, along, side, bend);
    }
    if buttons.just_released(MouseButton::Left) {
        editor.roadside.dragging = false;
    }
    if let Some((screen, ground, camera, camera_at)) = cursor {
        if buttons.just_pressed(MouseButton::Left) {
            let on_screen: Vec<Option<Vec2>> = editor
                .roadside
                .placed
                .iter()
                .map(|p| {
                    let at = spot(&track, p.along, p.side, p.kind) + Vec3::Y * post_height(&view);
                    camera.world_to_viewport(camera_at, at).ok()
                })
                .collect();
            let picked = super::view::nearest_on_screen(&on_screen, screen, PICK_REACH);
            editor.roadside.selected = picked;
            if picked.is_some() {
                editor.roadside.remember();
                editor.roadside.dragging = true;
            }
        } else if editor.roadside.dragging
            && buttons.pressed(MouseButton::Left)
            && let Some((along, side)) = along_and_side(&track, ground)
        {
            let along = along.round();
            if let Some(index) = editor.roadside.selected
                && let Some(placed) = editor.roadside.placed.get(index)
                && (placed.along != along || (placed.kind.sided() && placed.side != side))
            {
                editor.roadside.move_selected(along, side);
            }
        }
    }
    // Changed, and not still being dragged: built anew a moment later.
    if editor.roadside.placed != before && !editor.roadside.dragging {
        shaping.request_rebuild(time.elapsed_secs());
    }
    if buttons.just_released(MouseButton::Left)
        && before == editor.roadside.placed
        && editor.roadside.changed
    {
        shaping.request_rebuild(time.elapsed_secs());
    }
}

/// Colours of what stands by the road, by its kind.
fn kind_color(kind: Kind) -> Color {
    match kind {
        Kind::Gantry => Color::srgb(1.0, 0.76, 0.05),
        Kind::Billboard => Color::srgb(0.95, 0.35, 0.1),
        Kind::Chevron => Color::srgb(1.0, 1.0, 1.0),
        Kind::BrakeMarker => Color::srgb(0.3, 0.6, 1.0),
        Kind::Grandstand => Color::srgb(0.8, 0.3, 0.9),
    }
}

/// What stands by the road, drawn where it stands: a post up from the
/// ground (an arch, a bar across the road), the one picked yellow and
/// taller.
pub fn draw_roadside(
    editor: Res<Editor>,
    track: Res<Track>,
    view: Res<super::view::View>,
    mut gizmos: Gizmos,
) {
    if editor.mode != Mode::Roadside || track.sample_count() == 0 {
        return;
    }
    // As big on the screen however far the camera is.
    let post = post_height(&view);
    let ball = view.distance * 0.008;
    for (k, placed) in editor.roadside.placed.iter().enumerate() {
        let picked = editor.roadside.selected == Some(k);
        let color = if picked {
            Color::srgb(1.0, 0.95, 0.2)
        } else {
            kind_color(placed.kind)
        };
        let base = spot(&track, placed.along, placed.side, placed.kind);
        let top = base + Vec3::Y * post * if picked { 1.6 } else { 1.0 };
        gizmos.line(base, top, color);
        gizmos.sphere(Isometry3d::from_translation(top), ball, color);
        if !placed.kind.sided() {
            let index =
                ((placed.along / track.step_length()).round() as usize) % track.sample_count();
            let wall = track.wall_at(index);
            let (left, right) = (
                track.surface_point(index, wall) + Vec3::Y * post,
                track.surface_point(index, -wall) + Vec3::Y * post,
            );
            gizmos.line(left, right, color);
        }
    }
}

/// What the roadside mode says, for the editor's text.
pub fn roadside_text(roadside: &Roadside) -> String {
    let kinds: Vec<String> = Kind::ALL
        .iter()
        .enumerate()
        .map(|(k, &kind)| {
            if Some(kind) == roadside.adding {
                format!("[{} {}]", k + 1, kind.name())
            } else {
                format!("{} {}", k + 1, kind.name())
            }
        })
        .collect();
    let picked = roadside
        .selected
        .and_then(|k| roadside.placed.get(k))
        .map_or("nothing".to_string(), |p| {
            format!(
                "{} at {:.0} m{}{}: {}",
                p.kind.name(),
                p.along,
                if p.kind.sided() {
                    format!(", {}", side_text(p.side).to_lowercase())
                } else {
                    String::new()
                },
                if p.was.is_none() { ", new" } else { "" },
                p.text.split_whitespace().collect::<Vec<_>>().join(" ")
            )
        });
    format!(
        "Mode: ROADSIDE{} (M for the lap): N adds {}; drag one along the road, X other side, Del out, Tab next\nPicked: {picked}",
        if roadside.changed { ", changed" } else { "" },
        kinds.join("  "),
    )
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    /// Every circuit of `files`, what stands by its road read, one of each
    /// moved, one of each kind added and one taken out, written and read
    /// back so, the rest of the file as it was.
    pub fn what_stands_by_the_road_is_read_back_as_placed_on(files: &[(String, &'static str)]) {
        for (path, text) in files {
            let circuit: Circuit = crate::circuits::data::parse(path, text).unwrap();
            let mut roadside = Roadside::read(&circuit, text);
            assert_eq!(roadside.write(text).unwrap(), *text, "{path}: unchanged");
            for placed in &mut roadside.placed {
                let side = match placed.side {
                    Side::Left => Side::Right,
                    Side::Right => Side::Left,
                };
                placed.place(placed.along + 5.0, side);
            }
            for (k, kind) in Kind::ALL.into_iter().enumerate() {
                roadside.add(kind, 100.0 + 10.0 * k as f32, Side::Left, Side::Right);
            }
            roadside.selected = Some(0);
            roadside.remove_selected();
            let out: &'static str = String::leak(roadside.write(text).unwrap());
            let read: Circuit = crate::circuits::data::parse(path, out)
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            let again = Roadside::read(&read, out);
            let key = |p: &Placed| {
                (
                    p.kind as u8,
                    (p.along * 100.0).round() as i64,
                    p.side == Side::Left,
                )
            };
            let mut wanted: Vec<_> = roadside.placed.iter().map(key).collect();
            let mut got: Vec<_> = again.placed.iter().map(key).collect();
            wanted.sort_unstable();
            got.sort_unstable();
            assert_eq!(got, wanted, "{path}");
            assert_eq!(
                read.layout.control_points, circuit.layout.control_points,
                "{path}"
            );
        }
    }

    #[test]
    fn what_stands_by_the_road_is_read_back_as_placed() {
        let path = "circuits/bahia.ron".to_string();
        let text = crate::testing::demo_file(&path);
        what_stands_by_the_road_is_read_back_as_placed_on(&[(path, text)]);
    }

    #[test]
    fn a_new_one_is_written_as_the_files_write_them() {
        assert_eq!(
            Kind::BrakeMarker.template(120.0, Side::Right, Side::Right),
            "(along: 120.0, side: Right, distance: 100)"
        );
        assert_eq!(
            Kind::Chevron.template(80.5, Side::Left, Side::Right),
            "(along: 80.5, side: Left, direction: Right)"
        );
        for kind in Kind::ALL {
            let text = kind.template(1.0, Side::Left, Side::Left);
            assert!(
                text.starts_with("(along: 1.0") && text.ends_with(')'),
                "{text}"
            );
        }
    }

    #[test]
    fn changes_are_undone_and_done_again() {
        let mut roadside = Roadside::default();
        roadside.add(Kind::Billboard, 10.0, Side::Left, Side::Left);
        roadside.remember();
        roadside.move_selected(20.0, Side::Right);
        assert_eq!(
            (roadside.placed[0].along, roadside.placed[0].side),
            (20.0, Side::Right)
        );
        assert!(roadside.placed[0].text.contains("along: 20.0, side: Right"));
        roadside.flip_selected();
        assert_eq!(roadside.placed[0].side, Side::Left);
        roadside.undo();
        roadside.undo();
        assert_eq!(roadside.placed[0].along, 10.0);
        roadside.undo();
        assert!(roadside.placed.is_empty());
        roadside.redo();
        assert_eq!(roadside.placed.len(), 1);
    }
}
