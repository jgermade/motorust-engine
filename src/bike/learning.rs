//! The keyboard's self-learning (Options > Control > Keyboard behaviour in
//! Bézier, "Self-learning": centre or race): riding alone in free ride, the
//! line the curves are learnt from is drawn on the road, and how far the
//! bike strays from it is watched. Where it runs wide or cuts inside, and
//! on which curve the lean was then, is noted for the panel of the curves
//! (see [`crate::curves_hud`]), so that what it finds can be checked before
//! it corrects anything.
//!
//! The line is the centre of the road or the racing line (see
//! [`crate::track::racing_line`]). What it notes, in a corner (the road
//! bending tighter than [`CORNER`]), past [`STRAY`] m from the line and
//! still going away from it:
//!
//! | Lean | Going | Curve |
//! |---|---|---|
//! | leaning in, short of its limit | wide | A: too slow to lean over |
//! | held at its limit | wide | D: too little lean at this speed |
//! | coming upright the other way | wide | C: too slow to change sides |
//! | leaning in or let go | inside | E: too much turn for the lean |
//!
//! and on the straight after a corner, standing up, B: inside (back towards
//! the corner) if it stands up too slowly, wide if too quickly.
//!
//! In a corner, the turn the bike gives is also set against the one the
//! line's bend needs at its speed: asking for at least that much and
//! getting less for a moment, the curve to blame is noted before the bike
//! strays (see [`LearningLog::compare_turn`]).
//!
//! How fast the line can be ridden is planned as the rivals plan their
//! braking (see [`speed_plan`]), and drawn: red where it wants braking.
//! Going into a corner faster than that is braking late: counted, and
//! straying or going off wide then is the speed's, not the curves'.
//!
//! Going off the road in a corner or just after it (a wheel past the curb
//! onto the shoulder, into a wall, or down) is an outing, noted the same way and counting for
//! [`OUTING_WEIGHT`] corrections; but going wide with the bike already
//! turning all it can is the bike's limit, not the curves': it is counted
//! apart and corrects nothing (the corner wants less speed). Going wide
//! held at the lean's limit corrects D and E both: more lean at that speed,
//! more turn for it.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;

use super::curves::{CurveKind, CurveProbe, KeyboardMode, KeyboardShapes, LearnLine, Phase};
use super::{BikeState, BikeTuning, CrashState, Player};
use crate::menu::GameState;
use crate::race::{Mode, Rules};
use crate::settings::Settings;
use crate::track::racing_line::{path_curvature, racing_line, speed_plan};
use crate::track::{CURB_WIDTH, SURFACE_Y, Track};
use crate::welcome::GameMode;

pub struct LearningPlugin;

impl Plugin for LearningPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LearningLog>()
            .init_resource::<TargetLine>()
            .add_systems(OnEnter(GameState::Race), start_log)
            .add_systems(
                Update,
                (
                    follow_target_line,
                    show_target_line,
                    toggle_correction,
                    watch_the_line,
                )
                    .chain()
                    .run_if(in_state(GameState::Race)),
            );
    }
}

/// What changes between only noting and correcting the curves, in free
/// ride while learning: T on the keyboard, up on the d-pad.
pub const CORRECT_KEY: KeyCode = KeyCode::KeyT;
pub const CORRECT_BUTTON: GamepadButton = GamepadButton::DPadUp;

/// Short of the turn a corner needs by this share of all the bike's (or
/// past it by this, going this far inside the line, m), for this long (s):
/// the curves turn too little (or too much).
const TURN_SHORT: f32 = 0.08;
const TURN_OVER: f32 = 0.15;
const TURN_INSIDE: f32 = 0.5;
const TURN_HOLD: f32 = 0.3;

/// How many corrections an outing counts for.
pub const OUTING_WEIGHT: usize = 3;
/// From how much of all its turn the bike is turning all it can.
const ALL_ITS_TURN: f32 = 0.98;
/// Half the width of a tyre's tread (m): a wheel is on the shoulder once
/// its edge is past the curb, and how near a wall counts as against it.
const TYRE_HALF_WIDTH: f32 = 0.1;
const WALL_TOUCH: f32 = 0.6;

/// Whether a bike `distance` m from the centre line is off the road: a
/// wheel past the asphalt (`half_width` m either side) and its curbs onto
/// the shoulder, against the solid edge (`edge` m away), or `down`.
fn off_the_road(distance: f32, half_width: f32, edge: f32, down: bool) -> bool {
    down || distance + TYRE_HALF_WIDTH > half_width + CURB_WIDTH || distance + WALL_TOUCH >= edge
}

/// How far from the line counts as straying from it (m).
pub const STRAY: f32 = 1.5;
/// How tight the road bends to count as a corner (1/m: a radius of 150 m).
pub const CORNER: f32 = 1.0 / 150.0;
/// How many points of the centre line each way the bend is measured over
/// (about 2 m apart).
const BEND_WINDOW: isize = 6;
/// How long after a note on a curve the next one on it can come (s).
const NOTE_GAP: f32 = 1.0;
/// How long the straight after a corner counts as its way out (s).
const WAY_OUT: f32 = 1.5;
/// How wide the line is drawn (m), how high over the asphalt (itself
/// [`SURFACE_Y`] over the track's height), and its colour:
/// the amber of what is learnt.
const LINE_WIDTH: f32 = 0.4;
const LINE_LIFT: f32 = 0.04;
const LINE_COLOR: Color = Color::srgba(1.0, 0.72, 0.2, 0.75);
/// Where the line wants braking, red.
const BRAKE_COLOR: Color = Color::srgba(1.0, 0.18, 0.12, 0.85);
/// How much of the brakes the line's speeds are planned with: as the
/// rivals plan theirs, with room (see [`super::ai`]).
const PLANNED_BRAKING: f32 = 0.6;
/// Faster than the line's speed by this share and this much more (m/s) is
/// too fast for the corner: braking late.
const TOO_FAST_SHARE: f32 = 1.1;
const TOO_FAST_MARGIN: f32 = 2.0;

/// Which way the bike strayed from the line, round the corner: out wide or
/// in towards its inside.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Wide,
    Inside,
}

/// What the self-learning has seen this race: whether it is watching, how
/// far the bike is from the line now (m, positive out wide round the
/// corner, or to the left on a straight), whether it is in a corner, and,
/// for each curve, how many times it went wide and inside, and the last
/// note.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct LearningLog {
    pub active: bool,
    pub stray: f32,
    pub in_corner: bool,
    pub notes: [[u32; 2]; 7],
    pub last: Option<(CurveKind, Side, f32)>,
    /// How many times the bike went off the road in a corner, and how many
    /// of those turning all it could; whether it was off in the step
    /// before, and whether the last note was an outing.
    pub outings: u32,
    pub at_limit: u32,
    /// How many corners the bike went into faster than the line can be
    /// ridden there (braking late), and, while it is too fast, the speed
    /// it should be going (m/s).
    pub late: u32,
    pub too_fast: Option<f32>,
    pub was_late: bool,
    /// In a corner, the share of its turn the bike gives and the share the
    /// line's bend needs at its speed; since when it has given too little
    /// (or too much), for all the turn asked.
    pub given: f32,
    pub needed: Option<f32>,
    pub short_since: Option<f32>,
    pub over_since: Option<f32>,
    pub was_off: bool,
    pub last_outing: bool,
    /// The bend of the last corner (1/m, positive to the left) and when it
    /// was last in it (s), for its way out; how far to the left of the line
    /// the bike was in the step before; when each curve was last noted.
    pub corner: Option<(f32, f32)>,
    pub was: f32,
    pub noted: [f32; 7],
}

impl LearningLog {
    /// How many times curve `kind` went `side`.
    pub fn count(&self, kind: CurveKind, side: Side) -> u32 {
        self.notes[kind as usize][side as usize]
    }

    /// The bike going `speed` where the line can be ridden at `plan` (m/s),
    /// in a corner (`in_corner`) or not: too fast for it, by
    /// [`TOO_FAST_SHARE`] and [`TOO_FAST_MARGIN`], is braking late, counted
    /// once a corner. Straying while too fast is the speed's, not the
    /// curves'.
    pub fn pace(&mut self, speed: f32, plan: f32, in_corner: bool) {
        let fast = speed > plan * TOO_FAST_SHARE + TOO_FAST_MARGIN;
        self.too_fast = fast.then_some(plan);
        let late = fast && in_corner;
        if late && !self.was_late {
            self.late += 1;
        }
        if late || !in_corner {
            self.was_late = late;
        }
    }

    /// In a corner (`needed` the share of its turn the line's bend asks of
    /// the bike at its speed, if in one), how much the bike turns against
    /// it: asking at least that much (`probe.wanted`) and getting less for
    /// [`TURN_HOLD`] s, it will go wide, on the curve to blame; getting more
    /// and going inside (`out` m wide of the line, negative inside), E. Not
    /// while too fast: then no turn would do. Notes it at `now`.
    pub fn compare_turn(&mut self, needed: Option<f32>, out: f32, probe: &CurveProbe, now: f32) {
        self.needed = needed;
        self.given = probe.turn.abs();
        let Some(needed) = needed.filter(|_| self.too_fast.is_none()) else {
            (self.short_since, self.over_since) = (None, None);
            return;
        };
        let short = probe.wanted >= needed && self.given < needed - TURN_SHORT;
        let over = self.given > needed + TURN_OVER && out < -TURN_INSIDE;
        let held = |since: &mut Option<f32>, on: bool| {
            if !on {
                *since = None;
                return false;
            }
            now - *since.get_or_insert(now) >= TURN_HOLD
        };
        if held(&mut self.short_since, short)
            && let Some(kind) = Self::blame(Side::Wide, probe)
        {
            self.note(kind, Side::Wide, now);
        }
        if held(&mut self.over_since, over) {
            self.note(CurveKind::Turn, Side::Inside, now);
        }
    }

    /// The curve the lean was on going `side` round a corner, as `probe`
    /// says, if any: wide while leaning in short of what is asked is A,
    /// held where the stick asks short of the limit R, at the limit D,
    /// unwinding C, standing up B; inside is E.
    fn blame(side: Side, probe: &CurveProbe) -> Option<CurveKind> {
        let lean = probe.lean;
        if side == Side::Inside {
            return Some(CurveKind::Turn);
        }
        match lean.phase {
            Phase::LeaningIn if lean.lean.abs() < probe.asked.min(probe.limit) - 0.02 => {
                Some(CurveKind::LeanIn)
            }
            Phase::LeaningIn if probe.stick && probe.asked < probe.limit - 0.02 => {
                Some(CurveKind::Response)
            }
            Phase::LeaningIn => Some(CurveKind::TurnSpeed),
            Phase::Unwinding => Some(CurveKind::Unwind),
            Phase::StandingUp => Some(CurveKind::StandUp),
            Phase::Upright => None,
        }
    }

    /// The bike going off the road (`off`) `stray` m left of the line, at
    /// `now`: in a corner bending `bend` (or on the way out of the last
    /// one), an outing on the curve to blame, noted whatever came just
    /// before; turning all it can, out wide, the bike's limit instead.
    /// Returns the curve and the side, to correct.
    pub fn went_off(
        &mut self,
        off: bool,
        stray: f32,
        bend: f32,
        probe: &CurveProbe,
        now: f32,
    ) -> Option<(CurveKind, Side)> {
        let fresh = off && !self.was_off;
        self.was_off = off;
        if !fresh {
            return None;
        }
        let bend = if bend.abs() > CORNER {
            bend
        } else {
            match self.corner {
                Some((corner, left)) if now - left < WAY_OUT => corner,
                _ => return None,
            }
        };
        let side = if -stray * bend.signum() > 0.0 {
            Side::Wide
        } else {
            Side::Inside
        };
        self.outings += 1;
        if side == Side::Wide && (probe.turn.abs() >= ALL_ITS_TURN || self.too_fast.is_some()) {
            self.at_limit += 1;
            return None;
        }
        let kind = Self::blame(side, probe)?;
        self.notes[kind as usize][side as usize] += 1;
        self.noted[kind as usize] = now;
        self.last = Some((kind, side, now));
        self.last_outing = true;
        Some((kind, side))
    }

    /// Notes curve `kind` going `side` at `now` (s), if not noted just
    /// before.
    fn note(&mut self, kind: CurveKind, side: Side, now: f32) {
        let at = kind as usize;
        if now - self.noted[at] < NOTE_GAP && self.noted[at] > 0.0 {
            return;
        }
        self.noted[at] = now;
        self.notes[at][side as usize] += 1;
        self.last = Some((kind, side, now));
        self.last_outing = false;
    }

    /// Goes on with the bike `stray` m to the left of the line where the
    /// road bends `bend` (1/m, positive to the left), its lean as `probe`
    /// says, at `now` (s).
    pub fn watch(&mut self, stray: f32, bend: f32, probe: &CurveProbe, now: f32) {
        let in_corner = bend.abs() > CORNER;
        let moved = stray - self.was;
        self.was = stray;
        // Round a corner bending `bend`: how far out wide of the line the
        // bike is (negative inside), and whether it is going further from
        // the line, past straying.
        let round = |bend: f32| {
            let (out, going) = (-stray * bend.signum(), -moved * bend.signum());
            (out, out.abs() > STRAY && going * out > 0.0)
        };
        let lean = probe.lean;
        if in_corner {
            let (out, leaving) = round(bend);
            if leaving && out > 0.0 {
                // Standing up or upright in a corner, wide, or too fast for
                // it: not the lean's.
                if self.too_fast.is_none()
                    && matches!(lean.phase, Phase::LeaningIn | Phase::Unwinding)
                    && let Some(kind) = Self::blame(Side::Wide, probe)
                {
                    self.note(kind, Side::Wide, now);
                }
            } else if leaving {
                self.note(CurveKind::Turn, Side::Inside, now);
            }
            self.corner = Some((bend, now));
            self.stray = out;
        } else {
            // On the way out of the last corner, standing up.
            if let Some((corner, left)) = self.corner
                && now - left < WAY_OUT
            {
                let (out, leaving) = round(corner);
                if leaving && lean.phase == Phase::StandingUp {
                    let side = if out > 0.0 { Side::Wide } else { Side::Inside };
                    self.note(CurveKind::StandUp, side, now);
                }
            }
            self.stray = stray;
        }
        self.in_corner = in_corner;
    }
}

/// The line drawn and watched: which one, and how far to the left of the
/// centre line it is at each of its points (all nought for the centre).
#[derive(Resource, Default)]
pub struct TargetLine {
    pub line: Option<LearnLine>,
    pub lateral: Vec<f32>,
    /// The fastest the line can be ridden at each point (m/s), and whether
    /// each is in a braking zone (see [`speed_plan`]).
    pub speed: Vec<f32>,
    pub braking: Vec<bool>,
    /// How sharp it bends at each point (1/m, without its sign).
    pub bend: Vec<f32>,
}

impl TargetLine {
    /// The fastest the line can be ridden between points `index` and the
    /// next, `t` of the way (m/s; without a plan, no limit).
    pub fn bend_at(&self, index: usize, t: f32) -> f32 {
        let n = self.bend.len();
        if n == 0 {
            return 0.0;
        }
        let (a, b) = (self.bend[index % n], self.bend[(index + 1) % n]);
        a + (b - a) * t
    }

    pub fn plan_at(&self, index: usize, t: f32) -> f32 {
        let n = self.speed.len();
        if n == 0 {
            return f32::INFINITY;
        }
        let (a, b) = (self.speed[index % n], self.speed[(index + 1) % n]);
        a + (b - a) * t
    }

    /// How far to the left of the centre line the line is between points
    /// `index` and the next, `t` of the way.
    pub fn at(&self, index: usize, t: f32) -> f32 {
        let n = self.lateral.len();
        if n == 0 {
            return 0.0;
        }
        let (a, b) = (self.lateral[index % n], self.lateral[(index + 1) % n]);
        a + (b - a) * t
    }
}

/// The line the self-learning watches now, if it is: with the keyboard or
/// the stick in Bézier and learning, one player in free ride. If both are
/// learning, the keyboard's line.
pub fn learning_line(
    settings: &Settings,
    rules: Option<&Rules>,
    mode: GameMode,
) -> Option<LearnLine> {
    let free = rules.is_some_and(|rules| rules.mode == Mode::Free);
    if !free || mode != GameMode::SinglePlayer {
        return None;
    }
    let learning = |mode: KeyboardMode, shapes: &KeyboardShapes| {
        (mode == KeyboardMode::Bezier && shapes.learning()).then_some(shapes.learn)
    };
    learning(settings.keyboard_mode, &settings.keyboard_shapes)
        .or_else(|| learning(settings.stick_mode, &settings.stick_shapes))
}

/// The curves the self-learning works on: the stick's while it leans the
/// bike, the keyboard's otherwise.
pub fn learnt_shapes(settings: &mut Settings, stick: bool) -> &mut KeyboardShapes {
    if stick {
        &mut settings.stick_shapes
    } else {
        &mut settings.keyboard_shapes
    }
}

/// A new race, a new log.
fn start_log(mut log: ResMut<LearningLog>) {
    *log = LearningLog::default();
}

/// The line worked out again when the track or the line chosen change.
fn follow_target_line(
    settings: Res<Settings>,
    rules: Option<Res<Rules>>,
    mode: Res<GameMode>,
    track: Res<Track>,
    tuning: Res<BikeTuning>,
    mut target: ResMut<TargetLine>,
) {
    let line = learning_line(&settings, rules.as_deref(), *mode);
    if line == target.line && !track.is_changed() {
        return;
    }
    let n = track.centerline().len();
    target.lateral = match line {
        Some(LearnLine::Race) => racing_line(track.centerline(), |i| track.half_width_at(i)),
        _ => vec![0.0; n],
    };
    // How fast it can be ridden: braking as the rivals plan to, with room.
    let points: Vec<Vec2> = (0..n)
        .map(|i| track.offset_point(i, target.lateral[i]))
        .collect();
    target.bend = path_curvature(&points, BEND_WINDOW as usize);
    (target.speed, target.braking) = speed_plan(
        &target.bend,
        track.step_length(),
        tuning.max_speed,
        tuning.max_lateral_accel,
        tuning.brake_decel * PLANNED_BRAKING,
    );
    target.line = line;
}

/// The line on the road.
#[derive(Component)]
struct DrawnLine;

/// The line drawn on the road while it is watched, again when it changes.
fn show_target_line(
    mut commands: Commands,
    target: Res<TargetLine>,
    track: Res<Track>,
    drawn: Query<Entity, With<DrawnLine>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !target.is_changed() && !drawn.is_empty() {
        return;
    }
    for entity in &drawn {
        commands.entity(entity).despawn();
    }
    if target.line.is_none() || target.lateral.is_empty() {
        return;
    }
    commands.spawn((
        DrawnLine,
        DespawnOnExit(GameState::Race),
        Mesh3d(meshes.add(line_mesh(&track, &target.lateral, &target.braking))),
        MeshMaterial3d(materials.add(StandardMaterial {
            // Each stretch in its own colour (see `line_mesh`).
            base_color: Color::WHITE,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            depth_bias: 50.0,
            // Seen from above whichever way its triangles wind.
            cull_mode: None,
            double_sided: true,
            ..default()
        })),
        Transform::default(),
    ));
}

/// A strip `LINE_WIDTH` wide all round the lap, `lateral` m to the left of
/// each point of the centre line, just over the road: amber, and red where
/// it is `braking`.
fn line_mesh(track: &Track, lateral: &[f32], braking: &[bool]) -> Mesh {
    let n = lateral.len();
    let mut positions = Vec::with_capacity(2 * n);
    let mut colors = Vec::with_capacity(2 * n);
    for (index, &left) in lateral.iter().enumerate() {
        let color = if braking.get(index).copied().unwrap_or(false) {
            BRAKE_COLOR
        } else {
            LINE_COLOR
        };
        for side in [-0.5, 0.5] {
            let point = track.surface_point(index, left + side * LINE_WIDTH);
            positions.push(point + Vec3::Y * (SURFACE_Y + LINE_LIFT));
            colors.push(color.to_linear().to_f32_array());
        }
    }
    let mut indices = Vec::with_capacity(6 * n);
    for index in 0..n as u32 {
        let next = (index + 1) % n as u32;
        let (a, b, c, d) = (2 * index, 2 * index + 1, 2 * next, 2 * next + 1);
        indices.extend([a, c, b, b, c, d]);
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

/// While learning, T (or up on the d-pad) changes between only noting
/// where the bike strays and correcting the curves for it; not paused.
fn toggle_correction(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    pause: Res<crate::pause::Pause>,
    target: Res<TargetLine>,
    probe: Res<CurveProbe>,
    mut settings: ResMut<Settings>,
) {
    let pressed = keyboard.just_pressed(CORRECT_KEY)
        || gamepads.iter().any(|pad| pad.just_pressed(CORRECT_BUTTON));
    if pressed && target.line.is_some() && !pause.is_open() {
        let shapes = learnt_shapes(&mut settings, probe.stick);
        shapes.correct = !shapes.correct;
    }
}

/// Watches how far the player's bike strays from the line and, correcting,
/// corrects the curve it strayed on.
#[allow(clippy::too_many_arguments)]
fn watch_the_line(
    time: Res<Time>,
    target: Res<TargetLine>,
    track: Res<Track>,
    probe: Res<CurveProbe>,
    tuning: Res<BikeTuning>,
    player: Query<(&BikeState, &CrashState), With<Player>>,
    mut log: ResMut<LearningLog>,
    mut settings: ResMut<Settings>,
) {
    let active = target.line.is_some() && probe.live;
    if log.active != active {
        log.active = active;
    }
    let Ok((state, crash)) = player.single() else {
        return;
    };
    if !active {
        return;
    }
    let nearest = track.nearest_at(state.position, state.elevation);
    if nearest.road != 0 {
        return;
    }
    let now = time.elapsed_secs();
    let off = off_the_road(
        nearest.distance,
        track.half_width_beside(&nearest),
        track.edge_beside(&nearest),
        crash.is_down(),
    );
    let stray = nearest.lateral - target.at(nearest.index, nearest.t);
    let bend = (-BEND_WINDOW..=BEND_WINDOW)
        .map(|k| track.curvature((nearest.index as isize + k).max(0) as usize))
        .sum::<f32>()
        / (2 * BEND_WINDOW + 1) as f32;
    let plan = target.plan_at(nearest.index, nearest.t);
    log.pace(state.speed.abs(), plan, bend.abs() > CORNER);
    if let Some((kind, side)) = log.went_off(off, stray, bend, &probe, now) {
        if correcting(&settings, probe.stick) {
            for _ in 0..OUTING_WEIGHT {
                correct(&mut settings, probe.stick, kind, side, probe.speed_share);
            }
        }
        return;
    }
    if crash.is_down() || !state.grounded || state.speed < 5.0 {
        return;
    }
    let before = log.last;
    // The turn the line's bend asks of the bike at its speed.
    let speed = state.speed.abs();
    let needed = (bend.abs() > CORNER)
        .then(|| target.bend_at(nearest.index, nearest.t) * speed / tuning.max_yaw_rate(speed));
    log.compare_turn(needed, -stray * bend.signum(), &probe, now);
    log.watch(stray, bend, &probe, now);
    if log.last != before
        && correcting(&settings, probe.stick)
        && let Some((kind, side, _)) = log.last
    {
        correct(&mut settings, probe.stick, kind, side, probe.speed_share);
    }
}

/// Whether the curves of the stick (or the keyboard) are being corrected,
/// read without touching the settings, which are saved when they change.
fn correcting(settings: &Settings, stick: bool) -> bool {
    if stick {
        settings.stick_shapes.correct
    } else {
        settings.keyboard_shapes.correct
    }
}

/// Corrects curve `kind` of the stick's (or the keyboard's) for going
/// `side` at `speed_share` of the top speed: at the lean's limit, wide (F),
/// D and E too, more lean and more turn for it.
fn correct(settings: &mut Settings, stick: bool, kind: CurveKind, side: Side, speed_share: f32) {
    let shapes = learnt_shapes(settings, stick);
    let wide = side == Side::Wide;
    shapes.correct_for(kind, wide, speed_share);
    if kind == CurveKind::TurnSpeed && wide {
        shapes.correct_for(CurveKind::Speed, true, speed_share);
        shapes.correct_for(CurveKind::Turn, true, speed_share);
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;
    use crate::bike::curves::CurveLean;

    /// The lean on `phase`, `lean` over, its limit `limit`.
    fn probe(phase: Phase, lean: f32, limit: f32) -> CurveProbe {
        let mut following = CurveLean::default();
        (following.lean, following.phase, following.progress) = (lean, phase, 0.5);
        CurveProbe {
            live: true,
            stick: false,
            travel: 0.0,
            asked: 1.0,
            wanted: 1.0,
            lean: following,
            speed_share: 0.5,
            limit,
            turn: lean,
            turn_limit: 1.0,
        }
    }

    /// The same, the stick's, pushed asking for `asked`.
    fn stick_probe(phase: Phase, lean: f32, asked: f32, limit: f32) -> CurveProbe {
        CurveProbe {
            stick: true,
            travel: asked,
            asked,
            ..probe(phase, lean, limit)
        }
    }

    /// The bike going from `from` to `to` m left of the line, in steps of a
    /// tenth of a second, where the road bends `bend`.
    fn ride(log: &mut LearningLog, from: f32, to: f32, bend: f32, probe: &CurveProbe, start: f32) {
        for k in 0..=10 {
            let stray = from + (to - from) * k as f32 / 10.0;
            log.watch(stray, bend, probe, start + k as f32 * 0.1);
        }
    }

    const LEFT: f32 = 1.0 / 60.0;

    #[test]
    fn going_wide_while_leaning_in_is_a_too_slow() {
        let mut log = LearningLog::default();
        // A left-hander: wide is to the right.
        ride(
            &mut log,
            0.0,
            -3.0,
            LEFT,
            &probe(Phase::LeaningIn, 0.4, 0.9),
            1.0,
        );
        assert_eq!(log.count(CurveKind::LeanIn, Side::Wide), 1);
        assert!(log.in_corner && log.stray > STRAY);
        // At its limit: D.
        ride(
            &mut log,
            0.0,
            -3.0,
            LEFT,
            &probe(Phase::LeaningIn, 0.9, 0.9),
            5.0,
        );
        assert_eq!(log.count(CurveKind::TurnSpeed, Side::Wide), 1);
        // Unwinding: C.
        ride(
            &mut log,
            0.0,
            3.0,
            -LEFT,
            &probe(Phase::Unwinding, -0.5, 0.9),
            9.0,
        );
        assert_eq!(log.count(CurveKind::Unwind, Side::Wide), 1);
    }

    #[test]
    fn with_the_stick_going_wide_held_where_it_asks_is_its_response() {
        let mut log = LearningLog::default();
        // Held at the lean the stick asks, short of the limit: R.
        let held = stick_probe(Phase::LeaningIn, 0.5, 0.5, 0.9);
        ride(&mut log, 0.0, -3.0, LEFT, &held, 1.0);
        assert_eq!(log.count(CurveKind::Response, Side::Wide), 1);
        // Still leaning in towards it: A.
        let leaning = stick_probe(Phase::LeaningIn, 0.3, 0.5, 0.9);
        ride(&mut log, 0.0, -3.0, LEFT, &leaning, 5.0);
        assert_eq!(log.count(CurveKind::LeanIn, Side::Wide), 1);
        // Pushed all the way, at the limit: D.
        let full = stick_probe(Phase::LeaningIn, 0.9, 1.0, 0.9);
        ride(&mut log, 0.0, -3.0, LEFT, &full, 9.0);
        assert_eq!(log.count(CurveKind::TurnSpeed, Side::Wide), 1);
    }

    #[test]
    fn cutting_inside_is_too_much_turn_and_coming_back_is_nothing() {
        let mut log = LearningLog::default();
        ride(
            &mut log,
            0.0,
            3.0,
            LEFT,
            &probe(Phase::LeaningIn, 0.5, 0.9),
            1.0,
        );
        assert_eq!(log.count(CurveKind::Turn, Side::Inside), 1);
        // Coming back towards the line, however far: nothing more.
        ride(
            &mut log,
            3.0,
            2.0,
            LEFT,
            &probe(Phase::LeaningIn, 0.5, 0.9),
            5.0,
        );
        assert_eq!(log.notes.iter().flatten().sum::<u32>(), 1);
        // Near the line, nothing.
        ride(
            &mut log,
            0.0,
            1.0,
            LEFT,
            &probe(Phase::LeaningIn, 0.5, 0.9),
            9.0,
        );
        assert_eq!(log.notes.iter().flatten().sum::<u32>(), 1);
    }

    #[test]
    fn on_the_way_out_of_a_corner_standing_up_is_b() {
        let mut log = LearningLog::default();
        ride(
            &mut log,
            0.0,
            0.5,
            LEFT,
            &probe(Phase::LeaningIn, 0.5, 0.9),
            1.0,
        );
        // Out on the straight, standing up and still turning in: too slow.
        ride(
            &mut log,
            0.5,
            3.0,
            0.0,
            &probe(Phase::StandingUp, 0.3, 1.0),
            2.1,
        );
        assert_eq!(log.count(CurveKind::StandUp, Side::Inside), 1);
        // Long after the corner, nothing.
        ride(
            &mut log,
            0.0,
            3.0,
            0.0,
            &probe(Phase::StandingUp, 0.3, 1.0),
            10.0,
        );
        assert_eq!(log.count(CurveKind::StandUp, Side::Inside), 1);
    }

    #[test]
    fn going_off_in_a_corner_is_an_outing_on_the_curve_to_blame() {
        let mut log = LearningLog::default();
        // Leaning in, short of the limit, off wide in a left-hander: A.
        let leaning = probe(Phase::LeaningIn, 0.5, 0.9);
        assert_eq!(
            log.went_off(true, -6.0, LEFT, &leaning, 1.0),
            Some((CurveKind::LeanIn, Side::Wide))
        );
        assert!(log.last_outing && log.outings == 1);
        // Still off: nothing more; back on and off again inside: E.
        assert_eq!(log.went_off(true, -6.0, LEFT, &leaning, 1.1), None);
        assert_eq!(log.went_off(false, 0.0, LEFT, &leaning, 1.2), None);
        assert_eq!(
            log.went_off(true, 6.0, LEFT, &leaning, 1.3),
            Some((CurveKind::Turn, Side::Inside))
        );
        // At the lean's limit, wide: D (the correction raises E too).
        log.went_off(false, 0.0, LEFT, &leaning, 2.0);
        let held = probe(Phase::LeaningIn, 0.9, 0.9);
        assert_eq!(
            log.went_off(true, -6.0, LEFT, &held, 2.1),
            Some((CurveKind::TurnSpeed, Side::Wide))
        );
        // Turning all it can: the bike's limit, no curve to blame.
        log.went_off(false, 0.0, LEFT, &leaning, 3.0);
        let all = CurveProbe { turn: 1.0, ..held };
        assert_eq!(log.went_off(true, -6.0, LEFT, &all, 3.1), None);
        assert_eq!((log.outings, log.at_limit), (4, 1));
        // On a straight, long after any corner: not an outing.
        log.went_off(false, 0.0, 0.0, &leaning, 9.0);
        assert_eq!(log.went_off(true, -6.0, 0.0, &leaning, 9.1), None);
        assert_eq!(log.outings, 4);
    }

    #[test]
    fn going_wide_at_the_lean_s_limit_corrects_f_d_and_e() {
        let mut settings = Settings::default();
        settings.keyboard_shapes.set_learn(LearnLine::Centre);
        let before = settings.keyboard_shapes;
        // Near top speed: the shares at top speed rise too.
        correct(&mut settings, false, CurveKind::TurnSpeed, Side::Wide, 0.9);
        let after = settings.keyboard_shapes;
        for kind in [CurveKind::TurnSpeed, CurveKind::Speed] {
            assert!(after.bezier(kind).y2 < before.bezier(kind).y2, "{kind:?}");
        }
        assert!(after.bezier(CurveKind::Turn).y2 > before.bezier(CurveKind::Turn).y2);
        let top = |shapes: &KeyboardShapes| shapes.scale(CurveKind::Speed).unwrap();
        assert!(top(&after) > top(&before));
        // Slow, the shapes only.
        let at = after;
        correct(&mut settings, false, CurveKind::TurnSpeed, Side::Wide, 0.3);
        assert_eq!(top(&settings.keyboard_shapes), top(&at));
        // The stick's, untouched.
        assert_eq!(settings.stick_shapes, KeyboardShapes::classic(true));
    }

    #[test]
    fn a_wheel_past_the_curb_is_off_the_road() {
        let (half, edge) = (7.0, 11.5);
        // On the asphalt, and on the curb with the whole tyre: on the road.
        assert!(!off_the_road(6.0, half, edge, false));
        assert!(!off_the_road(half + CURB_WIDTH - 0.15, half, edge, false));
        // The tyre's edge past the curb, onto the shoulder: off.
        assert!(off_the_road(half + CURB_WIDTH - 0.05, half, edge, false));
        // Against the wall, or down: off.
        assert!(off_the_road(5.0, half, 5.4, false));
        assert!(off_the_road(0.0, half, edge, true));
    }

    #[test]
    fn going_into_a_corner_too_fast_is_braking_late_not_the_curves() {
        let mut log = LearningLog::default();
        let leaning = probe(Phase::LeaningIn, 0.5, 0.9);
        // On the straight, too fast for the corner ahead: told the speed,
        // not yet late.
        log.pace(50.0, 30.0, false);
        assert_eq!((log.too_fast, log.late), (Some(30.0), 0));
        // Into the corner still too fast: late, once.
        for _ in 0..10 {
            log.pace(50.0, 30.0, true);
        }
        assert_eq!(log.late, 1);
        // Going wide then: nothing blamed; off wide: the limit.
        ride(&mut log, 0.0, -3.0, LEFT, &leaning, 1.0);
        assert_eq!(log.notes.iter().flatten().sum::<u32>(), 0);
        assert_eq!(log.went_off(true, -6.0, LEFT, &leaning, 3.0), None);
        assert_eq!(log.at_limit, 1);
        // Slow enough: nothing.
        log.pace(31.0, 30.0, true);
        assert_eq!(log.too_fast, None);
        // The next corner, too fast again: late again.
        log.pace(31.0, 30.0, false);
        log.pace(50.0, 30.0, true);
        assert_eq!(log.late, 2);
    }

    #[test]
    fn turning_less_than_the_corner_needs_for_all_the_turn_asked_is_noted() {
        let mut log = LearningLog::default();
        // At the lean's limit, giving 60 % for the 80 % the corner needs,
        // asking for all of it: D, after a moment.
        let held = CurveProbe {
            turn: 0.6,
            wanted: 1.0,
            ..probe(Phase::LeaningIn, 0.9, 0.9)
        };
        log.compare_turn(Some(0.8), 0.0, &held, 1.0);
        assert_eq!(log.count(CurveKind::TurnSpeed, Side::Wide), 0);
        log.compare_turn(Some(0.8), 0.0, &held, 1.4);
        assert_eq!(log.count(CurveKind::TurnSpeed, Side::Wide), 1);
        assert_eq!((log.given, log.needed), (0.6, Some(0.8)));
        // Asking less than it needs: the player's choice, nothing.
        let mut log = LearningLog::default();
        let shy = CurveProbe {
            wanted: 0.5,
            ..held
        };
        log.compare_turn(Some(0.8), 0.0, &shy, 1.0);
        log.compare_turn(Some(0.8), 0.0, &shy, 1.5);
        assert_eq!(log.notes.iter().flatten().sum::<u32>(), 0);
        // Turning more than it needs and going inside: E.
        let sharp = CurveProbe { turn: 0.9, ..held };
        log.compare_turn(Some(0.5), -1.0, &sharp, 2.0);
        log.compare_turn(Some(0.5), -1.0, &sharp, 2.4);
        assert_eq!(log.count(CurveKind::Turn, Side::Inside), 1);
        // Too fast: no turn would do, nothing.
        let mut log = LearningLog::default();
        log.pace(50.0, 30.0, true);
        log.compare_turn(Some(0.8), 0.0, &held, 1.0);
        log.compare_turn(Some(0.8), 0.0, &held, 1.5);
        assert_eq!(log.notes.iter().flatten().sum::<u32>(), 0);
    }

    #[test]
    fn a_curve_is_noted_once_a_second_at_most() {
        let mut log = LearningLog::default();
        let leaning = probe(Phase::LeaningIn, 0.4, 0.9);
        for k in 0..20 {
            log.watch(-2.0 - k as f32 * 0.1, LEFT, &leaning, 1.0 + k as f32 * 0.1);
        }
        assert_eq!(log.count(CurveKind::LeanIn, Side::Wide), 2);
        assert_eq!(
            log.last.map(|(kind, side, _)| (kind, side)),
            Some((CurveKind::LeanIn, Side::Wide))
        );
    }

    #[test]
    fn the_target_line_is_read_between_its_points() {
        let target = TargetLine {
            line: Some(LearnLine::Race),
            lateral: vec![0.0, 2.0, 4.0],
            ..default()
        };
        assert_eq!(target.at(0, 0.5), 1.0);
        // Round the end of the lap, back to the first.
        assert_eq!(target.at(2, 0.5), 2.0);
        assert_eq!(TargetLine::default().at(3, 0.5), 0.0);
    }

    #[test]
    fn it_learns_only_alone_in_free_ride_with_the_keyboard_in_bezier() {
        let mut settings = Settings::default();
        let free = Rules {
            mode: Mode::Free,
            ..default()
        };
        let one = GameMode::SinglePlayer;
        assert_eq!(learning_line(&settings, Some(&free), one), None);
        // The stick's learning.
        settings.stick_mode = KeyboardMode::Bezier;
        settings.stick_shapes.set_learn(LearnLine::Centre);
        assert_eq!(
            learning_line(&settings, Some(&free), one),
            Some(LearnLine::Centre)
        );
        // Both: the keyboard's line.
        settings.keyboard_mode = KeyboardMode::Bezier;
        settings.keyboard_shapes.set_learn(LearnLine::Race);
        assert_eq!(
            learning_line(&settings, Some(&free), one),
            Some(LearnLine::Race)
        );
        let race = Rules {
            mode: Mode::Race,
            ..default()
        };
        assert_eq!(learning_line(&settings, Some(&race), one), None);
        assert_eq!(
            learning_line(&settings, Some(&free), GameMode::SplitScreen),
            None
        );
    }
}
