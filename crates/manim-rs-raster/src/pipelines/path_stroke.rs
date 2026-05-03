//! Stroke pipeline — port of
//! `manimlib/shaders/quadratic_bezier/stroke/` @ commit `c5e23d9`.
//!
//! Vertex stage: MVP + passthrough of the rich `StrokeVertex` attributes.
//! Fragment stage: analytic SDF AA over the stroke's centerline-parameterised
//! ribbon. Color is per-vertex; uniforms carry the anti-alias width (pixels)
//! and pixel size (scene units per pixel) needed to convert `uv.y` into a
//! pixel-space signed distance.

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

use crate::MSAA_SAMPLE_COUNT;
use crate::tessellator::StrokeVertex;

/// Matches the `Uniforms` struct in `shaders/path_stroke.wgsl`.
/// `mat4x4<f32>` = 64 bytes, `vec4<f32>` = 16 bytes. `params.x` =
/// anti-alias width (pixels); `params.y` = pixel size (scene units/pixel).
/// `params.zw` unused but present for 16-byte alignment.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct StrokeUniforms {
    pub mvp: [[f32; 4]; 4],
    pub params: [f32; 4],
}

impl StrokeUniforms {
    /// Pack the MVP and the AA-width / pixel-size scalars used for the
    /// fragment-shader signed-distance fade.
    pub fn new(mvp: Mat4, anti_alias_width: f32, pixel_size: f32) -> Self {
        Self {
            mvp: mvp.to_cols_array_2d(),
            params: [anti_alias_width, pixel_size, 0.0, 0.0],
        }
    }
}

/// Byte size of [`StrokeUniforms`] — used to size the GPU uniform buffer.
pub const UNIFORM_SIZE: u64 = std::mem::size_of::<StrokeUniforms>() as u64;

/// Compiled stroke pipeline plus the bind-group layout the runtime needs
/// to build a uniform binding.
pub(crate) struct StrokePipeline {
    pub(crate) pipeline: wgpu::RenderPipeline,
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
}

impl StrokePipeline {
    /// Compile the stroke WGSL shader and create the wgpu pipeline.
    pub(crate) fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("path_stroke.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/path_stroke.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stroke uniforms layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE),
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stroke pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        const STROKE_ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
            0 => Float32x2,
            1 => Float32x2,
            2 => Float32,
            3 => Float32,
            4 => Float32x4,
        ];
        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<StrokeVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &STROKE_ATTRS,
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stroke pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[vertex_buffer_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // Ribbon emits both windings depending on tangent direction.
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLE_COUNT,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
        }
    }
}
