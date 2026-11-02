//! The map editor: a circuit laid out in the game's own world, so that what
//! is edited is seen as it will be raced (`ROADMAP.md`, section 3, step 5).
//!
//! It edits the lap's control points, its height profile and its banking.
//! In a desktop development build it is entered from the tracks menu (E,
//! on the circuit chosen), or at start-up with `--edit` (or `--edit
//! <circuit>`: its file's name, as `bahia`, or its place in the menu, from
//! 0; or the environment's `MOTORUST_EDIT`); Esc goes back to the tracks.
//! It shows the circuit from above ([`view::View`]): its control points and
//! the lap through them as the game would lay it, its walls where they
//! stand, banked ([`limits::Preview`]), red where a bend is too tight or a
//! grade too steep and orange where the banking is felt as a bump off the
//! middle of the road, as the tests check ([`limits`]). Along the bottom, a
//! panel shows the lap's height from end to end with the marks it goes
//! through, and under it the stretches of banking ([`profile`]). A point, a
//! mark or a stretch's end is picked and dragged with the mouse; points and
//! marks are added after the one picked or taken out, stretches added,
//! split, taken out, and their factor, raising and grip changed; every
//! change can be undone ([`Edit`]). Saved, they are written into the
//! circuit's file in place of its own, the rest of it as it was, its
//! comments too ([`file`]); the circuits read again from the disk
//! (`crate::circuits::reload`), the world builds the circuit again as it
//! now is.
//!
//! The keys and the mouse:
//!
//! | | |
//! |---|---|
//! | left button | pick a point (on the map), a mark or a stretch (on the panel), and drag it |
//! | right button, or W A S D / arrows | move over the ground |
//! | wheel, or + / - | nearer, further |
//! | Q / E, R / F | turn, tilt |
//! | Tab / Shift+Tab | the next / last of what is picked |
//! | Insert or N | a point or mark halfway to the next; a stretch split in two |
//! | B | a stretch of banking where the cursor is on the panel's band |
//! | Delete or Backspace | what is picked, out |
//! | [ / ] | the stretch's factor down / up |
//! | T | the stretch raised on supports, or not |
//! | G / Shift+G | the stretch's grip up / down |
//! | Ctrl+Z / Ctrl+Y | undo / redo |
//! | Ctrl+S | save into the circuit's file |
//! | Page Up / Page Down | the last / next circuit (saved first) |
//! | Home | the whole circuit in view |
//! | Esc | back to the tracks (twice, with changes not saved) |

pub mod file;
pub mod limits;
pub mod profile;
pub mod terrain;
pub mod view;

use bevy::camera::visibility::RenderLayers;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::CameraPlayer;
use crate::circuits::reload::CircuitsReloaded;
use crate::circuits::{Circuits, CurrentCircuit};
use crate::hud::UiFont;
use crate::menu::GameState;
use crate::track::{BASE_ELEVATION, Layout, Track};
use file::EditedPoint;
use limits::Preview;
use profile::{Mark, Panel, Span};
use terrain::Shaping;
use view::View;

/// The flag that starts the editor, and the environment's name for it.
const FLAG: &str = "--edit";
const VAR: &str = "MOTORUST_EDIT";

/// What was asked for: `--edit`, `--edit <circuit>` or `--edit=<circuit>`
/// among `args`, or else `var`. `Some("")` for no circuit in particular.
fn asked(args: impl Iterator<Item = String>, var: Option<String>) -> Option<String> {
    let mut args = args.skip_while(|arg| !arg.starts_with(FLAG));
    match args.next() {
        Some(arg) if arg == FLAG => Some(
            args.next()
                .filter(|next| !next.starts_with("--"))
                .unwrap_or_default(),
        ),
        Some(arg) => arg
            .strip_prefix(FLAG)
            .and_then(|rest| rest.strip_prefix('='))
            .map(str::to_owned)
            .or(var),
        None => var,
    }
}

/// Whether the editor is there at all: in a desktop development build
/// (`make up`), where circuits are read from the disk and can be written
/// there.
pub const fn available() -> bool {
    cfg!(all(debug_assertions, not(target_arch = "wasm32")))
}

/// Whether the editor was asked for at start-up, and on which circuit.
pub fn requested() -> Option<String> {
    if available() {
        asked(std::env::args(), std::env::var(VAR).ok())
    } else {
        None
    }
}

/// The circuit `asked` names among the files `names` (in the menu's
/// order): by its file's name, with or without `.ron`, or by its place.
fn circuit_named(asked: &str, names: &[&str]) -> Option<usize> {
    let asked = asked.trim();
    if asked.is_empty() {
        return None;
    }
    asked
        .parse::<usize>()
        .ok()
        .filter(|&i| i < names.len())
        .or_else(|| {
            names.iter().position(|name| {
                let file = name.rsplit('/').next().unwrap_or(name);
                file == asked || file.strip_suffix(".ron") == Some(asked)
            })
        })
}

/// What is picked: a control point, a mark of the profile, a stretch of
/// banking, or nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Selection {
    #[default]
    None,
    Point(usize),
    Mark(usize),
    Span(usize),
}

/// Everything edited, as it was at some moment (to undo to it).
#[derive(Clone, Debug, Default, PartialEq)]
struct Snapshot {
    points: Vec<EditedPoint>,
    marks: Vec<Mark>,
    spans: Vec<Span>,
    selected: Selection,
}

/// The control points, the profile's marks and the stretches of banking
/// being edited, with what was done to them, to undo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    pub points: Vec<EditedPoint>,
    pub marks: Vec<Mark>,
    pub spans: Vec<Span>,
    pub selected: Selection,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Whether it differs from what was read (or last saved).
    pub changed: bool,
}

/// The fewest control points a lap can have.
pub const FEWEST_POINTS: usize = 4;

impl Edit {
    /// A circuit's lap as read from its file.
    pub fn of(layout: &Layout) -> Self {
        Self {
            points: layout
                .control_points
                .iter()
                .enumerate()
                .map(|(was, &at)| EditedPoint { at, was: Some(was) })
                .collect(),
            marks: Mark::read(layout.elevation),
            spans: Span::read(layout.banking),
            ..default()
        }
    }

    /// Where the points are.
    pub fn positions(&self) -> Vec<Vec2> {
        self.points.iter().map(|point| point.at).collect()
    }

    /// The profile as the game takes it.
    pub fn elevation(&self) -> Vec<(f32, f32)> {
        self.marks
            .iter()
            .map(|mark| (mark.along, mark.height))
            .collect()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            points: self.points.clone(),
            marks: self.marks.clone(),
            spans: self.spans.clone(),
            selected: self.selected,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) -> Snapshot {
        let now = self.snapshot();
        self.points = snapshot.points;
        self.marks = snapshot.marks;
        self.spans = snapshot.spans;
        self.selected = snapshot.selected;
        self.changed = true;
        now
    }

    /// Keeps what there is, to undo what comes next.
    pub fn remember(&mut self) {
        self.undo.push(self.snapshot());
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some(snapshot) = self.undo.pop() {
            let now = self.restore(snapshot);
            self.redo.push(now);
        }
    }

    pub fn redo(&mut self) {
        if let Some(snapshot) = self.redo.pop() {
            let now = self.restore(snapshot);
            self.undo.push(now);
        }
    }

    /// Point `index` moved to `to` (remembered before, once for a whole
    /// drag).
    pub fn move_point(&mut self, index: usize, to: Vec2) {
        if let Some(point) = self.points.get_mut(index) {
            point.at = to;
            self.changed = true;
        }
    }

    /// Mark `index` moved (see [`profile::move_mark`]).
    pub fn move_mark(&mut self, index: usize, along: f32, height: f32, lap: f32) {
        profile::move_mark(&mut self.marks, index, along, height, lap);
        self.changed = true;
    }

    /// An end of stretch `index` moved (see [`profile::move_span_end`]).
    pub fn move_span_end(&mut self, index: usize, start: bool, along: f32, lap: f32) {
        profile::move_span_end(&mut self.spans, index, start, along, lap);
        self.changed = true;
    }

    /// After what is picked, another: a point or a mark halfway to the
    /// next, picked; a stretch split in two, the second half picked.
    pub fn insert_after_selected(&mut self, lap: f32) {
        match self.selected {
            Selection::Point(index) if index < self.points.len() => {
                self.remember();
                let at = view::between(&self.positions(), index);
                self.points.insert(index + 1, EditedPoint { at, was: None });
                self.selected = Selection::Point(index + 1);
                self.changed = true;
            }
            Selection::Mark(index) => {
                if let Some((at, mark)) = profile::mark_after(&self.marks, index, lap) {
                    self.remember();
                    self.marks.insert(at, mark);
                    self.selected = Selection::Mark(at);
                    self.changed = true;
                }
            }
            Selection::Span(index) => {
                let before = self.snapshot();
                if profile::split_span(&mut self.spans, index, lap) {
                    self.undo.push(before);
                    self.redo.clear();
                    self.selected = Selection::Span(index + 1);
                    self.changed = true;
                }
            }
            _ => {}
        }
    }

    /// A stretch of banking at `along` (see [`profile::span_at`]), picked.
    pub fn add_span_at(&mut self, along: f32, lap: f32) {
        if let Some((index, span)) = profile::span_at(&self.spans, along, lap) {
            self.remember();
            self.spans.insert(index, span);
            self.selected = Selection::Span(index);
            self.changed = true;
        }
    }

    /// What is picked, taken out (never under [`FEWEST_POINTS`] points nor
    /// [`profile::FEWEST_MARKS`] marks); the one before it picked.
    pub fn remove_selected(&mut self) {
        let before = |index: usize, count: usize| (index + count - 1) % count;
        match self.selected {
            Selection::Point(index)
                if index < self.points.len() && self.points.len() > FEWEST_POINTS =>
            {
                self.remember();
                self.points.remove(index);
                self.selected = Selection::Point(before(index, self.points.len()));
            }
            Selection::Mark(index)
                if index < self.marks.len() && self.marks.len() > profile::FEWEST_MARKS =>
            {
                self.remember();
                self.marks.remove(index);
                self.selected = Selection::Mark(before(index, self.marks.len()));
            }
            Selection::Span(index) if index < self.spans.len() => {
                self.remember();
                self.spans.remove(index);
                self.selected = if self.spans.is_empty() {
                    Selection::None
                } else {
                    Selection::Span(index.min(self.spans.len() - 1))
                };
            }
            _ => return,
        }
        self.changed = true;
    }

    /// The stretch picked changed by `change`, remembered first.
    fn change_span(&mut self, change: impl FnOnce(&mut Span)) {
        if let Selection::Span(index) = self.selected
            && index < self.spans.len()
        {
            self.remember();
            change(&mut self.spans[index]);
            self.changed = true;
        }
    }

    /// The stretch's factor `steps` of [`profile::FACTOR_STEP`] more.
    pub fn change_factor(&mut self, steps: f32) {
        self.change_span(|span| {
            span.factor = (span.factor + steps * profile::FACTOR_STEP)
                .clamp(profile::FACTOR.0, profile::FACTOR.1);
            span.factor = (span.factor * 100.0).round() / 100.0;
        });
    }

    /// The stretch raised on supports, or no longer.
    pub fn toggle_raised(&mut self) {
        self.change_span(|span| span.raised = !span.raised);
    }

    /// The stretch's grip `steps` of [`profile::GRIP_STEP`] more.
    pub fn change_grip(&mut self, steps: f32) {
        self.change_span(|span| {
            span.grip =
                (span.grip + steps * profile::GRIP_STEP).clamp(profile::GRIP.0, profile::GRIP.1);
            span.grip = (span.grip * 100.0).round() / 100.0;
        });
    }

    /// The next of what is picked (`step` 1) or the last (-1); the first
    /// point with nothing picked.
    pub fn select_next(&mut self, step: isize) {
        let next = |index: Option<usize>, count: usize| {
            let n = count as isize;
            let from = index.map_or(if step > 0 { -1 } else { 0 }, |i| i as isize);
            (from + step).rem_euclid(n.max(1)) as usize
        };
        self.selected = match self.selected {
            Selection::Mark(index) if !self.marks.is_empty() => {
                Selection::Mark(next(Some(index), self.marks.len()))
            }
            Selection::Span(index) if !self.spans.is_empty() => {
                Selection::Span(next(Some(index), self.spans.len()))
            }
            Selection::Point(index) if !self.points.is_empty() => {
                Selection::Point(next(Some(index), self.points.len()))
            }
            _ if !self.points.is_empty() => Selection::Point(next(None, self.points.len())),
            other => other,
        };
    }
}

/// What the mouse is dragging.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Drag {
    #[default]
    None,
    Point(usize),
    Mark(usize),
    /// An end of a stretch: its start, or its end.
    SpanEnd(usize, bool),
}

/// How long after a change (s) the whole lap is laid out again, banking and
/// all: not at every move of a drag.
const SETTLE: f32 = 0.25;

/// The editor at work: the circuit it edits, what it edits, the lap
/// through it, and what it has to say.
#[derive(Resource, Default)]
pub struct Editor {
    pub circuit: usize,
    pub edit: Edit,
    /// The lap through the points and the profile as they are now; with
    /// its banking (`bank`, `bumps`) once it has settled.
    pub preview: Preview,
    /// The road's height at each control point (m), as the lap through them
    /// has it.
    pub heights: Vec<f32>,
    pub wall: f32,
    drag: Drag,
    /// The panel while something on it is dragged: as it was, so that it
    /// does not move under the mouse.
    held: Option<Panel>,
    /// When the whole lap is to be laid out again (s, real time).
    settle: Option<f32>,
    /// Where the cursor is along the lap, over the panel.
    hover: Option<f32>,
    /// Esc pressed once with changes not saved.
    leaving: bool,
    /// E (which turns the view) still held from entering the editor with
    /// it, from the tracks menu: not turning until it is let go.
    entered_with_e: bool,
    pub message: String,
}

impl Editor {
    /// The lap through the points and profile as they are now, at once;
    /// with its banking a moment later ([`lay_out_whole`]).
    fn lay_out(&mut self, circuits: &Circuits, now: f32) {
        let layout = &circuits.at(self.circuit).layout;
        self.wall = layout.wall;
        let points = self.edit.positions();
        let elevation = self.edit.elevation();
        self.preview = Preview::new(&points, &elevation, layout.wall);
        self.heights = self.point_heights();
        self.settle = Some(now + SETTLE);
    }

    /// Each control point's height, as the lap through them has it.
    fn point_heights(&self) -> Vec<f32> {
        self.edit
            .points
            .iter()
            .map(|point| {
                self.preview
                    .line
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1.distance(point.at).total_cmp(&b.1.distance(point.at)))
                    .map_or(BASE_ELEVATION, |(i, _)| self.preview.heights[i])
            })
            .collect()
    }

    /// The circuit `index` read afresh, all of it in view.
    fn open(&mut self, index: usize, circuits: &Circuits, view: &mut View, now: f32) {
        self.circuit = index;
        self.edit = Edit::of(&circuits.at(index).layout);
        self.drag = Drag::None;
        self.held = None;
        self.leaving = false;
        self.lay_out(circuits, now);
        *view = View::over(&self.edit.positions(), BASE_ELEVATION);
    }

    /// The lap's length (m), as it is laid out now.
    fn lap(&self) -> f32 {
        self.preview.length()
    }

    /// The panel for a window `size` px (or the one held while dragging).
    fn panel(&self, size: Vec2) -> Panel {
        self.held.unwrap_or_else(|| {
            Panel::new(
                size,
                self.lap(),
                self.preview
                    .heights
                    .iter()
                    .map(|h| h - BASE_ELEVATION)
                    .chain(self.edit.marks.iter().map(|mark| mark.height)),
            )
        })
    }
}

/// The editor's plugin: nothing outside a desktop development build.
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        if !available() {
            return;
        }
        app.init_resource::<Editor>()
            .init_resource::<Shaping>()
            .insert_resource(View::over(&[], 0.0))
            .init_gizmo_group::<PanelGizmos>()
            .add_systems(
                OnEnter(GameState::Editor),
                (
                    enter,
                    spawn_text,
                    spawn_panel_camera,
                    spawn_panel_background,
                ),
            )
            .add_systems(OnExit(GameState::Editor), leave)
            .add_systems(
                Update,
                (
                    follow_reloads,
                    terrain::shaping_keys,
                    steer_view,
                    pick_and_drag,
                    terrain::brush_the_ground,
                    edit_keys,
                    lay_out_whole,
                    place_camera,
                    draw,
                    draw_panel,
                    terrain::draw_brush,
                    show_text,
                )
                    .chain()
                    .run_if(in_state(GameState::Editor)),
            );
        if let Some(asked) = requested() {
            app.insert_resource(Asked(asked))
                .add_systems(Startup, start);
        }
    }
}

/// The circuit asked for with the flag.
#[derive(Resource)]
struct Asked(String);

/// Straight into the editor, on the circuit asked for.
fn start(
    asked: Res<Asked>,
    mut current: ResMut<CurrentCircuit>,
    mut next: ResMut<NextState<GameState>>,
) {
    if let Ok(files) = crate::circuits::files() {
        let names: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
        if let Some(index) = circuit_named(&asked.0, &names) {
            current.0 = index;
        } else if !asked.0.is_empty() {
            warn!("--edit: no circuit {:?}, editing the first", asked.0);
        }
    }
    next.set(GameState::Editor);
}

/// How far the fog starts and ends in the editor (m): far off, to see all
/// of a circuit from above; and as it was, to put back.
const EDITOR_FOG: (f32, f32) = (4000.0, 6000.0);

/// The fog as it was before the editor.
#[derive(Resource)]
struct FogBefore(FogFalloff);

/// The circuit chosen opened, the camera's fog pushed far off, and the
/// editor's lines and points drawn by the world's camera only, over what is
/// there.
#[allow(clippy::too_many_arguments)]
fn enter(
    mut commands: Commands,
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    current: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    mut editor: ResMut<Editor>,
    mut view: ResMut<View>,
    mut fog: Query<&mut DistanceFog, With<CameraPlayer>>,
    mut gizmos: ResMut<GizmoConfigStore>,
) {
    editor.open(current.0, &circuits, &mut view, time.elapsed_secs());
    editor.message.clear();
    editor.entered_with_e = keys.pressed(KeyCode::KeyE);
    for mut fog in &mut fog {
        commands.insert_resource(FogBefore(fog.falloff.clone()));
        fog.falloff = FogFalloff::Linear {
            start: EDITOR_FOG.0,
            end: EDITOR_FOG.1,
        };
    }
    // Out of a race the world's camera always sees the layer seen from
    // above (`crate::world::realm`); the interface's would draw them too,
    // flattened.
    let (config, _) = gizmos.config_mut::<DefaultGizmoConfigGroup>();
    config.render_layers = RenderLayers::layer(GIZMO_LAYER);
    config.depth_bias = -1.0;
    config.line.width = 3.0;
    let (config, _) = gizmos.config_mut::<PanelGizmos>();
    config.render_layers = RenderLayers::layer(PANEL_LAYER);
    config.line.width = 2.0;
}

/// The fog as it was, and the gizmos as any other screen has them.
fn leave(
    mut commands: Commands,
    before: Option<Res<FogBefore>>,
    mut fog: Query<&mut DistanceFog, With<CameraPlayer>>,
    mut gizmos: ResMut<GizmoConfigStore>,
) {
    if let Some(before) = before {
        for mut fog in &mut fog {
            fog.falloff = before.0.clone();
        }
        commands.remove_resource::<FogBefore>();
    }
    let (config, _) = gizmos.config_mut::<DefaultGizmoConfigGroup>();
    *config = GizmoConfig::default();
}

/// The layer the editor's lines and points are drawn on, which only the
/// world's camera sees; and the panel's, which only its own camera sees.
const GIZMO_LAYER: usize = crate::world::realm::OVERVIEW_LAYER;
const PANEL_LAYER: usize = 9;

/// The panel's lines, drawn flat on the screen by its own camera.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct PanelGizmos;

/// The panel's camera: over the world's, under the interface, seeing only
/// the panel's layer, one unit a pixel with its middle the screen's.
fn spawn_panel_camera(mut commands: Commands) {
    commands.spawn((
        DespawnOnExit(GameState::Editor),
        Camera2d,
        Camera {
            order: 50,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Msaa::Off,
        RenderLayers::layer(PANEL_LAYER),
    ));
}

/// The panel's background: dark, a little see-through, under its lines.
#[derive(Component)]
struct PanelBackground;

fn spawn_panel_background(mut commands: Commands) {
    commands.spawn((
        DespawnOnExit(GameState::Editor),
        PanelBackground,
        Sprite::from_color(Color::srgba(0.0, 0.0, 0.0, 0.6), Vec2::ONE),
        Transform::from_xyz(0.0, 0.0, -1.0),
        RenderLayers::layer(PANEL_LAYER),
    ));
}

/// The circuits read again (saved by the editor, or by hand): the one
/// edited opened afresh, unless it has changes of its own not saved.
fn follow_reloads(
    time: Res<Time<Real>>,
    mut reloads: MessageReader<CircuitsReloaded>,
    circuits: Res<Circuits>,
    mut editor: ResMut<Editor>,
    mut view: ResMut<View>,
) {
    for reloaded in reloads.read() {
        if reloaded.0.contains(&editor.circuit) && editor.circuit < circuits.len() {
            info!(
                "editor: circuit read again, changed: {}",
                editor.edit.changed
            );
            if editor.edit.changed {
                editor.message = "The file changed on the disk; not read (unsaved changes)".into();
            } else {
                let keep = *view;
                let circuit = editor.circuit;
                editor.open(circuit, &circuits, &mut view, time.elapsed_secs());
                *view = keep;
                editor.message = "Read again from the disk".into();
            }
        }
    }
}

/// Moving over the ground, nearer and further, turning and tilting.
#[allow(clippy::too_many_arguments)]
fn steer_view(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut view: ResMut<View>,
    mut editor: ResMut<Editor>,
    shaping: Res<Shaping>,
) {
    if editor.entered_with_e && !keys.pressed(KeyCode::KeyE) {
        editor.entered_with_e = false;
    }
    let dt = time.delta_secs();
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let pressed = |codes: &[KeyCode]| keys.any_pressed(codes.iter().copied()) as i32 as f32;
    let mut pan = Vec2::new(
        pressed(&[KeyCode::KeyD, KeyCode::ArrowRight])
            - pressed(&[KeyCode::KeyA, KeyCode::ArrowLeft]),
        pressed(&[KeyCode::KeyW, KeyCode::ArrowUp]) - pressed(&[KeyCode::KeyS, KeyCode::ArrowDown]),
    );
    if ctrl {
        pan = Vec2::ZERO;
    }
    let mut next = view.panned(pan * view.distance * 0.8 * dt);
    if buttons.pressed(MouseButton::Right) {
        // The ground follows the cursor, about.
        next = next.panned(Vec2::new(-motion.delta.x, motion.delta.y) * next.distance * 0.0015);
    }
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    // Shaping the ground, Shift and Ctrl with the wheel size the brush.
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let lines = if shaping.on && (ctrl || shift) {
        0.0
    } else {
        lines
    };
    let zoom = lines
        + (pressed(&[KeyCode::Equal, KeyCode::NumpadAdd])
            - pressed(&[KeyCode::Minus, KeyCode::NumpadSubtract]))
            * 6.0
            * dt;
    let e = if editor.entered_with_e {
        0.0
    } else {
        pressed(&[KeyCode::KeyE])
    };
    let turn = (pressed(&[KeyCode::KeyQ]) - e) * dt;
    let tilt = (pressed(&[KeyCode::KeyR]) - pressed(&[KeyCode::KeyF])) * dt;
    *view = next.zoomed(zoom).turned(turn * 1.5, tilt);
}

/// The camera where the view says.
fn place_camera(view: Res<View>, mut camera: Query<&mut Transform, With<CameraPlayer>>) {
    for mut transform in &mut camera {
        *transform = view.transform();
    }
}

/// How near the cursor (px) a point or a mark is picked, and a stretch's
/// end on the panel's band.
const PICK_REACH: f32 = 16.0;
const END_REACH: f32 = 7.0;

/// Picking with the left button, and dragging: a control point on the map
/// (over the level it is at), a mark or a stretch's end on the panel.
#[allow(clippy::too_many_arguments)]
fn pick_and_drag(
    time: Res<Time<Real>>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<CameraPlayer>>,
    circuits: Res<Circuits>,
    mut editor: ResMut<Editor>,
    shaping: Res<Shaping>,
) {
    if shaping.on {
        editor.hover = None;
        editor.drag = Drag::None;
        return;
    }
    let (Ok(window), Some((camera, camera_at))) = (window.single(), camera.iter().next()) else {
        return;
    };
    let now = time.elapsed_secs();
    let size = window.size();
    let Some(cursor) = window.cursor_position() else {
        editor.hover = None;
        return;
    };
    let panel = editor.panel(size);
    editor.hover = panel.contains(cursor).then(|| panel.along(cursor.x));
    if buttons.just_released(MouseButton::Left) {
        editor.drag = Drag::None;
        editor.held = None;
    }
    if buttons.just_pressed(MouseButton::Left) {
        let lap = editor.lap();
        let picked = if panel.on_band(cursor) {
            // A stretch's end near the cursor, or the stretch under it.
            let ends = editor
                .edit
                .spans
                .iter()
                .enumerate()
                .flat_map(|(index, span)| {
                    [(index, true, span.from), (index, false, span.end(lap))]
                });
            let end = ends
                .map(|(index, start, along)| (index, start, (panel.x(along) - cursor.x).abs()))
                .filter(|&(_, _, apart)| apart <= END_REACH)
                .min_by(|a, b| a.2.total_cmp(&b.2));
            match end {
                Some((index, start, _)) => (Selection::Span(index), Drag::SpanEnd(index, start)),
                None => {
                    let along = panel.along(cursor.x);
                    let under = editor.edit.spans.iter().position(|span| span.covers(along));
                    (under.map_or(Selection::None, Selection::Span), Drag::None)
                }
            }
        } else if panel.contains(cursor) {
            let on_screen: Vec<Option<Vec2>> = editor
                .edit
                .marks
                .iter()
                .map(|mark| Some(Vec2::new(panel.x(mark.along), panel.y(mark.height))))
                .collect();
            match view::nearest_on_screen(&on_screen, cursor, PICK_REACH) {
                Some(index) => (Selection::Mark(index), Drag::Mark(index)),
                None => (Selection::None, Drag::None),
            }
        } else {
            let on_screen: Vec<Option<Vec2>> = editor
                .edit
                .points
                .iter()
                .zip(&editor.heights)
                .map(|(point, &height)| {
                    camera
                        .world_to_viewport(camera_at, Vec3::new(point.at.x, height, point.at.y))
                        .ok()
                })
                .collect();
            match view::nearest_on_screen(&on_screen, cursor, PICK_REACH) {
                Some(index) => (Selection::Point(index), Drag::Point(index)),
                None => (Selection::None, Drag::None),
            }
        };
        editor.edit.selected = picked.0;
        editor.drag = picked.1;
        if picked.1 != Drag::None {
            editor.edit.remember();
            editor.held = Some(panel);
        }
    }
    if !buttons.pressed(MouseButton::Left) {
        return;
    }
    let lap = editor.lap();
    match editor.drag {
        Drag::None => {}
        Drag::Point(index) => {
            let height = editor.heights.get(index).copied().unwrap_or_default();
            if let Ok(ray) = camera.viewport_to_world(camera_at, cursor)
                && let Some(to) = view::on_level(ray.origin, *ray.direction, height)
                && editor.edit.points[index].at.distance(to) > 0.01
            {
                editor.edit.move_point(index, to);
                editor.lay_out(&circuits, now);
            }
        }
        Drag::Mark(index) => {
            let (along, height) = (panel.along(cursor.x), panel.height(cursor.y));
            let height = (height * 10.0).round() / 10.0;
            let mark = editor.edit.marks[index];
            if (mark.along - along).abs() > 0.05 || (mark.height - height).abs() > 0.05 {
                editor.edit.move_mark(index, along, height, lap);
                editor.lay_out(&circuits, now);
            }
        }
        Drag::SpanEnd(index, start) => {
            let along = panel.along(cursor.x).round();
            editor.edit.move_span_end(index, start, along, lap);
            editor.lay_out(&circuits, now);
        }
    }
}

/// The keys that change what is edited, save it, leave or go to another
/// circuit.
#[allow(clippy::too_many_arguments)]
fn edit_keys(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    circuits: Res<Circuits>,
    mut current: ResMut<CurrentCircuit>,
    mut editor: ResMut<Editor>,
    mut view: ResMut<View>,
    mut next: ResMut<NextState<GameState>>,
    mut shaping: ResMut<Shaping>,
) {
    let now = time.elapsed_secs();
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let before = editor.edit.clone();
    let lap = editor.lap();
    if keys.just_pressed(KeyCode::Tab) {
        editor.edit.select_next(if shift { -1 } else { 1 });
    }
    if keys.any_just_pressed([KeyCode::Insert, KeyCode::KeyN]) && !ctrl {
        editor.edit.insert_after_selected(lap);
    }
    if keys.just_pressed(KeyCode::KeyB)
        && let Some(along) = editor.hover
    {
        editor.edit.add_span_at(along, lap);
    }
    if keys.any_just_pressed([KeyCode::Delete, KeyCode::Backspace]) {
        editor.edit.remove_selected();
    }
    if keys.just_pressed(KeyCode::BracketLeft) && !shaping.on {
        editor.edit.change_factor(-1.0);
    }
    if keys.just_pressed(KeyCode::BracketRight) && !shaping.on {
        editor.edit.change_factor(1.0);
    }
    if keys.just_pressed(KeyCode::KeyT) {
        editor.edit.toggle_raised();
    }
    if keys.just_pressed(KeyCode::KeyG) {
        editor.edit.change_grip(if shift { -1.0 } else { 1.0 });
    }
    // In the terrain mode, its strokes; else the lap's changes.
    let undo = ctrl && keys.just_pressed(KeyCode::KeyZ) && !shift;
    let redo =
        ctrl && (keys.just_pressed(KeyCode::KeyY) || (keys.just_pressed(KeyCode::KeyZ) && shift));
    if shaping.on {
        if undo {
            shaping.undo();
        }
        if redo {
            shaping.redo();
        }
    } else {
        if undo {
            editor.edit.undo();
        }
        if redo {
            editor.edit.redo();
        }
    }
    if keys.just_pressed(KeyCode::Home) {
        let positions = editor.edit.positions();
        *view = View::over(&positions, view.height);
    }
    if ctrl && keys.just_pressed(KeyCode::KeyS) {
        editor.message = match save(&editor, &circuits, &shaping) {
            Ok(path) => {
                info!("editor: saved into {path}");
                editor.edit.changed = false;
                shaping.changed = false;
                format!("Saved into {path}")
            }
            Err(error) => format!("Not saved: {error}"),
        };
    }
    let unsaved = editor.edit.changed || shaping.changed;
    let step =
        keys.just_pressed(KeyCode::PageDown) as isize - keys.just_pressed(KeyCode::PageUp) as isize;
    if step != 0 {
        if unsaved {
            editor.message = "Unsaved changes: save them first (Ctrl+S), or undo them".into();
        } else {
            let index =
                (editor.circuit as isize + step).rem_euclid(circuits.len() as isize) as usize;
            current.0 = index;
            editor.open(index, &circuits, &mut view, now);
            editor.message.clear();
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        if unsaved && !editor.leaving {
            editor.leaving = true;
            editor.message = "Unsaved changes: Esc again to leave them, Ctrl+S to save them".into();
        } else {
            next.set(GameState::Tracks);
        }
    }
    if editor.edit.points != before.points
        || editor.edit.marks != before.marks
        || editor.edit.spans != before.spans
    {
        editor.leaving = false;
        editor.lay_out(&circuits, now);
    }
}

/// The lap laid out whole, as the game would, once the changes have
/// settled: its banking, and where it is felt as a bump.
fn lay_out_whole(time: Res<Time<Real>>, circuits: Res<Circuits>, mut editor: ResMut<Editor>) {
    let Some(at) = editor.settle else {
        return;
    };
    if time.elapsed_secs() < at || editor.drag != Drag::None {
        return;
    }
    editor.settle = None;
    let original = &circuits.at(editor.circuit).layout;
    // The game's track takes its lists for as long as it runs: these are
    // let go of only when the game ends (a few kilobytes at each settled
    // change).
    let layout = Layout {
        control_points: Vec::leak(editor.edit.positions()),
        elevation: Vec::leak(editor.edit.elevation()),
        banking: Vec::leak(editor.edit.spans.iter().map(Span::banking).collect()),
        branches: original.branches,
        narrows: original.narrows,
        wall: original.wall,
    };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Track::new(&layout))) {
        Ok(track) => {
            editor.preview = Preview::of_track(&track);
            editor.heights = editor.point_heights();
        }
        Err(_) => editor.message = "The lap could not be laid out whole".into(),
    }
}

/// The lap written into the circuit's file on the disk (see [`file`]): its
/// path in the content pack, or why not.
fn save(editor: &Editor, circuits: &Circuits, shaping: &Shaping) -> Result<String, String> {
    #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
    {
        let layout = &circuits.at(editor.circuit).layout;
        let (path, _) = crate::circuits::files()?
            .into_iter()
            .nth(editor.circuit)
            .ok_or("the circuit is no longer in the list")?;
        // The ground's shaping first, into the circuit's companion (listed
        // in its pack the first time): the circuit is read again with it.
        let mut saved = Vec::new();
        if shaping.changed
            && let Some(sculpt) = &shaping.sculpt
        {
            let companion = crate::world::sculpt::companion(&path);
            crate::content::list_beside(&path, &companion)?;
            crate::content::write_bytes(&companion, &sculpt.encode())?;
            saved.push(companion);
        }
        if editor.edit.changed || saved.is_empty() {
            let text = std::str::from_utf8(crate::content::reread(&path)?)
                .map_err(|error| error.to_string())?;
            let written = write_lap(text, &editor.edit, layout)?;
            crate::content::write(&path, &written)?;
            saved.insert(0, path);
        }
        Ok(saved.join(" and "))
    }
    #[cfg(not(all(debug_assertions, not(target_arch = "wasm32"))))]
    {
        let _ = (editor, circuits, shaping);
        Err("only a desktop development build writes circuits".into())
    }
}

/// `text`, a circuit's file read as `read`, with the lap as `edit` has it:
/// its control points, its profile and its banking (see [`file`]).
pub fn write_lap(text: &str, edit: &Edit, read: &Layout) -> Result<String, String> {
    let text = file::write_control_points(text, &edit.points, read.control_points)?;
    let read_marks = Mark::read(read.elevation);
    let marks = profile::items(
        &edit.marks,
        |mark| mark.was,
        Mark::text,
        &read_marks,
        |a, b| (a.along - b.along).abs() < 0.004 && (a.height - b.height).abs() < 0.004,
    );
    let text = file::write_list(&text, "elevation", &marks, None)?;
    let read_spans = Span::read(read.banking);
    let spans = profile::items(
        &edit.spans,
        |span| span.was,
        Span::text,
        &read_spans,
        |a, b| Span { was: None, ..*a } == Span { was: None, ..*b },
    );
    file::write_list(&text, "banking", &spans, Some("elevation"))
}

/// Colours of what is drawn: the lap where it keeps the limits, where it
/// breaks them and where its banking is felt as a bump; its walls; the
/// points and the one picked, and the polygon of the points; the profile's
/// marks; the cursor's place along the lap.
const LAP: Color = Color::srgb(0.2, 0.9, 0.3);
const BROKEN: Color = Color::srgb(1.0, 0.15, 0.1);
const BUMP: Color = Color::srgb(1.0, 0.55, 0.0);
const WALLS: Color = Color::srgba(1.0, 1.0, 1.0, 0.45);
const POINT: Color = Color::srgb(0.2, 0.6, 1.0);
const PICKED: Color = Color::srgb(1.0, 0.85, 0.1);
const HULL: Color = Color::srgba(0.2, 0.6, 1.0, 0.4);
const MARK: Color = Color::srgb(0.85, 0.4, 1.0);
const HOVER: Color = Color::WHITE;
/// How far over the road the lap is drawn (m), clear of its surface.
const OVER: f32 = 1.0;

/// The lap through the points, its walls (banked) and the points, the
/// profile's marks where they are along it, drawn over the world.
fn draw(editor: Res<Editor>, view: Res<View>, mut gizmos: Gizmos) {
    let preview = &editor.preview;
    let n = preview.line.len();
    if n < 2 {
        return;
    }
    let bank = |i: usize| preview.bank.get(i).copied().unwrap_or(0.0);
    let at = |i: usize, side: f32| {
        let i = i % n;
        let (prev, next) = (preview.line[(i + n - 1) % n], preview.line[(i + 1) % n]);
        let across = (next - prev).normalize_or_zero().perp();
        let p = preview.line[i] + across * side;
        Vec3::new(p.x, preview.heights[i] - side * bank(i).tan() + OVER, p.y)
    };
    for i in 0..n {
        let broken = preview.tight[i] || preview.steep[i];
        gizmos.line(
            at(i, 0.0),
            at(i + 1, 0.0),
            if broken { BROKEN } else { LAP },
        );
    }
    // Where it breaks the limits or bumps, marked across the road, to be
    // seen from afar.
    for i in (0..n).step_by(3) {
        if preview.tight[i] || preview.steep[i] {
            gizmos.line(at(i, -editor.wall), at(i, editor.wall), BROKEN);
        } else if preview.bumps.get(i).copied().unwrap_or(false) {
            gizmos.line(at(i, -editor.wall), at(i, editor.wall), BUMP);
        }
    }
    for side in [-editor.wall, editor.wall] {
        gizmos.linestrip((0..=n).map(|i| at(i, side)), WALLS);
    }
    // The points, as big on the screen however far the camera is.
    let size = view.distance * 0.012;
    let points = &editor.edit.points;
    let place = |k: usize| {
        let p = points[k].at;
        Vec3::new(
            p.x,
            editor.heights.get(k).copied().unwrap_or_default() + OVER,
            p.y,
        )
    };
    gizmos.linestrip((0..=points.len()).map(|k| place(k % points.len())), HULL);
    for k in 0..points.len() {
        let picked = editor.edit.selected == Selection::Point(k);
        let color = if picked { PICKED } else { POINT };
        gizmos.sphere(
            Isometry3d::from_translation(place(k)),
            size * if picked { 1.5 } else { 1.0 },
            color,
        );
    }
    // The marks of the profile, and the stretch picked, along the lap.
    let index_at = |along: f32| ((along / preview.step).round() as usize) % n;
    for (k, mark) in editor.edit.marks.iter().enumerate() {
        let i = index_at(mark.along);
        let picked = editor.edit.selected == Selection::Mark(k);
        let base = at(i, 0.0);
        gizmos.line(
            base,
            base + Vec3::Y * size * 3.0,
            if picked { PICKED } else { MARK },
        );
    }
    if let Selection::Span(k) = editor.edit.selected
        && let Some(span) = editor.edit.spans.get(k)
    {
        let (from, to) = (index_at(span.from), index_at(span.end(editor.lap())));
        let count = (to + n - from) % n;
        for side in [-editor.wall - 2.0, editor.wall + 2.0] {
            gizmos.linestrip((from..=from + count).map(|i| at(i, side)), PICKED);
        }
    }
    if let Some(along) = editor.hover {
        let base = at(index_at(along), 0.0);
        gizmos.sphere(Isometry3d::from_translation(base), size * 1.2, HOVER);
    }
}

/// Colours of the panel: its frame and grid, the road's height where it
/// keeps the limits and where too steep, the stretches of banking by how
/// much they bank (none, less, more), raised or gripping harder, and the
/// bank itself.
const FRAME: Color = Color::srgba(1.0, 1.0, 1.0, 0.5);
const GRID: Color = Color::srgba(1.0, 1.0, 1.0, 0.12);
const LEVEL: Color = Color::srgba(0.5, 0.5, 0.5, 0.9);
const LESS: Color = Color::srgb(0.3, 0.6, 1.0);
const MORE: Color = Color::srgb(0.2, 0.9, 0.5);
const RAISED: Color = Color::srgb(1.0, 0.6, 0.2);
const BANK: Color = Color::srgba(1.0, 1.0, 1.0, 0.8);

/// The panel: the lap's height from end to end, the marks it goes through,
/// and under it the stretches of banking and the bank, drawn flat on the
/// screen (see [`PanelGizmos`]).
fn draw_panel(
    editor: Res<Editor>,
    shaping: Res<Shaping>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut background: Query<(&mut Transform, &mut Visibility), With<PanelBackground>>,
    mut gizmos: Gizmos<PanelGizmos>,
) {
    let Ok(window) = window.single() else {
        return;
    };
    // Not while shaping the ground.
    for (_, mut visibility) in &mut background {
        *visibility = if shaping.on {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    if shaping.on {
        return;
    }
    let size = window.size();
    let panel = editor.panel(size);
    for (mut transform, _) in &mut background {
        let middle = Vec2::new(
            (panel.left + panel.right) / 2.0 - size.x / 2.0,
            size.y / 2.0 - (panel.top + panel.bottom) / 2.0,
        );
        let extent = Vec2::new(panel.right - panel.left, panel.bottom - panel.top)
            + Vec2::splat(2.0 * profile::PANEL_MARGIN);
        *transform =
            Transform::from_translation(middle.extend(-1.0)).with_scale(extent.extend(1.0));
    }
    let preview = &editor.preview;
    let n = preview.line.len();
    if n < 2 {
        return;
    }
    // From the screen (px, top left) to the panel camera's (middle, up).
    let flat = |x: f32, y: f32| Vec2::new(x - size.x / 2.0, size.y / 2.0 - y);
    let lap = editor.lap();
    // The frame, a line every 10 m of height and every 100 m of the lap.
    let corners = [
        flat(panel.left, panel.top),
        flat(panel.right, panel.top),
        flat(panel.right, panel.bottom),
        flat(panel.left, panel.bottom),
        flat(panel.left, panel.top),
    ];
    gizmos.linestrip_2d(corners, FRAME);
    gizmos.line_2d(
        flat(panel.left, panel.band),
        flat(panel.right, panel.band),
        FRAME,
    );
    let mut height = (panel.low / 10.0).ceil() * 10.0;
    while height < panel.high {
        let y = panel.y(height);
        gizmos.line_2d(flat(panel.left, y), flat(panel.right, y), GRID);
        height += 10.0;
    }
    let mut along = 0.0;
    while along < lap {
        let x = panel.x(along);
        gizmos.line_2d(flat(x, panel.top), flat(x, panel.band), GRID);
        along += 100.0;
    }
    // The road's height.
    for i in 0..n {
        let (a, b) = (i as f32 * preview.step, (i + 1) as f32 * preview.step);
        let j = (i + 1) % n;
        let color = if preview.steep[i] { BROKEN } else { LAP };
        gizmos.line_2d(
            flat(panel.x(a), panel.y(preview.heights[i] - BASE_ELEVATION)),
            flat(
                panel.x(b.min(lap)),
                panel.y(preview.heights[j] - BASE_ELEVATION),
            ),
            color,
        );
        if preview.bumps.get(i).copied().unwrap_or(false) {
            gizmos.line_2d(
                flat(panel.x(a), panel.band),
                flat(panel.x(a), panel.band - 6.0),
                BUMP,
            );
        }
    }
    // The marks.
    for (k, mark) in editor.edit.marks.iter().enumerate() {
        let picked = editor.edit.selected == Selection::Mark(k);
        let at = flat(panel.x(mark.along), panel.y(mark.height));
        gizmos.circle_2d(
            Isometry2d::from_translation(at),
            if picked { 7.0 } else { 5.0 },
            if picked { PICKED } else { MARK },
        );
    }
    // The stretches of banking, as bars across the band: how high, by
    // their factor (the band's middle a factor of 1).
    let band_height = panel.bottom - panel.band;
    let factor_y = |factor: f32| panel.bottom - band_height * (factor / 2.0).clamp(0.0, 1.0);
    for (k, span) in editor.edit.spans.iter().enumerate() {
        let (x0, x1) = (panel.x(span.from), panel.x(span.end(lap)));
        let y = factor_y(span.factor);
        let color = if span.raised || span.grip > 1.0 {
            RAISED
        } else if span.factor == 0.0 {
            LEVEL
        } else if span.factor < 1.0 {
            LESS
        } else {
            MORE
        };
        let picked = editor.edit.selected == Selection::Span(k);
        let outline = [
            flat(x0, panel.bottom),
            flat(x0, y),
            flat(x1, y),
            flat(x1, panel.bottom),
        ];
        gizmos.linestrip_2d(outline, if picked { PICKED } else { color });
        if span.grip > 1.0 {
            gizmos.line_2d(flat(x0, y + 3.0), flat(x1, y + 3.0), RAISED);
        }
    }
    // The bank itself (once laid out whole), round the middle of the band:
    // up for a bend to the left.
    if !preview.bank.is_empty() {
        let middle = (panel.band + panel.bottom) / 2.0;
        let scale = band_height / 2.0 / 0.4;
        gizmos.linestrip_2d(
            (0..n).map(|i| {
                flat(
                    panel.x(i as f32 * preview.step),
                    middle - preview.bank[i] * scale,
                )
            }),
            BANK,
        );
    }
    // Where the cursor is along the lap.
    if let Some(along) = editor.hover {
        let x = panel.x(along);
        gizmos.line_2d(flat(x, panel.top), flat(x, panel.bottom), HOVER);
    }
}

/// The editor's text, in the top left corner.
#[derive(Component)]
struct EditorText;

fn spawn_text(mut commands: Commands, font: Res<UiFont>) {
    commands.spawn((
        EditorText,
        DespawnOnExit(GameState::Editor),
        Text::new(""),
        TextFont {
            font: font.0.clone().into(),
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::WHITE),
        TextShadow::default(),
        Node {
            position_type: PositionType::Absolute,
            left: px(16),
            top: px(16),
            ..default()
        },
    ));
}

/// What is picked, said: a point where it is, a mark how far along and how
/// high, a stretch where it runs and how it banks.
fn picked_text(edit: &Edit, lap: f32) -> String {
    match edit.selected {
        Selection::None => "nothing".into(),
        Selection::Point(k) => edit.points.get(k).map_or_else(String::new, |point| {
            format!(
                "point {k} at ({:.1}, {:.1}){}",
                point.at.x,
                point.at.y,
                if point.was.is_none() { ", new" } else { "" }
            )
        }),
        Selection::Mark(k) => edit.marks.get(k).map_or_else(String::new, |mark| {
            format!("mark {k}: {:.1} m up at {:.1} m", mark.height, mark.along)
        }),
        Selection::Span(k) => edit.spans.get(k).map_or_else(String::new, |span| {
            format!(
                "banking {k}: {:.0}–{:.0} m, factor {:.2}{}{}",
                span.from,
                span.end(lap),
                span.factor,
                if span.raised { ", raised" } else { "" },
                if span.grip > 1.0 {
                    format!(", grip {:.2}", span.grip)
                } else {
                    String::new()
                }
            )
        }),
    }
}

/// What the editor shows: the circuit, its lap, the limits it breaks, what
/// is picked, the keys and what it last said.
fn show_text(
    editor: Res<Editor>,
    shaping: Res<Shaping>,
    circuits: Res<Circuits>,
    mut text: Query<&mut Text, With<EditorText>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let circuit = circuits.at(editor.circuit.min(circuits.len() - 1));
    let preview = &editor.preview;
    let count =
        |marks: &[bool]| marks.iter().filter(|&&marked| marked).count() as f32 * preview.step;
    let limits = if preview.keeps_the_limits() {
        "within the limits".to_string()
    } else {
        format!(
            "{:.0} m too tight, {:.0} m too steep (red), {:.0} m bumpy off the middle (orange)",
            count(&preview.tight),
            count(&preview.steep),
            count(&preview.bumps)
        )
    };
    let hover = editor
        .hover
        .map_or_else(String::new, |along| format!("; cursor at {along:.0} m"));
    **text = format!(
        "EDITOR: {}{}\n\
         Lap {:.0} m, {} points, {} marks, {} stretches of banking; tightest bend {:.1} m (at least {:.1}), steepest grade {:.1} % (under {:.0})\n\
         {limits}\n\
         {}\n\
         Picked: {}{hover}\n\
         Mouse: left picks and drags (points on the map, marks and banking's ends on the panel), right moves, wheel zooms. \
         WASD move, Q/E turn, R/F tilt, Tab next, N add, Del remove, B banking at the cursor, [ ] factor, T raised, G/Shift+G grip, \
         Ctrl+Z/Y undo/redo, Ctrl+S save, PgUp/PgDn circuit, Home all, Esc back\n\
         {}",
        circuit.name,
        if editor.edit.changed || shaping.changed {
            " (unsaved)"
        } else {
            ""
        },
        preview.length(),
        editor.edit.points.len(),
        editor.edit.marks.len(),
        editor.edit.spans.len(),
        preview.tightest,
        limits::min_radius(editor.wall),
        preview.steepest * 100.0,
        limits::MAX_GRADE * 100.0,
        terrain::shaping_text(&shaping),
        picked_text(&editor.edit, editor.lap()),
        editor.message,
    );
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    fn args(said: &[&str]) -> impl Iterator<Item = String> + use<> {
        let said: Vec<String> = std::iter::once("motorust".to_owned())
            .chain(said.iter().map(|arg| (*arg).to_owned()))
            .collect();
        said.into_iter()
    }

    #[test]
    fn the_editor_is_asked_for_with_its_flag_or_the_environment() {
        assert_eq!(asked(args(&[]), None), None);
        assert_eq!(asked(args(&["--edit"]), None), Some(String::new()));
        assert_eq!(
            asked(args(&["--edit", "bahia"]), None),
            Some("bahia".into())
        );
        assert_eq!(asked(args(&["--edit=3"]), None), Some("3".into()));
        assert_eq!(
            asked(args(&["--edit", "--from", "10"]), None),
            Some(String::new())
        );
        assert_eq!(asked(args(&[]), Some("2".into())), Some("2".into()));
    }

    #[test]
    fn a_circuit_is_named_by_its_file_or_its_place() {
        let names = ["circuits/bahia.ron", "circuits/toy_store.ron"];
        assert_eq!(circuit_named("toy_store", &names), Some(1));
        assert_eq!(circuit_named("bahia.ron", &names), Some(0));
        assert_eq!(circuit_named("1", &names), Some(1));
        assert_eq!(circuit_named("5", &names), None);
        assert_eq!(circuit_named("", &names), None);
        assert_eq!(circuit_named("moon", &names), None);
    }

    fn square() -> Edit {
        Edit {
            points: [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]
                .iter()
                .enumerate()
                .map(|(was, &(x, z))| EditedPoint {
                    at: Vec2::new(x, z),
                    was: Some(was),
                })
                .collect(),
            marks: Mark::read(&[(0.0, 0.0), (200.0, 4.0)]),
            ..default()
        }
    }

    #[test]
    fn points_are_added_after_the_one_picked_and_taken_out() {
        let mut edit = square();
        edit.insert_after_selected(400.0);
        assert_eq!(edit.points.len(), 4, "nothing picked, nothing added");
        edit.selected = Selection::Point(3);
        edit.insert_after_selected(400.0);
        assert_eq!(edit.points.len(), 5);
        assert_eq!(
            edit.points[4],
            EditedPoint {
                at: Vec2::new(0.0, 50.0),
                was: None
            }
        );
        assert_eq!(edit.selected, Selection::Point(4));
        assert!(edit.changed);
        edit.selected = Selection::Point(0);
        edit.remove_selected();
        assert_eq!(edit.points.len(), 4);
        assert_eq!(edit.points[0].was, Some(1));
        assert_eq!(edit.selected, Selection::Point(3));
        // Never fewer than four.
        edit.remove_selected();
        assert_eq!(edit.points.len(), 4);
    }

    #[test]
    fn marks_are_added_and_taken_out_but_never_under_two() {
        let mut edit = square();
        edit.selected = Selection::Mark(0);
        edit.insert_after_selected(400.0);
        assert_eq!(edit.marks.len(), 3);
        assert_eq!((edit.marks[1].along, edit.marks[1].height), (100.0, 2.0));
        assert_eq!(edit.selected, Selection::Mark(1));
        edit.remove_selected();
        edit.remove_selected();
        assert_eq!(edit.marks.len(), 2);
    }

    #[test]
    fn banking_is_added_split_changed_and_taken_out() {
        let mut edit = square();
        edit.add_span_at(100.0, 400.0);
        assert_eq!(edit.spans.len(), 1);
        assert_eq!(edit.selected, Selection::Span(0));
        edit.change_factor(3.0);
        assert!((edit.spans[0].factor - 1.8).abs() < 1e-5);
        edit.toggle_raised();
        edit.change_grip(2.0);
        assert!(edit.spans[0].raised);
        assert!((edit.spans[0].grip - 1.1).abs() < 1e-5);
        edit.change_grip(-10.0);
        assert_eq!(edit.spans[0].grip, 1.0);
        edit.insert_after_selected(400.0);
        assert_eq!(edit.spans.len(), 2);
        assert_eq!(edit.spans[1].from, edit.spans[0].to);
        edit.remove_selected();
        edit.remove_selected();
        assert!(edit.spans.is_empty());
        assert_eq!(edit.selected, Selection::None);
        // All of it undone, one step at a time.
        for _ in 0..8 {
            edit.undo();
        }
        assert!(edit.spans.is_empty());
    }

    #[test]
    fn every_change_is_undone_and_done_again() {
        let mut edit = square();
        let first = edit.points.clone();
        edit.remember();
        edit.move_point(1, Vec2::new(120.0, 10.0));
        let moved = edit.points.clone();
        edit.selected = Selection::Point(1);
        edit.insert_after_selected(400.0);
        let added = edit.points.clone();
        edit.undo();
        assert_eq!(edit.points, moved);
        edit.undo();
        assert_eq!(edit.points, first);
        edit.undo();
        assert_eq!(edit.points, first, "nothing more to undo");
        edit.redo();
        edit.redo();
        assert_eq!(edit.points, added);
        // A new change forgets what was undone.
        edit.undo();
        edit.remember();
        edit.move_point(0, Vec2::ONE);
        edit.redo();
        assert_eq!(edit.points[0].at, Vec2::ONE);
    }

    #[test]
    fn tab_goes_round_what_is_picked() {
        let mut edit = square();
        edit.select_next(1);
        assert_eq!(edit.selected, Selection::Point(0));
        edit.select_next(-1);
        assert_eq!(edit.selected, Selection::Point(3));
        edit.selected = Selection::Mark(1);
        edit.select_next(1);
        assert_eq!(edit.selected, Selection::Mark(0));
    }

    /// Every circuit of `files` (each its path and its text), its lap
    /// changed everywhere the editor changes it (a point and a mark moved,
    /// one of each added, a stretch of banking added and one changed),
    /// written and read back so, the rest of it as it was.
    pub fn a_lap_edited_is_read_back_as_edited_on(files: &[(String, &'static str)]) {
        for (path, text) in files {
            let circuit: crate::circuits::Circuit =
                crate::circuits::data::parse(path, text).unwrap();
            let layout = &circuit.layout;
            let mut edit = Edit::of(layout);
            assert_eq!(write_lap(text, &edit, layout).unwrap(), *text, "{path}");
            let lap = Track::new(layout).lap_length();
            edit.remember();
            edit.move_point(0, edit.points[0].at + Vec2::new(1.0, -2.0));
            edit.selected = Selection::Point(1);
            edit.insert_after_selected(lap);
            edit.move_mark(0, edit.marks[0].along, edit.marks[0].height + 0.5, lap);
            edit.selected = Selection::Mark(0);
            edit.insert_after_selected(lap);
            if let Some(gap) = (0..100)
                .map(|k| k as f32 * lap / 100.0)
                .find(|&along| profile::span_at(&edit.spans, along, lap).is_some())
            {
                edit.add_span_at(gap, lap);
            }
            if !edit.spans.is_empty() {
                edit.selected = Selection::Span(0);
                edit.change_factor(1.0);
            }
            let out: &'static str = String::leak(write_lap(text, &edit, layout).unwrap());
            let read: crate::circuits::Circuit = crate::circuits::data::parse(path, out)
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            let read = &read.layout;
            assert_eq!(read.control_points.len(), edit.points.len(), "{path}");
            for (a, b) in read.control_points.iter().zip(&edit.points) {
                assert!(a.distance(b.at) < 0.01, "{path}: {a} {}", b.at);
            }
            assert_eq!(read.elevation.len(), edit.marks.len(), "{path}");
            for (a, b) in read.elevation.iter().zip(&edit.marks) {
                assert!(
                    (a.0 - b.along).abs() < 0.01 && (a.1 - b.height).abs() < 0.01,
                    "{path}"
                );
            }
            assert_eq!(read.banking.len(), edit.spans.len(), "{path}");
            for (a, b) in read.banking.iter().zip(&edit.spans) {
                let a = Span {
                    was: None,
                    ..Span::read(std::slice::from_ref(a))[0]
                };
                let b = Span { was: None, ..*b };
                assert!(
                    (a.from - b.from).abs() < 0.01
                        && (a.to == b.to || (a.to - b.to).abs() < 0.01)
                        && (a.factor - b.factor).abs() < 0.001
                        && a.raised == b.raised
                        && (a.grip - b.grip).abs() < 0.001,
                    "{path}: {a:?} {b:?}"
                );
            }
            assert_eq!(read.wall, layout.wall);
        }
    }

    #[test]
    fn a_lap_edited_is_read_back_as_edited() {
        let path = "circuits/bahia.ron".to_string();
        let text = crate::testing::demo_file(&path);
        a_lap_edited_is_read_back_as_edited_on(&[(path, text)]);
    }
}
