//! El piloto. El cuerpo (tronco, cuello, brazos y piernas) es una sola malla
//! deformada por un esqueleto de seis huesos, así hombros, codos y cintura se
//! doblan sin costuras. Lo rígido (casco, joroba, botas, guantes y
//! deslizaderas) va sujeto a su hueso como piezas aparte.
//!
//! Huesos: la pelvis (que se descuelga en las curvas), el pecho (que se
//! incorpora al frenar, se va atrás o adelante con la inercia y se echa sobre
//! el manillar en los caballitos), el brazo y el antebrazo de cada lado (que
//! se recolocan para no soltar los puños) y el muslo y la espinilla de cada
//! lado (que mantienen los pies en las estriberas y, en las curvas, sacan la
//! rodilla de dentro hacia el suelo).

use bevy::{
    camera::visibility::DynamicSkinnedMeshBounds,
    mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    prelude::*,
};

use super::{Geometry, Livery, Mount, Paint, Parts, pivot};
use crate::bike::{BikeInput, BikeState, BikeTuning, CrashState};
use crate::geometry::{approach, smoothstep};
use crate::shapes::{SkinnedBody, SweepPoint, between, bone, bones, section};

/// Cadera del piloto: pivote para descolgarse en las curvas e incorporarse.
/// Va sentado bajo, metido en la moto, como en una deportiva.
pub(super) const HIPS: Vec3 = Vec3::new(0.0, 0.91, 0.24);
/// Hombro y puño derechos (los izquierdos son su reflejo).
const SHOULDER: Vec3 = Vec3::new(0.16, 1.07, -0.16);
pub(super) const GRIP: Vec3 = Vec3::new(0.23, 0.83, -0.53);
const UPPER_ARM: f32 = 0.3;
const FOREARM: f32 = 0.30;
/// Hacia dónde se doblan los codos: hacia fuera y un poco hacia abajo y atrás.
const ELBOW_POLE: Vec3 = Vec3::new(1.0, -0.2, 0.3);
/// Cadera, rodilla y tobillo (sobre la estribera) derechos.
const HIP_JOINT: Vec3 = Vec3::new(0.10, 0.92, 0.22);
const KNEE: Vec3 = Vec3::new(0.28, 0.73, -0.06);
pub(super) const ANKLE: Vec3 = Vec3::new(0.17, 0.45, 0.29);
/// Centro del casco.
pub(super) const HELMET: Vec3 = Vec3::new(0.0, 1.16, -0.30);

/// Turn of the chest that stands the back up straight over the hips: the
/// shoulders sit 0.40 m ahead of and 0.16 m above them (atan2(0.40, 0.16)).
const UPRIGHT_ANGLE: f32 = 1.19;

/// Cuánto se desplaza el piloto hacia el interior de la curva (m).
const HANG_OFF: f32 = 0.22;
/// Cuánto gira la rodilla de dentro hacia fuera, alrededor de la línea de la
/// cadera al tobillo, con la moto tumbada del todo (rad).
const KNEE_OUT: f32 = 1.15;
/// Inclinación extra del piloto respecto a la moto al descolgarse (rad).
const HANG_OFF_LEAN: f32 = 0.32;
/// Cuánto se incorpora el piloto al frenar o ir despacio, y cuánto baja los
/// hombros al agacharse del todo en las rectas (rad).
const SIT_UP_ANGLE: f32 = 0.35;
const TUCK_ANGLE: f32 = 0.15;
/// Por debajo de esta velocidad el piloto va incorporado (m/s).
const CRUISING_SPEED: f32 = 8.0;
const POSE_RESPONSE: f32 = 6.0;
/// La inercia desplaza el tronco: hacia atrás al acelerar y hacia el depósito
/// al frenar (m por cada m/s² de aceleración).
const SURGE_PER_ACCEL: f32 = 0.003;
/// Desplazamiento máximo del tronco: hacia delante .. hacia atrás (m).
const SURGE: (f32, f32) = (-0.07, 0.045);
const SURGE_RESPONSE: f32 = 9.0;
/// Qué parte del caballito compensa el piloto echándose sobre el manillar.
const WHEELIE_COUNTER: f32 = 0.45;

/// Índices de los huesos en la malla y en [`SkinnedMesh::joints`].
const PELVIS: u16 = 0;
const CHEST: u16 = 1;
/// Lados, en el orden de sus huesos: brazos derecho (2 y 3) e izquierdo (4
/// y 5) y piernas derecha (6 y 7) e izquierda (8 y 9).
const SIDES: [f32; 2] = [1.0, -1.0];

const fn upper_arm_bone(side_index: usize) -> u16 {
    2 + 2 * side_index as u16
}

const fn forearm_bone(side_index: usize) -> u16 {
    3 + 2 * side_index as u16
}

const fn thigh_bone(side_index: usize) -> u16 {
    6 + 2 * side_index as u16
}

const fn shin_bone(side_index: usize) -> u16 {
    7 + 2 * side_index as u16
}

fn mirror(side: f32) -> Vec3 {
    Vec3::new(side, 1.0, 1.0)
}

#[derive(Component)]
pub(super) struct Rider {
    bike: Entity,
    hang: f32,
    /// Whether it is going over to the other side (see
    /// [`crate::bike::swinging`]).
    swinging: bool,
    /// Dónde va la cadera en su moto.
    seat: Vec3,
}

#[derive(Component)]
pub(super) struct UpperBody {
    bike: Entity,
    sit_up: f32,
    tuck: f32,
    /// Desplazamiento del tronco por la inercia (+Z, hacia atrás).
    surge: f32,
}

/// Pivote del casco: gira hacia donde mira el piloto.
#[derive(Component)]
pub(super) struct Head {
    bike: Entity,
    look: f32,
    /// Pivotes de la pelvis y del pecho, de los que cuelga.
    pelvis: Entity,
    chest: Entity,
}

/// En la moto: dónde están los pivotes que mueven los brazos.
#[derive(Component)]
pub(super) struct Rig {
    rider: Entity,
    upper_body: Entity,
    steering: Entity,
    /// Medidas de la moto: puños, estriberas y pipa de la dirección.
    geometry: Geometry,
}

/// Hueso de un tramo del brazo: va del hombro al codo o del codo al puño.
#[derive(Component)]
pub(super) struct Arm {
    bike: Entity,
    /// 1 a la derecha, -1 a la izquierda.
    side: f32,
    forearm: bool,
}

/// Pieza de una pierna que se recoloca cada fotograma.
#[derive(Component)]
pub(super) struct Leg {
    bike: Entity,
    /// 1 a la derecha, -1 a la izquierda.
    side: f32,
    segment: LegSegment,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LegSegment {
    /// Hueso de la cadera a la rodilla.
    Thigh,
    /// Hueso de la rodilla al tobillo.
    Shin,
    /// Deslizadera, por fuera de la rodilla.
    KneeSlider,
}

/// Puntos de montaje del piloto: la pelvis, el pecho y la cabeza.
pub(super) struct RiderPivots {
    pub(super) pelvis: Entity,
    pub(super) chest: Entity,
    pub(super) head: Entity,
    geometry: Geometry,
}

pub(super) fn spawn_pivots(
    commands: &mut Commands,
    bike: Entity,
    body: Entity,
    geometry: Geometry,
) -> RiderPivots {
    let pelvis = pivot(
        commands,
        body,
        geometry.seat,
        Rider {
            bike,
            hang: 0.0,
            swinging: false,
            seat: geometry.seat,
        },
    );
    let chest = pivot(
        commands,
        pelvis,
        Vec3::ZERO,
        UpperBody {
            bike,
            sit_up: 0.0,
            tuck: 0.0,
            surge: 0.0,
        },
    );
    let head = pivot(
        commands,
        chest,
        HELMET - HIPS,
        Head {
            bike,
            look: 0.0,
            pelvis,
            chest,
        },
    );
    RiderPivots {
        pelvis,
        chest,
        head,
        geometry,
    }
}

/// Crea los huesos de los brazos, los guantes y el cuerpo deformable.
#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_body(
    commands: &mut Commands,
    bike: Entity,
    body: Entity,
    steering: Entity,
    pivots: &RiderPivots,
    livery: &Livery,
    bindposes: &Handle<SkinnedMeshInverseBindposes>,
    ball: &Handle<Mesh>,
    gloves: Handle<StandardMaterial>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.entity(bike).insert(Rig {
        rider: pivots.pelvis,
        upper_body: pivots.chest,
        steering,
        geometry: pivots.geometry,
    });

    let mut joints = vec![pivots.pelvis, pivots.chest];
    let mut legs = Vec::new();
    for side in SIDES {
        let (shoulder, elbow, grip) = arm_at_rest(side);
        for (forearm, rest) in [
            (false, between(shoulder, elbow)),
            (true, between(elbow, grip)),
        ] {
            let arm = commands
                .spawn((
                    Arm {
                        bike,
                        side,
                        forearm,
                    },
                    rest,
                    Visibility::default(),
                    ChildOf(body),
                ))
                .id();
            joints.push(arm);
            if forearm {
                commands.spawn((
                    Mesh3d(ball.clone()),
                    MeshMaterial3d(gloves.clone()),
                    Transform::from_xyz(0.0, 0.0, FOREARM)
                        .with_scale(Vec3::new(0.085, 0.075, 0.115)),
                    ChildOf(arm),
                ));
            }
        }
        let (hip, knee, ankle) = leg_at_rest(side);
        for (segment, rest) in [
            (LegSegment::Thigh, between(hip, knee)),
            (LegSegment::Shin, between(knee, ankle)),
        ] {
            legs.push(
                commands
                    .spawn((
                        Leg {
                            bike,
                            side,
                            segment,
                        },
                        rest,
                        Visibility::default(),
                        ChildOf(body),
                    ))
                    .id(),
            );
        }
        commands.spawn((
            Leg {
                bike,
                side,
                segment: LegSegment::KneeSlider,
            },
            Mesh3d(ball.clone()),
            MeshMaterial3d(gloves.clone()),
            Transform::from_translation(knee).with_scale(KNEE_SLIDER_SIZE),
            ChildOf(body),
        ));
    }
    joints.extend(legs);

    // El color va en los vértices: el material solo pone el brillo del cuero.
    commands.spawn((
        Mesh3d(meshes.add(body_mesh(livery))),
        MeshMaterial3d(materials.add(StandardMaterial {
            perceptual_roughness: 0.45,
            ..default()
        })),
        SkinnedMesh {
            inverse_bindposes: bindposes.clone(),
            joints,
        },
        DynamicSkinnedMeshBounds,
        Transform::default(),
        ChildOf(body),
    ));
}

/// Hombro, codo y puño de un lado con el piloto agachado y sin descolgarse:
/// la pose en la que se modela el cuerpo.
fn arm_at_rest(side: f32) -> (Vec3, Vec3, Vec3) {
    let (shoulder, grip) = (SHOULDER * mirror(side), GRIP * mirror(side));
    let elbow = elbow(
        shoulder,
        grip,
        UPPER_ARM,
        FOREARM,
        ELBOW_POLE * mirror(side),
    );
    (shoulder, elbow, grip)
}

/// Cadera, rodilla y tobillo de un lado con el piloto sin descolgarse.
fn leg_at_rest(side: f32) -> (Vec3, Vec3, Vec3) {
    (
        HIP_JOINT * mirror(side),
        KNEE * mirror(side),
        ANKLE * mirror(side),
    )
}

/// Largo del muslo y de la espinilla.
fn leg_lengths() -> (f32, f32) {
    (HIP_JOINT.distance(KNEE), KNEE.distance(ANKLE))
}

/// Hacia dónde se dobla la rodilla de un lado: en reposo, hacia delante y
/// algo hacia fuera; con `out` de 0 a 1, gira hacia fuera alrededor de la
/// línea de la cadera al tobillo hasta apuntar al suelo con la moto tumbada.
fn knee_pole(side: f32, hip: Vec3, ankle: Vec3, out: f32) -> Vec3 {
    let (rest_hip, rest_knee, rest_ankle) = leg_at_rest(side);
    let axis = (rest_ankle - rest_hip).normalize();
    let knee = rest_knee - rest_hip;
    let rest_pole = knee - axis * knee.dot(axis);
    // El eje apunta hacia abajo: girar en positivo lleva la rodilla derecha
    // hacia +X, y la izquierda, con el giro al revés, hacia -X.
    let pole = Quat::from_axis_angle(axis, KNEE_OUT * out * side) * rest_pole;
    // Si la cadera se ha movido, el plano de la pierna gira con ella.
    let now = (ankle - hip).normalize();
    Quat::from_rotation_arc(axis, now) * pole
}

/// Inversas de la pose de cada hueso en reposo, en el orden de los índices.
pub(super) fn inverse_bindposes() -> SkinnedMeshInverseBindposes {
    let mut poses = vec![Transform::from_translation(HIPS); 2];
    for side in SIDES {
        let (shoulder, elbow, grip) = arm_at_rest(side);
        poses.push(between(shoulder, elbow));
        poses.push(between(elbow, grip));
    }
    for side in SIDES {
        let (hip, knee, ankle) = leg_at_rest(side);
        poses.push(between(hip, knee));
        poses.push(between(knee, ankle));
    }
    poses
        .iter()
        .map(|pose| pose.to_matrix().inverse())
        .collect::<Vec<_>>()
        .into()
}

/// El cuerpo del piloto en reposo, en el espacio de la moto, con los colores
/// del mono.
fn body_mesh(livery: &Livery) -> Mesh {
    let (suit, trim) = (livery.suit.to_linear(), livery.suit_trim.to_linear());
    let point = |center: Vec3, radius: Vec2, color: LinearRgba, bones| SweepPoint {
        center,
        radius,
        color,
        bones,
    };
    let round = Vec2::splat;
    let mut body = SkinnedBody::default();

    // Tronco tumbado sobre el depósito: la parte de atrás va con la pelvis y la
    // de delante con el pecho, y entre medias se mezclan (la cintura).
    body.sweep(
        &[
            point(
                Vec3::new(0.0, 0.925, 0.415),
                Vec2::new(0.04, 0.03),
                suit,
                bone(PELVIS),
            ),
            point(
                Vec3::new(0.0, 0.94, 0.38),
                Vec2::new(0.115, 0.085),
                suit,
                bone(PELVIS),
            ),
            point(
                Vec3::new(0.0, 0.97, 0.29),
                Vec2::new(0.15, 0.11),
                suit,
                bones(PELVIS, CHEST, 0.2),
            ),
            point(
                Vec3::new(0.0, 1.01, 0.15),
                Vec2::new(0.17, 0.12),
                suit,
                bones(PELVIS, CHEST, 0.8),
            ),
            point(
                Vec3::new(0.0, 1.05, 0.0),
                Vec2::new(0.19, 0.115),
                suit,
                bone(CHEST),
            ),
            point(
                Vec3::new(0.0, 1.075, -0.13),
                Vec2::new(0.19, 0.095),
                suit,
                bone(CHEST),
            ),
            point(
                Vec3::new(0.0, 1.085, -0.205),
                Vec2::new(0.15, 0.07),
                suit,
                bone(CHEST),
            ),
            point(
                Vec3::new(0.0, 1.085, -0.235),
                Vec2::new(0.05, 0.03),
                suit,
                bone(CHEST),
            ),
        ],
        Vec3::Y,
    );
    // Cuello, desde dentro del tronco hasta dentro del casco.
    body.sweep(
        &[
            point(Vec3::new(0.0, 1.06, -0.16), round(0.06), suit, bone(CHEST)),
            point(
                Vec3::new(0.0, 1.105, -0.25),
                round(0.052),
                suit,
                bone(CHEST),
            ),
            point(Vec3::new(0.0, 1.135, -0.31), round(0.05), suit, bone(CHEST)),
        ],
        Vec3::Y,
    );

    for (index, side) in SIDES.into_iter().enumerate() {
        let (upper, fore) = (upper_arm_bone(index), forearm_bone(index));
        let (shoulder, elbow, grip) = arm_at_rest(side);
        let along_upper = (elbow - shoulder).normalize();
        let [before_elbow, elbow, after_elbow] = rounded_corner(shoulder, elbow, grip, 0.1);
        // Brazo: nace dentro del tronco (mezclado con el pecho), el codo se
        // dobla en arco y el cambio de color va al empezar el antebrazo.
        body.sweep(
            &[
                point(
                    shoulder - Vec3::X * 0.04 * side,
                    round(0.072),
                    trim,
                    bones(CHEST, upper, 0.4),
                ),
                point(
                    shoulder + along_upper * 0.07,
                    round(0.066),
                    trim,
                    bone(upper),
                ),
                point(before_elbow, round(0.056), trim, bone(upper)),
                point(elbow, round(0.054), trim, bones(upper, fore, 0.5)),
                point(after_elbow, round(0.052), trim, bone(fore)),
                point(after_elbow, round(0.052), suit, bone(fore)),
                point(grip, round(0.042), suit, bone(fore)),
            ],
            Vec3::Y,
        );

        // Pierna: muslo del color del mono y espinillera del de los detalles.
        let (thigh, shin) = (thigh_bone(index), shin_bone(index));
        let (hip, knee, ankle) = leg_at_rest(side);
        let [before_knee, knee, after_knee] = rounded_corner(hip, knee, ankle, 0.1);
        body.sweep(
            &[
                point(
                    hip + Vec3::new(-0.02 * side, 0.02, 0.05),
                    round(0.095),
                    suit,
                    bones(PELVIS, thigh, 0.5),
                ),
                point(
                    hip.lerp(before_knee, 0.5),
                    Vec2::new(0.09, 0.085),
                    suit,
                    bone(thigh),
                ),
                point(before_knee, round(0.076), suit, bone(thigh)),
                point(knee, round(0.072), suit, bones(thigh, shin, 0.5)),
                point(after_knee, round(0.066), suit, bone(shin)),
                point(after_knee, round(0.066), trim, bone(shin)),
                point(after_knee.lerp(ankle, 0.5), round(0.058), trim, bone(shin)),
                point(ankle, round(0.05), trim, bone(shin)),
            ],
            Vec3::Y,
        );
    }
    body.build()
}

/// Redondea la esquina de un camino `from` → `corner` → `to` con un arco de
/// radio `bend_radius`: devuelve dónde empieza el arco, su punto medio y
/// dónde acaba. Con un radio mayor que el grosor del tubo, la cara interior
/// de la articulación no se pliega sobre sí misma.
fn rounded_corner(from: Vec3, corner: Vec3, to: Vec3, bend_radius: f32) -> [Vec3; 3] {
    let (incoming, outgoing) = ((corner - from).normalize(), (to - corner).normalize());
    let turn = incoming.angle_between(outgoing);
    let reach = bend_radius * (turn / 2.0).tan();
    // El centro del arco está sobre la bisectriz del ángulo interior.
    let inwards = (outgoing - incoming).normalize();
    let sag = bend_radius / (turn / 2.0).cos() - bend_radius;
    [
        corner - incoming * reach,
        corner + inwards * sag,
        corner + outgoing * reach,
    ]
}

/// Piezas rígidas del piloto: casco, joroba y botas.
pub(super) fn build_rigid_parts(parts: &mut Parts) {
    use Mount::{Chassis, Head, UpperBody};

    // Joroba aerodinámica del mono, sobre la espalda.
    parts.loft(
        UpperBody,
        Paint::SuitTrim,
        &[
            section(-0.16, 1.145, 0.05, 0.02),
            section(-0.06, 1.18, 0.08, 0.05),
            section(0.06, 1.16, 0.07, 0.035),
            section(0.14, 1.13, 0.04, 0.02),
        ],
    );

    // Casco integral (con la visera y la decoración en la textura), con la
    // cabeza algo levantada para mirar por encima de la cúpula.
    let helmet = parts.meshes.add(helmet_mesh());
    parts.add(
        Head,
        Paint::Helmet,
        helmet,
        Transform::from_translation(HELMET).with_rotation(Quat::from_rotation_x(0.12)),
    );

    // Botas en las estriberas: van con la moto, no con el piloto.
    parts.loft_pair(
        Chassis,
        Paint::Plastic,
        ANKLE.x,
        &[
            section(0.17, 0.435, 0.038, 0.048),
            section(0.25, 0.425, 0.048, 0.064),
            section(0.36, 0.44, 0.043, 0.058),
        ],
    );
}

/// Tamaño de la deslizadera de la rodilla: plana, hacia fuera.
const KNEE_SLIDER_SIZE: Vec3 = Vec3::new(0.045, 0.1, 0.085);

/// Casco integral: una esfera deformada, más larga que ancha, con la
/// mentonera adelantada y un alerón detrás. Conserva las coordenadas de
/// textura de la esfera, así la decoración se pinta igual que en una bola.
pub(super) fn helmet_mesh() -> Mesh {
    // Semiejes: ancho, alto y largo (m).
    const RADII: Vec3 = Vec3::new(0.13, 0.138, 0.158);
    let mut mesh = Sphere::new(1.0).mesh().uv(32, 20);
    let shape = |d: Vec3| {
        let mut p = d * RADII;
        // Abajo, plano: la abertura del cuello.
        p.y = p.y.max(-0.105);
        // Mentonera: delante y abajo, sale hacia delante y baja.
        let chin = smoothstep(0.2, 0.8, -d.z) * smoothstep(-0.1, 0.6, -d.y);
        p += Vec3::new(0.0, -0.025, -0.035) * chin;
        // Alerón: detrás y arriba.
        let spoiler = smoothstep(0.5, 0.9, d.z) * smoothstep(0.1, 0.5, d.y);
        p += Vec3::new(0.0, 0.006, 0.022) * spoiler;
        p
    };
    let positions: Vec<[f32; 3]> = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|positions| positions.as_float3())
        .expect("la esfera tiene posiciones")
        .iter()
        .map(|&p| shape(Vec3::from(p)).to_array())
        .collect();
    // Normales del elipsoide: sin costura donde se cierra la textura.
    let normals: Vec<[f32; 3]> = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|positions| positions.as_float3())
        .expect("la esfera tiene posiciones")
        .iter()
        .map(|&p| (Vec3::from(p) / (RADII * RADII)).normalize().to_array())
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh
}

pub(super) fn hang_off(
    time: Res<Time>,
    tuning: Res<BikeTuning>,
    bikes: Query<(&BikeState, &BikeInput, Option<&super::super::CrashState>)>,
    mut riders: Query<(&mut Rider, &mut Transform)>,
) {
    for (mut rider, mut transform) in &mut riders {
        let Ok((state, input, crash)) = bikes.get(rider.bike) else {
            continue;
        };
        if crash.is_some_and(|c| c.is_down()) {
            rider.hang = approach(rider.hang, 0.0, POSE_RESPONSE, time.delta_secs());
            transform.translation = rider.seat;
            transform.rotation = Quat::IDENTITY;
            continue;
        }
        let target = (state.lean / tuning.max_lean).clamp(-1.0, 1.0);
        // Over to the other side as fast as the controls say, like the bike.
        rider.swinging = crate::bike::swinging(rider.swinging, rider.hang, target);
        let response = POSE_RESPONSE
            * if rider.swinging {
                1.0 - input.pivot_slowing
            } else {
                1.0
            };
        rider.hang = approach(rider.hang, target, response, time.delta_secs());
        // Inclinación positiva = hacia la izquierda (-X): el piloto se va hacia ese lado.
        transform.translation = rider.seat - Vec3::X * HANG_OFF * rider.hang;
        transform.rotation = Quat::from_rotation_z(HANG_OFF_LEAN * rider.hang);
    }
}

pub(super) fn sit_up(
    time: Res<Time>,
    tuning: Res<BikeTuning>,
    bikes: Query<(&BikeState, &BikeInput, &CrashState)>,
    mut bodies: Query<(&mut UpperBody, &mut Transform)>,
) {
    for (mut body, mut transform) in &mut bodies {
        let Ok((state, input, crash)) = bikes.get(body.bike) else {
            continue;
        };
        let dt = time.delta_secs();
        if !crash.is_down() {
            let slow = 1.0 - (state.speed.abs() / CRUISING_SPEED).min(1.0);
            // Tumbado va descolgado y agachado: incorporarse lo alejaría del puño de fuera.
            let upright = 1.0 - (state.lean / tuning.max_lean).abs().min(1.0);
            let target = input.brake.max(slow) * upright;
            body.sit_up = approach(body.sit_up, target, POSE_RESPONSE, dt);
            let tuck = super::tuck_amount(state, &tuning) * (1.0 - body.sit_up);
            body.tuck = approach(body.tuck, tuck, POSE_RESPONSE, dt);
            let surge = (state.accel * SURGE_PER_ACCEL).clamp(SURGE.0, SURGE.1);
            body.surge = approach(body.surge, surge, SURGE_RESPONSE, dt);
        }
        // Girar en X positivo levanta los hombros (que están hacia -Z); con la
        // moto encabritada, el piloto carga el peso delante.
        let wheelie = state.chassis.wheelie.max(0.0);
        let riding =
            SIT_UP_ANGLE * body.sit_up - TUCK_ANGLE * body.tuck - WHEELIE_COUNTER * wheelie;
        // Off the bike the back straightens, so the body lies flat on its back.
        let let_go = crash.let_go();
        transform.translation = Vec3::Z * body.surge * (1.0 - let_go);
        transform.rotation = Quat::from_rotation_x(riding.lerp(UPRIGHT_ANGLE, let_go));
    }
}

/// Gira el casco hacia donde se dirige la moto, sobre su propio eje, y lo
/// endereza respecto a la moto: no se tumba con el tronco al descolgarse.
pub(super) fn look_ahead(
    time: Res<Time>,
    tuning: Res<BikeTuning>,
    bikes: Query<(&BikeState, &CrashState)>,
    pivots: Query<&Transform, Without<Head>>,
    mut heads: Query<(&mut Head, &mut Transform)>,
) {
    for (mut head, mut transform) in &mut heads {
        let (Ok((state, crash)), Ok(pelvis), Ok(chest)) = (
            bikes.get(head.bike),
            pivots.get(head.pelvis),
            pivots.get(head.chest),
        ) else {
            continue;
        };
        // Off the bike the head goes with the body.
        let (target, lean) = if crash.is_down() {
            (0.0, 0.0)
        } else {
            (super::look_angle(state, &tuning), state.lean)
        };
        head.look = approach(head.look, target, POSE_RESPONSE, time.delta_secs());
        // En la moto: cabeceo del pecho (al incorporarse), enderezada y girada.
        // Girar en Y positivo lleva la vista (-Z) hacia la izquierda (-X).
        let wanted = Quat::from_rotation_z(-super::HEAD_LEVEL * lean)
            * chest.rotation
            * Quat::from_rotation_y(head.look);
        transform.rotation = (pelvis.rotation * chest.rotation).inverse() * wanted;
    }
}

/// Coloca los huesos de los brazos para que, se mueva como se mueva el
/// tronco, las manos sigan en los puños.
/// Index of a side (1 right, -1 left) in [`crate::bike::crash::Limbs`].
fn side_index(side: f32) -> usize {
    if side > 0.0 { 0 } else { 1 }
}

pub(super) fn reach_grips(
    rigs: Query<&Rig>,
    bikes: Query<&CrashState>,
    pivots: Query<&Transform, Without<Arm>>,
    mut arms: Query<(&Arm, &mut Transform)>,
) {
    for (arm, mut transform) in &mut arms {
        let Ok(rig) = rigs.get(arm.bike) else {
            continue;
        };
        let (Ok(pelvis), Ok(chest), Ok(steering)) = (
            pivots.get(rig.rider),
            pivots.get(rig.upper_body),
            pivots.get(rig.steering),
        ) else {
            continue;
        };
        let mirror = mirror(arm.side);
        let shoulder = (*pelvis * *chest).transform_point((SHOULDER - HIPS) * mirror);
        let geometry = &rig.geometry;
        let grip = steering.transform_point((geometry.grip - geometry.steering_head) * mirror);
        // Off the bike, the hands move as the fall goes (see `ThrownRider::swing`).
        let grip = match bikes.get(arm.bike).ok().filter(|crash| crash.is_down()) {
            Some(crash) => {
                let hand = crash.rider.limbs.hands[side_index(arm.side)];
                grip.lerp(pelvis.transform_point(hand), crash.let_go())
            }
            None => grip,
        };
        let elbow = elbow(shoulder, grip, UPPER_ARM, FOREARM, ELBOW_POLE * mirror);
        *transform = if arm.forearm {
            between(elbow, grip)
        } else {
            between(shoulder, elbow)
        };
    }
}

/// Coloca los huesos de las piernas para que los pies sigan en las
/// estriberas se mueva como se mueva la cadera (o bajen el pie al suelo en
/// parado), y saca la rodilla de dentro de la curva hacia el suelo.
pub(super) fn reach_pegs(
    rigs: Query<&Rig>,
    bikes: Query<(&BikeState, &CrashState)>,
    riders: Query<(&Rider, &Transform), Without<Leg>>,
    mut legs: Query<(&Leg, &mut Transform), Without<Rider>>,
) {
    let (thigh, shin) = leg_lengths();
    for (leg, mut transform) in &mut legs {
        let Ok(rig) = rigs.get(leg.bike) else {
            continue;
        };
        let Ok((rider, pelvis)) = riders.get(rig.rider) else {
            continue;
        };
        let hip = pelvis.transform_point((HIP_JOINT - HIPS) * mirror(leg.side));
        let mut ankle = rig.geometry.peg * mirror(leg.side);
        if leg.side > 0.0 {
            let stopped = bikes.get(leg.bike).map_or(0.0, |(state, crash)| {
                if state.grounded && !crash.is_down() {
                    (1.0 - state.speed.abs() / crate::bike::STOPPED_SPEED).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            });
            let foot_ground = Vec3::new(0.38, 0.19, rig.geometry.peg.z - 0.04);
            ankle = ankle.lerp(foot_ground, stopped);
        }
        // Off the bike, the feet move as the fall goes (see `ThrownRider::swing`).
        if let Some((_, crash)) = bikes
            .get(leg.bike)
            .ok()
            .filter(|(_, crash)| crash.is_down())
        {
            let foot = crash.rider.limbs.feet[side_index(leg.side)];
            ankle = ankle.lerp(pelvis.transform_point(foot), crash.let_go());
        }
        // Inclinación positiva = hacia la izquierda: la pierna de dentro es la izquierda.
        let inside = (-rider.hang * leg.side).max(0.0);
        let pole = knee_pole(leg.side, hip, ankle, inside);
        let knee = elbow(hip, ankle, thigh, shin, pole);
        match leg.segment {
            LegSegment::Thigh => *transform = between(hip, knee),
            LegSegment::Shin => *transform = between(knee, ankle),
            LegSegment::KneeSlider => {
                // Por fuera de la rodilla, hacia donde se dobla.
                let line = (ankle - hip).normalize();
                let out = (knee - hip)
                    .reject_from_normalized(line)
                    .normalize_or_zero();
                transform.translation = knee + out * 0.06;
                transform.rotation = Quat::from_rotation_arc(Vec3::X * leg.side, out);
            }
        }
    }
}

/// Codo de un brazo de dos tramos (`upper` y `fore`) que va de `shoulder` a
/// `hand`, doblado hacia `pole`. Si la mano queda fuera de alcance, el brazo
/// apunta hacia ella estirado.
fn elbow(shoulder: Vec3, hand: Vec3, upper: f32, fore: f32, pole: Vec3) -> Vec3 {
    let to_hand = hand - shoulder;
    let direction = to_hand.normalize();
    let distance = to_hand.length().clamp(1e-4, upper + fore);
    // Teorema del coseno: cuánto avanza el codo sobre la línea hombro-mano y
    // cuánto se separa de ella.
    let along = (upper * upper - fore * fore + distance * distance) / (2.0 * distance);
    let out = (upper * upper - along * along).max(0.0).sqrt();
    let bend = (pole - direction * pole.dot(direction)).normalize_or_zero();
    shoulder + direction * along + bend * out
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn elbow_keeps_arm_lengths_when_the_grip_is_reachable() {
        let elbow = elbow(SHOULDER, GRIP, UPPER_ARM, FOREARM, ELBOW_POLE);
        assert!((elbow.distance(SHOULDER) - UPPER_ARM).abs() < 1e-4);
        assert!((elbow.distance(GRIP) - FOREARM).abs() < 1e-4);
        assert!(
            elbow.x > SHOULDER.x.max(GRIP.x),
            "el codo derecho sale hacia fuera"
        );
    }

    #[test]
    fn arm_points_at_an_unreachable_hand() {
        let elbow = elbow(Vec3::ZERO, Vec3::new(0.0, 0.0, -2.0), 0.3, 0.3, Vec3::X);
        assert!(elbow.abs_diff_eq(Vec3::new(0.0, 0.0, -0.3), 1e-4));
    }

    #[test]
    fn grips_are_reachable_tucked_sitting_up_and_hanging_off() {
        // Al descolgarse se incorpora menos (ver `sit_up`).
        for hang in [-1.0, -0.5, 0.0, 0.5, 1.0f32] {
            for angle in [0.0, SIT_UP_ANGLE * (1.0 - hang.abs())] {
                let pelvis = Transform::from_translation(HIPS - Vec3::X * HANG_OFF * hang)
                    .with_rotation(Quat::from_rotation_z(HANG_OFF_LEAN * hang));
                for side in SIDES {
                    let chest = Quat::from_rotation_x(angle) * (SHOULDER - HIPS) * mirror(side);
                    let shoulder = pelvis.transform_point(chest);
                    assert!(
                        shoulder.distance(GRIP * mirror(side)) <= UPPER_ARM + FOREARM,
                        "ángulo {angle}, descolgado {hang}, lado {side}"
                    );
                }
            }
        }
    }

    #[test]
    fn rounded_corner_is_tangent_to_both_sides() {
        let corner = rounded_corner(Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), 0.2);
        // Esquina de 90°: el arco empieza y acaba a un radio de la esquina, y
        // su punto medio está a r·(√2 - 1) hacia dentro.
        assert!(corner[0].abs_diff_eq(Vec3::new(0.8, 0.0, 0.0), 1e-5));
        assert!(corner[2].abs_diff_eq(Vec3::new(1.0, 0.2, 0.0), 1e-5));
        let sag = 0.2 * (2f32.sqrt() - 1.0);
        let inwards = Vec3::new(-1.0, 1.0, 0.0).normalize();
        assert!(corner[1].abs_diff_eq(Vec3::X + inwards * sag, 1e-5));
    }

    #[test]
    fn legs_rest_on_the_pegs_and_the_inner_knee_goes_down() {
        let (thigh, shin) = leg_lengths();
        for side in SIDES {
            let (hip, knee, ankle) = leg_at_rest(side);
            // En reposo, la pierna articulada es la del modelo.
            let rest = elbow(hip, ankle, thigh, shin, knee_pole(side, hip, ankle, 0.0));
            assert!(rest.abs_diff_eq(knee, 1e-4), "{rest} frente a {knee}");
            // Rodilla de dentro: más hacia fuera.
            let out = elbow(hip, ankle, thigh, shin, knee_pole(side, hip, ankle, 1.0));
            assert!(
                out.x * side > knee.x * side + 0.1,
                "sale hacia fuera: {out}"
            );
            assert!((out.distance(ankle) - shin).abs() < 1e-4);
        }
    }

    #[test]
    fn helmet_is_longer_than_wide_with_the_chin_ahead() {
        let mesh = helmet_mesh();
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .unwrap();
        let (min, max) = positions
            .iter()
            .fold((Vec3::MAX, Vec3::MIN), |(min, max), &p| {
                (min.min(p.into()), max.max(p.into()))
            });
        let size = max - min;
        assert!(size.z > size.x, "más largo que ancho: {size}");
        assert!(-min.z > max.z, "la mentonera sale por delante");
    }

    #[test]
    fn body_bones_match_the_skeleton() {
        let joints = 2 + 4 * SIDES.len();
        assert_eq!(inverse_bindposes().len(), joints);
        let mesh = body_mesh(&super::super::COVER);
        let Some(bevy::mesh::VertexAttributeValues::Uint16x4(indices)) =
            mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
        else {
            panic!("la malla no tiene huesos");
        };
        assert!(indices.iter().flatten().all(|&j| (j as usize) < joints));
    }
}
