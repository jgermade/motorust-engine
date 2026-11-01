//! Sintetizador: todos los sonidos del juego se generan aquí, en el hilo del
//! audio, sin grabaciones. El juego solo cambia unos parámetros compartidos
//! ([`Controls`]) y encola sonidos sueltos; el sintetizador los suaviza para
//! que no haya saltos y lo mezcla todo en estéreo.
//!
//! El motor es un V4 de carreras de 1000 cc, de cuatro tiempos, que sube hasta
//! 17.500 rpm. Cada dos vueltas del cigüeñal hay cuatro explosiones, en dos
//! parejas casi seguidas (encendido "big bang"): eso le da el bramido ronco
//! de las motos de Gran Premio. Cada explosión es un pulso de presión con un golpe de ruido
//! que hace sonar las resonancias del escape y del motor; a fondo, la
//! saturación lo vuelve áspero. Por encima suena el silbido de la caja de
//! cambios de dientes rectos.

use std::f32::consts::{PI, TAU};
use std::num::NonZero;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

use bevy::audio::{ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;

use super::ambience::{Ambience, AmbienceParams};
use crate::geometry::smoothstep;

pub const SAMPLE_RATE: u32 = 44_100;
/// Motores que suenan a la vez: uno por moto de la parrilla, para que cada
/// moto tenga siempre el suyo (si una voz pasara de una moto a otra, el
/// sonido saltaría de unas revoluciones a otras a trompicones).
pub const VOICES: usize = 8;
/// Revoluciones del corte de inyección.
pub const REDLINE: f32 = 17_500.0;

/// Tiempo que tardan los parámetros en llegar a su valor nuevo (s): evita
/// chasquidos cuando el juego los cambia de golpe, una vez por fotograma.
/// Más largo que un fotograma lento (30 FPS), para que no se oigan escalones.
const SMOOTHING: f32 = 0.05;
/// Cada cuántas muestras se recogen los sonidos sueltos (unos 6 ms).
const QUEUE_EVERY: u32 = 256;
/// Cuándo explota cada cilindro en el ciclo de cuatro tiempos (dos vueltas,
/// de 0 a 1): a 0°, 90°, 290° y 380° de 720°, el "big bang" de un V4.
const FIRING: [f32; 4] = [0.0, 90.0 / 720.0, 290.0 / 720.0, 380.0 / 720.0];
/// Lo que suena cada cilindro: no son idénticos.
const CYLINDERS: [f32; 4] = [1.0, 0.88, 1.07, 0.94];
/// Resonancias del escape y del motor: frecuencia (Hz), anchura (Q) y peso.
const RESONANCES: [(f32, f32, f32); 3] = [(115.0, 1.3, 1.0), (420.0, 3.0, 1.4), (1450.0, 4.0, 0.7)];
/// Lo que dura el pulso de presión de cada explosión y su golpe de ruido (s).
const PULSE_TIME: f32 = 0.0012;
const BURST_TIME: f32 = 0.0025;
/// Nivel del motor antes de saturar.
const ENGINE_GAIN: f32 = 2.5;
/// Silbido de la caja de cambios: frecuencia por rpm (Hz) y volumen.
const WHINE_PER_RPM: f32 = 0.12;
const WHINE: f32 = 0.025;

/// Un número que el juego escribe y el audio lee, desde hilos distintos.
#[derive(Default)]
pub struct Param(AtomicU32);

impl Param {
    pub fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    pub fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Relaxed);
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Relaxed))
    }
}

/// Lo que el juego dice de un motor.
#[derive(Default)]
pub struct EngineParams {
    pub rpm: Param,
    /// Gas, de 0 a 1: con más gas suena más fuerte y más brillante.
    pub load: Param,
    /// Levantamiento de la rueda delantera (caballito), de 0 a 1.
    pub wheelie: Param,
    /// Volumen en cada oído.
    pub left: Param,
    pub right: Param,
}

/// Sonidos sueltos, que suenan una vez.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sound {
    /// Golpe contra un muro, de 0 a 1, con su volumen en cada oído.
    Impact {
        strength: f32,
        left: f32,
        right: f32,
    },
    /// Aterrizaje de un salto.
    Landing {
        strength: f32,
        left: f32,
        right: f32,
    },
    /// Cambiar de opción en el menú.
    Click,
    /// Confirmar en el menú.
    Confirm,
    /// A start light coming on: a short beep.
    StartLight,
    /// The start lights going out: a longer, higher beep.
    StartGo,
    /// A player won: a bright fanfare up a major chord, the last note held
    /// over the chord.
    Victory,
    /// A player crossed the line at the end, not first: two notes a fifth
    /// apart, held.
    Finished,
    /// A lap record, or the most laps against the clock: a quick sparkle of
    /// bells up high.
    Record,
    /// A rival went out in an elimination: a short falling sweep.
    KnockedOut,
    /// A player is out (eliminated or disqualified), or their time is up:
    /// three notes falling a semitone each, the last one wavering.
    Defeat,
}

/// Todo lo que el juego controla del sonido.
pub struct Controls {
    pub engines: [EngineParams; VOICES],
    /// Viento, chirrido de neumáticos y hierba, de 0 a 1.
    pub wind: Param,
    pub skid: Param,
    pub grass: Param,
    /// Volumen de efectos de sonido (FX), de 0 a 1.
    pub fx: Param,
    /// Volumen general.
    pub master: Param,
    /// De 0 a 1, cuánto se apagan los motores, el viento, los neumáticos y la
    /// hierba (en pausa); los avisos y clics siguen sonando.
    pub muffle: Param,
    /// The sounds of the track's surroundings (see [`super::ambience`]).
    pub ambience: AmbienceParams,
    queue: Mutex<Vec<Sound>>,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            engines: Default::default(),
            wind: Param::default(),
            skid: Param::default(),
            grass: Param::default(),
            fx: Param::new(1.0),
            master: Param::default(),
            muffle: Param::default(),
            ambience: AmbienceParams::default(),
            queue: Mutex::new(Vec::new()),
        }
    }
}

impl Controls {
    pub fn play(&self, sound: Sound) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(sound);
        }
    }
}

/// La mezcla de todo lo que suena, como fuente de audio de Bevy: no acaba
/// nunca.
#[derive(Asset, TypePath, Clone)]
pub struct Mix {
    pub controls: Arc<Controls>,
}

impl Decodable for Mix {
    type Decoder = Mixer;

    fn decoder(&self) -> Mixer {
        Mixer::new(self.controls.clone())
    }
}

/// Generador de números pseudoaleatorios (xorshift), para el ruido.
pub(super) struct Noise(pub(super) u32);

impl Noise {
    /// De -1 a 1.
    pub(super) fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    /// De 0 a 1.
    pub(super) fn chance(&mut self) -> f32 {
        self.next() * 0.5 + 0.5
    }
}

/// Coeficiente de un filtro de un polo con frecuencia de corte `cutoff`.
pub(super) fn one_pole(cutoff: f32) -> f32 {
    1.0 - (-TAU * cutoff / SAMPLE_RATE as f32).exp()
}

/// Filtro paso banda (biquad de RBJ), para resonancias y chirridos.
#[derive(Clone, Copy, Default)]
pub(super) struct BandPass {
    b0: f32,
    a1: f32,
    a2: f32,
    x: [f32; 2],
    y: [f32; 2],
}

impl BandPass {
    pub(super) fn tuned(center: f32, q: f32) -> Self {
        let mut filter = Self::default();
        filter.tune(center, q);
        filter
    }

    fn tune(&mut self, center: f32, q: f32) {
        let w0 = TAU * center / SAMPLE_RATE as f32;
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        self.b0 = alpha / a0;
        self.a1 = -2.0 * w0.cos() / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    pub(super) fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * (input - self.x[1]) - self.a1 * self.y[0] - self.a2 * self.y[1];
        self.x = [input, self.x[0]];
        self.y = [output, self.y[0]];
        output
    }
}

/// Lo que se apaga cada muestra lo que suena en un motor: calculado una vez,
/// no en cada muestra.
struct Decay {
    pulse: f32,
    burst: f32,
    body: f32,
    pop: f32,
}

impl Decay {
    fn new() -> Self {
        let dt = 1.0 / SAMPLE_RATE as f32;
        Self {
            pulse: (-dt / PULSE_TIME).exp(),
            burst: (-dt / BURST_TIME).exp(),
            body: one_pole(300.0),
            pop: (-dt * 45.0).exp(),
        }
    }
}

/// Un motor sonando.
struct EngineVoice {
    /// Fase del ciclo de cuatro tiempos (dos vueltas), de 0 a 1.
    cycle: f32,
    rpm: f32,
    load: f32,
    wheelie: f32,
    left: f32,
    right: f32,
    /// Pulso de presión y golpe de ruido de la última explosión.
    pulse: f32,
    burst: f32,
    resonances: [BandPass; 3],
    wheelie_resonance: BandPass,
    body: f32,
    whine: f32,
    /// Petardeo al cortar gas: envolvente y su ruido filtrado.
    pop: f32,
    pop_tone: BandPass,
}

impl Default for EngineVoice {
    fn default() -> Self {
        Self {
            cycle: 0.0,
            rpm: 0.0,
            load: 0.0,
            wheelie: 0.0,
            left: 0.0,
            right: 0.0,
            pulse: 0.0,
            burst: 0.0,
            resonances: RESONANCES.map(|(center, q, _)| BandPass::tuned(center, q)),
            wheelie_resonance: BandPass::tuned(620.0, 2.2),
            body: 0.0,
            whine: 0.0,
            pop: 0.0,
            pop_tone: BandPass::tuned(900.0, 1.5),
        }
    }
}

impl EngineVoice {
    fn sample(
        &mut self,
        params: &EngineParams,
        noise: &mut Noise,
        smooth: f32,
        decay: &Decay,
    ) -> (f32, f32) {
        self.rpm += (params.rpm.get() - self.rpm) * smooth;
        self.load += (params.load.get() - self.load) * smooth;
        self.wheelie += (params.wheelie.get() - self.wheelie) * smooth;
        self.left += (params.left.get() - self.left) * smooth;
        self.right += (params.right.get() - self.right) * smooth;
        if self.left + self.right < 1e-4 {
            return (0.0, 0.0);
        }
        let dt = 1.0 / SAMPLE_RATE as f32;
        let load = self.load.clamp(0.0, 1.0);
        let wheelie = self.wheelie.clamp(0.0, 1.0);

        // Al hacer caballito el motor brama con tono más agudo y desahogado.
        let effective_rpm = self.rpm * (1.0 + 0.10 * wheelie);

        // Dos vueltas por ciclo: explota cada cilindro al pasar por su ángulo.
        let previous = self.cycle;
        self.cycle = (self.cycle + effective_rpm / 120.0 * dt).fract();
        let wrapped = self.cycle < previous;
        for (cylinder, &at) in FIRING.iter().enumerate() {
            let fires = if wrapped {
                at > previous || at <= self.cycle
            } else {
                previous < at && at <= self.cycle
            };
            if fires {
                let strength =
                    (0.3 + 0.7 * load) * CYLINDERS[cylinder] * (1.0 + 0.1 * noise.next());
                self.pulse += strength;
                self.burst += strength * (0.25 + 0.75 * load);
                // Al cortar gas con el motor alto, petardea.
                if load < 0.1 && self.rpm > 8000.0 && noise.chance() < 0.07 {
                    self.pop = 1.0;
                }
            }
        }
        let excite = self.pulse;
        let rasp = noise.next() * self.burst;
        self.pulse *= decay.pulse;
        self.burst *= decay.burst;

        let mut out = 0.0;
        for (filter, &(_, _, weight)) in self.resonances.iter_mut().zip(&RESONANCES) {
            out += weight * filter.process(excite + 0.4 * rasp);
        }
        // Rugido y resonancia abierta de la admisión y el escape en caballito.
        if wheelie > 1e-3 {
            let roar = self.wheelie_resonance.process(excite + 0.8 * rasp);
            out += 0.85 * wheelie * roar;
        }
        self.body += (excite - self.body) * decay.body;
        out += 1.2 * self.body + 0.15 * rasp;

        self.whine = (self.whine + effective_rpm * WHINE_PER_RPM * dt).fract();
        out += WHINE * (0.3 + 0.7 * load + 0.5 * wheelie) * (TAU * self.whine).sin();

        if self.pop > 1e-3 {
            out += self.pop * 3.0 * self.pop_tone.process(noise.next());
            self.pop *= decay.pop;
        }

        // A fondo y en caballito satura más: más áspero y rotundo.
        let drive = ENGINE_GAIN * (1.0 + 1.5 * load + 0.8 * wheelie);
        let loudness = (0.22 + 0.3 * load + 0.15 * (effective_rpm / REDLINE).min(1.1))
            * (1.0 + 0.28 * wheelie);
        let out = (out * drive).tanh() * loudness;
        (out * self.left, out * self.right)
    }
}

/// A brassy note of `pitch` Hz, `t` s after it starts (nothing before),
/// held for `length` s with a soft edge, fading by `fade` each second: a few
/// harmonics, softer as they go up.
fn brass(pitch: f32, t: f32, length: f32, fade: f32) -> f32 {
    if t < 0.0 || t > length + 0.05 {
        return 0.0;
    }
    let edges = smoothstep(0.0, 0.015, t) * (1.0 - smoothstep(length - 0.05, length + 0.05, t));
    let wave = (1..=5)
        .map(|harmonic| (TAU * pitch * harmonic as f32 * t).sin() / harmonic as f32)
        .sum::<f32>();
    wave * 0.5 * edges * (-t * fade).exp()
}

/// A bell of `pitch` Hz struck `t` s ago (nothing before): its note and a
/// fainter one out of tune above, ringing out.
fn bell(pitch: f32, t: f32) -> f32 {
    if t < 0.0 {
        return 0.0;
    }
    let strike = smoothstep(0.0, 0.004, t);
    let ring = (TAU * pitch * t).sin() + 0.35 * (TAU * pitch * 2.76 * t).sin() * (-t * 6.0).exp();
    ring * strike * (-t * 4.5).exp() * 0.6
}

/// Keeps a sum of notes within ±1, rounding off its peaks.
fn soft_clip(value: f32) -> f32 {
    (value * 1.2).tanh() / 1.2f32.tanh()
}

/// Un sonido suelto que está sonando.
struct Shot {
    sound: Sound,
    time: f32,
    tone: f32,
}

impl Shot {
    fn duration(&self) -> f32 {
        match self.sound {
            Sound::Impact { .. } => 0.6,
            Sound::Landing { .. } => 0.4,
            Sound::Click => 0.08,
            Sound::Confirm => 0.3,
            Sound::StartLight => 0.2,
            Sound::StartGo => 0.6,
            Sound::Victory => 2.0,
            Sound::Finished => 1.2,
            Sound::Record => 1.1,
            Sound::KnockedOut => 0.55,
            Sound::Defeat => 1.8,
        }
    }

    fn sample(&mut self, noise: &mut Noise) -> (f32, f32) {
        let t = self.time;
        self.time += 1.0 / SAMPLE_RATE as f32;
        let (value, left, right) = match self.sound {
            Sound::Impact {
                strength,
                left,
                right,
            } => {
                // Golpe sordo y crujido.
                self.tone += (noise.next() - self.tone) * one_pole(1800.0);
                let thud = (TAU * 70.0 * t).sin() * (-t * 9.0).exp();
                let crunch = self.tone * 3.0 * (-t * 22.0).exp();
                (strength * (thud + crunch), left, right)
            }
            Sound::Landing {
                strength,
                left,
                right,
            } => {
                self.tone += (noise.next() - self.tone) * one_pole(600.0);
                let thud = (TAU * 52.0 * t).sin() * (-t * 13.0).exp();
                let rattle = self.tone * 1.5 * (-t * 35.0).exp();
                (strength * (thud + rattle), left, right)
            }
            Sound::Click => {
                let pitch = 700.0 + 3000.0 * t;
                let value = (TAU * pitch * t).sin() * (-t * 45.0).exp() * 0.35;
                (value, 1.0, 1.0)
            }
            Sound::Confirm => {
                let (note, start) = if t < 0.1 { (660.0, 0.0) } else { (990.0, 0.1) };
                let value = (TAU * note * t).sin() * (-(t - start) * 18.0).exp() * 0.35;
                (value, 1.0, 1.0)
            }
            // Square-ish beeps, like a timing system's, with soft edges.
            Sound::StartLight | Sound::StartGo => {
                let (note, length) = match self.sound {
                    Sound::StartLight => (880.0, 0.16),
                    _ => (1320.0, 0.5),
                };
                let edges = smoothstep(0.0, 0.01, t) * (1.0 - smoothstep(length - 0.04, length, t));
                let wave = (TAU * note * t).sin();
                let value = (wave * 3.0).tanh() / 3.0f32.tanh() * edges * 0.3;
                (value, 1.0, 1.0)
            }
            Sound::Victory => {
                // C, E, G up to the high C, which is held over the chord.
                let arpeggio = [523.25, 659.26, 783.99]
                    .iter()
                    .enumerate()
                    .map(|(k, &note)| brass(note, t - 0.13 * k as f32, 0.16, 8.0))
                    .sum::<f32>();
                let top = brass(1046.5, t - 0.39, 1.5, 1.6);
                let chord = [523.25, 659.26, 783.99]
                    .iter()
                    .map(|&note| brass(note, t - 0.39, 1.5, 1.6) * 0.4)
                    .sum::<f32>();
                (soft_clip(arpeggio + top + chord) * 0.3, 1.0, 1.0)
            }
            Sound::Finished => {
                // G and the D above it, held.
                let value = brass(392.0, t, 1.0, 2.6) + brass(587.33, t - 0.15, 0.95, 2.6);
                (soft_clip(value) * 0.28, 1.0, 1.0)
            }
            Sound::Record => {
                // E major up two octaves, quickly, in bells.
                let value = [1318.5, 1661.2, 1975.5, 2637.0]
                    .iter()
                    .enumerate()
                    .map(|(k, &note)| bell(note, t - 0.065 * k as f32))
                    .sum::<f32>();
                (soft_clip(value) * 0.3, 1.0, 1.0)
            }
            Sound::KnockedOut => {
                // From 900 Hz down to a third of it in 0.45 s, with a little
                // breath of noise.
                let (from, fall, length) = (900.0, 3.0f32, 0.45);
                let phase = from * length * (1.0 - fall.powf(-t / length)) / fall.ln();
                let edges = smoothstep(0.0, 0.01, t) * (1.0 - smoothstep(length - 0.12, length, t));
                self.tone += (noise.next() - self.tone) * one_pole(3000.0);
                let value = ((TAU * phase).sin() + 0.25 * self.tone) * edges;
                (value * 0.28, 1.0, 1.0)
            }
            Sound::Defeat => {
                // G, F sharp and F, like a trombone, the F held and wavering.
                let notes = [
                    (392.0, 0.0, 0.32),
                    (369.99, 0.36, 0.32),
                    (349.23, 0.72, 1.0),
                ];
                let value = notes
                    .iter()
                    .map(|&(note, start, length)| {
                        let at = t - start;
                        let waver = if length > 0.5 {
                            1.0 + 0.012 * (TAU * 6.0 * at.max(0.0)).sin() * smoothstep(0.2, 0.5, at)
                        } else {
                            1.0
                        };
                        brass(note * waver, at, length, 1.2)
                    })
                    .sum::<f32>();
                (soft_clip(value) * 0.3, 1.0, 1.0)
            }
        };
        (value * left, value * right)
    }
}

/// El mezclador: genera la mezcla muestra a muestra, alternando el canal
/// izquierdo y el derecho.
pub struct Mixer {
    controls: Arc<Controls>,
    engines: [EngineVoice; VOICES],
    decay: Decay,
    /// Muestras hasta volver a mirar si hay sonidos nuevos en la cola.
    until_queue: u32,
    shots: Vec<Shot>,
    noise: Noise,
    smooth: f32,
    wind: (f32, f32, f32),
    gust: f32,
    skid: (f32, BandPass, f32),
    grass: (f32, f32),
    surroundings: Ambience,
    ambient: f32,
    fx: f32,
    master: f32,
    right: Option<f32>,
}

impl Mixer {
    pub fn new(controls: Arc<Controls>) -> Self {
        // From the start at the chosen volume, even silent.
        let fx = controls.fx.get();
        Self {
            controls,
            engines: Default::default(),
            decay: Decay::new(),
            until_queue: 0,
            shots: Vec::new(),
            noise: Noise(0x9E37_79B9),
            smooth: 1.0 - (-1.0 / (SAMPLE_RATE as f32 * SMOOTHING)).exp(),
            wind: (0.0, 0.0, 0.0),
            gust: 0.0,
            skid: (0.0, BandPass::tuned(1150.0, 9.0), 0.0),
            grass: (0.0, 0.0),
            surroundings: Ambience::default(),
            ambient: 1.0,
            fx,
            master: 0.0,
            right: None,
        }
    }

    /// Una muestra estéreo.
    fn frame(&mut self) -> (f32, f32) {
        let controls = &*self.controls;
        // Los sonidos sueltos se recogen cada pocos milisegundos, no en cada
        // muestra: así el hilo del audio casi nunca toca el candado.
        if self.until_queue == 0 {
            self.until_queue = QUEUE_EVERY;
            if let Ok(mut queue) = controls.queue.try_lock() {
                for sound in queue.drain(..) {
                    self.shots.push(Shot {
                        sound,
                        time: 0.0,
                        tone: 0.0,
                    });
                }
            }
        }
        self.until_queue -= 1;
        let dt = 1.0 / SAMPLE_RATE as f32;
        let (mut left, mut right) = (0.0, 0.0);
        for (voice, params) in self.engines.iter_mut().zip(&controls.engines) {
            let (l, r) = voice.sample(params, &mut self.noise, self.smooth, &self.decay);
            left += l;
            right += r;
        }

        // Viento: ruido grave con ráfagas, más agudo cuanto más fuerte.
        let (gain, low, lower) = &mut self.wind;
        *gain += (controls.wind.get() - *gain) * self.smooth;
        if *gain > 1e-4 {
            self.gust = (self.gust + dt * 0.37).fract();
            let gust = 0.8 + 0.2 * (TAU * self.gust).sin();
            *low += (self.noise.next() - *low) * one_pole(250.0 + 2200.0 * *gain);
            *lower += (*low - *lower) * one_pole(300.0 + 2500.0 * *gain);
            let wind = *lower * 2.5 * *gain * gust;
            left += wind;
            right += wind;
        }

        // Neumáticos: ruido en una banda estrecha que tiembla.
        let (gain, band, wobble) = &mut self.skid;
        *gain += (controls.skid.get() - *gain) * self.smooth;
        if *gain > 1e-4 {
            *wobble = (*wobble + dt * 8.0).fract();
            band.tune(1150.0 + 90.0 * (TAU * *wobble).sin(), 9.0);
            let squeal = band.process(self.noise.next()) * 3.5 * *gain;
            left += squeal;
            right += squeal;
        }

        // Hierba: retumbo grave con chasquidos de grava.
        let (gain, rumble) = &mut self.grass;
        *gain += (controls.grass.get() - *gain) * self.smooth;
        if *gain > 1e-4 {
            *rumble += (self.noise.next() - *rumble) * one_pole(180.0);
            let gravel = if self.noise.chance() < 0.004 {
                self.noise.next()
            } else {
                0.0
            };
            let grass = (*rumble * 3.0 + gravel * 0.5) * *gain;
            left += grass;
            right += grass;
        }

        // The surroundings of the track.
        let (l, r) = self
            .surroundings
            .sample(&controls.ambience, &mut self.noise);
        left += l;
        right += r;

        // En pausa, todo lo de la carrera se apaga; los avisos, no.
        self.ambient += (1.0 - controls.muffle.get() - self.ambient) * self.smooth;
        left *= self.ambient;
        right *= self.ambient;

        for shot in &mut self.shots {
            let (l, r) = shot.sample(&mut self.noise);
            left += l;
            right += r;
        }
        self.shots.retain(|shot| shot.time < shot.duration());

        self.fx += (controls.fx.get() - self.fx) * self.smooth;
        self.master += (controls.master.get() - self.master) * self.smooth;
        let total_vol = self.fx * self.master;
        // Saturación suave: nunca pasa de 1.
        ((left * total_vol).tanh(), (right * total_vol).tanh())
    }
}

impl Iterator for Mixer {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        let (left, right) = self.frame();
        self.right = Some(right);
        Some(left)
    }
}

impl Source for Mixer {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(2).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// Reparte un volumen entre los dos oídos según `pan` (de -1, izquierda, a 1,
/// derecha), sin que baje en el centro.
pub fn pan(volume: f32, pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
    (volume * angle.cos(), volume * angle.sin())
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    fn render(controls: &Arc<Controls>, seconds: f32) -> Vec<(f32, f32)> {
        let mut mixer = Mixer::new(controls.clone());
        (0..(seconds * SAMPLE_RATE as f32) as usize)
            .map(|_| mixer.frame())
            .collect()
    }

    fn engine_at(rpm: f32, load: f32) -> Arc<Controls> {
        let controls = Arc::new(Controls::default());
        controls.master.set(1.0);
        let engine = &controls.engines[0];
        engine.rpm.set(rpm);
        engine.load.set(load);
        engine.left.set(1.0);
        engine.right.set(1.0);
        controls
    }

    /// Parecido de la señal consigo misma desplazada `lag` muestras.
    fn correlation(signal: &[f32], lag: usize) -> f32 {
        let (a, b) = (&signal[..signal.len() - lag], &signal[lag..]);
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm =
            (a.iter().map(|x| x * x).sum::<f32>() * b.iter().map(|y| y * y).sum::<f32>()).sqrt();
        dot / norm
    }

    #[test]
    fn silence_until_something_sounds() {
        let controls = Arc::new(Controls::default());
        controls.master.set(1.0);
        assert!(
            render(&controls, 0.2)
                .iter()
                .all(|&(l, r)| l == 0.0 && r == 0.0)
        );
    }

    #[test]
    fn the_engine_repeats_with_each_firing() {
        for rpm in [3000.0, 10000.0, 17000.0] {
            let samples: Vec<f32> = render(&engine_at(rpm, 1.0), 1.0)
                .into_iter()
                .skip(SAMPLE_RATE as usize / 4)
                .map(|(l, _)| l)
                .collect();
            assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
            // Se repite en cada ciclo de cuatro explosiones (dos vueltas): el
            // parecido es máximo justo en ese periodo, y no un poco antes o
            // después.
            let cycle = SAMPLE_RATE as f32 * 120.0 / rpm;
            let at = |factor: f32| correlation(&samples, (cycle * factor).round() as usize);
            let on_cycle = at(1.0);
            assert!(on_cycle > 0.5, "{rpm} rpm: {on_cycle}");
            for factor in [0.8, 0.9, 1.1, 1.2] {
                assert!(on_cycle > at(factor) + 0.03, "{rpm} rpm, x{factor}");
            }
        }
    }

    #[test]
    fn more_throttle_sounds_louder() {
        let loudness = |load| {
            let samples = render(&engine_at(8000.0, load), 0.5);
            samples.iter().map(|(l, _)| l * l).sum::<f32>() / samples.len() as f32
        };
        assert!(loudness(1.0) > 1.5 * loudness(0.0));
    }

    #[test]
    fn the_start_lights_beep_and_the_go_is_higher_and_longer() {
        let beep = |sound: Sound| {
            let controls = Arc::new(Controls::default());
            controls.master.set(1.0);
            let mut mixer = Mixer::new(controls.clone());
            controls.play(sound);
            let samples: Vec<f32> = (0..SAMPLE_RATE).map(|_| mixer.frame().0).collect();
            let heard = |s: &f32| s.abs() > 0.05;
            let length = samples.iter().rposition(heard).unwrap_or(0) as f32 / SAMPLE_RATE as f32;
            // How often it crosses nought: twice its pitch.
            let crossings = samples
                .windows(2)
                .take((length * SAMPLE_RATE as f32) as usize)
                .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
                .count() as f32
                / length.max(1e-3);
            assert!(mixer.shots.is_empty(), "and it ends");
            (length, crossings)
        };
        let (light, light_pitch) = beep(Sound::StartLight);
        let (go, go_pitch) = beep(Sound::StartGo);
        assert!(light > 0.1 && light < 0.2, "{light} s");
        assert!(go > 2.0 * light, "{go} s");
        assert!(go_pitch > 1.3 * light_pitch, "{go_pitch} > {light_pitch}");
    }

    #[test]
    fn the_moments_of_a_race_each_sound_their_own_way() {
        // (how long it is heard, how loud it is, where its pitch goes)
        let play = |sound: Sound| {
            let controls = Arc::new(Controls::default());
            controls.master.set(1.0);
            let mut mixer = Mixer::new(controls.clone());
            controls.play(sound);
            let samples: Vec<f32> = (0..3 * SAMPLE_RATE).map(|_| mixer.frame().0).collect();
            assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
            assert!(mixer.shots.is_empty(), "{sound:?} ends");
            let heard = |s: &f32| s.abs() > 0.02;
            let length = samples.iter().rposition(heard).unwrap_or(0) as f32 / SAMPLE_RATE as f32;
            let loud = samples.iter().map(|s| s.abs()).fold(0.0, f32::max);
            // How often it crosses nought over a stretch: twice its pitch.
            let pitch = |from: f32, to: f32| {
                let stretch = &samples
                    [(from * SAMPLE_RATE as f32) as usize..(to * SAMPLE_RATE as f32) as usize];
                stretch
                    .windows(2)
                    .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
                    .count() as f32
                    / (to - from)
            };
            (
                length,
                loud,
                pitch(0.0, 0.1),
                pitch(length - 0.4, length - 0.1),
            )
        };
        let (victory, loud, first, last) = play(Sound::Victory);
        assert!(victory > 1.4 && loud > 0.15, "{victory} s, {loud}");
        assert!(last > 1.5 * first, "the fanfare goes up: {first} to {last}");
        let (defeat, loud, first, last) = play(Sound::Defeat);
        assert!(defeat > 1.2 && loud > 0.15, "{defeat} s, {loud}");
        assert!(last < first, "the defeat goes down: {first} to {last}");
        let (finished, loud, ..) = play(Sound::Finished);
        assert!(finished > 0.6 && finished < victory && loud > 0.15);
        let (record, loud, first, _) = play(Sound::Record);
        assert!(
            record > 0.4 && loud > 0.15 && first > 1000.0,
            "high: {first}"
        );
        let (out, loud, ..) = play(Sound::KnockedOut);
        assert!(out > 0.2 && out < 0.6 && loud > 0.1, "short: {out} s");
    }

    #[test]
    fn single_sounds_play_and_end() {
        let controls = Arc::new(Controls::default());
        controls.master.set(1.0);
        let mut mixer = Mixer::new(controls.clone());
        controls.play(Sound::Impact {
            strength: 1.0,
            left: 1.0,
            right: 0.2,
        });
        controls.play(Sound::Click);
        let samples: Vec<_> = (0..SAMPLE_RATE / 10).map(|_| mixer.frame()).collect();
        assert!(samples.iter().any(|&(l, _)| l.abs() > 0.1), "suena");
        let louder_left: f32 = samples.iter().map(|(l, r)| l.abs() - r.abs()).sum();
        assert!(louder_left > 0.0, "suena a la izquierda");
        for _ in 0..SAMPLE_RATE {
            mixer.frame();
        }
        assert!(mixer.shots.is_empty(), "y se acaban");
    }

    #[test]
    fn panning_keeps_the_power() {
        let (l, r) = pan(1.0, 0.0);
        assert!((l * l + r * r - 1.0).abs() < 1e-5);
        assert!((l - r).abs() < 1e-5);
        let (l, r) = pan(1.0, 1.0);
        assert!(l.abs() < 1e-5 && (r - 1.0).abs() < 1e-5);
    }

    #[test]
    fn wheelie_sounds_louder_and_different() {
        let power = |wheelie| {
            let controls = engine_at(9000.0, 1.0);
            controls.engines[0].wheelie.set(wheelie);
            let samples = render(&controls, 0.5);
            samples.iter().map(|(l, _)| l * l).sum::<f32>() / samples.len() as f32
        };
        assert!(
            power(1.0) > 1.25 * power(0.0),
            "el caballito ruge con más fuerza"
        );
    }
}
