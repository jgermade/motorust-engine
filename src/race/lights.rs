//! The start lights, as in a Grand Prix: a moment after the grid is set
//! up, five red lights (or as many as the options say) come on one by one,
//! a second apart, and a moment after the last one (never quite the same,
//! or, as the options say, a second, on their beat) they go out and the
//! race starts. Until then every bike waits on the grid, though the players can
//! rev their engines ([`Race::held`]). Each light beeps as it comes on, and
//! going out beeps higher, while the lights flash green.
//!
//! In the options ([`crate::settings::RaceStart`]) the lights can also leave the bikes free
//! on the grid (once the first light is on): a player who moves off before
//! they go out jumps the start. The first false starts of each player (two
//! at first: [`crate::settings::Settings::false_starts`]) send everybody
//! back to the grid and the lights start again; the next one disqualifies. Or the race can start as it used to, when a player
//! first opens the throttle, with no lights.

use std::ops::Range;

use bevy::prelude::*;

use super::Standing;
use crate::bike::{BikeState, LocalPlayer, Player, Race, RecallToGrid, Retired, Rider};
use crate::menu::GameState;
use crate::pause;
use crate::settings::{LightsHold, RaceStart, Settings};
use crate::welcome::GameMode;

/// How many red lights there can be.
pub const LIGHTS: u32 = 5;
/// Seconds from the grid being set up to the first light, and between one
/// light and the next.
const FIRST_LIGHT: f32 = 1.0;
const BETWEEN: f32 = 1.0;
/// Seconds the five lights stay on before they go out, held at random
/// ([`LightsHold::Random`]): somewhere in this range, so that the start
/// cannot be timed.
const HOLD: Range<f32> = 0.3..1.3;
/// Seconds the lights show green once they are out.
const GREEN_FOR: f32 = 0.8;
/// The longest step (s) the lights take in one frame: after a hitch they
/// carry on where they were, instead of skipping ahead (or past a light).
const MAX_STEP: f32 = 0.1;
/// After a false start, seconds more before the first light: to read the
/// notice and let go of the throttle.
const RECALL_PAUSE: f32 = 1.5;
/// How fast a bike goes (m/s) once it has moved off the grid: before the
/// lights go out, a jump start.
const JUMP_SPEED: f32 = 1.0;

/// How big each light is (px) on a screen of its own, and on a split one;
/// how far down a screen of its own the panel of lights is (percent); its
/// padding, across and down, as a share of a light's size; and its border
/// (px).
const LAMP: f32 = 46.0;
const LAMP_SPLIT: f32 = 34.0;
const PANEL_TOP: f32 = 14.0;
const PANEL_PADDING: Vec2 = Vec2::new(0.45, 0.3);
const PANEL_BORDER: f32 = 2.0;

const RED: Color = Color::srgb(1.0, 0.1, 0.06);
const GREEN: Color = Color::srgb(0.15, 0.95, 0.3);
const DARK: Color = Color::srgb(0.12, 0.12, 0.13);

pub struct StartLightsPlugin;

impl Plugin for StartLightsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StartLights>()
            .add_message::<StartBeep>()
            .add_systems(
                OnEnter(GameState::Race),
                (set_the_lights, spawn_lights).chain(),
            )
            .add_systems(
                Update,
                (
                    (run_lights, jump_starts).chain().run_if(pause::running),
                    show_lights,
                )
                    .chain()
                    .run_if(in_state(GameState::Race)),
            );
    }
}

/// A beep of the lights: one coming on, or all of them going out.
#[derive(Message, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartBeep {
    Light,
    Go,
}

/// Where the start is at.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct StartLights {
    /// Seconds since the grid was set up.
    elapsed: f32,
    /// When the lights go out (s since the grid was set up).
    out_at: f32,
    /// Seconds since they went out, once they have.
    since_out: Option<f32>,
    /// How many there are.
    count: u32,
}

impl Default for StartLights {
    fn default() -> Self {
        Self::new(hold(LightsHold::Random, 0.5), LIGHTS)
    }
}

/// Seconds the lights stay on once all are lit, held as `kind` says: a
/// share `chance` (0 to 1) of the way through [`HOLD`], or as long as
/// between one light and the next.
pub fn hold(kind: LightsHold, chance: f32) -> f32 {
    match kind {
        LightsHold::Random => HOLD.start + (HOLD.end - HOLD.start) * chance.clamp(0.0, 1.0),
        LightsHold::Fixed => BETWEEN,
    }
}

impl StartLights {
    /// The `count` lights of a new start (1 to [`LIGHTS`]), staying on for
    /// `hold` s once all are lit (see [`hold`]).
    pub fn new(hold: f32, count: u32) -> Self {
        let count = count.clamp(1, LIGHTS);
        let last = FIRST_LIGHT + (count - 1) as f32 * BETWEEN;
        Self {
            elapsed: 0.0,
            out_at: last + hold.max(0.0),
            since_out: None,
            count,
        }
    }

    /// How many lights there are.
    pub fn count(&self) -> u32 {
        self.count
    }

    /// How many lights are on.
    pub fn lit(&self) -> u32 {
        if self.elapsed >= self.out_at || self.elapsed < FIRST_LIGHT {
            return 0;
        }
        (((self.elapsed - FIRST_LIGHT) / BETWEEN) as u32 + 1).min(self.count)
    }

    /// Whether they have gone out: the race is on.
    pub fn out(&self) -> bool {
        self.elapsed >= self.out_at
    }

    /// `dt` s later (at most [`MAX_STEP`]): what beeps. The lights go out
    /// their hold after the last one is seen to come on, however late a
    /// slow frame showed it.
    pub fn advance(&mut self, dt: f32) -> Option<StartBeep> {
        let dt = dt.clamp(0.0, MAX_STEP);
        if let Some(since) = &mut self.since_out {
            *since += dt;
            return None;
        }
        let (lit, out) = (self.lit(), self.out());
        let last = FIRST_LIGHT + (self.count - 1) as f32 * BETWEEN;
        self.elapsed += dt;
        if self.out() && !out {
            self.since_out = Some(0.0);
            Some(StartBeep::Go)
        } else if self.lit() > lit {
            if self.lit() == self.count {
                self.out_at = self.elapsed + (self.out_at - last);
            }
            Some(StartBeep::Light)
        } else {
            None
        }
    }
}

/// A share (0 to 1) that is hard to guess: from the nanoseconds of the
/// clock.
fn chance(time: &Time<Real>) -> f32 {
    let nanos = time.elapsed().subsec_nanos();
    let mixed = nanos.wrapping_mul(2_654_435_761) >> 8;
    (mixed % 1000) as f32 / 1000.0
}

/// A new race: new lights, as many as the options say.
fn set_the_lights(mut lights: ResMut<StartLights>, time: Res<Time<Real>>, settings: Res<Settings>) {
    *lights = StartLights::new(
        hold(settings.lights_hold, chance(&time)),
        settings.start_lights,
    );
}

/// With a start at the lights, the bikes wait for them; once out, the race
/// is on. Restarting the race lights them again.
fn run_lights(
    time: Res<Time>,
    real: Res<Time<Real>>,
    settings: Res<Settings>,
    mut race: ResMut<Race>,
    mut lights: ResMut<StartLights>,
    mut beeps: MessageWriter<StartBeep>,
) {
    let lights_on = settings.start.at_lights();
    let held = holds_now(settings.start, &lights);
    if race.lights != lights_on {
        race.lights = lights_on;
    }
    if race.held != held {
        race.held = held;
    }
    if !lights_on {
        return;
    }
    // Restarted (see `crate::bike`): back on the grid, the lights again.
    if !race.started && lights.out() {
        *lights = StartLights::new(hold(settings.lights_hold, chance(&real)), lights.count());
    }
    if let Some(beep) = lights.advance(time.delta_secs()) {
        beeps.write(beep);
    }
    if lights.out() && !race.started {
        race.started = true;
        info!("Empieza la carrera: semáforo");
    }
}

/// Whether the bikes are held on the grid now, starting as `start` says:
/// at the lights, free or not, nobody moves before the first one comes on
/// (after a false start, a moment to let go of the throttle).
fn holds_now(start: RaceStart, lights: &StartLights) -> bool {
    start.holds() || (start.at_lights() && lights.lit() == 0 && !lights.out())
}

/// Whether a bike going at `speed` (m/s) has moved off the grid.
fn moved_off(speed: f32) -> bool {
    speed.abs() > JUMP_SPEED
}

/// At the lights with the bikes free
/// ([`crate::settings::RaceStart::LightsUnheld`]), a player who moves off
/// the grid before they go out jumps the start. While they have false
/// starts left ([`Settings::false_starts`]), everybody goes back to the
/// grid and the lights start again; then they are disqualified, out of
/// the race (their bike rolls to a stop).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn jump_starts(
    mut commands: Commands,
    race: Res<Race>,
    settings: Res<Settings>,
    real: Res<Time<Real>>,
    mut lights: ResMut<StartLights>,
    mut recalls: MessageWriter<RecallToGrid>,
    standing: Option<ResMut<Standing>>,
    bikes: Query<
        (Entity, &Rider, &BikeState),
        (Or<(With<Player>, With<LocalPlayer>)>, Without<Retired>),
    >,
) {
    let Some(mut standing) = standing else {
        return;
    };
    if race.started || !race.lights || race.held {
        return;
    }
    let mut recall = false;
    for (bike, rider, state) in &bikes {
        if !moved_off(state.speed) || standing.out.contains(&rider.0) {
            continue;
        }
        let made = standing
            .false_starts
            .iter()
            .filter(|&&r| r == rider.0)
            .count() as u32;
        if made < settings.false_starts {
            info!(
                "Salida nula: {} ({}/{})",
                rider.0,
                made + 1,
                settings.false_starts
            );
            standing.false_starts.push(rider.0);
            recall = true;
        } else {
            info!("Salida anticipada: {} descalificado", rider.0);
            commands.entity(bike).insert(Retired);
            standing.out.push(rider.0);
            standing.disqualified.push(rider.0);
        }
    }
    if recall {
        recalls.write(RecallToGrid);
        *lights = StartLights::new(hold(settings.lights_hold, chance(&real)), lights.count());
        lights.elapsed = -RECALL_PAUSE;
    }
}

/// The lights on the screen.
#[derive(Component)]
struct LightsPanel;

/// One of the lights, from the left.
#[derive(Component)]
struct Lamp(u32);

/// How far down a screen of its own the panel of lights reaches (percent, at
/// the window's design size: the interface grows and shrinks with it).
#[cfg(any(test, feature = "testing"))]
pub fn panel_bottom() -> f32 {
    let tall = LAMP * (1.0 + 2.0 * PANEL_PADDING.y) + 2.0 * PANEL_BORDER;
    PANEL_TOP + tall / crate::ui::DESIGN_WINDOW.y * 100.0
}

fn spawn_lights(mut commands: Commands, game_mode: Res<GameMode>, lights: Res<StartLights>) {
    let two = *game_mode == GameMode::SplitScreen;
    let size = if two { LAMP_SPLIT } else { LAMP };
    commands
        .spawn((
            DespawnOnExit(GameState::Race),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                // In split screen, across the line between the halves.
                top: if two {
                    Val::Percent(50.0)
                } else {
                    Val::Percent(PANEL_TOP)
                },
                margin: UiRect::top(px(if two { -size } else { 0.0 })),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            LightsPanel,
            GlobalZIndex(5),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    column_gap: px(size * 0.35),
                    padding: UiRect::axes(px(size * PANEL_PADDING.x), px(size * PANEL_PADDING.y)),
                    border_radius: BorderRadius::all(px(size * 0.4)),
                    border: UiRect::all(px(PANEL_BORDER)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.02, 0.03, 0.88)),
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.15)),
            ))
            .with_children(|gantry| {
                for index in 0..lights.count() {
                    gantry.spawn((
                        Lamp(index),
                        Node {
                            width: px(size),
                            height: px(size),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(DARK),
                    ));
                }
            });
        });
}

/// The lights as they are: on in red one by one, then green a moment once
/// out; with no lights to wait for, hidden.
fn show_lights(
    settings: Res<Settings>,
    race: Res<Race>,
    lights: Res<StartLights>,
    mut panel: Query<&mut Visibility, With<LightsPanel>>,
    mut lamps: Query<(&Lamp, &mut BackgroundColor)>,
) {
    let green = lights.since_out.is_some_and(|since| since < GREEN_FOR);
    let shown = settings.start.at_lights() && (!race.started || green);
    for mut visibility in &mut panel {
        let wanted = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    let lit = lights.lit();
    for (lamp, mut color) in &mut lamps {
        let wanted = if green {
            GREEN
        } else if lamp.0 < lit {
            RED
        } else {
            DARK
        };
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn five_red_lights_come_on_a_second_apart_and_go_out() {
        let mut lights = StartLights::new(HOLD.start, LIGHTS);
        let mut beeps = Vec::new();
        let mut lit = Vec::new();
        let dt = 0.01;
        for step in 0..800 {
            if let Some(beep) = lights.advance(dt) {
                beeps.push((step as f32 * dt, beep));
            }
            lit.push(lights.lit());
        }
        let on: Vec<f32> = beeps
            .iter()
            .filter(|(_, beep)| *beep == StartBeep::Light)
            .map(|(at, _)| *at)
            .collect();
        assert_eq!(on.len(), LIGHTS as usize);
        for pair in on.windows(2) {
            assert!((pair[1] - pair[0] - BETWEEN).abs() < 0.02, "{on:?}");
        }
        // They go out once, a moment after the fifth.
        let go: Vec<f32> = beeps
            .iter()
            .filter(|(_, beep)| *beep == StartBeep::Go)
            .map(|(at, _)| *at)
            .collect();
        assert_eq!(go.len(), 1);
        assert!(go[0] - on[4] >= HOLD.start - 0.02);
        assert!(lights.out());
        assert_eq!(lights.lit(), 0);
        assert_eq!(lit.iter().max(), Some(&LIGHTS));
    }

    #[test]
    fn with_fewer_lights_they_go_out_sooner() {
        let mut lights = StartLights::new(HOLD.start, 3);
        let mut beeps = Vec::new();
        for _ in 0..800 {
            if let Some(beep) = lights.advance(0.01) {
                beeps.push(beep);
            }
            assert!(lights.lit() <= 3);
        }
        assert_eq!(
            beeps,
            [
                StartBeep::Light,
                StartBeep::Light,
                StartBeep::Light,
                StartBeep::Go
            ]
        );
        assert!(
            StartLights::new(HOLD.start, 3).out_at < StartLights::new(HOLD.start, 5).out_at - 1.9
        );
        // However many the settings say, one to five.
        assert_eq!(StartLights::new(HOLD.start, 0).count(), 1);
        assert_eq!(StartLights::new(HOLD.start, 9).count(), LIGHTS);
    }

    #[test]
    fn the_wait_after_the_last_light_holds_through_slow_frames() {
        // Whatever the frames take, hitches and all, the lights go out
        // their hold after the last one came on, give or take a step.
        let hold = HOLD.start;
        for frames in [
            vec![1.0 / 144.0],
            vec![1.0 / 60.0],
            vec![1.0 / 24.0],
            vec![1.0 / 60.0, 0.4, 1.0 / 60.0, 1.0 / 30.0],
        ] {
            let mut lights = StartLights::new(HOLD.start, LIGHTS);
            let (mut on, mut lit, mut go) = (0, None, None);
            for dt in frames.iter().cycle().take(2000) {
                match lights.advance(*dt) {
                    Some(StartBeep::Light) => {
                        on += 1;
                        lit = Some(lights.elapsed);
                    }
                    Some(StartBeep::Go) => go = Some(lights.elapsed),
                    None => {}
                }
            }
            assert_eq!(on, LIGHTS, "{frames:?}");
            let wait = go.expect("they go out") - lit.expect("the last one comes on");
            assert!(
                (hold - 1e-4..hold + MAX_STEP).contains(&wait),
                "{frames:?}: {wait}"
            );
        }
        // A frame stuck for seconds moves them on a step, no more.
        let mut lights = StartLights::new(HOLD.start, LIGHTS);
        assert_eq!(lights.advance(3.0), None);
        assert_eq!(lights.lit(), 0);
        assert_eq!(lights.elapsed, MAX_STEP);
    }

    #[test]
    fn the_wait_after_the_fifth_light_varies() {
        let soon = StartLights::new(hold(LightsHold::Random, 0.0), LIGHTS).out_at;
        let late = StartLights::new(hold(LightsHold::Random, 1.0), LIGHTS).out_at;
        assert!(late - soon > 0.9);
        assert!(soon > FIRST_LIGHT + 4.0 * BETWEEN);
    }

    #[test]
    fn a_fixed_wait_keeps_the_beat_of_the_lights() {
        // Whatever the chance, a second, as between one light and the next.
        for chance in [0.0, 0.37, 1.0] {
            assert_eq!(hold(LightsHold::Fixed, chance), BETWEEN);
        }
        let mut lights = StartLights::new(hold(LightsHold::Fixed, 0.9), LIGHTS);
        let mut beeps = Vec::new();
        let dt = 1.0 / 120.0;
        for _ in 0..1000 {
            if lights.advance(dt).is_some() {
                beeps.push(lights.elapsed);
            }
        }
        assert_eq!(beeps.len(), LIGHTS as usize + 1, "five lights and the go");
        for pair in beeps.windows(2) {
            assert!((pair[1] - pair[0] - BETWEEN).abs() < 2.0 * dt, "{beeps:?}");
        }
    }

    /// A grid at the lights, the bikes free or `held`: a rival and the
    /// players' bikes going at `speeds` (m/s).
    /// A grid at the lights, the bikes free or `held`, `let_off` false
    /// starts let off: a rival and the players' bikes going at `speeds`
    /// (m/s).
    fn grid(held: bool, let_off: u32, speeds: [f32; 3]) -> (App, Vec<Entity>) {
        let mut app = App::new();
        app.insert_resource(Race {
            started: false,
            lights: true,
            held,
        })
        .insert_resource(Standing::default())
        .insert_resource(Settings {
            false_starts: let_off,
            ..default()
        })
        .init_resource::<Time<Real>>()
        .init_resource::<StartLights>()
        .add_message::<RecallToGrid>()
        .add_systems(Update, jump_starts);
        let bikes = speeds
            .iter()
            .enumerate()
            .map(|(rider, &speed)| {
                let state = BikeState {
                    speed,
                    ..BikeState::parked_at((Vec2::ZERO, 0.0))
                };
                let mut bike = app.world_mut().spawn((Rider(rider), state));
                match rider {
                    1 => bike.insert(Player),
                    2 => bike.insert(LocalPlayer(1)),
                    _ => &mut bike,
                };
                bike.id()
            })
            .collect();
        (app, bikes)
    }

    #[test]
    fn a_player_who_moves_off_before_the_lights_go_out_is_disqualified() {
        // The rival and the second player keep still; the first moves off.
        let (mut app, bikes) = grid(false, 0, [3.0, 2.0, 0.2]);
        app.update();
        let standing = app.world().resource::<Standing>();
        assert_eq!(standing.disqualified, [1]);
        assert_eq!(standing.out, [1]);
        assert!(app.world().get::<Retired>(bikes[1]).is_some());
        assert!(app.world().get::<Retired>(bikes[2]).is_none());
        assert!(app.world().get::<Retired>(bikes[0]).is_none());
        // Once.
        app.update();
        assert_eq!(app.world().resource::<Standing>().out, [1]);
        // Out, the second one may move: the race is on.
        app.world_mut().resource_mut::<Race>().started = true;
        app.world_mut()
            .get_mut::<BikeState>(bikes[2])
            .unwrap()
            .speed = 5.0;
        app.update();
        assert_eq!(app.world().resource::<Standing>().disqualified, [1]);
    }

    #[test]
    fn with_the_bikes_held_nobody_jumps_the_start() {
        let (mut app, _) = grid(true, 2, [0.0, 2.0, 2.0]);
        app.update();
        assert!(app.world().resource::<Standing>().out.is_empty());
    }

    #[test]
    fn the_first_false_starts_send_everybody_back_to_the_grid() {
        let (mut app, bikes) = grid(false, 2, [0.0, 2.0, 0.0]);
        let recalls = |app: &mut App| {
            app.world_mut()
                .resource_mut::<Messages<RecallToGrid>>()
                .drain()
                .count()
        };
        for made in 1..=2 {
            // The lights some way on.
            app.world_mut().resource_mut::<StartLights>().elapsed = 3.5;
            app.update();
            let standing = app.world().resource::<Standing>();
            assert_eq!(standing.false_starts, vec![1; made], "{made}");
            assert!(standing.out.is_empty());
            assert!(app.world().get::<Retired>(bikes[1]).is_none());
            assert_eq!(recalls(&mut app), 1, "back to the grid");
            // The lights again from the start, after a pause.
            let lights = app.world().resource::<StartLights>();
            assert_eq!(lights.elapsed, -RECALL_PAUSE);
            assert_eq!(lights.lit(), 0);
        }
        // The third: disqualified.
        app.update();
        let standing = app.world().resource::<Standing>();
        assert_eq!(standing.disqualified, [1]);
        assert_eq!(standing.false_starts, [1, 1]);
        assert_eq!(recalls(&mut app), 0);
    }

    #[test]
    fn free_or_not_nobody_moves_before_the_first_light() {
        let mut lights = StartLights::new(hold(LightsHold::Random, 0.5), LIGHTS);
        assert!(holds_now(RaceStart::LightsUnheld, &lights));
        for _ in 0..12 {
            lights.advance(0.1);
        }
        assert!(!holds_now(RaceStart::LightsUnheld, &lights));
        assert!(holds_now(RaceStart::Lights, &lights));
        assert!(!holds_now(RaceStart::Throttle, &StartLights::default()));
    }
}
