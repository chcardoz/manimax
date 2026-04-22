//! Manimax rasterizer — wgpu device, offscreen render target, CPU readback,
//! and a single stroke pipeline.
//!
//! Slice B scope: a `Runtime` that owns one `Rgba8UnormSrgb` color texture at
//! a fixed resolution, one readback buffer, one stroke pipeline, and reusable
//! vertex/index/uniform buffers. `render` evaluates nothing — it takes a
//! `SceneState` produced by `manim-rs-eval` plus a `Camera`, draws every
//! object's polyline stroke, and returns tight RGBA bytes.
//!
//! Pre-solved wgpu gotcha (see `docs/slices/slice-b.md` §6.1): buffer copies
//! require `bytes_per_row` to be a multiple of 256. A 480-wide RGBA row is
//! 1920 bytes, padded up to 2048. The helper `unpad_rows` strips the 128
//! trailing bytes per row before returning.

pub mod camera;
pub mod pipelines;
pub mod tessellator;

pub use camera::Camera;
pub use tessellator::{Mesh, Vertex};

use bytemuck::cast_slice;
use glam::Mat4;
use manim_rs_eval::SceneState;
use manim_rs_ir::Object;

use crate::pipelines::path_stroke::{StrokePipeline, StrokeUniforms, UNIFORM_SIZE};
use crate::tessellator::tessellate_polyline;

const COPY_BYTES_PER_ROW_ALIGNMENT: u32 = 256;
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Per-object vertex/index buffer capacity. 64 KiB holds ~4 K vertices or
/// ~16 K u32 indices — more than enough for Slice B.
const GEOMETRY_BUFFER_SIZE: u64 = 64 * 1024;

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

pub struct Runtime {
    device: wgpu::Device,
    queue: wgpu::Queue,
    color_target: wgpu::Texture,
    color_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,

    stroke: StrokePipeline,
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
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

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("manim-rs-raster device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            }))?;

        let color_target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("manim-rs color target"),
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
        let color_view = color_target.create_view(&wgpu::TextureViewDescriptor::default());

        let unpadded_bytes_per_row = width * 4;
        let padded_bytes_per_row = align_up(unpadded_bytes_per_row, COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback_size = u64::from(padded_bytes_per_row) * u64::from(height);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("manim-rs readback buffer"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let stroke = StrokePipeline::new(&device, COLOR_FORMAT);

        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stroke vertex buffer"),
            size: GEOMETRY_BUFFER_SIZE,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stroke index buffer"),
            size: GEOMETRY_BUFFER_SIZE,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stroke uniform buffer"),
            size: UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stroke bind group"),
            layout: &stroke.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        Ok(Self {
            device,
            queue,
            color_target,
            color_view,
            readback,
            width,
            height,
            padded_bytes_per_row,
            stroke,
            vertex_buf,
            index_buf,
            uniform_buf,
            bind_group,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Clear the target to `color` and return tight RGBA bytes. Step 4 debug
    /// helper; kept around because the stroke-square example and future
    /// examples benefit from a no-geometry baseline.
    pub fn render_clear(&self, color: [f64; 4]) -> Result<Vec<u8>, RuntimeError> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("manim-rs clear encoder"),
            });
        self.begin_and_end_clear_pass(&mut encoder, color);
        self.copy_target_to_readback(&mut encoder);
        self.queue.submit(Some(encoder.finish()));
        self.readback_pixels()
    }

    /// Render a scene-state snapshot through the stroke pipeline.
    ///
    /// Slice B policy: re-tessellate every object every frame; position comes
    /// entirely from the MVP (no geometry rebuild on translation). The camera
    /// projection is composed with a per-object translation.
    ///
    /// Per-object submission is load-bearing: reusing one vertex/index/uniform
    /// buffer across multiple passes in a *single* submit drops every object
    /// but the last, because `queue.write_buffer` is ordered before all
    /// submitted command buffers (last write wins, every pass sees the final
    /// buffer contents). Submitting after each object forces the writes to
    /// interleave with the passes as authored. See `docs/gotchas.md`.
    pub fn render(
        &self,
        state: &SceneState,
        camera: &Camera,
        background: [f64; 4],
    ) -> Result<Vec<u8>, RuntimeError> {
        let projection = camera.projection();

        // Background clear happens in the first pass; subsequent per-object
        // passes use `LoadOp::Load` so draws accumulate.
        let mut first = true;
        for obj in &state.objects {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("manim-rs per-object encoder"),
                });
            let Object::Polyline {
                points,
                stroke_color,
                stroke_width,
                closed,
            } = &obj.object;
            let mesh = tessellate_polyline(points, *stroke_width, *closed);
            if mesh.indices.is_empty() {
                continue;
            }

            let v_bytes: &[u8] = cast_slice(&mesh.vertices);
            let i_bytes: &[u8] = cast_slice(&mesh.indices);
            if v_bytes.len() as u64 > GEOMETRY_BUFFER_SIZE {
                return Err(RuntimeError::GeometryOverflow {
                    kind: "vertex",
                    needed: v_bytes.len() as u64,
                    cap: GEOMETRY_BUFFER_SIZE,
                });
            }
            if i_bytes.len() as u64 > GEOMETRY_BUFFER_SIZE {
                return Err(RuntimeError::GeometryOverflow {
                    kind: "index",
                    needed: i_bytes.len() as u64,
                    cap: GEOMETRY_BUFFER_SIZE,
                });
            }

            let translation = Mat4::from_translation(glam::Vec3::new(
                obj.position[0],
                obj.position[1],
                obj.position[2],
            ));
            let mvp = projection * translation;
            let uniforms = StrokeUniforms::new(mvp, *stroke_color);

            self.queue
                .write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniforms));
            self.queue.write_buffer(&self.vertex_buf, 0, v_bytes);
            self.queue.write_buffer(&self.index_buf, 0, i_bytes);

            let load = if first {
                first = false;
                wgpu::LoadOp::Clear(wgpu::Color {
                    r: background[0],
                    g: background[1],
                    b: background[2],
                    a: background[3],
                })
            } else {
                wgpu::LoadOp::Load
            };

            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("stroke pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.color_view,
                        resolve_target: None,
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
                pass.set_pipeline(&self.stroke.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buf.slice(..v_bytes.len() as u64));
                pass.set_index_buffer(
                    self.index_buf.slice(..i_bytes.len() as u64),
                    wgpu::IndexFormat::Uint32,
                );
                pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
            }

            // Submit per object — see the doc comment on `render`.
            self.queue.submit(Some(encoder.finish()));
        }

        let mut tail_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("manim-rs readback encoder"),
                });

        // Empty scenes still need a clear so readback is well-defined.
        if first {
            self.begin_and_end_clear_pass(&mut tail_encoder, background);
        }

        self.copy_target_to_readback(&mut tail_encoder);
        self.queue.submit(Some(tail_encoder.finish()));
        self.readback_pixels()
    }

    fn begin_and_end_clear_pass(&self, encoder: &mut wgpu::CommandEncoder, color: [f64; 4]) {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("manim-rs clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.color_view,
                resolve_target: None,
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

    fn copy_target_to_readback(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.color_target,
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
