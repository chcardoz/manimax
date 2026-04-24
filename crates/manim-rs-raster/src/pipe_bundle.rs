//! Per-pipeline GPU resources. Stroke and fill each get one bundle so
//! `upload_mesh` / `draw_mesh` take a single reference instead of four
//! parallel field accesses.

use crate::INDEX_BUFFER_SIZE;

/// Vertex / index / uniform buffers and the bind group that points at the
/// uniforms — everything the runtime needs to drive one render pipeline.
pub(crate) struct PipeBundle {
    pub(crate) pipeline: wgpu::RenderPipeline,
    pub(crate) vertex_buf: wgpu::Buffer,
    pub(crate) vertex_buf_size: u64,
    pub(crate) index_buf: wgpu::Buffer,
    pub(crate) uniform_buf: wgpu::Buffer,
    pub(crate) bind_group: wgpu::BindGroup,
}

/// Allocate the four buffers and the bind group for one render pipeline.
/// `label_prefix` is only used to label the buffers in wgpu validation messages.
pub(crate) fn build_pipe_bundle(
    device: &wgpu::Device,
    label_prefix: &str,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    vertex_buf_size: u64,
    uniform_size: u64,
) -> PipeBundle {
    let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&format!("{label_prefix} vertex buffer")),
        size: vertex_buf_size,
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
        size: uniform_size,
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
        vertex_buf_size,
        index_buf,
        uniform_buf,
        bind_group,
    }
}

/// Bind a bundle's pipeline + buffers and issue one indexed draw.
pub(crate) fn draw_mesh(pass: &mut wgpu::RenderPass<'_>, bundle: &PipeBundle, index_count: u32) {
    pass.set_pipeline(&bundle.pipeline);
    pass.set_bind_group(0, &bundle.bind_group, &[]);
    pass.set_vertex_buffer(0, bundle.vertex_buf.slice(..));
    pass.set_index_buffer(bundle.index_buf.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..index_count, 0, 0..1);
}
