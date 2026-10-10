// Textured, unlit. One texture for the whole model; the vertex color is not used.

struct Uniforms {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(1) @binding(0) var model_texture: texture_2d<f32>;
@group(1) @binding(1) var model_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_position = uniforms.view_proj * vec4<f32>(input.position, 1.0);
    out.uv = input.uv;
    return out;
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSample(model_texture, model_sampler, input.uv);
    // Alpha test (cutout), since triangles are drawn unsorted.
    if (color.a < 0.5) {
        discard;
    }
    return vec4<f32>(color.rgb, 1.0);
}
