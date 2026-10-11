// Terrain: one vertex stage, two fragment stages.
//   fs_textured: samples the ground atlas at the tile's cell (tile-local uv mapped into the
//                cell), keeps the vertex alpha as the blend weight.
//   fs_flat:     the vertex colour, for cliff walls and water.

struct Uniforms {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) cell: vec4<f32>,
    @location(3) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) cell: vec4<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_position = uniforms.view_proj * vec4<f32>(input.position, 1.0);
    out.uv = input.uv;
    out.cell = input.cell;
    out.color = input.color;
    return out;
}

@fragment
fn fs_textured(input: VertexOut) -> @location(0) vec4<f32> {
    let atlas_uv = input.cell.xy + input.uv * input.cell.zw;
    let texel = textureSampleLevel(atlas, atlas_sampler, atlas_uv, 0.0);
    return vec4<f32>(texel.rgb, input.color.a);
}

@fragment
fn fs_flat(input: VertexOut) -> @location(0) vec4<f32> {
    return input.color;
}
