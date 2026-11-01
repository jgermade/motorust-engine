// The lawn tiled stochastically (Options > Graphics > Grass tiling): the
// ground cut into a grid of triangles, each corner reading the lawn shifted
// by a random amount of its own, the three pieces of each triangle blended
// round the lawn's average colour so that the blend keeps its contrast
// (after Heitz and Neyret, "High-Performance By-Example Noise using a
// Histogram-Preserving Blending Operator", 2018). The lawn never repeats.
// Its normal map read the same way. Otherwise the lawn's own material (see
// src/world/lawn.rs).

#import bevy_pbr::{
    forward_io::{FragmentOutput, VertexOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
}

// Must match LawnLook in src/world/lawn.rs.
struct LawnLook {
    // The lawn's average colour (linear).
    mean: vec4<f32>,
    // 1 with the lawn's normal map, 0 without.
    normals: f32,
    // How many triangles of the grid across the lawn.
    cells: f32,
    // How sharply the pieces meet: the power their weights are raised to.
    sharp: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> look: LawnLook;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var lawn_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var lawn_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var lawn_normals: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var lawn_normals_sampler: sampler;

// Two random numbers from two (Jarzynski and Olano's PCG 2D hash, as
// src/world/lawn.rs has it), from 0 to 1.
fn pcg2d(p: vec2<i32>) -> vec2<f32> {
    var v = bitcast<vec2<u32>>(p) * 1664525u + 1013904223u;
    v.x += v.y * 1664525u;
    v.y += v.x * 1664525u;
    v = v ^ (v >> vec2(16u));
    v.x += v.y * 1664525u;
    v.y += v.x * 1664525u;
    v = v ^ (v >> vec2(16u));
    return vec2<f32>(v) * (1.0 / 4294967296.0);
}

// The triangle of the grid at `uv`: its three corners and their weights.
struct Triangle {
    a: vec2<i32>,
    b: vec2<i32>,
    c: vec2<i32>,
    weights: vec3<f32>,
}

fn triangle_at(uv: vec2<f32>) -> Triangle {
    // Equilateral triangles: the grid skewed.
    let p = uv * look.cells;
    let skewed = vec2(p.x, -0.57735027 * p.x + 1.15470054 * p.y);
    let base = vec2<i32>(floor(skewed));
    let f = fract(skewed);
    let z = 1.0 - f.x - f.y;
    var triangle: Triangle;
    if z > 0.0 {
        triangle.a = base;
        triangle.b = base + vec2(0, 1);
        triangle.c = base + vec2(1, 0);
        triangle.weights = vec3(z, f.y, f.x);
    } else {
        triangle.a = base + vec2(1, 1);
        triangle.b = base + vec2(1, 0);
        triangle.c = base + vec2(0, 1);
        triangle.weights = vec3(-z, 1.0 - f.y, 1.0 - f.x);
    }
    // Sharpened, adding up to 1 again.
    let raised = pow(max(triangle.weights, vec3(0.0)), vec3(look.sharp));
    triangle.weights = raised / max(raised.x + raised.y + raised.z, 1e-6);
    return triangle;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

#ifdef VERTEX_UVS_A
    let uv = in.uv;
    // The pieces are read as the lawn itself would be, with its mipmaps
    // and its filtering: their shifts change nothing of that.
    let dx = dpdx(uv);
    let dy = dpdy(uv);
    let triangle = triangle_at(uv);
    let at_a = uv + pcg2d(triangle.a);
    let at_b = uv + pcg2d(triangle.b);
    let at_c = uv + pcg2d(triangle.c);
    let w = triangle.weights;

    let mean = look.mean.rgb;
    let offset = w.x * (textureSampleGrad(lawn_texture, lawn_sampler, at_a, dx, dy).rgb - mean)
        + w.y * (textureSampleGrad(lawn_texture, lawn_sampler, at_b, dx, dy).rgb - mean)
        + w.z * (textureSampleGrad(lawn_texture, lawn_sampler, at_c, dx, dy).rgb - mean);
    let color = max(mean + offset / sqrt(dot(w, w)), vec3(0.0));
    // Over the material's own colour and the ground's tint.
    pbr_input.material.base_color = vec4(
        pbr_input.material.base_color.rgb * color,
        pbr_input.material.base_color.a,
    );

#ifdef VERTEX_TANGENTS
    if look.normals > 0.5 {
        // The normal maps' slopes blended (two channels: x along u, y up
        // the texture), as the lawn's relief has them.
        let slopes = w.x * (textureSampleGrad(lawn_normals, lawn_normals_sampler, at_a, dx, dy).rg * 2.0 - 1.0)
            + w.y * (textureSampleGrad(lawn_normals, lawn_normals_sampler, at_b, dx, dy).rg * 2.0 - 1.0)
            + w.z * (textureSampleGrad(lawn_normals, lawn_normals_sampler, at_c, dx, dy).rg * 2.0 - 1.0);
        let nt = vec3(slopes, sqrt(max(1.0 - dot(slopes, slopes), 0.0)));
        // As Bevy's own normal mapping (mikktspace): the normal as it comes
        // and the tangent, the bitangent across them. (The lawn's materials
        // are one-sided: only their fronts are drawn.)
        let n = in.world_normal;
        let t = in.world_tangent.xyz;
        let b = in.world_tangent.w * cross(n, t);
        pbr_input.N = normalize(nt.x * t + nt.y * b + nt.z * n);
    }
#endif
#endif

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
