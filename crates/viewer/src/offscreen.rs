//! Offscreen terrain rendering to an image file (native only, no window or display).
//!
//! The terrain is drawn with the same pipelines as the window path ([`crate::terrain_gpu`]) into
//! an RGBA texture, which is read back and encoded as PNG. This is how a map is checked on a
//! machine without a display: `viewer --map map.w3x --mpq ... --render-png out.png`.
//!
//! Nothing here is bit-exact or game-accurate: it is presentation, like the window path.

use std::io::Write;
use std::path::Path;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::terrain::TerrainScene;
use crate::terrain_camera::TerrainCamera;
use crate::terrain_gpu::TerrainGpu;

/// Colour format of the target. Non-sRGB, like the window path: texel values are written as they
/// are.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Same clear colour as the window renderer.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.09,
    g: 0.10,
    b: 0.12,
    a: 1.0,
};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
}

/// Renders `terrain` from the default camera at `width x height` and returns tightly packed
/// RGBA8 rows, top row first.
pub fn render_terrain(terrain: &TerrainScene, width: u32, height: u32) -> Result<Vec<u8>, String> {
    render_terrain_at(terrain, width, height, None, 1.0)
}

/// Renders `terrain` from a camera position (optional) at `width x height` and returns tightly
/// packed RGBA8 rows, top row first. If `camera_target` is Some, the camera looks at that
/// world point with the default angles; otherwise the default camera is used. `zoom` divides
/// the camera distance (1 is the default framing, 4 is four times closer).
pub fn render_terrain_at(
    terrain: &TerrainScene,
    width: u32,
    height: u32,
    camera_target: Option<[f32; 3]>,
    zoom: f32,
) -> Result<Vec<u8>, String> {
    render_terrain_hud(terrain, width, height, camera_target, zoom, None)
}

/// As [`render_terrain_at`], with an optional UI layer drawn over the terrain in the same pass
/// (`viewer --hud`). The UI covers the whole image: its window is the viewport, and its transparent
/// parts show the terrain.
pub fn render_terrain_hud(
    terrain: &TerrainScene,
    width: u32,
    height: u32,
    camera_target: Option<[f32; 3]>,
    zoom: f32,
    hud: Option<&crate::ui::UiScene>,
) -> Result<Vec<u8>, String> {
    let (width, height) = (width.max(1), height.max(1));
    pollster::block_on(render_terrain_async(terrain, width, height, camera_target, zoom, hud))
}

/// Brings the camera `zoom` times closer to its target (the clip planes follow the distance).
pub fn zoomed(mut camera: TerrainCamera, zoom: f32) -> TerrainCamera {
    let zoom = if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 };
    camera.distance /= zoom;
    camera.near = camera.distance * 0.02;
    camera
}

async fn request_device(
    instance: &wgpu::Instance,
    force_fallback: bool,
) -> Result<(wgpu::Device, wgpu::Queue), String> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: force_fallback,
            compatible_surface: None,
            apply_limit_buckets: false,
        })
        .await
        .map_err(|e| format!("no adapter (fallback: {force_fallback}): {e}"))?;
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("offscreen device"),
            required_limits: adapter.limits(),
            ..Default::default()
        })
        .await
        .map_err(|e| format!("cannot create device: {e}"))
}

async fn render_terrain_async(
    terrain: &TerrainScene,
    width: u32,
    height: u32,
    camera_target: Option<[f32; 3]>,
    zoom: f32,
    hud: Option<&crate::ui::UiScene>,
) -> Result<Vec<u8>, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    // A hardware adapter if there is one, otherwise the software fallback (WARP on Windows).
    let (device, queue) = match request_device(&instance, false).await {
        Ok(pair) => pair,
        Err(first) => request_device(&instance, true)
            .await
            .map_err(|second| format!("{first}; {second}"))?,
    };

    let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("uniforms layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    let camera = if let Some([x, y, z]) = camera_target {
        // Position camera at world XY with fixed height and angle
        let mut cam = TerrainCamera::fit(&terrain.bounds, width as f32 / height as f32);
        cam.target = glam::Vec3::new(x, y, z);
        zoomed(cam, zoom)
    } else {
        zoomed(TerrainCamera::fit(&terrain.bounds, width as f32 / height as f32), zoom)
    };
    let uniforms = Uniforms {
        view_proj: camera.view_projection().to_cols_array_2d(),
    };
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("offscreen uniforms"),
        contents: bytemuck::bytes_of(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("offscreen uniforms"),
        layout: &uniform_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });

    let mut terrain_gpu = TerrainGpu::new(&device, FORMAT, &uniform_layout, &texture_layout);
    terrain_gpu.upload(
        &device,
        &queue,
        &texture_layout,
        &terrain.mesh,
        &terrain.atlas,
        &terrain.props,
    );

    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen color"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen depth"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

    // Rows read back from a texture are padded to 256 bytes.
    let row_bytes = width * 4;
    let padded =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("offscreen readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("offscreen frame"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("offscreen terrain pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        terrain_gpu.draw(&mut pass, &uniform_bind_group);
    }
    if let Some(ui_scene) = hud {
        // Same colour target, loaded (not cleared), no depth: the UI is alpha-blended over the terrain.
        let ui_gpu = crate::ui::gpu::UiGpu::new(
            &device,
            &queue,
            FORMAT,
            &uniform_layout,
            &texture_layout,
            ui_scene,
            (width, height),
        )?;
        let t = ui_gpu.text_stats;
        eprintln!(
            "ui: text: {} runs drawn, {} skipped, {} glyph quads, {} distinct glyphs, {} lines",
            t.runs_drawn, t.runs_skipped, t.glyphs, t.distinct_glyphs, t.lines
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("hud pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &uniform_bind_group, &[]);
        ui_gpu.render(&mut pass);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &color,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        extent,
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("device poll failed: {e}"))?;
    receiver
        .recv()
        .map_err(|e| format!("readback channel closed: {e}"))?
        .map_err(|e| format!("readback failed: {e}"))?;

    let mapped = slice
        .get_mapped_range()
        .map_err(|e| format!("mapped range: {e}"))?;
    let mut rgba = Vec::with_capacity((row_bytes * height) as usize);
    for row in 0..height as usize {
        let start = row * padded as usize;
        rgba.extend_from_slice(&mapped[start..start + row_bytes as usize]);
    }
    drop(mapped);
    readback.unmap();
    Ok(rgba)
}

/// What an image looks like, for checking that a render is not blank.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub width: u32,
    pub height: u32,
    /// Number of different RGB colours.
    pub distinct_colors: usize,
    /// Mean of R, G and B over all pixels, 0..=255.
    pub mean_rgb: [f64; 3],
    /// Pixel counts by Rec. 601 luma, eight bins of 32 each.
    pub luma_histogram: [u64; 8],
}

/// Computes a [`Summary`] of tightly packed RGBA8 pixels (alpha is ignored).
pub fn summarize(width: u32, height: u32, rgba: &[u8]) -> Summary {
    let mut colors = std::collections::HashSet::new();
    let mut sum = [0u64; 3];
    let mut histogram = [0u64; 8];
    let mut count = 0u64;
    for px in rgba.chunks_exact(4) {
        colors.insert([px[0], px[1], px[2]]);
        for c in 0..3 {
            sum[c] += u64::from(px[c]);
        }
        let luma = 0.299 * f64::from(px[0]) + 0.587 * f64::from(px[1]) + 0.114 * f64::from(px[2]);
        histogram[((luma as usize) / 32).min(7)] += 1;
        count += 1;
    }
    let denom = count.max(1) as f64;
    Summary {
        width,
        height,
        distinct_colors: colors.len(),
        mean_rgb: sum.map(|s| s as f64 / denom),
        luma_histogram: histogram,
    }
}

/// Encodes tightly packed RGBA8 pixels as a PNG file image.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let row = width as usize * 4;
    if rgba.len() != row * height as usize {
        return Err(format!(
            "{} bytes is not a {width}x{height} RGBA image",
            rgba.len()
        ));
    }
    // Scanlines, each prefixed by filter type 0 (none).
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgba.chunks_exact(row.max(1)) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    zlib.write_all(&raw).map_err(|e| e.to_string())?;
    let idat = zlib.finish().map_err(|e| e.to_string())?;

    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // 8-bit depth, colour type 6 (RGBA), deflate, adaptive filter method 0, no interlace.
    header.extend_from_slice(&[8, 6, 0, 0, 0]);

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    push_chunk(&mut out, b"IHDR", &header);
    push_chunk(&mut out, b"IDAT", &idat);
    push_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = flate2::Crc::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.sum().to_be_bytes());
}

/// Writes tightly packed RGBA8 pixels to a PNG file.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let bytes = encode_png(width, height, rgba)?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Renders a UI scene to RGBA8 pixels at the given resolution.
pub fn render_ui(ui_scene: &crate::ui::UiScene, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let (width, height) = (width.max(1), height.max(1));
    pollster::block_on(render_ui_async(ui_scene, width, height))
}

async fn render_ui_async(
    ui_scene: &crate::ui::UiScene,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let (device, queue) = match request_device(&instance, false).await {
        Ok(pair) => pair,
        Err(first) => request_device(&instance, true)
            .await
            .map_err(|second| format!("{first}; {second}"))?,
    };

    let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ui uniforms layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ui texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    // Create a dummy uniform buffer (UI doesn't use uniforms currently, but pipeline requires it)
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    struct Uniforms {
        _unused: [u32; 4],
    }
    let uniforms = Uniforms { _unused: [0; 4] };
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ui uniforms"),
        contents: bytemuck::bytes_of(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ui uniforms bind group"),
        layout: &uniform_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });

    let ui_gpu = crate::ui::gpu::UiGpu::new(
        &device,
        &queue,
        FORMAT,
        &uniform_layout,
        &texture_layout,
        ui_scene,
        (width, height),
    )?;
    let t = ui_gpu.text_stats;
    eprintln!(
        "ui: text: {} runs drawn, {} skipped, {} glyph quads, {} distinct glyphs, {} lines",
        t.runs_drawn, t.runs_skipped, t.glyphs, t.distinct_glyphs, t.lines
    );

    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui offscreen color"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());

    let row_bytes = width * 4;
    let padded = (row_bytes + 255) & !255;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ui offscreen readback"),
        size: (padded as u64) * (height as u64),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("ui offscreen encoder"),
    });

    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui offscreen pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &uniform_bind_group, &[]);
        ui_gpu.render(&mut pass);
    }

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &color,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        extent,
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("device poll failed: {e}"))?;
    receiver
        .recv()
        .map_err(|e| format!("readback channel closed: {e}"))?
        .map_err(|e| format!("readback failed: {e}"))?;

    let mapped = slice
        .get_mapped_range()
        .map_err(|e| format!("mapped range: {e}"))?;
    let mut rgba = Vec::with_capacity((row_bytes as u64 * height as u64) as usize);
    for row in 0..height as usize {
        let start = row * padded as usize;
        rgba.extend_from_slice(&mapped[start..start + row_bytes as usize]);
    }
    drop(mapped);
    readback.unmap();
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the chunks of a PNG and checks each CRC. Returns (kind, data) pairs.
    fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let mut pos = 8;
        let mut out = Vec::new();
        while pos < png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let kind: [u8; 4] = png[pos + 4..pos + 8].try_into().unwrap();
            let data = png[pos + 8..pos + 8 + len].to_vec();
            let crc = u32::from_be_bytes(png[pos + 8 + len..pos + 12 + len].try_into().unwrap());
            let mut expect = flate2::Crc::new();
            expect.update(&kind);
            expect.update(&data);
            assert_eq!(crc, expect.sum(), "CRC of {:?}", std::str::from_utf8(&kind));
            out.push((kind, data));
            pos += 12 + len;
        }
        out
    }

    #[test]
    fn png_has_a_valid_header_and_round_trips_pixels() {
        let (w, h) = (3u32, 2u32);
        let rgba: Vec<u8> = (0..(w * h * 4)).map(|i| (i * 7) as u8).collect();
        let png = encode_png(w, h, &rgba).unwrap();
        let parts = chunks(&png);
        let kinds: Vec<&[u8; 4]> = parts.iter().map(|(k, _)| k).collect();
        assert_eq!(kinds, [b"IHDR", b"IDAT", b"IEND"]);

        let ihdr = &parts[0].1;
        assert_eq!(u32::from_be_bytes(ihdr[0..4].try_into().unwrap()), w);
        assert_eq!(u32::from_be_bytes(ihdr[4..8].try_into().unwrap()), h);
        assert_eq!(&ihdr[8..13], &[8, 6, 0, 0, 0]);

        // Inflate the IDAT and undo the filter bytes: the pixels come back unchanged.
        use std::io::Read;
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(&parts[1].1[..])
            .read_to_end(&mut raw)
            .unwrap();
        let row = w as usize * 4;
        assert_eq!(raw.len(), (row + 1) * h as usize);
        let mut back = Vec::new();
        for line in raw.chunks_exact(row + 1) {
            assert_eq!(line[0], 0, "filter type none");
            back.extend_from_slice(&line[1..]);
        }
        assert_eq!(back, rgba);
    }

    #[test]
    fn png_rejects_a_wrong_pixel_count() {
        assert!(encode_png(2, 2, &[0; 15]).is_err());
    }

    #[test]
    fn summary_counts_colours_means_and_luma_bins() {
        // Two black pixels and two white pixels.
        let rgba = [
            0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        ];
        let s = summarize(2, 2, &rgba);
        assert_eq!(s.distinct_colors, 2);
        assert_eq!(s.mean_rgb, [127.5, 127.5, 127.5]);
        assert_eq!(s.luma_histogram[0], 2);
        assert_eq!(s.luma_histogram[7], 2);
        assert_eq!(s.luma_histogram.iter().sum::<u64>(), 4);
    }

    #[test]
    fn solid_image_has_one_colour() {
        let rgba = [40u8, 80, 120, 255].repeat(16);
        let s = summarize(4, 4, &rgba);
        assert_eq!(s.distinct_colors, 1);
        assert_eq!(s.mean_rgb, [40.0, 80.0, 120.0]);
    }
}
