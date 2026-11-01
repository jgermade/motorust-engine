//! Decoraciones de las motos: colores, dibujos del carenado, mono y casco.
//!
//! Los dibujos se describen con datos y se pintan en texturas al crear cada
//! moto, sin ficheros de imagen:
//!
//! - El carenado se pinta en una vista lateral de la moto (eje Z de la moto en
//!   horizontal, Y en vertical) que luego se proyecta de lado sobre todas las
//!   piezas de carrocería, como hacían muchos juegos de la época.
//! - El casco se pinta en función de la dirección desde su centro, así las
//!   franjas se describen como bandas alrededor de un eje.

use crate::world::generate_mipmaps;
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// Zona de la moto que cubre la textura del carenado, en el espacio de la moto.
const SIDE_Z: (f32, f32) = (-1.0, 0.9);
const SIDE_Y: (f32, f32) = (0.1, 1.2);
const BODYWORK_SIZE: (u32, u32) = (768, 384);
const HELMET_SIZE: (u32, u32) = (256, 128);
/// Muestras por píxel y eje al pintar: suaviza los bordes de los dibujos.
const SUPERSAMPLING: u32 = 2;

/// Colores y dorsal de una moto y su piloto.
#[derive(Clone, Copy, Debug)]
pub struct Livery {
    pub number: u8,
    pub plate: Color,
    pub number_ink: Color,
    pub bodywork: Bodywork,
    pub suit: Color,
    pub suit_trim: Color,
    pub helmet: Helmet,
    pub rims: Color,
    /// Color de los filetes del carenado.
    pub accent: Color,
}

/// Pintura del carenado: un color base y capas encima, en orden.
#[derive(Clone, Copy, Debug)]
pub struct Bodywork {
    pub base: Color,
    pub layers: &'static [Layer],
}

/// Una zona de color del carenado: un polígono en la vista lateral, con
/// vértices `(z, y)` en metros en el espacio de la moto (-Z es delante).
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub color: Color,
    pub outline: &'static [Vec2],
}

/// Detalle de un diseño de carenado, pintado encima de la decoración: lo que
/// en las láminas son tomas de aire, rejillas y filetes. Va en la vista
/// lateral, como las capas.
#[derive(Clone, Copy, Debug)]
pub enum Detail {
    /// Toma de aire o abertura: un hueco oscuro.
    Vent(&'static [Vec2]),
    /// Rejilla: un cuadrilátero `[a, b, c, d]` con `slats` lamas paralelas a
    /// `a`→`b`, apiladas de `a` hacia `d`.
    Grille {
        outline: &'static [Vec2; 4],
        slats: u8,
    },
    /// Filete del color de acento: una línea quebrada de `width` metros.
    Pinstripe { points: &'static [Vec2], width: f32 },
}

/// Fondo de las tomas de aire y lamas de las rejillas.
const VENT: Color = Color::srgb(0.035, 0.035, 0.045);
const SLAT: Color = Color::srgb(0.2, 0.2, 0.22);

impl Detail {
    fn color_at(&self, point: Vec2, accent: Color) -> Option<Color> {
        match *self {
            Detail::Vent(outline) => contains(outline, point).then_some(VENT),
            Detail::Grille { outline, slats } => {
                if !contains(outline, point) {
                    return None;
                }
                let [a, b, _, d] = *outline;
                let across = (b - a).perp();
                let t = (point - a).dot(across) / (d - a).dot(across);
                let slat = (t * slats as f32).rem_euclid(1.0);
                Some(if slat < 0.55 { VENT } else { SLAT })
            }
            Detail::Pinstripe { points, width } => points
                .windows(2)
                .any(|pair| distance_to_segment(point, pair[0], pair[1]) < width / 2.0)
                .then_some(accent),
        }
    }
}

fn distance_to_segment(point: Vec2, from: Vec2, to: Vec2) -> f32 {
    let along = to - from;
    let t = ((point - from).dot(along) / along.length_squared()).clamp(0.0, 1.0);
    point.distance(from + along * t)
}

#[derive(Clone, Copy, Debug)]
pub struct Helmet {
    pub base: Color,
    pub stripes: &'static [Stripe],
}

/// Franja del casco, descrita por las direcciones desde su centro (Y arriba,
/// -Z delante).
#[derive(Clone, Copy, Debug)]
pub enum Stripe {
    /// Banda alrededor del casco: direcciones cuyo producto con `axis` cae
    /// entre `from` y `to`. Con el eje inclinado hacia atrás, la banda baja
    /// por la nuca.
    Band {
        axis: Vec3,
        from: f32,
        to: f32,
        color: Color,
    },
    /// Franja de delante a atrás por encima del casco.
    Crown { half_width: f32, color: Color },
}

/// The player's: blue and white, with the 8 on a yellow plate: the screen,
/// the tank and the tail blue, the side white, with red pinstripes.
pub const COVER: Livery = Livery {
    number: 8,
    plate: Color::srgb(1.0, 0.8, 0.05),
    number_ink: Color::srgb(0.03, 0.03, 0.03),
    bodywork: Bodywork {
        base: Color::srgb(0.07, 0.3, 0.8),
        layers: &[Layer {
            color: Color::srgb(0.93, 0.94, 0.96),
            outline: &[
                Vec2::new(-0.66, 0.7),
                Vec2::new(-0.42, 0.18),
                Vec2::new(0.04, 0.18),
                Vec2::new(-0.03, 0.42),
                Vec2::new(-0.15, 0.7),
            ],
        }],
    },
    suit: Color::srgb(0.08, 0.22, 0.8),
    suit_trim: Color::srgb(0.95, 0.95, 0.95),
    helmet: Helmet {
        base: Color::srgb(0.96, 0.96, 0.96),
        stripes: &[Stripe::Crown {
            half_width: 0.13,
            color: Color::srgb(0.85, 0.08, 0.08),
        }],
    },
    rims: Color::srgb(0.1, 0.2, 0.62),
    accent: Color::srgb(0.85, 0.08, 0.08),
};

const ROCKET_RED: Color = Color::srgb(1.0, 0.24, 0.1);
const DARK_RIMS: Color = Color::srgb(0.16, 0.16, 0.17);
const GLOSS_WHITE: Color = Color::srgb(0.96, 0.96, 0.96);

/// Blanco brillante con la parte de arriba en rojo fluorescente, que en los
/// costados forma una flecha hacia delante, y el casco amarillo con una
/// franja roja sobre otra negra.
pub const FLECHA: Livery = Livery {
    number: 12,
    plate: GLOSS_WHITE,
    number_ink: Color::srgb(0.03, 0.03, 0.03),
    bodywork: Bodywork {
        base: GLOSS_WHITE,
        layers: &[Layer {
            color: ROCKET_RED,
            outline: &[
                Vec2::new(-1.1, 1.2),
                Vec2::new(-1.1, 0.82),
                Vec2::new(-0.66, 0.78),
                // La flecha: la punta va hacia delante, en el costado, y el
                // blanco queda por debajo.
                Vec2::new(-0.46, 0.6),
                Vec2::new(0.25, 0.7),
                Vec2::new(0.25, 0.88),
                Vec2::new(0.9, 0.92),
                Vec2::new(0.9, 1.2),
            ],
        }],
    },
    suit: GLOSS_WHITE,
    suit_trim: ROCKET_RED,
    helmet: Helmet {
        base: Color::srgb(0.98, 0.84, 0.05),
        stripes: &[
            Stripe::Band {
                axis: Vec3::new(0.0, 1.0, 0.35),
                from: 0.42,
                to: 0.52,
                color: ROCKET_RED,
            },
            Stripe::Band {
                axis: Vec3::new(0.0, 1.0, 0.35),
                from: 0.3,
                to: 0.42,
                color: ALMOST_BLACK,
            },
        ],
    },
    rims: DARK_RIMS,
    accent: Color::srgb(0.98, 0.84, 0.05),
};

const SUN_YELLOW: Color = Color::srgb(1.0, 0.8, 0.0);
const DEEP_BLUE: Color = Color::srgb(0.04, 0.24, 0.7);
const LIGHT_BLUE: Color = Color::srgb(0.35, 0.65, 0.95);

/// Delantera amarilla que pasa a azul en diagonal, con un filete azul claro,
/// y el casco blanco con franjas amarilla y azul.
pub const DIAGONAL: Livery = Livery {
    number: 5,
    plate: GLOSS_WHITE,
    number_ink: DEEP_BLUE,
    bodywork: Bodywork {
        base: DEEP_BLUE,
        layers: &[
            Layer {
                color: LIGHT_BLUE,
                outline: &[
                    Vec2::new(-1.1, 1.2),
                    Vec2::new(-1.1, 0.2),
                    Vec2::new(-0.02, 0.2),
                    Vec2::new(-0.54, 1.2),
                ],
            },
            Layer {
                color: SUN_YELLOW,
                outline: &[
                    Vec2::new(-1.1, 1.2),
                    Vec2::new(-1.1, 0.2),
                    Vec2::new(-0.08, 0.2),
                    Vec2::new(-0.6, 1.2),
                ],
            },
            // Colín azul con la punta amarilla.
            Layer {
                color: SUN_YELLOW,
                outline: &[
                    Vec2::new(0.5, 0.8),
                    Vec2::new(0.9, 0.8),
                    Vec2::new(0.9, 1.2),
                    Vec2::new(0.58, 1.2),
                ],
            },
        ],
    },
    suit: DEEP_BLUE,
    suit_trim: SUN_YELLOW,
    helmet: Helmet {
        base: GLOSS_WHITE,
        stripes: &[
            Stripe::Band {
                axis: Vec3::new(0.0, 1.0, 0.3),
                from: 0.24,
                to: 0.32,
                color: DEEP_BLUE,
            },
            Stripe::Band {
                axis: Vec3::new(0.0, 1.0, 0.3),
                from: 0.32,
                to: 0.44,
                color: SUN_YELLOW,
            },
            Stripe::Crown {
                half_width: 0.06,
                color: LIGHT_BLUE,
            },
        ],
    },
    rims: DARK_RIMS,
    accent: LIGHT_BLUE,
};

const LIME: Color = Color::srgb(0.38, 0.75, 0.1);
const ALMOST_BLACK: Color = Color::srgb(0.06, 0.06, 0.07);

/// Lime green, the fairing opened in black.
pub const VERDE: Livery = Livery {
    number: 10,
    plate: GLOSS_WHITE,
    number_ink: ALMOST_BLACK,
    bodywork: Bodywork {
        base: LIME,
        layers: &[Layer {
            color: ALMOST_BLACK,
            outline: &[
                Vec2::new(-0.32, 0.62),
                Vec2::new(0.12, 0.5),
                Vec2::new(0.12, 0.42),
                Vec2::new(-0.08, 0.26),
            ],
        }],
    },
    suit: ALMOST_BLACK,
    suit_trim: LIME,
    helmet: Helmet {
        base: LIME,
        stripes: &[
            Stripe::Band {
                axis: Vec3::new(0.0, 1.0, 0.35),
                from: 0.3,
                to: 0.42,
                color: ALMOST_BLACK,
            },
            Stripe::Crown {
                half_width: 0.08,
                color: GLOSS_WHITE,
            },
        ],
    },
    rims: DARK_RIMS,
    accent: GLOSS_WHITE,
};

const RACING_RED: Color = Color::srgb(0.8, 0.06, 0.08);
const RACING_BLUE: Color = Color::srgb(0.1, 0.17, 0.55);

/// Red with a blue side and white pinstripes.
pub const TRICOLOR: Livery = Livery {
    number: 33,
    plate: GLOSS_WHITE,
    number_ink: ALMOST_BLACK,
    bodywork: Bodywork {
        base: RACING_RED,
        layers: &[
            Layer {
                color: RACING_BLUE,
                outline: &[
                    Vec2::new(-0.66, 0.72),
                    Vec2::new(-0.3, 0.5),
                    Vec2::new(0.2, 0.52),
                    Vec2::new(0.2, 0.8),
                    Vec2::new(-0.4, 0.86),
                ],
            },
            Layer {
                color: GLOSS_WHITE,
                outline: &[
                    Vec2::new(-0.34, 0.5),
                    Vec2::new(-0.27, 0.5),
                    Vec2::new(-0.11, 0.76),
                    Vec2::new(-0.18, 0.76),
                ],
            },
            // Filetes azul y blanco a lo largo del colín.
            Layer {
                color: RACING_BLUE,
                outline: &[
                    Vec2::new(0.3, 0.8),
                    Vec2::new(0.9, 0.93),
                    Vec2::new(0.9, 0.955),
                    Vec2::new(0.3, 0.825),
                ],
            },
            Layer {
                color: GLOSS_WHITE,
                outline: &[
                    Vec2::new(0.3, 0.825),
                    Vec2::new(0.9, 0.955),
                    Vec2::new(0.9, 0.97),
                    Vec2::new(0.3, 0.84),
                ],
            },
        ],
    },
    suit: RACING_RED,
    suit_trim: GLOSS_WHITE,
    helmet: Helmet {
        base: GLOSS_WHITE,
        stripes: &[
            Stripe::Band {
                axis: Vec3::new(0.0, 1.0, 0.35),
                from: 0.25,
                to: 0.36,
                color: RACING_RED,
            },
            Stripe::Band {
                axis: Vec3::new(0.0, 1.0, 0.35),
                from: 0.36,
                to: 0.44,
                color: RACING_BLUE,
            },
        ],
    },
    rims: DARK_RIMS,
    accent: GLOSS_WHITE,
};

const ROSSO_RED: Color = Color::srgb(0.86, 0.08, 0.06);

/// Red, the top of the tank silver.
pub const ROSSO: Livery = Livery {
    number: 63,
    plate: GLOSS_WHITE,
    number_ink: ALMOST_BLACK,
    bodywork: Bodywork {
        base: ROSSO_RED,
        layers: &[Layer {
            color: Color::srgb(0.75, 0.75, 0.77),
            outline: &[
                Vec2::new(-0.3, 0.93),
                Vec2::new(0.14, 0.89),
                Vec2::new(0.14, 1.2),
                Vec2::new(-0.3, 1.2),
            ],
        }],
    },
    suit: ROSSO_RED,
    suit_trim: ALMOST_BLACK,
    helmet: Helmet {
        base: ROSSO_RED,
        stripes: &[
            Stripe::Band {
                axis: Vec3::new(0.0, 1.0, 0.3),
                from: 0.3,
                to: 0.4,
                color: GLOSS_WHITE,
            },
            Stripe::Crown {
                half_width: 0.05,
                color: ALMOST_BLACK,
            },
        ],
    },
    rims: DARK_RIMS,
    accent: GLOSS_WHITE,
};

/// Coordenadas de textura de una pieza de carrocería: la vista lateral
/// proyectada sobre sus vértices (que están en el espacio de la moto).
pub fn project_side_uvs(mesh: &mut Mesh) {
    let uvs: Vec<[f32; 2]> = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|positions| positions.as_float3())
        .expect("la pieza tiene posiciones")
        .iter()
        .map(|&[_, y, z]| side_uv(z, y))
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
}

fn side_uv(z: f32, y: f32) -> [f32; 2] {
    [
        (z - SIDE_Z.0) / (SIDE_Z.1 - SIDE_Z.0),
        (SIDE_Y.1 - y) / (SIDE_Y.1 - SIDE_Y.0),
    ]
}

/// Textura del carenado: la vista lateral pintada con las capas de la
/// decoración y, encima, los detalles del diseño, con los filetes del color
/// `accent`.
pub fn bodywork_texture(bodywork: &Bodywork, details: &[Detail], accent: Color) -> Image {
    paint(BODYWORK_SIZE, |u, v| {
        let point = Vec2::new(SIDE_Z.0.lerp(SIDE_Z.1, u), SIDE_Y.1.lerp(SIDE_Y.0, v));
        if let Some(color) = details
            .iter()
            .rev()
            .find_map(|detail| detail.color_at(point, accent))
        {
            return color;
        }
        bodywork
            .layers
            .iter()
            .rev()
            .find(|layer| contains(layer.outline, point))
            .map_or(bodywork.base, |layer| layer.color)
    })
}

/// Textura del casco, para la esfera de Bevy (polos en ±Z).
pub fn helmet_texture(helmet: &Helmet) -> Image {
    paint(HELMET_SIZE, |u, v| {
        // Inversa del mapeado de `Sphere::mesh().uv(..)`.
        let latitude = std::f32::consts::FRAC_PI_2 - v * std::f32::consts::PI;
        let longitude = u * std::f32::consts::TAU;
        let direction = Vec3::new(
            latitude.cos() * longitude.cos(),
            latitude.cos() * longitude.sin(),
            latitude.sin(),
        );
        if in_visor(direction) {
            return VISOR;
        }
        helmet
            .stripes
            .iter()
            .rev()
            .find(|stripe| stripe.covers(direction))
            .map_or(helmet.base, |stripe| stripe.color())
    })
}

/// Visera ahumada, pintada encima de la decoración del casco.
const VISOR: Color = Color::srgb(0.04, 0.05, 0.08);

/// Si una dirección desde el centro del casco (Y arriba, -Z delante) cae en
/// la ventana de la visera: una franja a la altura de los ojos que rodea el
/// frente y se estrecha hacia los lados.
fn in_visor(direction: Vec3) -> bool {
    let front = -direction.z;
    let top = 0.12 + 0.24 * front;
    front > 0.05 && (-0.06..top).contains(&direction.y)
}

impl Stripe {
    fn covers(&self, direction: Vec3) -> bool {
        match *self {
            Stripe::Band { axis, from, to, .. } => {
                (from..to).contains(&direction.dot(axis.normalize()))
            }
            Stripe::Crown { half_width, .. } => direction.x.abs() < half_width && direction.y > 0.0,
        }
    }

    fn color(&self) -> Color {
        match *self {
            Stripe::Band { color, .. } | Stripe::Crown { color, .. } => color,
        }
    }
}

/// Pinta una imagen evaluando `color_at(u, v)` varias veces por píxel.
fn paint(size: (u32, u32), color_at: impl Fn(f32, f32) -> Color) -> Image {
    let (width, height) = size;
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    let samples = (SUPERSAMPLING * SUPERSAMPLING) as f32;
    for y in 0..height {
        for x in 0..width {
            let mut sum = LinearRgba::NONE;
            for sy in 0..SUPERSAMPLING {
                for sx in 0..SUPERSAMPLING {
                    let u = (x as f32 + (sx as f32 + 0.5) / SUPERSAMPLING as f32) / width as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SUPERSAMPLING as f32) / height as f32;
                    sum += color_at(u, v).to_linear();
                }
            }
            let average = Color::from(sum * (1.0 / samples)).to_srgba();
            data.extend(average.to_u8_array());
        }
    }
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Sin mipmaps, las motos lejanas brillarían y parpadearían.
    generate_mipmaps(&mut image);
    image
}

/// ¿Está `point` dentro del polígono? (regla par-impar)
fn contains(outline: &[Vec2], point: Vec2) -> bool {
    let mut inside = false;
    let mut previous = outline[outline.len() - 1];
    for &current in outline {
        let crosses = (current.y > point.y) != (previous.y > point.y);
        if crosses {
            let x = previous.x
                + (point.y - previous.y) / (current.y - previous.y) * (current.x - previous.x);
            if point.x < x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn polygon_contains_its_inside_only() {
        let square = [
            Vec2::ZERO,
            Vec2::new(1.0, 0.0),
            Vec2::ONE,
            Vec2::new(0.0, 1.0),
        ];
        assert!(contains(&square, Vec2::splat(0.5)));
        assert!(!contains(&square, Vec2::new(1.5, 0.5)));
        assert!(!contains(&square, Vec2::new(0.5, -0.1)));
    }

    #[test]
    fn side_projection_covers_the_bike() {
        // Morro, colín, quilla y cúpula caen dentro de la textura.
        for (z, y) in [(-1.0, 0.84), (0.66, 1.03), (0.0, 0.3), (-0.6, 1.06)] {
            let [u, v] = side_uv(z, y);
            assert!(
                (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v),
                "({z}, {y})"
            );
        }
    }

    #[test]
    fn details_paint_vents_grilles_and_pinstripes() {
        const SQUARE: [Vec2; 4] = [
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ];
        const LINE: [Vec2; 2] = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)];
        let accent = Color::srgb(1.0, 0.0, 0.0);

        let vent = Detail::Vent(&SQUARE);
        assert_eq!(vent.color_at(Vec2::splat(0.5), accent), Some(VENT));
        assert_eq!(vent.color_at(Vec2::splat(1.5), accent), None);

        // Cuatro lamas horizontales: oscuro abajo de cada una, claro arriba.
        let grille = Detail::Grille {
            outline: &SQUARE,
            slats: 4,
        };
        assert_eq!(grille.color_at(Vec2::new(0.5, 0.05), accent), Some(VENT));
        assert_eq!(grille.color_at(Vec2::new(0.5, 0.2), accent), Some(SLAT));
        assert_eq!(grille.color_at(Vec2::new(0.5, 0.3), accent), Some(VENT));

        let stripe = Detail::Pinstripe {
            points: &LINE,
            width: 0.1,
        };
        assert_eq!(stripe.color_at(Vec2::new(0.5, 0.04), accent), Some(accent));
        assert_eq!(stripe.color_at(Vec2::new(0.5, 0.06), accent), None);
    }

    #[test]
    fn flecha_helmet_has_red_above_black() {
        let at = |height: f32| {
            let direction = Vec3::new(0.0, height, -(1.0 - height * height).sqrt());
            FLECHA
                .helmet
                .stripes
                .iter()
                .find(|s| s.covers(direction))
                .map(|s| s.color())
        };
        // Mirando de frente, la franja roja va por encima de la negra.
        let (mut red_height, mut black_height) = (None, None);
        for step in 0..100 {
            let height = step as f32 / 100.0;
            match at(height) {
                Some(c) if c == ROCKET_RED => red_height = Some(height),
                Some(c) if c == ALMOST_BLACK => black_height = Some(height),
                _ => {}
            }
        }
        assert!(red_height.unwrap() > black_height.unwrap());
    }
}
