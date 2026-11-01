//! Convenciones del plano del suelo y utilidades numéricas compartidas.
//!
//! El juego ocurre sobre el plano XZ de Bevy (Y hacia arriba). Las posiciones
//! sobre el suelo se guardan como `Vec2(x, z)` y la orientación como un ángulo
//! de guiñada (*heading*): 0 mira hacia -Z y los valores positivos giran a la
//! izquierda, igual que `Quat::from_rotation_y`.

use std::f32::consts::{PI, TAU};

use bevy::math::{FloatExt, Vec2, Vec3};

/// Convierte un punto del suelo a coordenadas del mundo a la altura `y`.
pub fn to_world(point: Vec2, y: f32) -> Vec3 {
    Vec3::new(point.x, y, point.y)
}

/// Vector unitario hacia delante para una orientación dada.
pub fn forward(heading: f32) -> Vec2 {
    Vec2::new(-heading.sin(), -heading.cos())
}

/// Orientación que mira en la dirección `direction`.
pub fn heading_of(direction: Vec2) -> f32 {
    (-direction.x).atan2(-direction.y)
}

/// Lleva un ángulo al rango [-π, π).
pub fn wrap_angle(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// Acerca `current` a `target` con un suavizado exponencial que no depende del
/// paso de tiempo. `rate` es la rapidez de respuesta (1/s): cuanto mayor, antes
/// llega.
pub fn approach(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    target + (current - target) * (-rate * dt).exp()
}

/// Interpolación suave entre 0 (en `edge0`) y 1 (en `edge1`).
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Número pseudoaleatorio en [0, 1) a partir de un índice (hash PCG). El
/// escenario sale igual en cada partida sin depender de una crate de azar.
pub fn random01(index: u32) -> f32 {
    let state = index.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    let hash = (word >> 22) ^ word;
    (hash >> 8) as f32 / (1 << 24) as f32
}

/// Ruido de valor en 2D, suave y en [0, 1]: valores al azar en una rejilla de
/// 1 × 1 interpolados entre sí.
pub fn value_noise(point: Vec2) -> f32 {
    let cell = point.floor();
    let fraction = point - cell;
    let (x, y) = (cell.x as i32, cell.y as i32);
    let corner = |dx: i32, dy: i32| {
        let key = (x + dx).wrapping_mul(73_856_093) ^ (y + dy).wrapping_mul(19_349_663);
        random01(key as u32)
    };
    let s = fraction * fraction * (Vec2::splat(3.0) - 2.0 * fraction);
    let bottom = corner(0, 0).lerp(corner(1, 0), s.x);
    let top = corner(0, 1).lerp(corner(1, 1), s.x);
    bottom.lerp(top, s.y)
}

/// Ruido fractal: varias capas de [`value_noise`] cada vez más finas y
/// suaves, normalizado a [0, 1].
pub fn fbm(point: Vec2, octaves: u32) -> f32 {
    let (mut sum, mut amplitude, mut total, mut frequency) = (0.0, 1.0, 0.0, 1.0);
    for octave in 0..octaves {
        // Cada capa se desplaza para que no coincidan sus rejillas.
        let shift = Vec2::splat(octave as f32 * 17.3);
        sum += amplitude * value_noise(point * frequency + shift);
        total += amplitude;
        amplitude *= 0.5;
        frequency *= 2.03;
    }
    sum / total
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use bevy::math::Quat;

    use super::*;

    #[test]
    fn heading_matches_bevy_rotation() {
        for heading in [0.0, 0.7, -2.0, PI] {
            let bevy_forward = Quat::from_rotation_y(heading) * Vec3::NEG_Z;
            assert!(bevy_forward.abs_diff_eq(to_world(forward(heading), 0.0), 1e-5));
        }
    }

    #[test]
    fn heading_of_inverts_forward() {
        for heading in [0.0, 1.0, -1.5, 3.0] {
            assert!((wrap_angle(heading_of(forward(heading)) - heading)).abs() < 1e-5);
        }
    }

    #[test]
    fn positive_heading_turns_left() {
        // Desde la orientación inicial (mirando a -Z), la izquierda es -X.
        assert!(forward(0.1).x < 0.0);
    }

    #[test]
    fn noise_stays_in_range_and_is_continuous() {
        for i in 0..2000 {
            let point = Vec2::new(i as f32 * 0.37 - 300.0, i as f32 * 0.11 - 80.0);
            let value = fbm(point, 4);
            assert!((0.0..=1.0).contains(&value));
            let nearby = fbm(point + Vec2::splat(1e-3), 4);
            assert!((value - nearby).abs() < 0.01);
        }
    }

    #[test]
    fn random01_stays_in_range() {
        for index in 0..10_000 {
            assert!((0.0..1.0).contains(&random01(index)));
        }
    }

    #[test]
    fn approach_converges_without_overshooting() {
        let mut value = 0.0;
        for _ in 0..1000 {
            value = approach(value, 1.0, 5.0, 1.0 / 120.0);
            assert!(value <= 1.0);
        }
        assert!((value - 1.0).abs() < 1e-4);
    }
}
