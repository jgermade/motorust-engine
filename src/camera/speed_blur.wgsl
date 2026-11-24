// The speed blur (see `speed_blur.rs`): under the bike, the road just
// ridden over, each pixel of a rider's camera's part of the picture as the
// average of a few samples taken from it towards where the road ahead
// vanishes, the lower and the faster, the further apart; over the bike and
// out to the sides it stays sharp, and the rest of the picture is copied as
// it is.

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
}

@group(0) @binding(2) var<uniform> blur: SpeedBlur;

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
    let amount = blur.strength * under * side * smoothstep(KEEP, KEEP_EDGE, bike);
    if amount < 0.002 {
        return color;
    }
    let size = vec2<f32>(textureDimensions(screen));
    let step = (blur.center - local) * (amount * REACH / f32(SAMPLES - 1));
    var sum = color.rgb;
    for (var i = 1; i < SAMPLES; i += 1) {
        let at = clamp(local + step * f32(i), vec2(0.0), vec2(1.0));
        let uv = (blur.viewport.xy + at * blur.viewport.zw) / size;
        sum += textureSampleLevel(screen, screen_sampler, uv, 0.0).rgb;
    }
    return vec4(sum / f32(SAMPLES), color.a);
}
