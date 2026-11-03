//! The editor's terrain mode, as a theme park game's terrain editor works:
//! a round brush on the ground under the cursor, drawn over it, that
//! raises, lowers, smooths, flattens or restores the ground, or paints it
//! grass, sand or rock (or as the land makes it again), while the left
//! button is held, bigger or smaller, stronger or weaker
//! ([`crate::world::sculpt::Brush`]). The ground's meshes are shaped in place
//! as it goes, the ground beside the roads left to them; a moment after a
//! stroke the circuit is built anew with what was shaped and painted, not
//! yet saved ([`rebuild_live`]: its ground's kinds, its trees, its grass),
//! the world before it kept in sight meanwhile. Saved, the shaping goes
//! into the circuit's companion file (see [`crate::world::sculpt`]).
//!
//! | | |
//! |---|---|
//! | M | the terrain mode, or back to the lap |
//! | 1 to 5 | raise, lower, smooth, flatten, restore |
//! | 6 to 9 | paint grass, sand, rock; unpaint |
//! | left button | the brush, while held |
//! | [ / ], or Shift+wheel | the brush smaller / bigger |
//! | , / . , or Ctrl+wheel | the brush weaker / stronger |
//! | Ctrl+Z / Ctrl+Y | undo / redo a stroke |

use std::collections::HashMap;
use std::sync::Arc;

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::Editor;
use crate::camera::CameraPlayer;
use crate::circuits::Circuits;
use crate::circuits::reload::CircuitsReloaded;
use crate::track::Track;
use crate::world::sculpt::{Brush, CELL, Paint, Sculpt, Tool};
use crate::world::terrain::{FINE_MARGIN, GroundPiece, Terrain, sculpt_reach};
use crate::world::{BuiltTerrains, Superseded, WorldRoot};

/// The terrain mode at work: whether it is on, the shaping as it is now and
/// as the circuit was built with it, the brush, the strokes to undo and
/// redo, and where the cursor is on the ground.
#[derive(Resource, Default)]
pub struct Shaping {
    pub on: bool,
    /// The shaping being edited (none until the first stroke, on a circuit
    /// with none), and the one the circuit's world was built with.
    pub sculpt: Option<Sculpt>,
    pub built: Option<&'static Sculpt>,
    pub brush: Brush,
    undo: Vec<Option<Sculpt>>,
    redo: Vec<Option<Sculpt>>,
    pub changed: bool,
    /// During a stroke, the height it flattens to (where it started).
    stroke: Option<f32>,
    pub cursor: Option<Vec3>,
    /// The ground's meshes as built: their vertices' heights, and how much
    /// of the shaping each takes (from how near a road it is; worked out as
    /// it is first needed), by mesh.
    built_heights: HashMap<AssetId<Mesh>, Vec<f32>>,
    reach: HashMap<AssetId<Mesh>, Vec<f32>>,
    /// When the circuit is to be built anew with what was shaped (s, real
    /// time): a moment after a stroke.
    settle: Option<f32>,
    /// The shaping a new world is being built with, and the terrain it
    /// replaces (to tell when it has come).
    /// (Told by its address alone: it is never read through.)
    pending: Option<(Option<&'static Sculpt>, usize)>,
}

/// How long after a stroke the circuit is built anew with it (s).
const SETTLE: f32 = 0.8;

/// The most strokes kept to undo.
const UNDO_DEPTH: usize = 30;

impl Shaping {
    /// The circuit's shaping as built, nothing edited.
    pub fn open(&mut self, built: Option<&'static Sculpt>) {
        let on = self.on;
        let brush = self.brush;
        *self = Self {
            on,
            brush,
            built,
            sculpt: built.cloned(),
            ..default()
        };
    }

    /// The circuit to be built anew with what is edited a moment from
    /// `now` (s, real time).
    pub fn request_rebuild(&mut self, now: f32) {
        self.settle = Some(now + SETTLE);
    }

    fn remember(&mut self) {
        self.undo.push(self.sculpt.clone());
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> bool {
        let Some(sculpt) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.sculpt, sculpt));
        self.changed = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(sculpt) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.sculpt, sculpt));
        self.changed = true;
        true
    }

    /// How much higher the ground is at `point` now than as built.
    pub fn change_at(&self, point: Vec2) -> f32 {
        self.sculpt.as_ref().map_or(0.0, |sculpt| sculpt.at(point))
            - self.built.map_or(0.0, |built| built.at(point))
    }
}

/// The ground as it stands now: as built, and shaped since.
pub fn ground_now(terrain: &Terrain, shaping: &Shaping, point: Vec2) -> f32 {
    terrain.height(point) + shaping.change_at(point)
}

/// Where a ray from `origin` along `direction` first meets the ground
/// `height` gives (within `reach` m), if it does: marched in steps growing
/// with the distance, the last one halved down to a few centimetres.
pub fn ray_to_ground(
    origin: Vec3,
    direction: Vec3,
    reach: f32,
    height: impl Fn(Vec2) -> f32,
) -> Option<Vec3> {
    let direction = direction.normalize_or_zero();
    let above = |t: f32| {
        let p = origin + direction * t;
        p.y - height(Vec2::new(p.x, p.z))
    };
    let (mut t, mut last) = (0.0, above(0.0));
    if last < 0.0 {
        return None;
    }
    while t < reach {
        let step = (t * 0.01).clamp(0.5, 20.0);
        let next = above(t + step);
        if next < 0.0 {
            let (mut low, mut high) = (t, t + step);
            for _ in 0..24 {
                let middle = (low + high) / 2.0;
                if above(middle) < 0.0 {
                    high = middle;
                } else {
                    low = middle;
                }
            }
            return Some(origin + direction * high);
        }
        t += step;
        last = next;
    }
    let _ = last;
    None
}

/// The terrain mode on and off (M), the circuit's shaping read when it
/// opens (see [`super::Editor`]), the brush's tool, size and strength.
pub fn shaping_keys(
    keys: Res<ButtonInput<KeyCode>>,
    actions: Res<super::ui::Actions>,
    scroll: Res<AccumulatedMouseScroll>,
    circuits: Res<Circuits>,
    editor: Res<Editor>,
    mut shaping: ResMut<Shaping>,
    mut opened: Local<Option<usize>>,
) {
    // A circuit opened (or read again): its shaping as built.
    let built = circuits.at(editor.circuit).scenery.sculpt;
    let reopened =
        *opened != Some(editor.circuit) || (!shaping.changed && !same(shaping.built, built));
    if reopened {
        shaping.open(built);
        *opened = Some(editor.circuit);
    }
    shaping.on = editor.mode == super::Mode::Terrain;
    if !shaping.on {
        return;
    }
    use super::ui::Action;
    for action in &actions.asked {
        if let Action::Tool(tool) = action {
            shaping.brush.tool = *tool;
        }
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    let mut size = actions.has(Action::Bigger) as i32 as f32 - actions.has(Action::Smaller) as i32 as f32;
    let mut strength =
        actions.has(Action::Stronger) as i32 as f32 - actions.has(Action::Weaker) as i32 as f32;
    if shift && !actions.over_ui {
        size += lines;
    }
    if ctrl && !actions.over_ui {
        strength += lines;
    }
    shaping.brush = shaping.brush.resized(size).strengthened(strength);
}

/// Whether two shapings are the same one.
fn same(a: Option<&'static Sculpt>, b: Option<&'static Sculpt>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => std::ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// The ground under the cursor, and the brush at work there while the left
/// button is held (one stroke from press to release, to undo), the ground's
/// meshes shaped as it goes.
#[allow(clippy::too_many_arguments)]
pub fn brush_the_ground(
    time: Res<Time<Real>>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<CameraPlayer>>,
    editor: Res<Editor>,
    built_terrains: Res<BuiltTerrains>,
    track: Res<Track>,
    roots: Query<(&WorldRoot, &Children, Has<Superseded>)>,
    pieces: Query<&Mesh3d, With<GroundPiece>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut shaping: ResMut<Shaping>,
    actions: Res<super::ui::Actions>,
) {
    shaping.cursor = None;
    if !shaping.on || actions.help || actions.over_ui {
        if buttons.just_released(MouseButton::Left) && shaping.stroke.is_some() {
            shaping.stroke = None;
            shaping.settle = Some(time.elapsed_secs() + SETTLE);
        }
        shaping.stroke = None;
        return;
    }
    let Some(terrain) = built_terrains.0.get(&editor.circuit).map(Arc::clone) else {
        return;
    };
    let (Ok(window), Some((camera, camera_at))) = (window.single(), camera.iter().next()) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_at, cursor) else {
        return;
    };
    let cursor = ray_to_ground(ray.origin, *ray.direction, 8000.0, |p| {
        ground_now(&terrain, &shaping, p)
    });
    shaping.cursor = cursor;
    if buttons.just_released(MouseButton::Left) && shaping.stroke.is_some() {
        shaping.stroke = None;
        shaping.settle = Some(time.elapsed_secs() + SETTLE);
    }
    let Some(at) = shaping.cursor else {
        return;
    };
    let center = Vec2::new(at.x, at.z);
    if buttons.just_pressed(MouseButton::Left) {
        shaping.remember();
        shaping.stroke = Some(at.y);
        if shaping.sculpt.is_none() {
            let (min, max) = track.bounds();
            let margin = Vec2::splat(FINE_MARGIN);
            shaping.sculpt = Some(Sculpt::flat(min - margin, max + margin, CELL));
        }
    }
    let Some(level) = shaping.stroke else {
        return;
    };
    if !buttons.pressed(MouseButton::Left) {
        return;
    }
    let brush = shaping.brush;
    let built = shaping.built;
    // The ground with nothing shaped, as near as the built terrain tells.
    let unshaped = |p: Vec2| terrain.height(p) - built.map_or(0.0, |built| built.at(p));
    if let Some(sculpt) = shaping.sculpt.as_mut() {
        sculpt.apply(&brush, center, time.delta_secs(), unshaped, level);
    }
    shaping.changed = true;
    // Painting shows when the circuit is built anew; shaping, at once.
    if brush.tool.paints() {
        return;
    }
    // The ground's meshes, in place, round the brush: those in sight (the
    // world being replaced, while a new one is built).
    let Some((_, children, _)) = roots
        .iter()
        .filter(|(root, _, _)| root.0 == editor.circuit)
        .max_by_key(|(_, _, superseded)| *superseded)
    else {
        return;
    };
    let reach = brush.radius + 2.0 * CELL;
    for child in children.iter() {
        let Ok(mesh) = pieces.get(child) else {
            continue;
        };
        let id = mesh.id();
        let Some(mut mesh) = meshes.get_mut(id) else {
            continue;
        };
        shape_mesh(&mut mesh, id, center, reach, &track, &terrain, &mut shaping);
    }
}

/// The vertices of a ground's `mesh` within `reach` of `center` put where
/// the ground now is: as built, shaped since as much as each takes (none
/// beside the roads), facing as the ground does there.
fn shape_mesh(
    mesh: &mut Mesh,
    id: AssetId<Mesh>,
    center: Vec2,
    reach: f32,
    track: &Track,
    terrain: &Terrain,
    shaping: &mut Shaping,
) {
    use bevy::mesh::VertexAttributeValues::Float32x3;
    let Shaping {
        sculpt,
        built,
        built_heights,
        reach: shares,
        ..
    } = shaping;
    let change = |p: Vec2| {
        sculpt.as_ref().map_or(0.0, |sculpt| sculpt.at(p)) - built.map_or(0.0, |built| built.at(p))
    };
    let Some(Float32x3(positions)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) else {
        return;
    };
    let heights = built_heights
        .entry(id)
        .or_insert_with(|| positions.iter().map(|p| p[1]).collect());
    let shares = shares
        .entry(id)
        .or_insert_with(|| vec![f32::NAN; positions.len()]);
    if heights.len() != positions.len() || shares.len() != positions.len() {
        return;
    }
    // The vertices moved, and how much of the shaping each takes.
    let mut moved: Vec<(usize, Vec2, f32)> = Vec::new();
    for (index, position) in positions.iter_mut().enumerate() {
        let p = Vec2::new(position[0], position[2]);
        if p.distance_squared(center) > reach * reach {
            continue;
        }
        if shares[index].is_nan() {
            shares[index] = sculpt_reach(track.nearest(p).distance);
        }
        let share = shares[index];
        if share > 0.0 {
            position[1] = heights[index] + change(p) * share;
            moved.push((index, p, share));
        }
    }
    if moved.is_empty() {
        return;
    }
    let Some(Float32x3(normals)) = mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL) else {
        return;
    };
    let height = |p: Vec2, share: f32| terrain.height(p) + change(p) * share;
    const D: f32 = 1.5;
    for (index, p, share) in moved {
        if let Some(normal) = normals.get_mut(index) {
            let along_x = Vec3::new(
                2.0 * D,
                height(p + Vec2::X * D, share) - height(p - Vec2::X * D, share),
                0.0,
            );
            let along_z = Vec3::new(
                0.0,
                height(p + Vec2::Y * D, share) - height(p - Vec2::Y * D, share),
                2.0 * D,
            );
            *normal = along_z.cross(along_x).normalize_or(Vec3::Y).to_array();
        }
    }
}

/// A moment after a stroke, the circuit built anew with what was shaped
/// and painted, not saved: new circuits made from their files as read, the
/// one edited shaped as the editor has it, and the world told it was read
/// again (it keeps the one in sight till the new one is whole, see
/// `crate::world::Superseded`). The new world's terrain, once it comes, is
/// the one the editor shapes from.
pub fn rebuild_live(
    time: Res<Time<Real>>,
    buttons: Res<ButtonInput<MouseButton>>,
    editor: Res<Editor>,
    built_terrains: Res<BuiltTerrains>,
    mut circuits: ResMut<Circuits>,
    mut reloaded: MessageWriter<CircuitsReloaded>,
    mut shaping: ResMut<Shaping>,
) {
    let terrain = built_terrains
        .0
        .get(&editor.circuit)
        .map(|terrain| Arc::as_ptr(terrain) as usize);
    // The new world has come: its terrain is shaped as `built` says.
    if let Some((built, before)) = shaping.pending
        && terrain.is_some_and(|terrain| terrain != before)
    {
        shaping.pending = None;
        shaping.built = built;
        shaping.built_heights.clear();
        shaping.reach.clear();
        // Shaped further meanwhile: again.
        if shaping.sculpt.as_ref() != built {
            shaping.settle = Some(time.elapsed_secs());
        }
    }
    let Some(at) = shaping.settle else {
        return;
    };
    if time.elapsed_secs() < at || buttons.pressed(MouseButton::Left) || shaping.pending.is_some() {
        return;
    }
    shaping.settle = None;
    let Some(terrain) = terrain else {
        return;
    };
    // The ground as shaped, and what stands by the road as placed (into
    // the circuit's file's text as read, in memory).
    let sculpt: Option<&'static Sculpt> = shaping
        .sculpt
        .clone()
        .map(|sculpt| &*Box::leak(Box::new(sculpt)));
    let built = (|| -> Result<Circuits, String> {
        let files = crate::circuits::files()?;
        let text = match editor.roadside.changed {
            true => {
                let (_, read) = files
                    .get(editor.circuit)
                    .ok_or("the circuit is no longer in the list")?;
                Some(&*String::leak(editor.roadside.write(read)?))
            }
            false => None,
        };
        Circuits::parse_shaped(
            &files,
            Some(crate::circuits::Edited {
                index: editor.circuit,
                text,
                sculpt,
            }),
        )
    })();
    match built {
        Ok(new) => {
            *circuits = new;
            reloaded.write(CircuitsReloaded(vec![editor.circuit]));
            shaping.pending = Some((sculpt, terrain));
        }
        Err(error) => warn!("editor: the circuit was not built anew: {error}"),
    }
}

/// Whether the circuit is being built anew with what was shaped, not read
/// again from the disk.
pub fn building_live(shaping: &Shaping) -> bool {
    shaping.pending.is_some()
}

/// Whether the circuit is being built anew with what was shaped.
pub fn rebuilding(shaping: &Shaping) -> bool {
    shaping.pending.is_some() || shaping.settle.is_some()
}

/// Colours of the brush by its tool.
fn tool_color(tool: Tool) -> Color {
    match tool {
        Tool::Raise => Color::srgb(0.3, 1.0, 0.4),
        Tool::Lower => Color::srgb(1.0, 0.45, 0.2),
        Tool::Smooth => Color::srgb(0.4, 0.75, 1.0),
        Tool::Flatten => Color::srgb(1.0, 0.9, 0.3),
        Tool::Restore => Color::srgb(0.9, 0.9, 0.9),
        Tool::Paint(paint) => paint_color(paint),
    }
}

/// Colours of the paints, as drawn over the ground.
fn paint_color(paint: Paint) -> Color {
    match paint {
        Paint::Grass => Color::srgb(0.2, 0.85, 0.2),
        Paint::Sand => Color::srgb(1.0, 0.85, 0.45),
        Paint::Rock => Color::srgb(0.6, 0.6, 0.65),
        Paint::Land => Color::srgb(1.0, 0.3, 0.8),
    }
}

/// The brush, on the ground round the cursor: its edge, and the ring
/// within which it works all the way.
pub fn draw_brush(
    shaping: Res<Shaping>,
    editor: Res<Editor>,
    built_terrains: Res<BuiltTerrains>,
    mut gizmos: Gizmos,
) {
    let (Some(at), Some(terrain)) = (shaping.cursor, built_terrains.0.get(&editor.circuit)) else {
        return;
    };
    let center = Vec2::new(at.x, at.z);
    let color = tool_color(shaping.brush.tool);
    for (radius, alpha) in [
        (shaping.brush.radius, 1.0),
        (shaping.brush.radius * 0.5, 0.5),
    ] {
        let ring = (0..=48).map(|k| {
            let angle = std::f32::consts::TAU * k as f32 / 48.0;
            let p = center + Vec2::from_angle(angle) * radius;
            Vec3::new(p.x, ground_now(terrain, &shaping, p) + 0.6, p.y)
        });
        gizmos.linestrip(ring, color.with_alpha(alpha));
    }
    gizmos.line(at, at + Vec3::Y * shaping.brush.radius * 0.3, color);
    // Painting, the ground painted round the brush, as it is painted (it
    // shows on the ground once the circuit is built anew).
    if shaping.brush.tool.paints()
        && let Some(sculpt) = &shaping.sculpt
    {
        let reach = shaping.brush.radius * 2.0;
        let step = (shaping.brush.radius / 8.0).max(sculpt.cell);
        let steps = (reach / step) as i32;
        for j in -steps..=steps {
            for i in -steps..=steps {
                let p = center + Vec2::new(i as f32, j as f32) * step;
                if p.distance(center) > reach {
                    continue;
                }
                let paint = sculpt.paint_at(p);
                if paint == Paint::Land {
                    continue;
                }
                let base = Vec3::new(p.x, ground_now(terrain, &shaping, p) + 0.4, p.y);
                let half = step * 0.3;
                gizmos.line(
                    base - Vec3::X * half,
                    base + Vec3::X * half,
                    paint_color(paint),
                );
                gizmos.line(
                    base - Vec3::Z * half,
                    base + Vec3::Z * half,
                    paint_color(paint),
                );
            }
        }
    }
}

/// What the terrain mode says, for the editor's text.
pub fn shaping_text(shaping: &Shaping) -> String {
    if !shaping.on {
        return "Mode: lap (M for the terrain)".into();
    }
    let tools: Vec<String> = Tool::ALL
        .iter()
        .enumerate()
        .map(|(k, &tool)| {
            if tool == shaping.brush.tool {
                format!("[{} {}]", k + 1, tool.name())
            } else {
                format!("{} {}", k + 1, tool.name())
            }
        })
        .collect();
    let _ = tools;
    format!(
        "Tool: {}\nBrush {:.0} m, strength {:.1}\nLeft button: shape or paint{}{}",
        shaping.brush.tool.name(),
        shaping.brush.radius,
        shaping.brush.strength,
        if shaping.changed { "\nShaped, not saved" } else { "" },
        if rebuilding(shaping) {
            "\nBuilding the circuit anew…"
        } else {
            ""
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ray_down_meets_the_ground_where_it_is() {
        // A slope rising to the east, a ray down from 100 m up.
        let height = |p: Vec2| p.x * 0.1;
        let at = ray_to_ground(
            Vec3::new(50.0, 100.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            500.0,
            height,
        )
        .unwrap();
        assert!((at.y - 5.0).abs() < 0.01, "{at}");
        // Slanting, it meets it further on, on the ground.
        let at = ray_to_ground(
            Vec3::new(0.0, 100.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            500.0,
            height,
        )
        .unwrap();
        assert!((at.y - height(Vec2::new(at.x, at.z))).abs() < 0.01, "{at}");
        // Up, or from under the ground, it does not.
        assert_eq!(ray_to_ground(Vec3::Y * 100.0, Vec3::Y, 500.0, height), None);
        assert_eq!(
            ray_to_ground(Vec3::new(50.0, 0.0, 0.0), -Vec3::Y, 500.0, height),
            None
        );
    }

    #[test]
    fn a_stroke_is_undone_and_done_again() {
        let mut shaping = Shaping::default();
        shaping.remember();
        shaping.sculpt = Some(Sculpt::flat(Vec2::ZERO, Vec2::splat(100.0), CELL));
        shaping.sculpt.as_mut().unwrap().apply(
            &Brush::default(),
            Vec2::splat(50.0),
            1.0,
            |_| 0.0,
            0.0,
        );
        let shaped = shaping.change_at(Vec2::splat(50.0));
        assert!(shaped > 0.0);
        assert!(shaping.undo());
        assert_eq!(shaping.change_at(Vec2::splat(50.0)), 0.0);
        assert!(!shaping.undo(), "nothing more to undo");
        assert!(shaping.redo());
        assert_eq!(shaping.change_at(Vec2::splat(50.0)), shaped);
    }
}
