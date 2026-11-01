//! El trazado del circuito: su línea central y las consultas que necesitan
//! la física, la IA y el escenario. No sabe nada de render, así se puede
//! probar sin abrir una ventana.

use std::f32::consts::TAU;

use bevy::prelude::*;
use serde::Deserialize;

use crate::geometry::{forward, heading_of, smoothstep, wrap_angle};

mod grid;
pub mod racing_line;

use grid::SegmentGrid;

/// Altura del asfalto sobre la hierba (si estuvieran a la misma, parpadearían).
pub const SURFACE_Y: f32 = 0.05;

/// El trazado de una pista.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// Puntos de control, en metros sobre el plano XZ y en orden de carrera.
    /// La pista es una curva Catmull-Rom cerrada que pasa por todos ellos.
    /// La salida está en el primero y la recta de meta va hacia -Z.
    #[serde(deserialize_with = "crate::circuits::data::leak")]
    pub control_points: &'static [Vec2],
    /// Altimetría: altura de la carretera (m) a lo largo de la vuelta (m
    /// desde la línea de meta). Entre puntos se interpola con suavidad.
    #[serde(deserialize_with = "crate::circuits::data::leak")]
    pub elevation: &'static [(f32, f32)],
    /// Tramos con más (o menos) peralte del que les toca por su curva.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub banking: &'static [Banking],
    /// Alternative routes that leave the lap and join it again.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub branches: &'static [Branch],
    /// Stretches where the walls close in, narrower than the road: a narrow
    /// bridge.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub narrows: &'static [Narrowing],
    /// How far the walls stand from the centre line where nothing closes
    /// them in (m): [`WALL_DISTANCE`] on most tracks, less on a narrow one,
    /// whose asphalt is as much narrower (half a metre short of them, as
    /// where they close in, see [`Narrowing`]).
    pub wall: f32,
}

/// A stretch of the lap, `from` and `to` metres from the finish line, where
/// the walls stand `wall` metres from the centre line, closer than the edge
/// of the asphalt: a bridge narrower than the road. They close in over the
/// [`NARROW_EASE`] metres before it and open out over as many after it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Narrowing {
    pub from: f32,
    pub to: f32,
    pub wall: f32,
}

/// How far before a narrow stretch the walls start closing in, and after it
/// they have opened out again (m).
pub const NARROW_EASE: f32 = 25.0;
/// Where the walls are closer than the edge of the asphalt, how far short of
/// them the road counts as ending (m): the bikes keep that far off them.
const NARROW_MARGIN: f32 = 0.5;

/// An alternative route: a road that forks off the lap `from` metres after
/// the finish line and joins it again `to` metres after it.
///
/// With a vertical loop, it runs through the control points of `approach`
/// to the mouth of the loop, goes round the loop, and from its way out runs
/// through the control points of `exit` back onto the lap. Both roads are
/// narrower than the lap. Once they part from the lap, they climb or drop
/// smoothly along all their length between it and the loop, to the height
/// of the loop.
///
/// Without one, it is a single road through the control points of
/// `approach` (`exit` is left empty), narrower than the lap and between
/// closer walls once it has parted from it: another way through, past rock
/// between the two. It runs at the height of the lap beside it, higher or
/// lower by what `rise` says: (metres along it, metres above the lap beside
/// it) at some points, smoothly in between, and none at its ends. Its walls
/// close in further where `narrows` says, as the lap's do (`from` and `to`
/// in metres along it): through an archway, say.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Branch {
    pub from: f32,
    pub to: f32,
    #[serde(deserialize_with = "crate::circuits::data::leak")]
    pub approach: &'static [Vec2],
    #[serde(default)]
    pub vertical_loop: Option<VerticalLoop>,
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub exit: &'static [Vec2],
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub rise: &'static [(f32, f32)],
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub narrows: &'static [Narrowing],
}

/// A vertical loop the bikes ride round, upside down at the top.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerticalLoop {
    /// Where the bikes go in: the middle of the running surface at the
    /// bottom (x, z).
    pub mouth: Vec2,
    /// The heading they go in and come out with (see [`crate::geometry`]).
    pub heading: f32,
    /// Height of the road at the bottom, like the heights of
    /// [`Layout::elevation`].
    pub height: f32,
    /// Radius of the running surface (m).
    pub radius: f32,
    /// How far to the left (negative: to the right) the way out is from the
    /// way in (m): the running surface drifts sideways as it goes round, so
    /// that it does not run into itself.
    pub shift: f32,
    /// Whether it has the toy cars' boosters, driving the bikes on: one on
    /// its straight lead in, and one on the lap beside it as far along, so
    /// that neither way gains by them (see [`Track::boosters`]).
    #[serde(default)]
    pub boosters: bool,
}

/// A branch without a loop (see [`Branch`]), as the bikes ride it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fork {
    /// The road it runs along (see [`Nearest::road`]).
    pub road: usize,
    /// The points of the lap's centre line where it leaves the lap and
    /// where it joins it again.
    pub fork: usize,
    pub join: usize,
    /// Which side of the lap it runs on: 1 to the left, -1 to the right.
    pub side: f32,
}

/// Un tramo (m desde la meta) en el que el peralte se multiplica por
/// `factor`, sin pasar del máximo.
///
/// Where it is `raised`, the road banks as a toy track's banked curve does,
/// its outside raised on supports: its inside wall stays where it would be
/// level (on the table or the floor it lies on), and the rest of it rises,
/// instead of the road turning about its middle (see [`Track::lift`]).
/// Where its `factor` is nought, the road lies level, and so do the roads
/// off it beside it, alike.
/// Where `grip` is more than 1, the road grips the tyres that much harder
/// across (as the toy cars' banked track does, see [`Track::grip_beside`]),
/// and may bank as much more steeply.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Banking {
    pub from: f32,
    pub to: f32,
    pub factor: f32,
    #[serde(default)]
    pub raised: bool,
    #[serde(default = "one")]
    pub grip: f32,
}

/// A [`Banking`]'s grip where a circuit's file leaves it out: as any road's.
fn one() -> f32 {
    1.0
}

/// Altura de la línea de meta sobre el nivel del mar: el resto del perfil va
/// por encima, así ni el lado bajo de los peraltes toca el agua.
pub const BASE_ELEVATION: f32 = 2.0;
/// Peralte (rad) por cada unidad de curvatura (1/m): cuanto más cerrada la
/// curva, más inclinada hacia dentro está la carretera.
const BANK_PER_CURVATURE: f32 = 7.0;
const MAX_BANK: f32 = 0.17;
/// Puntos a cada lado con los que se suaviza el peralte.
const BANK_SMOOTHING: usize = 10;
/// On a lap that banks raised, how many points either side the bank is
/// eased over once more (see [`Banking::raised`]).
const RAISED_BANK_SMOOTHING: usize = 12;
/// Where the road banks raised (see [`Banking::raised`]), how many points
/// either side its lift is eased over, twice: in and out over a few tens of
/// metres, no crest or dip to take off from.
const LIFT_SMOOTHING: usize = 10;

const TRACK_WIDTH: f32 = 14.0;
pub const CURB_WIDTH: f32 = 1.2;
/// Distancia de la línea central a la cara interior de los muros que
/// bordean la carretera: lo que se puede salir uno de la pista.
pub const WALL_DISTANCE: f32 = 11.5;
/// Separación entre puntos de la línea central; también el largo de cada
/// franja de los pianos.
pub const SAMPLE_SPACING: f32 = 2.0;
/// Cuántos puntos por detrás de la línea de meta está la primera fila de la parrilla.
const START_BEHIND_SAMPLES: usize = 5;
/// Distancia entre filas de la parrilla, en puntos del trazado.
const GRID_ROW_SAMPLES: usize = 5;
/// Parrilla como la de los Grandes Premios: tres motos por fila, escalonadas (cada una un
/// punto del trazado más atrás que la de su izquierda), a estos metros a la
/// izquierda de la línea central.
const GRID_LANES: [f32; 3] = [4.5, 0.0, -4.5];

/// Half the width of the asphalt of a road off the lap (m).
const SIDE_HALF_WIDTH: f32 = 5.0;
/// While a road off the lap shares the road between the lap's walls, it
/// lies on the lap's surface, with its height and its banking. From twice
/// as far from the lap's centre line as the lap's walls, where their walls
/// part, it goes over to its own height and banking, all of it this much
/// further away (m).
const JUNCTION_BLEND: f32 = 30.0;
/// A branch without a loop goes over to its own height faster, over this
/// much further away (m): it can climb or drop away from the lap as soon as
/// there is rock between them.
const FORK_BLEND: f32 = 10.0;
/// Points to either side over which how far along the lap a branch without
/// a loop runs beside is smoothed.
const FORK_SMOOTHING: usize = 8;
/// Past where it parts from the lap, a branch without a loop goes over,
/// over this much further away (m), from the height of the lap's surface
/// beside it to that of the lap as far along it as it counts as: far from
/// a lap that winds, the point of the lap nearest to it runs on in fits and
/// starts, and its climbs and drops with it.
const FORK_FAR_BLEND: f32 = 45.0;
/// Distance from the centre line of a branch without a loop to its walls
/// once it has parted from the lap (m): closer than the lap's (or as close,
/// on a narrow track).
pub const FORK_WALL: f32 = 8.5;
/// Half the width of the running surface of a loop (m): the asphalt of the
/// road that leads to it and its curbs.
pub const LOOP_HALF_WIDTH: f32 = SIDE_HALF_WIDTH + CURB_WIDTH;
/// Distance from the middle of a loop to the walls at its mouth (m), and
/// over how much road before and after it the walls close in to there and
/// open out again (m).
pub const LOOP_WALL: f32 = LOOP_HALF_WIDTH + 0.3;
const FUNNEL: f32 = 70.0;
/// The last metres of road into a loop and the first out of it, straight
/// along it.
const LOOP_LEAD: f32 = 25.0;

/// Tipo de suelo bajo un punto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// Asfalto o piano.
    Asphalt,
    /// Grass: it slows the bike down and its blades fly off the wheels.
    Grass,
    /// Bare earth, sand or gravel: it slows the bike down like grass, and
    /// the wheels raise a cloud of dust of its colour.
    Dirt(Dust),
    /// Concrete or planks, such as a tunnel's walkway or a bridge's deck:
    /// as firm as the road, so it neither slows the bike down nor throws
    /// anything up.
    Paved,
}

impl Surface {
    /// Whether it is rough ground off the road, which slows the bike down:
    /// grass or dirt.
    pub fn is_rough(self) -> bool {
        matches!(self, Surface::Grass | Surface::Dirt(_))
    }
}

/// What the dirt beside a road is, which gives its dust its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dust {
    /// Pale sand of a beach, or limestone grit.
    Sand,
    /// Brown earth.
    Earth,
    /// Red-brown earth of a desert, or red sandstone gravel.
    Desert,
    /// Black slate gravel.
    Slate,
}

/// Datos estáticos del circuito.
#[derive(Resource)]
pub struct Track {
    /// Línea central equiespaciada, en orden de carrera; el último punto
    /// enlaza con el primero.
    centerline: Vec<Vec2>,
    /// Altura de la carretera en cada punto de la línea central.
    elevation: Vec<f32>,
    /// Peralte en cada punto (rad): positivo si el lado derecho está más alto,
    /// como en una curva a la izquierda.
    bank: Vec<f32>,
    /// How much each point of the centre line is raised for its banking
    /// (m), where it banks raised (see [`Banking::raised`]): at least as much
    /// as its inside wall would sink turning about the middle. Empty: none
    /// (a road off the lap lies level where the lap banks raised).
    lift: Vec<f32>,
    /// Whether each point of the lap's centre line banks raised. Empty:
    /// none do.
    raised: Vec<bool>,
    /// How much harder than usual each point of the lap grips across (see
    /// [`Banking::grip`]). Empty: as usual all round.
    grip: Vec<f32>,
    /// Whether each point of the lap's centre line is set level (its
    /// banking's factor nought). Empty: none is.
    level: Vec<bool>,
    half_width: f32,
    /// Distancia entre puntos de la línea central (ver [`Self::step_length`]).
    step: f32,
    /// Where the solid edge is at each point of the centre line, to the left
    /// and to the right (m from it): the walls, or the earth where it rises
    /// before them (see [`Self::set_edges`]). Empty: the walls everywhere.
    edges: Vec<[f32; 2]>,
    /// What the ground between the asphalt and the walls is at each point
    /// of the centre line, to the left and to the right (see
    /// [`Self::set_shoulders`]). Empty: grass everywhere.
    shoulders: Vec<[Surface; 2]>,
    /// Whether the centre line closes on itself: the lap does; a road off it
    /// (see [`Branch`]) does not.
    closed: bool,
    /// How far the walls are from the centre line at each point (m). Empty:
    /// at [`Self::wall`] everywhere.
    walls: Vec<f32>,
    /// How far the walls stand from the centre line where nothing closes
    /// them in (m): the lap's (see [`Layout::wall`]), on a road off it too.
    wall: f32,
    /// On a road off the lap, how far along the lap each of its points
    /// counts as (m from the finish line). Empty on the lap.
    lap_along: Vec<f32>,
    /// On a road off the lap, how much its surface is its own at each point,
    /// from 0 where it lies on the lap's to 1 once it is apart. Empty on
    /// the lap.
    apart: Vec<f32>,
    /// The roads off the lap: for each branch, the road to its loop and the
    /// road back. Numbered from 1 in [`Nearest::road`]; the lap is 0.
    side_roads: Vec<Track>,
    loops: Vec<Loop>,
    /// The boosters on its roads (see [`Self::boosters`]).
    boosters: Vec<Booster>,
    forks: Vec<Fork>,
    /// This road's segments by where they are, to find the nearest without
    /// going through them all (see [`Self::nearest_at`]). Empty until the
    /// track is made: then every segment is looked at.
    grid: SegmentGrid,
}

/// El punto de la línea central más cercano a otro.
#[derive(Clone, Copy, Debug)]
pub struct Nearest {
    pub point: Vec2,
    pub distance: f32,
    /// Tramo de la línea central en el que está (de `index` a `index + 1`) y
    /// por dónde de él (0..1).
    pub index: usize,
    pub t: f32,
    /// Distancia con signo a la línea central: positiva a la izquierda.
    pub lateral: f32,
    /// Which road it is on: 0 for the lap, or a road off it (see
    /// [`Track::road`]). `index` counts along that road.
    pub road: usize,
}

/// A vertical loop as the bikes ride it (see [`VerticalLoop`]): where it
/// is, and the roads in and out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loop {
    pub mouth: Vec2,
    /// Unit vector the way the bikes go in, on the ground.
    pub ahead: Vec2,
    pub heading: f32,
    /// Height of the running surface at the bottom (m).
    pub height: f32,
    pub radius: f32,
    pub shift: f32,
    /// The roads (see [`Nearest::road`]) that lead in and out.
    pub approach: usize,
    pub exit: usize,
    /// The points of the lap's centre line where the road in leaves it and
    /// where the road out joins it.
    pub fork: usize,
    pub join: usize,
    /// Whether it has boosters on its leads (see [`VerticalLoop::boosters`]).
    pub boosters: bool,
}

/// A booster on a road (see [`VerticalLoop::boosters`]): from `from` to
/// `to` along it (m from its start), all across it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Booster {
    pub road: usize,
    pub from: f32,
    pub to: f32,
}

/// The speed a booster drives a bike on up to (m/s, about 270 km/h, near
/// its top speed: it always pushes), how long it is (m), and where on a
/// loop's lead in it is (m before the mouth, where the lead begins).
pub const BOOSTER_SPEED: f32 = 75.0;
pub const BOOSTER_LENGTH: f32 = 12.0;
const BOOSTER_INTO: f32 = 2.0 * LOOP_LEAD - 6.0;

impl Loop {
    /// Unit vector to the left of the way in, on the ground: across the
    /// running surface.
    pub fn left(&self) -> Vec2 {
        Vec2::new(self.ahead.y, -self.ahead.x)
    }

    /// Where the bikes come out: the middle of the running surface at the
    /// bottom, beside the mouth.
    pub fn way_out(&self) -> Vec2 {
        self.mouth + self.left() * self.shift
    }

    /// Metres of running surface per radian round (m).
    pub fn arc(&self) -> f32 {
        self.radius.hypot(self.shift / TAU)
    }

    /// How long the running surface is, all the way round (m).
    pub fn length(&self) -> f32 {
        TAU * self.arc()
    }

    /// Point of the running surface `angle` radians round from the mouth,
    /// `lateral` metres to the left of its middle (x, height, z).
    pub fn surface_point(&self, angle: f32, lateral: f32) -> Vec3 {
        let ground = self.mouth
            + self.ahead * self.radius * angle.sin()
            + self.left() * (self.shift * angle / TAU + lateral);
        Vec3::new(
            ground.x,
            self.height + self.radius * (1.0 - angle.cos()),
            ground.y,
        )
    }

    /// Unit vector along the running surface the way the bikes go, `angle`
    /// radians round (not counting the drift sideways).
    pub fn tangent(&self, angle: f32) -> Vec3 {
        let ahead = self.ahead * angle.cos();
        Vec3::new(ahead.x, angle.sin(), ahead.y)
    }

    /// Unit vector out of the running surface, towards the middle of the
    /// loop: which way is up for a bike riding it.
    pub fn up(&self, angle: f32) -> Vec3 {
        let back = -self.ahead * angle.sin();
        Vec3::new(back.x, angle.cos(), back.y)
    }

    /// If something at `position`, at `height`, has just gone in at the
    /// mouth, how far in (m) and how far to the left of the middle (m).
    pub fn entered(&self, position: Vec2, height: f32) -> Option<(f32, f32)> {
        let from = position - self.mouth;
        let (into, lateral) = (from.dot(self.ahead), from.dot(self.left()));
        ((0.0..4.0).contains(&into)
            && lateral.abs() < LOOP_WALL
            && (height - self.height).abs() < 1.5)
            .then_some((into, lateral))
    }
}

/// For the tests: the engine's demo circuit's, Bahía's.
#[cfg(any(test, feature = "testing"))]
impl Default for Track {
    fn default() -> Self {
        Self::new(&crate::testing::demo_circuit().layout)
    }
}

impl Track {
    pub fn new(layout: &Layout) -> Self {
        let mut track = Self::lap(layout);
        for branch in layout.branches {
            track.add_branch(branch);
        }
        track.make_grids();
        track.boosters = track.lay_boosters();
        track
    }

    /// The boosters of the loops that have them (see
    /// [`VerticalLoop::boosters`]): on the lead in, and on the lap beside
    /// it, from the point of the lap nearest where the other begins.
    fn lay_boosters(&self) -> Vec<Booster> {
        self.loops
            .iter()
            .filter(|looped| looped.boosters)
            .flat_map(|looped| {
                let approach = self.road(looped.approach);
                let from = approach.length() - BOOSTER_INTO;
                let at = approach.centerline[(from / approach.step).round() as usize];
                let beside = self.project_on_lap(at);
                let lap_from = (beside.index as f32 + beside.t) * self.step;
                [(looped.approach, from), (0, lap_from)].map(|(road, from)| Booster {
                    road,
                    from,
                    to: from + BOOSTER_LENGTH,
                })
            })
            .collect()
    }

    /// The grids of the lap and of the roads off it, once they are all laid.
    fn make_grids(&mut self) {
        let n = self.centerline.len();
        self.grid = SegmentGrid::new(self.segment_count(), |index| {
            (self.centerline[index], self.centerline[(index + 1) % n])
        });
        for road in &mut self.side_roads {
            road.make_grids();
        }
    }

    /// The lap of a layout, without its branches.
    fn lap(layout: &Layout) -> Self {
        let mut track = Self {
            centerline: resample(&sample_spline(layout.control_points), SAMPLE_SPACING),
            elevation: Vec::new(),
            bank: Vec::new(),
            lift: Vec::new(),
            raised: Vec::new(),
            grip: Vec::new(),
            level: Vec::new(),
            half_width: (TRACK_WIDTH / 2.0).min(layout.wall - NARROW_MARGIN),
            step: 0.0,
            edges: Vec::new(),
            shoulders: Vec::new(),
            closed: true,
            walls: Vec::new(),
            wall: layout.wall,
            lap_along: Vec::new(),
            apart: Vec::new(),
            side_roads: Vec::new(),
            loops: Vec::new(),
            boosters: Vec::new(),
            forks: Vec::new(),
            grid: SegmentGrid::default(),
        };
        track.step = track.segments().map(|(a, b)| a.distance(b)).sum::<f32>()
            / track.centerline.len() as f32;
        let (n, step) = (track.centerline.len(), track.step);
        let lap = n as f32 * step;
        track.elevation = (0..n)
            .map(|i| BASE_ELEVATION + profile_height(layout.elevation, lap, i as f32 * step))
            .collect();
        let factor = |j: usize| {
            let along = (j % n) as f32 * step;
            layout
                .banking
                .iter()
                .find(|banking| (banking.from..banking.to).contains(&along))
                .map_or(1.0, |banking| banking.factor)
        };
        track.bank = (0..n)
            .map(|i| {
                let window = (i + n - BANK_SMOOTHING)..=(i + n + BANK_SMOOTHING);
                let count = window.clone().count() as f32;
                let curvature = window.map(|j| track.curvature(j) * factor(j)).sum::<f32>() / count;
                (curvature * BANK_PER_CURVATURE).clamp(-MAX_BANK, MAX_BANK)
            })
            .collect();
        if !layout.narrows.is_empty() {
            track.walls = (0..n)
                .map(|i| {
                    let along = i as f32 * step;
                    layout
                        .narrows
                        .iter()
                        .map(|narrow| {
                            // From where they start closing in to where they
                            // have opened out again.
                            let total =
                                (narrow.to - narrow.from).rem_euclid(lap) + 2.0 * NARROW_EASE;
                            let into = (along - narrow.from + NARROW_EASE).rem_euclid(lap);
                            let closed = if into > total {
                                0.0
                            } else {
                                smoothstep(0.0, NARROW_EASE, into)
                                    * smoothstep(0.0, NARROW_EASE, total - into)
                            };
                            track.wall.lerp(narrow.wall, closed)
                        })
                        .fold(track.wall, f32::min)
                })
                .collect();
        }
        // On a lap that banks raised anywhere (a toy track's banked curves,
        // on supports), the bank comes to its most softly instead of
        // stopping dead at it, and is eased over as many points again, all
        // round (or where its kinds of banking meet it would jump): else its
        // outside rises and falls in kinks, felt as bumps off the middle of
        // the road.
        let raised_at = |i: usize| {
            let along = (i % n) as f32 * step;
            layout
                .banking
                .iter()
                .find(|banking| (banking.from..banking.to).contains(&along))
                .is_some_and(|banking| banking.raised)
        };
        let grip = |i: usize| {
            let along = (i % n) as f32 * step;
            layout
                .banking
                .iter()
                .find(|banking| (banking.from..banking.to).contains(&along))
                .map_or(1.0, |banking| banking.grip)
        };
        if (0..n).any(|i| grip(i) != 1.0) {
            track.grip = (0..n).map(grip).collect();
        }
        if (0..n).any(raised_at) {
            // (Where it grips harder, it may bank as much more steeply.)
            let soft: Vec<f32> = (0..n)
                .map(|i| {
                    let window = (i + n - BANK_SMOOTHING)..=(i + n + BANK_SMOOTHING);
                    let count = window.clone().count() as f32;
                    let curvature =
                        window.map(|j| track.curvature(j) * factor(j)).sum::<f32>() / count;
                    let most = MAX_BANK * grip(i);
                    most * (curvature * BANK_PER_CURVATURE / most).tanh()
                })
                .collect();
            let reach = RAISED_BANK_SMOOTHING as isize;
            track.bank = (0..n as isize)
                .map(|i| {
                    (i - reach..=i + reach)
                        .map(|j| soft[j.rem_euclid(n as isize) as usize])
                        .sum::<f32>()
                        / (2 * reach + 1) as f32
                })
                .collect();
        }
        track.level = (0..n).map(|i| factor(i) == 0.0).collect();
        // Where it banks raised, its middle goes up by as much as its inside
        // wall would sink, eased in and out.
        track.raised = (0..n)
            .map(|i| {
                let along = i as f32 * step;
                layout
                    .banking
                    .iter()
                    .find(|banking| (banking.from..banking.to).contains(&along))
                    .is_some_and(|banking| banking.raised)
            })
            .collect();
        if track.raised.contains(&true) {
            let sink: Vec<f32> = (0..n)
                .map(|i| {
                    if track.raised[i] {
                        track.wall_at(i) * track.bank[i].tan().abs()
                    } else {
                        0.0
                    }
                })
                .collect();
            track.lift = eased(&sink);
            for (elevation, lift) in track.elevation.iter_mut().zip(&track.lift) {
                *elevation += lift;
            }
        } else {
            track.raised.clear();
        }
        track
    }

    /// Adds the two roads of a branch, and the loop between them, or the
    /// one road of a branch without a loop.
    fn add_branch(&mut self, branch: &Branch) {
        let Some(spec) = &branch.vertical_loop else {
            self.add_fork(branch);
            return;
        };
        let ahead = forward(spec.heading);
        let looped = Loop {
            mouth: spec.mouth,
            ahead,
            heading: spec.heading,
            height: BASE_ELEVATION + spec.height,
            radius: spec.radius,
            shift: spec.shift,
            approach: self.side_roads.len() + 1,
            exit: self.side_roads.len() + 2,
            fork: self.index_at(branch.from),
            join: self.index_at(branch.to),
            boosters: spec.boosters,
        };
        let lap_point = |along: f32| self.centerline[self.index_at(along)];
        // The last stretch into the loop and the first out of it are
        // straight, and each road leaves the lap, or joins it, along it.
        let lead = ahead * LOOP_LEAD;
        let (mouth, way_out) = (looped.mouth, looped.way_out());
        let mut points = vec![lap_point(branch.from)];
        points.extend_from_slice(branch.approach);
        points.extend([mouth - 2.0 * lead, mouth - lead, mouth]);
        let approach = open_curve(&points, lap_point(branch.from - 20.0), mouth + lead);
        let mut points = vec![way_out, way_out + lead, way_out + 2.0 * lead];
        points.extend_from_slice(branch.exit);
        points.push(lap_point(branch.to));
        let exit = open_curve(&points, way_out - lead, lap_point(branch.to + 20.0));

        // How far along the lap each point of the branch counts as: the
        // stretch of lap it stands in for, shared out by distance.
        let length = |line: &[Vec2]| line.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>();
        let (into, out) = (length(&approach), length(&exit));
        let total = into + looped.length() + out;
        let lap = self.lap_length();
        let span = (branch.to - branch.from).rem_euclid(lap);
        let counts_as = |from: f32| move |s: f32| (branch.from + span * (from + s) / total) % lap;
        let approach = self.side_road(approach, Some((&looped, true)), &[], counts_as(0.0));
        let exit = self.side_road(
            exit,
            Some((&looped, false)),
            &[],
            counts_as(into + looped.length()),
        );
        self.side_roads.extend([approach, exit]);
        self.loops.push(looped);
    }

    /// Adds the road of a branch without a loop: from the lap through the
    /// control points back onto it, leaving it and joining it along it.
    fn add_fork(&mut self, branch: &Branch) {
        let lap_point = |along: f32| self.centerline[self.index_at(along)];
        let mut points = vec![lap_point(branch.from)];
        points.extend_from_slice(branch.approach);
        points.push(lap_point(branch.to));
        let centerline = open_curve(
            &points,
            lap_point(branch.from - 20.0),
            lap_point(branch.to + 20.0),
        );
        let length = centerline
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .sum::<f32>();
        let lap = self.lap_length();
        let span = (branch.to - branch.from).rem_euclid(lap);
        let counts_as = |s: f32| (branch.from + span * s / length) % lap;
        let middle = centerline[centerline.len() / 2];
        let side = self.project_on_lap(middle).lateral.signum();
        let mut road = self.side_road(centerline, None, branch.rise, counts_as);
        for (i, wall) in road.walls.iter_mut().enumerate() {
            let along = i as f32 * road.step;
            *wall = branch
                .narrows
                .iter()
                .map(|narrow| {
                    let closed = smoothstep(narrow.from - NARROW_EASE, narrow.from, along)
                        * (1.0 - smoothstep(narrow.to, narrow.to + NARROW_EASE, along));
                    wall.lerp(narrow.wall.min(*wall), closed)
                })
                .fold(*wall, f32::min);
        }
        self.forks.push(Fork {
            road: self.side_roads.len() + 1,
            fork: self.index_at(branch.from),
            join: self.index_at(branch.to),
            side,
        });
        self.side_roads.push(road);
    }

    /// A road off the lap along `centerline`. With a loop, it comes from the
    /// lap into the loop if the flag says so, or else out of the loop onto
    /// the lap; without one, it is a branch that leaves the lap and joins it
    /// again. `counts_as` says how far along the lap each metre of it counts
    /// as.
    fn side_road(
        &self,
        centerline: Vec<Vec2>,
        toward: Option<(&Loop, bool)>,
        rise: &[(f32, f32)],
        counts_as: impl Fn(f32) -> f32,
    ) -> Track {
        let n = centerline.len();
        let step = centerline
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .sum::<f32>()
            / (n - 1) as f32;
        let mut road = Track {
            centerline,
            elevation: Vec::new(),
            bank: Vec::new(),
            lift: Vec::new(),
            raised: Vec::new(),
            grip: Vec::new(),
            level: Vec::new(),
            half_width: SIDE_HALF_WIDTH,
            step,
            edges: Vec::new(),
            shoulders: Vec::new(),
            closed: false,
            walls: Vec::new(),
            wall: self.wall,
            lap_along: Vec::new(),
            apart: Vec::new(),
            side_roads: Vec::new(),
            loops: Vec::new(),
            boosters: Vec::new(),
            forks: Vec::new(),
            grid: SegmentGrid::default(),
        };
        let length = road.length();
        // Metres from the end at the loop, if any.
        let from_loop = |i: usize| {
            let s = i as f32 * step;
            match toward {
                Some((_, true)) => length - s,
                Some((_, false)) => s,
                None => f32::INFINITY,
            }
        };
        // Near the lap the road lies on its surface; once their walls part,
        // it has a surface of its own.
        let junction_apart = 2.0 * self.wall;
        let on_lap: Vec<Nearest> = road
            .centerline
            .iter()
            .map(|&point| self.project_on_lap(point))
            .collect();
        let lap_height: Vec<f32> = on_lap.iter().map(|on| self.height_on_road(on)).collect();
        let apart: Vec<f32> = on_lap
            .iter()
            .map(|on| {
                let blend = if toward.is_some() {
                    JUNCTION_BLEND
                } else {
                    FORK_BLEND
                };
                smoothstep(junction_apart, junction_apart + blend, on.distance)
            })
            .collect();
        let elevation = match toward {
            Some((looped, into_loop)) => {
                Self::toward_loop(&apart, &lap_height, looped.height, into_loop, step)
            }
            // Without a loop, at the height of the lap's surface beside it,
            // higher or lower by its rise. Far from the lap its nearest
            // point moves along the lap's polyline in fits and starts: how
            // far along the lap it is beside is smoothed first, less of it
            // towards the ends, where it lies on the lap; and further away,
            // it runs at the height of the lap as far along it as it counts
            // as, which runs on steadily.
            None => {
                let laps = self.sample_count();
                let first = on_lap[0].index;
                let beside: Vec<f32> = on_lap
                    .iter()
                    .map(|on| ((on.index + laps - first) % laps) as f32 + on.t)
                    .collect();
                (0..n)
                    .map(|i| {
                        // Evenly either side, so that a steady run is left
                        // as it is.
                        let reach = FORK_SMOOTHING.min(i).min(n - 1 - i);
                        let near = i - reach..i + reach + 1;
                        let count = near.len() as f32;
                        let along = first as f32 + beside[near].iter().sum::<f32>() / count;
                        let k = along.floor() as usize % laps;
                        let (next, t) = ((k + 1) % laps, along.fract());
                        let elevation = self.elevation[k].lerp(self.elevation[next], t);
                        let bank = self.bank[k].lerp(self.bank[next], t);
                        let lateral = on_lap[i].lateral.clamp(-WALL_DISTANCE, WALL_DISTANCE);
                        let beside = elevation - lateral * bank.tan();
                        let counted = counts_as(i as f32 * step) / self.step;
                        let k = counted.floor() as usize % laps;
                        let far =
                            self.elevation[k].lerp(self.elevation[(k + 1) % laps], counted.fract());
                        let away = smoothstep(
                            junction_apart,
                            junction_apart + FORK_FAR_BLEND,
                            on_lap[i].distance,
                        );
                        beside.lerp(far, away) + open_profile(rise, length, i as f32 * step)
                    })
                    .collect()
            }
        };
        // It banks for its own curves, but not in the straight into or out
        // of the loop, nor where the lap beside it banks raised (a toy
        // track's, on supports), nor where the lap is set level beside it:
        // there it lies level, as the lap.
        let level_into_loop = |i: usize| smoothstep(LOOP_LEAD, LOOP_LEAD + 30.0, from_loop(i));
        let bank = (0..n)
            .map(|i| {
                let on = &on_lap[i];
                let lap_bank =
                    self.bank[on.index].lerp(self.bank[(on.index + 1) % self.sample_count()], on.t);
                let window =
                    (i as isize - BANK_SMOOTHING as isize)..=(i as isize + BANK_SMOOTHING as isize);
                let count = window.clone().count() as f32;
                let curvature = window.map(|j| road.curvature(road.wrap(j))).sum::<f32>() / count;
                let flat = |of: &[bool]| of.get(on.index).copied().unwrap_or(false);
                let own_bank = if flat(&self.raised) || flat(&self.level) {
                    0.0
                } else {
                    (curvature * BANK_PER_CURVATURE).clamp(-MAX_BANK, MAX_BANK)
                };
                lap_bank.lerp(own_bank, apart[i]) * level_into_loop(i)
            })
            .collect();
        road.elevation = elevation;
        road.bank = bank;
        // The walls close in towards a loop; on a branch without one, once
        // it has parted from the lap.
        road.walls = (0..n)
            .map(|i| match toward {
                Some(_) => LOOP_WALL.lerp(self.wall, smoothstep(10.0, FUNNEL, from_loop(i))),
                None => self.wall.lerp(FORK_WALL.min(self.wall), apart[i]),
            })
            .collect();
        road.apart = apart;
        road.lap_along = (0..n).map(|i| counts_as(i as f32 * step)).collect();
        road
    }

    /// The heights of a road from the lap into a loop at `height` (or out
    /// of it, if not `into_loop`), whose points are `step` apart and lie on
    /// the lap's surface at `lap_height` as far as `apart` says. Its own
    /// height runs smoothly along it, from the lap's height and grade where
    /// it parts from the lap to the height of the loop, reached level at the
    /// straight into or out of it: spread over all the road in between, with
    /// no ramp or dip for a bike to take off from or slam into.
    fn toward_loop(
        apart: &[f32],
        lap_height: &[f32],
        height: f32,
        into_loop: bool,
        step: f32,
    ) -> Vec<f32> {
        let n = apart.len();
        // Counted from the loop out to the lap.
        let outwards = |k: usize| if into_loop { n - 1 - k } else { k };
        let parts = (0..n)
            .rposition(|k| apart[outwards(k)] > 0.0)
            .unwrap_or(n - 1);
        let lead = ((LOOP_LEAD / step).round() as usize).min(parts);
        let span = (parts - lead).max(1) as f32 * step;
        let grade = (lap_height[outwards((parts + 1).min(n - 1))]
            - lap_height[outwards(parts.saturating_sub(1))])
            / (2.0 * step);
        let mut elevation = vec![height; n];
        for k in 0..n {
            let u = k.saturating_sub(lead) as f32 * step / span;
            elevation[outwards(k)] = if k >= parts {
                lap_height[outwards(k)]
            } else {
                ease(height, lap_height[outwards(parts)], grade * span, u)
            };
        }
        elevation
    }

    /// The nearest point of the lap's own centre line, leaving out the roads
    /// off it.
    fn project_on_lap(&self, point: Vec2) -> Nearest {
        if self.searches_grid(point) {
            return self
                .nearest_in_grid(point, f32::INFINITY, |_| true)
                .expect("el trazado tiene puntos");
        }
        self.project_on_lap_every(point)
    }

    /// Like [`Self::project_on_lap`], going through every segment.
    fn project_on_lap_every(&self, point: Vec2) -> Nearest {
        (0..self.sample_count())
            .map(|index| self.project(index, point))
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
            .expect("el trazado tiene puntos")
    }

    pub fn half_width(&self) -> f32 {
        self.half_width
    }

    /// The roads off the lap, as numbered in [`Nearest::road`] from 1.
    pub fn side_roads(&self) -> &[Track] {
        &self.side_roads
    }

    /// The vertical loops of the branches.
    pub fn loops(&self) -> &[Loop] {
        &self.loops
    }

    /// The branches without a loop.
    pub fn forks(&self) -> &[Fork] {
        &self.forks
    }

    /// Adds a loop on its own, without roads in or out: for the tests.
    #[cfg(any(test, feature = "testing"))]
    pub fn add_loop_for_tests(&mut self, looped: Loop) {
        self.loops.push(looped);
    }

    /// Road number `road` (see [`Nearest::road`]): the lap itself for 0.
    pub fn road(&self, road: usize) -> &Track {
        match road {
            0 => self,
            _ => &self.side_roads[road - 1],
        }
    }

    /// Whether the centre line closes on itself, as the lap's does.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// How many stretches the centre line has between its points.
    pub fn segment_count(&self) -> usize {
        if self.closed {
            self.centerline.len()
        } else {
            self.centerline.len() - 1
        }
    }

    /// How long the centre line is, from end to end (m): a whole lap for the
    /// lap.
    pub fn length(&self) -> f32 {
        self.segment_count() as f32 * self.step
    }

    /// The boosters on the roads (see [`VerticalLoop::boosters`]): on the
    /// leads into the loops that have them, and on the lap beside them.
    pub fn boosters(&self) -> &[Booster] {
        &self.boosters
    }

    /// The speed a booster under a point already found drives a bike on to
    /// (m/s, see [`BOOSTER_SPEED`]), or nought where there is none.
    pub fn booster_beside(&self, nearest: &Nearest) -> f32 {
        let road = self.road(nearest.road);
        let along = (nearest.index as f32 + nearest.t) * road.step;
        let on = self.boosters.iter().any(|booster| {
            booster.road == nearest.road
                && (booster.from..booster.to).contains(&along)
                && nearest.lateral.abs() < road.wall_at(nearest.index)
        });
        if on { BOOSTER_SPEED } else { 0.0 }
    }

    /// How far along the lap point `index` of the centre line counts as (m
    /// from the finish line): on a road off the lap, the stretch of the lap
    /// it stands in for.
    pub fn lap_along_at(&self, index: usize) -> f32 {
        self.lap_along
            .get(self.wrap(index as isize))
            .copied()
            .unwrap_or(index as f32 * self.step)
    }

    /// Whether `point` is on some other road than number `road` (see
    /// [`Nearest::road`]): nearer the centre line of one than `within` says
    /// for the point of it nearest to `point`.
    pub fn on_other_road(
        &self,
        road: usize,
        point: Vec2,
        within: impl Fn(&Track, &Nearest) -> f32,
    ) -> bool {
        (0..=self.side_roads.len())
            .filter(|&other| other != road)
            .any(|other| {
                let line = self.road(other);
                let (nearest, _) = line.search(point, None, other);
                nearest.is_some_and(|nearest| nearest.distance < within(line, &nearest))
            })
    }

    /// How far the walls are from the centre line at point `index` (m).
    pub fn wall_at(&self, index: usize) -> f32 {
        self.walls
            .get(self.wrap(index as isize))
            .copied()
            .unwrap_or(self.wall)
    }

    /// How far the walls stand from the centre line where nothing closes
    /// them in (m): see [`Layout::wall`].
    pub fn wall(&self) -> f32 {
        self.wall
    }

    /// How much point `index` of the centre line is raised for its banking
    /// (m): where it banks raised (see [`Banking::raised`]), at least as
    /// much as its inside wall would sink turning about the middle, eased in
    /// and out; elsewhere nothing.
    /// How much harder than usual road `road` grips across at point `index`
    /// of its centre line (see [`Banking::grip`]): the lap's, as usual off
    /// it.
    pub fn grip_at(&self, road: usize, index: usize) -> f32 {
        if road != 0 || self.grip.is_empty() {
            return 1.0;
        }
        self.grip[self.wrap(index as isize)]
    }

    /// How much harder than usual the road grips across beside a point
    /// already found (see [`Banking::grip`]): the lap's, as usual off it.
    pub fn grip_beside(&self, nearest: &Nearest) -> f32 {
        if nearest.road != 0 || self.grip.is_empty() {
            return 1.0;
        }
        let (i, j) = (
            self.wrap(nearest.index as isize),
            self.wrap(nearest.index as isize + 1),
        );
        self.grip[i].lerp(self.grip[j], nearest.t)
    }

    pub fn lift(&self, index: usize) -> f32 {
        self.lift
            .get(self.wrap(index as isize))
            .copied()
            .unwrap_or(0.0)
    }

    /// The height of the road beside a point already found as it would lie
    /// level where it banks raised (see [`Banking::raised`]): its inside
    /// wall's, which stays there; elsewhere, as [`Self::height_beside`].
    pub fn level_beside(&self, nearest: &Nearest) -> f32 {
        let road = self.road(nearest.road);
        let (i, j) = (
            road.wrap(nearest.index as isize),
            road.wrap(nearest.index as isize + 1),
        );
        if !road.raised.get(i).copied().unwrap_or(false) {
            return self.height_beside(nearest);
        }
        let elevation = road.elevation[i].lerp(road.elevation[j], nearest.t);
        elevation - road.lift[i].lerp(road.lift[j], nearest.t)
    }

    /// Point `index` of the centre line, counting round the lap, or held at
    /// the ends of a road off it.
    fn wrap(&self, index: isize) -> usize {
        let n = self.centerline.len();
        if self.closed {
            index.rem_euclid(n as isize) as usize
        } else {
            index.clamp(0, n as isize - 1) as usize
        }
    }

    /// Where the solid edge is beside each road, at each point of its centre
    /// line, to the left and to the right: the walls, or the earth where the
    /// terrain rises before them. The lap first, then the roads off it in
    /// order (see [`Nearest::road`]). Each is capped at its walls.
    pub fn set_edges(&mut self, mut edges: Vec<Vec<[f32; 2]>>) {
        edges.resize(1 + self.side_roads.len(), Vec::new());
        for (road, edges) in edges.into_iter().enumerate() {
            let road = match road {
                0 => &mut *self,
                _ => &mut self.side_roads[road - 1],
            };
            debug_assert!(edges.is_empty() || edges.len() == road.centerline.len());
            let capped = edges
                .into_iter()
                .enumerate()
                .map(|(index, sides)| sides.map(|edge| edge.min(road.wall_at(index))))
                .collect();
            road.edges = capped;
        }
    }

    /// What the ground between the asphalt and the walls of each road is,
    /// at each point of its centre line, to the left and to the right:
    /// grass, dirt, or the concrete or planks of a tunnel or a bridge. The
    /// lap first, then the roads off it in order (see [`Nearest::road`]).
    pub fn set_shoulders(&mut self, mut shoulders: Vec<Vec<[Surface; 2]>>) {
        shoulders.resize(1 + self.side_roads.len(), Vec::new());
        for (road, shoulders) in shoulders.into_iter().enumerate() {
            let road = match road {
                0 => &mut *self,
                _ => &mut self.side_roads[road - 1],
            };
            debug_assert!(shoulders.is_empty() || shoulders.len() == road.centerline.len());
            road.shoulders = shoulders;
        }
    }

    /// How far from the centre line the solid edge is beside a point
    /// (m): the wall, or the earth where it rises before the wall.
    pub fn edge_beside(&self, nearest: &Nearest) -> f32 {
        let road = self.road(nearest.road);
        let (i, j) = (
            road.wrap(nearest.index as isize),
            road.wrap(nearest.index as isize + 1),
        );
        let side = usize::from(nearest.lateral < 0.0);
        let edge = |index: usize| {
            road.edges
                .get(index)
                .map_or(road.wall_at(index), |edges| edges[side])
        };
        edge(i).lerp(edge(j), nearest.t)
    }

    /// The closest the solid edge comes on either side of the road of
    /// `nearest` anywhere near it (within `reach` points of its centre
    /// line).
    pub fn closest_edge_near(&self, nearest: &Nearest, reach: usize) -> f32 {
        let road = self.road(nearest.road);
        let (index, reach) = (nearest.index as isize, reach as isize);
        (index - reach..=index + reach)
            .map(|i| road.wrap(i))
            .flat_map(|i| road.edges.get(i).copied().unwrap_or([road.wall_at(i); 2]))
            .fold(WALL_DISTANCE, f32::min)
    }

    pub fn sample_count(&self) -> usize {
        self.centerline.len()
    }

    /// Lo que mide una vuelta (m).
    pub fn lap_length(&self) -> f32 {
        self.centerline.len() as f32 * self.step_length()
    }

    /// Por dónde de la vuelta (m desde la meta) está el punto más cercano.
    /// On a road off the lap, the stretch of the lap it stands in for.
    pub fn along(&self, nearest: &Nearest) -> f32 {
        let road = self.road(nearest.road);
        if road.closed {
            return (nearest.index as f32 + nearest.t) * road.step_length();
        }
        let (a, b) = (
            road.lap_along[road.wrap(nearest.index as isize)],
            road.lap_along[road.wrap(nearest.index as isize + 1)],
        );
        // Across the finish line, the next point counts as one more lap.
        let b = if b < a { b + self.lap_length() } else { b };
        a.lerp(b, nearest.t) % self.lap_length()
    }

    /// El punto de la línea central más cercano a `along` metros de la meta.
    pub fn index_at(&self, along: f32) -> usize {
        let n = self.centerline.len();
        ((along.rem_euclid(self.lap_length()) / self.step_length()).round() as usize) % n
    }

    /// Distancia real entre puntos consecutivos: casi `SAMPLE_SPACING`,
    /// ajustada para que la vuelta cierre exacta.
    pub fn step_length(&self) -> f32 {
        self.step
    }

    /// Posición y orientación en el punto `index` de la línea central.
    pub fn pose_at(&self, index: usize) -> (Vec2, f32) {
        let (i, j) = (self.wrap(index as isize), self.wrap(index as isize + 1));
        let (point, next) = (self.centerline[i], self.centerline[j]);
        if i == j {
            // The last point of a road off the lap: the way it was going.
            let prev = self.centerline[self.wrap(index as isize - 1)];
            return (point, heading_of(point - prev));
        }
        (point, heading_of(next - point))
    }

    /// Puesto en la parrilla de salida: `row` filas por detrás de la primera
    /// y `lane` metros a la izquierda de la línea central. Para las pruebas, que
    /// colocan motos en cualquier carril.
    #[cfg(any(test, feature = "testing"))]
    pub fn grid_pose(&self, row: usize, lane: f32) -> (Vec2, f32) {
        let index = self.centerline.len() - START_BEHIND_SAMPLES - row * GRID_ROW_SAMPLES;
        let (_, heading) = self.pose_at(index);
        (self.offset_point(index, lane), heading)
    }

    /// Puesto `slot` de la parrilla de salida (0 es la pole), con tres motos
    /// por fila escalonadas.
    pub fn grid_slot(&self, slot: usize) -> (Vec2, f32) {
        let (row, column) = (slot / GRID_LANES.len(), slot % GRID_LANES.len());
        let index = self.centerline.len() - START_BEHIND_SAMPLES - row * GRID_ROW_SAMPLES - column;
        let (_, heading) = self.pose_at(index);
        (self.offset_point(index, GRID_LANES[column]), heading)
    }

    /// Grid slot `slot`, with the grid `along` metres along the lap
    /// instead of behind the finish line: the same formation, for looking
    /// at the scenery part way round (see [`crate::debug_start`]).
    pub fn grid_slot_at(&self, slot: usize, along: f32) -> (Vec2, f32) {
        let (row, column) = (slot / GRID_LANES.len(), slot % GRID_LANES.len());
        let back = START_BEHIND_SAMPLES + row * GRID_ROW_SAMPLES + column;
        let index = self.wrap(self.index_at(along) as isize - back as isize);
        let (_, heading) = self.pose_at(index);
        (self.offset_point(index, GRID_LANES[column]), heading)
    }

    /// How far to the left of the centre line grid slot `slot` is (m).
    pub fn grid_lane(slot: usize) -> f32 {
        GRID_LANES[slot % GRID_LANES.len()]
    }

    /// Índice del punto de la línea central más cercano a `point`.
    #[cfg(any(test, feature = "testing"))]
    pub fn nearest_index(&self, point: Vec2) -> usize {
        self.nearest_index_on(point, None)
    }

    /// Like [`Self::nearest_index`], for something at `height`: where the
    /// track passes over itself, the point of the stretch at its level.
    /// Only the lap counts, not the roads off it.
    pub fn nearest_index_at(&self, point: Vec2, height: f32) -> usize {
        self.nearest_index_on(point, Some(height))
    }

    fn nearest_index_on(&self, point: Vec2, height: Option<f32>) -> usize {
        // On the lap, each point starts a segment, and so is in its squares.
        if !(self.closed && self.searches_grid(point)) {
            return self.nearest_index_every(point, height);
        }
        let closest = |on_level: &dyn Fn(usize) -> bool, reach: f32| {
            self.grid
                .search(
                    point,
                    reach,
                    |distance| distance * distance,
                    |index| on_level(index).then(|| self.centerline[index].distance_squared(point)),
                )
                .map(|(_, index)| index)
        };
        let nearest = closest(&|_| true, f32::INFINITY).unwrap_or(0);
        let reach = self.centerline[nearest].distance(point) + LEVEL_SPREAD;
        height
            .and_then(|height| {
                closest(&|index| on_level_with(self.elevation[index], height), reach)
            })
            .filter(|&index| self.centerline[index].distance(point) <= reach)
            .unwrap_or(nearest)
    }

    /// Like [`Self::nearest_index_on`], going through every point.
    fn nearest_index_every(&self, point: Vec2, height: Option<f32>) -> usize {
        let closest = |on_level: &dyn Fn(usize) -> bool| {
            let mut best: Option<(f32, usize)> = None;
            for (index, at) in self.centerline.iter().enumerate() {
                let distance = at.distance_squared(point);
                if best.is_none_or(|(least, _)| distance < least) && on_level(index) {
                    best = Some((distance, index));
                }
            }
            best.map(|(_, index)| index)
        };
        let nearest = closest(&|_| true).unwrap_or(0);
        let reach = self.centerline[nearest].distance(point) + LEVEL_SPREAD;
        height
            .and_then(|height| closest(&|index| on_level_with(self.elevation[index], height)))
            .filter(|&index| self.centerline[index].distance(point) <= reach)
            .unwrap_or(nearest)
    }

    /// El punto de la línea central (no solo de sus vértices) más cercano,
    /// on the lap or on a road off it.
    /// Seen from above: where the track passes over itself, it may be on
    /// either stretch. For something on the track, see [`Self::nearest_at`].
    pub fn nearest(&self, point: Vec2) -> Nearest {
        self.nearest_on(point, None)
    }

    /// Like [`Self::nearest`], for something at `height` (a bike, its
    /// rider): where the track passes over itself, the point on the stretch
    /// at its level, so a bike in the tunnel underneath stays on its road and
    /// one on the road above stays on that one.
    pub fn nearest_at(&self, point: Vec2, height: f32) -> Nearest {
        self.nearest_on(point, Some(height))
    }

    fn nearest_on(&self, point: Vec2, height: Option<f32>) -> Nearest {
        let roads = || (0..=self.side_roads.len()).map(|road| (road, self.road(road)));
        if !roads().all(|(_, road)| road.searches_grid(point)) {
            return self.nearest_every(point, height);
        }
        // The same as going through every segment of every road: the nearest
        // of each, the first road's of equals; then, if the point is at a
        // height, the nearest on its level that is near enough to count.
        // Each road after the first only needs looking at as far as the
        // nearest found so far: only one nearer can take its place.
        let nearest_where = |reach: f32, keep: &dyn Fn(&Track, &Nearest) -> bool| {
            roads().fold(None, |best: Option<Nearest>, (number, road)| {
                let reach = best.map_or(reach, |best| best.distance.min(reach));
                let here = road
                    .nearest_in_grid(point, reach, |here| keep(road, here))
                    .map(|here| Nearest {
                        road: number,
                        ..here
                    });
                closer(best, here)
            })
        };
        let nearest = nearest_where(f32::INFINITY, &|_, _| true).expect("el trazado tiene puntos");
        let Some(height) = height else {
            return nearest;
        };
        let reach = nearest.distance + LEVEL_SPREAD;
        nearest_where(reach, &|road, here| {
            on_level_with(road.height_on_road(here), height)
        })
        .filter(|level| level.distance <= reach)
        .unwrap_or(nearest)
    }

    /// Whether this road's grid can find the nearest segment to `point`: a
    /// point that is not one (not a number) goes through every segment, as
    /// does a track still being laid.
    fn searches_grid(&self, point: Vec2) -> bool {
        !self.grid.is_empty() && point.is_finite()
    }

    /// The nearest point of this road's segments to `point` among those
    /// `keep` keeps, as on road 0, if any is within `reach` (m); maybe one a
    /// little further. The lowest segment's of equals.
    fn nearest_in_grid(
        &self,
        point: Vec2,
        reach: f32,
        keep: impl Fn(&Nearest) -> bool,
    ) -> Option<Nearest> {
        self.grid
            .search(
                point,
                reach,
                |distance| distance,
                |index| {
                    let here = self.project(index, point);
                    keep(&here).then_some(here.distance)
                },
            )
            .map(|(_, index)| self.project(index, point))
    }

    /// Like [`Self::nearest_on`], going through every segment.
    fn nearest_every(&self, point: Vec2, height: Option<f32>) -> Nearest {
        let (mut nearest, mut on_level) = self.search(point, height, 0);
        for (road, side_road) in self.side_roads.iter().enumerate() {
            let (here, level) = side_road.search(point, height, road + 1);
            nearest = closer(nearest, here);
            on_level = closer(on_level, level);
        }
        let nearest = nearest.expect("el trazado tiene puntos");
        on_level
            .filter(|level| level.distance <= nearest.distance + LEVEL_SPREAD)
            .unwrap_or(nearest)
    }

    /// The nearest point of this road's own centre line to `point`, and the
    /// nearest on the level of `height`, if any (see [`Self::nearest_at`]),
    /// counted as road number `road`.
    fn search(
        &self,
        point: Vec2,
        height: Option<f32>,
        road: usize,
    ) -> (Option<Nearest>, Option<Nearest>) {
        let (mut nearest, mut on_level): (Option<Nearest>, Option<Nearest>) = (None, None);
        for index in 0..self.segment_count() {
            let here = Nearest {
                road,
                ..self.project(index, point)
            };
            let closer =
                |best: &Option<Nearest>| best.is_none_or(|best| here.distance < best.distance);
            if closer(&nearest) {
                nearest = Some(here);
            }
            if let Some(height) = height
                && closer(&on_level)
                && on_level_with(self.height_on_road(&here), height)
            {
                on_level = Some(here);
            }
        }
        (nearest, on_level)
    }

    /// Like [`Self::nearest`], looking only at the `count` segments of the
    /// centre line from `from` on (going round past the finish line).
    pub fn nearest_among(&self, point: Vec2, from: usize, count: usize) -> Nearest {
        let n = self.centerline.len();
        (from..from + count.max(1))
            .map(|index| self.project(index % n, point))
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
            .expect("hay algún tramo")
    }

    /// The point of the segment from `index` to the next point of the centre
    /// line nearest to `point`.
    fn project(&self, index: usize, point: Vec2) -> Nearest {
        let n = self.centerline.len();
        let (a, b) = (self.centerline[index], self.centerline[(index + 1) % n]);
        let ab = b - a;
        let t = ((point - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        let closest = a + ab * t;
        let left = Vec2::new(ab.y, -ab.x).normalize();
        Nearest {
            point: closest,
            distance: point.distance(closest),
            index,
            t,
            lateral: (point - closest).dot(left),
            road: 0,
        }
    }

    /// Altura de la carretera (con su peralte) en un punto. Más allá de los
    /// muros sigue la inclinación que tiene en ellos.
    pub fn road_height(&self, point: Vec2) -> f32 {
        self.height_beside(&self.nearest(point))
    }

    /// Like [`Self::road_height`], under something at `height` (see
    /// [`Self::nearest_at`]).
    pub fn road_height_at(&self, point: Vec2, height: f32) -> f32 {
        self.height_beside(&self.nearest_at(point, height))
    }

    /// Altura de la carretera junto a un punto ya localizado. Where a road
    /// off the lap shares the lap's road, the lap's surface, so that the two
    /// are one.
    pub fn height_beside(&self, nearest: &Nearest) -> f32 {
        let road = self.road(nearest.road);
        let own = road.height_on_road(nearest);
        if road.closed {
            return own;
        }
        let (i, j) = (
            road.wrap(nearest.index as isize),
            road.wrap(nearest.index as isize + 1),
        );
        let apart = road.apart[i].lerp(road.apart[j], nearest.t);
        if apart >= 1.0 {
            return own;
        }
        let across = road.centerline[j] - road.centerline[i];
        let point = nearest.point + Vec2::new(across.y, -across.x).normalize() * nearest.lateral;
        self.height_on_road(&self.project_on_lap(point))
            .lerp(own, apart)
    }

    /// Like [`Self::surface_point`], on road number `road` (see
    /// [`Nearest::road`]): where a road off the lap shares the lap's road, on
    /// the lap's surface.
    pub fn surface_point_on(&self, road: usize, index: usize, lateral: f32) -> Vec3 {
        let side_road = self.road(road);
        let point = side_road.surface_point(index, lateral);
        let apart = side_road
            .apart
            .get(side_road.wrap(index as isize))
            .copied()
            .unwrap_or(1.0);
        if apart >= 1.0 {
            return point;
        }
        let lap = self.height_on_road(&self.project_on_lap(Vec2::new(point.x, point.z)));
        Vec3::new(point.x, lap.lerp(point.y, apart), point.z)
    }

    /// Like [`Self::height_beside`], for a point of this road itself.
    fn height_on_road(&self, nearest: &Nearest) -> f32 {
        let (i, j) = (
            self.wrap(nearest.index as isize),
            self.wrap(nearest.index as isize + 1),
        );
        let elevation = self.elevation[i].lerp(self.elevation[j], nearest.t);
        let bank = self.bank[i].lerp(self.bank[j], nearest.t);
        let lateral = nearest.lateral.clamp(-WALL_DISTANCE, WALL_DISTANCE);
        elevation - lateral * bank.tan()
    }

    /// Which way the road beside a point already found goes, and the point
    /// `lateral` metres to the left of its centre line there.
    pub fn pose_beside(&self, nearest: &Nearest, lateral: f32) -> (Vec2, f32) {
        let road = self.road(nearest.road);
        let (_, heading) = road.pose_at(nearest.index);
        let point = road
            .offset_point(nearest.index, lateral)
            .lerp(road.offset_point(nearest.index + 1, lateral), nearest.t);
        (point, heading)
    }

    /// Half the width of the asphalt of the road beside a point already
    /// found.
    /// Half the width of the asphalt beside a point (m): short of the walls
    /// where they close in on it (see [`Narrowing`]).
    pub fn half_width_beside(&self, nearest: &Nearest) -> f32 {
        self.road(nearest.road).half_width_at(nearest.index)
    }

    /// Half the width of the asphalt at point `index` of the centre line
    /// (m): short of the walls where they close in on it (see
    /// [`Narrowing`]).
    pub fn half_width_at(&self, index: usize) -> f32 {
        self.half_width.min(self.wall_at(index) - NARROW_MARGIN)
    }

    /// Punto de la superficie de la carretera `lateral` metros a la izquierda
    /// del punto `index` de la línea central.
    pub fn surface_point(&self, index: usize, lateral: f32) -> Vec3 {
        let index = self.wrap(index as isize);
        let ground = self.offset_point(index, lateral);
        let height = self.elevation[index] - lateral * self.bank[index].tan();
        Vec3::new(ground.x, height, ground.y)
    }

    /// Alturas de la línea central, una por punto: para calcular el terreno.
    pub fn elevations(&self) -> &[f32] {
        &self.elevation
    }

    pub fn centerline(&self) -> &[Vec2] {
        &self.centerline
    }

    pub fn distance_to_centerline(&self, point: Vec2) -> f32 {
        self.nearest(point).distance
    }

    /// What the ground is beside a point: the asphalt and its curbs, or
    /// past them what the shoulder there is made of (see
    /// [`Self::set_shoulders`]).
    pub fn surface_beside(&self, nearest: &Nearest) -> Surface {
        if nearest.distance <= self.half_width_beside(nearest) + CURB_WIDTH {
            return Surface::Asphalt;
        }
        let road = self.road(nearest.road);
        let index = road.wrap(nearest.index as isize + isize::from(nearest.t >= 0.5));
        let side = usize::from(nearest.lateral < 0.0);
        road.shoulders
            .get(index)
            .map_or(Surface::Grass, |sides| sides[side])
    }

    /// Pendiente de la carretera en la dirección `direction` (unitaria): metros
    /// que sube por cada metro que se avanza, under something at `height`
    /// (see [`Self::nearest_at`]).
    pub fn grade(&self, point: Vec2, direction: Vec2, height: f32) -> f32 {
        const REACH: f32 = 0.75;
        let ahead = self.road_height_at(point + direction * REACH, height);
        let behind = self.road_height_at(point - direction * REACH, height);
        (ahead - behind) / (2.0 * REACH)
    }

    /// Curvatura vertical de la carretera (1/m) junto a un punto: positiva en
    /// los cambios de rasante (lomas), negativa en los valles.
    pub fn crest(&self, nearest: &Nearest) -> f32 {
        let road = self.road(nearest.road);
        let at = |offset: isize| {
            let index = nearest.index as isize + offset;
            let i = road.wrap(index);
            if road.closed || i as isize == index {
                return road.elevation[i];
            }
            // Past either end of a road off the lap, it carries on the way
            // it went: held flat, a climb onto the lap would look like a
            // crest to take off from.
            let inwards = if index < 0 { i + 1 } else { i - 1 };
            let past = (index - i as isize).abs() as f32;
            road.elevation[i] + (road.elevation[i] - road.elevation[inwards]) * past
        };
        let span = 2.0 * road.step_length();
        -(at(2) - 2.0 * at(0) + at(-2)) / (span * span)
    }

    /// Punto desplazado `lateral` metros de la línea central (positivo = izquierda).
    pub fn offset_point(&self, index: usize, lateral: f32) -> Vec2 {
        let index = self.wrap(index as isize);
        self.centerline[index] + self.left_normal(index) * lateral
    }

    /// Curvatura en el punto `index`: 1/radio, positiva en curvas a la izquierda.
    pub fn curvature(&self, index: usize) -> f32 {
        let index = self.wrap(index as isize);
        let (before, after) = (self.wrap(index as isize - 1), self.wrap(index as isize + 1));
        if before == index || after == index {
            // The ends of a road off the lap.
            return 0.0;
        }
        let prev = self.centerline[before];
        let point = self.centerline[index];
        let next = self.centerline[after];
        let turn = wrap_angle(heading_of(next - point) - heading_of(point - prev));
        turn / (0.5 * (prev.distance(point) + point.distance(next)))
    }

    /// Curvatura sin signo media alrededor de un punto: la de un solo punto
    /// tiene ruido.
    pub fn smoothed_curvature(&self, index: usize, window: usize) -> f32 {
        let (index, window) = (index as isize, window as isize);
        let range = (index - window)..=(index + window);
        let count = range.clone().count() as f32;
        range
            .map(|i| self.curvature(self.wrap(i)).abs())
            .sum::<f32>()
            / count
    }

    /// Esquinas del rectángulo que contiene la línea central, and those of
    /// the roads off it.
    pub fn bounds(&self) -> (Vec2, Vec2) {
        self.side_roads
            .iter()
            .flat_map(|road| &road.centerline)
            .chain(&self.centerline)
            .fold(
                (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                |(min, max), &point| (min.min(point), max.max(point)),
            )
    }

    fn segments(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        let next = self.centerline.iter().cycle().skip(1);
        self.centerline
            .iter()
            .copied()
            .zip(next.copied())
            .take(self.segment_count())
    }

    /// Normal unitaria hacia la izquierda en el punto `index`.
    fn left_normal(&self, index: usize) -> Vec2 {
        let prev = self.centerline[self.wrap(index as isize - 1)];
        let next = self.centerline[self.wrap(index as isize + 1)];
        let tangent = (next - prev).normalize();
        Vec2::new(tangent.y, -tangent.x)
    }
}

/// The nearer of two points found on the centre lines, if any.
fn closer(a: Option<Nearest>, b: Option<Nearest>) -> Option<Nearest> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.distance < a.distance { b } else { a }),
        (a, b) => a.or(b),
    }
}

/// How far above or below the road something on it can be and still count
/// as on it (m): a bike in the air over a crest, a rider thrown up. Where
/// the track passes over itself, the two stretches are much further apart.
const LEVEL_REACH: f32 = 6.0;
/// How much further than the nearest road the road at something's level
/// may be and still be the one under it (m): where the track passes over
/// itself, the two stretches are close. Something high in the air (falling
/// off a loop) is not over some far stretch that happens to be that high.
const LEVEL_SPREAD: f32 = 2.0 * WALL_DISTANCE;

/// Whether something at `height` is on the level of road at `road` (see
/// [`Track::nearest_at`]).
fn on_level_with(road: f32, height: f32) -> bool {
    (road - height).abs() <= LEVEL_REACH
}

/// Height `u` (0..1) of the way along a smooth curve (a cubic) that leaves
/// `from` level and reaches `to` going up `rise` metres per length of curve.
fn ease(from: f32, to: f32, rise: f32, u: f32) -> f32 {
    let (u2, u3) = (u * u, u * u * u);
    (2.0 * u3 - 3.0 * u2 + 1.0) * from + (3.0 * u2 - 2.0 * u3) * to + (u3 - u2) * rise
}

/// The height at `s` of an open profile `length` metres long through
/// `points` (metres along it, height), level at zero at both ends: a smooth
/// curve (Hermite with Catmull-Rom tangents) through them all.
fn open_profile(points: &[(f32, f32)], length: f32, s: f32) -> f32 {
    let all: Vec<(f32, f32)> = std::iter::once((0.0, 0.0))
        .chain(points.iter().copied())
        .chain(std::iter::once((length, 0.0)))
        .collect();
    let s = s.clamp(0.0, length);
    let i = all
        .windows(2)
        .position(|pair| s <= pair[1].0)
        .unwrap_or(all.len() - 2);
    let tangent = |k: usize| {
        if k == 0 || k == all.len() - 1 {
            0.0
        } else {
            (all[k + 1].1 - all[k - 1].1) / (all[k + 1].0 - all[k - 1].0)
        }
    };
    let (p1, p2) = (all[i], all[i + 1]);
    let span = (p2.0 - p1.0).max(1e-3);
    let (m1, m2) = (tangent(i) * span, tangent(i + 1) * span);
    let t = (s - p1.0) / span;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * p1.1
        + (t3 - 2.0 * t2 + t) * m1
        + (-2.0 * t3 + 3.0 * t2) * p2.1
        + (t3 - t2) * m2
}

/// Altura de un perfil cerrado de `lap` metros en el punto `s`, con una curva
/// suave (Hermite con tangentes de Catmull-Rom) que pasa por los puntos dados.
fn profile_height(points: &[(f32, f32)], lap: f32, s: f32) -> f32 {
    let n = points.len();
    // Puntos del perfil como si se repitiera vuelta tras vuelta.
    let at = |i: isize| {
        let wrapped = i.rem_euclid(n as isize) as usize;
        let laps = i.div_euclid(n as isize) as f32;
        (points[wrapped].0 + laps * lap, points[wrapped].1)
    };
    let s = s.rem_euclid(lap);
    let i = points.iter().rposition(|&(at, _)| at <= s).unwrap_or(0) as isize;
    let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
    let span = p2.0 - p1.0;
    let tangent1 = (p2.1 - p0.1) / (p2.0 - p0.0) * span;
    let tangent2 = (p3.1 - p1.1) / (p3.0 - p1.0) * span;
    let t = (s - p1.0) / span;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * p1.1
        + (t3 - 2.0 * t2 + t) * tangent1
        + (-2.0 * t3 + 3.0 * t2) * p2.1
        + (t3 - t2) * tangent2
}

/// Points about `spacing` metres apart along the smooth closed curve through
/// `points`, drawn like the centre line of a track: for coastlines.
/// `values` round a lap eased in and out: at each point the most of them
/// within twice [`LIFT_SMOOTHING`] points, averaged over as many either
/// side twice. No less than they were anywhere, and smooth, its grade too.
fn eased(values: &[f32]) -> Vec<f32> {
    let n = values.len() as isize;
    let at = |values: &[f32], i: isize| values[i.rem_euclid(n) as usize];
    let reach = LIFT_SMOOTHING as isize;
    let most: Vec<f32> = (0..n)
        .map(|i| {
            (i - 2 * reach..=i + 2 * reach)
                .map(|j| at(values, j))
                .fold(0.0, f32::max)
        })
        .collect();
    let average = |values: &[f32]| -> Vec<f32> {
        (0..n)
            .map(|i| {
                (i - reach..=i + reach).map(|j| at(values, j)).sum::<f32>() / (2 * reach + 1) as f32
            })
            .collect()
    };
    average(&average(&most))
}

pub fn closed_curve(points: &[Vec2], spacing: f32) -> Vec<Vec2> {
    resample(&sample_spline(points), spacing)
}

/// Points about [`SAMPLE_SPACING`] apart (adjusted so that the ends fall on
/// points) along the smooth curve from the first of `points` to the last,
/// through the others, drawn like the centre line of the lap. It leaves the
/// first as if it came from `before` and reaches the last as if it went on
/// to `after`.
fn open_curve(points: &[Vec2], before: Vec2, after: Vec2) -> Vec<Vec2> {
    const STEPS_PER_SEGMENT: usize = 512;
    let all: Vec<Vec2> = std::iter::once(before)
        .chain(points.iter().copied())
        .chain(std::iter::once(after))
        .collect();
    let mut samples = Vec::with_capacity(all.len() * STEPS_PER_SEGMENT);
    for w in all.windows(4) {
        for step in 0..STEPS_PER_SEGMENT {
            let u = step as f32 / STEPS_PER_SEGMENT as f32;
            samples.push(catmull_rom(w[0], w[1], w[2], w[3], u));
        }
    }
    samples.push(all[all.len() - 2]);
    let lengths: Vec<f32> = samples.windows(2).map(|w| w[0].distance(w[1])).collect();
    let total: f32 = lengths.iter().sum();
    let count = ((total / SAMPLE_SPACING).round() as usize).max(1);
    let step = total / count as f32;
    let mut resampled = Vec::with_capacity(count + 1);
    let (mut segment, mut segment_start) = (0, 0.0);
    for k in 0..count {
        let target = k as f32 * step;
        while segment < lengths.len() - 1 && segment_start + lengths[segment] < target {
            segment_start += lengths[segment];
            segment += 1;
        }
        let t = ((target - segment_start) / lengths[segment].max(1e-6)).clamp(0.0, 1.0);
        resampled.push(samples[segment].lerp(samples[segment + 1], t));
    }
    resampled.push(samples[samples.len() - 1]);
    resampled
}

/// Muestrea densamente la curva Catmull-Rom centrípeta cerrada que pasa por
/// `points`. La variante centrípeta no hace bucles raros aunque los puntos
/// estén a distancias muy distintas entre sí.
fn sample_spline(points: &[Vec2]) -> Vec<Vec2> {
    const STEPS_PER_SEGMENT: usize = 512;
    let n = points.len();
    let mut samples = Vec::with_capacity(n * STEPS_PER_SEGMENT);
    for i in 0..n {
        let p0 = points[(i + n - 1) % n];
        let p1 = points[i];
        let p2 = points[(i + 1) % n];
        let p3 = points[(i + 2) % n];
        for step in 0..STEPS_PER_SEGMENT {
            let u = step as f32 / STEPS_PER_SEGMENT as f32;
            samples.push(catmull_rom(p0, p1, p2, p3, u));
        }
    }
    samples
}

/// Punto en `u ∈ [0, 1)` del tramo entre `p1` y `p2` (algoritmo de Barry-Goldman).
fn catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, u: f32) -> Vec2 {
    let t0 = 0.0;
    let t1 = t0 + p0.distance(p1).sqrt();
    let t2 = t1 + p1.distance(p2).sqrt();
    let t3 = t2 + p2.distance(p3).sqrt();
    let t = t1.lerp(t2, u);
    let a1 = (p0 * (t1 - t) + p1 * (t - t0)) / (t1 - t0);
    let a2 = (p1 * (t2 - t) + p2 * (t - t1)) / (t2 - t1);
    let a3 = (p2 * (t3 - t) + p3 * (t - t2)) / (t3 - t2);
    let b1 = (a1 * (t2 - t) + a2 * (t - t0)) / (t2 - t0);
    let b2 = (a2 * (t3 - t) + a3 * (t - t1)) / (t3 - t1);
    (b1 * (t2 - t) + b2 * (t - t1)) / (t2 - t1)
}

/// Recorre el polígono cerrado `points` y devuelve puntos equiespaciados a unos
/// `spacing` metros (ajustado para que la vuelta cierre exacta).
fn resample(points: &[Vec2], spacing: f32) -> Vec<Vec2> {
    let n = points.len();
    let lengths: Vec<f32> = (0..n)
        .map(|i| points[i].distance(points[(i + 1) % n]))
        .collect();
    let total: f32 = lengths.iter().sum();
    let count = (total / spacing).round() as usize;
    let step = total / count as f32;

    let mut resampled = Vec::with_capacity(count);
    let (mut segment, mut segment_start) = (0, 0.0);
    for k in 0..count {
        let target = k as f32 * step;
        while segment < n - 1 && segment_start + lengths[segment] < target {
            segment_start += lengths[segment];
            segment += 1;
        }
        let t = ((target - segment_start) / lengths[segment]).clamp(0.0, 1.0);
        resampled.push(points[segment].lerp(points[(segment + 1) % n], t));
    }
    resampled
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    fn track() -> Track {
        Track::default()
    }

    /// Todas las pistas del juego, con su nombre.
    pub fn every_track(circuits: &'static crate::circuits::Circuits) -> Vec<(&'static str, Track)> {
        circuits
            .iter()
            .map(|circuit| (circuit.name, Track::new(&circuit.layout)))
            .collect()
    }

    /// Whether two points found on the centre lines are the very same.
    fn same(a: &Nearest, b: &Nearest) -> bool {
        a.road == b.road
            && a.index == b.index
            && a.point == b.point
            && a.distance.to_bits() == b.distance.to_bits()
            && a.t.to_bits() == b.t.to_bits()
            && a.lateral.to_bits() == b.lateral.to_bits()
    }

    pub fn the_grid_finds_what_going_through_every_segment_finds_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for (name, track) in every_track(circuits) {
            let mut checked = 0;
            for road in 0..=track.side_roads().len() {
                let line = track.road(road);
                // Along every road, on it, at its walls, past them and far
                // off; at its height, up in the air, and at other roads'
                // heights, which is where the track passes over itself.
                for index in (0..line.segment_count()).step_by(3) {
                    for (k, lateral) in [0.0, 3.3, -6.1, 11.4, -15.0, 40.0, -160.0]
                        .into_iter()
                        .enumerate()
                    {
                        let point = line.offset_point(index, lateral)
                            + Vec2::new(0.37, -0.21) * (index % 5) as f32;
                        let ground = line.elevation[index];
                        let other = track.elevation[(index * 7 + k * 131) % track.elevation.len()];
                        let nearest = track.nearest(point);
                        assert!(
                            same(&nearest, &track.nearest_every(point, None)),
                            "{name}: {point}"
                        );
                        for height in [ground, ground + 4.0, ground + 25.0, other, other - 3.0] {
                            let grid = track.nearest_at(point, height);
                            let every = track.nearest_every(point, Some(height));
                            assert!(
                                same(&grid, &every),
                                "{name}: {point} at {height}: {grid:?} {every:?}"
                            );
                            assert_eq!(
                                track.nearest_index_at(point, height),
                                track.nearest_index_every(point, Some(height)),
                                "{name}: {point} at {height}"
                            );
                        }
                        assert!(
                            same(
                                &track.project_on_lap(point),
                                &track.project_on_lap_every(point)
                            ),
                            "{name}: {point}"
                        );
                        checked += 1;
                    }
                }
            }
            assert!(checked > 1000, "{name}: {checked}");
        }
    }

    #[test]
    fn the_grid_finds_what_going_through_every_segment_finds() {
        the_grid_finds_what_going_through_every_segment_finds_on(&crate::testing::DEMO);
    }

    pub fn the_grid_finds_the_points_of_segments_corners_as_the_search_did_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // On a corner's point, two segments are as near: the first counts,
        // in the grid as when every segment was looked at.
        for (name, track) in every_track(circuits) {
            for index in (0..track.segment_count()).step_by(5) {
                let point = track.centerline[index];
                let height = track.elevation[index];
                assert!(
                    same(
                        &track.nearest_at(point, height),
                        &track.nearest_every(point, Some(height))
                    ),
                    "{name}: {index}"
                );
                assert_eq!(track.nearest_index_at(point, height), index, "{name}");
            }
        }
    }

    #[test]
    fn the_grid_finds_the_points_of_segments_corners_as_the_search_did() {
        the_grid_finds_the_points_of_segments_corners_as_the_search_did_on(&crate::testing::DEMO);
    }

    /// How long finding a bike on each track takes, with the grid and going
    /// through every segment: `cargo test --release -- --ignored --nocapture
    /// time_to_find`.
    pub fn time_to_find_a_bike_on_each_track_on(circuits: &'static crate::circuits::Circuits) {
        use std::hint::black_box;
        use std::time::Instant;
        for (name, track) in every_track(circuits) {
            let points: Vec<(Vec2, f32)> = (0..track.segment_count())
                .step_by(7)
                .map(|index| (track.offset_point(index, 2.5), track.elevation[index] + 0.5))
                .collect();
            let time = |find: &dyn Fn(Vec2, f32) -> Nearest| {
                let started = Instant::now();
                for _ in 0..20 {
                    for &(point, height) in &points {
                        black_box(find(black_box(point), height));
                    }
                }
                started.elapsed().as_secs_f64() * 1e6 / (20 * points.len()) as f64
            };
            let grid = time(&|point, height| track.nearest_at(point, height));
            let every = time(&|point, height| track.nearest_every(point, Some(height)));
            println!(
                "{name:14} {:5} segments: grid {grid:6.2} µs, every segment {every:7.2} µs ({:.0}×)",
                track.segment_count(),
                every / grid
            );
        }
    }

    #[test]
    #[ignore = "a measurement, not a check"]
    fn time_to_find_a_bike_on_each_track() {
        time_to_find_a_bike_on_each_track_on(&crate::testing::DEMO);
    }

    #[test]
    fn a_point_that_is_not_one_still_finds_some_road() {
        let track = track();
        let point = Vec2::new(f32::NAN, 3.0);
        // Not a number is not even equal to itself: the segment found is.
        let (grid, every) = (track.nearest(point), track.nearest_every(point, None));
        assert_eq!((grid.road, grid.index), (every.road, every.index));
        assert_eq!(
            track.nearest_index_at(point, 2.0),
            track.nearest_index_every(point, Some(2.0))
        );
    }

    #[test]
    fn centerline_is_evenly_spaced_and_closed() {
        let track = track();
        for (a, b) in track.segments() {
            let length = a.distance(b);
            assert!(
                (length - SAMPLE_SPACING).abs() < 0.1 * SAMPLE_SPACING,
                "tramo de {length} m"
            );
        }
    }

    #[test]
    fn the_grid_has_three_staggered_bikes_per_row_on_the_road() {
        let track = track();
        let poses: Vec<_> = (0..8).map(|slot| track.grid_slot(slot)).collect();
        for (slot, (point, _)) in poses.iter().enumerate() {
            assert!(
                track.distance_to_centerline(*point) < track.half_width() - 1.0,
                "puesto {slot} en el asfalto"
            );
            for (other, (point2, _)) in poses.iter().enumerate().skip(slot + 1) {
                assert!(
                    point.distance(*point2) > 3.0,
                    "puestos {slot} y {other} separados"
                );
            }
        }
        // Cada moto, por detrás de la anterior: la pole delante de todas.
        let start = track.centerline().len() - 1;
        let behind = |slot: usize| {
            let index = track.nearest_index(poses[slot].0);
            (start + track.centerline().len() - index) % track.centerline().len()
        };
        assert!((1..8).all(|slot| behind(slot) > behind(slot - 1)));
    }

    #[test]
    fn the_grid_can_be_moved_along_the_lap_keeping_its_formation() {
        let track = track();
        let lap = track.lap_length();
        for along in [0.0, 300.0, lap / 2.0, lap - 10.0] {
            let moved: Vec<_> = (0..8).map(|slot| track.grid_slot_at(slot, along)).collect();
            // The pole stands just behind the distance asked, where the
            // grid stands behind the finish line, and on the asphalt.
            let pole = track.nearest(moved[0].0);
            let behind = (along - track.along(&pole)).rem_euclid(lap);
            assert!(
                behind < (START_BEHIND_SAMPLES + 2) as f32 * SAMPLE_SPACING,
                "the pole is {behind} m behind {along} m"
            );
            for (slot, (point, _)) in moved.iter().enumerate() {
                assert!(
                    track.distance_to_centerline(*point) < track.half_width() - 1.0,
                    "slot {slot} off the asphalt"
                );
                for (other, (point2, _)) in moved.iter().enumerate().skip(slot + 1) {
                    assert!(
                        point.distance(*point2) > 3.0,
                        "slots {slot} and {other} on top of each other"
                    );
                }
            }
        }
        // Moved by the lap, it is the grid as it always was.
        for slot in 0..8 {
            let (point, heading) = track.grid_slot(slot);
            let (moved, turned) = track.grid_slot_at(slot, lap);
            assert!(point.distance(moved) < 1e-3, "slot {slot}");
            assert!((heading - turned).abs() < 1e-3, "slot {slot}");
        }
    }

    pub fn curves_are_drivable_on(circuits: &'static crate::circuits::Circuits) {
        // Por debajo de este radio el muro del interior de la curva se doblaría
        // sobre sí mismo, y además no habría quien la tomase: two and a half
        // times as far as the walls stand from the centre line.
        for (name, track) in every_track(circuits) {
            let min_radius = 2.5 * track.wall();
            for index in 0..track.centerline.len() {
                let radius = 1.0 / track.curvature(index).abs();
                assert!(
                    radius > min_radius,
                    "curva de radio {radius} m en {index} de {name}"
                );
            }
        }
    }

    #[test]
    fn curves_are_drivable() {
        curves_are_drivable_on(&crate::testing::DEMO);
    }

    pub fn walls_of_distant_stretches_do_not_meet_on(circuits: &'static crate::circuits::Circuits) {
        for circuit in circuits.iter() {
            let (name, track) = (circuit.name, Track::new(&circuit.layout));
            let n = track.centerline.len();
            let lap = track.lap_length();
            let in_tunnel = |i: usize| {
                let along = i as f32 * track.step;
                circuit
                    .scenery
                    .tunnels
                    .iter()
                    .any(|span| span.contains(along, lap))
            };
            // Puntos cercanos sobre el trazado pueden estar cerca en el
            // espacio; más allá, entre sus muros tiene que haber sitio para el
            // escenario.
            let neighborhood = (8.0 * track.wall() / SAMPLE_SPACING) as usize;
            let clearance = 2.0 * track.wall() + 10.0;
            for i in 0..n {
                for j in i + neighborhood..n.min(i + n - neighborhood) {
                    let distance = track.centerline[i].distance(track.centerline[j]);
                    if distance > clearance {
                        continue;
                    }
                    // Unless one passes over the other, which goes through a
                    // tunnel well below.
                    let (low, high) = if track.elevation[i] < track.elevation[j] {
                        (i, j)
                    } else {
                        (j, i)
                    };
                    assert!(
                        in_tunnel(low) && track.elevation[high] - track.elevation[low] > 12.0,
                        "los puntos {i} y {j} de {name} se tocan"
                    );
                }
            }
        }
    }

    #[test]
    fn walls_of_distant_stretches_do_not_meet() {
        walls_of_distant_stretches_do_not_meet_on(&crate::testing::DEMO);
    }

    pub fn start_faces_down_the_main_straight_on(circuits: &'static crate::circuits::Circuits) {
        for (name, track) in every_track(circuits) {
            let (position, heading) = track.grid_pose(0, 0.0);
            assert!(position.x.abs() < 1.0, "{name}");
            assert!(
                position.y > 0.0,
                "la salida está por detrás de la meta en {name}"
            );
            assert!(heading.abs() < 0.05, "la recta va hacia -Z en {name}");
        }
    }

    #[test]
    fn start_faces_down_the_main_straight() {
        start_faces_down_the_main_straight_on(&crate::testing::DEMO);
    }

    #[test]
    fn surface_changes_at_the_curb() {
        let track = track();
        let edge = track.half_width + CURB_WIDTH;
        assert_eq!(
            track.surface_beside(&track.nearest(track.offset_point(100, 0.0))),
            Surface::Asphalt
        );
        assert_eq!(
            track.surface_beside(&track.nearest(track.offset_point(100, edge - 0.1))),
            Surface::Asphalt
        );
        assert_eq!(
            track.surface_beside(&track.nearest(track.offset_point(100, -edge - 0.5))),
            Surface::Grass
        );
    }

    #[test]
    fn the_shoulders_say_what_lies_past_the_curbs() {
        let mut track = track();
        let mut shoulders = vec![[Surface::Grass; 2]; track.sample_count()];
        shoulders[100] = [Surface::Paved, Surface::Dirt(Dust::Sand)];
        track.set_shoulders(vec![shoulders]);
        let edge = track.half_width + CURB_WIDTH;
        let beside = |index: usize, lateral: f32| {
            track.surface_beside(&track.nearest(track.offset_point(index, lateral)))
        };
        assert_eq!(beside(100, edge + 1.0), Surface::Paved);
        assert_eq!(beside(100, -edge - 1.0), Surface::Dirt(Dust::Sand));
        assert_eq!(beside(100, edge - 0.1), Surface::Asphalt);
        assert_eq!(beside(110, edge + 1.0), Surface::Grass);
        assert!(Surface::Grass.is_rough() && Surface::Dirt(Dust::Earth).is_rough());
        assert!(!Surface::Paved.is_rough() && !Surface::Asphalt.is_rough());
    }

    /// The tracks with branches, with their names.
    pub fn tracks_with_branches(
        circuits: &'static crate::circuits::Circuits,
    ) -> Vec<(&'static str, Track)> {
        let tracks: Vec<_> = every_track(circuits)
            .into_iter()
            .filter(|(_, track)| !track.loops.is_empty())
            .collect();
        assert!(!tracks.is_empty(), "some track has a branch");
        tracks
    }

    pub fn a_branch_leaves_the_lap_and_joins_it_again_along_it_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for (name, track) in tracks_with_branches(circuits) {
            for looped in &track.loops {
                let (into, out) = (track.road(looped.approach), track.road(looped.exit));
                // It starts on the lap, going its way, and ends on it too.
                for (road, index) in [(into, 0), (out, out.sample_count() - 1)] {
                    let point = road.centerline[index];
                    let on_lap = track.project_on_lap(point);
                    assert!(on_lap.distance < 0.5, "{name}: {}", on_lap.distance);
                    let (_, lap_heading) = track.pose_at(on_lap.index);
                    let (_, heading) = road.pose_at(index);
                    let turn = wrap_angle(heading - lap_heading).abs();
                    assert!(turn < 0.08, "{name}: {turn} rad off the lap");
                    // On the lap's surface, height and banking.
                    let lap_height = track.height_on_road(&on_lap);
                    for lateral in [-4.0, 0.0, 4.0] {
                        let here = road.surface_point(index, lateral);
                        let lap = track.road_height(Vec2::new(here.x, here.z));
                        assert!((here.y - lap).abs() < 0.05, "{name}: {} m", here.y - lap);
                    }
                    assert!((road.elevation[index] - lap_height).abs() < 0.01);
                }
                // Into the loop and out of it, straight along it, at its height.
                let mouth = into.sample_count() - 1;
                assert!(into.centerline[mouth].distance(looped.mouth) < 0.01);
                assert!(out.centerline[0].distance(looped.way_out()) < 0.01);
                for (road, index) in [(into, mouth - 5), (out, 5)] {
                    let (_, heading) = road.pose_at(index);
                    assert!(wrap_angle(heading - looped.heading).abs() < 0.01, "{name}");
                    assert!((road.elevation[index] - looped.height).abs() < 0.01);
                    assert!(road.bank[index].abs() < 1e-3);
                    assert!((road.wall_at(index) - LOOP_WALL).abs() < 1e-3);
                }
            }
        }
    }

    pub fn branches_are_rideable_and_clear_of_the_lap_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for (name, track) in tracks_with_branches(circuits) {
            let (lap_wall, min_radius) = (track.wall(), 2.5 * track.wall());
            for looped in &track.loops {
                for (leaving_lap, number) in [(true, looped.approach), (false, looped.exit)] {
                    let road = track.road(number);
                    let n = road.sample_count();
                    for i in 0..n - 1 {
                        let radius = 1.0 / road.curvature(i).abs();
                        assert!(
                            radius > min_radius,
                            "{name}, road {number}: {radius} m at {i}"
                        );
                        let grade = (road.elevation[i + 1] - road.elevation[i]) / road.step;
                        assert!(grade.abs() < 0.13, "{name}, road {number}: {grade} at {i}");
                        assert!(road.bank[i].abs() <= MAX_BANK);
                    }
                    // From where it leaves the lap (or joins it) the two share
                    // the road between their walls, which stay as wide as the
                    // lap's, until they part; from there on their walls stay
                    // apart.
                    let from_lap: Vec<usize> = if leaving_lap {
                        (0..n).collect()
                    } else {
                        (0..n).rev().collect()
                    };
                    let mut parted = false;
                    for i in from_lap {
                        let lap = track.project_on_lap(road.centerline[i]).distance;
                        let apart = lap > lap_wall + road.wall_at(i) + 2.0;
                        assert!(
                            apart || !parted,
                            "{name}, road {number}: {lap} m from the lap at {i}"
                        );
                        parted |= apart;
                        // Where they share it, their walls are as far out, so
                        // the road between them is the same seen from either.
                        if lap < lap_wall + road.wall_at(i) {
                            assert!(road.wall_at(i) > lap_wall - 0.25, "{name}: {i}");
                        }
                    }
                    assert!(parted, "{name}, road {number}");
                }
            }
        }
    }

    pub fn a_boosted_loop_has_a_booster_on_its_lead_in_and_one_beside_it_on_the_lap_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut boosted = 0;
        for (name, track) in every_track(circuits) {
            let loops: Vec<&Loop> = track.loops().iter().filter(|l| l.boosters).collect();
            assert_eq!(track.boosters().len(), 2 * loops.len(), "{name}");
            for looped in loops {
                boosted += 1;
                let on = |road: usize| {
                    *track
                        .boosters()
                        .iter()
                        .find(|booster| booster.road == road)
                        .unwrap_or_else(|| panic!("{name}: none on road {road}"))
                };
                let (into, beside) = (on(looped.approach), on(0));
                // On the straight lead into the loop, all of it.
                let approach = track.road(looped.approach);
                let length = approach.length();
                assert!(into.from >= length - 2.0 * LOOP_LEAD && into.to <= length - LOOP_LEAD);
                // Driving the bikes on there, across the road, and not
                // before nor after; and on the lap as far along.
                let middle = |booster: Booster| {
                    let road = track.road(booster.road);
                    let index = ((booster.from + booster.to) / 2.0 / road.step) as usize;
                    road.centerline[index]
                };
                for (booster, road) in [(into, approach), (beside, &track)] {
                    let index = ((booster.from + booster.to) / 2.0 / road.step) as usize;
                    for lateral in [-3.0, 0.0, 3.0] {
                        let point = road.offset_point(index, lateral);
                        let nearest = track.nearest_at(point, road.elevation[index]);
                        assert_eq!(track.booster_beside(&nearest), BOOSTER_SPEED, "{name}");
                    }
                    for off in [-10.0, 10.0 + BOOSTER_LENGTH] {
                        let index = ((booster.from + off) / road.step) as usize;
                        let point = road.centerline[index];
                        let nearest = track.nearest_at(point, road.elevation[index]);
                        assert_eq!(track.booster_beside(&nearest), 0.0, "{name}: {off}");
                    }
                }
                let along = |booster: Booster| {
                    let nearest = track.project_on_lap(middle(booster));
                    (nearest.index as f32 + nearest.t) * track.step
                };
                assert!((along(into) - along(beside)).abs() < 3.0, "{name}");
            }
        }
        assert!(boosted > 0);
    }

    pub fn branches_climb_and_drop_smoothly_on_their_own_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // Where a road off the lap has a surface of its own, it has no ramp
        // or dip sharper than the crests of the lap.
        for (name, track) in tracks_with_branches(circuits) {
            for looped in &track.loops {
                for number in [looped.approach, looped.exit] {
                    let road = track.road(number);
                    let height = |i: usize| track.surface_point_on(number, i, 0.0).y;
                    let span = 2.0 * road.step;
                    for i in 2..road.sample_count() - 2 {
                        if road.apart[i] == 0.0 {
                            continue;
                        }
                        let grade = (height(i + 1) - height(i)) / road.step;
                        assert!(grade.abs() < 0.06, "{name}, road {number}: {grade} at {i}");
                        let bend =
                            (height(i + 2) - 2.0 * height(i) + height(i - 2)) / (span * span);
                        assert!(bend.abs() < 0.006, "{name}, road {number}: {bend} at {i}");
                    }
                }
            }
        }
    }

    pub fn bikes_on_a_branch_find_its_road_on(circuits: &'static crate::circuits::Circuits) {
        for (name, track) in tracks_with_branches(circuits) {
            for looped in &track.loops {
                let into = track.road(looped.approach);
                let index = into.sample_count() - 20;
                let point = into.offset_point(index, 2.0);
                let height = track.surface_point_on(looped.approach, index, 2.0).y;
                let nearest = track.nearest_at(point, height);
                assert_eq!(nearest.road, looped.approach, "{name}");
                assert!((nearest.lateral - 2.0).abs() < 0.05);
                assert!((track.height_beside(&nearest) - height).abs() < 0.05);
                // Its walls close in towards the loop (unless the lap's are
                // as close), and it counts as a stretch of the lap further on
                // than its start.
                assert!(track.edge_beside(&nearest) < track.wall().max(LOOP_WALL + 0.01));
                let start = track.nearest_at(into.centerline[0], into.elevation[0]);
                assert!(track.along(&nearest) > track.along(&start));
                let (_, heading) = track.pose_beside(&nearest, 0.0);
                assert!(wrap_angle(heading - looped.heading).abs() < 0.05);
            }
        }
    }

    /// The tracks with branches without a loop, with their names.
    pub fn tracks_with_forks(
        circuits: &'static crate::circuits::Circuits,
    ) -> Vec<(&'static str, Track)> {
        let tracks: Vec<_> = every_track(circuits)
            .into_iter()
            .filter(|(_, track)| !track.forks.is_empty())
            .collect();
        assert!(!tracks.is_empty(), "some track has a fork");
        tracks
    }

    pub fn a_fork_leaves_the_lap_and_joins_it_again_past_rock_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        for (name, track) in tracks_with_forks(circuits) {
            let (lap_wall, min_radius) = (track.wall(), 2.5 * track.wall());
            for fork in &track.forks {
                let road = track.road(fork.road);
                let n = road.sample_count();
                // It starts on the lap, going its way, and ends on it too,
                // on its surface.
                for (index, at) in [(0, fork.fork), (n - 1, fork.join)] {
                    let on_lap = track.project_on_lap(road.centerline[index]);
                    assert!(on_lap.distance < 0.5, "{name}: {}", on_lap.distance);
                    assert!(on_lap.index.abs_diff(at) <= 1, "{name}: {}", on_lap.index);
                    let (_, lap_heading) = track.pose_at(on_lap.index);
                    let (_, heading) = road.pose_at(index);
                    let turn = wrap_angle(heading - lap_heading).abs();
                    assert!(turn < 0.08, "{name}: {turn} rad off the lap");
                    let lap_height = track.height_on_road(&on_lap);
                    assert!((road.elevation[index] - lap_height).abs() < 0.05);
                }
                // Rideable, and between walls as wide as the lap's where the
                // two share the road; further on, rock between them.
                let mut widest = 0.0f32;
                for i in 0..n - 1 {
                    let radius = 1.0 / road.curvature(i).abs();
                    assert!(radius > min_radius, "{name}: {radius} m at {i}");
                    let grade = (road.elevation[i + 1] - road.elevation[i]) / road.step;
                    assert!(grade.abs() < 0.12, "{name}: {grade} at {i}");
                    assert!(road.bank[i].abs() <= MAX_BANK);
                    // Its climbs and drops, as ridden, are rideable, with no
                    // crest sharp enough to take off from flat out.
                    let height = |k: usize| track.surface_point_on(fork.road, k, 0.0).y;
                    let surface = (height(i + 1) - height(i)) / road.step;
                    assert!(surface.abs() < 0.12, "{name}: {surface} at {i}");
                    if (2..n - 2).contains(&i) {
                        let span = 2.0 * road.step;
                        let bend =
                            (height(i + 2) - 2.0 * height(i) + height(i - 2)) / (span * span);
                        assert!(bend > -0.0035, "{name}: crest of {bend} at {i}");
                        let crest = track.crest(&Nearest {
                            point: road.centerline[i],
                            distance: 0.0,
                            index: i,
                            t: 0.0,
                            lateral: 0.0,
                            road: fork.road,
                        });
                        assert!(crest < 0.0035, "{name}: crest of {crest} at {i}");
                    }
                    let lap = track.project_on_lap(road.centerline[i]);
                    if lap.distance > 1.0 {
                        assert_eq!(lap.lateral.signum(), fork.side, "{name}: {i}");
                    }
                    if lap.distance < lap_wall + road.wall_at(i) {
                        assert!(road.wall_at(i) > lap_wall - 0.25, "{name}: {i}");
                    }
                    widest = widest.max(lap.distance - lap_wall - road.wall_at(i));
                }
                assert!(widest > 10.0, "{name}: {widest} m of rock at most");
                // A bike half way along finds its road, counted as a stretch
                // of the lap between the fork and the join.
                let index = n / 2;
                let point = road.offset_point(index, 2.0);
                let height = track.surface_point_on(fork.road, index, 2.0).y;
                let nearest = track.nearest_at(point, height);
                assert_eq!(nearest.road, fork.road, "{name}");
                assert!((track.height_beside(&nearest) - height).abs() < 0.05);
                let along = track.along(&nearest);
                let step = track.step_length();
                assert!(along > fork.fork as f32 * step && along < fork.join as f32 * step);
            }
        }
    }

    pub fn walls_close_in_on_a_narrow_bridge_and_open_out_after_it_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let narrowed: Vec<_> = circuits
            .iter()
            .filter(|circuit| !circuit.layout.narrows.is_empty())
            .collect();
        // A bridge over a lake and another over an inlet.
        assert!(narrowed.len() >= 2, "{} tracks", narrowed.len());
        for (layout, narrow) in narrowed.iter().flat_map(|circuit| {
            let layout = &circuit.layout;
            layout.narrows.iter().map(move |narrow| (layout, narrow))
        }) {
            let track = Track::new(layout);
            let middle = track.index_at((narrow.from + narrow.to) / 2.0);
            // Narrower than the asphalt, and the road ends short of them.
            assert!(narrow.wall < TRACK_WIDTH / 2.0);
            assert!((track.wall_at(middle) - narrow.wall).abs() < 1e-4);
            assert!(track.half_width_at(middle) < narrow.wall);
            // Closed in all the way from where it begins (from the first point
            // of the centre line on it).
            let first = track.index_at(narrow.from + track.step_length());
            assert!((track.wall_at(first) - narrow.wall).abs() < 1e-4);
            // Well before and after it, the walls are where they always are,
            // and they close in and open out without a step.
            for along in [
                narrow.from - NARROW_EASE - 5.0,
                narrow.to + NARROW_EASE + 5.0,
            ] {
                assert_eq!(track.wall_at(track.index_at(along)), track.wall());
                assert_eq!(
                    track.half_width_at(track.index_at(along)),
                    track.half_width()
                );
            }
            let first = track.index_at(narrow.from - NARROW_EASE - 10.0);
            for k in 0..((NARROW_EASE + 20.0) / track.step_length()) as usize {
                let (a, b) = (track.wall_at(first + k), track.wall_at(first + k + 1));
                assert!(b <= a + 1e-5 && a - b < 0.8, "{a} {b}");
            }
        }
        // Elsewhere, and on every other track, nothing changes.
        let bay = Track::default();
        assert!((0..bay.sample_count()).all(|i| bay.wall_at(i) == WALL_DISTANCE));
    }

    pub fn the_banking_puts_no_bumps_off_the_middle_of_the_road_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // Ridden at 30 m/s 4 m either side of the middle, how sharply the
        // road rises and falls is never more than 0.6 g from what it is in
        // the middle (every track well under it, the steepest banking at
        // 0.3 g): the bank changes smoothly, coming to its most softly (as
        // it did not once on raised bends, 3 g off the middle, felt as
        // bumps).
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let (n, step, speed) = (track.sample_count(), track.step_length(), 30.0f32);
            let felt = |i: usize, lateral: f32| {
                let h = |i: usize| track.surface_point(i % n, lateral).y;
                (h(i - 1) - 2.0 * h(i) + h(i + 1)) / (step * step) * speed * speed
            };
            for i in 1..n {
                for lateral in [-4.0, 4.0] {
                    let off = (felt(i, lateral) - felt(i, 0.0)).abs();
                    assert!(
                        off < 0.6 * 9.81,
                        "{}: {off} m/s² at {} m, {lateral} m off the middle",
                        circuit.name,
                        i as f32 * step
                    );
                }
            }
        }
    }

    #[test]
    fn the_banking_puts_no_bumps_off_the_middle_of_the_road() {
        the_banking_puts_no_bumps_off_the_middle_of_the_road_on(&crate::testing::DEMO);
    }

    pub fn slopes_and_banking_stay_rideable_on(circuits: &'static crate::circuits::Circuits) {
        for (name, track) in every_track(circuits) {
            let (n, step) = (track.sample_count(), track.step_length());
            for i in 0..n {
                let grade = (track.elevation[(i + 1) % n] - track.elevation[i]) / step;
                assert!(
                    grade.abs() < 0.13,
                    "pendiente del {:.0} % en {i} de {name}",
                    grade * 100.0
                );
                // (Steeper where the road grips harder, see `Banking::grip`,
                // and eased in and out over the points either side.)
                let reach = (BANK_SMOOTHING + RAISED_BANK_SMOOTHING) as isize;
                let grip = (i as isize - reach..=i as isize + reach)
                    .map(|j| track.grip_at(0, j.rem_euclid(n as isize) as usize))
                    .fold(1.0, f32::max);
                assert!(track.bank[i].abs() <= MAX_BANK * grip + 1e-5);
            }
            // Las curvas cerradas sí tienen peralte.
            assert!(track.bank.iter().any(|b| b.abs() > 0.08), "{name}");
        }
    }

    #[test]
    fn slopes_and_banking_stay_rideable() {
        slopes_and_banking_stay_rideable_on(&crate::testing::DEMO);
    }

    pub fn raised_banking_keeps_the_inside_where_it_would_lie_level_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // Its inside wall no lower than the road would lie level, and its
        // outside well up where it banks well.
        let mut banked = 0;
        for (name, track) in every_track(circuits) {
            for i in 0..track.sample_count() {
                if !track.raised.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let wall = track.wall_at(i);
                let (left, right) = (
                    track.surface_point(i, wall).y,
                    track.surface_point(i, -wall).y,
                );
                let level = track.elevation[i] - track.lift[i];
                assert!(left.min(right) > level - 0.01, "{name}: at {i}");
                if track.bank[i].abs() > 0.15 {
                    assert!(left.max(right) > level + 2.0, "{name}: at {i}");
                    banked += 1;
                }
            }
        }
        assert!(banked > 0);
    }

    pub fn banked_spans_bank_more_on(circuits: &'static crate::circuits::Circuits) {
        let layout = &circuits[1].layout;
        let banked = Track::new(layout);
        let plain = Track::new(&Layout {
            banking: &[],
            ..*layout
        });
        let most = |track: &Track, from: f32, to: f32| {
            (track.index_at(from)..track.index_at(to))
                .map(|i| track.bank[i].abs())
                .fold(0.0, f32::max)
        };
        let span = &layout.banking[0];
        assert!(most(&banked, span.from, span.to) > 1.8 * most(&plain, span.from, span.to));
        // Lejos del tramo, igual.
        assert_eq!(most(&banked, 1400.0, 1600.0), most(&plain, 1400.0, 1600.0));
    }

    #[test]
    fn positions_along_the_lap_round_trip() {
        let track = track();
        let lap = track.lap_length();
        assert!((lap - track.sample_count() as f32 * track.step_length()).abs() < 1e-3);
        let index = track.index_at(321.0);
        let nearest = track.nearest(track.offset_point(index, 3.0));
        assert!((track.along(&nearest) - 321.0).abs() < track.step_length());
        // Más allá de una vuelta, da la vuelta.
        assert_eq!(track.index_at(lap + 321.0), index);
    }

    #[test]
    fn profile_passes_through_its_points_and_closes() {
        let lap = 1000.0;
        let points = [(0.0, 0.0), (300.0, 10.0), (600.0, 4.0)];
        for (s, h) in points {
            assert!((profile_height(&points, lap, s) - h).abs() < 1e-4);
        }
        let before = profile_height(&points, lap, lap - 0.01);
        assert!((before - profile_height(&points, lap, 0.0)).abs() < 0.01);
    }

    #[test]
    fn banking_raises_the_outside_of_the_curve() {
        let track = track();
        let index = (0..track.sample_count())
            .max_by(|&a, &b| track.bank[a].abs().total_cmp(&track.bank[b].abs()))
            .unwrap();
        let curvature = track.curvature(index);
        // El exterior de una curva a la izquierda es la derecha (lateral < 0).
        let outside = track.surface_point(index, -curvature.signum() * 7.0);
        let inside = track.surface_point(index, curvature.signum() * 7.0);
        assert!(outside.y > inside.y);
    }

    #[test]
    fn nearest_point_lies_on_the_centerline() {
        let track = track();
        let nearest = track.nearest(track.offset_point(200, 5.0));
        assert!((nearest.distance - 5.0).abs() < 0.05);
        assert!((nearest.lateral - 5.0).abs() < 0.05);
        let height = track.road_height(track.offset_point(200, 5.0));
        assert!((height - track.surface_point(200, 5.0).y).abs() < 0.05);
        assert!(nearest.point.distance(track.offset_point(200, 0.0)) < 0.1);
    }
}
