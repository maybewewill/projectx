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

// Blended layer: the texel's own alpha (transition mask) times the vertex weight.
@fragment
fn fs_blend(input: VertexOut) -> @location(0) vec4<f32> {
    let atlas_uv = input.cell.xy + input.uv * input.cell.zw;
    let texel = textureSampleLevel(atlas, atlas_sampler, atlas_uv, 0.0);
    return vec4<f32>(texel.rgb, texel.a * input.color.a);
}

@fragment
fn fs_flat(input: VertexOut) -> @location(0) vec4<f32> {
    return input.color;
}

// Model texture (props, see props.rs): the bound texture at the model UV, which may wrap.
// The vertex colour (the lit tint) multiplies the texel. Four variants, one per filter mode
// group of the layer (see `props::PropBlend`):
//   fs_model_opaque: filter mode none, the texel alpha is ignored.
//   fs_model:        filter mode transparent (cut-out): texels below alpha 0.5 are discarded.
//   fs_model_alpha:  filter mode blend: the texel alpha blends over the background.
//   fs_model_add:    filter mode additive: the texel adds to the background, weighted by alpha.
@fragment
fn fs_model_opaque(input: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSampleLevel(atlas, atlas_sampler, input.uv, 0.0);
    return vec4<f32>(texel.rgb * input.color.rgb, 1.0);
}

@fragment
fn fs_model(input: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSampleLevel(atlas, atlas_sampler, input.uv, 0.0);
    if (texel.a < 0.5) {
        discard;
    }
    return vec4<f32>(texel.rgb * input.color.rgb, 1.0);
}

@fragment
fn fs_model_alpha(input: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSampleLevel(atlas, atlas_sampler, input.uv, 0.0);
    return vec4<f32>(texel.rgb * input.color.rgb, texel.a);
}

@fragment
fn fs_model_add(input: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSampleLevel(atlas, atlas_sampler, input.uv, 0.0);
    return vec4<f32>(texel.rgb * input.color.rgb * texel.a, 1.0);
}
