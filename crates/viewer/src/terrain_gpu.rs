//! wgpu side of the terrain: four pipelines (ground, cliffs, overlays, water) that share one
//! shader, one atlas texture, and the view-projection uniform of the model pass.
//!
//! Draw order and depth rules are what make the blending correct:
//! 1. `ground`: opaque, writes depth. Every tile's dominant texture.
//! 2. `cliffs`: opaque, writes depth.
//! 3. `overlays`: alpha-blended over the ground, depth-tested but not written (coplanar with
//!    the ground, so `LessEqual`).
//! 4. `water`: alpha-blended, depth-tested, not written.

use bytemuck::cast_slice;
use wgpu::util::DeviceExt;

use crate::terrain::{Atlas, Mesh, TerrainMesh, TerrainVertex};

const SHADER: &str = include_str!("terrain.wgsl");

/// Format of the atlas texture. Non-sRGB, like the model textures.
const ATLAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

impl GpuMesh {
    fn new(device: &wgpu::Device, label: &str, mesh: &Mesh) -> Option<GpuMesh> {
        if mesh.is_empty() {
            return None;
        }
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: cast_slice::<TerrainVertex, u8>(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: cast_slice::<u32, u8>(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Some(GpuMesh {
            vertices,
            indices,
            index_count: mesh.indices.len() as u32,
        })
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }
}

/// Everything the renderer needs for the terrain. Pipelines are created once; the meshes and
/// the atlas are replaced by [`TerrainGpu::upload`].
pub(crate) struct TerrainGpu {
    ground: wgpu::RenderPipeline,
    cliffs: wgpu::RenderPipeline,
    overlays: wgpu::RenderPipeline,
    water: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    content: Option<Content>,
}

struct Content {
    ground: Option<GpuMesh>,
    cliffs: Option<GpuMesh>,
    overlays: Option<GpuMesh>,
    water: Option<GpuMesh>,
    atlas: wgpu::BindGroup,
}

impl TerrainGpu {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        uniform_layout: &wgpu::BindGroupLayout,
        texture_layout: &wgpu::BindGroupLayout,
    ) -> TerrainGpu {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain pipeline layout"),
            bind_group_layouts: &[Some(uniform_layout), Some(texture_layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x3,
            1 => Float32x2,
            2 => Float32x4,
            3 => Float32x4
        ];

        // (label, fragment entry, blend, depth write, depth compare)
        let make = |label: &str,
                    fragment: &str,
                    blend: wgpu::BlendState,
                    depth_write: bool,
                    depth_compare: wgpu::CompareFunction| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<TerrainVertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(depth_compare),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        let opaque = wgpu::BlendState::REPLACE;
        let alpha = wgpu::BlendState::ALPHA_BLENDING;
        let ground = make(
            "terrain ground",
            "fs_textured",
            opaque,
            true,
            wgpu::CompareFunction::Less,
        );
        let cliffs = make(
            "terrain cliffs",
            "fs_flat",
            opaque,
            true,
            wgpu::CompareFunction::Less,
        );
        let overlays = make(
            "terrain overlays",
            "fs_blend",
            alpha,
            false,
            wgpu::CompareFunction::LessEqual,
        );
        let water = make(
            "terrain water",
            "fs_flat",
            alpha,
            false,
            wgpu::CompareFunction::LessEqual,
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("terrain atlas sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        TerrainGpu {
            ground,
            cliffs,
            overlays,
            water,
            sampler,
            content: None,
        }
    }

    /// Replaces the terrain meshes and the atlas.
    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture_layout: &wgpu::BindGroupLayout,
        mesh: &TerrainMesh,
        atlas: &Atlas,
    ) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain atlas"),
            size: wgpu::Extent3d {
                width: atlas.size,
                height: atlas.size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ATLAS_FORMAT,
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
            &atlas.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas.size * 4),
                rows_per_image: Some(atlas.size),
            },
            wgpu::Extent3d {
                width: atlas.size,
                height: atlas.size,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain atlas"),
            layout: texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        self.content = Some(Content {
            ground: GpuMesh::new(device, "terrain ground", &mesh.ground),
            cliffs: GpuMesh::new(device, "terrain cliffs", &mesh.cliffs),
            overlays: GpuMesh::new(device, "terrain overlays", &mesh.overlays),
            water: GpuMesh::new(device, "terrain water", &mesh.water),
            atlas: atlas_bind,
        });
    }

    /// Records the terrain draws into an open pass. `uniforms` is the view-projection bind group.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, uniforms: &wgpu::BindGroup) {
        let Some(content) = &self.content else {
            return;
        };
        pass.set_bind_group(0, uniforms, &[]);
        pass.set_bind_group(1, &content.atlas, &[]);
        if let Some(mesh) = &content.ground {
            pass.set_pipeline(&self.ground);
            mesh.draw(pass);
        }
        if let Some(mesh) = &content.cliffs {
            pass.set_pipeline(&self.cliffs);
            mesh.draw(pass);
        }
        if let Some(mesh) = &content.overlays {
            pass.set_pipeline(&self.overlays);
            mesh.draw(pass);
        }
        if let Some(mesh) = &content.water {
            pass.set_pipeline(&self.water);
            mesh.draw(pass);
        }
    }
}
