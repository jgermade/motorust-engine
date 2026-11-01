//! Modelos 3D de las motos y sus pilotos, y su animación: ruedas, dirección,
//! suspensión, luz de freno y postura del piloto.
//!
//! La geometría se define una sola vez como un plano de montaje
//! ([`BikeBlueprint`]): una lista de piezas, cada una con su malla, su pintura
//! y el punto de la moto al que va sujeta. Cada moto reutiliza esas mallas y
//! solo cambia la decoración ([`Livery`]), así los rivales salen casi gratis.
//!
//! Hay dos clases de motos: las hechas por código ([`designs`]), con su piloto
//! también hecho por código ([`rider`]), y las importadas de Meshy
//! ([`imported`]), cada una con su piloto de Meshy ([`pilot`]). Cada una tiene sus
//! medidas ([`Geometry`]): dónde están los ejes, la dirección, los puños y las
//! estriberas.
//!
//! Todas las medidas están en metros, en el espacio de la moto: X a la
//! derecha, Y hacia arriba, -Z hacia delante y el origen en el suelo, entre
//! las dos ruedas.

pub mod designs;
pub mod imported;
mod livery;
pub mod pilot;
mod rider;
pub mod tyre;

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::{mesh::skinning::SkinnedMeshInverseBindposes, prelude::*};

use super::crash::ThrownRider;
use super::{BikeInput, BikeState, BikeTuning, CrashState, LeanPivot, PreviousBikeState};
use crate::geometry::{smoothstep, to_world};
use crate::shapes::{Section, between, faceted, faceted_tube, flat_shaded, loft, tube};
use crate::track::SURFACE_Y;
pub use designs::Design;
pub use livery::{COVER, DIAGONAL, FLECHA, Livery, ROSSO, TRICOLOR, VERDE};
use rider::HIPS;

const WHEEL_RADIUS: f32 = 0.31;
const FRONT_AXLE: Vec3 = Vec3::new(0.0, WHEEL_RADIUS, -0.70);
const REAR_AXLE: Vec3 = Vec3::new(0.0, WHEEL_RADIUS, 0.68);
/// Pivote de la dirección, arriba de la horquilla.
const STEERING_HEAD: Vec3 = Vec3::new(0.0, 0.84, -0.50);
/// How far a bike reaches ahead of where it stands, to the front of its
/// front tyre (m). Every bike is built round the same wheelbase (a test
/// holds them to it); the starting grid's boxes are drawn round it.
pub const NOSE: f32 = -FRONT_AXLE.z + WHEEL_RADIUS;
/// How far a bike reaches behind where it stands, to the back of its rear
/// tyre (m).
pub const TAIL: f32 = REAR_AXLE.z + WHEEL_RADIUS;

/// Medidas de una moto que usan las animaciones: dónde están los ejes y la
/// pipa de la dirección, cuánto miden las ruedas y dónde apoya el piloto.
#[derive(Component, Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    pub front_axle: Vec3,
    pub rear_axle: Vec3,
    pub front_radius: f32,
    pub rear_radius: f32,
    pub steering_head: Vec3,
    /// Puño derecho con la dirección recta (el izquierdo es su reflejo).
    pub grip: Vec3,
    /// Tobillo derecho, sobre la estribera.
    pub peg: Vec3,
    /// Cadera del piloto sentado.
    pub seat: Vec3,
}

/// Las de las motos hechas por código.
const STANDARD: Geometry = Geometry {
    front_axle: FRONT_AXLE,
    rear_axle: REAR_AXLE,
    front_radius: WHEEL_RADIUS,
    rear_radius: WHEEL_RADIUS,
    steering_head: STEERING_HEAD,
    grip: rider::GRIP,
    peg: rider::ANKLE,
    seat: HIPS,
};
/// Giro máximo visible del manillar: parado .. a toda velocidad (rad).
const MAX_STEER_ANGLE: (f32, f32) = (0.45, 0.04);
/// El piloto mira al punto de su trayectoria por el que pasará dentro de este
/// tiempo (s), hasta un giro de cabeza máximo (rad).
const LOOK_AHEAD: f32 = 1.6;
const MAX_LOOK: f32 = 0.6;
/// Qué parte de la inclinación de la moto compensa el piloto con la cabeza:
/// en las curvas la lleva más derecha que la moto, con la vista hacia el
/// horizonte.
const HEAD_LEVEL: f32 = 0.4;

/// Cuánto se agacha el piloto sobre el depósito, de 0 a 1: del todo en las
/// rectas deprisa, y menos cuanto más tumbada va la moto (en las curvas va
/// descolgado) o más despacio.
fn tuck_amount(state: &BikeState, tuning: &BikeTuning) -> f32 {
    let straight = 1.0 - (state.lean / tuning.max_lean).abs().min(1.0);
    smoothstep(15.0, 45.0, state.speed.abs()) * straight * straight
}

/// Hacia dónde gira la cabeza el piloto (rad, positivo a la izquierda): hacia
/// donde se dirige la moto, así mira dentro de la curva mientras la toma.
fn look_angle(state: &BikeState, tuning: &BikeTuning) -> f32 {
    // Sobre un arco, el punto que se alcanza tras girar un ángulo se ve con
    // la mitad de ese ángulo.
    let turn = state.steer * tuning.max_yaw_rate(state.speed) * LOOK_AHEAD;
    (0.5 * turn).clamp(-MAX_LOOK, MAX_LOOK)
}

pub struct BikeModelPlugin;

impl Plugin for BikeModelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BikeBlueprint>().add_systems(
            Update,
            (
                spin_wheels,
                brake_light,
                (
                    (steer_front, rider::hang_off, rider::sit_up),
                    (
                        move_chassis,
                        rider::reach_grips,
                        rider::reach_pegs,
                        rider::look_ahead,
                        pilot::pose_pilots,
                    ),
                    // After the chassis: it undoes the chassis transforms.
                    update_rider_mount,
                )
                    .chain(),
            ),
        );
    }
}

/// Dónde va sujeta cada pieza. Las piezas de las ruedas giran con ellas, las de
/// la dirección giran con el manillar, etc.
#[derive(Clone, Copy, Debug)]
enum Mount {
    Chassis,
    Steering,
    FrontWheel,
    RearWheel,
    /// Tronco y brazos: se incorporan al frenar.
    UpperBody,
    /// Casco: va con el tronco y gira hacia donde mira el piloto.
    Head,
}

impl Mount {
    /// Origen del punto de montaje en el espacio de una moto.
    fn origin(self, geometry: &Geometry) -> Vec3 {
        match self {
            Mount::Chassis => Vec3::ZERO,
            Mount::Steering => geometry.steering_head,
            Mount::FrontWheel => geometry.front_axle,
            Mount::RearWheel => geometry.rear_axle,
            Mount::UpperBody => HIPS,
            Mount::Head => rider::HELMET,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Paint {
    /// Carrocería: lleva la textura con los dibujos de la decoración.
    Bodywork,
    Plate,
    Number,
    Plastic,
    Rubber,
    /// Chasis, motor y basculante: gris oscuro.
    Frame,
    /// Llantas, del color de la decoración.
    Rim,
    Metal,
    /// Discos de freno: acero más oscuro que el metal pulido.
    Disc,
    /// Escapes de titanio.
    Titanium,
    Gold,
    SuitTrim,
    Helmet,
    Glass,
    Headlight,
    Taillight,
}

impl Paint {
    const ALL: [Paint; 16] = [
        Paint::Bodywork,
        Paint::Plate,
        Paint::Number,
        Paint::Plastic,
        Paint::Rubber,
        Paint::Frame,
        Paint::Rim,
        Paint::Metal,
        Paint::Disc,
        Paint::Titanium,
        Paint::Gold,
        Paint::SuitTrim,
        Paint::Helmet,
        Paint::Glass,
        Paint::Headlight,
        Paint::Taillight,
    ];

    fn material(self, livery: &Livery, textures: &LiveryTextures) -> StandardMaterial {
        let painted = |color: Color, roughness: f32| StandardMaterial {
            base_color: color,
            perceptual_roughness: roughness,
            ..default()
        };
        let glowing = |color: Color, glow: LinearRgba| StandardMaterial {
            base_color: color,
            emissive: glow,
            ..default()
        };
        let textured = |texture: &Handle<Image>, roughness: f32| StandardMaterial {
            base_color_texture: Some(texture.clone()),
            perceptual_roughness: roughness,
            ..default()
        };
        match self {
            Paint::Bodywork => textured(&textures.bodywork, 0.3),
            Paint::Plate => painted(livery.plate, 0.4),
            Paint::Number => painted(livery.number_ink, 0.6),
            Paint::Plastic => painted(Color::srgb(0.06, 0.06, 0.07), 0.6),
            Paint::Rubber => painted(Color::srgb(0.035, 0.035, 0.035), 0.95),
            Paint::Frame => painted(Color::srgb(0.13, 0.13, 0.14), 0.7),
            Paint::Rim => painted(livery.rims, 0.4),
            // Sin mapas de entorno, un metal de verdad se vería negro: mejor un
            // gris claro brillante, como en los juegos de la época.
            Paint::Metal => painted(Color::srgb(0.72, 0.73, 0.76), 0.25),
            Paint::Gold => painted(Color::srgb(0.88, 0.66, 0.18), 0.35),
            Paint::Titanium => painted(Color::srgb(0.5, 0.46, 0.42), 0.35),
            Paint::Disc => painted(Color::srgb(0.36, 0.36, 0.38), 0.3),
            Paint::SuitTrim => painted(livery.suit_trim, 0.5),
            Paint::Helmet => textured(&textures.helmet, 0.2),
            Paint::Glass => painted(Color::srgb(0.07, 0.09, 0.13), 0.1),
            Paint::Headlight => {
                glowing(Color::srgb(1.0, 0.97, 0.85), LinearRgba::rgb(3.0, 2.8, 2.2))
            }
            Paint::Taillight => {
                glowing(Color::srgb(0.9, 0.05, 0.05), LinearRgba::rgb(3.0, 0.1, 0.1))
            }
        }
    }
}

/// Texturas pintadas para una decoración.
struct LiveryTextures {
    bodywork: Handle<Image>,
    helmet: Handle<Image>,
}

struct Part {
    mount: Mount,
    paint: Paint,
    /// Material propio, en vez del de la pintura (los modelos importados).
    material: Option<Handle<StandardMaterial>>,
    mesh: Handle<Mesh>,
    /// Colocación respecto a su punto de montaje.
    transform: Transform,
}

/// Placa para el dorsal: un plano con su centro, su "derecha" vista de frente,
/// su "arriba" y su normal hacia fuera.
struct Plate {
    frame: Transform,
    /// Alto de las cifras (m).
    digit_height: f32,
}

/// Carrocería de un diseño: sus piezas, dónde van los dorsales y sus medidas.
struct Body {
    parts: Vec<Part>,
    plates: Vec<Plate>,
    geometry: Geometry,
    /// Si lleva las ruedas, la horquilla y el piloto hechos por código; si
    /// no, trae los suyos y su piloto de Meshy.
    procedural: bool,
    /// Si la horquilla y el manillar giran con la dirección.
    steers: bool,
    pilot: Option<pilot::PilotBlueprint>,
}

/// Mallas de la moto y el piloto, compartidas por todas las motos.
#[derive(Resource)]
pub struct BikeBlueprint {
    /// Ruedas y horquilla de las motos hechas por código.
    running_gear: Vec<Part>,
    /// Piezas rígidas del piloto hecho por código: casco, joroba y botas.
    rider_parts: Vec<Part>,
    /// Carrocerías, en el orden de [`Design::all`] ([`Design::index`]).
    bodies: Vec<Body>,
    /// Cubo de 1 m y esfera de 1 m de diámetro, que se escalan a cada pieza.
    cube: Handle<Mesh>,
    ball: Handle<Mesh>,
    /// Pose de reposo del esqueleto del piloto.
    rider_bindposes: Handle<SkinnedMeshInverseBindposes>,
}

impl FromWorld for BikeBlueprint {
    fn from_world(world: &mut World) -> Self {
        let rider_bindposes = world
            .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
            .add(rider::inverse_bindposes());
        // De cada moto importada: sus materiales y su piloto.
        let exhaust = imported::exhaust_material(&mut world.resource_mut::<Assets<Image>>());
        let exhaust = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(exhaust);
        // The slicks of each compound, shared by every bike that wears them.
        let tyres: Vec<Handle<StandardMaterial>> = tyre::Compound::ALL
            .into_iter()
            .map(|compound| {
                let material = tyre::material(compound, &mut world.resource_mut::<Assets<Image>>());
                world
                    .resource_mut::<Assets<StandardMaterial>>()
                    .add(material)
            })
            .collect();
        // Every design: the roster read first, which names the models.
        world.init_resource::<crate::bike::Roster>();
        let designs = Design::all();
        let mut imports: Vec<_> = designs
            .iter()
            .copied()
            .map(|design| {
                design.model().map(|model| {
                    let body =
                        imported::material(model, &mut world.resource_mut::<Assets<Image>>());
                    let body = world.resource_mut::<Assets<StandardMaterial>>().add(body);
                    let compound = tyre::Compound::of(design);
                    let materials = imported::Materials {
                        body,
                        exhaust: exhaust.clone(),
                        tyres: tyres[tyre::Compound::ALL
                            .iter()
                            .position(|&c| c == compound)
                            .expect("every compound has its material")]
                        .clone(),
                    };
                    (model, materials, pilot::PilotBlueprint::new(world, model))
                })
            })
            .collect();
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let mut parts = Parts {
            cube: meshes.add(Cuboid::from_length(1.0)),
            ball: meshes.add(Sphere::new(0.5).mesh().uv(20, 14)),
            meshes: &mut meshes,
            geometry: STANDARD,
            list: Vec::new(),
        };
        build_wheel(&mut parts, Mount::FrontWheel, FRONT_AXLE, 0.13, true);
        build_wheel(&mut parts, Mount::RearWheel, REAR_AXLE, 0.17, false);
        build_steering(&mut parts);
        let running_gear = std::mem::take(&mut parts.list);
        rider::build_rigid_parts(&mut parts);
        let rider_parts = std::mem::take(&mut parts.list);
        let bodies = designs
            .into_iter()
            .zip(imports.iter_mut())
            .map(|(design, import)| {
                let (pilot, steers) = match import.take() {
                    Some((model, materials, pilot)) => {
                        parts.geometry = model.geometry;
                        imported::build(&mut parts, model, &materials);
                        (Some(pilot), model.steers)
                    }
                    None => {
                        parts.geometry = STANDARD;
                        designs::build(&mut parts, design);
                        (None, true)
                    }
                };
                Body {
                    parts: std::mem::take(&mut parts.list),
                    plates: designs::plates(design),
                    geometry: parts.geometry,
                    procedural: pilot.is_none(),
                    steers,
                    pilot,
                }
            })
            .collect();
        let (cube, ball) = (parts.cube.clone(), parts.ball.clone());
        Self {
            running_gear,
            rider_parts,
            bodies,
            cube,
            ball,
            rider_bindposes,
        }
    }
}

/// Ayudante para ir añadiendo piezas con coordenadas en el espacio de la moto.
struct Parts<'a> {
    meshes: &'a mut Assets<Mesh>,
    /// Cubo de 1 m y esfera de 1 m de diámetro, que se escalan a cada pieza.
    cube: Handle<Mesh>,
    ball: Handle<Mesh>,
    /// Medidas de la moto que se está montando: dónde están sus pivotes.
    geometry: Geometry,
    list: Vec<Part>,
}

impl Parts<'_> {
    fn add(&mut self, mount: Mount, paint: Paint, mesh: Handle<Mesh>, mut transform: Transform) {
        transform.translation -= mount.origin(&self.geometry);
        self.list.push(Part {
            mount,
            paint,
            material: None,
            mesh,
            transform,
        });
    }

    /// Pieza de un modelo importado, con su propio material.
    fn add_textured(
        &mut self,
        mount: Mount,
        mesh: Handle<Mesh>,
        material: Handle<StandardMaterial>,
    ) {
        self.add(mount, Paint::Plastic, mesh, Transform::IDENTITY);
        if let Some(part) = self.list.last_mut() {
            part.material = Some(material);
        }
    }

    fn loft(&mut self, mount: Mount, paint: Paint, sections: &[Section]) {
        let mut mesh = loft(sections);
        if paint == Paint::Bodywork {
            livery::project_side_uvs(&mut mesh);
        }
        let mesh = self.meshes.add(mesh);
        self.add(mount, paint, mesh, Transform::IDENTITY);
    }

    /// Pieza facetada con el contorno `outline` (ver [`faceted`]).
    fn facets(&mut self, mount: Mount, paint: Paint, outline: &[Vec2], sections: &[Section]) {
        let mut mesh = faceted(outline, sections);
        if paint == Paint::Bodywork {
            livery::project_side_uvs(&mut mesh);
        }
        let mesh = self.meshes.add(mesh);
        self.add(mount, paint, mesh, Transform::IDENTITY);
    }

    /// Prisma de `sides` caras de `from` a `to`.
    fn faceted_tube(
        &mut self,
        mount: Mount,
        paint: Paint,
        sides: usize,
        from: Vec3,
        to: Vec3,
        radius: (f32, f32),
    ) {
        let mesh = faceted_tube(sides, radius.0, radius.1, from.distance(to));
        let mesh = self.meshes.add(mesh);
        self.add(mount, paint, mesh, between(from, to));
    }

    /// Viga de sección rectangular (`size`: grueso en X y alto) de `from` a
    /// `to`, y su reflejo en el otro lado.
    fn beam_pair(&mut self, mount: Mount, paint: Paint, from: Vec3, to: Vec3, size: Vec2) {
        let mirror = Vec3::new(-1.0, 1.0, 1.0);
        for (from, to) in [(from, to), (from * mirror, to * mirror)] {
            let length = from.distance(to);
            let transform = between(from, to)
                * Transform::from_xyz(0.0, 0.0, length / 2.0).with_scale(size.extend(length));
            self.add(mount, paint, self.cube.clone(), transform);
        }
    }

    /// Loft desplazado `x` metros a cada lado (para piezas que van por pares).
    fn loft_pair(&mut self, mount: Mount, paint: Paint, x: f32, sections: &[Section]) {
        let mesh = self.meshes.add(loft(sections));
        for side in [-x, x] {
            self.add(
                mount,
                paint,
                mesh.clone(),
                Transform::from_xyz(side, 0.0, 0.0),
            );
        }
    }

    /// Tubo de `from` a `to` y su reflejo en el otro lado (X → -X).
    fn tube_pair(&mut self, mount: Mount, paint: Paint, from: Vec3, to: Vec3, radius: (f32, f32)) {
        let mesh = self.meshes.add(tube(radius.0, radius.1, from.distance(to)));
        let mirror = Vec3::new(-1.0, 1.0, 1.0);
        self.add(mount, paint, mesh.clone(), between(from, to));
        self.add(mount, paint, mesh, between(from * mirror, to * mirror));
    }

    fn tube(&mut self, mount: Mount, paint: Paint, from: Vec3, to: Vec3, radius: (f32, f32)) {
        let mesh = self.meshes.add(tube(radius.0, radius.1, from.distance(to)));
        self.add(mount, paint, mesh, between(from, to));
    }

    fn block(&mut self, mount: Mount, paint: Paint, center: Vec3, size: Vec3) {
        let cube = self.cube.clone();
        self.add(
            mount,
            paint,
            cube,
            Transform::from_translation(center).with_scale(size),
        );
    }

    /// Elipsoide de diámetros `size`.
    fn ball(&mut self, mount: Mount, paint: Paint, center: Vec3, size: Vec3) {
        let ball = self.ball.clone();
        self.add(
            mount,
            paint,
            ball,
            Transform::from_translation(center).with_scale(size),
        );
    }

    /// Disco (cilindro corto) con el eje en X, como una rueda.
    fn disc(&mut self, mount: Mount, paint: Paint, center: Vec3, radius: f32, width: f32) {
        let mesh = self
            .meshes
            .add(Cylinder::new(radius, width).mesh().resolution(32).build());
        self.add(mount, paint, mesh, on_axle(center));
    }
}

/// Colocación de una pieza de revolución (eje en Y) con el eje en X.
fn on_axle(center: Vec3) -> Transform {
    Transform::from_translation(center).with_rotation(Quat::from_rotation_z(FRAC_PI_2))
}

fn build_wheel(parts: &mut Parts, mount: Mount, axle: Vec3, width: f32, front: bool) {
    // Neumático facetado, como en las láminas, y una llanta de radios finos
    // por la que se ven los discos de freno: lo que deja ver que gira.
    let tube_radius = width / 2.0;
    let tire = Torus {
        minor_radius: tube_radius,
        major_radius: WHEEL_RADIUS - tube_radius,
    };
    let tire = parts.meshes.add(flat_shaded(
        tire.mesh().minor_resolution(8).major_resolution(28).build(),
    ));
    parts.add(mount, Paint::Rubber, tire, on_axle(axle));
    let rim = WHEEL_RADIUS - width - 0.005;
    let ring = Torus {
        minor_radius: 0.016,
        major_radius: rim,
    };
    let ring = parts
        .meshes
        .add(ring.mesh().minor_resolution(6).major_resolution(28).build());
    parts.add(mount, Paint::Rim, ring, on_axle(axle));
    parts.disc(mount, Paint::Rim, axle, 0.045, width * 0.8);
    const SPOKES: usize = 5;
    for i in 0..SPOKES {
        let rotation = Quat::from_rotation_x(i as f32 * TAU / SPOKES as f32);
        let length = rim - 0.04;
        parts.add(
            mount,
            Paint::Rim,
            parts.cube.clone(),
            Transform::from_translation(axle + rotation * Vec3::Y * (length / 2.0 + 0.04))
                .with_rotation(rotation)
                .with_scale(Vec3::new(width * 0.35, length, 0.022)),
        );
    }
    // Discos de freno: dos delante y uno detrás, con la corona al otro lado.
    let discs: &[(f32, f32)] = if front {
        &[(-0.075, 0.15), (0.075, 0.15)]
    } else {
        &[(0.07, 0.11)]
    };
    for &(x, radius) in discs {
        parts.disc(mount, Paint::Disc, axle + Vec3::X * x, radius, 0.008);
    }
    if !front {
        parts.disc(mount, Paint::Frame, axle - Vec3::X * 0.07, 0.1, 0.012);
    }
}

fn build_steering(parts: &mut Parts) {
    use Mount::Steering;

    let rake = (STEERING_HEAD - FRONT_AXLE).normalize();
    parts.tube_pair(
        Steering,
        Paint::Gold,
        FRONT_AXLE + Vec3::X * 0.075,
        STEERING_HEAD + Vec3::X * 0.075 + rake * 0.06,
        (0.03, 0.026),
    );
    // Botellas, abajo, y pinzas de freno doradas sobre los discos.
    parts.tube_pair(
        Steering,
        Paint::Frame,
        FRONT_AXLE + Vec3::X * 0.075,
        FRONT_AXLE + Vec3::X * 0.075 + rake * 0.2,
        (0.036, 0.034),
    );
    for side in [-1.0, 1.0] {
        parts.block(
            Steering,
            Paint::Gold,
            FRONT_AXLE + Vec3::new(0.1 * side, 0.08, 0.1),
            Vec3::new(0.025, 0.1, 0.06),
        );
    }
    parts.block(
        Steering,
        Paint::Metal,
        STEERING_HEAD,
        Vec3::new(0.2, 0.03, 0.07),
    );
    // Semimanillares, bajos, justo detrás de la cúpula.
    parts.tube_pair(
        Steering,
        Paint::Plastic,
        Vec3::new(0.075, 0.835, -0.505),
        Vec3::new(0.235, 0.83, -0.53),
        (0.017, 0.017),
    );
}

/// Placa para el dorsal, centrada en `center` y mirando hacia `normal`, con
/// cifras de `digit_height` metros.
fn plate(center: Vec3, normal: Vec3, digit_height: f32) -> Plate {
    let right = Vec3::Y.cross(normal).normalize();
    let up = normal.cross(right);
    Plate {
        frame: Transform::from_translation(center)
            .with_rotation(Quat::from_mat3(&Mat3::from_cols(right, up, normal))),
        digit_height,
    }
}

/// Segmentos (como en un display de 7 segmentos) que forman un número, en el
/// plano de la placa: centro en (x, y) y tamaño.
fn number_segments(number: u8, digit_height: f32) -> Vec<(Vec2, Vec2)> {
    // Segmentos encendidos para cada cifra, en el orden a b c d e f g
    // (arriba, arriba-dcha, abajo-dcha, abajo, abajo-izda, arriba-izda, centro).
    const DIGITS: [[bool; 7]; 10] = {
        const X: bool = true;
        const O: bool = false;
        [
            [X, X, X, X, X, X, O],
            [O, X, X, O, O, O, O],
            [X, X, O, X, X, O, X],
            [X, X, X, X, O, O, X],
            [O, X, X, O, O, X, X],
            [X, O, X, X, O, X, X],
            [X, O, X, X, X, X, X],
            [X, X, X, O, O, O, O],
            [X, X, X, X, X, X, X],
            [X, X, X, X, O, X, X],
        ]
    };
    let (h, w) = (digit_height, digit_height * 0.6);
    let t = digit_height * 0.17;
    let horizontal = Vec2::new(w, t);
    let vertical = Vec2::new(t, h / 2.0);
    let layout = [
        (Vec2::new(0.0, (h - t) / 2.0), horizontal),
        (Vec2::new((w - t) / 2.0, h / 4.0), vertical),
        (Vec2::new((w - t) / 2.0, -h / 4.0), vertical),
        (Vec2::new(0.0, -(h - t) / 2.0), horizontal),
        (Vec2::new(-(w - t) / 2.0, -h / 4.0), vertical),
        (Vec2::new(-(w - t) / 2.0, h / 4.0), vertical),
        (Vec2::ZERO, horizontal),
    ];

    let digits: Vec<usize> = number
        .to_string()
        .bytes()
        .map(|b| (b - b'0') as usize)
        .collect();
    let gap = w * 0.35;
    let total = digits.len() as f32 * (w + gap) - gap;
    digits
        .iter()
        .enumerate()
        .flat_map(|(i, &digit)| {
            let offset = Vec2::X * (-total / 2.0 + w / 2.0 + i as f32 * (w + gap));
            layout
                .iter()
                .zip(DIGITS[digit])
                .filter(|(_, on)| *on)
                .map(move |(&(center, size), _)| (center + offset, size))
        })
        .collect()
}

/// Piezas móviles: cada una recuerda a qué moto pertenece.
#[derive(Component)]
struct Wheel {
    bike: Entity,
    angle: f32,
    radius: f32,
    front: bool,
}

#[derive(Component)]
struct Steering {
    bike: Entity,
}

/// Mount point of the rider, under `body`. Riding, it goes with the chassis;
/// when the rider goes down ([`CrashState`]) it carries them wherever the
/// crash simulation throws them, independently of the bike.
#[derive(Component, Debug, Clone, Copy)]
pub struct RiderMount {
    pub bike: Entity,
}

/// En la moto: los pivotes que mueven el caballito y la suspensión.
#[derive(Component)]
struct ChassisRig {
    /// Leans the whole bike (see [`LeanPivot`]).
    lean: Entity,
    /// Gira toda la moto sobre una rueda en los caballitos.
    tilt: Entity,
    /// Lo que va sobre la suspensión: todo menos las ruedas.
    body: Entity,
    steering: Entity,
    front_wheel: Entity,
    rear_wheel: Entity,
}

/// En la moto: el material de su piloto trasero, que se enciende al frenar.
#[derive(Component)]
struct BrakeLight {
    material: Handle<StandardMaterial>,
    on: bool,
}

/// Monta la moto y el piloto como hijos de `bike`.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    bike: Entity,
    blueprint: &BikeBlueprint,
    design: Design,
    livery: &Livery,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    let bodywork = &blueprint.bodies[design.index()];
    let geometry = bodywork.geometry;
    // Puntos de montaje: entidades vacías que las animaciones mueven.
    let lean = pivot(commands, bike, Vec3::ZERO, LeanPivot);
    let tilt = pivot(commands, lean, Vec3::ZERO, ());
    let body = pivot(commands, tilt, Vec3::ZERO, ());
    let rider_mount = pivot(commands, body, Vec3::ZERO, RiderMount { bike });
    let steering = pivot(commands, body, geometry.steering_head, ());
    if bodywork.steers {
        commands.entity(steering).insert(Steering { bike });
    }
    let wheel = |radius, front| Wheel {
        bike,
        angle: 0.0,
        radius,
        front,
    };
    let front_wheel = pivot(
        commands,
        steering,
        geometry.front_axle - geometry.steering_head,
        wheel(geometry.front_radius, true),
    );
    let rear_wheel = pivot(
        commands,
        body,
        geometry.rear_axle,
        wheel(geometry.rear_radius, false),
    );
    commands.entity(bike).insert((
        geometry,
        ChassisRig {
            lean,
            tilt,
            body,
            steering,
            front_wheel,
            rear_wheel,
        },
    ));
    let rider = bodywork
        .procedural
        .then(|| rider::spawn_pivots(commands, bike, rider_mount, geometry));

    // Las motos importadas traen su propio material: la decoración solo se
    // pinta en las hechas por código.
    let palette: Vec<Handle<StandardMaterial>> = if bodywork.procedural {
        let textures = LiveryTextures {
            bodywork: images.add(livery::bodywork_texture(
                &livery.bodywork,
                designs::details(design),
                livery.accent,
            )),
            helmet: images.add(livery::helmet_texture(&livery.helmet)),
        };
        Paint::ALL
            .iter()
            .map(|paint| materials.add(paint.material(livery, &textures)))
            .collect()
    } else {
        Vec::new()
    };
    let paint = |p: Paint| palette[p as usize].clone();
    if bodywork.procedural {
        commands.entity(bike).insert(BrakeLight {
            material: paint(Paint::Taillight),
            on: true,
        });
    }

    let shared: &[Part] = if bodywork.procedural {
        &blueprint.running_gear
    } else {
        &[]
    };
    let rider_parts: &[Part] = if rider.is_some() {
        &blueprint.rider_parts
    } else {
        &[]
    };
    for part in shared.iter().chain(rider_parts).chain(&bodywork.parts) {
        let parent = match part.mount {
            Mount::Chassis => body,
            Mount::Steering => steering,
            Mount::FrontWheel => front_wheel,
            Mount::RearWheel => rear_wheel,
            Mount::UpperBody => match &rider {
                Some(rider) => rider.chest,
                None => continue,
            },
            Mount::Head => match &rider {
                Some(rider) => rider.head,
                None => continue,
            },
        };
        commands.spawn((
            Mesh3d(part.mesh.clone()),
            MeshMaterial3d(part.material.clone().unwrap_or_else(|| paint(part.paint))),
            part.transform,
            ChildOf(parent),
        ));
    }

    match (&rider, &bodywork.pilot) {
        (Some(rider), _) => rider::spawn_body(
            commands,
            bike,
            rider_mount,
            steering,
            rider,
            livery,
            &blueprint.rider_bindposes,
            &blueprint.ball,
            paint(Paint::Plastic),
            meshes,
            materials,
        ),
        (None, Some(pilot)) => pilot::spawn(commands, bike, rider_mount, pilot, design),
        (None, None) => {}
    }

    for plate in &bodywork.plates {
        let radius = plate.digit_height * 0.95;
        commands.spawn((
            Mesh3d(blueprint.cube.clone()),
            MeshMaterial3d(paint(Paint::Plate)),
            plate.frame * Transform::from_scale(Vec3::new(2.0 * radius * 1.15, 2.0 * radius, 0.01)),
            ChildOf(body),
        ));
        for (center, size) in number_segments(livery.number, plate.digit_height) {
            commands.spawn((
                Mesh3d(blueprint.cube.clone()),
                MeshMaterial3d(paint(Paint::Number)),
                plate.frame
                    * Transform::from_xyz(center.x, center.y, 0.007).with_scale(size.extend(0.004)),
                ChildOf(body),
            ));
        }
    }
}

fn pivot(commands: &mut Commands, parent: Entity, origin: Vec3, role: impl Bundle) -> Entity {
    commands
        .spawn((
            role,
            Transform::from_translation(origin),
            Visibility::default(),
            ChildOf(parent),
        ))
        .id()
}

/// Coloca el chasis según el caballito y la suspensión del estado físico
/// (interpolado entre pasos), y las ruedas para que sigan en el suelo aunque
/// el chasis se hunda o se levante.
fn move_chassis(
    fixed: Res<Time<Fixed>>,
    bikes: Query<(&BikeState, &PreviousBikeState, &ChassisRig, &Geometry)>,
    mut transforms: Query<&mut Transform>,
) {
    let alpha = fixed.overstep_fraction();
    for (state, previous, rig, geometry) in &bikes {
        let (from, to) = (&previous.0.chassis, &state.chassis);
        let wheelie = from.wheelie.lerp(to.wheelie, alpha);
        let squat = from.squat.lerp(to.squat, alpha);
        // En el suelo, la suspensión sube o baja el chasis sobre las ruedas;
        // en el aire, el chasis sigue su vuelo y son las ruedas las que cuelgan.
        let lift = |s: &BikeState| if s.grounded { s.chassis.heave } else { 0.0 };
        let droop = |s: &BikeState| if s.grounded { 0.0 } else { s.chassis.heave };
        let lift = lift(&previous.0).lerp(lift(state), alpha);
        let droop = droop(&previous.0).lerp(droop(state), alpha);

        // El caballito gira sobre el punto de apoyo de la rueda que sigue en el suelo.
        let contact = Vec3::Z
            * if wheelie >= 0.0 {
                geometry.rear_axle.z
            } else {
                geometry.front_axle.z
            };
        let tilt = Quat::from_rotation_x(wheelie);
        if let Ok(mut transform) = transforms.get_mut(rig.tilt) {
            transform.rotation = tilt;
            transform.translation = contact - tilt * contact;
        }

        // La suspensión cabecea sobre el centro de la moto y sube o baja.
        let center = (geometry.front_axle + geometry.rear_axle) / 2.0;
        let pitch = Quat::from_rotation_x(squat);
        let body = Transform::from_translation(center - pitch * center + Vec3::Y * lift)
            .with_rotation(pitch);
        if let Ok(mut transform) = transforms.get_mut(rig.body) {
            *transform = body;
        }

        // Las ruedas, donde estarían sin suspensión: se ve trabajar la horquilla
        // y el amortiguador.
        let unsprung = body.to_matrix().inverse();
        let hanging = Vec3::Y * droop;
        if let Ok(mut transform) = transforms.get_mut(rig.rear_wheel) {
            transform.translation = unsprung.transform_point3(geometry.rear_axle - hanging);
        }
        let steering = transforms
            .get(rig.steering)
            .map_or(Quat::IDENTITY, |transform| transform.rotation);
        if let Ok(mut transform) = transforms.get_mut(rig.front_wheel) {
            let axle =
                unsprung.transform_point3(geometry.front_axle - hanging) - geometry.steering_head;
            transform.translation = steering.inverse() * axle;
        }
    }
}

/// Places the rider's mount. Riding, it sits on the chassis; once the rider
/// is down it puts their hips where the crash simulation has thrown them
/// (interpolated between steps), undoing the transforms of the bike it hangs
/// from, so the rider moves on their own.
fn update_rider_mount(
    fixed: Res<Time<Fixed>>,
    bikes: Query<(&CrashState, &ChassisRig, &Geometry)>,
    mounts: Query<(Entity, &RiderMount)>,
    mut transforms: Query<&mut Transform>,
) {
    let alpha = fixed.overstep_fraction();
    for (entity, mount) in &mounts {
        let Ok((crash, rig, geometry)) = bikes.get(mount.bike) else {
            continue;
        };
        let local = if crash.is_down() {
            let (from, to) = (&crash.previous, &crash.rider);
            let position = from.position.lerp(to.position, alpha);
            let height = (from.ground + from.height).lerp(to.ground + to.height, alpha);
            // Facing where the bike faced, rolled over by the fall and the
            // tumble, and laid on the back.
            let rotation = ThrownRider::body_rotation(
                to.heading,
                from.roll.lerp(to.roll, alpha),
                from.lie.lerp(to.lie, alpha),
            );
            let hips = Transform::from_translation(to_world(position, height + SURFACE_Y))
                .with_rotation(rotation);
            let world = hips.to_matrix() * Mat4::from_translation(-geometry.seat);
            let parent = [mount.bike, rig.lean, rig.tilt, rig.body]
                .iter()
                .map(|&pivot| {
                    transforms
                        .get(pivot)
                        .map_or(Mat4::IDENTITY, |transform| transform.to_matrix())
                })
                .fold(Mat4::IDENTITY, |parent, pivot| parent * pivot);
            Transform::from_matrix(parent.inverse() * world)
        } else {
            Transform::IDENTITY
        };
        if let Ok(mut transform) = transforms.get_mut(entity)
            && *transform != local
        {
            *transform = local;
        }
    }
}

fn brake_light(
    mut bikes: Query<(&BikeInput, &mut BrakeLight)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (input, mut light) in &mut bikes {
        let on = input.brake > 0.0;
        // Tocar el material solo cuando cambia: cada cambio se sube a la GPU.
        if on == light.on {
            continue;
        }
        light.on = on;
        if let Some(mut material) = materials.get_mut(&light.material) {
            material.emissive = if on {
                LinearRgba::rgb(12.0, 0.3, 0.3)
            } else {
                LinearRgba::rgb(1.5, 0.05, 0.05)
            };
        }
    }
}

fn spin_wheels(
    time: Res<Time>,
    bikes: Query<(&BikeState, Option<&CrashState>)>,
    mut wheels: Query<(&mut Wheel, &mut Transform)>,
) {
    for (mut wheel, mut transform) in &mut wheels {
        let Ok((state, crash)) = bikes.get(wheel.bike) else {
            continue;
        };
        // Down, the wheels spin on by themselves, slowing down.
        let speed = match crash.filter(|crash| crash.is_down()) {
            Some(crash) => crash.wheels[if wheel.front { 0 } else { 1 }],
            None => state.speed,
        };
        // Hacia delante (-Z), la parte de arriba de la rueda avanza: giro negativo en X.
        wheel.angle = (wheel.angle - speed * time.delta_secs() / wheel.radius) % TAU;
        transform.rotation = Quat::from_rotation_x(wheel.angle);
    }
}

fn steer_front(
    tuning: Res<BikeTuning>,
    bikes: Query<(&BikeState, &Geometry)>,
    mut fronts: Query<(&Steering, &mut Transform)>,
) {
    for (steering, mut transform) in &mut fronts {
        let Ok((state, geometry)) = bikes.get(steering.bike) else {
            continue;
        };
        let axis = (geometry.steering_head - geometry.front_axle).normalize();
        // A mucha velocidad la moto gira inclinándose, no con el manillar.
        let speed_ratio = (state.speed.abs() / tuning.max_speed).min(1.0).sqrt();
        let max_angle = MAX_STEER_ANGLE.0.lerp(MAX_STEER_ANGLE.1, speed_ratio);
        transform.rotation = Quat::from_axis_angle(axis, state.steer * max_angle);
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    pub fn every_bike_reaches_no_further_than_its_nose_and_tail_on(
        roster: &'static crate::bike::Roster,
    ) {
        for design in designs::every(roster) {
            let geometry = design.model().map_or(STANDARD, |model| model.geometry);
            let front = -geometry.front_axle.z + geometry.front_radius;
            let back = geometry.rear_axle.z + geometry.rear_radius;
            // The imported models were measured to within a few centimetres.
            assert!(front <= NOSE + 0.03, "{design:?}: {front} m ahead");
            assert!(back <= TAIL + 0.03, "{design:?}: {back} m behind");
        }
        assert!((1.9..2.1).contains(&(NOSE + TAIL)), "a bike's length");
    }

    #[test]
    fn every_bike_reaches_no_further_than_its_nose_and_tail() {
        every_bike_reaches_no_further_than_its_nose_and_tail_on(&crate::testing::DEMO_ROSTER);
    }

    #[test]
    fn number_eight_lights_every_segment() {
        assert_eq!(number_segments(8, 0.1).len(), 7);
        assert_eq!(number_segments(1, 0.1).len(), 2);
        assert_eq!(number_segments(88, 0.1).len(), 14);
    }

    #[test]
    fn two_digit_numbers_read_left_to_right() {
        // El 1 (dos segmentos verticales a la derecha de su cifra) va primero.
        let segments = number_segments(17, 0.1);
        let one_x = segments[0].0.x;
        let seven_x = segments[2].0.x;
        assert!(one_x < seven_x);
    }

    #[test]
    fn palette_order_matches_paints() {
        for (index, paint) in Paint::ALL.into_iter().enumerate() {
            assert_eq!(paint as usize, index);
        }
    }

    pub fn plates_face_outwards_on(roster: &'static crate::bike::Roster) {
        for plate in designs::every(roster).into_iter().flat_map(designs::plates) {
            let normal = plate.frame.rotation * Vec3::Z;
            let outwards = plate.frame.translation * Vec3::new(1.0, 0.0, 1.0);
            assert!(normal.dot(outwards) > 0.0);
        }
    }

    #[test]
    fn plates_face_outwards() {
        plates_face_outwards_on(&crate::testing::DEMO_ROSTER);
    }
}
