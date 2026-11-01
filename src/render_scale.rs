//! The render scale (Options > Graphics): the world drawn at less than the
//! window's pixels and scaled up to it, the interface over it at the
//! window's own. Under 100 %, the 3D cameras draw into an image that size,
//! and the interface's camera, which draws over the whole window, first
//! shows it as big as the window ([`ScaledWorld`], smoothed as it is
//! stretched) and sharpens it (Bevy's contrast adaptive sharpening, the
//! sharpening half of AMD's FSR 1), before the interface goes on top,
//! unsharpened. At 100 % the 3D cameras draw on the window, as they always
//! have, and the interface's camera over what they drew.
//!
//! A change of scale makes no new pipeline for the world, only textures of
//! another size (the sharpening's pipeline, and the interface's without
//! multisampling, are made the first time), so it can be changed in a race.
//!
//! The world is never drawn at more pixels than the screen itself has. A
//! Mac's scaled display modes (a 1440p monitor "looking like" 2048 × 1152
//! in HiDPI, a laptop's "more space") give the window more pixels than the
//! panel, and macOS shrinks the whole picture to fit it: what is drawn past
//! the panel's own is never seen. So the share is of the window as the
//! panel would show it ([`NativeShare`]), at most the window. In a
//! fullscreen over the desktop, which covers the screen as it is, a fixed
//! resolution draws the world at that many pixels at most, the screen's
//! shape kept (see [`Settings::desktop_resolution`]): it changes at once,
//! as the render scale does.

use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::camera::{CameraUpdateSystems, ImageRenderTarget, RenderTarget};
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::ui::IsDefaultUiCamera;
use bevy::window::{Monitor, OnMonitor, PrimaryMonitor, PrimaryWindow, WindowRef};

use crate::settings::Settings;
use crate::world::detail::{Detail, choose_detail};
use crate::world::realm::PortalView;

pub struct RenderScalePlugin;

impl Plugin for RenderScalePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NativeShare>()
            .add_systems(Startup, spawn_scaled_world)
            .add_systems(Update, (find_native_sizes, follow_native_share).chain())
            // With the detail of the frame before, as the split screen's
            // halves in `Update`: both change together.
            .add_systems(
                PostUpdate,
                scale_the_world
                    .before(CameraUpdateSystems)
                    .before(choose_detail),
            );
    }
}

/// How much the world drawn smaller is sharpened as it is scaled up (Bevy's
/// `sharpening_strength`, 0 to 1; 0.6 is its own).
const SHARPENING: f32 = 0.5;
/// The smallest share of the window's resolution the world is drawn at (%),
/// whatever the settings say.
const LEAST: u32 = 25;

/// The pixels the world is drawn at in a window of `window` physical pixels
/// at `percent` % of its resolution: that share of each side, one pixel at
/// least; all of them at 100 %.
pub fn render_size(window: UVec2, percent: u32) -> UVec2 {
    if percent >= 100 {
        return window;
    }
    let share = percent.max(LEAST) as f32 / 100.0;
    (window.as_vec2() * share)
        .round()
        .as_uvec2()
        .max(UVec2::ONE)
}

/// How much of each side of the window the screen's own pixels cover (0 to
/// 1): 1 unless the display mode gives the window more pixels than the
/// panel has (see the module's notes), as 0.625 for a 1440p panel drawn at
/// 4096 × 2304; less in a fullscreen over the desktop at a fixed
/// resolution ([`capped`]). The world is drawn at most at that share.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct NativeShare(pub f32);

impl Default for NativeShare {
    fn default() -> Self {
        Self(1.0)
    }
}

/// The pixels the world is drawn at in a window of `window` physical
/// pixels at `percent` % of the screen's own (see [`NativeShare`]).
pub fn drawn_size(window: UVec2, percent: u32, native: NativeShare) -> UVec2 {
    let share = native.0.clamp(0.0, 1.0);
    let base = if share < 1.0 {
        (window.as_vec2() * share)
            .round()
            .as_uvec2()
            .max(UVec2::ONE)
    } else {
        window
    };
    render_size(base, percent)
}

/// A monitor's own resolution, the panel's pixels, where it differs from
/// the display mode's ([`Monitor::physical_width`] and height): found once
/// for each monitor, on macOS (see [`native`]).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct NativeSize(pub UVec2);

/// Finds the panel's own resolution of each monitor that is new or has
/// changed (its display mode).
fn find_native_sizes(
    mut commands: Commands,
    monitors: Query<(Entity, &Monitor), Changed<Monitor>>,
) {
    for (entity, monitor) in &monitors {
        match native_size(monitor) {
            Some(size) => {
                info!(
                    "Screen {}: {} × {} pixels drawn, {} × {} its own",
                    monitor.name.as_deref().unwrap_or("?"),
                    monitor.physical_width,
                    monitor.physical_height,
                    size.x,
                    size.y
                );
                commands.entity(entity).insert(NativeSize(size));
            }
            None => {
                commands.entity(entity).remove::<NativeSize>();
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn native_size(monitor: &Monitor) -> Option<UVec2> {
    native::panel_size(
        UVec2::new(monitor.physical_width, monitor.physical_height),
        monitor.physical_position,
    )
}

/// Elsewhere a display mode is the screen's own pixels, or fewer.
#[cfg(not(target_os = "macos"))]
fn native_size(_monitor: &Monitor) -> Option<UVec2> {
    None
}

#[cfg(target_os = "macos")]
mod native;

/// The share of the window the screen's own pixels cover (see
/// [`NativeShare`]): of the monitor it is on, or else the main one; no more
/// than a fixed resolution lets through in a fullscreen over the desktop.
fn follow_native_share(
    settings: Option<Res<Settings>>,
    window: Query<(&Window, Option<&OnMonitor>), With<PrimaryWindow>>,
    monitors: Query<(&Monitor, Option<&NativeSize>, Has<PrimaryMonitor>)>,
    mut share: ResMut<NativeShare>,
) {
    let Ok((window, on)) = window.single() else {
        return;
    };
    let monitor = on
        .and_then(|on| monitors.get(on.0).ok())
        .or_else(|| monitors.iter().find(|(_, _, primary)| *primary));
    let now = monitor.map_or(NativeShare::default(), |(monitor, native, _)| {
        native_share(
            UVec2::new(monitor.physical_width, monitor.physical_height),
            native.map(|native| native.0),
        )
    });
    let chosen = settings.and_then(|settings| settings.desktop_resolution());
    share.set_if_neq(capped(now, window.physical_size(), chosen));
}

/// `share` (see [`NativeShare`]) no more than a fixed resolution of
/// `chosen` pixels lets through in a window of `window` pixels: the smaller
/// share of the two sides, so the world keeps the window's shape.
pub fn capped(share: NativeShare, window: UVec2, chosen: Option<UVec2>) -> NativeShare {
    let Some(chosen) = chosen.filter(|chosen| chosen.min_element() > 0) else {
        return share;
    };
    if window.min_element() == 0 {
        return share;
    }
    let fits = (chosen.as_vec2() / window.as_vec2()).min_element();
    NativeShare(share.0.min(fits).clamp(0.0, 1.0))
}

/// How much of each side of a display mode of `mode` pixels a panel of
/// `native` pixels covers: 1 when it has as many or more, or is unknown.
pub fn native_share(mode: UVec2, native: Option<UVec2>) -> NativeShare {
    let Some(native) = native.filter(|native| native.min_element() > 0) else {
        return NativeShare::default();
    };
    if mode.min_element() == 0 {
        return NativeShare::default();
    }
    let share = (native.as_vec2() / mode.as_vec2()).min_element();
    NativeShare(share.min(1.0))
}

/// The share of the window's resolution the world is drawn at now (%): as
/// the options say, or as far as the performance forced has come (see
/// [`Detail::scale`]); all of it with no detail yet.
pub fn render_percent(detail: Option<&Detail>) -> u32 {
    detail.map_or(100, |detail| detail.scale)
}

/// The world drawn smaller, shown over the whole window by the interface's
/// camera (the only 2D one): its image is what the 3D cameras draw into.
#[derive(Component)]
struct ScaledWorld;

fn spawn_scaled_world(mut commands: Commands) {
    commands.spawn((ScaledWorld, Sprite::default(), Visibility::Hidden));
}

/// An image `size` pixels big for the 3D cameras to draw into, smoothed
/// when it is stretched over the window.
fn target_image(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    // Drawn into, never filled from here; each size a new image, nothing
    // of an old one to copy into it (Bevy would warn it found none).
    image.data = None;
    image.copy_on_resize = false;
    image.sampler = ImageSampler::linear();
    image
}

/// The world drawn at the render scale in use: into an image that
/// size, shown and sharpened by the interface's camera, or, at 100 %, on
/// the window.
#[allow(clippy::type_complexity)]
fn scale_the_world(
    mut commands: Commands,
    // The detail (with the scale chosen) and the screen's own share.
    (detail, native): (Option<Res<Detail>>, Res<NativeShare>),
    window: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut shown: Query<(&mut Sprite, &mut Visibility), With<ScaledWorld>>,
    mut worlds: Query<(&mut RenderTarget, &mut Projection), (With<Camera3d>, Without<PortalView>)>,
    mut interface: Query<
        (Entity, &mut Camera, Has<ContrastAdaptiveSharpening>),
        (With<IsDefaultUiCamera>, Without<Camera3d>),
    >,
) {
    let (Ok(window), Ok((mut sprite, mut visibility))) = (window.single(), shown.single_mut())
    else {
        return;
    };
    let full = window.physical_size();
    let size = drawn_size(full, render_percent(detail.as_deref()), *native);
    let scaled = size != full;
    if scaled {
        // An image of the size wanted: a new one when the size changes, so
        // that the cameras drawing into it take its size in the same frame.
        if images
            .get(&sprite.image)
            .is_none_or(|image| image.size() != size)
        {
            sprite.image = images.add(target_image(size));
        }
        let covers = Some(window.size());
        if sprite.custom_size != covers {
            sprite.custom_size = covers;
        }
    }
    visibility.set_if_neq(if scaled {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    for (mut target, mut projection) in &mut worlds {
        let right = match &*target {
            RenderTarget::Image(image) => scaled && image.handle == sprite.image,
            RenderTarget::Window(_) => !scaled,
            _ => false,
        };
        if right {
            continue;
        }
        *target = if scaled {
            RenderTarget::Image(ImageRenderTarget {
                handle: sprite.image.clone(),
                scale_factor: 1.0,
            })
        } else {
            RenderTarget::Window(WindowRef::Primary)
        };
        // Bevy sizes a camera again when its window, its image or its
        // projection changes, not its target.
        projection.set_changed();
    }
    for (camera, mut settings, sharpened) in &mut interface {
        // Scaled, it draws the world first, over the whole window: it
        // clears it. Otherwise it draws over what the world's cameras drew.
        let clears = !matches!(settings.clear_color, ClearColorConfig::None);
        if clears != scaled {
            settings.clear_color = if scaled {
                ClearColorConfig::Default
            } else {
                ClearColorConfig::None
            };
        }
        if scaled && !sharpened {
            commands.entity(camera).insert(ContrastAdaptiveSharpening {
                enabled: true,
                sharpening_strength: SHARPENING,
                denoise: false,
            });
        } else if !scaled && sharpened {
            commands
                .entity(camera)
                .remove::<ContrastAdaptiveSharpening>();
        }
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn the_world_is_drawn_at_a_share_of_each_side_of_the_window() {
        let retina = UVec2::new(3024, 1898);
        assert_eq!(render_size(retina, 100), retina);
        assert_eq!(render_size(retina, 70), UVec2::new(2117, 1329));
        assert_eq!(render_size(retina, 50), UVec2::new(1512, 949));
        // Never more than the window, never less than a quarter of each
        // side, never nothing.
        assert_eq!(render_size(retina, 150), retina);
        assert_eq!(render_size(retina, 5), render_size(retina, LEAST));
        assert_eq!(render_size(UVec2::ZERO, 50), UVec2::ONE);
        assert_eq!(render_percent(None), 100);
    }

    #[test]
    fn the_world_is_never_drawn_at_more_pixels_than_the_screen_has() {
        // A 1440p monitor in a HiDPI mode that looks like 2048 × 1152: the
        // window has 4096 × 2304 pixels, the panel 2560 × 1440.
        let hidpi = UVec2::new(4096, 2304);
        let share = native_share(hidpi, Some(UVec2::new(2560, 1440)));
        assert_eq!(share, NativeShare(0.625));
        assert_eq!(drawn_size(hidpi, 100, share), UVec2::new(2560, 1440));
        assert_eq!(drawn_size(hidpi, 85, share), UVec2::new(2176, 1224));
        // A window on it drawn by its own share too.
        assert_eq!(
            drawn_size(UVec2::new(2048, 1152), 100, share),
            UVec2::new(1280, 720)
        );
        // A panel with as many pixels as the mode, or more, or unknown,
        // changes nothing.
        let retina = UVec2::new(3024, 1898);
        for native in [
            Some(retina),
            Some(UVec2::new(6000, 4000)),
            None,
            Some(UVec2::ZERO),
        ] {
            let share = native_share(retina, native);
            assert_eq!(share, NativeShare::default());
            assert_eq!(drawn_size(retina, 70, share), render_size(retina, 70));
        }
        assert_eq!(
            native_share(UVec2::ZERO, Some(retina)),
            NativeShare::default()
        );
    }

    #[test]
    fn over_the_desktop_a_fixed_resolution_draws_the_world_at_that_many_pixels_at_most() {
        // A 16 : 10 laptop panel covered by the window, 1920 × 1080 chosen:
        // the world 1080 pixels high, the panel's shape kept, scaled up.
        let laptop = UVec2::new(3456, 2234);
        let chosen = Some(UVec2::new(1920, 1080));
        let share = capped(NativeShare::default(), laptop, chosen);
        assert_eq!(drawn_size(laptop, 100, share), UVec2::new(1671, 1080));
        // And the render scale on top of it.
        assert_eq!(drawn_size(laptop, 50, share), UVec2::new(836, 540));
        // Bigger than the window, or none chosen: as before.
        let large = Some(UVec2::new(5120, 2880));
        assert_eq!(
            capped(NativeShare::default(), laptop, large),
            NativeShare::default()
        );
        assert_eq!(capped(NativeShare(0.625), laptop, None), NativeShare(0.625));
        // With the panel's own share smaller still, the smaller.
        let hidpi = UVec2::new(4096, 2304);
        let share = capped(NativeShare(0.625), hidpi, Some(UVec2::new(2560, 1440)));
        assert_eq!(drawn_size(hidpi, 100, share), UVec2::new(2560, 1440));
        assert_eq!(
            capped(NativeShare(0.625), UVec2::ZERO, chosen),
            NativeShare(0.625)
        );
    }

    #[test]
    fn under_100_percent_the_world_is_drawn_into_an_image_shown_over_the_window() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Image>()
            .add_plugins(RenderScalePlugin);
        let mut window = Window::default();
        window.resolution.set_physical_resolution(2000, 1000);
        app.world_mut().spawn((window, PrimaryWindow));
        let world = app.world_mut().spawn(Camera3d::default()).id();
        let interface = app
            .world_mut()
            .spawn((
                Camera2d,
                Camera {
                    clear_color: ClearColorConfig::None,
                    ..default()
                },
                IsDefaultUiCamera,
            ))
            .id();
        let mut detail = Detail {
            scale: 70,
            ..default()
        };
        app.insert_resource(detail);
        app.update();
        let mut sprites = app.world_mut().query::<(&Sprite, &Visibility)>();
        let target = app.world().get::<RenderTarget>(world).unwrap();
        let RenderTarget::Image(drawn) = target else {
            panic!("{target:?}");
        };
        let images = app.world().resource::<Assets<Image>>();
        assert_eq!(
            images.get(&drawn.handle).unwrap().size(),
            UVec2::new(1400, 700)
        );
        let (sprite, visibility) = sprites.single(app.world()).unwrap();
        assert_eq!(sprite.image, drawn.handle);
        assert_eq!(*visibility, Visibility::Inherited);
        assert!(
            app.world()
                .get::<ContrastAdaptiveSharpening>(interface)
                .is_some()
        );
        let camera = app.world().get::<Camera>(interface).unwrap();
        assert!(matches!(camera.clear_color, ClearColorConfig::Default));
        // Back at 100 %: on the window, as always.
        detail.scale = 100;
        app.insert_resource(detail);
        app.update();
        let target = app.world().get::<RenderTarget>(world).unwrap();
        assert!(matches!(target, RenderTarget::Window(_)), "{target:?}");
        assert!(
            app.world()
                .get::<ContrastAdaptiveSharpening>(interface)
                .is_none()
        );
        let camera = app.world().get::<Camera>(interface).unwrap();
        assert!(matches!(camera.clear_color, ClearColorConfig::None));
        let (_, visibility) = sprites.single(app.world()).unwrap();
        assert_eq!(*visibility, Visibility::Hidden);
    }
}
