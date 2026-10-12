//! GPU rendering for FDF UI: converts the draw list to one vertex and index stream in paint order
//! and draws it in batches, one per texture. Text runs become glyph quads from the atlas built by
//! `ui::text`, so they are drawn by the same pipeline and tinted by their colour.

use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

use crate::ui::text::{self, TextQuad, TextStats};
use crate::ui::UiScene;

/// Texture key of the glyph atlas. It starts with a control character, so no FDF path can clash.
const TEXT_ATLAS_KEY: &str = "\u{1}glyph atlas";

/// A single vertex for UI rendering (position + UV + color).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct UiVertex {
    pub pos: [f32; 2],      // Normalized screen coordinates
    pub uv: [f32; 2],       // Texture coordinates
    pub color: [f32; 4],    // RGBA
}

/// Compiled GPU data for rendering a UI scene.
pub struct UiGpu {
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    /// (texture key, first index, index count), in paint order.
    batches: Vec<(String, u32, u32)>,
    /// Textures kept alive while the bind groups refer to them.
    _textures: HashMap<String, wgpu::Texture>,
    /// Bind group (texture + sampler) for each texture key, including the glyph atlas.
    bind_groups: HashMap<String, wgpu::BindGroup>,
    /// Pipeline for rendering UI quads.
    pipeline: wgpu::RenderPipeline,
    /// Text counts for the report.
    pub text_stats: TextStats,
}

/// Collects quads in paint order. Consecutive quads with the same texture share one batch.
#[derive(Default)]
struct Batcher {
    verts: Vec<UiVertex>,
    indices: Vec<u32>,
    batches: Vec<(String, u32, u32)>,
}

impl Batcher {
    /// Adds one quad. Corners are bottom-left, bottom-right, top-right, top-left in NDC, with
    /// the matching UVs.
    fn quad(&mut self, key: &str, corners: [[f32; 2]; 4], uvs: [[f32; 2]; 4], color: [f32; 4]) {
        let base = self.verts.len() as u32;
        for (pos, uv) in corners.into_iter().zip(uvs) {
            self.verts.push(UiVertex { pos, uv, color });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        match self.batches.last_mut() {
            Some((last, _, count)) if last == key => *count += 6,
            _ => {
                let start = self.indices.len() as u32 - 6;
                self.batches.push((key.to_string(), start, 6));
            }
        }
    }

    /// Adds a glyph quad given in window pixels (y down) for a viewport of `vw` x `vh` pixels.
    fn text_quad(&mut self, q: &TextQuad, vw: f32, vh: f32) {
        let [left, top, right, bottom] = q.rect_px;
        let x = |px: f32| px / vw * 2.0 - 1.0;
        let y = |py: f32| 1.0 - py / vh * 2.0;
        let [u0, v0, u1, v1] = q.uv;
        self.quad(
            TEXT_ATLAS_KEY,
            [[x(left), y(bottom)], [x(right), y(bottom)], [x(right), y(top)], [x(left), y(top)]],
            [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
            q.color,
        );
    }
}

/// Uploads RGBA8 pixels as a texture and returns it with a bind group (texture + sampler).
fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture_layout: &wgpu::BindGroupLayout,
    name: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> (wgpu::Texture, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("ui texture {}", name)),
        size: wgpu::Extent3d {
            width,
            height,
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
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(&format!("ui sampler {}", name)),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("ui bind group {}", name)),
        layout: texture_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    (texture, bind_group)
}

impl UiGpu {
    /// Creates GPU data for rendering the UI scene into a `viewport` of the given pixel size.
    /// Text is laid out for that size, so the glyphs are rasterised at the matching pixel size.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        uniform_layout: &wgpu::BindGroupLayout,
        texture_layout: &wgpu::BindGroupLayout,
        ui_scene: &UiScene,
        viewport: (u32, u32),
    ) -> Result<Self, String> {
        let window = ui_scene.screen.window;
        let (vw, vh) = (viewport.0.max(1) as f32, viewport.1.max(1) as f32);

        let text_runs: Vec<&fdf::TextRun> = ui_scene
            .draw_list
            .items
            .iter()
            .filter_map(|item| match item {
                fdf::DrawItem::Text(run) => Some(run),
                _ => None,
            })
            .collect();
        let atlas = text::build_atlas(&text_runs, &ui_scene.fonts, window, viewport);

        // Quads and text in paint order (the draw list is already in paint order).
        let mut batcher = Batcher::default();
        let mut text_index = 0usize;
        for item in &ui_scene.draw_list.items {
            match item {
                fdf::DrawItem::Quad(quad) => {
                    // UI space: origin bottom-left, window = 0..0.6 high; NDC is -1..1 on both axes.
                    let rect = &quad.rect;
                    let left = (rect.left - window.left) / window.width();
                    let right = (rect.right - window.left) / window.width();
                    let bottom = (rect.bottom - window.bottom) / window.height();
                    let top = (rect.top - window.bottom) / window.height();
                    let (ndc_left, ndc_right) = (left * 2.0 - 1.0, right * 2.0 - 1.0);
                    let (ndc_bottom, ndc_top) = (bottom * 2.0 - 1.0, top * 2.0 - 1.0);
                    let uv = quad.uv;
                    batcher.quad(
                        &quad.texture,
                        [
                            [ndc_left, ndc_bottom],
                            [ndc_right, ndc_bottom],
                            [ndc_right, ndc_top],
                            [ndc_left, ndc_top],
                        ],
                        // TexCoord is (left, right, top, bottom) and texture rows run top to bottom, so the
                        // bottom corners take the bottom texcoord.
                        [[uv[0], uv[3]], [uv[1], uv[3]], [uv[1], uv[2]], [uv[0], uv[2]]],
                        [1.0, 1.0, 1.0, 1.0],
                    );
                }
                fdf::DrawItem::Text(_) => {
                    for glyph in &atlas.runs[text_index] {
                        batcher.text_quad(glyph, vw, vh);
                    }
                    text_index += 1;
                }
            }
        }

        let (vertex_buffer, index_buffer) = if batcher.indices.is_empty() {
            (None, None)
        } else {
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ui vertex buffer"),
                contents: bytemuck::cast_slice(&batcher.verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ui index buffer"),
                contents: bytemuck::cast_slice(&batcher.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            (Some(vertex_buffer), Some(index_buffer))
        };

        // One texture per FDF texture, plus the glyph atlas.
        let mut uploads: Vec<(&str, u32, u32, &[u8])> = ui_scene
            .textures
            .iter()
            .map(|(name, t)| (name.as_str(), t.width, t.height, t.rgba.as_slice()))
            .collect();
        uploads.push((TEXT_ATLAS_KEY, atlas.width, atlas.height, atlas.rgba.as_slice()));
        let mut _textures = HashMap::new();
        let mut bind_groups = HashMap::new();
        for (name, width, height, rgba) in uploads {
            let (texture, bind_group) = upload_texture(device, queue, texture_layout, name, width, height, rgba);
            _textures.insert(name.to_string(), texture);
            bind_groups.insert(name.to_string(), bind_group);
        }

        // Create render pipeline
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ui.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui pipeline layout"),
            bind_group_layouts: &[Some(uniform_layout), Some(texture_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<UiVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,  // pos
                        1 => Float32x2,  // uv
                        2 => Float32x4,  // color
                    ],
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        Ok(UiGpu {
            vertex_buffer,
            index_buffer,
            batches: batcher.batches,
            _textures,
            bind_groups,
            pipeline,
            text_stats: atlas.stats,
        })
    }

    /// Renders the UI to the given render pass, one batch per texture in paint order.
    pub fn render(&self, render_pass: &mut wgpu::RenderPass) {
        let (Some(vertex_buffer), Some(index_buffer)) = (&self.vertex_buffer, &self.index_buffer) else {
            return;
        };
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for (key, start, count) in &self.batches {
            if let Some(bind_group) = self.bind_groups.get(key) {
                render_pass.set_bind_group(1, bind_group, &[]);
                render_pass.draw_indexed(*start..*start + *count, 0, 0..1);
            }
        }
    }
}
