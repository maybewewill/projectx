//! GPU side of an animated model. One pipeline per blend mode; the matrix palette is a uniform
//! array; per-draw parameters are read through a dynamic offset into one buffer.
//!
//! Not run here (no GPU in the build environment). The CPU side it consumes is unit tested.

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

use super::asset::{ModelAsset, MAX_PALETTE};
use super::draw::{BlendMode, ModelFrame};

const SHADER: &str = include_str!("skinned.wgsl");
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Byte stride of per-draw parameters: a multiple of every backend's dynamic offset alignment.
const PARAM_STRIDE: usize = 256;
/// Draws per frame. Extra draws are skipped.
pub const MAX_DRAWS: usize = 1024;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    group: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    color: [f32; 4],
    alpha_test: f32,
    unshaded: f32,
    pad: [f32; 2],
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

/// GPU objects for one model. Created once per model; drawn every frame.
pub struct ModelGpu {
    /// Indexed by `BlendMode::index`.
    pipelines: Vec<wgpu::RenderPipeline>,
    camera: wgpu::Buffer,
    palette: wgpu::Buffer,
    params: wgpu::Buffer,
    scene_group: wgpu::BindGroup,
    /// Indexed like `ModelAsset::textures`.
    texture_groups: Vec<wgpu::BindGroup>,
    /// Indexed like `ModelAsset::meshes`.
    meshes: Vec<GpuMesh>,
}

fn uniform_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    dynamic: bool,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        count: None,
    }
}

fn blend_state(mode: BlendMode) -> wgpu::BlendState {
    use wgpu::BlendFactor::{Dst, One, OneMinusSrcAlpha, SrcAlpha, Src, Zero};
    let component = |src_factor, dst_factor| wgpu::BlendComponent {
        src_factor,
        dst_factor,
        operation: wgpu::BlendOperation::Add,
    };
    let color = match mode {
        BlendMode::Opaque | BlendMode::AlphaTest => return wgpu::BlendState::REPLACE,
        BlendMode::Alpha => component(SrcAlpha, OneMinusSrcAlpha),
        BlendMode::Additive => component(SrcAlpha, One),
        BlendMode::AddAlpha => component(One, One),
        BlendMode::Modulate => component(Dst, Zero),
        BlendMode::Modulate2x => component(Dst, Src),
    };
    wgpu::BlendState {
        color,
        alpha: wgpu::BlendComponent::REPLACE,
    }
}

impl ModelGpu {
    /// Uploads the static parts of `asset`: meshes, textures and the pipelines.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        asset: &ModelAsset,
    ) -> ModelGpu {
        use wgpu::ShaderStages;

        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model scene layout"),
            entries: &[
                uniform_entry(0, ShaderStages::VERTEX, false),
                uniform_entry(1, ShaderStages::VERTEX, false),
            ],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                uniform_entry(2, ShaderStages::FRAGMENT, true),
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("model skinned shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("model skinned layout"),
            bind_group_layouts: &[Some(&scene_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let pipelines = BlendMode::ALL
            .iter()
            .map(|&mode| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("model skinned pipeline"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![
                                0 => Float32x3,
                                1 => Float32x3,
                                2 => Float32x2,
                                3 => Uint32
                            ],
                        })],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(blend_state(mode)),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    // WC3 meshes do not share one winding order, so nothing is culled.
                    primitive: wgpu::PrimitiveState {
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(mode.writes_depth()),
                        depth_compare: Some(wgpu::CompareFunction::Less),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .collect();

        let camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model camera"),
            size: std::mem::size_of::<[[f32; 4]; 4]>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let palette = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model palette"),
            size: (MAX_PALETTE * std::mem::size_of::<[[f32; 4]; 4]>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model draw params"),
            size: (MAX_DRAWS * PARAM_STRIDE) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scene_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model scene"),
            layout: &scene_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: palette.as_entire_binding(),
                },
            ],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("model sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let param_size = wgpu::BufferSize::new(std::mem::size_of::<Params>() as u64);
        let texture_groups = asset
            .textures
            .iter()
            .map(|slot| {
                let view = upload_texture(device, queue, &slot.data);
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("model texture"),
                    layout: &texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &params,
                                offset: 0,
                                size: param_size,
                            }),
                        },
                    ],
                })
            })
            .collect();

        let meshes = asset
            .meshes
            .iter()
            .map(|mesh| {
                let vertices: Vec<Vertex> = mesh
                    .positions
                    .iter()
                    .zip(&mesh.normals)
                    .zip(&mesh.uvs)
                    .zip(&mesh.groups)
                    .map(|(((&position, &normal), &uv), &group)| Vertex {
                        position,
                        normal,
                        uv,
                        group,
                    })
                    .collect();
                GpuMesh {
                    vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("model vertices"),
                        contents: bytemuck::cast_slice(&vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                    indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("model indices"),
                        contents: bytemuck::cast_slice(&mesh.indices),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                    index_count: mesh.indices.len() as u32,
                }
            })
            .collect();

        ModelGpu {
            pipelines,
            camera,
            palette,
            params,
            scene_group,
            texture_groups,
            meshes,
        }
    }

    /// Writes the camera, palette and per-draw parameters for `frame`.
    pub fn prepare(&self, queue: &wgpu::Queue, view_proj: Mat4, frame: &ModelFrame) {
        let identity = Mat4::IDENTITY.to_cols_array_2d();
        queue.write_buffer(
            &self.camera,
            0,
            bytemuck::bytes_of(&view_proj.to_cols_array_2d()),
        );
        let mut palette = vec![identity; MAX_PALETTE];
        for (slot, m) in palette.iter_mut().zip(&frame.palette) {
            *slot = m.to_cols_array_2d();
        }
        queue.write_buffer(&self.palette, 0, bytemuck::cast_slice(&palette));

        let draws = frame.draws.len().min(MAX_DRAWS);
        if draws == 0 {
            return;
        }
        let mut bytes = vec![0u8; draws * PARAM_STRIDE];
        for (i, d) in frame.draws.iter().take(draws).enumerate() {
            let p = Params {
                color: d.color,
                alpha_test: d.mode.alpha_threshold(),
                unshaded: if d.unshaded { 1.0 } else { 0.0 },
                pad: [0.0; 2],
            };
            let at = i * PARAM_STRIDE;
            bytes[at..at + std::mem::size_of::<Params>()].copy_from_slice(bytemuck::bytes_of(&p));
        }
        queue.write_buffer(&self.params, 0, &bytes);
    }

    /// Records the draws of `frame` into a pass that targets the surface and a depth buffer.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, frame: &ModelFrame) {
        pass.set_bind_group(0, &self.scene_group, &[]);
        for (i, d) in frame.draws.iter().take(MAX_DRAWS).enumerate() {
            let (Some(mesh), Some(texture), Some(pipeline)) = (
                self.meshes.get(d.mesh),
                self.texture_groups.get(d.texture),
                self.pipelines.get(d.mode.index()),
            ) else {
                continue;
            };
            pass.set_pipeline(pipeline);
            pass.set_bind_group(1, texture, &[(i * PARAM_STRIDE) as u32]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }
    }
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &crate::scene::TextureData,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("model texture"),
        size: wgpu::Extent3d {
            width: data.width,
            height: data.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &data.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(data.width * 4),
            rows_per_image: Some(data.height),
        },
        wgpu::Extent3d {
            width: data.width,
            height: data.height,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
