//! Polyline → triangle-mesh tessellation via lyon's `StrokeTessellator`.
//!
//! Slice B: rigid-width polyline, no Bézier, no per-vertex width, no AA.
//! See `docs/porting-notes/stroke.md` for the full delta vs. manimgl's
//! `quadratic_bezier/stroke/` pipeline (PORT_STUB_MANIMGL_STROKE).

use bytemuck::{Pod, Zeroable};
use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex as LyonFillVertex,
    FillVertexConstructor, StrokeOptions, StrokeTessellator, StrokeVertex, StrokeVertexConstructor,
    VertexBuffers,
};
use manim_rs_ir::{PathVerb, Vec3};

use crate::pipelines::path_fill::FillVertex;

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

/// Position-only mesh produced by the fill tessellator.
pub struct FillMesh {
    pub vertices: Vec<FillVertex>,
    pub indices: Vec<u32>,
}

struct StrokeCtor;
impl StrokeVertexConstructor<Vertex> for StrokeCtor {
    fn new_vertex(&mut self, v: StrokeVertex<'_, '_>) -> Vertex {
        let p = v.position();
        Vertex {
            position: [p.x, p.y],
            uv: [0.0, 0.0],
        }
    }
}

struct FillCtor;
impl FillVertexConstructor<FillVertex> for FillCtor {
    fn new_vertex(&mut self, v: LyonFillVertex<'_>) -> FillVertex {
        let p = v.position();
        FillVertex {
            position: [p.x, p.y],
        }
    }
}

/// Build a lyon `Path` from a polyline in IR space (z dropped).
fn polyline_to_path(points: &[Vec3], closed: bool) -> Path {
    let mut builder = Path::builder();
    builder.begin(point(points[0][0], points[0][1]));
    for p in &points[1..] {
        builder.line_to(point(p[0], p[1]));
    }
    builder.end(closed);
    builder.build()
}

/// Build a lyon `Path` from a `BezPath` verb stream. Manimgl's path model
/// allows multiple sub-paths per object; respect `MoveTo` between drawn verbs.
fn verbs_to_path(verbs: &[PathVerb]) -> Path {
    let mut builder = Path::builder();
    let mut started = false;
    for verb in verbs {
        match verb {
            PathVerb::MoveTo { to } => {
                if started {
                    builder.end(false);
                }
                builder.begin(point(to[0], to[1]));
                started = true;
            }
            PathVerb::LineTo { to } => {
                builder.line_to(point(to[0], to[1]));
            }
            PathVerb::QuadTo { ctrl, to } => {
                builder.quadratic_bezier_to(point(ctrl[0], ctrl[1]), point(to[0], to[1]));
            }
            PathVerb::CubicTo { ctrl1, ctrl2, to } => {
                builder.cubic_bezier_to(
                    point(ctrl1[0], ctrl1[1]),
                    point(ctrl2[0], ctrl2[1]),
                    point(to[0], to[1]),
                );
            }
            PathVerb::Close {} => {
                if started {
                    builder.end(true);
                    started = false;
                }
            }
        }
    }
    if started {
        builder.end(false);
    }
    builder.build()
}

/// Tessellate a polyline (N >= 2 points) into a stroke mesh. The 3D `points`
/// array has z dropped — Slice B is planar.
pub fn tessellate_polyline(points: &[Vec3], stroke_width: f32, closed: bool) -> Mesh {
    assert!(points.len() >= 2, "polyline needs at least 2 points");
    let path = polyline_to_path(points, closed);
    tessellate_stroke_path(&path, stroke_width)
}

/// Fill the interior of a closed polyline. Returns an empty mesh for open
/// polylines — fill is meaningless without closure.
pub fn tessellate_polyline_fill(points: &[Vec3], closed: bool) -> FillMesh {
    if !closed || points.len() < 3 {
        return FillMesh {
            vertices: Vec::new(),
            indices: Vec::new(),
        };
    }
    let path = polyline_to_path(points, true);
    tessellate_fill_path(&path)
}

/// Stroke a `BezPath` verb stream. Empty mesh for paths with no drawable verbs.
pub fn tessellate_bezpath(verbs: &[PathVerb], stroke_width: f32) -> Mesh {
    if verbs.is_empty() {
        return Mesh {
            vertices: Vec::new(),
            indices: Vec::new(),
        };
    }
    let path = verbs_to_path(verbs);
    tessellate_stroke_path(&path, stroke_width)
}

/// Fill a `BezPath` interior. Manimgl uses non-zero fill; we match that.
pub fn tessellate_bezpath_fill(verbs: &[PathVerb]) -> FillMesh {
    if verbs.is_empty() {
        return FillMesh {
            vertices: Vec::new(),
            indices: Vec::new(),
        };
    }
    let path = verbs_to_path(verbs);
    tessellate_fill_path(&path)
}

fn tessellate_stroke_path(path: &Path, stroke_width: f32) -> Mesh {
    let mut buffers: VertexBuffers<Vertex, u32> = VertexBuffers::new();
    let opts = StrokeOptions::DEFAULT.with_line_width(stroke_width);
    let mut tess = StrokeTessellator::new();
    tess.tessellate_path(
        path,
        &opts,
        &mut BuffersBuilder::new(&mut buffers, StrokeCtor),
    )
    .expect("stroke tessellation");
    Mesh {
        vertices: buffers.vertices,
        indices: buffers.indices,
    }
}

fn tessellate_fill_path(path: &Path) -> FillMesh {
    let mut buffers: VertexBuffers<FillVertex, u32> = VertexBuffers::new();
    let opts = FillOptions::DEFAULT.with_fill_rule(FillRule::NonZero);
    let mut tess = FillTessellator::new();
    tess.tessellate_path(
        path,
        &opts,
        &mut BuffersBuilder::new(&mut buffers, FillCtor),
    )
    .expect("fill tessellation");
    FillMesh {
        vertices: buffers.vertices,
        indices: buffers.indices,
    }
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
        assert!(
            mesh.indices.len() % 3 == 0,
            "indices must form whole triangles"
        );
        assert!(
            mesh.indices.len() >= 6,
            "a stroked square is at least two triangles"
        );
    }
}
