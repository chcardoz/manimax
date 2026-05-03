//! Fill pipeline — solid-color filled paths (Polyline interiors and BezPath
//! interiors). Sibling to `path_stroke`; uses a position-only vertex since
//! there is nothing to sample per-pixel.

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

use crate::MSAA_SAMPLE_COUNT;
use manim_rs_ir::RgbaSrgb;

/// Fill mesh vertex — position only.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct FillVertex {
    pub position: [f32; 2],
}

/// Fill uniform: `{ mat4x4 mvp, vec4 color }` — matches the fill WGSL.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct FillUniforms {
    pub mvp: [[f32; 4]; 4],
    pub color: [f32; 4],
}

impl FillUniforms {
    /// Pack a model-view-projection matrix and a color into the layout the
    /// fill WGSL expects.
    pub fn new(mvp: Mat4, color: RgbaSrgb) -> Self {
        Self {
            mvp: mvp.to_cols_array_2d(),
            color,
        }
    }
}

/// Byte size of [`FillUniforms`] — used to size the GPU uniform buffer.
pub const UNIFORM_SIZE: u64 = std::mem::size_of::<FillUniforms>() as u64;

/// Compiled fill pipeline plus the bind-group layout the runtime needs to
/// build a uniform binding.
pub(crate) struct FillPipeline {
    pub(crate) pipeline: wgpu::RenderPipeline,
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
}

impl FillPipeline {
    /// Compile the fill WGSL shader and create the wgpu pipeline.
    pub(crate) fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("path_fill.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/path_fill.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fill uniforms layout"),
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
            label: Some("fill pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        const FILL_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![
            0 => Float32x2,
        ];
        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<FillVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &FILL_ATTRS,
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fill pipeline"),
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
