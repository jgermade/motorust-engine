//! Cámara de persecución: detrás y por encima de la moto, siguiendo sus giros
//! con algo de retraso (and, with the curves' handling, in a tight corner
//! turning into it, ahead of the bike, to show the way out), quedándose atrás al acelerar y abriendo el ángulo de
//! visión con la velocidad. El stick derecho del mando dice hacia dónde mira
//! respecto a la moto: arriba, hacia delante como siempre; a un lado, a ese
//! lado, y abajo, hacia atrás. Al soltarlo vuelve a mirar hacia delante. It
//! looks there at once, as the stick moves, and back ahead at once too.
//!
//! Once the players' race is over, it follows the bikes the race watches
//! come in ([`Watching`]), and then the shots of them riding on, cutting
//! from one to the next, over the whole screen in split screen too.

use std::ops::Range;

use bevy::camera::{CameraUpdateSystems, RenderTarget, Viewport};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::bike::curves::KeyboardMode;
use crate::bike::{BikeState, BikeTuning, CrashState, Knock, LocalPlayer, Player};
use crate::geometry::{approach, smoothstep, wrap_angle};
use crate::lobby::{ControllerBinding, LocalPlayersConfig};
use crate::menu::GameState;
use crate::race::Watching;
use crate::render_scale::{NativeShare, drawn_size, render_percent};
use crate::settings::Settings;
use crate::welcome::GameMode;
use crate::world::detail::Detail;
// The fog is the colour of the sky at the horizon: far away, the land melts
// into the sky.
use crate::world::sky::{FOG, HORIZON};

/// Hasta dónde se ve (m): más allá del cielo, que está a 4,5 km.
const FAR: f32 = 6000.0;
/// Distancia detrás de la moto: parada .. a toda velocidad.
const DISTANCE: Range<f32> = 3.4..4.2;
const HEIGHT: f32 = 1.55;
/// La cámara mira a un punto por delante de la moto, no a la moto.
const LOOK_AHEAD: f32 = 5.0;
const LOOK_HEIGHT: f32 = 1.0;
/// Campo de visión vertical: parada .. a toda velocidad. Abrirlo con la
/// velocidad exagera la sensación de rapidez.
const FOV: Range<f32> = 1.05..1.35;
/// Rapidez con la que la cámara se pone detrás de la moto al girar (1/s).
const YAW_RESPONSE: f32 = 5.0;
/// Como si la cámara también tuviera inercia, se queda atrás al acelerar y se
/// echa encima al frenar (m por cada m/s²).
const SURGE_PER_ACCEL: f32 = 0.035;
/// Lo más que se acerca .. se aleja por la inercia (m).
const SURGE: Range<f32> = -0.6..0.45;
const SURGE_RESPONSE: f32 = 3.0;
/// Cuánto hay que inclinar el stick derecho para que la cámara mire hacia
/// donde apunta; por debajo, mira hacia delante.
const LOOK_DEADZONE: f32 = 0.4;
/// Lo que tarda la cámara en ir de una pantalla de menú a la siguiente (s).
const GLIDE_TIME: f32 = 1.2;
/// How fast the camera comes back upright after leaving a loop the wrong
/// way, falling off it (1/s).
const UPRIGHT_RESPONSE: f32 = 6.0;
/// In a tight corner the camera turns into it, ahead of the bike, to show
/// its way out: not at all turning slower than the first (rad/s), all of
/// [`CORNER_LEAD`] (rad) from the second on, eased in and out at
/// [`LEAD_RESPONSE`] (1/s). A gentle bend, ridden at any speed, turns the
/// bike slower than that.
const LEAD_TURNS: Range<f32> = 0.5..1.4;
const CORNER_LEAD: f32 = 0.25;
const LEAD_RESPONSE: f32 = 3.0;

/// How far the camera turns into a corner (rad, on the side it turns to)
/// for the bike turning `yaw_rate` (rad/s, positive to the left).
fn corner_lead(yaw_rate: f32) -> f32 {
    CORNER_LEAD * smoothstep(LEAD_TURNS.start, LEAD_TURNS.end, yaw_rate.abs()) * yaw_rate.signum()
}

pub struct ChaseCameraPlugin;

impl Plugin for ChaseCameraPlugin {
    fn build(&self, app: &mut App) {
        // Entre los menús la cámara se desliza de un sitio a otro, with the
        // track seen from above while it is chosen; seen from the road, it
        // cuts (see [`start_glide`]). The welcome screen, the lobby and the
        // modes share one flight over the track (`crate::welcome`): between
        // them it just flies on, with no glide that would stop it and set it
        // off again.
        for (from, to) in [
            (GameState::Welcome, GameState::Tracks),
            (GameState::Tracks, GameState::Welcome),
            (GameState::Lobby, GameState::Tracks),
            (GameState::Tracks, GameState::Lobby),
            (GameState::Tracks, GameState::Select),
            (GameState::Select, GameState::Tracks),
            (GameState::Modes, GameState::Tracks),
            (GameState::Tracks, GameState::Modes),
            (GameState::Modes, GameState::Select),
            (GameState::Select, GameState::Modes),
        ] {
            app.add_systems(
                OnTransition {
                    exited: from,
                    entered: to,
                },
                start_glide,
            );
        }
        app.insert_resource(ClearColor(HORIZON))
            .add_systems(Startup, spawn_camera)
            .add_systems(PostUpdate, follow_window_size.before(CameraUpdateSystems))
            .init_resource::<CameraGlide>()
            .add_systems(OnEnter(GameState::Select), setup_split_cameras)
            .add_systems(OnEnter(GameState::Race), (setup_split_cameras, reset_chase))
            .add_systems(OnEnter(GameState::Tracks), cleanup_camera_viewports)
            .add_systems(OnEnter(GameState::Welcome), cleanup_camera_viewports)
            .add_systems(OnEnter(GameState::Lobby), cleanup_camera_viewports)
            .add_systems(OnEnter(GameState::Modes), cleanup_camera_viewports)
            .add_systems(
                Update,
                (
                    update_camera_viewports
                        .run_if(in_state(GameState::Select).or_else(in_state(GameState::Race))),
                    follow_player.run_if(in_state(GameState::Race)),
                ),
            );
    }
}

/// A qué jugador sigue esta cámara (0 para Player 1, 1 para Player 2).
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct CameraPlayer(pub usize);

#[derive(Component, Default)]
struct ChaseCamera {
    /// Orientación actual de la cámara; `None` hasta el primer fotograma.
    yaw: Option<f32>,
    /// Distancia extra por la inercia (m).
    surge: f32,
    /// Hacia dónde mira respecto a la moto (rad, positivo a la izquierda),
    /// ya suavizado.
    look: f32,
    /// How far the camera is turned head over heels with the bike, about
    /// its own sideways axis: round a loop it goes round with the bike.
    /// `None` is upright.
    tilt: Option<Quat>,
    /// The bike it watches come in, if it is not the player's.
    watched: Option<Entity>,
    /// How far it turns into a tight corner ahead of the bike (rad,
    /// positive to the left), and the bike's heading in the frame before,
    /// for how fast it turns.
    lead: f32,
    last_yaw: Option<f32>,
}

/// El stick derecho del mando que más se esté moviendo (x a la derecha, y
/// hacia arriba), o cero.
pub fn right_stick(gamepads: &Query<&Gamepad>) -> Vec2 {
    gamepads
        .iter()
        .map(|gamepad| {
            let axis = |axis| gamepad.get(axis).unwrap_or(0.0);
            Vec2::new(
                axis(GamepadAxis::RightStickX),
                axis(GamepadAxis::RightStickY),
            )
        })
        .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))
        .unwrap_or(Vec2::ZERO)
}

fn gamepad_right_stick(gamepads: &Query<&Gamepad>, entity: Entity) -> Vec2 {
    if let Ok(gamepad) = gamepads.get(entity) {
        let axis = |a| gamepad.get(a).unwrap_or(0.0);
        Vec2::new(
            axis(GamepadAxis::RightStickX),
            axis(GamepadAxis::RightStickY),
        )
    } else {
        Vec2::ZERO
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Camera::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV.start,
            far: FAR,
            ..default()
        }),
        // Neblina del color del horizonte: lo lejano se funde con el cielo.
        DistanceFog {
            color: HORIZON,
            falloff: FogFalloff::Linear {
                start: FOG.0,
                end: FOG.1,
            },
            ..default()
        },
        ChaseCamera::default(),
        CameraPlayer(0),
    ));

    // Cámara dedicada a la interfaz (UI) para que la maquetación siempre cubra
    // la ventana completa, independientemente de los viewports de pantalla dividida 3D.
    commands.spawn((
        Camera2d,
        Camera {
            order: 100,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        bevy::ui::IsDefaultUiCamera,
    ));
}

/// En modo pantalla dividida, crea la segunda cámara para el Jugador 2 (abajo).
/// It draws after the first on the same picture, without clearing it:
/// clearing, it cleared the whole picture, not only its half, and with one
/// sample a pixel (FXAA, or no smoothing) the top half was left the colour
/// of the sky. Its own half is drawn over whole, the sky first; multisampled,
/// Bevy copies the picture into its samples first.
fn setup_split_cameras(
    mut commands: Commands,
    game_mode: Res<GameMode>,
    existing: Query<Entity, (With<CameraPlayer>, With<Camera3d>)>,
) {
    if *game_mode == GameMode::SplitScreen && existing.iter().count() < 2 {
        commands.spawn((
            Camera3d::default(),
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: FOV.start,
                far: FAR,
                ..default()
            }),
            DistanceFog {
                color: HORIZON,
                falloff: FogFalloff::Linear {
                    start: FOG.0,
                    end: FOG.1,
                },
                ..default()
            },
            ChaseCamera::default(),
            CameraPlayer(1),
        ));
    }
}

/// Divide la pantalla en dos viewports horizontales:
/// Jugador 1 arriba (0 .. alto/2), Jugador 2 abajo (alto/2 .. alto).
/// While the others are watched come in, the first camera takes the whole
/// screen.
/// Every camera on the window takes its new size in the same frame. Bevy
/// reads a camera's size again only when the window reports a resize or the
/// camera's projection changes. On macOS, asking for another window size
/// changes `Window` at once, but winit reports the resize frames later (or,
/// during a fullscreen animation, never): meanwhile the 3D camera, whose
/// projection changes every frame, would take the new size and the UI camera
/// keep the old one. They share the window's colour textures, sized by
/// whichever comes first, while the depth is the 3D camera's size, and the
/// GPU stops with "attachments have differing sizes".
fn follow_window_size(
    window: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&Camera, &RenderTarget, &mut Projection)>,
) {
    let Ok(window) = window.single() else {
        return;
    };
    let size = window.physical_size();
    for (camera, target, mut projection) in &mut cameras {
        if matches!(target, RenderTarget::Window(_))
            && camera.physical_target_size().is_some_and(|now| now != size)
        {
            projection.set_changed();
        }
    }
}

fn update_camera_viewports(
    windows: Query<&Window, With<PrimaryWindow>>,
    game_mode: Res<GameMode>,
    watching: Option<Res<Watching>>,
    detail: Option<Res<Detail>>,
    native: Option<Res<NativeShare>>,
    mut cameras: Query<(&CameraPlayer, &mut Camera)>,
) {
    if *game_mode == GameMode::SplitScreen && watching.is_none() {
        let Some(window) = windows.iter().next() else {
            return;
        };
        // Halves of what the world is drawn on: the window, or the image
        // the size of the render scale (see `crate::render_scale`).
        let size = drawn_size(
            window.physical_size(),
            render_percent(detail.as_deref()),
            native.as_deref().copied().unwrap_or_default(),
        );
        let (width, height) = (size.x, size.y);
        if width == 0 || height == 0 {
            return;
        }

        let half_height = height / 2;
        for (cam_player, mut camera) in &mut cameras {
            camera.is_active = true;
            if cam_player.0 == 0 {
                camera.order = 0;
                camera.viewport = Some(Viewport {
                    physical_position: UVec2::new(0, 0),
                    physical_size: UVec2::new(width, half_height),
                    depth: 0.0..1.0,
                });
            } else if cam_player.0 == 1 {
                camera.order = 1;
                camera.viewport = Some(Viewport {
                    physical_position: UVec2::new(0, half_height),
                    physical_size: UVec2::new(width, height.saturating_sub(half_height)),
                    depth: 0.0..1.0,
                });
            }
        }
    } else {
        for (cam_player, mut camera) in &mut cameras {
            if cam_player.0 == 0 {
                camera.order = 0;
                if camera.viewport.is_some() {
                    camera.viewport = None;
                }
                camera.is_active = true;
            } else if camera.is_active {
                camera.is_active = false;
            }
        }
    }
}

/// Al salir a menús compartidos de pantalla completa, restaura la cámara principal y elimina la secundaria.
fn cleanup_camera_viewports(
    mut commands: Commands,
    mut cameras: Query<(Entity, &CameraPlayer, &mut Camera), With<Camera3d>>,
) {
    for (entity, cam_player, mut camera) in &mut cameras {
        if cam_player.0 == 0 {
            camera.order = 0;
            camera.viewport = None;
            camera.is_active = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
}
/// Para que la cámara vaya poco a poco de donde estaba al sitio que le toca
/// en la pantalla nueva, en lugar de saltar: dónde estaba, su campo de
/// visión y cuánto hace que empezó a moverse (s).
#[derive(Resource, Default)]
pub struct CameraGlide {
    from: Option<(Transform, f32)>,
    elapsed: f32,
}

impl CameraGlide {
    /// Pone la cámara en `wanted` con campo de visión `fov`, o de camino hacia
    /// allí si acaba de cambiar de pantalla.
    pub fn apply(
        &mut self,
        transform: &mut Transform,
        projection: &mut Projection,
        wanted: Transform,
        fov: f32,
        dt: f32,
    ) {
        let (place, angle) = match self.from {
            Some((from, from_fov)) => {
                self.elapsed += dt;
                let t = smoothstep(0.0, GLIDE_TIME, self.elapsed);
                if t >= 1.0 {
                    self.from = None;
                }
                (
                    Transform {
                        translation: from.translation.lerp(wanted.translation, t),
                        rotation: from.rotation.slerp(wanted.rotation, t),
                        ..wanted
                    },
                    from_fov.lerp(fov, t),
                )
            }
            None => (wanted, fov),
        };
        *transform = place;
        if let Projection::Perspective(perspective) = projection {
            perspective.fov = angle;
        }
    }
}

/// From one menu to another, the camera glides from where it was, if the
/// track is seen from above while it is chosen. Seen from the road
/// ([`crate::settings::TrackView::Trackside`]), it cuts: the flight over
/// the road goes on into the track screen as it is, and the map and the
/// bikes take each other's place at once.
#[allow(clippy::type_complexity)]
fn start_glide(
    cameras: Query<(&Transform, &Projection), (With<Camera3d>, With<ChaseCamera>)>,
    settings: Option<Res<Settings>>,
    mut glide: ResMut<CameraGlide>,
) {
    if settings.is_some_and(|settings| settings.track_view.at_the_road()) {
        *glide = CameraGlide::default();
        return;
    }
    let Some((transform, projection)) = cameras.iter().next() else {
        return;
    };
    let fov = match projection {
        Projection::Perspective(perspective) => perspective.fov,
        _ => FOV.start,
    };
    *glide = CameraGlide {
        from: Some((*transform, fov)),
        elapsed: 0.0,
    };
}

/// Al empezar la carrera, la cámara se coloca de golpe detrás del jugador.
fn reset_chase(mut cameras: Query<&mut ChaseCamera>) {
    for mut camera in &mut cameras {
        *camera = ChaseCamera::default();
    }
}

/// Hacia dónde apunta el stick derecho respecto a la moto (rad, positivo a
/// la izquierda: arriba 0, a la derecha -π/2 y abajo ±π), o `None` si apenas
/// se inclina.
fn stick_direction(stick: Vec2) -> Option<f32> {
    (stick.length() > LOOK_DEADZONE).then(|| (-stick.x).atan2(stick.y))
}

/// Dónde va la cámara y a qué punto mira, a `distance` de la moto en `target`
/// orientada hacia `heading`: detrás de la moto según esa dirección, mirando
/// por delante de ella.
#[cfg(any(test, feature = "testing"))]
fn chase_view(target: Vec3, heading: f32, distance: f32) -> (Vec3, Vec3) {
    let (position, look_at, _) = tilted_chase_view(target, heading, distance, Quat::IDENTITY);
    (position, look_at)
}

/// Like [`chase_view`], with the view turned by `tilt` (in the frame of
/// the bike, after its heading): behind and above the bike in that frame.
/// Also returns which way is up for the camera.
fn tilted_chase_view(target: Vec3, heading: f32, distance: f32, tilt: Quat) -> (Vec3, Vec3, Vec3) {
    let frame = Quat::from_rotation_y(heading) * tilt;
    let (behind, up) = (frame * Vec3::Z, frame * Vec3::Y);
    let position = target + behind * distance + up * HEIGHT;
    (
        position,
        target - behind * LOOK_AHEAD + up * LOOK_HEIGHT,
        up,
    )
}

/// The tilt (see [`tilted_chase_view`]) of the camera facing `yaw` behind a
/// bike turned `rotation` round a loop: turned over with the bike, and on
/// the running surface turned as far as the camera has followed it there,
/// about the bike's own up. `bike_yaw` is the heading the bike would have
/// off the loop, which the camera's `yaw` follows; once the camera has
/// caught up, it is right behind the bike. Out of the loop, upright, there
/// is no tilt left.
fn loop_tilt(rotation: Quat, yaw: f32, bike_yaw: f32) -> Quat {
    Quat::from_rotation_y(-yaw) * rotation * Quat::from_rotation_y(yaw - bike_yaw)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn follow_player(
    time: Res<Time>,
    tuning: Res<BikeTuning>,
    settings: Res<Settings>,
    gamepads: Query<&Gamepad>,
    game_mode: Res<GameMode>,
    local_players: Res<LocalPlayersConfig>,
    player_bikes: Query<
        (&Transform, &BikeState, Option<(&Knock, &CrashState)>),
        (With<Player>, Without<ChaseCamera>),
    >,
    other_bikes: Query<
        (
            &Transform,
            &BikeState,
            Option<(&Knock, &CrashState)>,
            &LocalPlayer,
        ),
        Without<ChaseCamera>,
    >,
    every_bike: Query<
        (&Transform, &BikeState, Option<(&Knock, &CrashState)>),
        Without<ChaseCamera>,
    >,
    watching: Option<Res<Watching>>,
    mut cameras: Query<(
        &mut Transform,
        &mut Projection,
        &mut ChaseCamera,
        &CameraPlayer,
    )>,
) {
    let watched = watching.map(|watching| watching.bike);
    let curves_steer = settings.keyboard_mode == KeyboardMode::Bezier
        || settings.stick_mode == KeyboardMode::Bezier;
    for (mut transform, mut projection, mut chase, cam_player) in &mut cameras {
        // Watching another bike, or back on the player's: a cut, the camera
        // right behind it.
        let watches = watched.filter(|_| cam_player.0 == 0);
        if chase.watched != watches {
            *chase = ChaseCamera {
                watched: watches,
                ..default()
            };
        }
        let bike_data = if let Some(bike) = watches {
            every_bike.get(bike).ok()
        } else if cam_player.0 == 0 {
            player_bikes.iter().next()
        } else {
            other_bikes
                .iter()
                .find(|(.., lp)| lp.0 == cam_player.0)
                .map(|(t, s, c, _)| (t, s, c))
        };

        let Some((bike_transform, state, crash)) = bike_data else {
            continue;
        };
        // While the rider is down the bike tumbles and spins: the camera
        // keeps looking the way it was, and pulls in as the bike slows down.
        let down = crash.is_some_and(|(_, crash)| crash.is_down());
        let speed = match crash {
            Some((knock, crash)) if crash.is_down() => knock.velocity.length(),
            _ => state.speed.abs(),
        };

        // El Transform de la moto ya viene interpolado y solo lleva guiñada (la
        // inclinación va en un hijo) y cabeceo. Round a loop the pitch goes
        // past the vertical, so the heading comes from the bike itself: the
        // one it will have off the loop, turned as it is on the running
        // surface.
        let (bike_yaw, _, _) = bike_transform.rotation.to_euler(EulerRot::YXZ);
        let bike_yaw = match state.ride {
            Some(ride) => state.heading + ride.yaw,
            None => bike_yaw,
        };
        let yaw = match chase.yaw {
            Some(yaw) if down => yaw,
            Some(yaw) => {
                let blend = 1.0 - (-YAW_RESPONSE * time.delta_secs()).exp();
                yaw + wrap_angle(bike_yaw - yaw) * blend
            }
            None => bike_yaw,
        };
        chase.yaw = Some(yaw);

        // Into a tight corner, ahead of the bike; not while it tumbles.
        let dt_turn = time.delta_secs().max(1e-4);
        let yaw_rate = chase
            .last_yaw
            .map_or(0.0, |last| wrap_angle(bike_yaw - last) / dt_turn);
        chase.last_yaw = Some(bike_yaw);
        // Only with the curves' handling (Bézier, for the keyboard or the
        // stick): with the classic controls, the camera as it always was.
        let lead = if down || state.ride.is_some() || !curves_steer {
            0.0
        } else {
            corner_lead(yaw_rate)
        };
        chase.lead = approach(chase.lead, lead, LEAD_RESPONSE, time.delta_secs());

        // Stick derecho: la cámara mira hacia donde apunta, y al soltarlo
        // vuelve a mirar hacia delante; at once, as the stick moves.
        let dt = time.delta_secs();
        let stick = if *game_mode == GameMode::SplitScreen {
            let binding = if cam_player.0 == 0 {
                local_players.p1
            } else {
                local_players.p2
            };
            match binding {
                Some(ControllerBinding::Gamepad(entity)) => gamepad_right_stick(&gamepads, entity),
                _ => Vec2::ZERO,
            }
        } else {
            right_stick(&gamepads)
        };

        chase.look = stick_direction(stick).unwrap_or(0.0);

        let speed_factor = (speed / tuning.max_speed).clamp(0.0, 1.0);
        let surge = (state.accel * SURGE_PER_ACCEL).clamp(SURGE.start, SURGE.end);
        chase.surge = approach(chase.surge, surge, SURGE_RESPONSE, dt);
        let distance = DISTANCE.start.lerp(DISTANCE.end, speed_factor) + chase.surge;
        // Round a loop the camera turns over with the bike, behind and above
        // it in its own frame; off it, it comes back upright.
        chase.tilt = if state.ride.is_some() {
            Some(loop_tilt(bike_transform.rotation, yaw, bike_yaw))
        } else {
            chase.tilt.and_then(|tilt| {
                let blend = 1.0 - (-UPRIGHT_RESPONSE * dt).exp();
                let tilt = tilt.slerp(Quat::IDENTITY, blend);
                (tilt.angle_between(Quat::IDENTITY) > 1e-3).then_some(tilt)
            })
        };
        let (position, look_at, up) = tilted_chase_view(
            bike_transform.translation,
            yaw + chase.look + chase.lead,
            distance,
            chase.tilt.unwrap_or(Quat::IDENTITY),
        );
        transform.translation = position;
        transform.look_at(look_at, up);

        if let Projection::Perspective(perspective) = projection.as_mut() {
            perspective.fov = FOV.start.lerp(FOV.end, speed_factor);
        }
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};

    use super::*;

    #[test]
    fn the_camera_turns_into_tight_corners_only() {
        // A gentle bend at any speed: nothing.
        assert_eq!(corner_lead(0.3), 0.0);
        assert_eq!(corner_lead(-0.5), 0.0);
        // A hairpin: all of it, into the corner's side.
        assert_eq!(corner_lead(2.0), CORNER_LEAD);
        assert_eq!(corner_lead(-2.0), -CORNER_LEAD);
        // In between, some, growing with the turn.
        let some = corner_lead(0.9);
        assert!(some > 0.0 && some < CORNER_LEAD);
        assert!(corner_lead(1.1) > some);
    }

    #[test]
    fn from_the_road_the_menus_cut_and_from_above_they_glide() {
        use crate::settings::TrackView;
        use bevy::ecs::system::RunSystemOnce;
        for (view, glides) in [(TrackView::Trackside, false), (TrackView::Aerial, true)] {
            let mut world = World::new();
            world.insert_resource(Settings {
                track_view: view,
                ..default()
            });
            // A glide under way is let go of too.
            world.insert_resource(CameraGlide {
                from: Some((Transform::default(), FOV.start)),
                elapsed: 0.5,
            });
            world.spawn((
                Camera3d::default(),
                ChaseCamera::default(),
                Transform::from_xyz(1.0, 2.0, 3.0),
                Projection::default(),
            ));
            world.run_system_once(start_glide).unwrap();
            let glide = world.resource::<CameraGlide>();
            assert_eq!(glide.from.is_some(), glides, "{view:?}");
            if glides {
                assert_eq!(glide.elapsed, 0.0);
                assert_eq!(glide.from.unwrap().0.translation, Vec3::new(1.0, 2.0, 3.0));
            }
        }
    }

    #[test]
    fn the_second_player_s_camera_draws_over_the_first_s_without_clearing_it() {
        let mut app = App::new();
        app.insert_resource(GameMode::SplitScreen)
            .add_systems(Update, setup_split_cameras);
        app.world_mut()
            .spawn((Camera3d::default(), Camera::default(), CameraPlayer(0)));
        app.update();
        let mut cameras = app
            .world_mut()
            .query::<(&CameraPlayer, &Camera)>()
            .iter(app.world())
            .map(|(player, camera)| (player.0, camera.order, camera.clear_color))
            .collect::<Vec<_>>();
        cameras.sort_by_key(|(player, ..)| *player);
        assert_eq!(cameras.len(), 2);
        assert!(matches!(cameras[0].2, ClearColorConfig::Default));
        assert_eq!(cameras[1].1, 1, "after the first");
        assert!(matches!(cameras[1].2, ClearColorConfig::None));
        // Only one more, however often it is asked for.
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&CameraPlayer>()
                .iter(app.world())
                .count(),
            2
        );
    }

    #[test]
    fn the_right_stick_points_where_the_camera_looks() {
        let direction = |x, y| stick_direction(Vec2::new(x, y));
        // Arriba, hacia delante; a la derecha, a la derecha (ángulo negativo);
        // abajo, hacia atrás.
        assert_eq!(direction(0.0, 1.0), Some(0.0));
        assert!((direction(1.0, 0.0).unwrap() + FRAC_PI_2).abs() < 1e-6);
        assert!((direction(-1.0, 0.0).unwrap() - FRAC_PI_2).abs() < 1e-6);
        assert!((direction(0.0, -1.0).unwrap().abs() - PI).abs() < 1e-6);
        // Apenas inclinado no cuenta.
        assert_eq!(direction(0.2, -0.2), None);
    }

    #[test]
    fn the_camera_looks_past_the_bike_where_it_points() {
        // Moto parada en el origen mirando a -Z: su derecha es +X.
        let view = |look| chase_view(Vec3::ZERO, look, 4.0);
        // Hacia delante: detrás de la moto, mirando por delante de ella.
        let (camera, looking) = view(0.0);
        assert!(camera.z > 3.9 && camera.x.abs() < 1e-4);
        assert!(looking.z < 0.0);
        // A la derecha: la cámara a la izquierda de la moto, mirando a la derecha.
        let (camera, looking) = view(-FRAC_PI_2);
        assert!(camera.x < -3.9 && looking.x > 0.0);
        // Hacia atrás: la cámara delante de la moto, mirando hacia atrás.
        let (camera, looking) = view(PI);
        assert!(camera.z < -3.9 && looking.z > 0.0);
    }

    #[test]
    fn round_a_loop_the_camera_follows_the_bike_turning_on_the_running_surface() {
        // A bike heading 0.5 rad round a loop, turned 0.3 rad to its left on
        // the running surface: off the loop it would head 0.8 rad.
        let (heading, yaw) = (0.5, 0.3);
        let turned = |pitch: f32| {
            Quat::from_rotation_y(heading)
                * Quat::from_rotation_x(pitch)
                * Quat::from_rotation_y(yaw)
        };
        // Whether two rotations turn the axes the same way.
        let same = |a: Quat, b: Quat| {
            [Vec3::X, Vec3::Y, Vec3::Z]
                .iter()
                .all(|&axis| (a * axis - b * axis).length() < 1e-4)
        };
        for pitch in [0.3, FRAC_PI_2, PI, 4.5] {
            let rotation = turned(pitch);
            // Caught up, the camera is right behind the bike, upright in
            // its frame.
            let tilt = loop_tilt(rotation, heading + yaw, heading + yaw);
            let frame = Quat::from_rotation_y(heading + yaw) * tilt;
            assert!(same(frame, rotation), "{pitch}");
            // Still turned as the bike was before it turned, it stays on
            // the running surface: its up is the bike's.
            let tilt = loop_tilt(rotation, heading, heading + yaw);
            let frame = Quat::from_rotation_y(heading) * tilt;
            let unturned = Quat::from_rotation_y(heading) * Quat::from_rotation_x(pitch);
            assert!(same(frame, unturned), "{pitch}");
            assert!((frame * Vec3::Y - rotation * Vec3::Y).length() < 1e-4);
        }
        // Out of the loop, a whole turn round, there is no tilt left.
        let tilt = loop_tilt(turned(TAU), heading + yaw, heading + yaw);
        assert!(same(tilt, Quat::IDENTITY));
    }
}
