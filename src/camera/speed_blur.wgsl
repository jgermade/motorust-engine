// The speed blur (see `speed_blur.rs`): each pixel of a rider's camera's
// part of the picture as the average of a few samples taken from it
// towards where the road ahead vanishes, the further out and the faster,
// the further apart; the rest of the picture copied as it is.

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
// From how far out from the vanishing point (a share of the viewport's
// height) it starts to blur, and where it blurs all it does.
const INNER: f32 = 0.18;
const OUTER: f32 = 0.75;
// How far round the bike (the same share) it stays sharp, and where it
// blurs as the rest.
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
    let out = length((local - blur.center) * shape);
    let bike = length((local - blur.keep) * shape);
    let amount = blur.strength * smoothstep(INNER, OUTER, out)
        * smoothstep(KEEP, KEEP_EDGE, bike);
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
