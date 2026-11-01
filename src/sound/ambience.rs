//! The sounds of a track's surroundings, heard while choosing the bike: the
//! sea breaking on the shore, seagulls, small birds in the trees, cicadas in
//! the heat and the wind, as much of each as the track has
//! ([`crate::circuits::Ambience`]). Synthesised like everything else: the
//! waves are noise that swells, brightens as it breaks and washes back; a
//! seagull's call is a few harsh notes that rise and fall; a bird's, quick
//! high chirps; the cicadas, a band of noise beating fast.

use std::f32::consts::{PI, TAU};

use super::synth::{BandPass, Noise, Param, SAMPLE_RATE, one_pole, pan};
use crate::geometry::smoothstep;

/// Seconds the surroundings take to fade in or out.
const FADE: f32 = 1.5;
/// How loud the whole of it is: about as loud as the bike idling on the
/// screen, a backdrop to it.
const LEVEL: f32 = 0.35;

/// What the game says of the surroundings: how much of each, 0 to 1.
#[derive(Default)]
pub struct AmbienceParams {
    pub sea: Param,
    pub gulls: Param,
    pub birds: Param,
    pub cicadas: Param,
    pub wind: Param,
}

/// One of the two layers of the sea, one to each side: a wave rolling in,
/// breaking and washing back over `period` s.
#[derive(Default)]
struct Wave {
    phase: f32,
    period: f32,
    low: f32,
    lower: f32,
}

impl Wave {
    /// How much of the wave sounds at `phase` (0 to 1 over the wave): it
    /// swells, breaks a little past the middle and washes back.
    fn swell(phase: f32) -> f32 {
        if phase < 0.55 {
            0.8 * smoothstep(0.0, 0.55, phase).powi(2)
        } else {
            0.15 + 0.85 * (-(phase - 0.55) * 7.0).exp()
        }
    }

    fn sample(&mut self, noise: &mut Noise, dt: f32) -> f32 {
        if self.period == 0.0 {
            self.period = 7.0 + 4.0 * noise.chance();
            self.phase = noise.chance();
        }
        self.phase += dt / self.period;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.period = 7.0 + 4.0 * noise.chance();
        }
        let swell = Self::swell(self.phase);
        // Brighter as it breaks.
        let cutoff = 250.0 + 1800.0 * swell;
        self.low += (noise.next() - self.low) * one_pole(cutoff);
        self.lower += (self.low - self.lower) * one_pole(cutoff);
        self.lower * swell * 2.2
    }
}

/// A call, a note at a time: a seagull's few harsh notes, or a bird's quick
/// chirps.
#[derive(Default)]
struct Caller {
    /// Seconds to the next call.
    wait: f32,
    /// Notes left in the call, and time into the one sounding.
    notes: u32,
    time: f32,
    /// The note sounding: how long it lasts, the gap after it, its pitch
    /// (Hz) and where its pitch goes (a share of it, up or down).
    length: f32,
    gap: f32,
    pitch: f32,
    sweep: f32,
    phase: f32,
    /// Where the caller is (-1 left to 1 right) and how near (0 to 1).
    pan: f32,
    near: f32,
}

/// What kind of caller it is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Gull,
    Bird,
}

impl Caller {
    /// The next note of the call, or a new call.
    fn next_note(&mut self, kind: Kind, noise: &mut Noise) {
        self.time = 0.0;
        let (length, gap, pitch, sweep) = match kind {
            Kind::Gull => (
                0.2 + 0.12 * noise.chance(),
                0.07 + 0.08 * noise.chance(),
                1300.0 + 500.0 * noise.chance(),
                0.35,
            ),
            Kind::Bird => (
                0.04 + 0.05 * noise.chance(),
                0.03 + 0.04 * noise.chance(),
                3000.0 + 2500.0 * noise.chance(),
                0.3 * noise.next(),
            ),
        };
        (self.length, self.gap, self.pitch, self.sweep) = (length, gap, pitch, sweep);
    }

    fn sample(&mut self, kind: Kind, amount: f32, noise: &mut Noise, dt: f32) -> (f32, f32) {
        if self.notes == 0 {
            self.wait -= dt;
            if self.wait > 0.0 {
                return (0.0, 0.0);
            }
            // A new call, after a while: sooner the more of them there are.
            let (notes, wait) = match kind {
                Kind::Gull => (
                    2 + (noise.chance() * 4.0) as u32,
                    2.5 + 7.0 * noise.chance(),
                ),
                Kind::Bird => (
                    2 + (noise.chance() * 6.0) as u32,
                    0.4 + 3.0 * noise.chance(),
                ),
            };
            self.notes = notes;
            self.wait = wait / amount.max(0.2);
            self.pan = 0.8 * noise.next();
            self.near = 0.4 + 0.6 * noise.chance();
            self.next_note(kind, noise);
        }
        let t = self.time;
        self.time += dt;
        if t >= self.length + self.gap {
            self.notes -= 1;
            if self.notes > 0 {
                self.next_note(kind, noise);
            }
            return (0.0, 0.0);
        }
        if t >= self.length {
            return (0.0, 0.0);
        }
        let along = t / self.length;
        let value = match kind {
            // Up and back down, harsh: a few harmonics and a little rasp.
            Kind::Gull => {
                let pitch =
                    self.pitch * (1.0 + self.sweep * (PI * along.powf(0.7)).sin() - 0.1 * along);
                self.phase = (self.phase + pitch * dt).fract();
                let p = TAU * self.phase;
                let tone = p.sin() + 0.6 * (2.0 * p).sin() + 0.35 * (3.0 * p).sin();
                // In and out softly: a sudden edge would click.
                let envelope = smoothstep(0.0, 0.12, along) * (1.0 - smoothstep(0.6, 1.0, along));
                (0.25 * tone + 0.08 * noise.next()) * envelope
            }
            // A quick sweep, pure.
            Kind::Bird => {
                let pitch = self.pitch * (1.0 + self.sweep * along);
                self.phase = (self.phase + pitch * dt).fract();
                (TAU * self.phase).sin() * (PI * along).sin() * 0.3
            }
        };
        pan(value * self.near * amount, self.pan)
    }
}

/// The surroundings as they sound.
pub(super) struct Ambience {
    /// Each one's amount, faded towards what the game says.
    sea: f32,
    gulls: f32,
    birds: f32,
    cicadas: f32,
    wind: f32,
    fade: f32,
    waves: [Wave; 2],
    gull_callers: [Caller; 2],
    bird_callers: [Caller; 2],
    cicada_bands: [BandPass; 2],
    cicada_beat: f32,
    cicada_swell: f32,
    wind_low: f32,
    wind_lower: f32,
    gusts: f32,
}

impl Default for Ambience {
    fn default() -> Self {
        Self {
            sea: 0.0,
            gulls: 0.0,
            birds: 0.0,
            cicadas: 0.0,
            wind: 0.0,
            fade: 1.0 - (-1.0 / (SAMPLE_RATE as f32 * FADE / 3.0)).exp(),
            waves: [Wave::default(), Wave::default()],
            gull_callers: Default::default(),
            bird_callers: Default::default(),
            cicada_bands: [BandPass::tuned(4300.0, 6.0), BandPass::tuned(4650.0, 6.0)],
            cicada_beat: 0.0,
            cicada_swell: 0.0,
            wind_low: 0.0,
            wind_lower: 0.0,
            gusts: 0.0,
        }
    }
}

impl Ambience {
    /// A stereo sample of the surroundings `params` asks for.
    pub(super) fn sample(&mut self, params: &AmbienceParams, noise: &mut Noise) -> (f32, f32) {
        let fade = self.fade;
        self.sea += (params.sea.get() - self.sea) * fade;
        self.gulls += (params.gulls.get() - self.gulls) * fade;
        self.birds += (params.birds.get() - self.birds) * fade;
        self.cicadas += (params.cicadas.get() - self.cicadas) * fade;
        self.wind += (params.wind.get() - self.wind) * fade;
        let dt = 1.0 / SAMPLE_RATE as f32;
        let (mut left, mut right) = (0.0, 0.0);

        if self.sea > 1e-4 {
            let [one, other] = &mut self.waves;
            let (a, b) = (one.sample(noise, dt), other.sample(noise, dt));
            left += self.sea * (0.75 * a + 0.25 * b);
            right += self.sea * (0.25 * a + 0.75 * b);
        }
        for (caller, kind, amount) in self
            .gull_callers
            .iter_mut()
            .map(|caller| (caller, Kind::Gull, self.gulls))
            .chain(
                self.bird_callers
                    .iter_mut()
                    .map(|caller| (caller, Kind::Bird, self.birds)),
            )
        {
            if amount > 1e-3 {
                let (l, r) = caller.sample(kind, amount, noise, dt);
                left += l;
                right += r;
            }
        }
        if self.cicadas > 1e-4 {
            // Noise in a high band, beating some fifty times a second, the
            // whole of it swelling and fading every few seconds.
            self.cicada_beat = (self.cicada_beat + 48.0 * dt).fract();
            self.cicada_swell = (self.cicada_swell + dt / 7.0).fract();
            let beat = (PI * self.cicada_beat).sin().powi(4);
            let swell = 0.6 + 0.4 * (TAU * self.cicada_swell).sin();
            let gain = self.cicadas * beat * swell * 0.6;
            let [one, other] = &mut self.cicada_bands;
            left += one.process(noise.next()) * gain;
            right += other.process(noise.next()) * gain;
        }
        if self.wind > 1e-4 {
            // Low, soft noise in slow gusts.
            self.gusts = (self.gusts + dt / 9.0).fract();
            let gust = 0.55 + 0.45 * (TAU * self.gusts).sin() * (TAU * 1.7 * self.gusts).cos();
            let cutoff = 350.0 + 500.0 * gust;
            self.wind_low += (noise.next() - self.wind_low) * one_pole(cutoff);
            self.wind_lower += (self.wind_low - self.wind_lower) * one_pole(cutoff);
            let wind = self.wind_lower * 2.0 * self.wind * gust;
            left += wind;
            right += wind;
        }
        (left * LEVEL, right * LEVEL)
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    /// `seconds` of the surroundings `set` asks for, as (left, right).
    fn render(set: impl Fn(&AmbienceParams), seconds: f32) -> Vec<(f32, f32)> {
        let params = AmbienceParams::default();
        set(&params);
        let mut ambience = Ambience::default();
        let mut noise = Noise(0x1234_5678);
        (0..(seconds * SAMPLE_RATE as f32) as usize)
            .map(|_| ambience.sample(&params, &mut noise))
            .collect()
    }

    /// The loudness of each `window` s (RMS of both sides).
    fn loudness(samples: &[(f32, f32)], window: f32) -> Vec<f32> {
        samples
            .chunks((window * SAMPLE_RATE as f32) as usize)
            .map(|chunk| {
                let power: f32 = chunk.iter().map(|(l, r)| l * l + r * r).sum();
                (power / (2 * chunk.len()) as f32).sqrt()
            })
            .collect()
    }

    #[test]
    fn nothing_sounds_unless_asked() {
        let samples = render(|_| {}, 2.0);
        assert!(samples.iter().all(|(l, r)| *l == 0.0 && *r == 0.0));
    }

    #[test]
    fn the_sea_rolls_in_breaks_and_washes_back() {
        let samples = render(|p| p.sea.set(1.0), 30.0);
        // After the fade in: loud and quiet by turns, never too loud.
        let levels = loudness(&samples[SAMPLE_RATE as usize * 4..], 0.5);
        let (low, high) = levels
            .iter()
            .fold((f32::MAX, 0.0f32), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        assert!(high > 2.5 * low, "waves: {low} to {high}");
        assert!(high > 0.01 && high < 0.5, "{high}");
        assert!(samples.iter().all(|(l, r)| l.abs() < 1.0 && r.abs() < 1.0));
    }

    #[test]
    fn gulls_and_birds_call_now_and_then() {
        for (name, set) in [
            (
                "gulls",
                Box::new(|p: &AmbienceParams| p.gulls.set(1.0)) as Box<dyn Fn(&AmbienceParams)>,
            ),
            ("birds", Box::new(|p: &AmbienceParams| p.birds.set(1.0))),
        ] {
            let samples = render(set, 40.0);
            let levels = loudness(&samples[SAMPLE_RATE as usize * 2..], 0.1);
            let calling = levels.iter().filter(|&&v| v > 0.005).count() as f32;
            let share = calling / levels.len() as f32;
            assert!(
                share > 0.02 && share < 0.7,
                "{name}: calling {share} of the time"
            );
        }
    }

    #[test]
    fn cicadas_buzz_high_and_the_wind_blows_low() {
        let crossings = |samples: &[(f32, f32)]| {
            samples
                .windows(2)
                .filter(|pair| (pair[0].0 > 0.0) != (pair[1].0 > 0.0))
                .count() as f32
                / (samples.len() as f32 / SAMPLE_RATE as f32)
        };
        let cicadas = render(|p| p.cicadas.set(1.0), 6.0);
        let wind = render(|p| p.wind.set(1.0), 6.0);
        let (cicadas, wind) = (
            &cicadas[SAMPLE_RATE as usize * 3..],
            &wind[SAMPLE_RATE as usize * 3..],
        );
        assert!(loudness(cicadas, 3.0)[0] > 0.005);
        assert!(loudness(wind, 3.0)[0] > 0.005);
        // Zero crossings a second: thousands for the buzz, hundreds for the
        // wind.
        assert!(crossings(cicadas) > 5000.0, "{}", crossings(cicadas));
        assert!(crossings(wind) < 2000.0, "{}", crossings(wind));
    }
}
