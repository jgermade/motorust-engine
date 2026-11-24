//! The shaders made ready on the grid: as a race is set up, Bevy only
//! compiles the pipelines of what a camera sees, so those of all the
//! circuit first in view compile at once (on the M4 mini, 60 of them, with
//! frames of up to 150 ms), and the rest as each comes into view, the race
//! under way. Here the riders' cameras see all of the circuit for a moment
//! ([`NoCpuCulling`]: Bevy then queues everything for them, and the GPU's
//! own culling keeps the drawing as it was), so that its pipelines all
//! compile on the grid; the start lights wait for them ([`WarmUp::done`],
//! see [`crate::race::lights`]), and once none has been waiting for a
//! moment, or after [`LONGEST`] s, the cameras cull again.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bevy::camera::visibility::NoCpuCulling;
use bevy::prelude::*;
use bevy::render::render_resource::PipelineCache;
use bevy::render::{Render, RenderApp, RenderSystems};

use super::{ChaseCamera, setup_split_cameras};
use crate::menu::GameState;

/// The fewest frames the cameras see everything for: what they see then
/// takes a couple of frames to reach the render world and be queued.
const FEWEST_FRAMES: u32 = 8;
/// How long (s) no pipeline may have been waiting, nor a new one come, for
/// the warm-up to be over: textures still loading hold some back a moment.
const SETTLED: f32 = 0.3;
/// The longest (s) the warm-up lasts, however many are still waiting.
pub const LONGEST: f32 = 6.0;
/// The longest step (s) one frame counts for: the frame the circuit is
/// built in takes seconds, and is no time spent waiting for the shaders.
const MAX_STEP: f32 = 0.1;

pub struct WarmUpPlugin;

impl Plugin for WarmUpPlugin {
    fn build(&self, app: &mut App) {
        let pipelines = Pipelines::default();
        app.insert_resource(pipelines.clone())
            .init_resource::<WarmUp>()
            .add_systems(
                OnEnter(GameState::Race),
                start_warm_up.after(setup_split_cameras),
            )
            .add_systems(Update, warm_up.run_if(in_state(GameState::Race)));
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(pipelines)
                .add_systems(Render, count_pipelines.in_set(RenderSystems::Cleanup));
        }
    }
}

/// How many pipelines the render world has, and how many of them are still
/// waiting to be compiled, as it last counted them: shared by both worlds.
#[derive(Resource, Clone, Default)]
struct Pipelines(Arc<[AtomicUsize; 2]>);

impl Pipelines {
    fn read(&self) -> Count {
        Count {
            all: self.0[0].load(Ordering::Relaxed),
            waiting: self.0[1].load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct Count {
    all: usize,
    waiting: usize,
}

fn count_pipelines(cache: Res<PipelineCache>, pipelines: Res<Pipelines>) {
    pipelines.0[0].store(cache.pipelines().count(), Ordering::Relaxed);
    pipelines.0[1].store(cache.waiting_pipelines().count(), Ordering::Relaxed);
}

/// Where the warm-up of a race is at.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct WarmUp {
    /// Frames and seconds (real) since it began.
    frames: u32,
    elapsed: f32,
    /// Seconds since a pipeline was last waiting or new, and the count then.
    settled: f32,
    last: Count,
    done: bool,
}

impl Default for WarmUp {
    /// Over: out of a race, nothing waits for it.
    fn default() -> Self {
        Self {
            frames: 0,
            elapsed: 0.0,
            settled: 0.0,
            last: Count::default(),
            done: true,
        }
    }
}

impl WarmUp {
    fn new() -> Self {
        Self {
            done: false,
            ..default()
        }
    }

    /// Whether it is over: the pipelines ready, or it has waited long
    /// enough for them.
    pub fn done(&self) -> bool {
        self.done
    }

    /// A frame of `dt` s later, the render world having counted `count`:
    /// whether it is over now.
    fn step(&mut self, dt: f32, count: Count) -> bool {
        if self.done {
            return false;
        }
        let dt = dt.clamp(0.0, MAX_STEP);
        self.frames += 1;
        self.elapsed += dt;
        if count.waiting > 0 || count.all != self.last.all {
            self.settled = 0.0;
        } else {
            self.settled += dt;
        }
        self.last = count;
        self.done =
            self.elapsed >= LONGEST || (self.frames >= FEWEST_FRAMES && self.settled >= SETTLED);
        self.done
    }
}

/// A race set up: its riders' cameras see all of it.
fn start_warm_up(
    mut commands: Commands,
    mut warm: ResMut<WarmUp>,
    pipelines: Res<Pipelines>,
    cameras: Query<Entity, With<ChaseCamera>>,
) {
    *warm = WarmUp::new();
    warm.last = pipelines.read();
    for camera in &cameras {
        commands.entity(camera).insert(NoCpuCulling);
    }
}

/// Once the pipelines are ready, the cameras cull again.
fn warm_up(
    mut commands: Commands,
    real: Res<Time<Real>>,
    mut warm: ResMut<WarmUp>,
    pipelines: Res<Pipelines>,
    cameras: Query<Entity, (With<ChaseCamera>, With<NoCpuCulling>)>,
) {
    let count = pipelines.read();
    if warm.step(real.delta_secs(), count) {
        info!(
            "Shaders ready in {:.2} s ({} frames): {} pipelines, {} still waiting",
            warm.elapsed, warm.frames, count.all, count.waiting
        );
        for camera in &cameras {
            commands.entity(camera).remove::<NoCpuCulling>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f32 = 1.0 / 60.0;

    fn count(all: usize, waiting: usize) -> Count {
        Count { all, waiting }
    }

    #[test]
    fn it_is_over_once_nothing_has_waited_for_a_moment_after_a_few_frames() {
        let mut warm = WarmUp::new();
        assert!(!warm.done());
        // Some compile for a while, then a few more come.
        for _ in 0..20 {
            assert!(!warm.step(FRAME, count(60, 12)));
        }
        // A new one has come: it starts settling from here.
        assert!(!warm.step(FRAME, count(66, 0)));
        let frames = (SETTLED / FRAME).ceil() as usize;
        for _ in 1..frames {
            assert!(!warm.step(FRAME, count(66, 0)));
        }
        assert!((0..2).any(|_| warm.step(FRAME, count(66, 0))));
        assert!(warm.done());
        // Over, it stays over.
        assert!(!warm.step(FRAME, count(70, 4)));
        assert!(warm.done());
    }

    #[test]
    fn it_takes_its_fewest_frames_even_with_nothing_to_compile() {
        let mut warm = WarmUp::new();
        let slow = 0.5;
        for _ in 1..FEWEST_FRAMES {
            assert!(!warm.step(slow, count(80, 0)));
        }
        assert!(warm.step(slow, count(80, 0)));
    }

    #[test]
    fn a_long_frame_counts_for_little() {
        let mut warm = WarmUp::new();
        // The circuit built in one frame of 2.3 s, nothing waiting: not
        // settled by it.
        warm.step(2.3, count(70, 0));
        assert!(warm.elapsed <= MAX_STEP && warm.settled <= MAX_STEP);
    }

    #[test]
    fn it_gives_up_waiting_after_the_longest() {
        let mut warm = WarmUp::new();
        let frames = (LONGEST / FRAME).ceil() as usize + 2;
        let over = (0..frames).position(|_| warm.step(FRAME, count(60, 3)));
        assert!(over.is_some());
        assert!(warm.elapsed >= LONGEST);
    }

    #[test]
    fn out_of_a_race_it_is_over() {
        assert!(WarmUp::default().done());
    }

    #[test]
    fn the_cameras_see_everything_until_it_is_over() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_state::<GameState>()
            .add_plugins(WarmUpPlugin)
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(ChaseCamera::default());
            });
        app.update();
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Race);
        app.update();
        let culling = |app: &mut App| {
            app.world_mut()
                .query_filtered::<(), (With<ChaseCamera>, With<NoCpuCulling>)>()
                .iter(app.world())
                .count()
        };
        assert_eq!(culling(&mut app), 1);
        assert!(!app.world().resource::<WarmUp>().done());
        // No render world here: nothing ever waits, so it is over once it
        // has taken its fewest frames and settled.
        let mut frames = 0;
        while !app.world().resource::<WarmUp>().done() {
            std::thread::sleep(std::time::Duration::from_millis(40));
            app.update();
            frames += 1;
            assert!(frames < 200, "never over");
        }
        app.update();
        assert_eq!(culling(&mut app), 0);
    }
}
