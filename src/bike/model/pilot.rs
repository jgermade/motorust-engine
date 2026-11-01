//! Los pilotos de Meshy: cada uno, una malla con esqueleto de Mixamo (cadera,
//! tres vértebras, cuello, cabeza y brazos y piernas de tres tramos) que se
//! coloca en postura de carrera cada fotograma.
//!
//! La postura se calcula a partir del esqueleto en reposo (de pie):
//!
//! - La cadera va sobre el asiento, se descuelga en las curvas y se mueve con
//!   la inercia.
//! - La espalda se dobla hacia delante sobre el depósito (menos al frenar o
//!   ir despacio, y más en los caballitos) y el cuello y la cabeza se levantan
//!   para mirar al frente.
//! - La cabeza no sigue al tronco: gira sobre el cuello hacia donde va la moto
//!   y se endereza para llevar la vista más cerca del horizonte.
//! - Brazos y piernas usan cinemática inversa de dos tramos: las manos van a
//!   los puños, que giran con la dirección, y los pies a las estriberas; en las
//!   curvas, la rodilla de dentro sale hacia el suelo.

use bevy::{
    camera::visibility::DynamicSkinnedMeshBounds,
    mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    prelude::*,
};

use super::imported::{self, Joint, Model};
use super::{ChassisRig, Design, Geometry};
use crate::bike::{BikeInput, BikeState, BikeTuning};
use crate::geometry::approach;

/// Cuánto se inclina la pelvis hacia delante sobre el asiento (rad).
const PELVIS_PITCH: f32 = 0.9;
/// Cuánto se dobla cada vértebra hacia delante, de abajo arriba (rad).
const SPINE_BEND: [f32; 3] = [0.2, 0.17, 0.12];
/// Cuánto se levantan el cuello y la cabeza para mirar al frente (rad).
const NECK_LIFT: f32 = 0.6;
const HEAD_LIFT: f32 = 0.5;
/// Qué parte del giro de la cabeza hace el cuello, y cuánto se acerca el
/// cuello a la postura de la cabeza en vez de a la del tronco.
const NECK_TURN: f32 = 0.35;
const NECK_FOLLOW: f32 = 0.5;
/// Qué parte de la espalda se endereza al incorporarse, y cuánto más se
/// dobla al agacharse del todo en las rectas.
const SIT_UP: f32 = 0.45;
const TUCK: f32 = 0.35;
/// Lo más que se dobla la espalda para llegar a los puños (1 es agachado) y
/// lo que les falta a los brazos para ir estirados del todo (m).
const MAX_BEND: f32 = 1.8;
const REACH_MARGIN: f32 = 0.01;
/// Cuánto se desplaza la cadera hacia dentro de la curva (m) y cuánto se
/// tumba el tronco hacia ese lado (rad), con la moto tumbada del todo.
const HANG_OFF: f32 = 0.21;
const HANG_OFF_LEAN: f32 = 0.34;
/// Torsión del tronco hacia dentro de la curva, con la moto tumbada del todo
/// (rad): el hombro de fuera se adelanta hacia su puño.
const HANG_OFF_TWIST: f32 = 0.5;
/// Giro de la rodilla de dentro hacia fuera, con la moto tumbada del todo (rad).
const KNEE_OUT: f32 = 1.1;
/// Hacia dónde apuntan los dedos de la mano derecha al agarrar el puño
/// (la izquierda es su reflejo): hacia delante, algo hacia abajo y hacia
/// dentro, con la palma encima del puño.
const FINGERS: Vec3 = Vec3::new(-0.2, -0.5, -1.0);
/// De la muñeca al centro de la palma (m) y altura de la palma sobre el eje
/// del puño.
const PALM_FROM_WRIST: f32 = 0.07;
/// Off the bike, how far a relaxed hand bends in at the wrist (the tangent of
/// the angle).
const RELAXED_WRIST: f32 = 0.9;
const PALM_ABOVE_GRIP: f32 = 0.03;
/// La inercia desplaza la cadera (m por cada m/s²) entre estos topes.
const SURGE_PER_ACCEL: f32 = 0.003;
const SURGE: (f32, f32) = (-0.05, 0.035);
const CRUISING_SPEED: f32 = 8.0;
const POSE_RESPONSE: f32 = 6.0;

/// Índices de los huesos que mueve la postura.
#[derive(Clone, Debug)]
pub(super) struct Bones {
    hips: usize,
    spine: [usize; 3],
    neck: usize,
    head: usize,
    /// Brazo, antebrazo y mano; derecho e izquierdo.
    arms: [[usize; 3]; 2],
    /// Muslo, pierna, pie y puntera; derecha e izquierda.
    legs: [[usize; 4]; 2],
}

/// Esqueleto en reposo y lo que hace falta para posarlo.
pub(super) struct Skeleton {
    joints: Vec<Joint>,
    bones: Bones,
    /// Por hueso: posición respecto a su padre y giro respecto a él, en reposo.
    offsets: Vec<Vec3>,
    local_rotations: Vec<Quat>,
}

impl Skeleton {
    fn new(joints: Vec<Joint>) -> Self {
        let find = |name: &str| {
            joints
                .iter()
                .position(|j| j.name == name)
                .unwrap_or_else(|| panic!("al esqueleto le falta {name}"))
        };
        let side = |s: &str| {
            (
                [
                    find(&format!("{s}Arm")),
                    find(&format!("{s}ForeArm")),
                    find(&format!("{s}Hand")),
                ],
                [
                    find(&format!("{s}UpLeg")),
                    find(&format!("{s}Leg")),
                    find(&format!("{s}Foot")),
                    find(&format!("{s}ToeBase")),
                ],
            )
        };
        let ((right_arm, right_leg), (left_arm, left_leg)) = (side("Right"), side("Left"));
        let bones = Bones {
            hips: find("Hips"),
            spine: [find("Spine"), find("Spine1"), find("Spine2")],
            neck: find("Neck"),
            head: find("Head"),
            arms: [right_arm, left_arm],
            legs: [right_leg, left_leg],
        };
        let (offsets, local_rotations) = joints
            .iter()
            .map(|joint| match joint.parent {
                Some(p) => {
                    let parent = &joints[p];
                    let inverse = parent.rotation.inverse();
                    (
                        inverse * (joint.position - parent.position),
                        inverse * joint.rotation,
                    )
                }
                None => (joint.position, joint.rotation),
            })
            .unzip();
        Self {
            joints,
            bones,
            offsets,
            local_rotations,
        }
    }

    /// Inversas de la postura de reposo de cada hueso.
    fn inverse_bindposes(&self) -> SkinnedMeshInverseBindposes {
        self.joints
            .iter()
            .map(|j| {
                Transform::from_translation(j.position)
                    .with_rotation(j.rotation)
                    .to_matrix()
                    .inverse()
            })
            .collect::<Vec<_>>()
            .into()
    }
}

/// Malla, material y esqueleto del piloto de una moto, compartidos por todas
/// las motos de ese modelo.
pub(super) struct PilotBlueprint {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    bindposes: Handle<SkinnedMeshInverseBindposes>,
    pub(super) skeleton: Skeleton,
}

impl PilotBlueprint {
    pub(super) fn new(world: &mut World, model: &Model) -> Self {
        let mut pack = imported::read(model.pilot());
        let skeleton = Skeleton::new(pack.skeleton);
        let mesh = pack
            .meshes
            .pop()
            .expect("el paquete del piloto tiene su malla")
            .mesh
            .with_generated_skinned_mesh_bounds()
            .expect("la malla del piloto tiene huesos");
        let texture = imported::texture(model.pilot_color(), true);
        let texture = world.resource_mut::<Assets<Image>>().add(texture);
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color_texture: Some(texture),
                perceptual_roughness: 0.55,
                ..default()
            });
        let bindposes = world
            .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
            .add(skeleton.inverse_bindposes());
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        Self {
            mesh,
            material,
            bindposes,
            skeleton,
        }
    }
}

/// En la moto: los huesos de su piloto y su postura suavizada.
#[derive(Component)]
pub(super) struct Pilot {
    /// El diseño de la moto, que dice de qué piloto es el esqueleto.
    design: Design,
    joints: Vec<Entity>,
    hang: f32,
    sit_up: f32,
    tuck: f32,
    surge: f32,
    look: f32,
    foot_down: f32,
}

/// Crea los huesos (hijos de `body`, en el espacio de la moto) y la malla.
pub(super) fn spawn(
    commands: &mut Commands,
    bike: Entity,
    body: Entity,
    blueprint: &PilotBlueprint,
    design: Design,
) {
    let joints: Vec<Entity> = blueprint
        .skeleton
        .joints
        .iter()
        .map(|joint| {
            commands
                .spawn((
                    Transform::from_translation(joint.position).with_rotation(joint.rotation),
                    Visibility::default(),
                    ChildOf(body),
                ))
                .id()
        })
        .collect();
    commands.spawn((
        Mesh3d(blueprint.mesh.clone()),
        MeshMaterial3d(blueprint.material.clone()),
        SkinnedMesh {
            inverse_bindposes: blueprint.bindposes.clone(),
            joints: joints.clone(),
        },
        DynamicSkinnedMeshBounds,
        Transform::default(),
        ChildOf(body),
    ));
    commands.entity(bike).insert(Pilot {
        design,
        joints,
        hang: 0.0,
        sit_up: 0.0,
        tuck: 0.0,
        surge: 0.0,
        look: 0.0,
        foot_down: 1.0,
    });
}

/// Lo que decide la postura en un instante, en el espacio de la moto.
#[derive(Clone, Copy, Debug)]
pub(super) struct Stance {
    pub seat: Vec3,
    /// Puños derecho e izquierdo, ya girados con la dirección.
    pub grips: [Vec3; 2],
    /// Tobillos derecho e izquierdo, sobre las estriberas.
    pub pegs: [Vec3; 2],
    /// Descolgarse: de -1 (a la derecha) a 1 (a la izquierda).
    pub hang: f32,
    /// Incorporarse, de 0 a 1.
    pub sit_up: f32,
    /// Agacharse sobre el depósito, de 0 a 1.
    pub tuck: f32,
    /// Desplazamiento de la cadera por la inercia (+Z, hacia atrás).
    pub surge: f32,
    /// Caballito (rad): el piloto se echa sobre el manillar.
    pub wheelie: f32,
    /// Giro de la cabeza (rad), positivo hacia la izquierda.
    pub look: f32,
    /// Inclinación de la moto (rad), positiva hacia la izquierda.
    pub lean: f32,
    /// Bajar el pie al suelo en parado, de 0 a 1.
    pub foot_down: f32,
    /// Off the bike, from 0 to 1: the back straightens up over the hips, so
    /// the body lies flat once laid on its back.
    pub down: f32,
}

/// Postura de cada hueso (posición y giro en el espacio de la moto).
pub(super) fn pose(skeleton: &Skeleton, stance: &Stance) -> Vec<Transform> {
    let joints = &skeleton.joints;
    let bones = &skeleton.bones;
    let fingers = [1.0, -1.0].map(|side| (FINGERS * Vec3::new(side, 1.0, 1.0)).normalize());
    // La palma va sobre el puño y la muñeca, detrás de ella. Off the bike
    // there is no grip: the wrist goes where the hand reaches for.
    let wrists = [0, 1].map(|side| {
        let gripping = palm_target(stance.grips[side]) - fingers[side] * PALM_FROM_WRIST;
        gripping.lerp(stance.grips[side], stance.down)
    });

    // La espalda se dobla lo que pide la postura y, si así los brazos no
    // llegan a los puños (al incorporarse o al descolgarse), lo justo más.
    let wanted = ((1.0 - SIT_UP * stance.sit_up)
        + TUCK * stance.tuck * (1.0 - stance.sit_up)
        + 0.6 * stance.wheelie.max(0.0))
        * (1.0 - stance.down);
    let reaches = |bend: f32| {
        let (position, _) = torso(skeleton, stance, bend);
        bones
            .arms
            .iter()
            .zip(wrists)
            .all(|(&[arm, forearm, hand], wrist)| {
                let length = skeleton.offsets[forearm].length() + skeleton.offsets[hand].length();
                position[arm].distance(wrist) <= length - REACH_MARGIN
            })
    };
    // Off the bike the hands do not need to reach the grips.
    let bend = if stance.down > 0.0 || reaches(wanted) {
        wanted
    } else {
        let (mut short, mut enough) = (wanted, MAX_BEND);
        for _ in 0..12 {
            let middle = 0.5 * (short + enough);
            if reaches(middle) {
                enough = middle;
            } else {
                short = middle;
            }
        }
        enough
    };
    let (mut position, mut rotation) = torso(skeleton, stance, bend);
    turn_head(skeleton, stance, bend, &mut position, &mut rotation);

    // Brazos a los puños y piernas a las estriberas.
    for (side_index, side) in [1.0f32, -1.0].into_iter().enumerate() {
        let [arm, forearm, hand] = bones.arms[side_index];
        let pole = Vec3::new(side, -0.7, 0.5);
        reach(
            skeleton,
            &mut position,
            &mut rotation,
            [arm, forearm, hand],
            wrists[side_index],
            pole,
        );
        let gripping = grip_rotation(&joints[hand], fingers[side_index]);
        rotation[hand] = if stance.down > 0.0 {
            // Off the bike the hand hangs from the forearm, relaxed: bent a
            // little at the wrist towards the body, the palm facing it.
            let inward = Vec3::X * -side;
            let forearm_along =
                (position[hand] - position[forearm]).normalize_or(fingers[side_index]);
            let along = (forearm_along + inward * RELAXED_WRIST).normalize_or(forearm_along);
            let relaxed = hand_rotation(&joints[hand], along, inward);
            gripping.slerp(relaxed, stance.down)
        } else {
            gripping
        };

        let [thigh, shin, foot, toe] = bones.legs[side_index];
        let hip = position[thigh];
        let ankle = stance.pegs[side_index];
        // Rodilla hacia delante y algo hacia fuera; la de dentro de la curva, al suelo.
        let inside = (-stance.hang * side).max(0.0);
        let axis = (ankle - hip).normalize();
        let forward = Vec3::new(side * 0.35, 0.0, -1.0).reject_from_normalized(axis);
        let pole = Quat::from_axis_angle(axis, KNEE_OUT * inside * side) * forward;
        reach(
            skeleton,
            &mut position,
            &mut rotation,
            [thigh, shin, foot],
            ankle,
            pole,
        );
        position[toe] = position[foot] + rotation[foot] * skeleton.offsets[toe];
        rotation[toe] = rotation[foot] * skeleton.local_rotations[toe];
    }

    position
        .into_iter()
        .zip(rotation)
        .map(|(t, r)| Transform::from_translation(t).with_rotation(r))
        .collect()
}

/// Cadera, espalda, cabeza y el resto del esqueleto colgando de ellas, con la
/// espalda doblada hacia delante según `bend` (1 es agachado sobre el depósito).
fn torso(skeleton: &Skeleton, stance: &Stance, bend: f32) -> (Vec<Vec3>, Vec<Quat>) {
    let joints = &skeleton.joints;
    let bones = &skeleton.bones;
    let mut position = vec![Vec3::ZERO; joints.len()];
    let mut rotation = vec![Quat::IDENTITY; joints.len()];
    let pitch = |angle: f32| Quat::from_rotation_x(-angle);
    let hip_shift = Vec3::new(
        -HANG_OFF * stance.hang + 0.05 * stance.foot_down,
        -0.06 * stance.foot_down,
        stance.surge,
    );
    position[bones.hips] = stance.seat + hip_shift;
    rotation[bones.hips] = Quat::from_rotation_z(HANG_OFF_LEAN * 0.5 * stance.hang)
        * pitch(PELVIS_PITCH * bend)
        * joints[bones.hips].rotation;

    // Cinemática directa desde la cadera, doblando la espalda y levantando la
    // cabeza: cada giro se aplica en el hueso y arrastra a sus hijos.
    for index in 0..joints.len() {
        let Some(parent) = joints[index].parent else {
            continue;
        };
        position[index] = position[parent] + rotation[parent] * skeleton.offsets[index];
        let mut world = rotation[parent] * skeleton.local_rotations[index];
        if let Some(level) = bones.spine.iter().position(|&s| s == index) {
            world = pitch(SPINE_BEND[level] * bend) * world;
            if level == 2 {
                world = Quat::from_rotation_z(HANG_OFF_LEAN * 0.5 * stance.hang) * world;
            }
            if level > 0 {
                // Torsión sobre la propia espalda (el eje Y de los huesos).
                world *= Quat::from_rotation_y(0.5 * HANG_OFF_TWIST * stance.hang);
            }
        } else if index == bones.neck {
            world = pitch(-NECK_LIFT * bend) * world;
        } else if index == bones.head {
            world = pitch(-HEAD_LIFT * bend) * world;
        }
        rotation[index] = world;
    }
    (position, rotation)
}

/// Coloca la cabeza a partir de cómo iría con el tronco sin descolgar: la
/// endereza respecto a la moto y la gira alrededor de la coronilla, como al
/// mirar a un lado, sin ladearla. El cuello queda a medias entre el tronco y
/// la cabeza.
fn turn_head(
    skeleton: &Skeleton,
    stance: &Stance,
    bend: f32,
    position: &mut [Vec3],
    rotation: &mut [Quat],
) {
    let bones = &skeleton.bones;
    let (_, upright) = torso(
        skeleton,
        &Stance {
            hang: 0.0,
            ..*stance
        },
        bend,
    );
    let level = Quat::from_rotation_z(-super::HEAD_LEVEL * stance.lean);
    // La coronilla es lo que en reposo (de pie) apunta hacia arriba. Girar
    // en positivo alrededor de ella lleva la vista hacia la izquierda.
    let rest = skeleton.joints[bones.head].rotation;
    let crown = upright[bones.head] * (rest.inverse() * Vec3::Y);
    let turn = |share: f32| level * Quat::from_axis_angle(crown, share * stance.look);
    let neck = turn(NECK_TURN) * upright[bones.neck];
    rotation[bones.neck] = rotation[bones.neck].slerp(neck, NECK_FOLLOW);
    rotation[bones.head] = turn(1.0) * upright[bones.head];

    // La cabeza y lo que cuelga de ella siguen al cuello.
    let mut moved = vec![false; skeleton.joints.len()];
    for (index, joint) in skeleton.joints.iter().enumerate() {
        let Some(parent) = joint.parent else {
            continue;
        };
        if index == bones.head || moved[parent] {
            moved[index] = true;
            position[index] = position[parent] + rotation[parent] * skeleton.offsets[index];
            if index != bones.head {
                rotation[index] = rotation[parent] * skeleton.local_rotations[index];
            }
        }
    }
}

/// Centro de la palma al agarrar un puño.
fn palm_target(grip: Vec3) -> Vec3 {
    grip + Vec3::Y * PALM_ABOVE_GRIP
}

/// Giro de la mano para que los dedos apunten hacia `fingers` con la palma
/// hacia abajo, sobre el puño. En reposo (los brazos en cruz) la mano apunta
/// por su eje Y, como todos los huesos de Mixamo, con la palma hacia el suelo.
fn grip_rotation(rest: &Joint, fingers: Vec3) -> Quat {
    hand_rotation(rest, fingers, Vec3::NEG_Y)
}

/// Giro de una mano con los dedos hacia `fingers` y la palma, en lo posible,
/// hacia `palm`.
fn hand_rotation(rest: &Joint, fingers: Vec3, palm: Vec3) -> Quat {
    let local_palm = rest.rotation.inverse() * Vec3::NEG_Y;
    let frame = |along: Vec3, palm: Vec3| {
        let palm = palm
            .reject_from_normalized(along)
            .try_normalize()
            .unwrap_or_else(|| along.any_orthonormal_vector());
        Mat3::from_cols(along, palm, along.cross(palm))
    };
    let local = frame(Vec3::Y, local_palm);
    let world = frame(fingers, palm);
    Quat::from_mat3(&(world * local.transpose())).normalize()
}

/// Cinemática inversa de dos tramos: gira `upper` y `lower` para que el
/// extremo `end` llegue a `target`, con el codo (o la rodilla) hacia `pole`.
/// Cada tramo gira lo mínimo desde su postura anterior, así no se retuerce.
fn reach(
    skeleton: &Skeleton,
    position: &mut [Vec3],
    rotation: &mut [Quat],
    [upper, lower, end]: [usize; 3],
    target: Vec3,
    pole: Vec3,
) {
    let root = position[upper];
    let (upper_length, lower_length) = (
        skeleton.offsets[lower].length(),
        skeleton.offsets[end].length(),
    );
    let joint = bend_point(root, target, upper_length, lower_length, pole);

    let current = rotation[upper] * skeleton.offsets[lower];
    rotation[upper] =
        Quat::from_rotation_arc(current.normalize(), (joint - root).normalize()) * rotation[upper];
    position[lower] = root + rotation[upper] * skeleton.offsets[lower];
    rotation[lower] = rotation[upper] * skeleton.local_rotations[lower];

    let current = rotation[lower] * skeleton.offsets[end];
    rotation[lower] =
        Quat::from_rotation_arc(current.normalize(), (target - position[lower]).normalize())
            * rotation[lower];
    position[end] = position[lower] + rotation[lower] * skeleton.offsets[end];
    rotation[end] = rotation[lower] * skeleton.local_rotations[end];
}

/// Punto de doblez (codo o rodilla) de dos tramos que van de `root` a `target`,
/// doblados hacia `pole`. Si no llega, se estira hacia el objetivo.
fn bend_point(root: Vec3, target: Vec3, upper: f32, lower: f32, pole: Vec3) -> Vec3 {
    let to_target = target - root;
    let direction = to_target.normalize();
    let distance = to_target.length().clamp(1e-4, upper + lower - 1e-4);
    let along = (upper * upper - lower * lower + distance * distance) / (2.0 * distance);
    let out = (upper * upper - along * along).max(0.0).sqrt();
    let bend = pole.reject_from_normalized(direction).normalize_or_zero();
    root + direction * along + bend * out
}

/// Coloca cada fotograma los huesos de los pilotos.
#[allow(clippy::type_complexity)]
pub(super) fn pose_pilots(
    time: Res<Time>,
    tuning: Res<BikeTuning>,
    blueprint: Res<super::BikeBlueprint>,
    mut bikes: Query<(
        &BikeState,
        &BikeInput,
        &Geometry,
        &ChassisRig,
        &mut Pilot,
        Option<&super::super::CrashState>,
    )>,
    mut transforms: Query<&mut Transform>,
) {
    let dt = time.delta_secs();
    for (state, input, geometry, rig, mut pilot, crash) in &mut bikes {
        let Some(pilot_blueprint) = &blueprint.bodies[pilot.design.index()].pilot else {
            continue;
        };
        let is_crashing = crash.is_some_and(|c| c.is_down());
        let hang = if is_crashing {
            0.0
        } else {
            (state.lean / tuning.max_lean).clamp(-1.0, 1.0)
        };
        pilot.hang = approach(pilot.hang, hang, POSE_RESPONSE, dt);
        let slow = 1.0 - (state.speed.abs() / CRUISING_SPEED).min(1.0);
        // Down, the torso straightens: laid on the back it lies flat.
        let sit_up_target = if is_crashing {
            1.0
        } else {
            input.brake.max(slow)
        };
        pilot.sit_up = approach(pilot.sit_up, sit_up_target, POSE_RESPONSE, dt);
        let tuck = if is_crashing {
            0.0
        } else {
            super::tuck_amount(state, &tuning)
        };
        pilot.tuck = approach(pilot.tuck, tuck, POSE_RESPONSE, dt);
        let surge = if is_crashing {
            0.0
        } else {
            (state.accel * SURGE_PER_ACCEL).clamp(SURGE.0, SURGE.1)
        };
        pilot.surge = approach(pilot.surge, surge, POSE_RESPONSE * 1.5, dt);
        let look = if is_crashing {
            0.0
        } else {
            super::look_angle(state, &tuning)
        };
        pilot.look = approach(pilot.look, look, POSE_RESPONSE, dt);
        let stopped = if state.grounded && !is_crashing {
            (1.0 - state.speed.abs() / crate::bike::STOPPED_SPEED).clamp(0.0, 1.0)
        } else {
            0.0
        };
        pilot.foot_down = approach(pilot.foot_down, stopped, POSE_RESPONSE, dt);

        let steering = transforms
            .get(rig.steering)
            .map_or(Transform::IDENTITY, |t| *t);
        let grip = |side: f32| {
            let mirror = Vec3::new(side, 1.0, 1.0);
            steering.transform_point((geometry.grip - geometry.steering_head) * mirror)
        };
        let peg = |side: f32| geometry.peg * Vec3::new(side, 1.0, 1.0);
        let foot_ground = Vec3::new(0.38, 0.19, geometry.seat.z);
        let right_peg = peg(1.0).lerp(foot_ground, pilot.foot_down);

        let (grips, pegs) = if let Some(c) = crash.filter(|c| c.is_down()) {
            // Off the bike, hands and feet move as the fall goes (see
            // `ThrownRider::limbs`).
            let let_go = c.let_go();
            let limbs = c.rider.limbs;
            let off = |offset: Vec3| geometry.seat + offset;
            (
                [
                    grip(1.0).lerp(off(limbs.hands[0]), let_go),
                    grip(-1.0).lerp(off(limbs.hands[1]), let_go),
                ],
                [
                    right_peg.lerp(off(limbs.feet[0]), let_go),
                    peg(-1.0).lerp(off(limbs.feet[1]), let_go),
                ],
            )
        } else {
            ([grip(1.0), grip(-1.0)], [right_peg, peg(-1.0)])
        };

        let stance = Stance {
            seat: geometry.seat,
            grips,
            pegs,
            hang: pilot.hang,
            sit_up: pilot.sit_up,
            tuck: pilot.tuck,
            surge: pilot.surge,
            wheelie: if is_crashing {
                0.0
            } else {
                state.chassis.wheelie
            },
            look: pilot.look,
            lean: if is_crashing { 0.0 } else { state.lean },
            foot_down: pilot.foot_down,
            down: crash.map_or(0.0, |c| c.let_go()),
        };
        for (entity, transform) in pilot
            .joints
            .iter()
            .zip(pose(&pilot_blueprint.skeleton, &stance))
        {
            if let Ok(mut joint) = transforms.get_mut(*entity) {
                *joint = transform;
            }
        }
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::super::MAX_LOOK;
    use super::*;

    /// The roster's models (the first, the measures' reference).
    pub fn models(roster: &'static crate::bike::Roster) -> Vec<&'static Model> {
        super::super::designs::every(roster)
            .into_iter()
            .filter_map(Design::model)
            .collect()
    }

    fn skeleton(roster: &'static crate::bike::Roster) -> Skeleton {
        skeleton_of(models(roster)[0])
    }

    fn skeleton_of(model: &Model) -> Skeleton {
        Skeleton::new(imported::read(model.pilot()).skeleton)
    }

    fn stance(roster: &'static crate::bike::Roster) -> Stance {
        stance_on(models(roster)[0])
    }

    fn stance_on(model: &Model) -> Stance {
        let geometry = model.geometry;
        let mirror = |p: Vec3, side: f32| p * Vec3::new(side, 1.0, 1.0);
        Stance {
            seat: geometry.seat,
            grips: [mirror(geometry.grip, 1.0), mirror(geometry.grip, -1.0)],
            pegs: [mirror(geometry.peg, 1.0), mirror(geometry.peg, -1.0)],
            hang: 0.0,
            sit_up: 0.0,
            tuck: 0.0,
            surge: 0.0,
            wheelie: 0.0,
            look: 0.0,
            lean: 0.0,
            foot_down: 0.0,
            down: 0.0,
        }
    }

    pub fn hands_reach_the_grips_and_feet_the_pegs_on_every_bike_on(
        roster: &'static crate::bike::Roster,
    ) {
        for model in models(roster) {
            let skeleton = skeleton_of(model);
            for (hang, look, sit_up, tuck) in [
                (0.0, 0.0, 0.0, 0.0),
                (0.0, 0.0, 0.0, 1.0),
                (0.0, 0.0, 1.0, 0.0),
                (1.0, MAX_LOOK, 0.0, 0.0),
                (-1.0, -MAX_LOOK, 0.0, 0.0),
            ] {
                let stance = Stance {
                    hang,
                    look,
                    sit_up,
                    tuck,
                    ..stance_on(model)
                };
                hands_and_feet_reach(&skeleton, &stance);
            }
        }
    }

    #[test]
    fn hands_reach_the_grips_and_feet_the_pegs_on_every_bike() {
        hands_reach_the_grips_and_feet_the_pegs_on_every_bike_on(&crate::testing::DEMO_ROSTER);
    }

    pub fn right_foot_reaches_the_ground_when_stopped_on(roster: &'static crate::bike::Roster) {
        for model in models(roster) {
            let skeleton = skeleton_of(model);
            let geometry = model.geometry;
            let foot_ground = Vec3::new(0.38, 0.19, geometry.seat.z);
            let mut stance = stance_on(model);
            stance.lean = crate::bike::STOPPED_LEAN;
            stance.foot_down = 1.0;
            stance.pegs[0] = foot_ground;
            let posed = pose(&skeleton, &stance);
            let right_foot = posed[skeleton.bones.legs[0][2]].translation;
            assert!(
                right_foot.distance(foot_ground) < 0.01,
                "pie derecho en el suelo a {}",
                right_foot.distance(foot_ground)
            );
        }
    }

    #[test]
    fn right_foot_reaches_the_ground_when_stopped() {
        right_foot_reaches_the_ground_when_stopped_on(&crate::testing::DEMO_ROSTER);
    }

    fn hands_and_feet_reach(skeleton: &Skeleton, stance: &Stance) {
        let posed = pose(skeleton, stance);
        for side in 0..2 {
            // La palma está sobre el puño.
            let hand = posed[skeleton.bones.arms[side][2]];
            let palm = hand.translation + hand.rotation * Vec3::Y * PALM_FROM_WRIST;
            let target = palm_target(stance.grips[side]);
            assert!(
                palm.distance(target) < 0.01,
                "palma a {}",
                palm.distance(target)
            );
            // Con los dedos hacia delante y la palma hacia abajo.
            assert!((hand.rotation * Vec3::Y).z < -0.7);
            let palm_normal = hand.rotation
                * (skeleton.joints[skeleton.bones.arms[side][2]]
                    .rotation
                    .inverse()
                    * Vec3::NEG_Y);
            assert!(palm_normal.y < -0.6, "palma hacia abajo: {palm_normal}");
            let foot = posed[skeleton.bones.legs[side][2]].translation;
            assert!(
                foot.distance(stance.pegs[side]) < 0.01,
                "pie a {}",
                foot.distance(stance.pegs[side])
            );
        }
    }

    pub fn bones_keep_their_lengths_on(roster: &'static crate::bike::Roster) {
        let skeleton = skeleton(roster);
        let mut stance = stance(roster);
        stance.hang = 1.0;
        let posed = pose(&skeleton, &stance);
        for (index, joint) in skeleton.joints.iter().enumerate() {
            if let Some(parent) = joint.parent {
                let length = posed[index].translation.distance(posed[parent].translation);
                assert!(
                    (length - skeleton.offsets[index].length()).abs() < 1e-3,
                    "{}",
                    joint.name
                );
            }
        }
    }

    pub fn tucked_pilot_lies_over_the_tank_and_looks_ahead_on(
        roster: &'static crate::bike::Roster,
    ) {
        let skeleton = skeleton(roster);
        let posed = pose(&skeleton, &stance(roster));
        let bones = &skeleton.bones;
        let (hips, chest) = (
            posed[bones.hips].translation,
            posed[bones.spine[2]].translation,
        );
        let back = (chest - hips).normalize();
        assert!(back.z < -0.6, "tumbado hacia delante: {back}");
        assert!(chest.y > hips.y, "con los hombros por encima de la cadera");
        // La cabeza mira al frente, no al suelo.
        let head = posed[bones.head].rotation;
        let face = head * (skeleton.joints[bones.head].rotation.inverse() * Vec3::NEG_Z);
        assert!(face.z < -0.5 && face.y > -0.6, "mira al frente: {face}");
    }

    pub fn head_turns_towards_the_corner_without_tilting_on(roster: &'static crate::bike::Roster) {
        let skeleton = skeleton(roster);
        let bones = &skeleton.bones;
        let rest = skeleton.joints[bones.head].rotation.inverse();
        // Hacia dónde mira la cara y hacia dónde apunta la coronilla.
        let head = |look: f32, lean: f32| {
            let stance = Stance {
                look,
                lean,
                hang: lean / 0.96,
                ..stance(roster)
            };
            let rotation = pose(&skeleton, &stance)[bones.head].rotation;
            (rotation * (rest * Vec3::NEG_Z), rotation * (rest * Vec3::Y))
        };
        let (ahead, up) = head(0.0, 0.0);
        let (left, turned_up) = head(MAX_LOOK, 0.0);
        assert!(left.x < ahead.x - 0.3, "mira a la izquierda: {left}");
        assert!((left.y - ahead.y).abs() < 0.15, "sin cabecear: {left}");
        assert!(turned_up.distance(up) < 0.02, "sin ladearse: {turned_up}");
        assert!(head(-MAX_LOOK, 0.0).0.x > ahead.x + 0.3);
        // Con la moto tumbada a la izquierda, la cabeza se tumba menos que ella:
        // en la moto, la coronilla se va hacia la derecha.
        let lean = 0.9;
        let (_, leaning_up) = head(0.0, lean);
        let world_up = Quat::from_rotation_z(lean) * leaning_up;
        let tilt = world_up.x.atan2(world_up.y).abs();
        assert!(tilt < 0.7 * lean, "inclinación de la cabeza: {tilt}");
    }

    pub fn on_the_straight_the_pilot_tucks_in_behind_the_screen_on(
        roster: &'static crate::bike::Roster,
    ) {
        let skeleton = skeleton(roster);
        let bones = &skeleton.bones;
        let head = |tuck| {
            pose(
                &skeleton,
                &Stance {
                    tuck,
                    ..stance(roster)
                },
            )[bones.head]
        };
        let (riding, tucked) = (head(0.0), head(1.0));
        assert!(
            tucked.translation.y < riding.translation.y - 0.04,
            "agacha la cabeza: {} y {}",
            riding.translation.y,
            tucked.translation.y
        );
        // Y sigue mirando al frente.
        let rest = skeleton.joints[bones.head].rotation.inverse();
        let face = tucked.rotation * (rest * Vec3::NEG_Z);
        assert!(face.z < -0.7 && face.y > -0.5, "mira al frente: {face}");
    }

    pub fn inner_knee_goes_out_in_a_corner_on(roster: &'static crate::bike::Roster) {
        let skeleton = skeleton(roster);
        let mut stance = stance(roster);
        let straight = pose(&skeleton, &stance);
        stance.hang = 1.0;
        let leaning = pose(&skeleton, &stance);
        // Tumbado a la izquierda: la rodilla izquierda (-X) sale hacia fuera.
        let knee = skeleton.bones.legs[1][1];
        assert!(leaning[knee].translation.x < straight[knee].translation.x - 0.08);
    }
}
