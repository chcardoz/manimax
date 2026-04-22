//! Polyline → triangle-mesh tessellation via lyon's `StrokeTessellator`.
//!
//! Slice B: rigid-width polyline, no Bézier, no per-vertex width, no AA.
//! See `docs/porting-notes/stroke.md` for the full delta vs. manimgl's
//! `quadratic_bezier/stroke/` pipeline (PORT_STUB_MANIMGL_STROKE).

use bytemuck::{Pod, Zeroable};
use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, StrokeOptions, StrokeTessellator, StrokeVertex, StrokeVertexConstructor,
    VertexBuffers,
};
use manim_rs_ir::Vec3;

/// Vertex layout uploaded to the GPU. `uv` is unused in Slice B but kept so the
/// vertex buffer stride stays stable when Slice D adds attributes.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

struct StrokeCtor;
impl StrokeVertexConstructor<Vertex> for StrokeCtor {
    fn new_vertex(&mut self, v: StrokeVertex<'_, '_>) -> Vertex {
        let p = v.position();
        Vertex { position: [p.x, p.y], uv: [0.0, 0.0] }
    }
}

/// Tessellate a polyline (N >= 2 points) into a stroke mesh. The 3D `points`
/// array has z dropped — Slice B is planar.
pub fn tessellate_polyline(points: &[Vec3], stroke_width: f32, closed: bool) -> Mesh {
    assert!(points.len() >= 2, "polyline needs at least 2 points");

    let mut builder = Path::builder();
    builder.begin(point(points[0][0], points[0][1]));
    for p in &points[1..] {
        builder.line_to(point(p[0], p[1]));
    }
    builder.end(closed);
    let path = builder.build();

    let mut buffers: VertexBuffers<Vertex, u32> = VertexBuffers::new();
    let opts = StrokeOptions::DEFAULT.with_line_width(stroke_width);
    let mut tess = StrokeTessellator::new();
    tess.tessellate_path(&path, &opts, &mut BuffersBuilder::new(&mut buffers, StrokeCtor))
        .expect("stroke tessellation");
    Mesh { vertices: buffers.vertices, indices: buffers.indices }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_tessellation_yields_triangles() {
        let points: Vec<Vec3> = vec![
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ];
        let mesh = tessellate_polyline(&points, 0.1, true);
        assert!(!mesh.vertices.is_empty(), "stroke mesh must have vertices");
        assert!(mesh.indices.len() % 3 == 0, "indices must form whole triangles");
        assert!(mesh.indices.len() >= 6, "a stroked square is at least two triangles");
    }
}
