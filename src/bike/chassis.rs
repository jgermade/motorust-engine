//! Los movimientos del chasis: caballitos, suspensión e inercia.
//!
//! Son muelles con amortiguador que avanzan a paso fijo junto al resto de la
//! física, así se ven igual a cualquier tasa de fotogramas:
//!
//! - **Caballito**: la moto gira sobre la rueda trasera y levanta el morro al
//!   tirar del manillar acelerando (y un poco sola al salir a fondo). Con la
//!   rueda en el aire apenas se gira, pero se gana algo de aceleración y de
//!   punta: en las rectas compensa. Sobre la delantera, levanta la trasera en las frenadas
//!   fuertes.
//! - **Suspensión**: el chasis se agacha detrás al acelerar, se hunde delante
//!   al frenar y se comprime al aterrizar, mientras las ruedas siguen en el
//!   suelo.

use crate::geometry::smoothstep;

/// Caballito máximo (rad): más allá la moto daría la vuelta.
pub const MAX_WHEELIE: f32 = 0.6;
/// Caballito tirando del manillar a toda velocidad (rad), y la velocidad
/// (m/s) desde la que ya no se levanta más.
const FAST_WHEELIE: f32 = 0.38;
const FAST_WHEELIE_SPEED: f32 = 65.0;
/// Lo que levanta la trasera una frenada a fondo a mucha velocidad (rad).
const MAX_STOPPIE: f32 = 0.12;
/// Caballito que sale solo al acelerar a fondo desde parado (rad), y la
/// velocidad (m/s) a la que ya no sale.
const LAUNCH_LIFT: f32 = 0.12;
const LAUNCH_SPEED: f32 = 18.0;
/// Gas mínimo (fracción) para poder levantar la rueda.
const WHEELIE_MIN_THROTTLE: f32 = 0.25;
/// Lo que se puede levantar el morro en el aire tirando del manillar (rad).
const AIR_PITCH: f32 = 0.35;
/// Rigidez del caballito (rad/s² por rad): sube despacio y cae deprisa,
/// porque al bajar empuja la gravedad.
const WHEELIE_RISE: f32 = 24.0;
const WHEELIE_FALL: f32 = 55.0;
/// Amortiguación del caballito (1/s): poca, para que oscile un poco.
const WHEELIE_DAMPING: f32 = 5.0;
/// Lo que se gana con un caballito largo (fracción): de punta, y de empuje
/// del motor, que con todo el peso en la rueda de atrás tracciona mejor.
const WHEELIE_TOP_SPEED: f32 = 0.16;
const WHEELIE_DRIVE: f32 = 0.45;
/// Parte del golpe de la rueda al caer que pasa a la suspensión.
const TOUCHDOWN_KICK: f32 = 0.35;

/// Cabeceo de la suspensión por cada m/s² de aceleración (rad), y sus topes:
/// al frenar se hunde más de lo que se agacha al acelerar.
const SQUAT_PER_ACCEL: f32 = 0.0045;
const MAX_SQUAT: f32 = 0.05;
const MAX_DIVE: f32 = 0.075;
const SQUAT_STIFFNESS: f32 = 120.0;
const SQUAT_DAMPING: f32 = 10.0;
/// Recorrido de la suspensión (m): cuánto puede comprimirse y extenderse. En
/// el aire se extiende del todo y las ruedas cuelgan.
const MAX_COMPRESSION: f32 = 0.12;
const MAX_EXTENSION: f32 = 0.07;
const HEAVE_STIFFNESS: f32 = 160.0;
const HEAVE_DAMPING: f32 = 10.0;
/// Compresión por cada m/s² de aceleración vertical (en los valles), m.
const HEAVE_PER_ACCEL: f32 = 0.004;

/// Estado del chasis, además de la trayectoria de la moto.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Chassis {
    /// Caballito (rad): positivo con la rueda delantera en el aire (gira
    /// sobre la trasera) y negativo con la trasera en el aire (sobre la
    /// delantera).
    pub wheelie: f32,
    pub wheelie_rate: f32,
    /// Cabeceo de la suspensión (rad): positivo con la trasera agachada al
    /// acelerar, negativo con el morro hundido al frenar.
    pub squat: f32,
    pub squat_rate: f32,
    /// Altura del chasis respecto a la normal (m): negativa, comprimido.
    pub heave: f32,
    pub heave_rate: f32,
}

/// Lo que mueve el chasis en un paso.
#[derive(Clone, Copy, Debug)]
pub struct Forces {
    /// Cuánto tira el piloto del manillar para levantar el morro (0..1).
    pub pull: f32,
    /// Freno (0..1).
    pub brake: f32,
    /// Puño del gas (0..1).
    pub throttle: f32,
    /// Fracción del empuje máximo del motor que se está usando (0..1): se
    /// apaga al acercarse a la punta.
    pub power: f32,
    /// Velocidad hacia delante (m/s).
    pub speed: f32,
    /// Aceleración hacia delante que se nota, sin la de la cuesta (m/s²).
    pub accel: f32,
    /// Aceleración vertical de las ruedas por la forma de la carretera (m/s²).
    pub lift: f32,
    pub grounded: bool,
}

impl Chassis {
    /// Cuánto se puede girar con el caballito o la trasera levantada (0..1).
    /// How much the front is loaded by the braking (0 to 1): how far the
    /// fork has dived, of all it can. It comes as the bike brakes and goes
    /// as the fork comes back up, a moment after.
    pub fn front_load(&self) -> f32 {
        smoothstep(0.0, MAX_DIVE, -self.squat)
    }

    pub fn steering_grip(&self) -> f32 {
        1.0 - 0.85 * smoothstep(0.05, 0.4, self.wheelie)
            - 0.3 * smoothstep(0.03, 0.1, -self.wheelie)
    }

    /// Lo que se gana de punta con un caballito largo (fracción).
    pub fn wheelie_boost(&self) -> f32 {
        WHEELIE_TOP_SPEED * self.wheelie_lift()
    }

    /// Lo que empuja de más el motor con un caballito largo (fracción).
    pub fn wheelie_drive(&self) -> f32 {
        WHEELIE_DRIVE * self.wheelie_lift()
    }

    /// Cuánto cuenta el caballito para ganar velocidad (0..1): nada con la
    /// rueda apenas levantada, todo con un caballito largo.
    pub fn wheelie_lift(&self) -> f32 {
        smoothstep(0.10, 0.30, self.wheelie)
    }

    /// Avanza el chasis un paso de `dt` segundos.
    pub fn step(&self, forces: &Forces, dt: f32) -> Chassis {
        let mut next = *self;
        next.step_wheelie(forces, dt);
        next.step_suspension(forces, dt);
        next
    }

    fn step_wheelie(&mut self, forces: &Forces, dt: f32) {
        let target = if !forces.grounded {
            // En el aire se puede levantar algo el morro para aterrizar de atrás.
            forces.pull * AIR_PITCH
        } else if forces.brake > 0.0 && forces.speed > 0.0 {
            // Al frenar cae la delantera, y a fondo y deprisa se levanta la trasera.
            -MAX_STOPPIE * forces.brake * forces.brake * smoothstep(15.0, 45.0, forces.speed)
        } else if forces.speed >= 0.0 {
            let able = smoothstep(WHEELIE_MIN_THROTTLE, 0.9, forces.throttle);
            let fast = smoothstep(LAUNCH_SPEED, FAST_WHEELIE_SPEED, forces.speed);
            let height = MAX_WHEELIE + (FAST_WHEELIE - MAX_WHEELIE) * fast;
            let pulled = forces.pull * able * height;
            let launch = forces.power * LAUNCH_LIFT * (1.0 - forces.speed / LAUNCH_SPEED).max(0.0);
            pulled.max(launch)
        } else {
            0.0
        };
        let rising = (target - self.wheelie) * self.wheelie.signum() > 0.0 || self.wheelie == 0.0;
        let stiffness = if rising { WHEELIE_RISE } else { WHEELIE_FALL };
        let accel = stiffness * (target - self.wheelie) - WHEELIE_DAMPING * self.wheelie_rate;
        self.wheelie_rate += accel * dt;
        let previous = self.wheelie;
        self.wheelie = (self.wheelie + self.wheelie_rate * dt).clamp(-MAX_STOPPIE, MAX_WHEELIE);

        // Una rueda vuelve a tocar el suelo: el golpe pasa a la suspensión y
        // la moto no sigue girando hacia el otro lado.
        if previous != 0.0 && previous.signum() != self.wheelie.signum() {
            self.squat_rate += self.wheelie_rate * TOUCHDOWN_KICK;
            self.heave_rate -= self.wheelie_rate.abs() * TOUCHDOWN_KICK * 0.5;
            self.wheelie = 0.0;
            self.wheelie_rate = 0.0;
        }
        if self.wheelie.abs() >= MAX_WHEELIE {
            self.wheelie_rate = 0.0;
        }
    }

    fn step_suspension(&mut self, forces: &Forces, dt: f32) {
        // Sin apoyo, la suspensión se extiende.
        let (squat_target, heave_target) = if forces.grounded {
            (
                (forces.accel * SQUAT_PER_ACCEL).clamp(-MAX_DIVE, MAX_SQUAT),
                (-forces.lift * HEAVE_PER_ACCEL).clamp(-MAX_COMPRESSION, MAX_EXTENSION),
            )
        } else {
            (0.0, MAX_EXTENSION)
        };
        let squat_accel =
            SQUAT_STIFFNESS * (squat_target - self.squat) - SQUAT_DAMPING * self.squat_rate;
        self.squat_rate += squat_accel * dt;
        self.squat += self.squat_rate * dt;

        let heave_accel =
            HEAVE_STIFFNESS * (heave_target - self.heave) - HEAVE_DAMPING * self.heave_rate;
        self.heave_rate += heave_accel * dt;
        self.heave += self.heave_rate * dt;
        if !(-MAX_COMPRESSION..=MAX_EXTENSION).contains(&self.heave) {
            // Hace tope: se para en seco.
            self.heave = self.heave.clamp(-MAX_COMPRESSION, MAX_EXTENSION);
            self.heave_rate = 0.0;
        }
    }

    /// El golpe de aterrizar con `impact` m/s de velocidad vertical
    /// (negativa, hacia abajo) comprime la suspensión.
    pub fn land(&mut self, impact: f32) {
        self.heave_rate += impact.min(0.0) * 0.6;
        // Si cae con el morro levantado, la delantera golpea después.
        self.squat_rate += impact.min(0.0) * 0.08;
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 120.0;

    fn forces() -> Forces {
        Forces {
            pull: 0.0,
            brake: 0.0,
            throttle: 0.0,
            power: 0.0,
            speed: 30.0,
            accel: 0.0,
            lift: 0.0,
            grounded: true,
        }
    }

    fn run(mut chassis: Chassis, forces: Forces, seconds: f32) -> (Chassis, f32, f32) {
        let (mut highest, mut lowest) = (f32::MIN, f32::MAX);
        for _ in 0..(seconds / DT) as usize {
            chassis = chassis.step(&forces, DT);
            highest = highest.max(chassis.wheelie);
            lowest = lowest.min(chassis.wheelie);
        }
        (chassis, highest, lowest)
    }

    #[test]
    fn pulling_while_accelerating_lifts_the_front_without_looping() {
        let wheelie = Forces {
            pull: 1.0,
            throttle: 1.0,
            power: 0.8,
            ..forces()
        };
        let (chassis, highest, _) = run(Chassis::default(), wheelie, 3.0);
        assert!(chassis.wheelie > 0.3, "caballito de {}", chassis.wheelie);
        assert!(highest <= MAX_WHEELIE);
        assert!(
            chassis.steering_grip() < 0.4,
            "con la rueda en el aire no se gira"
        );
        assert!(chassis.wheelie_boost() > 0.0);
        assert!(chassis.wheelie_drive() > 0.0);
    }

    #[test]
    fn without_throttle_the_front_does_not_lift() {
        let coasting = Forces {
            pull: 1.0,
            ..forces()
        };
        let (_, highest, _) = run(Chassis::default(), coasting, 2.0);
        assert!(highest < 0.01);
    }

    #[test]
    fn releasing_drops_the_front_and_the_fork_takes_the_hit() {
        let wheelie = Forces {
            pull: 1.0,
            throttle: 1.0,
            power: 0.8,
            ..forces()
        };
        let (lifted, _, _) = run(Chassis::default(), wheelie, 3.0);
        let mut chassis = lifted;
        let mut dived = 0.0f32;
        for _ in 0..240 {
            chassis = chassis.step(&forces(), DT);
            dived = dived.min(chassis.squat);
        }
        assert!(chassis.wheelie.abs() < 0.01, "vuelve al suelo");
        assert!(dived < -0.01, "el golpe hunde el morro");
    }

    #[test]
    fn at_top_speed_the_wheelie_is_lower_but_still_pays() {
        let flat_out = Forces {
            pull: 1.0,
            throttle: 1.0,
            power: 0.05,
            speed: 74.0,
            ..forces()
        };
        let (chassis, highest, _) = run(Chassis::default(), flat_out, 3.0);
        assert!(highest < FAST_WHEELIE + 0.05, "sube {highest}");
        assert!(
            chassis.wheelie > 0.25,
            "pero se sostiene: {}",
            chassis.wheelie
        );
        assert!(chassis.wheelie_boost() > 0.03);
    }

    #[test]
    fn full_throttle_from_standstill_lifts_a_little_by_itself() {
        let launch = Forces {
            throttle: 1.0,
            power: 1.0,
            speed: 2.0,
            ..forces()
        };
        let (_, highest, _) = run(Chassis::default(), launch, 1.0);
        assert!(highest > 0.02 && highest < 0.12, "sube {highest}");
    }

    #[test]
    fn hard_braking_dives_and_lifts_the_rear() {
        let braking = Forces {
            brake: 1.0,
            speed: 50.0,
            accel: -28.0,
            ..forces()
        };
        let (chassis, _, lowest) = run(Chassis::default(), braking, 1.0);
        assert!(chassis.squat < -0.05, "morro hundido: {}", chassis.squat);
        assert!(lowest < -0.05, "trasera levantada: {lowest}");
    }

    #[test]
    fn accelerating_squats_the_rear() {
        let accelerating = Forces {
            accel: 10.0,
            throttle: 1.0,
            power: 0.8,
            ..forces()
        };
        let (chassis, _, _) = run(Chassis::default(), accelerating, 1.0);
        assert!(chassis.squat > 0.03);
    }

    #[test]
    fn in_the_air_the_suspension_relaxes() {
        let flying = Forces {
            grounded: false,
            ..forces()
        };
        let (chassis, _, _) = run(Chassis::default(), flying, 0.3);
        assert!(chassis.heave > 0.05, "extendida: {}", chassis.heave);
        assert!(chassis.heave <= MAX_EXTENSION);
    }

    #[test]
    fn landing_compresses_then_settles_within_travel() {
        let mut chassis = Chassis::default();
        chassis.land(-6.0);
        let mut deepest = 0.0f32;
        for _ in 0..360 {
            chassis = chassis.step(&forces(), DT);
            deepest = deepest.min(chassis.heave);
        }
        assert!(deepest < -0.08, "comprime {deepest}");
        assert!(deepest >= -MAX_COMPRESSION);
        assert!(chassis.heave.abs() < 0.01, "vuelve a su sitio");
    }
}
