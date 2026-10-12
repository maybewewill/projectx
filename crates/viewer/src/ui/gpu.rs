//! GPU rendering for FDF UI: converts draw list to vertex buffers and renders with wgpu.

use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

use crate::ui::UiScene;

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
    /// Vertex buffers for each texture used in the UI.
    vertex_buffers: HashMap<String, wgpu::Buffer>,
    /// Index buffers for each texture.
    index_buffers: HashMap<String, wgpu::Buffer>,
    /// Index counts for each texture.
    index_counts: HashMap<String, u32>,
    /// Bind groups for each texture.
    bind_groups: HashMap<String, wgpu::BindGroup>,
    /// Pipeline for rendering UI quads.
    pipeline: wgpu::RenderPipeline,
}

impl UiGpu {
    /// Creates GPU data for rendering the UI scene.
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        uniform_layout: &wgpu::BindGroupLayout,
        texture_layout: &wgpu::BindGroupLayout,
        ui_scene: &UiScene,
    ) -> Result<Self, String> {
        // Build vertex and index data from the draw list
        let mut texture_verts: HashMap<String, Vec<UiVertex>> = HashMap::new();
        let mut texture_indices: HashMap<String, Vec<u32>> = HashMap::new();

        for item in &ui_scene.draw_list.items {
            match item {
                fdf::DrawItem::Quad(quad) => {
                    // Convert UI space to normalized device coordinates
                    // UI space: origin bottom-left, x: 0..0.6, y: 0..0.6 (in aspect-ratio aware coords)
                    // NDC: origin center, x: -1..1, y: -1..1

                    let rect = &quad.rect;
                    let screen = &ui_scene.screen;

                    // Normalize to screen coordinates
                    let left = (rect.left - screen.window.left) / screen.window.width();
                    let right = (rect.right - screen.window.left) / screen.window.width();
                    let bottom = (rect.bottom - screen.window.bottom) / screen.window.height();
                    let top = (rect.top - screen.window.bottom) / screen.window.height();

                    // Convert to NDC
                    let ndc_left = left * 2.0 - 1.0;
                    let ndc_right = right * 2.0 - 1.0;
                    let ndc_bottom = bottom * 2.0 - 1.0;
                    let ndc_top = top * 2.0 - 1.0;

                    let uv = &quad.uv;
                    let color = [1.0, 1.0, 1.0, 1.0]; // White; will blend with texture

                    // Get or create vertex/index list for this texture
                    let verts = texture_verts.entry(quad.texture.clone()).or_insert_with(Vec::new);
                    let indices = texture_indices.entry(quad.texture.clone()).or_insert_with(Vec::new);

                    let base_idx = verts.len() as u32;

                    // Quad vertices (bottom-left, bottom-right, top-right, top-left)
                    verts.push(UiVertex {
                        pos: [ndc_left, ndc_bottom],
                        uv: [uv[0], uv[2]],
                        color,
                    });
                    verts.push(UiVertex {
                        pos: [ndc_right, ndc_bottom],
                        uv: [uv[1], uv[2]],
                        color,
                    });
                    verts.push(UiVertex {
                        pos: [ndc_right, ndc_top],
                        uv: [uv[1], uv[3]],
                        color,
                    });
                    verts.push(UiVertex {
                        pos: [ndc_left, ndc_top],
                        uv: [uv[0], uv[3]],
                        color,
                    });

                    // Two triangles
                    indices.push(base_idx);
                    indices.push(base_idx + 1);
                    indices.push(base_idx + 2);
                    indices.push(base_idx);
                    indices.push(base_idx + 2);
                    indices.push(base_idx + 3);
                }
                fdf::DrawItem::Text(_text_run) => {
                    // Text rendering is a placeholder: draw a solid box
                    // TODO: implement proper text rendering with a bitmap font
                }
            }
        }

        // Create buffers for each texture
        let mut vertex_buffers = HashMap::new();
        let mut index_buffers = HashMap::new();
        let mut index_counts = HashMap::new();

        for (tex_name, verts) in texture_verts.iter() {
            if verts.is_empty() {
                continue;
            }
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("ui vertex buffer {}", tex_name)),
                contents: bytemuck::cast_slice(verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            vertex_buffers.insert(tex_name.clone(), vertex_buffer);

            if let Some(indices) = texture_indices.get(tex_name) {
                let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("ui index buffer {}", tex_name)),
                    contents: bytemuck::cast_slice(indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                index_counts.insert(tex_name.clone(), indices.len() as u32);
                index_buffers.insert(tex_name.clone(), index_buffer);
            }
        }

        // Note: wgpu texture creation with persistence is complex. For now, we skip texture
        // binding and use a placeholder. TODO: implement persistent texture storage.
        let bind_groups = HashMap::new();
        // Empty for now - textures would be stored in a more persistent structure

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
            vertex_buffers,
            index_buffers,
            index_counts,
            bind_groups,
            pipeline,
        })
    }

    /// Renders the UI to the given render pass.
    pub fn render(&self, render_pass: &mut wgpu::RenderPass) {
        render_pass.set_pipeline(&self.pipeline);

        for (tex_name, vertex_buffer) in &self.vertex_buffers {
            if let (Some(index_buffer), Some(index_count), Some(bind_group)) = (
                self.index_buffers.get(tex_name),
                self.index_counts.get(tex_name),
                self.bind_groups.get(tex_name),
            ) {
                render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.set_bind_group(1, bind_group, &[]);
                render_pass.draw_indexed(0..*index_count, 0, 0..1);
            }
        }
    }
}
