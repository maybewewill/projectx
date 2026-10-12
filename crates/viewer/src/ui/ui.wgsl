// FDF UI rendering shader: textured quads with blend modes

struct VertexInput {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vertex_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.pos, 0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

@group(1) @binding(0)
var ui_texture: texture_2d<f32>;
@group(1) @binding(1)
var ui_sampler: sampler;

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(ui_texture, ui_sampler, input.uv);
    let final_color = tex_color * input.color;
    return final_color;
}
