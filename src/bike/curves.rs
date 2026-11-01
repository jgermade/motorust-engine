//! The keyboard's curves: with Options > Control > Advanced's keyboard
//! behaviour on Bézier, the keyboard and the d-pad lean the bike over and the lean
//! turns it, each step shaped by a Bézier curve (see [`crate::bezier`]):
//!
//! - **A, leaning in**: how far over the rider is for how long the key has
//!   been held, and how long that takes.
//! - **B, standing up**: how far back up for how long it has been let go.
//! - **C, unwinding**: turning the other way, how the lean comes back to
//!   upright first (at once, as always, or over a time).
//! - **D, the lean at speed**: how far over the bike goes at most for how
//!   fast it goes, down to a share at top speed.
//! - **E, the turn for the lean**: how much the bike turns for how far
//!   over it is.
//!
//! Each curve's shape is chosen (see [`CurveShape`]): the classic one (the
//! keyboard as it is without the curves, the default), CSS's easings, its
//! own four numbers, or the one the game learns from how the player rides.
//! The lean shown is the one the curves give (see [`super::step`]), so
//! what is seen is what turns the bike.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::bezier::Bezier;
use crate::locale::Language;

/// How the keyboard and the d-pad steer, as Options > Control > Advanced
/// says: as always (Options > Control > Keyboard behaviour's rates and
/// limit), or by the curves (that page then sets them).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum KeyboardMode {
    #[default]
    Classic,
    Bezier,
}

pub const KEYBOARD_MODES: [KeyboardMode; 2] = [KeyboardMode::Classic, KeyboardMode::Bezier];

impl KeyboardMode {
    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (KeyboardMode::Classic, Language::Spanish) => "Clásico",
            (KeyboardMode::Classic, Language::English) => "Classic",
            (KeyboardMode::Bezier, _) => "Bézier",
        }
    }
}

/// A curve run over a time: how far through it is how long since it began
/// out of `seconds`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TimedCurve {
    pub curve: Bezier,
    pub seconds: f32,
}

/// The keyboard's curves the bike follows: what [`KeyboardShapes`] (what
/// Options > Control > Keyboard behaviour chooses in Bézier) comes to.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct KeyboardCurves {
    /// A: from upright to all the way over, holding a key.
    pub lean_in: TimedCurve,
    /// B: from all the way over to upright, with no key held.
    pub stand_up: TimedCurve,
    /// C: from all the way over to upright, holding the other way's key; at
    /// once with no time (then it leans in from upright as A says).
    pub unwind: TimedCurve,
    /// D: how much of the bike's lean is kept, from all of it standing still
    /// down to `top_speed_lean` at top speed, for the share of top speed.
    pub speed: Bezier,
    pub top_speed_lean: f32,
    /// E: the share of the bike's turn for the share of its lean.
    pub turn: Bezier,
    /// F: how much of that turn is kept, from all of it standing still down
    /// to `top_speed_turn` at top speed, for the share of top speed.
    pub turn_speed: Bezier,
    pub top_speed_turn: f32,
    /// How the lean shown follows the one felt.
    pub look: LeanLook,
    /// R, the stick's: the lean asked for how far it is pushed past its
    /// dead zone.
    pub response: Bezier,
}

impl Default for KeyboardCurves {
    fn default() -> Self {
        KeyboardShapes::default().curves()
    }
}

/// How much a correction moves a learnt curve's control points up or down,
/// and the least E's may come down to (y1, y2).
pub const NUDGE: f32 = 0.02;
pub const TURN_FLOOR: (f32, f32) = (0.1, 0.3);

/// Bounds of what the curves' numbers can be set to.
pub const SECONDS: (f32, f32) = (0.05, 1.5);
pub const TOP_SPEED_LEAN: (f32, f32) = (0.3, 1.0);
pub const TOP_SPEED_TURN: (f32, f32) = (0.3, 1.0);
/// How much a correction moves a learnt time (a share of it), and a
/// learnt share at top speed (D's lean, F's turn), only from this share of
/// top speed on.
pub const NUDGE_TIME: f32 = 0.03;
pub const NUDGE_TOP: f32 = 0.01;
pub const NEAR_TOP_SPEED: f32 = 0.75;

/// The classic keyboard's times and its lean at top speed: as
/// `DigitalSteer` at medium, held 1.875 t + 1.875 t² up to all of it
/// (0.385 s), let go 0.25 t + t² back to upright (0.883 s), upright at once
/// the other way; its limit with speed two thirds of the way from all of
/// it to 0.8.
const CLASSIC_SECONDS: [f32; 3] = [0.385, 0.883, 0.0];
/// The classic stick's: leaning in at 3.8 a second, standing up at 12 (see
/// `AnalogSteer`), upright at once the other way.
const CLASSIC_STICK_SECONDS: [f32; 3] = [1.0 / 3.8, 1.0 / 12.0, 0.0];
const CLASSIC_TOP_SPEED_LEAN: f32 = 1.0 - (1.0 - 0.8) * 2.0 / 3.0;

impl KeyboardCurves {
    /// The Bézier of curve `kind`.
    pub fn bezier(&self, kind: CurveKind) -> Bezier {
        match kind {
            CurveKind::LeanIn => self.lean_in.curve,
            CurveKind::StandUp => self.stand_up.curve,
            CurveKind::Unwind => self.unwind.curve,
            CurveKind::Speed => self.speed,
            CurveKind::Turn => self.turn,
            CurveKind::TurnSpeed => self.turn_speed,
            CurveKind::Response => self.response,
        }
    }

    /// The share of the turn E gives that is kept going at `speed_share` of
    /// the top speed: F.
    pub fn turn_limit(&self, speed_share: f32) -> f32 {
        1.0 - (1.0 - self.top_speed_turn) * self.turn_speed.ease(speed_share.abs())
    }

    /// The share of its turn the bike makes leaning `lean` at
    /// `speed_share` of its top speed: E for the lean, times F for the
    /// speed.
    pub fn turn_at(&self, lean: f32, speed_share: f32) -> f32 {
        self.turn_for(lean) * self.turn_limit(speed_share)
    }

    /// The lean the stick asks for pushed `travel` past its dead zone (0 to
    /// 1, either way): R, on the side it is pushed to.
    pub fn lean_asked(&self, travel: f32) -> f32 {
        self.response.ease(travel.abs()).copysign(travel) * f32::from(travel != 0.0)
    }

    /// The most the bike leans over (a share of all it can) going at
    /// `speed_share` of its top speed: D.
    pub fn lean_limit(&self, speed_share: f32) -> f32 {
        1.0 - (1.0 - self.top_speed_lean) * self.speed.ease(speed_share.abs())
    }

    /// The share of its turn the bike makes leaning `lean` (a share of all
    /// it can, either way): E, on the side it leans to.
    pub fn turn_for(&self, lean: f32) -> f32 {
        self.turn.ease(lean.abs()).copysign(lean) * f32::from(lean != 0.0)
    }
}

/// The shape chosen for one of the keyboard's curves.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum CurveShape {
    /// The classic keyboard's (see [`CurveShape::fixed`]).
    #[default]
    Classic,
    /// CSS's own easings.
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    /// Its four numbers as set.
    Custom,
    /// As the game has learnt from how the player rides.
    Learned,
}

pub const CURVE_SHAPES: [CurveShape; 8] = [
    CurveShape::Classic,
    CurveShape::Linear,
    CurveShape::Ease,
    CurveShape::EaseIn,
    CurveShape::EaseOut,
    CurveShape::EaseInOut,
    CurveShape::Custom,
    CurveShape::Learned,
];

impl CurveShape {
    /// Its Bézier for curve `kind`, if it has one of its own (not custom
    /// nor learnt). The classic keyboard's lean in and stand up are
    /// polynomials of the time, so their control points stand on the thirds
    /// across (y1 = a/3, y2 = (a + 1)/3 for a t + (1 - a) t²); its limit
    /// with speed, the smoothstep from 15 to 70 m/s, comes close as an
    /// easing with both control points at 0.56 across; it unwinds at once
    /// (any curve) and turns as far as it leans.
    ///
    /// The classic stick's (`stick`) leans in and stands up at a steady
    /// rate (`AnalogSteer`), straight lines, and its response, the travel
    /// past its dead zone to the power of 1.35, comes within 0.006 of
    /// control points on the thirds across at 0.14 and 0.56 up.
    pub fn fixed(self, kind: CurveKind, stick: bool) -> Option<Bezier> {
        let third = 1.0 / 3.0;
        Some(match self {
            CurveShape::Classic => match kind {
                CurveKind::LeanIn | CurveKind::StandUp if stick => Bezier::LINEAR,
                CurveKind::LeanIn => Bezier::new(third, 0.2406, 2.0 * third, 0.5739),
                CurveKind::StandUp => Bezier::new(third, 0.0736, 2.0 * third, 0.4069),
                CurveKind::Speed | CurveKind::TurnSpeed => Bezier::new(0.56, 0.0, 0.56, 1.0),
                CurveKind::Response => Bezier::new(third, 0.14, 2.0 * third, 0.56),
                CurveKind::Unwind | CurveKind::Turn => Bezier::LINEAR,
            },
            CurveShape::Linear => Bezier::LINEAR,
            CurveShape::Ease => Bezier::new(0.25, 0.1, 0.25, 1.0),
            CurveShape::EaseIn => Bezier::new(0.42, 0.0, 1.0, 1.0),
            CurveShape::EaseOut => Bezier::new(0.0, 0.0, 0.58, 1.0),
            CurveShape::EaseInOut => Bezier::new(0.42, 0.0, 0.58, 1.0),
            CurveShape::Custom | CurveShape::Learned => return None,
        })
    }

    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (CurveShape::Classic, Language::Spanish) => "Clásica",
            (CurveShape::Classic, Language::English) => "Classic",
            (CurveShape::Linear, Language::Spanish) => "Lineal",
            (CurveShape::Linear, Language::English) => "Linear",
            (CurveShape::Ease, _) => "Ease",
            (CurveShape::EaseIn, _) => "Ease-in",
            (CurveShape::EaseOut, _) => "Ease-out",
            (CurveShape::EaseInOut, _) => "Ease-in-out",
            (CurveShape::Custom, Language::Spanish) => "Personalizada",
            (CurveShape::Custom, Language::English) => "Custom",
            (CurveShape::Learned, Language::Spanish) => "Aprendida",
            (CurveShape::Learned, Language::English) => "Learned",
        }
    }
}

/// What is chosen for one of the keyboard's curves: its shape, its own four
/// numbers (used when custom) and what has been learnt for it, if anything
/// (used when learnt).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CurveChoice {
    pub shape: CurveShape,
    pub custom: Bezier,
    pub learned: Option<Bezier>,
    /// Its number (time or share at top speed) as learnt, if anything.
    pub learned_number: Option<f32>,
}

impl CurveChoice {
    /// Classic (the keyboard's or the `stick`'s), its custom numbers the
    /// classic ones to start from.
    fn classic(kind: CurveKind, stick: bool) -> Self {
        Self {
            shape: CurveShape::Classic,
            custom: CurveShape::Classic
                .fixed(kind, stick)
                .unwrap_or(Bezier::LINEAR),
            learned: None,
            learned_number: None,
        }
    }
}

impl Default for CurveChoice {
    fn default() -> Self {
        Self::classic(CurveKind::Turn, false)
    }
}

/// Whether the game learns the keyboard's curves from how the player rides,
/// and how far from which line it measures them (see
/// [`super::learning`]): the centre of the road, or a racing line (see
/// [`crate::track::racing_line`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum LearnLine {
    #[default]
    Off,
    Centre,
    Race,
}

pub const LEARN_LINES: [LearnLine; 3] = [LearnLine::Off, LearnLine::Centre, LearnLine::Race];

impl LearnLine {
    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (LearnLine::Off, _) => "Off",
            (LearnLine::Centre, Language::Spanish) => "Centro",
            (LearnLine::Centre, Language::English) => "Centre",
            (LearnLine::Race, Language::Spanish) => "Carrera",
            (LearnLine::Race, Language::English) => "Race",
        }
    }
}

/// How the lean shown follows the lean felt, the one the curves give and
/// the bike turns by: as it is (within a few hundredths of a second), soft
/// (about 80 ms behind) or smooth (about 150 ms, and eased in and out, so
/// that going over to the other side is gentle too). Only what is seen:
/// the turn, the learning and the panel go by the lean felt.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum LeanLook {
    #[default]
    Real,
    Soft,
    Smooth,
}

pub const LEAN_LOOKS: [LeanLook; 3] = [LeanLook::Real, LeanLook::Soft, LeanLook::Smooth];

impl LeanLook {
    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (LeanLook::Real, _) => "Real",
            (LeanLook::Soft, Language::Spanish) => "Suave",
            (LeanLook::Soft, Language::English) => "Soft",
            (LeanLook::Smooth, Language::Spanish) => "Muy suave",
            (LeanLook::Smooth, Language::English) => "Smooth",
        }
    }
}

/// The keyboard's curves as Options > Control > Keyboard behaviour chooses
/// them in Bézier (or the stick's, as Stick behaviour does, `stick`: with
/// R, its response, too), saved with the settings: each curve's shape, the
/// times of A, B and C, the lean D keeps at top speed, and whether the game
/// is learning them all from how the player rides (then every curve is the
/// learnt one, whatever its shape).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyboardShapes {
    pub stick: bool,
    pub response: CurveChoice,
    pub lean_in: CurveChoice,
    pub stand_up: CurveChoice,
    pub unwind: CurveChoice,
    pub speed: CurveChoice,
    pub turn: CurveChoice,
    pub turn_speed: CurveChoice,
    pub lean_in_seconds: f32,
    pub stand_up_seconds: f32,
    pub unwind_seconds: f32,
    pub top_speed_lean: f32,
    /// F's share of the turn kept at top speed (all of it at first).
    pub top_speed_turn: f32,
    /// The line the game learns them from, if it is learning them: then
    /// every curve is the learnt one.
    pub learn: LearnLine,
    /// Whether learning corrects the learnt curves, or only notes where
    /// the bike strays (the key of free ride changes it, see
    /// [`super::learning`]).
    pub correct: bool,
    /// How the lean shown follows the one the curves give, which turns the
    /// bike: as it is, or smoothed (see [`LeanLook`]).
    pub look: LeanLook,
}

impl Default for KeyboardShapes {
    fn default() -> Self {
        Self::classic(false)
    }
}

impl KeyboardShapes {
    /// The classic curves of the keyboard, or of the `stick`.
    pub fn classic(stick: bool) -> Self {
        let classic = |kind| CurveChoice::classic(kind, stick);
        let [lean_in_seconds, stand_up_seconds, unwind_seconds] = if stick {
            CLASSIC_STICK_SECONDS
        } else {
            CLASSIC_SECONDS
        };
        Self {
            stick,
            response: classic(CurveKind::Response),
            lean_in: classic(CurveKind::LeanIn),
            stand_up: classic(CurveKind::StandUp),
            unwind: classic(CurveKind::Unwind),
            speed: classic(CurveKind::Speed),
            turn: classic(CurveKind::Turn),
            turn_speed: classic(CurveKind::TurnSpeed),
            lean_in_seconds,
            stand_up_seconds,
            unwind_seconds,
            top_speed_lean: CLASSIC_TOP_SPEED_LEAN,
            top_speed_turn: 1.0,
            learn: LearnLine::Off,
            correct: false,
            look: LeanLook::Real,
        }
    }

    /// Its curves, in the order the options and the panel show them: the
    /// stick's response first.
    pub fn kinds(&self) -> &'static [CurveKind] {
        if self.stick {
            &STICK_CURVE_KINDS
        } else {
            &CURVE_KINDS
        }
    }

    pub fn choice(&self, kind: CurveKind) -> &CurveChoice {
        match kind {
            CurveKind::Response => &self.response,
            CurveKind::LeanIn => &self.lean_in,
            CurveKind::StandUp => &self.stand_up,
            CurveKind::Unwind => &self.unwind,
            CurveKind::Speed => &self.speed,
            CurveKind::Turn => &self.turn,
            CurveKind::TurnSpeed => &self.turn_speed,
        }
    }

    pub fn choice_mut(&mut self, kind: CurveKind) -> &mut CurveChoice {
        match kind {
            CurveKind::Response => &mut self.response,
            CurveKind::LeanIn => &mut self.lean_in,
            CurveKind::StandUp => &mut self.stand_up,
            CurveKind::Unwind => &mut self.unwind,
            CurveKind::Speed => &mut self.speed,
            CurveKind::Turn => &mut self.turn,
            CurveKind::TurnSpeed => &mut self.turn_speed,
        }
    }

    /// The shape curve `kind` has now: learnt for all of them while the
    /// game learns them, the one chosen otherwise.
    pub fn shape(&self, kind: CurveKind) -> CurveShape {
        if self.learning() {
            CurveShape::Learned
        } else {
            self.choice(kind).shape
        }
    }

    /// The Bézier of curve `kind` now: its shape's own, its custom numbers,
    /// or what has been learnt (until anything has, its custom numbers).
    pub fn bezier(&self, kind: CurveKind) -> Bezier {
        let choice = self.choice(kind);
        match self.shape(kind) {
            CurveShape::Custom => choice.custom,
            CurveShape::Learned => choice.learned.unwrap_or(choice.custom),
            shape => shape.fixed(kind, self.stick).unwrap_or(Bezier::LINEAR),
        }
    }

    /// Curve `kind` gets the shape `step` along. Learnt for the first time,
    /// it starts from the curve it had.
    pub fn step_shape(&mut self, kind: CurveKind, step: i32) {
        let was = self.bezier(kind);
        let choice = self.choice_mut(kind);
        choice.shape = crate::settings::cycle(&CURVE_SHAPES, choice.shape, step);
        if choice.shape == CurveShape::Learned && choice.learned.is_none() {
            choice.learned = Some(was);
        }
    }

    /// Whether the game is learning the curves.
    pub fn learning(&self) -> bool {
        self.learn != LearnLine::Off
    }

    /// The game learning every curve from `line`, or not: on, each curve
    /// not learnt yet starts from the one it has; off, each goes back to
    /// its shape.
    pub fn set_learn(&mut self, line: LearnLine) {
        if line != LearnLine::Off && !self.learning() {
            for &kind in self.kinds() {
                let (was, number) = (self.bezier(kind), self.own_scale(kind));
                let choice = self.choice_mut(kind);
                choice.learned.get_or_insert(was);
                if let Some(number) = number {
                    choice.learned_number.get_or_insert(number);
                }
            }
        }
        self.learn = line;
    }

    /// Corrects the learnt curve `kind` a little for the bike having gone
    /// `wide` (or inside) on it at `speed_share` of its top speed, and its
    /// learnt number with it: A and C quicker (rising sooner, and shorter),
    /// R asking for more lean, E more turn for the lean, D keeping more lean
    /// and F more turn (and, near top speed, more of each at top speed),
    /// for going wide; E less turn for the lean for cutting inside; B
    /// quicker (and shorter) for standing up too slowly (inside), slower
    /// (and longer) for too quickly (wide). The curve only rises or falls,
    /// both control points together, within the square; E never comes down
    /// below [`TURN_FLOOR`], so a touch of the key always turns the bike;
    /// the numbers stay within the options' bounds, and C at once stays so.
    pub fn correct_for(&mut self, kind: CurveKind, wide: bool, speed_share: f32) {
        let up = match (kind, wide) {
            (CurveKind::LeanIn | CurveKind::Unwind | CurveKind::Response, true) => 1.0,
            (CurveKind::Speed | CurveKind::TurnSpeed, true) => -1.0,
            (CurveKind::Turn, false) => -1.0,
            (CurveKind::Turn, true) => 1.0,
            (CurveKind::StandUp, false) => 1.0,
            (CurveKind::StandUp, true) => -1.0,
            _ => return,
        };
        let choice = self.choice_mut(kind);
        let Some(curve) = choice.learned.as_mut() else {
            return;
        };
        let floor = if kind == CurveKind::Turn {
            TURN_FLOOR
        } else {
            (0.0, 0.0)
        };
        let moved = |value: f32, floor: f32| {
            ((value + up * NUDGE).clamp(floor, 1.0) * 1000.0).round() / 1000.0
        };
        curve.y1 = moved(curve.y1, floor.0);
        curve.y2 = moved(curve.y2, floor.1);
        // Its number: a time shorter for quicker, longer for slower; a
        // share at top speed higher for more, near top speed only.
        let Some(number) = choice.learned_number.as_mut() else {
            return;
        };
        let rounded = |value: f32| (value * 1000.0).round() / 1000.0;
        match kind {
            CurveKind::LeanIn | CurveKind::StandUp | CurveKind::Unwind if *number > 0.0 => {
                *number = rounded((*number * (1.0 - up * NUDGE_TIME)).clamp(SECONDS.0, SECONDS.1));
            }
            CurveKind::Speed | CurveKind::TurnSpeed if speed_share >= NEAR_TOP_SPEED => {
                *number = rounded((*number + NUDGE_TOP).min(1.0));
            }
            _ => {}
        }
    }

    /// The number curve `kind` is scaled by, as set, if it has one: the
    /// seconds of A, B and C, the lean kept at top speed of D, the turn
    /// kept at top speed of F.
    pub fn own_scale(&self, kind: CurveKind) -> Option<f32> {
        match kind {
            CurveKind::LeanIn => Some(self.lean_in_seconds),
            CurveKind::StandUp => Some(self.stand_up_seconds),
            CurveKind::Unwind => Some(self.unwind_seconds),
            CurveKind::Speed => Some(self.top_speed_lean),
            CurveKind::TurnSpeed => Some(self.top_speed_turn),
            CurveKind::Turn | CurveKind::Response => None,
        }
    }

    /// The number curve `kind` is scaled by now: as learnt while the game
    /// learns them (until anything is, as set), as set otherwise.
    pub fn scale(&self, kind: CurveKind) -> Option<f32> {
        let own = self.own_scale(kind)?;
        Some(if self.learning() {
            self.choice(kind).learned_number.unwrap_or(own)
        } else {
            own
        })
    }

    /// Steps the number curve `kind` is scaled by (`step` steps of 0.05),
    /// kept within its bounds. C may come down to nought: at once.
    pub fn step_scale(&mut self, kind: CurveKind, step: i32) {
        let stepped = |value: f32, (low, high): (f32, f32)| {
            ((value + 0.05 * step as f32).clamp(low, high) * 1000.0).round() / 1000.0
        };
        match kind {
            CurveKind::LeanIn => self.lean_in_seconds = stepped(self.lean_in_seconds, SECONDS),
            CurveKind::StandUp => {
                self.stand_up_seconds = stepped(self.stand_up_seconds, SECONDS);
            }
            CurveKind::Unwind => {
                // From the least time down to nought, and back up.
                let next = stepped(self.unwind_seconds, (0.0, SECONDS.1));
                self.unwind_seconds = if next > 0.0 && next < SECONDS.0 {
                    if step < 0 { 0.0 } else { SECONDS.0 }
                } else {
                    next
                };
            }
            CurveKind::Speed => {
                self.top_speed_lean = stepped(self.top_speed_lean, TOP_SPEED_LEAN);
            }
            CurveKind::TurnSpeed => {
                self.top_speed_turn = stepped(self.top_speed_turn, TOP_SPEED_TURN);
            }
            CurveKind::Turn | CurveKind::Response => {}
        }
    }

    /// The curves the bike follows.
    pub fn curves(&self) -> KeyboardCurves {
        let number = |kind| self.scale(kind).unwrap_or_default();
        let timed = |kind| TimedCurve {
            curve: self.bezier(kind),
            seconds: number(kind),
        };
        KeyboardCurves {
            lean_in: timed(CurveKind::LeanIn),
            stand_up: timed(CurveKind::StandUp),
            unwind: timed(CurveKind::Unwind),
            speed: self.bezier(CurveKind::Speed),
            top_speed_lean: number(CurveKind::Speed),
            turn: self.bezier(CurveKind::Turn),
            turn_speed: self.bezier(CurveKind::TurnSpeed),
            top_speed_turn: number(CurveKind::TurnSpeed),
            look: self.look,
            response: self.bezier(CurveKind::Response),
        }
    }
}

/// Each of the keyboard's curves.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CurveKind {
    #[default]
    LeanIn,
    StandUp,
    Unwind,
    Speed,
    Turn,
    /// The stick's only: the lean asked for how far it is pushed.
    Response,
    /// F: the turn kept for the speed.
    TurnSpeed,
}

pub const CURVE_KINDS: [CurveKind; 6] = [
    CurveKind::LeanIn,
    CurveKind::StandUp,
    CurveKind::Unwind,
    CurveKind::Speed,
    CurveKind::Turn,
    CurveKind::TurnSpeed,
];

/// The stick's curves: its response, then the keyboard's.
pub const STICK_CURVE_KINDS: [CurveKind; 7] = [
    CurveKind::Response,
    CurveKind::LeanIn,
    CurveKind::StandUp,
    CurveKind::Unwind,
    CurveKind::Speed,
    CurveKind::Turn,
    CurveKind::TurnSpeed,
];

impl CurveKind {
    /// Its letter, as the panel of the curves and the options call it.
    pub fn letter(self) -> char {
        match self {
            CurveKind::LeanIn => 'A',
            CurveKind::StandUp => 'B',
            CurveKind::Unwind => 'C',
            CurveKind::Speed => 'D',
            CurveKind::Turn => 'E',
            CurveKind::Response => 'R',
            CurveKind::TurnSpeed => 'F',
        }
    }

    pub fn name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (CurveKind::LeanIn, Language::Spanish) => "Tumbada al pulsar",
            (CurveKind::LeanIn, Language::English) => "Leaning in",
            (CurveKind::StandUp, Language::Spanish) => "Levantada al soltar",
            (CurveKind::StandUp, Language::English) => "Standing up",
            (CurveKind::Unwind, Language::Spanish) => "Cambio de lado",
            (CurveKind::Unwind, Language::English) => "Other way",
            (CurveKind::Speed, Language::Spanish) => "Tumbada según velocidad",
            (CurveKind::Speed, Language::English) => "Lean for speed",
            (CurveKind::Turn, Language::Spanish) => "Giro según tumbada",
            (CurveKind::Turn, Language::English) => "Turn for lean",
            (CurveKind::TurnSpeed, Language::Spanish) => "Giro según velocidad",
            (CurveKind::TurnSpeed, Language::English) => "Turn for speed",
            (CurveKind::Response, Language::Spanish) => "Respuesta del stick",
            (CurveKind::Response, Language::English) => "Stick response",
        }
    }

    /// Its name in few words, for the panel of the curves.
    pub fn short_name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (CurveKind::LeanIn, Language::Spanish) => "Tumbada",
            (CurveKind::LeanIn, Language::English) => "Lean in",
            (CurveKind::StandUp, Language::Spanish) => "Levantada",
            (CurveKind::StandUp, Language::English) => "Stand up",
            (CurveKind::Unwind, Language::Spanish) => "Cambio",
            (CurveKind::Unwind, Language::English) => "Other way",
            (CurveKind::Speed, Language::Spanish) => "Velocidad",
            (CurveKind::Speed, Language::English) => "Speed",
            (CurveKind::Turn, Language::Spanish) => "Giro",
            (CurveKind::Turn, Language::English) => "Turn",
            (CurveKind::TurnSpeed, Language::Spanish) => "Giro vel.",
            (CurveKind::TurnSpeed, Language::English) => "Turn speed",
            (CurveKind::Response, Language::Spanish) => "Respuesta",
            (CurveKind::Response, Language::English) => "Response",
        }
    }

    /// What its control points' coordinates measure, as the options name
    /// them: across and up.
    pub fn axes(self, lang: Language) -> (&'static str, &'static str) {
        match (self, lang) {
            (CurveKind::LeanIn, Language::Spanish) => ("tiempo", "tumbada"),
            (CurveKind::LeanIn, Language::English) => ("time", "lean"),
            (CurveKind::StandUp | CurveKind::Unwind, Language::Spanish) => ("tiempo", "levantada"),
            (CurveKind::StandUp | CurveKind::Unwind, Language::English) => ("time", "rise"),
            (CurveKind::Speed, Language::Spanish) => ("velocidad", "reducción"),
            (CurveKind::Speed, Language::English) => ("speed", "reduction"),
            (CurveKind::Turn, Language::Spanish) => ("tumbada", "giro"),
            (CurveKind::Turn, Language::English) => ("lean", "turn"),
            (CurveKind::TurnSpeed, Language::Spanish) => ("velocidad", "recorte"),
            (CurveKind::TurnSpeed, Language::English) => ("speed", "reduction"),
            (CurveKind::Response, Language::Spanish) => ("stick", "tumbada"),
            (CurveKind::Response, Language::English) => ("stick", "lean"),
        }
    }

    /// What its number is, as the options name it.
    pub fn scale_name(self, lang: Language) -> &'static str {
        match (self, lang) {
            (CurveKind::Speed, Language::Spanish) => "Tumbada a toda velocidad",
            (CurveKind::Speed, Language::English) => "Lean at top speed",
            (CurveKind::TurnSpeed, Language::Spanish) => "Giro a toda velocidad",
            (CurveKind::TurnSpeed, Language::English) => "Turn at top speed",
            (_, Language::Spanish) => "Duración",
            (_, Language::English) => "Duration",
        }
    }

    /// Its number as it is shown: seconds, at once (C at nought), or the
    /// share of the lean.
    pub fn scale_text(self, value: f32, lang: Language) -> String {
        match self {
            CurveKind::Speed | CurveKind::TurnSpeed => format!("{:.0} %", value * 100.0),
            CurveKind::Unwind if value <= 0.0 => match lang {
                Language::Spanish => "al momento".to_string(),
                Language::English => "at once".to_string(),
            },
            _ => format!("{value:.2} s"),
        }
    }
}

/// Which of the keyboard's curves the lean is following.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    /// Upright with no key held.
    #[default]
    Upright,
    /// Leaning in or held over (A, up to D's limit).
    LeaningIn,
    /// Standing back up with no key held (B).
    StandingUp,
    /// Coming back up holding the other way (C).
    Unwinding,
}

/// The lean the keyboard or the d-pad asks for with the curves: how far
/// over (a share of all the bike can, positive to the left), which curve it
/// is following and how far through it (0 to 1), and the way held then.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct CurveLean {
    pub lean: f32,
    pub phase: Phase,
    pub progress: f32,
    held: f32,
}

impl CurveLean {
    /// Goes on `dt` seconds asking for the lean `target` (a share of all
    /// the bike can, positive to the left: -1, 0 or 1 from the keys, any
    /// from the stick) at `speed_share` of the top speed. Short of it on
    /// its side, the lean leans in as A says, up to it or D's limit; past
    /// it (or with nothing asked), stands back up to it as B says; on the
    /// other side, comes upright first as C says. Each curve is followed
    /// from wherever the lean is (the first point of it at that height),
    /// so going from one to another never jumps. Returns the lean.
    pub fn update(
        &mut self,
        target: f32,
        curves: &KeyboardCurves,
        speed_share: f32,
        dt: f32,
    ) -> f32 {
        let target = target.clamp(-1.0, 1.0);
        let asked = target.abs();
        let side = if asked < 1e-4 { 0.0 } else { target.signum() };
        let limit = curves.lean_limit(speed_share);
        let mut dt = dt.max(0.0);
        // At most twice round: unwinding may end within the step and lean
        // in with what is left of it.
        for _ in 0..2 {
            let over = self.lean.abs();
            let phase = if side == 0.0 {
                if over > 0.0 {
                    Phase::StandingUp
                } else {
                    Phase::Upright
                }
            } else if self.lean * side < 0.0 {
                Phase::Unwinding
            } else if over > asked + 1e-4 {
                Phase::StandingUp
            } else {
                Phase::LeaningIn
            };
            let curve = match phase {
                Phase::Upright => {
                    *self = Self::default();
                    return 0.0;
                }
                Phase::LeaningIn => curves.lean_in,
                Phase::StandingUp => curves.stand_up,
                Phase::Unwinding => curves.unwind,
            };
            // Where on the curve the lean is: leaning in, how far over;
            // coming back up, how much of the way back.
            let height = |lean: f32| match phase {
                Phase::LeaningIn => lean,
                _ => 1.0 - lean,
            };
            if phase != self.phase || side != self.held {
                (self.phase, self.held) = (phase, side);
                self.progress = curve.curve.first_reaching(height(over));
            }
            if phase == Phase::Unwinding && curve.seconds <= 0.0 {
                // Upright at once, and on into the turn.
                *self = Self::default();
                continue;
            }
            let duration = curve.seconds.max(1e-3);
            let to_end = (1.0 - self.progress) * duration;
            let spent = dt.min(to_end);
            self.progress = (self.progress + spent / duration).min(1.0);
            let shape = curve.curve.ease(self.progress);
            match phase {
                Phase::LeaningIn => {
                    let most = asked.min(limit);
                    if shape > most {
                        // Held where it is asked (or the limit is), to go on
                        // from there if more is asked or the limit rises.
                        self.progress = curve.curve.first_reaching(most);
                    }
                    self.lean = shape.min(most) * side;
                    return self.lean;
                }
                Phase::StandingUp => {
                    // Down to what is asked on this side, if anything.
                    let floor = if self.lean * side > 0.0 { asked } else { 0.0 };
                    self.lean = (1.0 - shape).min(over).max(floor).copysign(self.lean);
                    if self.progress >= 1.0 && floor == 0.0 {
                        *self = Self::default();
                    }
                    return self.lean;
                }
                Phase::Unwinding => {
                    self.lean = ((1.0 - shape).min(over)).copysign(self.lean);
                    if self.progress < 1.0 {
                        return self.lean;
                    }
                    // Upright: the rest of the step leans in.
                    *self = Self::default();
                    dt -= spent;
                }
                Phase::Upright => unreachable!(),
            }
        }
        self.lean
    }
}

/// How the first player's lean is going on the keyboard's curves, for the
/// panel of the curves (see [`crate::curves_hud`]): which curve and how far
/// through it, how far over, how fast the bike goes (a share of its top
/// speed), D's limit there and the turn E gives.
#[derive(Resource, Clone, Copy, Default, Debug, PartialEq)]
pub struct CurveProbe {
    /// Whether the curves are steering a player's bike now, and whether
    /// the stick's (with how far it is pushed past its dead zone and the
    /// lean that asks for) or the keyboard's.
    pub live: bool,
    pub stick: bool,
    pub travel: f32,
    pub asked: f32,
    /// The share of its turn the lean asked would give (E of it, before D's
    /// limit): the turn the player wants.
    pub wanted: f32,
    pub lean: CurveLean,
    pub speed_share: f32,
    pub limit: f32,
    pub turn: f32,
    /// F's share of the turn kept at that speed.
    pub turn_limit: f32,
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 120.0;

    /// The lean, step by step, holding `pressed(t)` at `speed_share`.
    fn run(
        curves: &KeyboardCurves,
        speed_share: f32,
        seconds: f32,
        pressed: impl Fn(f32) -> f32,
    ) -> Vec<f32> {
        let mut lean = CurveLean::default();
        (0..(seconds / DT).round() as usize)
            .map(|frame| lean.update(pressed(frame as f32 * DT), curves, speed_share, DT))
            .collect()
    }

    fn at(values: &[f32], seconds: f32) -> f32 {
        values[(seconds / DT).round() as usize - 1]
    }

    #[test]
    fn every_curve_starts_classic_and_takes_each_shape_in_turn() {
        let mut shapes = KeyboardShapes::default();
        for kind in CURVE_KINDS {
            assert_eq!(shapes.shape(kind), CurveShape::Classic);
            // Its custom numbers start from the classic curve.
            assert_eq!(shapes.choice(kind).custom, shapes.bezier(kind));
        }
        shapes.step_shape(CurveKind::Turn, 1);
        assert_eq!(shapes.shape(CurveKind::Turn), CurveShape::Linear);
        shapes.step_shape(CurveKind::Turn, 1);
        assert_eq!(
            shapes.bezier(CurveKind::Turn),
            Bezier::new(0.25, 0.1, 0.25, 1.0)
        );
        // Custom: its own numbers.
        shapes.turn.custom = Bezier::new(0.1, 0.6, 0.5, 1.0);
        shapes.turn.shape = CurveShape::Custom;
        assert_eq!(shapes.bezier(CurveKind::Turn), shapes.turn.custom);
        assert_eq!(shapes.curves().turn, shapes.turn.custom);
        // Back round from the first to the last: learnt.
        shapes.step_shape(CurveKind::LeanIn, -1);
        assert_eq!(shapes.shape(CurveKind::LeanIn), CurveShape::Learned);
    }

    #[test]
    fn a_learnt_curve_starts_from_the_one_it_had_and_learning_makes_them_all_learnt() {
        let mut shapes = KeyboardShapes::default();
        shapes.speed.shape = CurveShape::EaseIn;
        let ease_in = shapes.bezier(CurveKind::Speed);
        // Chosen learnt, it starts from the curve it had.
        shapes.step_shape(CurveKind::Speed, 4);
        assert_eq!(shapes.shape(CurveKind::Speed), CurveShape::Learned);
        assert_eq!(shapes.bezier(CurveKind::Speed), ease_in);
        shapes.speed.shape = CurveShape::Linear;
        // Learning: every curve learnt, each from what it had.
        let before = shapes.curves();
        shapes.set_learn(LearnLine::Centre);
        for kind in CURVE_KINDS {
            assert_eq!(shapes.shape(kind), CurveShape::Learned);
        }
        assert_eq!(shapes.bezier(CurveKind::LeanIn), before.lean_in.curve);
        // D was learnt before: it keeps what it had learnt.
        assert_eq!(shapes.bezier(CurveKind::Speed), ease_in);
        // What is learnt changes the curve followed.
        shapes.lean_in.learned = Some(Bezier::LINEAR);
        assert_eq!(shapes.curves().lean_in.curve, Bezier::LINEAR);
        // Off, each back to its shape.
        shapes.set_learn(LearnLine::Off);
        assert_eq!(shapes.shape(CurveKind::Speed), CurveShape::Linear);
        assert_eq!(shapes.curves(), before);
    }

    #[test]
    fn corrections_move_a_learnt_curve_the_way_it_strayed_within_bounds() {
        let mut shapes = KeyboardShapes::default();
        // Nothing learnt yet: nothing to correct.
        shapes.correct_for(CurveKind::LeanIn, true, 0.5);
        assert_eq!(shapes, KeyboardShapes::default());
        shapes.set_learn(LearnLine::Centre);
        let was = shapes.bezier(CurveKind::LeanIn);
        shapes.correct_for(CurveKind::LeanIn, true, 0.5);
        let now = shapes.bezier(CurveKind::LeanIn);
        assert!((now.y1 - was.y1 - NUDGE).abs() < 1e-3 && (now.y2 - was.y2 - NUDGE).abs() < 1e-3);
        assert_eq!((now.x1, now.x2), (was.x1, was.x2));
        // D: more lean (less reduction) for going wide.
        let d = shapes.bezier(CurveKind::Speed);
        shapes.correct_for(CurveKind::Speed, true, 0.5);
        assert!(shapes.bezier(CurveKind::Speed).y2 < d.y2);
        // E: less turn for cutting inside, never below its floor.
        for _ in 0..200 {
            shapes.correct_for(CurveKind::Turn, false, 0.5);
        }
        let e = shapes.bezier(CurveKind::Turn);
        assert_eq!((e.y1, e.y2), TURN_FLOOR);
        // E: more turn for going wide.
        shapes.correct_for(CurveKind::Turn, true, 0.5);
        assert!(shapes.bezier(CurveKind::Turn).y2 > TURN_FLOOR.1);
        // What does not point at a curve changes nothing.
        let before = shapes;
        shapes.correct_for(CurveKind::LeanIn, false, 0.5);
        shapes.correct_for(CurveKind::Speed, false, 0.5);
        assert_eq!(shapes, before);
    }

    #[test]
    fn the_stick_has_its_response_first_and_its_classic_curves_are_the_stick_s() {
        let (keys, stick) = (KeyboardShapes::default(), KeyboardShapes::classic(true));
        assert_eq!(keys.kinds(), CURVE_KINDS);
        assert_eq!(stick.kinds(), STICK_CURVE_KINDS);
        // Its classic response: as the stick's, the travel to the power of
        // 1.35, within a hundredth.
        let curves = stick.curves();
        for k in 0..=20 {
            let travel = k as f32 / 20.0;
            let asked = curves.lean_asked(travel);
            assert!(
                (asked - travel.powf(1.35)).abs() < 0.01,
                "{travel}: {asked}"
            );
            assert_eq!(curves.lean_asked(-travel), -asked);
        }
        // Leaning in at the stick's 3.8 a second, straight.
        let values = run(&curves, 0.0, 0.2, |_| 1.0);
        assert!(
            (at(&values, 0.1) - 0.38).abs() < 0.01,
            "{}",
            at(&values, 0.1)
        );
        assert!((at(&values, 0.2) - 0.76).abs() < 0.01);
    }

    #[test]
    fn with_the_stick_the_lean_goes_to_what_it_asks_and_back_to_less() {
        let curves = KeyboardShapes::classic(true).curves();
        // Half way over asked: half way over, held there.
        let values = run(&curves, 0.0, 2.0, |t| {
            if t < 0.6 {
                0.5
            } else if t < 1.2 {
                0.2
            } else {
                0.0
            }
        });
        assert!((at(&values, 0.6) - 0.5).abs() < 1e-3);
        // Less asked: back up to it, not upright.
        assert!((at(&values, 1.2) - 0.2).abs() < 1e-3);
        assert!(values[72..144].iter().all(|&v| v >= 0.2 - 1e-4));
        // Nothing: upright.
        assert_eq!(at(&values, 2.0), 0.0);
        // The other way: through upright, over to the right.
        let flip = run(&curves, 0.0, 1.0, |t| if t < 0.5 { 0.6 } else { -0.6 });
        assert!((at(&flip, 1.0) + 0.6).abs() < 1e-3);
    }

    #[test]
    fn f_keeps_a_share_of_the_turn_for_the_speed_and_e_and_f_combine() {
        let mut shapes = KeyboardShapes::default();
        // Classic: all the turn at any speed.
        let curves = shapes.curves();
        for share in [0.0, 0.5, 1.0] {
            assert_eq!(curves.turn_limit(share), 1.0);
        }
        shapes.top_speed_turn = 0.7;
        let curves = shapes.curves();
        assert_eq!(curves.turn_limit(0.0), 1.0);
        assert!((curves.turn_limit(1.0) - 0.7).abs() < 1e-5);
        // E for the lean times F for the speed, on the lean's side.
        let (lean, share) = (0.6, 1.0);
        let turn = curves.turn_at(-lean, share);
        assert!((turn + curves.turn_for(lean) * 0.7).abs() < 1e-5);
        // F in both sets, last.
        assert_eq!(*shapes.kinds().last().unwrap(), CurveKind::TurnSpeed);
        assert_eq!(
            *KeyboardShapes::classic(true).kinds().last().unwrap(),
            CurveKind::TurnSpeed
        );
    }

    #[test]
    fn learning_learns_the_numbers_too_apart_from_the_ones_set() {
        let mut shapes = KeyboardShapes::default();
        let own = shapes.own_scale(CurveKind::LeanIn).unwrap();
        shapes.set_learn(LearnLine::Race);
        // A wide: quicker, so shorter.
        shapes.correct_for(CurveKind::LeanIn, true, 0.5);
        let learnt = shapes.scale(CurveKind::LeanIn).unwrap();
        assert!((learnt - own * (1.0 - NUDGE_TIME)).abs() < 1e-3);
        assert_eq!(shapes.curves().lean_in.seconds, learnt);
        // B wide: slower, so longer.
        let b = shapes.scale(CurveKind::StandUp).unwrap();
        shapes.correct_for(CurveKind::StandUp, true, 0.5);
        assert!(shapes.scale(CurveKind::StandUp).unwrap() > b);
        // C at once stays at once.
        shapes.correct_for(CurveKind::Unwind, true, 0.5);
        assert_eq!(shapes.scale(CurveKind::Unwind), Some(0.0));
        // D: more lean at top speed, only near it.
        let d = shapes.scale(CurveKind::Speed).unwrap();
        shapes.correct_for(CurveKind::Speed, true, 0.3);
        assert_eq!(shapes.scale(CurveKind::Speed).unwrap(), d);
        shapes.correct_for(CurveKind::Speed, true, 0.9);
        assert!((shapes.scale(CurveKind::Speed).unwrap() - (d + NUDGE_TOP)).abs() < 1e-3);
        // F at all of it: no further.
        shapes.correct_for(CurveKind::TurnSpeed, true, 0.9);
        assert_eq!(shapes.scale(CurveKind::TurnSpeed), Some(1.0));
        // Never past the bounds.
        for _ in 0..200 {
            shapes.correct_for(CurveKind::LeanIn, true, 0.5);
        }
        assert_eq!(shapes.scale(CurveKind::LeanIn), Some(SECONDS.0));
        // Off: the numbers as set; learning again, the learnt ones.
        shapes.set_learn(LearnLine::Off);
        assert_eq!(shapes.scale(CurveKind::LeanIn), Some(own));
        shapes.set_learn(LearnLine::Centre);
        assert_eq!(shapes.scale(CurveKind::LeanIn), Some(SECONDS.0));
    }

    #[test]
    fn holding_a_key_leans_over_along_a_in_its_time() {
        let curves = KeyboardCurves::default();
        let values = run(&curves, 0.0, 1.0, |_| 1.0);
        // Moving from the first step, all the way over by A's time.
        assert!(values[0] > 0.0);
        assert!(values.windows(2).all(|w| w[1] >= w[0]));
        assert!((at(&values, 0.39) - 1.0).abs() < 1e-3);
        // Faster and faster, as the classic keyboard.
        assert!(at(&values, 0.2) - at(&values, 0.1) > at(&values, 0.1));
        // The other way, the same to the right.
        let right = run(&curves, 0.0, 1.0, |_| -1.0);
        assert_eq!(right[20], -values[20]);
    }

    #[test]
    fn the_classic_preset_leans_as_the_classic_keyboard_steers() {
        // DigitalSteer at medium: 1.875 t + 1.875 t² held; from all the way
        // over, 0.25 t + t² back once let go.
        let curves = KeyboardCurves::default();
        let held = run(&curves, 0.0, 0.3, |_| 1.0);
        for seconds in [0.05, 0.1, 0.2, 0.3] {
            let classic = 1.875 * seconds + 1.875 * seconds * seconds;
            assert!(
                (at(&held, seconds) - classic).abs() < 0.02,
                "{seconds}: {}",
                at(&held, seconds)
            );
        }
        let released = run(&curves, 0.0, 1.5, |t| if t < 0.5 { 1.0 } else { 0.0 });
        for after in [0.2, 0.4, 0.6] {
            let classic = 1.0 - (0.25 * after + after * after);
            assert!(
                (at(&released, 0.5 + after) - classic).abs() < 0.02,
                "{after}: {}",
                at(&released, 0.5 + after)
            );
        }
        assert_eq!(at(&released, 1.5), 0.0);
    }

    #[test]
    fn the_lean_is_limited_at_speed_as_d_says() {
        let curves = KeyboardCurves::default();
        assert!((curves.lean_limit(0.0) - 1.0).abs() < 1e-6);
        let top = curves.lean_limit(1.0);
        assert!((top - curves.top_speed_lean).abs() < 1e-5);
        assert!(curves.lean_limit(0.5) < 1.0 && curves.lean_limit(0.5) > top);
        let flat_out = run(&curves, 1.0, 1.0, |_| 1.0);
        assert!((at(&flat_out, 1.0) - top).abs() < 1e-5);
        // As fast to answer, whatever the speed.
        let parked = run(&curves, 0.0, 1.0, |_| 1.0);
        assert_eq!(at(&flat_out, 0.1), at(&parked, 0.1));
    }

    #[test]
    fn the_limit_rising_lets_the_lean_go_on_without_a_jump() {
        let curves = KeyboardCurves::default();
        let mut lean = CurveLean::default();
        for _ in 0..120 {
            lean.update(1.0, &curves, 1.0, DT);
        }
        let held = lean.lean;
        // Slowing down: the limit opens and the lean goes on from there.
        let next = lean.update(1.0, &curves, 0.0, DT);
        assert!(next > held && next - held < 0.05, "{held} -> {next}");
    }

    #[test]
    fn letting_go_halfway_comes_back_from_where_it_was() {
        let curves = KeyboardCurves::default();
        let values = run(&curves, 0.0, 1.5, |t| if t < 0.15 { 1.0 } else { 0.0 });
        let let_go = at(&values, 0.15);
        let after = at(&values, 0.15 + DT);
        assert!(
            after <= let_go && let_go - after < 0.02,
            "{let_go} -> {after}"
        );
        assert!(values.iter().skip(18).all(|&v| v <= let_go));
        assert_eq!(*values.last().unwrap(), 0.0);
    }

    #[test]
    fn turning_the_other_way_comes_upright_as_c_says() {
        let flip = |t: f32| if t < 0.5 { 1.0 } else { -1.0 };
        // Classic: upright at once and over the other way as A says.
        let curves = KeyboardCurves::default();
        let values = run(&curves, 0.0, 1.0, flip);
        let after = at(&values, 0.5 + DT);
        assert!(after < 0.0 && after > -0.05, "{after}");
        // Over a time: still leaning left a moment later, right after it.
        let mut slow = curves;
        slow.unwind.seconds = 0.2;
        let values = run(&slow, 0.0, 1.5, flip);
        assert!(at(&values, 0.6) > 0.0);
        assert!(at(&values, 0.75) < 0.0);
        assert!(values.windows(2).skip(60).all(|w| w[1] <= w[0]));
        assert!((at(&values, 1.5) + 1.0).abs() < 1e-3);
    }

    #[test]
    fn the_turn_follows_the_lean_through_e() {
        let mut curves = KeyboardCurves::default();
        assert_eq!(curves.turn_for(0.0), 0.0);
        assert!((curves.turn_for(0.5) - 0.5).abs() < 1e-5);
        assert!((curves.turn_for(-0.5) + 0.5).abs() < 1e-5);
        curves.turn = Bezier::new(0.1, 0.6, 0.5, 1.0);
        assert!(curves.turn_for(0.2) > 0.4);
    }

    #[test]
    fn the_same_ride_at_any_frame_rate() {
        // Smooth: easing in and out, unwinding over a moment.
        let mut shapes = KeyboardShapes {
            lean_in_seconds: 0.5,
            stand_up_seconds: 0.6,
            unwind_seconds: 0.15,
            ..default()
        };
        shapes.lean_in.shape = CurveShape::EaseInOut;
        shapes.stand_up.shape = CurveShape::EaseOut;
        shapes.unwind.shape = CurveShape::Ease;
        let curves = shapes.curves();
        let ride = |dt: f32| {
            let mut lean = CurveLean::default();
            for frame in 0..(1.1 / dt).round() as usize {
                // In the middle of the step, clear of where the keys change.
                let t = (frame as f32 + 0.5) * dt;
                let pressed = if t < 0.4 {
                    1.0
                } else if t < 0.7 {
                    -1.0
                } else {
                    0.0
                };
                lean.update(pressed, &curves, 0.3, dt);
            }
            lean.lean
        };
        let (fine, coarse) = (ride(1.0 / 240.0), ride(1.0 / 40.0));
        assert!((fine - coarse).abs() < 0.05, "{fine} vs {coarse}");
    }

    #[test]
    fn the_scales_step_within_their_bounds_and_c_reaches_at_once() {
        let mut curves = KeyboardShapes::default();
        for _ in 0..100 {
            curves.step_scale(CurveKind::LeanIn, 1);
        }
        assert_eq!(curves.lean_in_seconds, SECONDS.1);
        for _ in 0..100 {
            curves.step_scale(CurveKind::Speed, -1);
        }
        assert_eq!(curves.top_speed_lean, TOP_SPEED_LEAN.0);
        // C: from nought up to the least time, and back down to nought.
        assert_eq!(curves.unwind_seconds, 0.0);
        curves.step_scale(CurveKind::Unwind, 1);
        assert_eq!(curves.unwind_seconds, SECONDS.0);
        curves.step_scale(CurveKind::Unwind, -1);
        assert_eq!(curves.unwind_seconds, 0.0);
        curves.step_scale(CurveKind::Unwind, -1);
        assert_eq!(curves.unwind_seconds, 0.0);
    }
}
