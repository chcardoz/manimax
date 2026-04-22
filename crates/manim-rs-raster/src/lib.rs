//! Manimax rasterizer — wgpu device, MSAA-resolved offscreen render target,
//! CPU readback, and stroke + fill pipelines.
//!
//! Slice C scope: a `Runtime` that owns one MSAA color texture (4×) and a
//! single-sample resolve texture at a fixed resolution, one readback buffer,
//! a stroke pipeline and a fill pipeline. `render` evaluates nothing — it
//! takes a `SceneState` produced by `manim-rs-eval` plus a `Camera`, draws
//! every object's fill (if any) and stroke (if any), resolves MSAA to the
//! single-sample target, and returns tight RGBA bytes.
//!
//! Pre-solved wgpu gotcha (see `docs/slices/slice-b.md` §6.1): buffer copies
//! require `bytes_per_row` to be a multiple of 256. A 480-wide RGBA row is
//! 1920 bytes, padded up to 2048. The helper `unpad_rows` strips the 128
//! trailing bytes per row before returning.

pub mod camera;
pub mod pipelines;
pub mod tessellator;

pub use camera::Camera;
pub use tessellator::{FillMesh, Mesh, Vertex};

use bytemuck::cast_slice;
use glam::Mat4;
use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{Object, RgbaSrgb};

use crate::pipelines::path_fill::{FillPipeline, FillUniforms};
use crate::pipelines::path_stroke::{StrokePipeline, StrokeUniforms, UNIFORM_SIZE};
use crate::tessellator::{
    tessellate_bezpath, tessellate_bezpath_fill, tessellate_polyline, tessellate_polyline_fill,
};

const COPY_BYTES_PER_ROW_ALIGNMENT: u32 = 256;
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// MSAA sample count. 4× is universally supported across wgpu backends and
/// gives clean sub-pixel coverage for the thin strokes Manimax renders.
pub const MSAA_SAMPLE_COUNT: u32 = 4;

/// Per-object geometry caps. A single object's tessellated mesh must fit
/// within both of these or `RuntimeError::GeometryOverflow` fires. Calibrated
/// for Slice C; raise when real scenes push the ceiling.
const MAX_VERTICES_PER_OBJECT: u64 = 4096;
const MAX_INDICES_PER_OBJECT: u64 = 16384;

/// wgpu buffer sizes derived from the count caps. Vertex / index buffers are
/// sized independently — `sizeof(Vertex)` changes (Slice D will add attrs)
/// move the vertex-buffer size without touching the index side.
const VERTEX_BUFFER_SIZE: u64 =
    MAX_VERTICES_PER_OBJECT * std::mem::size_of::<tessellator::Vertex>() as u64;
const INDEX_BUFFER_SIZE: u64 = MAX_INDICES_PER_OBJECT * std::mem::size_of::<u32>() as u64;

fn align_up(v: u32, align: u32) -> u32 {
    let r = v % align;
    if r == 0 { v } else { v + align - r }
}

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

/// Per-pipeline GPU resources. Stroke and fill each have one of these; grouping
/// them keeps `Runtime` readable and lets `upload_mesh` / `draw_mesh` take a
/// single reference instead of four parallel field accesses.
struct PipeBundle {
    pipeline: wgpu::RenderPipeline,
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// Paints an `ObjectState` resolves into for one frame. Either side may be
/// absent (no stroke / no fill, or zero-index mesh after tessellation).
struct ObjectDraw {
    fill: Option<(FillMesh, RgbaSrgb)>,
    stroke: Option<(Mesh, RgbaSrgb)>,
}

impl ObjectDraw {
    fn is_empty(&self) -> bool {
        self.fill.is_none() && self.stroke.is_none()
    }
}

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
        );

        let fill_pipe = FillPipeline::new(&device, COLOR_FORMAT);
        let fill = build_pipe_bundle(
            &device,
            "fill",
            fill_pipe.pipeline,
            &fill_pipe.bind_group_layout,
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

    pub fn width(&self) -> u32 {
        self.width
    }
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
            self.render_one_object(&draw, mvp, load)?;
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
        mvp: Mat4,
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
        if let Some((mesh, color)) = &draw.stroke {
            self.upload_mesh(
                &self.stroke,
                cast_slice(&mesh.vertices),
                cast_slice(&mesh.indices),
                bytemuck::bytes_of(&StrokeUniforms::new(mvp, *color)),
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
            if let Some((mesh, _)) = &draw.stroke {
                draw_mesh(&mut pass, &self.stroke, mesh.indices.len() as u32);
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
        check_geometry_size(v_bytes.len() as u64, i_bytes.len() as u64)?;
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

fn build_pipe_bundle(
    device: &wgpu::Device,
    label_prefix: &str,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> PipeBundle {
    let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&format!("{label_prefix} vertex buffer")),
        size: VERTEX_BUFFER_SIZE,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let index_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&format!("{label_prefix} index buffer")),
        size: INDEX_BUFFER_SIZE,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&format!("{label_prefix} uniform buffer")),
        size: UNIFORM_SIZE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("{label_prefix} bind group")),
        layout: bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });
    PipeBundle {
        pipeline,
        vertex_buf,
        index_buf,
        uniform_buf,
        bind_group,
    }
}

fn draw_mesh(pass: &mut wgpu::RenderPass<'_>, bundle: &PipeBundle, index_count: u32) {
    pass.set_pipeline(&bundle.pipeline);
    pass.set_bind_group(0, &bundle.bind_group, &[]);
    pass.set_vertex_buffer(0, bundle.vertex_buf.slice(..));
    pass.set_index_buffer(bundle.index_buf.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..index_count, 0, 0..1);
}

fn build_mvp(projection: &Mat4, state: &ObjectState) -> Mat4 {
    let translation = Mat4::from_translation(glam::Vec3::new(
        state.position[0],
        state.position[1],
        state.position[2],
    ));
    let rotation = Mat4::from_rotation_z(state.rotation);
    let scale = Mat4::from_scale(glam::Vec3::splat(state.scale));
    *projection * translation * rotation * scale
}

fn clear_load(background: [f64; 4]) -> wgpu::LoadOp<wgpu::Color> {
    wgpu::LoadOp::Clear(wgpu::Color {
        r: background[0],
        g: background[1],
        b: background[2],
        a: background[3],
    })
}

/// Tessellate the object's geometry and resolve its paint colors. Returns
/// an empty `ObjectDraw` (both sides `None`) if no paint produces drawable
/// indices — the caller skips empty draws.
fn tessellate_object(state: &ObjectState) -> ObjectDraw {
    let (fill_raw, stroke_raw) = match &*state.object {
        Object::Polyline {
            points,
            closed,
            stroke,
            fill,
        } => (
            fill.as_ref()
                .map(|f| (tessellate_polyline_fill(points, *closed), f.color)),
            stroke
                .as_ref()
                .map(|s| (tessellate_polyline(points, s.width, *closed), s.color)),
        ),
        Object::BezPath {
            verbs,
            stroke,
            fill,
        } => (
            fill.as_ref()
                .map(|f| (tessellate_bezpath_fill(verbs), f.color)),
            stroke
                .as_ref()
                .map(|s| (tessellate_bezpath(verbs, s.width), s.color)),
        ),
    };

    let fill = fill_raw.and_then(|(mesh, color)| {
        (!mesh.indices.is_empty()).then(|| {
            (
                mesh,
                with_opacity(resolve_color(color, state.color_override), state.opacity),
            )
        })
    });
    let stroke = stroke_raw.and_then(|(mesh, color)| {
        (!mesh.indices.is_empty()).then(|| {
            (
                mesh,
                with_opacity(resolve_color(color, state.color_override), state.opacity),
            )
        })
    });
    ObjectDraw { fill, stroke }
}

fn resolve_color(base: RgbaSrgb, color_override: Option<RgbaSrgb>) -> RgbaSrgb {
    color_override.unwrap_or(base)
}

fn with_opacity(mut color: RgbaSrgb, opacity: f32) -> RgbaSrgb {
    color[3] *= opacity;
    color
}

fn check_geometry_size(v_len: u64, i_len: u64) -> Result<(), RuntimeError> {
    if v_len > VERTEX_BUFFER_SIZE {
        return Err(RuntimeError::GeometryOverflow {
            kind: "vertex",
            needed: v_len,
            cap: VERTEX_BUFFER_SIZE,
        });
    }
    if i_len > INDEX_BUFFER_SIZE {
        return Err(RuntimeError::GeometryOverflow {
            kind: "index",
            needed: i_len,
            cap: INDEX_BUFFER_SIZE,
        });
    }
    Ok(())
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
