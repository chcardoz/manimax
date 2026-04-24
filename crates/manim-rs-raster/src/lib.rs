//! Manimax rasterizer — wgpu device, MSAA-resolved offscreen render target,
//! CPU readback, and stroke + fill pipelines.
//!
//! `Runtime::render` takes a `SceneState` (from `manim-rs-eval`) plus a
//! `Camera`, draws each object's fill and stroke through MSAA 4×, resolves
//! to a single-sample target, and returns tight RGBA bytes.
//!
//! Pipeline (top-down view of one `render(state, camera, bg)` call):
//!
//! ```text
//!   Runtime::render
//!     ├─ for each ObjectState in state.objects:
//!     │     tessellate_object        Object → ObjectDraw (fill mesh + stroke ribbon)
//!     │     build_mvp                projection · translate · rotate · scale
//!     │     render_one_object        upload_mesh → render pass → submit (per object)
//!     │       └─ draw_mesh           bind pipeline + buffers, draw_indexed
//!     ├─ copy_resolve_to_readback    MSAA resolve → padded readback buffer
//!     └─ readback_pixels             map → strip 256-byte row padding → RGBA Vec<u8>
//! ```
//!
//! Why one submit per object: `queue.write_buffer` is ordered before all
//! submitted command buffers, so a single submit would clobber the shared
//! vertex/index/uniform buffers — only the last object would survive.
//!
//! wgpu buffer-copy invariant: `bytes_per_row` must be a multiple of 256.
//! `readback_pixels` strips the padding before returning (a 480-wide RGBA row
//! is 1920 bytes, padded to 2048).
//!
//! Module map:
//! - [`camera`] — orthographic camera, fixed for Slice B.
//! - [`tessellator`] — path → quadratic stream + stroke ribbon expansion.
//! - [`pipelines`] — wgpu pipeline objects for stroke and fill.
//! - `render_object` (private) — per-object tessellation + paint resolution + MVP.
//! - `pipe_bundle` (private) — per-pipeline GPU buffers and the bind group.

pub mod camera;
pub mod pipelines;
pub mod tessellator;

mod pipe_bundle;
mod render_object;

pub use camera::Camera;
pub use tessellator::{QuadraticSegment, StrokeVertex, expand_stroke, sample_bezpath};

use bytemuck::cast_slice;
use manim_rs_eval::SceneState;

use crate::pipe_bundle::{PipeBundle, build_pipe_bundle, draw_mesh};
use crate::pipelines::path_fill::{FillPipeline, FillUniforms, UNIFORM_SIZE as FILL_UNIFORM_SIZE};
use crate::pipelines::path_stroke::{
    StrokePipeline, StrokeUniforms, UNIFORM_SIZE as STROKE_UNIFORM_SIZE,
};
use crate::render_object::{
    ObjectDraw, build_mvp, check_geometry_size, clear_load, tessellate_object,
};

/// Fragment-shader AA fade width in pixels. Matches manimgl's
/// `ANTI_ALIAS_WIDTH = 1.5`.
const ANTI_ALIAS_WIDTH: f32 = 1.5;

const COPY_BYTES_PER_ROW_ALIGNMENT: u32 = 256;
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// MSAA sample count. 4× is universally supported across wgpu backends and
/// gives clean sub-pixel coverage for the thin strokes Manimax renders.
pub const MSAA_SAMPLE_COUNT: u32 = 4;

/// Per-object geometry caps. A single object's tessellated mesh must fit
/// within both of these or `RuntimeError::GeometryOverflow` fires.
const MAX_VERTICES_PER_OBJECT: u64 = 4096;
const MAX_INDICES_PER_OBJECT: u64 = 16384;

const STROKE_VERTEX_BUFFER_SIZE: u64 =
    MAX_VERTICES_PER_OBJECT * std::mem::size_of::<tessellator::StrokeVertex>() as u64;
const FILL_VERTEX_BUFFER_SIZE: u64 =
    MAX_VERTICES_PER_OBJECT * std::mem::size_of::<pipelines::path_fill::FillVertex>() as u64;
pub(crate) const INDEX_BUFFER_SIZE: u64 =
    MAX_INDICES_PER_OBJECT * std::mem::size_of::<u32>() as u64;

fn align_up(v: u32, align: u32) -> u32 {
    let r = v % align;
    if r == 0 { v } else { v + align - r }
}

/// Anything wgpu init or per-frame rendering can fail with.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("no wgpu adapter found (headless Metal init failed?)")]
    NoAdapter,
    #[error("failed to acquire wgpu device: {0}")]
    DeviceRequest(#[from] wgpu::RequestDeviceError),
    #[error("buffer map failed: {0:?}")]
    BufferMap(wgpu::BufferAsyncError),
    #[error("wgpu poll failed: {0}")]
    Poll(wgpu::PollError),
    #[error("geometry buffer overflow: {kind} needs {needed} bytes, have {cap}")]
    GeometryOverflow {
        kind: &'static str,
        needed: u64,
        cap: u64,
    },
}

/// Owns the wgpu device, MSAA + resolve targets, readback buffer, and the
/// stroke/fill pipelines. Construct once, call [`Runtime::render`] per frame.
pub struct Runtime {
    device: wgpu::Device,
    queue: wgpu::Queue,
    msaa_view: wgpu::TextureView,
    resolve_target: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,

    stroke: PipeBundle,
    fill: PipeBundle,
}

impl Runtime {
    /// Stand up the wgpu device and pipelines for a `width × height` target.
    /// One-time cost is hundreds of milliseconds; reuse the same `Runtime`
    /// across all frames of a render.
    pub fn new(width: u32, height: u32) -> Result<Self, RuntimeError> {
        assert!(width > 0 && height > 0, "runtime needs non-zero dimensions");

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|_| RuntimeError::NoAdapter)?;

        // Use the adapter's reported limits rather than `downlevel_defaults`,
        // whose 2048x2048 texture cap blocks 4K renders on hardware that can
        // easily support them (Apple Silicon, modern desktop GPUs). Callers
        // get what the GPU can actually do; if a device lacks 4K support, the
        // texture-creation error surfaces with an obvious limit message.
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("manim-rs-raster device"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            }))?;

        // MSAA color target — render here.
        let msaa_color_target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("manim-rs MSAA color target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: MSAA_SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let msaa_view = msaa_color_target.create_view(&wgpu::TextureViewDescriptor::default());

        // Single-sample resolve target — what we actually copy out.
        let resolve_target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("manim-rs resolve target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let resolve_view = resolve_target.create_view(&wgpu::TextureViewDescriptor::default());

        let unpadded_bytes_per_row = width * 4;
        let padded_bytes_per_row = align_up(unpadded_bytes_per_row, COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback_size = u64::from(padded_bytes_per_row) * u64::from(height);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("manim-rs readback buffer"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let stroke_pipe = StrokePipeline::new(&device, COLOR_FORMAT);
        let stroke = build_pipe_bundle(
            &device,
            "stroke",
            stroke_pipe.pipeline,
            &stroke_pipe.bind_group_layout,
            STROKE_VERTEX_BUFFER_SIZE,
            STROKE_UNIFORM_SIZE,
        );

        let fill_pipe = FillPipeline::new(&device, COLOR_FORMAT);
        let fill = build_pipe_bundle(
            &device,
            "fill",
            fill_pipe.pipeline,
            &fill_pipe.bind_group_layout,
            FILL_VERTEX_BUFFER_SIZE,
            FILL_UNIFORM_SIZE,
        );

        Ok(Self {
            device,
            queue,
            msaa_view,
            resolve_target,
            resolve_view,
            readback,
            width,
            height,
            padded_bytes_per_row,
            stroke,
            fill,
        })
    }

    /// Render-target width in pixels, fixed at construction.
    pub fn width(&self) -> u32 {
        self.width
    }
    /// Render-target height in pixels, fixed at construction.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Clear the target to `color` and return tight RGBA bytes. Debug helper;
    /// kept around because the stroke-square example and future examples
    /// benefit from a no-geometry baseline.
    pub fn render_clear(&self, color: [f64; 4]) -> Result<Vec<u8>, RuntimeError> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("manim-rs clear encoder"),
            });
        self.begin_and_end_clear_pass(&mut encoder, color);
        self.copy_resolve_to_readback(&mut encoder);
        self.queue.submit(Some(encoder.finish()));
        self.readback_pixels()
    }

    /// Render a scene-state snapshot.
    ///
    /// Per-object policy: re-tessellate every object every frame; transforms
    /// come entirely from the MVP. Per-object draw order is **fill, then
    /// stroke** so the outline sits on top of the interior, matching manimgl.
    ///
    /// Per-object submission is load-bearing: reusing one vertex/index/
    /// uniform buffer across multiple passes in a *single* submit drops
    /// every object but the last, because `queue.write_buffer` is ordered
    /// before all submitted command buffers. Submitting after each object
    /// forces the writes to interleave with the passes as authored. See
    /// `docs/gotchas.md`.
    pub fn render(
        &self,
        state: &SceneState,
        camera: &Camera,
        background: [f64; 4],
    ) -> Result<Vec<u8>, RuntimeError> {
        let projection = camera.projection();
        // Orthographic pixel size in scene units. The stroke fragment shader
        // needs this to convert ribbon-space `uv.y` into pixel distance for AA.
        let pixel_size = (camera.right - camera.left) / self.width as f32;

        // The first drawn object clears the background; subsequent ones load.
        let mut needs_clear = true;
        for obj in &state.objects {
            let draw = tessellate_object(obj);
            if draw.is_empty() {
                continue;
            }

            let mvp = build_mvp(&projection, obj);
            let load = if needs_clear {
                needs_clear = false;
                clear_load(background)
            } else {
                wgpu::LoadOp::Load
            };
            self.render_one_object(&draw, mvp, pixel_size, load)?;
        }

        let mut tail_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("manim-rs readback encoder"),
                });

        // Empty scenes still need a clear so readback is well-defined.
        if needs_clear {
            self.begin_and_end_clear_pass(&mut tail_encoder, background);
        }

        self.copy_resolve_to_readback(&mut tail_encoder);
        self.queue.submit(Some(tail_encoder.finish()));
        self.readback_pixels()
    }

    fn render_one_object(
        &self,
        draw: &ObjectDraw,
        mvp: glam::Mat4,
        pixel_size: f32,
        load: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), RuntimeError> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("manim-rs per-object encoder"),
            });

        if let Some((mesh, color)) = &draw.fill {
            self.upload_mesh(
                &self.fill,
                cast_slice(&mesh.vertices),
                cast_slice(&mesh.indices),
                bytemuck::bytes_of(&FillUniforms::new(mvp, *color)),
            )?;
        }
        if let Some(buffers) = &draw.stroke {
            self.upload_mesh(
                &self.stroke,
                cast_slice(&buffers.vertices),
                cast_slice(&buffers.indices),
                bytemuck::bytes_of(&StrokeUniforms::new(mvp, ANTI_ALIAS_WIDTH, pixel_size)),
            )?;
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("paint pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa_view,
                    resolve_target: Some(&self.resolve_view),
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if let Some((mesh, _)) = &draw.fill {
                draw_mesh(&mut pass, &self.fill, mesh.indices.len() as u32);
            }
            if let Some(buffers) = &draw.stroke {
                draw_mesh(&mut pass, &self.stroke, buffers.indices.len() as u32);
            }
        }

        // Submit per object — see the doc comment on `render`.
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    fn begin_and_end_clear_pass(&self, encoder: &mut wgpu::CommandEncoder, color: [f64; 4]) {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("manim-rs clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.msaa_view,
                resolve_target: Some(&self.resolve_view),
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: color[0],
                        g: color[1],
                        b: color[2],
                        a: color[3],
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    fn copy_resolve_to_readback(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.resolve_target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn upload_mesh(
        &self,
        bundle: &PipeBundle,
        v_bytes: &[u8],
        i_bytes: &[u8],
        uniforms: &[u8],
    ) -> Result<(), RuntimeError> {
        check_geometry_size(
            v_bytes.len() as u64,
            i_bytes.len() as u64,
            bundle.vertex_buf_size,
        )?;
        self.queue.write_buffer(&bundle.uniform_buf, 0, uniforms);
        self.queue.write_buffer(&bundle.vertex_buf, 0, v_bytes);
        self.queue.write_buffer(&bundle.index_buf, 0, i_bytes);
        Ok(())
    }

    fn readback_pixels(&self) -> Result<Vec<u8>, RuntimeError> {
        let slice = self.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(RuntimeError::Poll)?;
        rx.recv()
            .expect("map_async callback channel dropped")
            .map_err(RuntimeError::BufferMap)?;

        let unpadded = self.width as usize * 4;
        let padded = self.padded_bytes_per_row as usize;
        let out = {
            let mapped = slice.get_mapped_range();
            let mut out = Vec::with_capacity(unpadded * self.height as usize);
            for row in 0..self.height as usize {
                let start = row * padded;
                out.extend_from_slice(&mapped[start..start + unpadded]);
            }
            out
        };
        self.readback.unmap();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::align_up;

    #[test]
    fn align_up_rounds_correctly() {
        assert_eq!(align_up(0, 256), 0);
        assert_eq!(align_up(1, 256), 256);
        assert_eq!(align_up(256, 256), 256);
        assert_eq!(align_up(1920, 256), 2048);
        assert_eq!(align_up(2048, 256), 2048);
    }
}
