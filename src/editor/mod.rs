//! The map editor: a circuit laid out in the game's own world, so that what
//! is edited is seen as it will be raced (`ROADMAP.md`, section 3, step 5).
//!
//! So far it edits the lap's control points. Started with `--edit` (or
//! `--edit <circuit>`: its file's name, as `bahia`, or its place in the
//! menu, from 0; or the environment's `MOTORUST_EDIT`), in a desktop
//! development build, it shows the circuit chosen from above
//! ([`view::View`]), its control points and the centre line through them as
//! it would be laid ([`limits::Preview`]), red where a bend is too tight or
//! a grade too steep, as the tests check ([`limits`]). A point is picked and
//! dragged with the mouse, added after the one picked or taken out; every
//! change can be undone ([`Edit`]). Saved, the points are written into the
//! circuit's file in place of its own, the rest of it as it was, its
//! comments too ([`file`]); the circuits read again from the disk
//! (`crate::circuits::reload`), the world builds the circuit again as it
//! now is.
//!
//! The keys and the mouse:
//!
//! | | |
//! |---|---|
//! | left button | pick a point, and drag it |
//! | right button, or W A S D / arrows | move over the ground |
//! | wheel, or + / - | nearer, further |
//! | Q / E, R / F | turn, tilt |
//! | Tab / Shift+Tab | the next / last point |
//! | Insert or N | a point halfway to the next |
//! | Delete or Backspace | the point out |
//! | Ctrl+Z / Ctrl+Y | undo / redo |
//! | Ctrl+S | save into the circuit's file |
//! | Page Up / Page Down | the last / next circuit (saved first) |
//! | Home | the whole circuit in view |

pub mod file;
pub mod limits;
pub mod view;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::camera::visibility::RenderLayers;
use bevy::window::PrimaryWindow;

use crate::camera::CameraPlayer;
use crate::circuits::reload::CircuitsReloaded;
use crate::circuits::{Circuits, CurrentCircuit};
use crate::hud::UiFont;
use crate::menu::GameState;
use file::EditedPoint;
use limits::Preview;
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

/// Whether the editor was asked for, and on which circuit: only in a
/// desktop development build, where circuits are read from the disk and
/// can be written there.
pub fn requested() -> Option<String> {
    if cfg!(all(debug_assertions, not(target_arch = "wasm32"))) {
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

/// The control points being edited, with what was done to them, to undo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    pub points: Vec<EditedPoint>,
    pub selected: Option<usize>,
    undo: Vec<(Vec<EditedPoint>, Option<usize>)>,
    redo: Vec<(Vec<EditedPoint>, Option<usize>)>,
    /// Whether it differs from what was read (or last saved).
    pub changed: bool,
}

/// The fewest control points a lap can have.
pub const FEWEST_POINTS: usize = 4;

impl Edit {
    /// The points of a circuit as read from its file.
    pub fn of(points: &[Vec2]) -> Self {
        Self {
            points: points
                .iter()
                .enumerate()
                .map(|(was, &at)| EditedPoint { at, was: Some(was) })
                .collect(),
            ..default()
        }
    }

    /// Where the points are.
    pub fn positions(&self) -> Vec<Vec2> {
        self.points.iter().map(|point| point.at).collect()
    }

    /// Keeps what there is, to undo what comes next.
    pub fn remember(&mut self) {
        self.undo.push((self.points.clone(), self.selected));
        self.redo.clear();
    }

    /// Point `index` moved to `to` (remembered before, once for a whole
    /// drag).
    pub fn move_point(&mut self, index: usize, to: Vec2) {
        if let Some(point) = self.points.get_mut(index) {
            point.at = to;
            self.changed = true;
        }
    }

    /// A point halfway from the one picked to the next, picked.
    pub fn insert_after_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        self.remember();
        let at = view::between(&self.positions(), index);
        self.points.insert(index + 1, EditedPoint { at, was: None });
        self.selected = Some(index + 1);
        self.changed = true;
    }

    /// The point picked taken out (never under [`FEWEST_POINTS`]); the one
    /// before it picked.
    pub fn remove_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        if self.points.len() <= FEWEST_POINTS {
            return;
        }
        self.remember();
        self.points.remove(index);
        self.selected = Some((index + self.points.len() - 1) % self.points.len());
        self.changed = true;
    }

    /// The next point picked (`step` 1) or the last (-1).
    pub fn select_next(&mut self, step: isize) {
        let n = self.points.len() as isize;
        if n == 0 {
            return;
        }
        let from = self
            .selected
            .map_or(if step > 0 { -1 } else { 0 }, |i| i as isize);
        self.selected = Some((from + step).rem_euclid(n) as usize);
    }

    pub fn undo(&mut self) {
        if let Some((points, selected)) = self.undo.pop() {
            self.redo
                .push((std::mem::replace(&mut self.points, points), self.selected));
            self.selected = selected;
            self.changed = true;
        }
    }

    pub fn redo(&mut self) {
        if let Some((points, selected)) = self.redo.pop() {
            self.undo
                .push((std::mem::replace(&mut self.points, points), self.selected));
            self.selected = selected;
            self.changed = true;
        }
    }
}

/// The editor at work: the circuit it edits, its points, the lap through
/// them, and what it has to say.
#[derive(Resource, Default)]
pub struct Editor {
    pub circuit: usize,
    pub edit: Edit,
    pub preview: Preview,
    /// The road's height at each point (m), as the lap through them has it.
    pub heights: Vec<f32>,
    pub wall: f32,
    /// A point being dragged.
    pub dragging: Option<usize>,
    pub message: String,
}

impl Editor {
    /// The lap through the points as they are now.
    fn lay_out(&mut self, circuits: &Circuits) {
        let layout = &circuits.at(self.circuit).layout;
        self.wall = layout.wall;
        let points = self.edit.positions();
        self.preview = Preview::new(&points, layout.elevation, layout.wall);
        self.heights = points
            .iter()
            .map(|&point| {
                self.preview
                    .line
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1.distance(point).total_cmp(&b.1.distance(point)))
                    .map_or(crate::track::BASE_ELEVATION, |(i, _)| {
                        self.preview.heights[i]
                    })
            })
            .collect();
    }

    /// The circuit `index` read afresh, all of it in view.
    fn open(&mut self, index: usize, circuits: &Circuits, view: &mut View) {
        self.circuit = index;
        self.edit = Edit::of(circuits.at(index).layout.control_points);
        self.dragging = None;
        self.lay_out(circuits);
        *view = View::over(&self.edit.positions(), crate::track::BASE_ELEVATION);
    }
}

/// The editor's plugin: does nothing unless it was asked for (see
/// [`requested`]).
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        let Some(asked) = requested() else {
            return;
        };
        app.insert_resource(Asked(asked))
            .init_resource::<Editor>()
            .insert_resource(View::over(&[], 0.0))
            .add_systems(Startup, start)
            .add_systems(OnEnter(GameState::Editor), (enter, spawn_text))
            .add_systems(
                Update,
                (
                    follow_reloads,
                    steer_view,
                    pick_and_drag,
                    edit_keys,
                    place_camera,
                    draw,
                    show_text,
                )
                    .chain()
                    .run_if(in_state(GameState::Editor)),
            );
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

/// The circuit chosen opened, and the camera's fog pushed far off, to see
/// all of it from above.
fn enter(
    current: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    mut editor: ResMut<Editor>,
    mut view: ResMut<View>,
    mut fog: Query<&mut DistanceFog, With<CameraPlayer>>,
    mut gizmos: ResMut<GizmoConfigStore>,
) {
    editor.open(current.0, &circuits, &mut view);
    for mut fog in &mut fog {
        fog.falloff = FogFalloff::Linear {
            start: 4000.0,
            end: 6000.0,
        };
    }
    // Drawn by the world's camera only, out of a race always on the layer
    // seen from above (`crate::world::realm`); the interface's would draw
    // them too, flattened. Over what is there.
    let (config, _) = gizmos.config_mut::<DefaultGizmoConfigGroup>();
    config.render_layers = RenderLayers::layer(GIZMO_LAYER);
    config.depth_bias = -1.0;
    config.line.width = 3.0;
}

/// The layer the editor's lines and points are drawn on, which only the
/// world's camera sees.
const GIZMO_LAYER: usize = crate::world::realm::OVERVIEW_LAYER;

/// The circuits read again (saved by the editor, or by hand): the one
/// edited opened afresh, unless it has changes of its own not saved.
fn follow_reloads(
    mut reloads: MessageReader<CircuitsReloaded>,
    circuits: Res<Circuits>,
    mut editor: ResMut<Editor>,
    mut view: ResMut<View>,
) {
    for reloaded in reloads.read() {
        if reloaded.0.contains(&editor.circuit) && editor.circuit < circuits.len() {
            info!("editor: circuit read again, changed: {}", editor.edit.changed);
            if editor.edit.changed {
                editor.message = "The file changed on the disk; not read (unsaved changes)".into();
            } else {
                let keep = *view;
                let circuit = editor.circuit;
                editor.open(circuit, &circuits, &mut view);
                *view = keep;
                editor.message = "Read again from the disk".into();
            }
        }
    }
}

/// Moving over the ground, nearer and further, turning and tilting.
fn steer_view(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut view: ResMut<View>,
) {
    let dt = time.delta_secs();
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let pressed = |codes: [KeyCode; 2]| keys.any_pressed(codes) as i32 as f32;
    let mut pan = Vec2::new(
        pressed([KeyCode::KeyD, KeyCode::ArrowRight])
            - pressed([KeyCode::KeyA, KeyCode::ArrowLeft]),
        pressed([KeyCode::KeyW, KeyCode::ArrowUp]) - pressed([KeyCode::KeyS, KeyCode::ArrowDown]),
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
    let zoom = lines
        + (pressed([KeyCode::Equal, KeyCode::NumpadAdd])
            - pressed([KeyCode::Minus, KeyCode::NumpadSubtract]))
            * 6.0
            * dt;
    let turn =
        (pressed([KeyCode::KeyQ, KeyCode::KeyQ]) - pressed([KeyCode::KeyE, KeyCode::KeyE])) * dt;
    let tilt =
        (pressed([KeyCode::KeyR, KeyCode::KeyR]) - pressed([KeyCode::KeyF, KeyCode::KeyF])) * dt;
    *view = next.zoomed(zoom).turned(turn * 1.5, tilt);
}

/// The camera where the view says.
fn place_camera(view: Res<View>, mut camera: Query<&mut Transform, With<CameraPlayer>>) {
    for mut transform in &mut camera {
        *transform = view.transform();
    }
}

/// How near the cursor (px) a point is picked.
const PICK_REACH: f32 = 16.0;

/// Picking a point with the left button, and dragging it over the level
/// it is at.
fn pick_and_drag(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<CameraPlayer>>,
    circuits: Res<Circuits>,
    mut editor: ResMut<Editor>,
) {
    let (Ok(window), Some((camera, camera_at))) = (window.single(), camera.iter().next()) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if buttons.just_released(MouseButton::Left) {
        editor.dragging = None;
    }
    if buttons.just_pressed(MouseButton::Left) {
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
        let picked = view::nearest_on_screen(&on_screen, cursor, PICK_REACH);
        editor.edit.selected = picked;
        if let Some(index) = picked {
            editor.edit.remember();
            editor.dragging = Some(index);
        }
    }
    if let Some(index) = editor.dragging
        && buttons.pressed(MouseButton::Left)
        && let Ok(ray) = camera.viewport_to_world(camera_at, cursor)
    {
        let height = editor.heights.get(index).copied().unwrap_or_default();
        if let Some(to) = view::on_level(ray.origin, *ray.direction, height)
            && editor.edit.points[index].at.distance(to) > 0.01
        {
            editor.edit.move_point(index, to);
            editor.lay_out(&circuits);
        }
    }
}

/// The keys that change the points, save them, or go to another circuit.
#[allow(clippy::too_many_arguments)]
fn edit_keys(
    keys: Res<ButtonInput<KeyCode>>,
    circuits: Res<Circuits>,
    mut current: ResMut<CurrentCircuit>,
    mut editor: ResMut<Editor>,
    mut view: ResMut<View>,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let before = editor.edit.points.clone();
    if keys.just_pressed(KeyCode::Tab) {
        editor.edit.select_next(if shift { -1 } else { 1 });
    }
    if keys.any_just_pressed([KeyCode::Insert, KeyCode::KeyN]) && !ctrl {
        editor.edit.insert_after_selected();
    }
    if keys.any_just_pressed([KeyCode::Delete, KeyCode::Backspace]) {
        editor.edit.remove_selected();
    }
    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        if shift {
            editor.edit.redo();
        } else {
            editor.edit.undo();
        }
    }
    if ctrl && keys.just_pressed(KeyCode::KeyY) {
        editor.edit.redo();
    }
    if keys.just_pressed(KeyCode::Home) {
        let positions = editor.edit.positions();
        *view = View::over(&positions, view.height);
    }
    if ctrl && keys.just_pressed(KeyCode::KeyS) {
        editor.message = match save(&editor) {
            Ok(path) => {
                editor.edit.changed = false;
                info!("editor: saved into {path}");
                format!("Saved into {path}")
            }
            Err(error) => format!("Not saved: {error}"),
        };
    }
    let step =
        keys.just_pressed(KeyCode::PageDown) as isize - keys.just_pressed(KeyCode::PageUp) as isize;
    if step != 0 {
        if editor.edit.changed {
            editor.message = "Unsaved changes: save them first (Ctrl+S), or undo them".into();
        } else {
            let index =
                (editor.circuit as isize + step).rem_euclid(circuits.len() as isize) as usize;
            current.0 = index;
            editor.open(index, &circuits, &mut view);
            editor.message.clear();
        }
    }
    if editor.edit.points != before {
        editor.lay_out(&circuits);
    }
}

/// The points written into the circuit's file on the disk (see [`file`]):
/// its path in the content pack, or why not.
fn save(editor: &Editor) -> Result<String, String> {
    #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
    {
        let (path, _) = crate::circuits::files()?
            .into_iter()
            .nth(editor.circuit)
            .ok_or("the circuit is no longer in the list")?;
        let text = std::str::from_utf8(crate::content::reread(&path)?)
            .map_err(|error| error.to_string())?;
        let written = file::write_control_points(text, &editor.edit.points)?;
        crate::content::write(&path, &written)?;
        Ok(path)
    }
    #[cfg(not(all(debug_assertions, not(target_arch = "wasm32"))))]
    {
        let _ = editor;
        Err("only a desktop development build writes circuits".into())
    }
}

/// Colours of what is drawn: the lap where it keeps the limits and where it
/// breaks them, its walls, the points and the one picked, and the polygon
/// of the points.
const LAP: Color = Color::srgb(0.2, 0.9, 0.3);
const BROKEN: Color = Color::srgb(1.0, 0.15, 0.1);
const WALLS: Color = Color::srgba(1.0, 1.0, 1.0, 0.35);
const POINT: Color = Color::srgb(0.2, 0.6, 1.0);
const PICKED: Color = Color::srgb(1.0, 0.85, 0.1);
const HULL: Color = Color::srgba(0.2, 0.6, 1.0, 0.4);
/// How far over the road the lap is drawn (m), clear of its surface.
const OVER: f32 = 1.0;

/// The lap through the points, its walls and the points, drawn over the
/// world.
fn draw(editor: Res<Editor>, view: Res<View>, mut gizmos: Gizmos) {
    let preview = &editor.preview;
    let n = preview.line.len();
    if n < 2 {
        return;
    }
    let at = |i: usize, side: f32| {
        let (prev, next) = (preview.line[(i + n - 1) % n], preview.line[(i + 1) % n]);
        let across = (next - prev).normalize_or_zero().perp();
        let p = preview.line[i] + across * side;
        Vec3::new(p.x, preview.heights[i] + OVER, p.y)
    };
    for i in 0..n {
        let j = (i + 1) % n;
        let broken = preview.tight[i] || preview.steep[i];
        gizmos.line(at(i, 0.0), at(j, 0.0), if broken { BROKEN } else { LAP });
    }
    // Where it breaks the limits, marked across the road, to be seen from
    // afar.
    for i in (0..n).step_by(3) {
        if preview.tight[i] || preview.steep[i] {
            gizmos.line(at(i, -editor.wall), at(i, editor.wall), BROKEN);
        }
    }
    for side in [-editor.wall, editor.wall] {
        gizmos.linestrip((0..=n).map(|i| at(i % n, side)), WALLS);
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
        let picked = editor.edit.selected == Some(k);
        let color = if picked { PICKED } else { POINT };
        gizmos.sphere(
            Isometry3d::from_translation(place(k)),
            size * if picked { 1.5 } else { 1.0 },
            color,
        );
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

/// What the editor shows: the circuit, its lap, the limits it breaks, the
/// point picked, the keys and what it last said.
fn show_text(
    editor: Res<Editor>,
    circuits: Res<Circuits>,
    mut text: Query<&mut Text, With<EditorText>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let circuit = circuits.at(editor.circuit.min(circuits.len() - 1));
    let preview = &editor.preview;
    let tight = preview.tight.iter().filter(|&&t| t).count();
    let steep = preview.steep.iter().filter(|&&s| s).count();
    let limits = if preview.keeps_the_limits() {
        "within the limits".to_string()
    } else {
        format!(
            "{:.0} m too tight, {:.0} m too steep (red)",
            tight as f32 * preview.step,
            steep as f32 * preview.step
        )
    };
    let picked = editor.edit.selected.map_or("none".to_string(), |k| {
        let point = editor.edit.points[k];
        format!(
            "{k} at ({:.1}, {:.1}){}",
            point.at.x,
            point.at.y,
            if point.was.is_none() { ", new" } else { "" }
        )
    });
    **text = format!(
        "EDITOR: {}{}\n\
         Lap {:.0} m, {} points; tightest bend {:.1} m (at least {:.1}), steepest grade {:.1} % (under {:.0})\n\
         {limits}\n\
         Point: {picked}\n\
         Mouse: left picks and drags a point, right moves, wheel zooms. Keys: WASD move, Q/E turn, R/F tilt, \
         Tab next point, N add, Del remove, Ctrl+Z/Y undo/redo, Ctrl+S save, PgUp/PgDn circuit, Home all\n\
         {}",
        circuit.name,
        if editor.edit.changed {
            " (unsaved)"
        } else {
            ""
        },
        preview.length(),
        editor.edit.points.len(),
        preview.tightest,
        limits::min_radius(editor.wall),
        preview.steepest * 100.0,
        limits::MAX_GRADE * 100.0,
        editor.message,
    );
}

#[cfg(test)]
mod tests {
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
        Edit::of(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 0.0),
            Vec2::new(100.0, 100.0),
            Vec2::new(0.0, 100.0),
        ])
    }

    #[test]
    fn points_are_added_after_the_one_picked_and_taken_out() {
        let mut edit = square();
        edit.insert_after_selected();
        assert_eq!(edit.points.len(), 4, "nothing picked, nothing added");
        edit.selected = Some(3);
        edit.insert_after_selected();
        assert_eq!(edit.points.len(), 5);
        assert_eq!(
            edit.points[4],
            EditedPoint {
                at: Vec2::new(0.0, 50.0),
                was: None
            }
        );
        assert_eq!(edit.selected, Some(4));
        assert!(edit.changed);
        edit.selected = Some(0);
        edit.remove_selected();
        assert_eq!(edit.points.len(), 4);
        assert_eq!(edit.points[0].was, Some(1));
        assert_eq!(edit.selected, Some(3));
        // Never fewer than four.
        edit.remove_selected();
        assert_eq!(edit.points.len(), 4);
    }

    #[test]
    fn every_change_is_undone_and_done_again() {
        let mut edit = square();
        let first = edit.points.clone();
        edit.remember();
        edit.move_point(1, Vec2::new(120.0, 10.0));
        let moved = edit.points.clone();
        edit.selected = Some(1);
        edit.insert_after_selected();
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
    fn tab_goes_round_the_points() {
        let mut edit = square();
        edit.select_next(1);
        assert_eq!(edit.selected, Some(0));
        edit.select_next(-1);
        assert_eq!(edit.selected, Some(3));
        edit.select_next(1);
        assert_eq!(edit.selected, Some(0));
    }
}
