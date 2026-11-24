//! The speed blur (Options > Graphics > Road blur): as a bike goes fast,
//! the road it has just ridden over, under it in its rider's camera's
//! picture, blurs in streaks towards where the road ahead vanishes, more
//! the lower; over the bike, out to the sides and on the bike itself it
//! stays sharp. One pass over the picture, after the edges are smoothed,
//! as Bevy's FXAA is drawn: a few samples of each pixel taken towards the
//! vanishing point, more and further apart the faster and the lower. It
//! needs nothing drawn twice (no depth or motion of the scene), so it
//! costs the same whatever the circuit: a fraction of a millisecond at
//! 1080p.
//!
//! With the screen split, both cameras draw into one picture: each one's
//! pass blurs only its own part of it (its viewport, see [`SpeedBlur`]),
//! and copies the rest as it is. Its settings for each camera are in one
//! buffer, at an offset each; the bind group is made each frame, so it
//! always binds the buffer as it is (Bevy's own `FullscreenMaterial` keeps
//! its bind groups while the textures last, and when a second camera makes
//! the buffer grow it binds the old one, and the GPU stops the game).

use bevy::anti_alias::{fxaa::fxaa, smaa::smaa};
use bevy::asset::{embedded_asset, load_embedded_asset};
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::ecs::query::QueryItem;
use bevy::prelude::*;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, FilterMode, FragmentState, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, SpecializedRenderPipeline,
    SpecializedRenderPipelines, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::sync_component::SyncComponent;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{GpuResourceAppExt, Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;
use bevy::transform::TransformSystems;

use super::ChaseCamera;
use crate::geometry::smoothstep;
use crate::menu::GameState;
use crate::settings::Settings;

pub(super) struct SpeedBlurPlugin;

impl Plugin for SpeedBlurPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            aim_speed_blur
                .after(TransformSystems::Propagate)
                .after(bevy::camera::CameraUpdateSystems),
        );
        // Its pass only where the world is drawn (not in the tests' apps).
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }
        embedded_asset!(app, "speed_blur.wgsl");
        app.add_plugins((
            ExtractComponentPlugin::<SpeedBlur>::default(),
            UniformComponentPlugin::<SpeedBlur>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_gpu_resource::<SpecializedRenderPipelines<BlurPipeline>>()
            .add_systems(RenderStartup, make_pipeline)
            .add_systems(Render, specialize.in_set(RenderSystems::Prepare))
            // After the edges are smoothed (blurring first would leave them
            // to sharpen again).
            .add_systems(
                Core3d,
                speed_blur
                    .after(fxaa)
                    .after(smaa)
                    .in_set(Core3dSystems::PostProcess),
            );
    }
}

/// The share of the bike's top speed the picture starts to blur at; it
/// blurs most at the top speed.
const FROM_SPEED: f32 = 0.45;
/// How far ahead the point the road vanishes at is taken (m): far enough
/// to be where the camera's view of it ends.
const VANISHING: f32 = 2000.0;
/// How high over the bike's origin the middle of the bike and rider is
/// (m), kept sharp.
const RIDER: f32 = 0.8;

/// The speed blur of a rider's camera, as the shader takes it
/// (`speed_blur.wgsl`): where it draws, where the road ahead vanishes and
/// the bike is, and how much it blurs. Each camera that follows a bike has
/// one (required by [`ChaseCamera`]), aimed every frame
/// ([`aim_speed_blur`]); while it does not blur, no pass is drawn for it.
#[derive(Component, Clone, Copy, Default, Debug, ShaderType)]
pub struct SpeedBlur {
    /// The part of the picture its camera draws: its corner and its size,
    /// in pixels.
    pub viewport: Vec4,
    /// Where the road ahead vanishes, and where the bike is, across and
    /// down the viewport (0 to 1).
    pub center: Vec2,
    pub keep: Vec2,
    /// How much it blurs: none at 0, most at 1.
    pub strength: f32,
    /// The viewport's width over its height.
    pub aspect: f32,
    /// To a whole number of 16 bytes, as WebGL wants its uniforms.
    padding: Vec2,
}

impl SyncComponent for SpeedBlur {
    type Target = SpeedBlur;
}

impl ExtractComponent for SpeedBlur {
    type QueryData = &'static SpeedBlur;
    type QueryFilter = With<Camera>;
    type Out = SpeedBlur;

    /// Only while it blurs: otherwise its pass is not drawn.
    fn extract_component(blur: QueryItem<'_, '_, Self::QueryData>) -> Option<Self::Out> {
        (blur.strength > 0.0).then_some(*blur)
    }
}

/// The speed blur's pipeline: the picture, its sampler and the settings of
/// each camera, at an offset each.
#[derive(Resource)]
struct BlurPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen_shader: FullscreenShader,
    shader: Handle<Shader>,
}

fn make_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    asset_server: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "speed_blur_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<SpeedBlur>(true),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(BlurPipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
        shader: load_embedded_asset!(asset_server.as_ref(), "speed_blur.wgsl"),
    });
}

impl SpecializedRenderPipeline for BlurPipeline {
    /// The format of the picture it draws into.
    type Key = TextureFormat;

    fn specialize(&self, format: TextureFormat) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("speed_blur".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

/// The pipeline a camera's speed blur is drawn with.
#[derive(Component)]
struct BlurPipelineId(CachedRenderPipelineId);

/// The pipeline for each camera that blurs, for the picture it draws into.
fn specialize(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<BlurPipeline>>,
    pipeline: Res<BlurPipeline>,
    views: Query<(Entity, &ExtractedView), With<SpeedBlur>>,
) {
    for (entity, view) in &views {
        let id = pipelines.specialize(&cache, &pipeline, view.target_format);
        commands.entity(entity).insert(BlurPipelineId(id));
    }
}

/// A camera's speed blur, drawn from its picture into the other one (see
/// the module's notes for why its bind group is made every time). Only
/// while it blurs: then it has its [`SpeedBlur`] (and its offset among the
/// cameras' settings this frame).
fn speed_blur(
    view: ViewQuery<(
        &ViewTarget,
        &BlurPipelineId,
        &SpeedBlur,
        &DynamicUniformIndex<SpeedBlur>,
    )>,
    pipeline: Res<BlurPipeline>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<SpeedBlur>>,
    mut ctx: RenderContext,
) {
    let (target, id, _, index) = view.into_inner();
    let Some(render_pipeline) = cache.get_render_pipeline(id.0) else {
        return;
    };
    let Some(settings) = uniforms.uniforms().binding() else {
        return;
    };
    let post_process = target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        "speed_blur_bind_group",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((post_process.source, &pipeline.sampler, settings)),
    );
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("speed_blur"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post_process.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(render_pipeline);
    pass.set_bind_group(0, &bind_group, &[index.index()]);
    pass.draw(0..3, 0..1);
}

/// How much the picture blurs at `speed`, a share of the bike's top speed:
/// none up to [`FROM_SPEED`], then more and more, smoothly, up to all of it
/// at the top speed.
pub fn blur_at(speed: f32) -> f32 {
    smoothstep(FROM_SPEED, 1.0, speed)
}

/// Where a point at `ndc` (Bevy's normalized device coordinates: -1 to 1,
/// up) is across and down the viewport (0 to 1).
pub fn viewport_point(ndc: Vec3) -> Vec2 {
    Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5)
}

/// Each rider's camera's speed blur, from its bike as its camera follows
/// it ([`ChaseCamera`]): how fast it goes, where it is and which way the
/// road ahead goes. None but in a race, with the setting on.
fn aim_speed_blur(
    state: Option<Res<State<GameState>>>,
    settings: Res<Settings>,
    mut cameras: Query<(&ChaseCamera, &Camera, &GlobalTransform, &mut SpeedBlur)>,
) {
    let racing = state.is_some_and(|state| *state.get() == GameState::Race);
    for (chase, camera, transform, mut blur) in &mut cameras {
        let strength = if racing && settings.speed_blur && chase.ahead != Vec3::ZERO {
            blur_at(chase.speed)
        } else {
            0.0
        };
        let Some(rect) = camera.physical_viewport_rect().filter(|_| strength > 0.0) else {
            if blur.strength != 0.0 {
                blur.strength = 0.0;
            }
            continue;
        };
        let at = |point: Vec3| {
            camera
                .world_to_ndc(transform, point)
                .filter(|ndc| ndc.z > 0.0)
                .map(viewport_point)
        };
        let from = transform.translation();
        let size = rect.size().as_vec2().max(Vec2::ONE);
        *blur = SpeedBlur {
            viewport: Vec4::new(rect.min.x as f32, rect.min.y as f32, size.x, size.y),
            center: at(from + chase.ahead * VANISHING).unwrap_or(Vec2::new(0.5, 0.4)),
            keep: at(chase.bike + Vec3::Y * RIDER).unwrap_or(Vec2::new(0.5, 0.7)),
            strength,
            aspect: size.x / size.y,
            padding: Vec2::ZERO,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blur_grows_smoothly_from_well_over_half_speed_to_the_top() {
        assert_eq!(blur_at(0.0), 0.0);
        assert_eq!(blur_at(FROM_SPEED), 0.0);
        assert_eq!(blur_at(1.0), 1.0);
        assert_eq!(blur_at(1.3), 1.0);
        let mut last = 0.0;
        for step in 0..=20 {
            let blur = blur_at(FROM_SPEED + (1.0 - FROM_SPEED) * step as f32 / 20.0);
            assert!(blur >= last, "{blur} after {last}");
            last = blur;
        }
        // Gently from where it starts.
        assert!(blur_at(FROM_SPEED + 0.05) < 0.05);
    }

    #[test]
    fn points_go_across_and_down_the_viewport() {
        assert_eq!(viewport_point(Vec3::ZERO), Vec2::splat(0.5));
        assert_eq!(viewport_point(Vec3::new(-1.0, 1.0, 0.5)), Vec2::ZERO);
        assert_eq!(viewport_point(Vec3::new(1.0, -1.0, 0.5)), Vec2::ONE);
    }

    #[test]
    fn the_shader_takes_whole_lines_of_sixteen_bytes() {
        assert_eq!(SpeedBlur::min_size().get(), 48);
    }

    /// A rider's camera behind a bike going `speed` (a share of the top
    /// speed), in `state`, with the setting `on`: its speed blur once aimed.
    fn aimed(state: GameState, on: bool, speed: f32) -> SpeedBlur {
        let mut app = App::new();
        app.insert_resource(Settings {
            speed_blur: on,
            ..default()
        })
        .insert_resource(State::new(state))
        .add_systems(Update, aim_speed_blur);
        use bevy::camera::CameraProjection;
        let mut projection = PerspectiveProjection::default();
        projection.update(1920.0, 1080.0);
        let camera = Camera {
            computed: bevy::camera::ComputedCameraValues {
                target_info: Some(bevy::camera::RenderTargetInfo {
                    physical_size: UVec2::new(1920, 1080),
                    scale_factor: 1.0,
                }),
                clip_from_view: projection.get_clip_from_view(),
                ..default()
            },
            ..default()
        };
        let projection = Projection::Perspective(projection);
        // Behind and over the bike, looking a little down at it, the road
        // going on ahead (-z).
        let transform =
            Transform::from_xyz(0.0, 2.5, 6.0).looking_at(Vec3::new(0.0, 0.8, -4.0), Vec3::Y);
        let entity = app
            .world_mut()
            .spawn((
                camera,
                projection,
                GlobalTransform::from(transform),
                ChaseCamera {
                    speed,
                    bike: Vec3::ZERO,
                    ahead: Vec3::NEG_Z,
                    ..default()
                },
                SpeedBlur::default(),
            ))
            .id();
        app.update();
        *app.world().get::<SpeedBlur>(entity).unwrap()
    }

    #[test]
    fn it_blurs_only_racing_fast_with_the_setting_on() {
        let fast = aimed(GameState::Race, true, 1.0);
        assert_eq!(fast.strength, 1.0);
        assert_eq!(fast.viewport, Vec4::new(0.0, 0.0, 1920.0, 1080.0));
        assert!((fast.aspect - 16.0 / 9.0).abs() < 1e-4);
        // The road vanishes in the middle across, over the bike, which is
        // under the middle.
        assert!((fast.center.x - 0.5).abs() < 1e-3, "{}", fast.center);
        assert!((fast.keep.x - 0.5).abs() < 1e-3, "{}", fast.keep);
        assert!(
            fast.center.y < 0.5 && fast.keep.y > fast.center.y,
            "{fast:?}"
        );
        assert_eq!(aimed(GameState::Race, true, 0.3).strength, 0.0);
        assert_eq!(aimed(GameState::Race, false, 1.0).strength, 0.0);
        assert_eq!(aimed(GameState::Select, true, 1.0).strength, 0.0);
    }
}
