// Skinned, unlit-or-simply-lit model layer. Bones arrive as averaged matrix groups in a
// uniform array (WebGL2 has no storage buffers in vertex shaders); the vertex carries the
// group index. Per-draw parameters come through a dynamic uniform offset.

struct Camera {
    view_proj: mat4x4<f32>,
};

struct Palette {
    mats: array<mat4x4<f32>, 256>,
};

struct Params {
    color: vec4<f32>,
    alpha_test: f32,
    unshaded: f32,
    pad0: f32,
    pad1: f32,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> palette: Palette;

@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(1) @binding(2) var<uniform> params: Params;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) group: u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    let m = palette.mats[v.group];
    var out: VsOut;
    out.clip = camera.view_proj * (m * vec4<f32>(v.position, 1.0));
    out.uv = v.uv;
    out.normal = (m * vec4<f32>(v.normal, 0.0)).xyz;
    return out;
}

@fragment
fn fs_main(f: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(tex, samp, f.uv);
    let c = texel * params.color;
    if (c.a < params.alpha_test) {
        discard;
    }
    // One directional light from above, so the model reads as 3D. Unshaded layers skip it.
    let light = normalize(vec3<f32>(0.3, -0.4, 0.85));
    let lambert = 0.45 + 0.55 * clamp(dot(normalize(f.normal), light), 0.0, 1.0);
    let shade = mix(lambert, 1.0, params.unshaded);
    return vec4<f32>(c.rgb * shade, c.a);
}
