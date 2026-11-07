//! On the web, the sound's own way out to the speakers, in place of Bevy's.
//!
//! Bevy plays its sounds through rodio and cpal, whose web backend keeps
//! only two buffers of 2048 samples (some 46 ms each) queued, refilled from
//! the page's one thread as each ends. There, every frame longer than about
//! 90 ms (the game preparing a track, a heavy frame in a race) left a gap,
//! and its clock never caught up again: from then on each buffer was
//! started late, over the one before, and the sound crackled for good.
//!
//! Here the sound is written each frame up to [`Clock::ahead`] seconds
//! ahead of the speakers, in chunks of [`CHUNK`] samples, each scheduled at
//! its time with Web Audio. A frame too long for that leaves a single gap:
//! the clock starts again from the speakers' time (and keeps a little more
//! ahead from then on), with nothing played over what played before.
//!
//! What plays is a list of [`Voices`]: the synthesised mix of the effects
//! (`super::synth`) and the songs, each with its volume ([`Voice`]), which
//! goes from one chunk to the next in a ramp, not a step.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The samples a second everything is played at: the synthesiser's own.
pub const RATE: u32 = super::synth::SAMPLE_RATE;
/// Samples (each a left and a right) a chunk: some 23 ms.
pub const CHUNK: usize = 1024;
/// How far ahead of the speakers the sound is written as it starts (s), and
/// at most, after frames that were too long for it.
const AHEAD: f64 = 0.12;
const MOST_AHEAD: f64 = 0.3;
/// Where the clock starts again after a gap, ahead of the speakers (s).
const RESTART: f64 = 0.03;

/// When the next chunk is to play, by the speakers' clock (s), and how far
/// ahead of them it is kept.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    next: f64,
    pub ahead: f64,
    started: bool,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            next: 0.0,
            ahead: AHEAD,
            started: false,
        }
    }
}

impl Clock {
    /// The chunks to write at `now` (the speakers' time, s), each as long
    /// as `chunk` (s): their number, and when the first plays. If the last
    /// one written has already played (a frame too long), the clock starts
    /// again from `now`, and keeps further ahead from then on.
    pub fn due(&mut self, now: f64, chunk: f64) -> (usize, f64) {
        if self.next < now + 0.005 {
            if self.started {
                self.ahead = (self.ahead * 1.25).min(MOST_AHEAD);
            }
            self.next = now + RESTART;
        }
        self.started = true;
        let first = self.next;
        let count = ((now + self.ahead - self.next) / chunk).ceil().max(0.0) as usize;
        self.next += count as f64 * chunk;
        (count, first)
    }
}

/// What a voice plays at, and whether it is to stop: shared between what
/// plays it and the game, which changes it.
#[derive(Clone, Default)]
pub struct Voice(Arc<VoiceState>);

#[derive(Default)]
struct VoiceState {
    /// Its volume, as the bits of an `f32`.
    gain: AtomicU32,
    stopped: AtomicBool,
}

impl Voice {
    pub fn new(gain: f32) -> Self {
        let voice = Self::default();
        voice.set_gain(gain);
        voice
    }

    pub fn set_gain(&self, gain: f32) {
        self.0.gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn gain(&self) -> f32 {
        f32::from_bits(self.0.gain.load(Ordering::Relaxed))
    }

    /// It stops at the next chunk, and is let go of.
    pub fn stop(&self) {
        self.0.stopped.store(true, Ordering::Relaxed);
    }

    fn stopped(&self) -> bool {
        self.0.stopped.load(Ordering::Relaxed)
    }
}

/// A sound playing: its samples, a left and a right in turn at [`RATE`],
/// and its voice, with the volume it had in the chunk before.
struct Playing {
    samples: Box<dyn Iterator<Item = f32>>,
    voice: Voice,
    gain: f32,
}

/// What plays, mixed.
#[derive(Default)]
pub struct Voices(Vec<Playing>);

impl Voices {
    /// Plays `samples` (a left and a right in turn, at [`RATE`]) with the
    /// volume of `voice`, from the next chunk.
    pub fn play(&mut self, samples: impl Iterator<Item = f32> + 'static, voice: Voice) {
        let gain = voice.gain();
        self.0.push(Playing {
            samples: Box::new(samples),
            voice,
            gain,
        });
    }

    /// How many sounds play.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The next chunk of everything that plays, mixed, into `left` and
    /// `right` (of the same length): each sound at its volume, which goes
    /// from the chunk before's to its own along the chunk. A sound that
    /// ends, or was stopped, is let go of.
    pub fn mix(&mut self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        let frames = left.len();
        self.0.retain_mut(|playing| {
            if playing.voice.stopped() {
                return false;
            }
            let (from, to) = (playing.gain, playing.voice.gain());
            playing.gain = to;
            let step = (to - from) / frames.max(1) as f32;
            for frame in 0..frames {
                let (Some(l), Some(r)) = (playing.samples.next(), playing.samples.next()) else {
                    return false;
                };
                let gain = from + step * (frame + 1) as f32;
                left[frame] += l * gain;
                right[frame] += r * gain;
            }
            true
        });
    }
}

/// Bevy's side of it, on the web: the page's audio, what plays, and its
/// clock.
#[cfg(target_arch = "wasm32")]
pub mod web {
    use bevy::prelude::*;
    use web_sys::{AudioContext, AudioContextOptions};

    use super::{CHUNK, Clock, RATE, Voice, Voices};

    /// The page's audio, and what plays through it (it lives on the page's
    /// thread, as everything there).
    pub struct WebAudio {
        context: Option<AudioContext>,
        clock: Clock,
        pub voices: Voices,
        left: Vec<f32>,
        right: Vec<f32>,
    }

    impl Default for WebAudio {
        fn default() -> Self {
            let options = AudioContextOptions::new();
            options.set_sample_rate(RATE as f32);
            let context = AudioContext::new_with_context_options(&options)
                .inspect_err(|error| warn!("No audio on this page: {error:?}"))
                .ok();
            Self {
                context,
                clock: Clock::default(),
                voices: Voices::default(),
                left: vec![0.0; CHUNK],
                right: vec![0.0; CHUNK],
            }
        }
    }

    /// A song's voice, on its entity: the song stops as the entity goes.
    #[derive(Component)]
    pub struct WebSink(pub Voice);

    impl WebSink {
        pub fn set_volume(&mut self, volume: bevy::audio::Volume) {
            self.0.set_gain(volume.to_linear());
        }
    }

    impl Drop for WebSink {
        fn drop(&mut self) {
            self.0.stop();
        }
    }

    /// Writes what plays up to the clock's time ahead, chunk by chunk, each
    /// scheduled at its time.
    pub fn play(mut audio: NonSendMut<WebAudio>) {
        let audio = &mut *audio;
        let Some(context) = audio.context.as_ref() else {
            return;
        };
        let chunk = CHUNK as f64 / RATE as f64;
        let (count, mut at) = audio.clock.due(context.current_time(), chunk);
        for _ in 0..count {
            audio.voices.mix(&mut audio.left, &mut audio.right);
            if let Err(error) = schedule(context, &audio.left, &audio.right, at) {
                warn!("A chunk of sound could not be played: {error:?}");
                return;
            }
            at += chunk;
        }
    }

    fn schedule(
        context: &AudioContext,
        left: &[f32],
        right: &[f32],
        at: f64,
    ) -> Result<(), js_sys::wasm_bindgen::JsValue> {
        let buffer = context.create_buffer(2, CHUNK as u32, RATE as f32)?;
        buffer.copy_to_channel(left, 0)?;
        buffer.copy_to_channel(right, 1)?;
        let source = context.create_buffer_source()?;
        source.set_buffer(Some(&buffer));
        source.connect_with_audio_node(&context.destination())?;
        source.start_with_when(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: f64 = CHUNK as f64 / RATE as f64;

    #[test]
    fn the_clock_keeps_its_time_ahead_and_no_more() {
        let mut clock = Clock::default();
        let (count, first) = clock.due(1.0, STEP);
        assert!((first - (1.0 + RESTART)).abs() < 1e-9);
        assert!(first + count as f64 * STEP >= 1.0 + AHEAD);
        assert!(first + (count - 1) as f64 * STEP < 1.0 + AHEAD);
        // A frame later, only what the speakers played meanwhile.
        let (more, at) = clock.due(1.0 + 1.0 / 60.0, STEP);
        assert!(more <= 1, "{more}");
        assert!(
            (at - (first + count as f64 * STEP)).abs() < 1e-9,
            "one after another"
        );
        assert_eq!(clock.ahead, AHEAD);
    }

    #[test]
    fn after_a_frame_too_long_the_clock_starts_again_and_keeps_further_ahead() {
        let mut clock = Clock::default();
        clock.due(1.0, STEP);
        // Half a second with no frame: what was written has played.
        let (count, first) = clock.due(1.5, STEP);
        assert!(
            (first - (1.5 + RESTART)).abs() < 1e-9,
            "from now, not over the last"
        );
        assert!(count > 0);
        assert!(clock.ahead > AHEAD);
        for _ in 0..20 {
            let now = clock.next + 1.0;
            clock.due(now, STEP);
        }
        assert_eq!(clock.ahead, MOST_AHEAD);
    }

    /// A left of `left` and a right of `right`, for ever.
    fn steady(left: f32, right: f32) -> impl Iterator<Item = f32> {
        [left, right].into_iter().cycle()
    }

    #[test]
    fn voices_mix_each_at_its_volume_ramping_to_a_new_one() {
        let mut voices = Voices::default();
        let (one, other) = (Voice::new(1.0), Voice::new(0.5));
        voices.play(steady(0.2, -0.2), one.clone());
        voices.play(steady(0.4, 0.4), other.clone());
        let (mut left, mut right) = (vec![0.0; 8], vec![0.0; 8]);
        voices.mix(&mut left, &mut right);
        assert!(left.iter().all(|l| (l - 0.4).abs() < 1e-6), "{left:?}");
        assert!(right.iter().all(|r| r.abs() < 1e-6), "{right:?}");
        // Faded out along the next chunk, not at a step.
        other.set_gain(0.0);
        voices.mix(&mut left, &mut right);
        assert!(left.windows(2).all(|pair| pair[1] < pair[0]), "{left:?}");
        assert!((left[7] - 0.2).abs() < 1e-6);
        assert_eq!(voices.len(), 2);
        other.stop();
        voices.mix(&mut left, &mut right);
        assert_eq!(voices.len(), 1, "stopped, let go of");
    }

    #[test]
    fn a_sound_that_ends_is_let_go_of() {
        let mut voices = Voices::default();
        voices.play([0.5, 0.5, 0.5, 0.5].into_iter(), Voice::new(1.0));
        let (mut left, mut right) = (vec![0.0; 8], vec![0.0; 8]);
        voices.mix(&mut left, &mut right);
        assert!(voices.is_empty());
        assert_eq!(left[0], 0.5);
    }
}
