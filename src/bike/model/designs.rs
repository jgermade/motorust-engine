//! Diseños de carrocería, sacados de láminas *low poly* de superbikes
//! actuales (vistas lateral, frontal y trasera). Cada uno es una lista de
//! piezas facetadas con las medidas tomadas de la vista lateral, a escala con
//! la batalla de la moto. No llevan logotipos: solo la forma.
//!
//! Lo común a todos (chasis, motor, basculante, estriberas...) está en
//! [`common`]; ruedas, horquilla y piloto son iguales en todas las motos y van
//! aparte.

use bevy::prelude::*;

use super::imported::{self, Model};
use super::livery::Detail;
use super::{Mount, Paint, Parts, Plate, REAR_AXLE, WHEEL_RADIUS, plate};
use crate::shapes::{Section, section};

/// Forma de la carrocería de una moto.
///
/// A file (the roster) names a design built in code by its name, `Afilada`,
/// and a model by its folder in `assets/models/`, `Model("racer")`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "DesignFile")]
pub enum Design {
    /// Morro afilado con faros rasgados en los costados, retrovisores en la
    /// cúpula y escape corto junto a la rueda.
    Afilada,
    /// Frontal vertical, quilla larga y escape largo subiendo hacia el colín.
    Escudo,
    /// Morro bajo con alerones, colín como una cuchilla, escape bajo el motor
    /// y basculante monobrazo.
    Alerones,
    /// Faro central en rombo, quilla oscura, retrovisores y un escape grande
    /// y hexagonal.
    Rombo,
    /// A model made elsewhere (with Meshy), with its own rider (see
    /// [`imported`]).
    Model(&'static Model),
}

/// How a file writes a [`Design`].
#[derive(serde::Deserialize)]
enum DesignFile {
    Afilada,
    Escudo,
    Alerones,
    Rombo,
    Model(String),
}

impl TryFrom<DesignFile> for Design {
    type Error = String;

    fn try_from(file: DesignFile) -> Result<Self, String> {
        Ok(match file {
            DesignFile::Afilada => Design::Afilada,
            DesignFile::Escudo => Design::Escudo,
            DesignFile::Alerones => Design::Alerones,
            DesignFile::Rombo => Design::Rombo,
            DesignFile::Model(folder) => Design::Model(imported::model(&folder)?),
        })
    }
}

impl Design {
    /// The designs built in code.
    pub const BUILT_IN_CODE: [Design; 4] = [
        Design::Afilada,
        Design::Escudo,
        Design::Alerones,
        Design::Rombo,
    ];

    /// Every design: those built in code, then the models named so far (by
    /// the roster), in the order of [`Design::index`].
    pub fn all() -> Vec<Design> {
        Self::BUILT_IN_CODE
            .into_iter()
            .chain(imported::models().into_iter().map(Design::Model))
            .collect()
    }

    pub fn index(self) -> usize {
        match self {
            Design::Afilada => 0,
            Design::Escudo => 1,
            Design::Alerones => 2,
            Design::Rombo => 3,
            Design::Model(model) => Self::BUILT_IN_CODE.len() + model.index(),
        }
    }

    /// El modelo de los diseños importados.
    pub(super) fn model(self) -> Option<&'static Model> {
        match self {
            Design::Model(model) => Some(model),
            _ => None,
        }
    }
}

/// Every design of `roster`'s: those built in code, then its models, for
/// the tests.
#[cfg(any(test, feature = "testing"))]
pub fn every(roster: &crate::bike::Roster) -> Vec<Design> {
    let mut designs = Design::BUILT_IN_CODE.to_vec();
    for entry in roster.iter() {
        if !designs.contains(&entry.design()) {
            designs.push(entry.design());
        }
    }
    designs
}

/// Contornos de las piezas facetadas (ver [`crate::shapes::faceted`]): de -1
/// a 1, en sentido antihorario visto desde detrás.
///
/// Cúpula y morro: hombros achaflanados y lomo plano.
const COWL: &[Vec2] = &[
    Vec2::new(1.0, -0.5),
    Vec2::new(1.0, 0.25),
    Vec2::new(0.7, 0.8),
    Vec2::new(0.25, 1.0),
    Vec2::new(-0.25, 1.0),
    Vec2::new(-0.7, 0.8),
    Vec2::new(-1.0, 0.25),
    Vec2::new(-1.0, -0.5),
    Vec2::new(-0.55, -1.0),
    Vec2::new(0.55, -1.0),
];
/// Cúpula ahumada: una cuña en arco que nace dentro del morro.
const SCREEN: &[Vec2] = &[
    Vec2::new(1.0, -1.0),
    Vec2::new(0.75, 0.35),
    Vec2::new(0.3, 0.9),
    Vec2::new(0.0, 1.0),
    Vec2::new(-0.3, 0.9),
    Vec2::new(-0.75, 0.35),
    Vec2::new(-1.0, -1.0),
];
/// Carenado: costados verticales (donde va el dorsal), un hombro ancho que
/// se mete hacia el depósito y quilla en V.
const FAIRING: &[Vec2] = &[
    Vec2::new(1.0, -0.1),
    Vec2::new(1.0, 0.55),
    Vec2::new(0.82, 1.0),
    Vec2::new(-0.82, 1.0),
    Vec2::new(-1.0, 0.55),
    Vec2::new(-1.0, -0.1),
    Vec2::new(-0.7, -0.75),
    Vec2::new(-0.3, -1.0),
    Vec2::new(0.3, -1.0),
    Vec2::new(0.7, -0.75),
];
/// Depósito: más ancho arriba, con una arista por el lomo.
const TANK: &[Vec2] = &[
    Vec2::new(0.8, -1.0),
    Vec2::new(1.0, -0.2),
    Vec2::new(0.85, 0.55),
    Vec2::new(0.4, 0.92),
    Vec2::new(0.0, 1.0),
    Vec2::new(-0.4, 0.92),
    Vec2::new(-0.85, 0.55),
    Vec2::new(-1.0, -0.2),
    Vec2::new(-0.8, -1.0),
];
const TAIL: &[Vec2] = &[
    Vec2::new(0.75, -1.0),
    Vec2::new(1.0, 0.0),
    Vec2::new(0.8, 0.8),
    Vec2::new(0.35, 1.0),
    Vec2::new(-0.35, 1.0),
    Vec2::new(-0.8, 0.8),
    Vec2::new(-1.0, 0.0),
    Vec2::new(-0.75, -1.0),
];
const SEAT: &[Vec2] = &[
    Vec2::new(1.0, -1.0),
    Vec2::new(1.0, 0.3),
    Vec2::new(0.7, 1.0),
    Vec2::new(-0.7, 1.0),
    Vec2::new(-1.0, 0.3),
    Vec2::new(-1.0, -1.0),
];
const FENDER: &[Vec2] = &[
    Vec2::new(1.0, -1.0),
    Vec2::new(0.9, 0.5),
    Vec2::new(0.4, 1.0),
    Vec2::new(-0.4, 1.0),
    Vec2::new(-0.9, 0.5),
    Vec2::new(-1.0, -1.0),
];
const BLOCK: &[Vec2] = &[
    Vec2::new(1.0, -0.6),
    Vec2::new(1.0, 0.7),
    Vec2::new(0.6, 1.0),
    Vec2::new(-0.6, 1.0),
    Vec2::new(-1.0, 0.7),
    Vec2::new(-1.0, -0.6),
    Vec2::new(-0.6, -1.0),
    Vec2::new(0.6, -1.0),
];

/// Semiancho del carenado donde van los dorsales.
const SIDE_HALF_WIDTH: f32 = 0.235;

/// Tomas de aire, rejillas y filetes de cada diseño, sacados de las láminas
/// y pintados sobre el carenado (ver [`Detail`]). Dejan libres los dorsales.
pub(super) fn details(design: Design) -> &'static [Detail] {
    match design {
        Design::Afilada => AFILADA_DETAILS,
        Design::Escudo => ESCUDO_DETAILS,
        Design::Alerones => ALERONES_DETAILS,
        Design::Rombo => ROMBO_DETAILS,
        Design::Model(_) => &[],
    }
}

/// Afilada: abertura detrás del faro, toma en flecha tras la rueda, hueco
/// trasero que deja ver el motor, rejilla en la quilla y dos filetes que
/// bajan desde el depósito.
const AFILADA_DETAILS: &[Detail] = &[
    Detail::Vent(&[
        Vec2::new(-0.7, 0.705),
        Vec2::new(-0.6, 0.725),
        Vec2::new(-0.58, 0.69),
        Vec2::new(-0.68, 0.67),
    ]),
    Detail::Vent(&[
        Vec2::new(-0.48, 0.66),
        Vec2::new(-0.39, 0.66),
        Vec2::new(-0.31, 0.46),
        Vec2::new(-0.39, 0.46),
    ]),
    Detail::Vent(&[
        Vec2::new(-0.02, 0.62),
        Vec2::new(0.1, 0.58),
        Vec2::new(0.12, 0.36),
        Vec2::new(0.02, 0.4),
    ]),
    Detail::Grille {
        outline: &[
            Vec2::new(-0.36, 0.31),
            Vec2::new(-0.17, 0.31),
            Vec2::new(-0.15, 0.24),
            Vec2::new(-0.34, 0.24),
        ],
        slats: 4,
    },
    Detail::Pinstripe {
        points: &[
            Vec2::new(-0.54, 0.8),
            Vec2::new(-0.2, 0.72),
            Vec2::new(0.1, 0.665),
        ],
        width: 0.012,
    },
    Detail::Pinstripe {
        points: &[
            Vec2::new(-0.52, 0.775),
            Vec2::new(-0.2, 0.695),
            Vec2::new(0.1, 0.64),
        ],
        width: 0.008,
    },
    Detail::Pinstripe {
        points: &[Vec2::new(0.3, 0.8), Vec2::new(0.84, 0.92)],
        width: 0.01,
    },
];

/// Escudo: ranura tras la rueda, toma bajo el faro, rejillas en la quilla
/// larga y filetes por el borde del depósito y del costado.
const ESCUDO_DETAILS: &[Detail] = &[
    Detail::Vent(&[
        Vec2::new(-0.66, 0.735),
        Vec2::new(-0.57, 0.745),
        Vec2::new(-0.56, 0.71),
        Vec2::new(-0.64, 0.7),
    ]),
    Detail::Vent(&[
        Vec2::new(-0.47, 0.67),
        Vec2::new(-0.37, 0.67),
        Vec2::new(-0.41, 0.55),
        Vec2::new(-0.51, 0.55),
    ]),
    Detail::Grille {
        outline: &[
            Vec2::new(-0.32, 0.34),
            Vec2::new(-0.14, 0.34),
            Vec2::new(-0.12, 0.25),
            Vec2::new(-0.3, 0.25),
        ],
        slats: 3,
    },
    Detail::Grille {
        outline: &[
            Vec2::new(0.02, 0.31),
            Vec2::new(0.2, 0.31),
            Vec2::new(0.22, 0.25),
            Vec2::new(0.04, 0.25),
        ],
        slats: 3,
    },
    Detail::Pinstripe {
        points: &[Vec2::new(-0.52, 0.795), Vec2::new(0.14, 0.755)],
        width: 0.01,
    },
    Detail::Pinstripe {
        points: &[
            Vec2::new(-0.64, 0.665),
            Vec2::new(-0.3, 0.645),
            Vec2::new(-0.05, 0.62),
        ],
        width: 0.01,
    },
];

/// Alerones: tres branquias en el costado, el radiador asomando abajo y un
/// filete por el borde del colín.
const ALERONES_DETAILS: &[Detail] = &[
    Detail::Vent(&[
        Vec2::new(-0.47, 0.72),
        Vec2::new(-0.43, 0.72),
        Vec2::new(-0.37, 0.58),
        Vec2::new(-0.41, 0.58),
    ]),
    Detail::Vent(&[
        Vec2::new(-0.4, 0.73),
        Vec2::new(-0.36, 0.73),
        Vec2::new(-0.3, 0.6),
        Vec2::new(-0.34, 0.6),
    ]),
    Detail::Vent(&[
        Vec2::new(-0.33, 0.74),
        Vec2::new(-0.29, 0.74),
        Vec2::new(-0.24, 0.63),
        Vec2::new(-0.28, 0.63),
    ]),
    Detail::Grille {
        outline: &[
            Vec2::new(-0.33, 0.42),
            Vec2::new(-0.21, 0.42),
            Vec2::new(-0.23, 0.3),
            Vec2::new(-0.35, 0.3),
        ],
        slats: 4,
    },
    Detail::Pinstripe {
        points: &[Vec2::new(0.4, 0.835), Vec2::new(0.77, 0.99)],
        width: 0.008,
    },
];

/// Rombo: tomas oscuras en el costado blanco y en la cúpula, y filetes en
/// el hombro del carenado y en el colín.
const ROMBO_DETAILS: &[Detail] = &[
    Detail::Vent(&[
        Vec2::new(-0.68, 0.72),
        Vec2::new(-0.6, 0.735),
        Vec2::new(-0.58, 0.7),
        Vec2::new(-0.66, 0.685),
    ]),
    Detail::Vent(&[
        Vec2::new(-0.5, 0.58),
        Vec2::new(-0.44, 0.58),
        Vec2::new(-0.37, 0.43),
        Vec2::new(-0.43, 0.43),
    ]),
    Detail::Vent(&[
        Vec2::new(-0.05, 0.6),
        Vec2::new(0.04, 0.58),
        Vec2::new(0.06, 0.46),
        Vec2::new(0.0, 0.48),
    ]),
    Detail::Pinstripe {
        points: &[Vec2::new(-0.56, 0.7), Vec2::new(-0.16, 0.69)],
        width: 0.014,
    },
    Detail::Pinstripe {
        points: &[Vec2::new(0.4, 0.84), Vec2::new(0.84, 0.935)],
        width: 0.012,
    },
    Detail::Pinstripe {
        points: &[Vec2::new(0.4, 0.82), Vec2::new(0.84, 0.915)],
        width: 0.006,
    },
];

/// Añade las piezas de la carrocería de `design`.
pub(super) fn build(parts: &mut Parts, design: Design) {
    common(parts, design == Design::Alerones);
    match design {
        Design::Afilada => afilada(parts),
        Design::Escudo => escudo(parts),
        Design::Alerones => alerones(parts),
        Design::Rombo => rombo(parts),
        Design::Model(_) => unreachable!("las motos importadas no se hacen por código"),
    }
}

/// Dorsales a los lados del carenado.
pub(super) fn plates(design: Design) -> Vec<Plate> {
    let (y, z) = match design {
        Design::Afilada | Design::Escudo | Design::Rombo => (0.55, -0.18),
        Design::Alerones => (0.54, -0.18),
        Design::Model(_) => return Vec::new(),
    };
    let x = SIDE_HALF_WIDTH + 0.003;
    vec![
        plate(Vec3::new(x, y, z), Vec3::X, 0.075),
        plate(Vec3::new(-x, y, z), Vec3::NEG_X, 0.075),
    ]
}

fn afilada(parts: &mut Parts) {
    use Mount::Chassis;

    parts.facets(
        Chassis,
        Paint::Bodywork,
        COWL,
        &[
            section(-0.87, 0.745, 0.05, 0.04),
            section(-0.79, 0.78, 0.13, 0.085),
            section(-0.68, 0.82, 0.2, 0.125),
            section(-0.555, 0.85, 0.225, 0.14),
        ],
    );
    screen(parts, 1.10);
    side_fairing(
        parts,
        &[
            section(-0.30, 0.50, 0.235, 0.30),
            section(-0.05, 0.46, 0.235, 0.27),
            section(0.16, 0.44, 0.17, 0.22).slanted(-0.7),
        ],
    );
    tank(parts, 0.875);
    seat(parts, 0.85);
    parts.facets(
        Chassis,
        Paint::Bodywork,
        TAIL,
        &[
            section(0.10, 0.75, 0.15, 0.06),
            section(0.42, 0.83, 0.15, 0.07),
            section(0.62, 0.90, 0.10, 0.055),
            section(0.84, 0.935, 0.03, 0.025),
        ],
    );
    tail_light(parts, Vec3::new(0.0, 0.93, 0.80));
    // Escape corto, de titanio quemado, junto a la rueda.
    header(parts, Vec3::new(0.15, 0.28, 0.42));
    silencer(
        parts,
        Vec3::new(0.17, 0.30, 0.44),
        Vec3::new(0.20, 0.58, 0.76),
        (0.065, 0.07),
    );
    headlights(parts, Vec3::new(0.085, 0.74, -0.79), 0.1, 0.6, 0.3);
    intake(
        parts,
        Vec3::new(0.0, 0.725, -0.868),
        Vec3::new(0.07, 0.045, 0.02),
    );
    mirrors(parts);
    plate_hanger(parts, Vec3::new(0.0, 0.87, 0.70));
    fender(parts, Paint::Bodywork);
}

fn rombo(parts: &mut Parts) {
    use Mount::Chassis;

    parts.facets(
        Chassis,
        Paint::Bodywork,
        COWL,
        &[
            section(-0.86, 0.74, 0.06, 0.05),
            section(-0.78, 0.775, 0.14, 0.09),
            section(-0.68, 0.815, 0.2, 0.125),
            section(-0.555, 0.845, 0.225, 0.14),
        ],
    );
    screen(parts, 1.10);
    side_fairing(
        parts,
        &[
            section(-0.30, 0.49, 0.235, 0.30),
            section(-0.05, 0.46, 0.235, 0.27),
            section(0.10, 0.44, 0.17, 0.22).slanted(-0.6),
        ],
    );
    // Quilla oscura bajo el carenado.
    parts.facets(
        Chassis,
        Paint::Plastic,
        BLOCK,
        &[
            section(-0.36, 0.22, 0.15, 0.06),
            section(0.20, 0.22, 0.15, 0.06),
            section(0.30, 0.25, 0.10, 0.04),
        ],
    );
    tank(parts, 0.86);
    seat(parts, 0.85);
    parts.facets(
        Chassis,
        Paint::Bodywork,
        TAIL,
        &[
            section(0.10, 0.75, 0.15, 0.06),
            section(0.44, 0.84, 0.15, 0.07),
            section(0.64, 0.91, 0.10, 0.055),
            section(0.84, 0.95, 0.03, 0.025),
        ],
    );
    tail_light(parts, Vec3::new(0.0, 0.945, 0.81));
    header(parts, Vec3::new(0.15, 0.27, 0.38));
    silencer(
        parts,
        Vec3::new(0.17, 0.30, 0.40),
        Vec3::new(0.22, 0.60, 0.82),
        (0.08, 0.09),
    );
    // Faro central en rombo y dos rasgados pequeños a los lados.
    parts.add(
        Chassis,
        Paint::Headlight,
        parts.cube.clone(),
        Transform::from_xyz(0.0, 0.74, -0.855)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4))
            .with_scale(Vec3::new(0.055, 0.055, 0.03)),
    );
    headlights(parts, Vec3::new(0.1, 0.775, -0.79), 0.06, 0.6, 0.3);
    for side in [-1.0, 1.0] {
        intake(
            parts,
            Vec3::new(0.045 * side, 0.74, -0.862),
            Vec3::new(0.03, 0.05, 0.015),
        );
    }
    mirrors(parts);
    plate_hanger(parts, Vec3::new(0.0, 0.88, 0.70));
    fender(parts, Paint::Plastic);
}

/// Soporte de la matrícula: baja en diagonal desde debajo del colín, por
/// detrás de la rueda, con los intermitentes a los lados.
fn plate_hanger(parts: &mut Parts, from: Vec3) {
    let to = from + Vec3::new(0.0, -0.24, 0.26);
    let length = from.distance(to);
    let hanger = super::between(from, to)
        * Transform::from_xyz(0.0, 0.0, length / 2.0).with_scale(Vec3::new(0.05, 0.015, length));
    parts.add(Mount::Chassis, Paint::Plastic, parts.cube.clone(), hanger);
    parts.block(
        Mount::Chassis,
        Paint::Plastic,
        to + Vec3::new(0.0, -0.03, 0.0),
        Vec3::new(0.16, 0.09, 0.012),
    );
    for side in [-1.0, 1.0] {
        parts.tube(
            Mount::Chassis,
            Paint::Plastic,
            from + Vec3::new(0.0, -0.08, 0.09),
            from + Vec3::new(0.11 * side, -0.08, 0.1),
            (0.008, 0.008),
        );
        parts.ball(
            Mount::Chassis,
            Paint::Headlight,
            from + Vec3::new(0.12 * side, -0.08, 0.1),
            Vec3::new(0.035, 0.02, 0.02),
        );
    }
}

/// Retrovisores en los costados de la cúpula.
fn mirrors(parts: &mut Parts) {
    for side in [-1.0, 1.0] {
        parts.tube(
            Mount::Chassis,
            Paint::Plastic,
            Vec3::new(0.19 * side, 0.92, -0.62),
            Vec3::new(0.25 * side, 0.96, -0.62),
            (0.01, 0.01),
        );
        parts.block(
            Mount::Chassis,
            Paint::Plastic,
            Vec3::new(0.28 * side, 0.97, -0.62),
            Vec3::new(0.08, 0.045, 0.03),
        );
    }
}

fn escudo(parts: &mut Parts) {
    use Mount::Chassis;

    // Frontal casi vertical, con los faros en la cara de delante.
    parts.facets(
        Chassis,
        Paint::Bodywork,
        COWL,
        &[
            section(-0.87, 0.76, 0.06, 0.07),
            section(-0.80, 0.79, 0.14, 0.10),
            section(-0.68, 0.83, 0.2, 0.13),
            section(-0.555, 0.855, 0.225, 0.14),
        ],
    );
    screen(parts, 1.12);
    side_fairing(
        parts,
        &[
            section(-0.30, 0.50, 0.235, 0.30),
            section(-0.05, 0.47, 0.235, 0.28),
            section(0.14, 0.47, 0.18, 0.22).slanted(-0.8),
        ],
    );
    // Quilla larga, que llega hasta debajo de las estriberas.
    parts.facets(
        Chassis,
        Paint::Bodywork,
        BLOCK,
        &[
            section(-0.30, 0.25, 0.16, 0.07),
            section(0.12, 0.25, 0.16, 0.07),
            section(0.38, 0.29, 0.09, 0.045),
        ],
    );
    tank(parts, 0.86);
    seat(parts, 0.85);
    // Colín muy levantado.
    parts.facets(
        Chassis,
        Paint::Bodywork,
        TAIL,
        &[
            section(0.10, 0.74, 0.15, 0.06),
            section(0.40, 0.81, 0.15, 0.07),
            section(0.62, 0.90, 0.10, 0.06),
            section(0.84, 0.97, 0.03, 0.03),
        ],
    );
    tail_light(parts, Vec3::new(0.0, 0.965, 0.81));
    // Escape largo que sube desde debajo del motor hasta la altura del colín.
    header(parts, Vec3::new(0.15, 0.24, 0.28));
    silencer(
        parts,
        Vec3::new(0.17, 0.25, 0.26),
        Vec3::new(0.21, 0.68, 0.80),
        (0.07, 0.085),
    );
    headlights(parts, Vec3::new(0.055, 0.75, -0.865), 0.07, 0.3, 0.35);
    intake(
        parts,
        Vec3::new(0.0, 0.725, -0.875),
        Vec3::new(0.09, 0.05, 0.015),
    );
    fender(parts, Paint::Bodywork);
}

fn alerones(parts: &mut Parts) {
    use Mount::Chassis;

    // Morro bajo y afilado, con alerones a los lados.
    parts.facets(
        Chassis,
        Paint::Bodywork,
        COWL,
        &[
            section(-0.87, 0.72, 0.05, 0.035),
            section(-0.79, 0.76, 0.13, 0.08),
            section(-0.68, 0.80, 0.2, 0.12),
            section(-0.555, 0.83, 0.22, 0.14),
        ],
    );
    parts.beam_pair(
        Chassis,
        Paint::Plastic,
        Vec3::new(0.24, 0.70, -0.74),
        Vec3::new(0.24, 0.715, -0.60),
        Vec2::new(0.08, 0.012),
    );
    screen(parts, 1.09);
    side_fairing(
        parts,
        &[
            section(-0.30, 0.48, 0.235, 0.29),
            section(-0.05, 0.45, 0.235, 0.27),
            section(0.12, 0.43, 0.17, 0.21).slanted(-0.8),
        ],
    );
    // Tapa del motor, octogonal y dorada, asomando por el carenado izquierdo.
    parts.faceted_tube(
        Chassis,
        Paint::Gold,
        8,
        Vec3::new(-0.195, 0.40, -0.02),
        Vec3::new(-0.225, 0.40, -0.02),
        (0.055, 0.055),
    );
    // Escape bajo el motor: una cuña plateada que sale hacia atrás.
    parts.facets(
        Chassis,
        Paint::Metal,
        BLOCK,
        &[
            section(-0.05, 0.21, 0.14, 0.06),
            section(0.20, 0.23, 0.13, 0.06),
            section(0.38, 0.30, 0.07, 0.04),
        ],
    );
    tank(parts, 0.87);
    seat(parts, 0.86);
    // Bajo el asiento, el soporte oscuro; encima, el colín como una cuchilla.
    parts.facets(
        Chassis,
        Paint::Plastic,
        TAIL,
        &[
            section(0.10, 0.74, 0.12, 0.05),
            section(0.44, 0.80, 0.11, 0.04),
        ],
    );
    parts.facets(
        Chassis,
        Paint::Bodywork,
        TAIL,
        &[
            section(0.40, 0.865, 0.12, 0.045),
            section(0.48, 0.875, 0.13, 0.06),
            section(0.64, 0.95, 0.09, 0.05),
            section(0.77, 1.01, 0.03, 0.025),
        ],
    );
    tail_light(parts, Vec3::new(0.0, 0.99, 0.74));
    headlights(parts, Vec3::new(0.07, 0.755, -0.80), 0.09, 0.55, 0.4);
    // La boca, bajo el morro.
    intake(
        parts,
        Vec3::new(0.0, 0.695, -0.86),
        Vec3::new(0.07, 0.025, 0.03),
    );
    fender(parts, Paint::Plastic);
}

/// Chasis, motor, basculante, amortiguador, estriberas y cuadro de mandos.
fn common(parts: &mut Parts, single_sided: bool) {
    use Mount::Chassis;

    // Vigas del chasis: de la pipa de la dirección al eje del basculante,
    // por fuera del carenado, como la franja oscura entre el depósito y los
    // costados de las láminas.
    parts.beam_pair(
        Chassis,
        Paint::Frame,
        Vec3::new(0.225, 0.88, -0.46),
        Vec3::new(0.19, 0.66, 0.20),
        Vec2::new(0.03, 0.09),
    );
    parts.beam_pair(
        Chassis,
        Paint::Frame,
        Vec3::new(0.2, 0.64, 0.18),
        Vec3::new(0.16, 0.36, 0.26),
        Vec2::new(0.04, 0.12),
    );
    // Motor, con la tapa del embrague a la derecha.
    parts.facets(
        Chassis,
        Paint::Frame,
        BLOCK,
        &[
            section(-0.30, 0.43, 0.14, 0.15),
            section(0.0, 0.42, 0.155, 0.16),
            section(0.20, 0.40, 0.13, 0.12),
        ],
    );
    parts.faceted_tube(
        Chassis,
        Paint::Metal,
        8,
        Vec3::new(0.15, 0.38, 0.08),
        Vec3::new(0.17, 0.38, 0.08),
        (0.065, 0.065),
    );
    // Basculante (por los dos lados o solo por la izquierda) y amortiguador.
    let pivot = Vec3::new(0.12, 0.42, 0.24);
    let axle = Vec3::new(0.11, WHEEL_RADIUS, REAR_AXLE.z);
    if single_sided {
        let mirror = Vec3::new(-1.0, 1.0, 1.0);
        let (from, to) = (pivot * mirror, axle * mirror);
        let length = from.distance(to);
        let arm = super::between(from, to)
            * Transform::from_xyz(0.0, 0.0, length / 2.0).with_scale(Vec3::new(0.05, 0.1, length));
        parts.add(Chassis, Paint::Frame, parts.cube.clone(), arm);
    } else {
        parts.beam_pair(Chassis, Paint::Frame, pivot, axle, Vec2::new(0.04, 0.085));
    }
    parts.tube(
        Chassis,
        Paint::Gold,
        Vec3::new(0.0, 0.42, 0.38),
        Vec3::new(0.0, 0.68, 0.24),
        (0.03, 0.03),
    );
    // Estriberas retrasadas, con sus placas.
    parts.tube_pair(
        Chassis,
        Paint::Metal,
        Vec3::new(0.12, 0.41, 0.32),
        Vec3::new(0.22, 0.41, 0.32),
        (0.015, 0.015),
    );
    parts.beam_pair(
        Chassis,
        Paint::Metal,
        Vec3::new(0.115, 0.53, 0.24),
        Vec3::new(0.115, 0.37, 0.38),
        Vec2::new(0.012, 0.05),
    );
    // Radiador oscuro tras la rueda, en el hueco de delante del carenado,
    // inclinado como su borde: lo que se ve de frente entre las barras.
    parts.add(
        Chassis,
        Paint::Plastic,
        parts.cube.clone(),
        Transform::from_xyz(0.0, 0.5, -0.385)
            .with_rotation(Quat::from_rotation_x(-0.54))
            .with_scale(Vec3::new(0.3, 0.34, 0.015)),
    );
    // Cuadro de mandos, bajo el borde de la cúpula.
    parts.block(
        Chassis,
        Paint::Plastic,
        Vec3::new(0.0, 0.93, -0.49),
        Vec3::new(0.17, 0.05, 0.07),
    );
}

/// Cúpula ahumada que sube desde el morro hasta `top` metros.
fn screen(parts: &mut Parts, top: f32) {
    let rise = top - 1.10;
    parts.facets(
        Mount::Chassis,
        Paint::Glass,
        SCREEN,
        &[
            section(-0.77, 0.905 + rise * 0.2, 0.09, 0.03),
            section(-0.68, 0.95 + rise * 0.5, 0.16, 0.06),
            section(-0.60, 1.0 + rise * 0.8, 0.16, 0.07),
            section(-0.52, 1.05 + rise, 0.10, 0.05),
        ],
    );
}

/// Carenado lateral: por delante sigue el borde de la rueda (que puede girar
/// y subir con la suspensión sin tocarlo) y luego `sections`.
fn side_fairing(parts: &mut Parts, sections: &[Section]) {
    let front = section(-0.37, sections[0].y, 0.19, sections[0].half_height - 0.01);
    let all: Vec<Section> = std::iter::once(front.slanted(-0.6))
        .chain(sections.iter().copied())
        .collect();
    parts.facets(Mount::Chassis, Paint::Bodywork, FAIRING, &all);
}

/// Depósito alto con el lomo a `ridge` metros (más 0,095 de semialto).
fn tank(parts: &mut Parts, ridge: f32) {
    parts.facets(
        Mount::Chassis,
        Paint::Bodywork,
        TANK,
        &[
            section(-0.52, ridge - 0.035, 0.12, 0.05),
            section(-0.44, ridge - 0.01, 0.18, 0.09),
            section(-0.24, ridge, 0.20, 0.095),
            section(-0.04, ridge - 0.025, 0.17, 0.085),
            section(0.14, ridge - 0.065, 0.12, 0.05),
        ],
    );
}

/// Asiento bajo, que sube hacia atrás hasta `rear` metros.
fn seat(parts: &mut Parts, rear: f32) {
    parts.facets(
        Mount::Chassis,
        Paint::Plastic,
        SEAT,
        &[
            section(0.12, 0.80, 0.11, 0.025),
            section(0.28, 0.815, 0.135, 0.03),
            section(0.44, rear, 0.12, 0.03),
        ],
    );
}

fn tail_light(parts: &mut Parts, at: Vec3) {
    parts.ball(
        Mount::Chassis,
        Paint::Taillight,
        at,
        Vec3::new(0.06, 0.03, 0.05),
    );
}

/// Colector desde debajo del motor hasta `to`, a la derecha.
fn header(parts: &mut Parts, to: Vec3) {
    parts.tube(
        Mount::Chassis,
        Paint::Metal,
        Vec3::new(0.06, 0.22, -0.20),
        to,
        (0.03, 0.035),
    );
}

/// Silencioso hexagonal de titanio, con la boca oscura.
fn silencer(parts: &mut Parts, from: Vec3, to: Vec3, radius: (f32, f32)) {
    parts.faceted_tube(Mount::Chassis, Paint::Titanium, 6, from, to, radius);
    let along = (to - from).normalize();
    parts.faceted_tube(
        Mount::Chassis,
        Paint::Plastic,
        6,
        to - along * 0.01,
        to + along * 0.005,
        (radius.1 * 0.7, radius.1 * 0.7),
    );
}

/// Faros finos y rasgados a los dos lados del morro, centrados en `right`
/// (el derecho), de `length` metros, girados hacia atrás `sweep` y con la
/// punta de fuera levantada `tilt` (rad): la mirada de las láminas.
fn headlights(parts: &mut Parts, right: Vec3, length: f32, sweep: f32, tilt: f32) {
    for side in [-1.0, 1.0] {
        parts.add(
            Mount::Chassis,
            Paint::Headlight,
            parts.cube.clone(),
            Transform::from_translation(right * Vec3::new(side, 1.0, 1.0))
                .with_rotation(Quat::from_euler(
                    EulerRot::YXZ,
                    -sweep * side,
                    0.0,
                    tilt * side,
                ))
                .with_scale(Vec3::new(length, 0.02, 0.03)),
        );
    }
}

/// Toma de aire en el morro: una pieza oscura que asoma de la superficie.
fn intake(parts: &mut Parts, center: Vec3, size: Vec3) {
    parts.block(Mount::Chassis, Paint::Plastic, center, size);
}

/// Guardabarros delantero, que gira con la dirección.
fn fender(parts: &mut Parts, paint: Paint) {
    parts.facets(
        Mount::Steering,
        paint,
        FENDER,
        &[
            section(-0.88, 0.58, 0.065, 0.015),
            section(-0.80, 0.645, 0.08, 0.02),
            section(-0.66, 0.66, 0.08, 0.02),
            section(-0.55, 0.62, 0.065, 0.015),
        ],
    );
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    pub fn designs_index_in_order_on(roster: &'static crate::bike::Roster) {
        for (index, design) in every(roster).into_iter().enumerate() {
            assert_eq!(design.index(), index);
        }
    }

    #[test]
    fn designs_index_in_order() {
        designs_index_in_order_on(&crate::testing::DEMO_ROSTER);
    }
}
