//! Mandos en macOS con GameController, el framework de Apple: el que mejor
//! conoce los mandos de PlayStation (DualSense, DualShock 4), Xbox, Switch
//! Pro y MFi. macOS pone el DualSense en un modo en el que gilrs lo ve
//! conectado pero no le llega ni un botón; con GameController sí.
//!
//! Cada fotograma se mira qué mandos hay y cómo están sus botones y sus
//! sticks, y lo que cambia se lleva a Bevy como si viniera de gilrs. Los
//! mandos que lee GameController no los cuenta gilrs ([`ClaimedPads`]).

use core::ffi::c_float;

use bevy::input::gamepad::{
    GamepadAxis, GamepadButton, GamepadConnection, GamepadConnectionEvent,
    RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::prelude::*;
use objc2::rc::Retained;
use objc2_game_controller::{
    GCController, GCControllerButtonInput, GCControllerDirectionPad, GCDevice,
    GCDualSenseAdaptiveTrigger, GCDualSenseAdaptiveTriggerPositionalAmplitudes, GCDualSenseGamepad,
    GCExtendedGamepad,
};

use super::{ActiveController, ClaimedPads, VIBRATION_START, vibration_zones};
use crate::bike::{BikeInput, BikeState, CollisionImpact, LocalPlayer, Player};
use crate::lobby::{ControllerBinding, LocalPlayersConfig};
use crate::menu::GameState;
use crate::pause::Pause;
use crate::welcome::GameMode;

/// Los botones, en el orden en que se leen: con los nombres de Bevy, que son
/// los de su sitio en el mando (A, abajo; B, a la derecha...).
const BUTTONS: [GamepadButton; 17] = [
    GamepadButton::South,
    GamepadButton::East,
    GamepadButton::West,
    GamepadButton::North,
    GamepadButton::LeftTrigger,
    GamepadButton::RightTrigger,
    GamepadButton::LeftTrigger2,
    GamepadButton::RightTrigger2,
    GamepadButton::Start,
    GamepadButton::Select,
    GamepadButton::Mode,
    GamepadButton::LeftThumb,
    GamepadButton::RightThumb,
    GamepadButton::DPadUp,
    GamepadButton::DPadDown,
    GamepadButton::DPadLeft,
    GamepadButton::DPadRight,
];
const AXES: [GamepadAxis; 4] = [
    GamepadAxis::LeftStickX,
    GamepadAxis::LeftStickY,
    GamepadAxis::RightStickX,
    GamepadAxis::RightStickY,
];
/// Cambios más pequeños que esto no se mandan.
const CHANGE: f32 = 1e-3;

/// Target effect configuration for a DualSense adaptive trigger.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub enum TriggerEffect {
    /// Effect is turned off (normal free trigger travel).
    Off,
    /// Constant resistance from `start_position` onwards.
    Feedback {
        start_position: f32,
        resistive_strength: f32,
    },
    /// Linear slope resistance between start and end positions.
    SlopeFeedback {
        start_position: f32,
        end_position: f32,
        start_strength: f32,
        end_strength: f32,
    },
    /// Haptic vibration directly inside the trigger motor.
    Vibration {
        start_position: f32,
        amplitude: f32,
        frequency: f32,
    },
    /// Weapon-style resistance with a break point.
    #[allow(dead_code)]
    Weapon {
        start_position: f32,
        end_position: f32,
        resistive_strength: f32,
    },
}

impl TriggerEffect {
    /// Checks whether two effects are close enough in values to skip re-sending redundant HID packets.
    pub fn is_substantially_equal(&self, other: &Self) -> bool {
        match (self, other) {
            (TriggerEffect::Off, TriggerEffect::Off) => true,
            (
                TriggerEffect::Feedback {
                    start_position: s1,
                    resistive_strength: r1,
                },
                TriggerEffect::Feedback {
                    start_position: s2,
                    resistive_strength: r2,
                },
            ) => (s1 - s2).abs() < 0.04 && (r1 - r2).abs() < 0.04,
            (
                TriggerEffect::SlopeFeedback {
                    start_position: s1,
                    end_position: e1,
                    start_strength: st1,
                    end_strength: et1,
                },
                TriggerEffect::SlopeFeedback {
                    start_position: s2,
                    end_position: e2,
                    start_strength: st2,
                    end_strength: et2,
                },
            ) => {
                (s1 - s2).abs() < 0.04
                    && (e1 - e2).abs() < 0.04
                    && (st1 - st2).abs() < 0.04
                    && (et1 - et2).abs() < 0.04
            }
            (
                TriggerEffect::Vibration {
                    start_position: s1,
                    amplitude: a1,
                    frequency: f1,
                },
                TriggerEffect::Vibration {
                    start_position: s2,
                    amplitude: a2,
                    frequency: f2,
                },
            ) => (s1 - s2).abs() < 0.04 && (a1 - a2).abs() < 0.04 && (f1 - f2).abs() < 0.04,
            (
                TriggerEffect::Weapon {
                    start_position: s1,
                    end_position: e1,
                    resistive_strength: r1,
                },
                TriggerEffect::Weapon {
                    start_position: s2,
                    end_position: e2,
                    resistive_strength: r2,
                },
            ) => (s1 - s2).abs() < 0.04 && (e1 - e2).abs() < 0.04 && (r1 - r2).abs() < 0.04,
            _ => false,
        }
    }
}

/// Adaptive trigger handles for a connected DualSense controller.
pub struct DualSenseTriggers {
    pub left: Retained<GCDualSenseAdaptiveTrigger>,
    pub right: Retained<GCDualSenseAdaptiveTrigger>,
    pub last_left: TriggerEffect,
    pub last_right: TriggerEffect,
}

impl DualSenseTriggers {
    pub fn update(&mut self, left_target: TriggerEffect, right_target: TriggerEffect) {
        if !self.last_left.is_substantially_equal(&left_target) {
            self.last_left = left_target;
            unsafe { apply_effect(&self.left, left_target) };
        }
        if !self.last_right.is_substantially_equal(&right_target) {
            self.last_right = right_target;
            unsafe { apply_effect(&self.right, right_target) };
        }
    }

    pub fn turn_off(&mut self) {
        self.update(TriggerEffect::Off, TriggerEffect::Off);
    }
}

/// Applies an effect to a DualSense adaptive trigger.
unsafe fn apply_effect(trigger: &GCDualSenseAdaptiveTrigger, effect: TriggerEffect) {
    match effect {
        TriggerEffect::Off => {
            unsafe { trigger.setModeOff() };
        }
        TriggerEffect::Feedback {
            start_position,
            resistive_strength,
        } => unsafe {
            trigger.setModeFeedbackWithStartPosition_resistiveStrength(
                start_position.clamp(0.0, 1.0) as c_float,
                resistive_strength.clamp(0.0, 1.0) as c_float,
            );
        },
        TriggerEffect::SlopeFeedback {
            start_position,
            end_position,
            start_strength,
            end_strength,
        } => {
            let start = start_position.clamp(0.0, 1.0);
            let end = end_position.clamp(start, 1.0);
            unsafe {
                trigger
                    .setModeSlopeFeedbackWithStartPosition_endPosition_startStrength_endStrength(
                        start as c_float,
                        end as c_float,
                        start_strength.clamp(0.0, 1.0) as c_float,
                        end_strength.clamp(0.0, 1.0) as c_float,
                    );
            }
        }
        TriggerEffect::Vibration {
            start_position,
            amplitude,
            frequency,
        } => unsafe {
            // Only from `start_position` on: vibrating at rest, the trigger
            // would read as pressed (see `vibration_zones`).
            let amp = amplitude.clamp(0.0, 1.0) as c_float;
            let freq = frequency.clamp(0.0, 1.0) as c_float;
            let amplitudes = GCDualSenseAdaptiveTriggerPositionalAmplitudes {
                values: vibration_zones(start_position, amp),
            };
            trigger.setModeVibrationWithAmplitudes_frequency(amplitudes, freq);
        },
        TriggerEffect::Weapon {
            start_position,
            end_position,
            resistive_strength,
        } => {
            let start = start_position.clamp(0.0, 1.0);
            let end = end_position.clamp(start, 1.0);
            unsafe {
                trigger.setModeWeaponWithStartPosition_endPosition_resistiveStrength(
                    start as c_float,
                    end as c_float,
                    resistive_strength.clamp(0.0, 1.0) as c_float,
                );
            }
        }
    }
}

/// Los mandos que lee GameController. Solo se puede tocar desde el hilo
/// principal, así que va como recurso no `Send`.
#[derive(Default)]
pub struct ApplePads {
    pads: Vec<Pad>,
}

impl Drop for ApplePads {
    fn drop(&mut self) {
        for pad in &mut self.pads {
            if let Some(ds) = &mut pad.dualsense {
                ds.turn_off();
            }
        }
    }
}

struct Pad {
    controller: Retained<GCController>,
    entity: Entity,
    name: String,
    buttons: [f32; BUTTONS.len()],
    axes: [f32; AXES.len()],
    dualsense: Option<DualSenseTriggers>,
}

/// Los mensajes de los mandos, como los manda `bevy::gilrs`.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PadMessages<'w> {
    events: MessageWriter<'w, RawGamepadEvent>,
    connections: MessageWriter<'w, GamepadConnectionEvent>,
    buttons: MessageWriter<'w, RawGamepadButtonChangedEvent>,
    axes: MessageWriter<'w, RawGamepadAxisChangedEvent>,
}

impl PadMessages<'_> {
    fn connection(&mut self, event: GamepadConnectionEvent) {
        self.events.write(event.clone().into());
        self.connections.write(event);
    }
}

/// Mira qué mandos hay y lleva a Bevy lo que ha cambiado en cada uno.
pub fn read_apple_pads(
    mut apple: NonSendMut<ApplePads>,
    mut names: ResMut<ClaimedPads>,
    mut commands: Commands,
    mut messages: PadMessages,
) {
    // SAFETY: GameController se usa desde el hilo principal (el recurso no
    // es `Send`, así que Bevy corre este sistema en él).
    let controllers = unsafe { GCController::controllers() };
    let same = |a: &Retained<GCController>, b: &Retained<GCController>| {
        Retained::as_ptr(a) == Retained::as_ptr(b)
    };

    // Los que se han desconectado.
    apple.pads.retain(|pad| {
        let still = controllers.iter().any(|c| same(&c, &pad.controller));
        if !still {
            info!("Mando desconectado: {}", pad.name);
            if let Some(ds) = &pad.dualsense {
                unsafe {
                    ds.left.setModeOff();
                    ds.right.setModeOff();
                }
            }
            messages.connection(GamepadConnectionEvent::new(
                pad.entity,
                GamepadConnection::Disconnected,
            ));
            commands.entity(pad.entity).despawn();
        }
        still
    });

    // Los nuevos, si son mandos completos (con dos sticks y gatillos).
    for controller in controllers.iter() {
        if apple
            .pads
            .iter()
            .any(|pad| same(&pad.controller, &controller))
        {
            continue;
        }
        // SAFETY: como arriba.
        let Some(extended) = (unsafe { controller.extendedGamepad() }) else {
            continue;
        };
        let dualsense = extended.downcast::<GCDualSenseGamepad>().ok().map(|ds| {
            info!("DualSense detectado con gatillos adaptativos");
            unsafe {
                DualSenseTriggers {
                    left: ds.leftTrigger(),
                    right: ds.rightTrigger(),
                    last_left: TriggerEffect::Off,
                    last_right: TriggerEffect::Off,
                }
            }
        });
        let name = unsafe { controller.vendorName() }
            .map_or_else(|| "Mando".to_string(), |name| name.to_string());
        info!("Mando: {name} (leído con GameController de macOS)");
        let entity = commands.spawn_empty().id();
        messages.connection(GamepadConnectionEvent::new(
            entity,
            GamepadConnection::Connected {
                name: name.clone(),
                vendor_id: None,
                product_id: None,
            },
        ));
        apple.pads.push(Pad {
            controller,
            entity,
            name,
            buttons: [0.0; BUTTONS.len()],
            axes: [0.0; AXES.len()],
            dualsense,
        });
    }

    // Lo que ha cambiado en cada uno.
    for pad in &mut apple.pads {
        // SAFETY: como arriba.
        let Some(gamepad) = (unsafe { pad.controller.extendedGamepad() }) else {
            continue;
        };
        for ((&button, last), now) in BUTTONS.iter().zip(&mut pad.buttons).zip(buttons(&gamepad)) {
            if (now - *last).abs() > CHANGE {
                *last = now;
                let event = RawGamepadButtonChangedEvent::new(pad.entity, button, now);
                messages.events.write(event.into());
                messages.buttons.write(event);
            }
        }
        for ((&axis, last), now) in AXES.iter().zip(&mut pad.axes).zip(axes(&gamepad)) {
            if (now - *last).abs() > CHANGE {
                *last = now;
                let event = RawGamepadAxisChangedEvent::new(pad.entity, axis, now);
                messages.events.write(event.into());
                messages.axes.write(event);
            }
        }
    }

    let current: Vec<(Entity, String)> = apple
        .pads
        .iter()
        .map(|pad| (pad.entity, pad.name.clone()))
        .collect();
    if names.0 != current {
        names.0 = current;
    }
}

/// Cómo están los botones, en el orden de [`BUTTONS`] (de 0 a 1; los
/// gatillos, según lo que se aprieten).
fn buttons(gamepad: &GCExtendedGamepad) -> [f32; BUTTONS.len()] {
    let value = |button: Retained<GCControllerButtonInput>| unsafe { button.value() };
    let optional = |button: Option<Retained<GCControllerButtonInput>>| button.map_or(0.0, value);
    // SAFETY: GameController desde el hilo principal.
    unsafe {
        let dpad: Retained<GCControllerDirectionPad> = gamepad.dpad();
        let dpad_val = |b: Retained<GCControllerButtonInput>| {
            if b.isPressed() { 1.0 } else { 0.0 }
        };
        [
            value(gamepad.buttonA()),
            value(gamepad.buttonB()),
            value(gamepad.buttonX()),
            value(gamepad.buttonY()),
            value(gamepad.leftShoulder()),
            value(gamepad.rightShoulder()),
            value(gamepad.leftTrigger()),
            value(gamepad.rightTrigger()),
            value(gamepad.buttonMenu()),
            optional(gamepad.buttonOptions()),
            optional(gamepad.buttonHome()),
            optional(gamepad.leftThumbstickButton()),
            optional(gamepad.rightThumbstickButton()),
            dpad_val(dpad.up()),
            dpad_val(dpad.down()),
            dpad_val(dpad.left()),
            dpad_val(dpad.right()),
        ]
    }
}

/// Cómo están los sticks, en el orden de [`AXES`] (de -1 a 1, arriba
/// positivo, como en Bevy).
fn axes(gamepad: &GCExtendedGamepad) -> [f32; AXES.len()] {
    // SAFETY: GameController desde el hilo principal.
    unsafe {
        let (left, right) = (gamepad.leftThumbstick(), gamepad.rightThumbstick());
        [
            left.xAxis().value(),
            left.yAxis().value(),
            right.xAxis().value(),
            right.yAxis().value(),
        ]
    }
}

/// Computes realistic adaptive trigger sensations based on the bike's physical state, inputs, and collision impact.
/// Both triggers offer gradual resistance (zero at the beginning, subtle resistance at the end of the stroke),
/// with a soft vibration pulse upon colliding with other bikes.
/// Returns (LeftTrigger / Brake, RightTrigger / Throttle).
pub fn compute_bike_trigger_effects(
    state: &BikeState,
    _input: &BikeInput,
    impact: Option<&CollisionImpact>,
) -> (TriggerEffect, TriggerEffect) {
    if !state.grounded {
        // Airborne: free triggers without resistance.
        return (TriggerEffect::Off, TriggerEffect::Off);
    }

    // Soft vibration when colliding with other bikes.
    if let Some(imp) = impact.filter(|i| i.timer > 0.0) {
        let amplitude = (0.35 + 0.35 * imp.intensity).clamp(0.40, 0.75);
        let collision_vibe = TriggerEffect::Vibration {
            start_position: VIBRATION_START,
            amplitude,
            frequency: 0.45,
        };
        return (collision_vibe, collision_vibe);
    }

    // Right trigger (Throttle) and Left trigger (Brake): equalized gradual profile.
    // Free for initial travel, transitioning with a very gentle slope to a soft, light resistance at the end.
    let gradual_profile = TriggerEffect::SlopeFeedback {
        start_position: 0.35,
        end_position: 1.0,
        start_strength: 0.0,
        end_strength: 0.16,
    };

    (gradual_profile, gradual_profile)
}

/// Updates DualSense adaptive triggers based on the current race state.
#[allow(clippy::too_many_arguments)]
pub fn update_dualsense_triggers(
    mut apple: NonSendMut<ApplePads>,
    game_state: Option<Res<State<GameState>>>,
    pause: Option<Res<Pause>>,
    game_mode: Option<Res<GameMode>>,
    local_players: Option<Res<LocalPlayersConfig>>,
    active_controller: Option<Res<ActiveController>>,
    p1_bike: Query<(&BikeState, &BikeInput, Option<&CollisionImpact>), With<Player>>,
    other_bikes: Query<
        (
            &BikeState,
            &BikeInput,
            Option<&CollisionImpact>,
            &LocalPlayer,
        ),
        Without<Player>,
    >,
) {
    let in_race = game_state.is_some_and(|s| *s.get() == GameState::Race);
    let is_paused = pause.is_some_and(|p| p.is_open());

    // If not in race or paused, turn off all trigger effects.
    if !in_race || is_paused {
        for pad in &mut apple.pads {
            if let Some(ds) = &mut pad.dualsense {
                ds.turn_off();
            }
        }
        return;
    }

    let is_split = game_mode.is_some_and(|m| *m == GameMode::SplitScreen);
    let p1_telemetry = p1_bike.iter().next();

    if !is_split {
        // Modo 1 jugador: solo el mando activo recibe efectos en los gatillos.
        let active_pad = active_controller.as_ref().and_then(|ac| ac.gamepad);
        let p1_effects = p1_telemetry
            .map(|(state, input, impact)| compute_bike_trigger_effects(state, input, impact));

        for pad in &mut apple.pads {
            let Some(ds) = &mut pad.dualsense else {
                continue;
            };

            if active_pad == Some(pad.entity) {
                if let Some((left, right)) = p1_effects {
                    ds.update(left, right);
                } else {
                    ds.turn_off();
                }
            } else {
                ds.turn_off();
            }
        }
    } else {
        // Split-screen mode: route telemetry to the bound gamepad entity.
        let p1_effects = p1_telemetry.map(|(s, i, imp)| compute_bike_trigger_effects(s, i, imp));
        let p2_effects = other_bikes
            .iter()
            .find(|(_, _, _, lp)| lp.0 == 1)
            .map(|(s, i, imp, _)| compute_bike_trigger_effects(s, i, imp));

        let p1_binding = local_players.as_ref().and_then(|lp| lp.p1);
        let p2_binding = local_players.as_ref().and_then(|lp| lp.p2);

        for pad in &mut apple.pads {
            let Some(ds) = &mut pad.dualsense else {
                continue;
            };

            let binding = ControllerBinding::Gamepad(pad.entity);
            if p1_binding == Some(binding) {
                if let Some((l, r)) = p1_effects {
                    ds.update(l, r);
                } else {
                    ds.turn_off();
                }
            } else if p2_binding == Some(binding) {
                if let Some((l, r)) = p2_effects {
                    ds.update(l, r);
                } else {
                    ds.turn_off();
                }
            } else {
                ds.turn_off();
            }
        }
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn trigger_effect_substantially_equal_detects_tolerances() {
        let e1 = TriggerEffect::SlopeFeedback {
            start_position: 0.10,
            end_position: 1.0,
            start_strength: 0.0,
            end_strength: 0.25,
        };
        let e2 = TriggerEffect::SlopeFeedback {
            start_position: 0.11,
            end_position: 1.0,
            start_strength: 0.0,
            end_strength: 0.26,
        };
        assert!(e1.is_substantially_equal(&e2));

        let e3 = TriggerEffect::SlopeFeedback {
            start_position: 0.10,
            end_position: 1.0,
            start_strength: 0.0,
            end_strength: 0.40,
        };
        assert!(!e1.is_substantially_equal(&e3));

        assert!(TriggerEffect::Off.is_substantially_equal(&TriggerEffect::Off));
        assert!(!TriggerEffect::Off.is_substantially_equal(&e1));
    }

    #[test]
    fn airborne_bike_disables_throttle_resistance() {
        let mut state = BikeState::parked_at((Vec2::ZERO, 0.0));
        state.grounded = false;
        state.speed = 30.0;
        let input = BikeInput {
            throttle: 1.0,
            ..default()
        };

        let (left, right) = compute_bike_trigger_effects(&state, &input, None);
        assert_eq!(right, TriggerEffect::Off);
        assert_eq!(left, TriggerEffect::Off);
    }

    #[test]
    fn grounded_triggers_provide_gradual_soft_resistance_without_vibration() {
        let mut state = BikeState::parked_at((Vec2::ZERO, 0.0));
        state.grounded = true;
        state.speed = 40.0;
        state.grass = 1.0;
        let input = BikeInput {
            throttle: 1.0,
            brake: 1.0,
            ..default()
        };

        let (left, right) = compute_bike_trigger_effects(&state, &input, None);

        // Right trigger (Throttle): gradual slope resistance starting at zero.
        match right {
            TriggerEffect::SlopeFeedback {
                start_strength,
                end_strength,
                start_position,
                end_position,
            } => {
                assert_eq!(start_strength, 0.0);
                assert!((0.12..=0.20).contains(&end_strength));
                assert!((0.30..=0.40).contains(&start_position));
                assert_eq!(end_position, 1.0);
            }
            _ => panic!("Expected SlopeFeedback on right trigger"),
        }

        // Left trigger (Brake): gradual slope resistance starting at zero, no vibration.
        match left {
            TriggerEffect::SlopeFeedback {
                start_strength,
                end_strength,
                start_position,
                end_position,
            } => {
                assert_eq!(start_strength, 0.0);
                assert!((0.12..=0.20).contains(&end_strength));
                assert!((0.30..=0.40).contains(&start_position));
                assert_eq!(end_position, 1.0);
            }
            _ => panic!("Expected SlopeFeedback on left trigger without vibration"),
        }
    }

    #[test]
    fn collision_impact_triggers_soft_vibration() {
        let mut state = BikeState::parked_at((Vec2::ZERO, 0.0));
        state.grounded = true;
        state.speed = 35.0;
        let input = BikeInput::default();
        let impact = CollisionImpact {
            timer: 0.15,
            intensity: 0.6,
            ..Default::default()
        };

        let (left, right) = compute_bike_trigger_effects(&state, &input, Some(&impact));
        match (left, right) {
            (
                TriggerEffect::Vibration {
                    amplitude: a1,
                    start_position: s1,
                    ..
                },
                TriggerEffect::Vibration {
                    amplitude: a2,
                    start_position: s2,
                    ..
                },
            ) => {
                assert!((0.40..=0.75).contains(&a1));
                assert!((0.40..=0.75).contains(&a2));
                // Never at rest: the trigger would read as pressed.
                assert_eq!((s1, s2), (VIBRATION_START, VIBRATION_START));
            }
            _ => panic!("Expected soft vibration during collision impact"),
        }
    }
}
