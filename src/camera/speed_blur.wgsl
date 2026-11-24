// The speed blur (see `speed_blur.rs`): under the bike, the road just
// ridden over, each pixel of a rider's camera's part of the picture as the
// average of a few samples taken from it towards where the road ahead
// vanishes, the lower and the faster, the further apart; over the bike and
// out to the sides it stays sharp, and the rest of the picture is copied as
// it is. Only the road: what stands over it (another bike, by its depth)
// stays sharp, and is left out of the samples of the road round it.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;

struct SpeedBlur {
    // The camera's part of the picture: its corner and its size (pixels).
    viewport: vec4<f32>,
    // Where the road ahead vanishes, and where the bike is, across and
    // down the viewport (0 to 1).
    center: vec2<f32>,
    keep: vec2<f32>,
    // How much it blurs (0 to 1), and the viewport's width over its height.
    strength: f32,
    aspect: f32,
    padding: vec2<f32>,
    // From the camera's clip space to the world, and the road under the
    // bike as a plane: the way up off it, and how far along that it is.
    world_from_clip: mat4x4<f32>,
    road: vec4<f32>,
}

@group(0) @binding(2) var<uniform> blur: SpeedBlur;

#ifdef DEPTH
#ifdef DEPTH_MULTISAMPLED
@group(0) @binding(3) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(3) var depth: texture_depth_2d;
#endif
#endif

// How many samples each pixel is the average of.
const SAMPLES: i32 = 10;
// How far towards the vanishing point the samples reach at the most, a
// share of the way there.
const REACH: f32 = 0.12;
// How far out to either side of the bike (a share of the viewport's
// width) it blurs all it does, and from where on it stays sharp.
const WIDE: f32 = 0.28;
const WIDE_EDGE: f32 = 0.46;
// How far round the bike (a share of the viewport's height) it stays
// sharp, and where it blurs as the rest under it.
const KEEP: f32 = 0.08;
const KEEP_EDGE: f32 = 0.24;
// How high over the road's plane under the bike (m) what is drawn may be
// and still blur as the road, and from where on it stays sharp.
const OVER_ROAD: f32 = 0.15;
const OVER_ROAD_EDGE: f32 = 0.35;

// How much of what is drawn at `at` (across and down the viewport, 0 to 1)
// is not road: 0 on the road, 1 standing over it (or the sky). All road
// where the depth cannot be read.
fn off_road(at: vec2<f32>, size: vec2<f32>) -> f32 {
#ifdef DEPTH
    let depth_size = vec2<f32>(textureDimensions(depth));
    let uv = (blur.viewport.xy + at * blur.viewport.zw) / size;
    let pixel = vec2<i32>(min(uv * depth_size, depth_size - 1.0));
    let z = textureLoad(depth, pixel, 0);
    // The sky, as far as it goes (Bevy's depth is 1 near, 0 at infinity).
    if z <= 0.0 {
        return 1.0;
    }
    let clip = vec4(at.x * 2.0 - 1.0, 1.0 - at.y * 2.0, z, 1.0);
    let world = blur.world_from_clip * clip;
    let height = dot(world.xyz / world.w, blur.road.xyz) - blur.road.w;
    return smoothstep(OVER_ROAD, OVER_ROAD_EDGE, height);
#else
    return 0.0;
#endif
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let color = textureSampleLevel(screen, screen_sampler, in.uv, 0.0);
    let local = (in.position.xy - blur.viewport.xy) / blur.viewport.zw;
    if any(local < vec2(0.0)) || any(local > vec2(1.0)) {
        return color;
    }
    let shape = vec2(blur.aspect, 1.0);
    let bike = length((local - blur.keep) * shape);
    // Only under the bike, more the lower, all of it at the bottom; less
    // out to the sides, none at the edges.
    let under = smoothstep(blur.keep.y, 1.0, local.y);
    let side = 1.0 - smoothstep(WIDE, WIDE_EDGE, abs(local.x - blur.keep.x));
    var amount = blur.strength * under * side * smoothstep(KEEP, KEEP_EDGE, bike);
    if amount < 0.002 {
        return color;
    }
    let size = vec2<f32>(textureDimensions(screen));
    amount *= 1.0 - off_road(local, size);
    if amount < 0.002 {
        return color;
    }
    let step = (blur.center - local) * (amount * REACH / f32(SAMPLES - 1));
    // The samples of the road only, each as much as it is road.
    var sum = color.rgb;
    var weight = 1.0;
    for (var i = 1; i < SAMPLES; i += 1) {
        let at = clamp(local + step * f32(i), vec2(0.0), vec2(1.0));
        let uv = (blur.viewport.xy + at * blur.viewport.zw) / size;
        let road = 1.0 - off_road(at, size);
        sum += textureSampleLevel(screen, screen_sampler, uv, 0.0).rgb * road;
        weight += road;
    }
    return vec4(sum / weight, color.a);
}
