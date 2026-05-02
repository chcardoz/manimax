//! Path sampling → quadratic-segment stream + stroke ribbon expansion.
//!
//! `sample_bezpath` + `expand_stroke` drive the stroke pipeline; fill still
//! uses lyon's `FillTessellator`.

use bytemuck::{Pod, Zeroable};
use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillVertex as LyonFillVertex, FillVertexConstructor,
    VertexBuffers,
};
use manim_rs_ir::{JointKind, PathVerb, Vec3};

use crate::pipelines::path_fill::FillVertex;

pub(crate) use lyon::tessellation::FillTessellator;

/// Position-only mesh produced by the fill tessellator.
pub struct FillMesh {
    pub vertices: Vec<FillVertex>,
    pub indices: Vec<u32>,
}

/// 2D quadratic Bézier segment — the uniform representation consumed by
/// the stroke pipeline. `p0` and `p2` are the endpoints; `p1` is the
/// control point. Straight lines are encoded as degenerate quadratics with
/// `p1 = (p0 + p2) / 2` so all downstream code can assume a quadratic.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct QuadraticSegment {
    pub p0: [f32; 2],
    pub p1: [f32; 2],
    pub p2: [f32; 2],
}

/// Fixed subdivision depth for `CubicTo` → quadratic approximation.
/// Depth 2 → 4 quadratic segments per cubic. Raise if sharp cubics kink.
const CUBIC_SPLIT_DEPTH: u32 = 2;

fn v2(v: Vec3) -> [f32; 2] {
    [v[0], v[1]]
}

fn midpoint(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}

/// De Casteljau split of a cubic at t=0.5. Returns (left, right) cubics.
fn split_cubic(
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    p3: [f32; 2],
) -> (
    ([f32; 2], [f32; 2], [f32; 2], [f32; 2]),
    ([f32; 2], [f32; 2], [f32; 2], [f32; 2]),
) {
    let m01 = midpoint(p0, p1);
    let m12 = midpoint(p1, p2);
    let m23 = midpoint(p2, p3);
    let m012 = midpoint(m01, m12);
    let m123 = midpoint(m12, m23);
    let m0123 = midpoint(m012, m123);
    ((p0, m01, m012, m0123), (m0123, m123, m23, p3))
}

/// Least-squares quadratic approximation of a cubic: keep endpoints, set
/// the middle control to `(3*c1 + 3*c2 - p0 - p3) / 4`. Accurate enough
/// when the cubic has been pre-subdivided to short arcs.
fn cubic_to_quadratic(p0: [f32; 2], c1: [f32; 2], c2: [f32; 2], p3: [f32; 2]) -> QuadraticSegment {
    let q1 = [
        (3.0 * c1[0] + 3.0 * c2[0] - p0[0] - p3[0]) * 0.25,
        (3.0 * c1[1] + 3.0 * c2[1] - p0[1] - p3[1]) * 0.25,
    ];
    QuadraticSegment { p0, p1: q1, p2: p3 }
}

// Port of `reference/manimgl/manimlib/shaders/quadratic_bezier/stroke/geom.glsl`
// @ commit `c5e23d9`. CPU-side equivalent of manimgl's geometry shader:
// polyline each quadratic, widen to a triangle strip with per-vertex
// attributes, join consecutive ribbons with miter/bevel.

/// Upper bound on samples per quadratic segment. Matches manimgl
/// (`MAX_STEPS = 32` in `geom.glsl`).
pub const STROKE_MAX_STEPS: usize = 32;

/// Scales rough arc length (scene units) to step count before clamping.
/// Matches manimgl (`POLYLINE_FACTOR = 100`).
pub const POLYLINE_FACTOR: f32 = 100.0;

/// Cosine of the angle between tangents at which AUTO switches from
/// miter to bevel. Matches manimgl (`MITER_COS_ANGLE_THRESHOLD = -0.8`).
pub const MITER_COS_ANGLE_THRESHOLD: f32 = -0.8;

/// Expanded stroke vertex consumed by the fragment shader. Position is in
/// scene space; `uv.y` is the perpendicular offset ratio (±1 at the stroke
/// edge, 0 on centerline) and `uv.x` is the arc parameter within the
/// segment (0..1). `joint_angle` is 0 within a segment and the signed angle
/// between incoming and outgoing tangents at joint vertices.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct StrokeVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub stroke_width: f32,
    pub joint_angle: f32,
    pub color: [f32; 4],
}

fn qb_point(t: f32, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2]) -> [f32; 2] {
    let u = 1.0 - t;
    [
        u * u * p0[0] + 2.0 * u * t * p1[0] + t * t * p2[0],
        u * u * p0[1] + 2.0 * u * t * p1[1] + t * t * p2[1],
    ]
}

fn qb_tangent(t: f32, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2]) -> [f32; 2] {
    let u = 1.0 - t;
    let tx = 2.0 * u * (p1[0] - p0[0]) + 2.0 * t * (p2[0] - p1[0]);
    let ty = 2.0 * u * (p1[1] - p0[1]) + 2.0 * t * (p2[1] - p1[1]);
    let len = (tx * tx + ty * ty).sqrt().max(1e-6);
    [tx / len, ty / len]
}

fn perp(t: [f32; 2]) -> [f32; 2] {
    [-t[1], t[0]]
}

fn rough_arc_len(seg: &QuadraticSegment) -> f32 {
    // Average of chord and control-net lengths — same heuristic manimgl
    // uses for adaptive subdivision.
    let chord = ((seg.p2[0] - seg.p0[0]).powi(2) + (seg.p2[1] - seg.p0[1]).powi(2)).sqrt();
    let n1 = ((seg.p1[0] - seg.p0[0]).powi(2) + (seg.p1[1] - seg.p0[1]).powi(2)).sqrt();
    let n2 = ((seg.p2[0] - seg.p1[0]).powi(2) + (seg.p2[1] - seg.p1[1]).powi(2)).sqrt();
    (chord + n1 + n2) * 0.5
}

fn step_count(seg: &QuadraticSegment) -> usize {
    let n = (rough_arc_len(seg) * POLYLINE_FACTOR).ceil() as usize;
    n.clamp(2, STROKE_MAX_STEPS)
}

/// Expand a stream of quadratic segments into a triangle strip ribbon
/// with per-vertex width and miter/bevel joints.
///
/// - `widths.len()` must be `1` (scalar broadcast) or `segments.len() + 1`
///   (endpoint-indexed; widths interpolate linearly across each segment).
/// - `color` is broadcast to every vertex; per-vertex color is out of
///   scope.
/// - `joint` controls the corner strategy. `Auto` mirrors manimgl's cosine
///   threshold.
///
/// Empty input returns an empty buffer. Degenerate (zero-length) segments
/// are emitted as trivial rectangles with no joint fix-up needed — they
/// contribute the same position twice and the fragment shader will collapse
/// them to zero coverage.
pub fn expand_stroke(
    segments: &[QuadraticSegment],
    widths: &[f32],
    color: [f32; 4],
    joint: JointKind,
) -> VertexBuffers<StrokeVertex, u32> {
    let mut buffers = VertexBuffers::<StrokeVertex, u32>::new();
    if segments.is_empty() {
        return buffers;
    }
    assert!(
        widths.len() == 1 || widths.len() == segments.len() + 1,
        "widths must have length 1 (scalar) or segments.len()+1 (per-vertex), got {} for {} segments",
        widths.len(),
        segments.len(),
    );
    let width_at = |i: usize| -> f32 {
        if widths.len() == 1 {
            widths[0]
        } else {
            widths[i]
        }
    };

    // Per-segment "end" data used to bridge joints with the next segment.
    let mut prev: Option<([f32; 2], [f32; 2], f32, u32, u32)> = None;

    for (i, seg) in segments.iter().enumerate() {
        let n = step_count(seg);
        let w_start = width_at(i);
        let w_end = width_at(i + 1);

        let mut first_in_seg: Option<(u32, u32)> = None;
        let mut last_in_seg: Option<(u32, u32)> = None;
        let mut start_tangent: [f32; 2] = [1.0, 0.0];
        let mut end_tangent: [f32; 2] = [1.0, 0.0];

        for j in 0..n {
            let t = j as f32 / (n - 1) as f32;
            let p = qb_point(t, seg.p0, seg.p1, seg.p2);
            let tang = qb_tangent(t, seg.p0, seg.p1, seg.p2);
            let np = perp(tang);
            let w = w_start * (1.0 - t) + w_end * t;
            let hw = w * 0.5;

            let left_idx = buffers.vertices.len() as u32;
            buffers.vertices.push(StrokeVertex {
                position: [p[0] + np[0] * hw, p[1] + np[1] * hw],
                uv: [t, 1.0],
                stroke_width: w,
                joint_angle: 0.0,
                color,
            });
            let right_idx = left_idx + 1;
            buffers.vertices.push(StrokeVertex {
                position: [p[0] - np[0] * hw, p[1] - np[1] * hw],
                uv: [t, -1.0],
                stroke_width: w,
                joint_angle: 0.0,
                color,
            });

            if let Some((pl, pr)) = last_in_seg {
                buffers.indices.extend_from_slice(&[pl, pr, left_idx]);
                buffers
                    .indices
                    .extend_from_slice(&[pr, right_idx, left_idx]);
            } else {
                first_in_seg = Some((left_idx, right_idx));
                start_tangent = tang;
            }
            last_in_seg = Some((left_idx, right_idx));
            if j == n - 1 {
                end_tangent = tang;
            }
        }

        let (start_l, start_r) = first_in_seg.unwrap();
        let (end_l, end_r) = last_in_seg.unwrap();

        // Joint between previous segment and this one.
        if let Some((prev_p, prev_t, prev_w, prev_l, prev_r)) = prev {
            let gap_sq = (prev_p[0] - seg.p0[0]).powi(2) + (prev_p[1] - seg.p0[1]).powi(2);
            if gap_sq < 1e-8 {
                let cos_theta = prev_t[0] * start_tangent[0] + prev_t[1] * start_tangent[1];
                let cross = prev_t[0] * start_tangent[1] - prev_t[1] * start_tangent[0];
                let use_miter = match joint {
                    JointKind::Miter => true,
                    JointKind::Bevel => false,
                    JointKind::Auto => cos_theta > MITER_COS_ANGLE_THRESHOLD,
                };
                if use_miter {
                    let pn = perp(prev_t);
                    let sn = perp(start_tangent);
                    let denom = 1.0 + pn[0] * sn[0] + pn[1] * sn[1];
                    let denom = if denom.abs() < 1e-6 { 1e-6 } else { denom };
                    let hw = 0.5 * (prev_w + w_start) * 0.5;
                    let mx = hw * (pn[0] + sn[0]) / denom;
                    let my = hw * (pn[1] + sn[1]) / denom;
                    let miter_w = (prev_w + w_start) * 0.5;
                    let joint_angle = cos_theta.acos() * cross.signum();
                    let ml_idx = buffers.vertices.len() as u32;
                    buffers.vertices.push(StrokeVertex {
                        position: [seg.p0[0] + mx, seg.p0[1] + my],
                        uv: [0.0, 1.0],
                        stroke_width: miter_w,
                        joint_angle,
                        color,
                    });
                    let mr_idx = ml_idx + 1;
                    buffers.vertices.push(StrokeVertex {
                        position: [seg.p0[0] - mx, seg.p0[1] - my],
                        uv: [0.0, -1.0],
                        stroke_width: miter_w,
                        joint_angle,
                        color,
                    });
                    // Fill the gap between prev end and current start via the miter quad.
                    buffers.indices.extend_from_slice(&[prev_l, prev_r, ml_idx]);
                    buffers.indices.extend_from_slice(&[prev_r, mr_idx, ml_idx]);
                    buffers
                        .indices
                        .extend_from_slice(&[ml_idx, mr_idx, start_l]);
                    buffers
                        .indices
                        .extend_from_slice(&[mr_idx, start_r, start_l]);
                } else {
                    // Bevel: quad directly between the two ribbon ends.
                    buffers
                        .indices
                        .extend_from_slice(&[prev_l, prev_r, start_l]);
                    buffers
                        .indices
                        .extend_from_slice(&[prev_r, start_r, start_l]);
                }
            }
        }

        prev = Some((seg.p2, end_tangent, w_end, end_l, end_r));
    }

    buffers
}

fn subdivide_cubic_to_quads(
    p0: [f32; 2],
    c1: [f32; 2],
    c2: [f32; 2],
    p3: [f32; 2],
    depth: u32,
    out: &mut Vec<QuadraticSegment>,
) {
    if depth == 0 {
        out.push(cubic_to_quadratic(p0, c1, c2, p3));
        return;
    }
    let (left, right) = split_cubic(p0, c1, c2, p3);
    subdivide_cubic_to_quads(left.0, left.1, left.2, left.3, depth - 1, out);
    subdivide_cubic_to_quads(right.0, right.1, right.2, right.3, depth - 1, out);
}

/// Walk a `BezPath` verb stream and emit a flat list of 2D quadratic
/// Bézier segments. Lines become degenerate quadratics; cubics split at
/// fixed depth and each leaf is approximated as a quadratic; `Close`
/// emits a line-style segment back to the current sub-path's opening
/// point. `MoveTo` advances the cursor without emitting.
///
/// Empty input returns an empty list. Drawing verbs before the first
/// `MoveTo` are treated as originating at `[0, 0]` — valid IR always
/// opens a sub-path with `MoveTo`, so this is a defensive default.
pub fn sample_bezpath(verbs: &[PathVerb]) -> Vec<QuadraticSegment> {
    let mut out = Vec::new();
    let mut cursor: [f32; 2] = [0.0, 0.0];
    let mut subpath_start: [f32; 2] = [0.0, 0.0];
    for verb in verbs {
        match verb {
            PathVerb::MoveTo { to } => {
                cursor = v2(*to);
                subpath_start = cursor;
            }
            PathVerb::LineTo { to } => {
                let p2 = v2(*to);
                out.push(QuadraticSegment {
                    p0: cursor,
                    p1: midpoint(cursor, p2),
                    p2,
                });
                cursor = p2;
            }
            PathVerb::QuadTo { ctrl, to } => {
                let p1 = v2(*ctrl);
                let p2 = v2(*to);
                out.push(QuadraticSegment { p0: cursor, p1, p2 });
                cursor = p2;
            }
            PathVerb::CubicTo { ctrl1, ctrl2, to } => {
                let c1 = v2(*ctrl1);
                let c2 = v2(*ctrl2);
                let p3 = v2(*to);
                subdivide_cubic_to_quads(cursor, c1, c2, p3, CUBIC_SPLIT_DEPTH, &mut out);
                cursor = p3;
            }
            PathVerb::Close {} => {
                if cursor != subpath_start {
                    out.push(QuadraticSegment {
                        p0: cursor,
                        p1: midpoint(cursor, subpath_start),
                        p2: subpath_start,
                    });
                }
                cursor = subpath_start;
            }
        }
    }
    out
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

/// Convert a polyline in IR space into a stream of degenerate quadratic
/// segments, each encoding one linear edge. `closed` appends a final edge
/// from the last point back to the first. Returns an empty list if the
/// polyline is degenerate.
pub fn polyline_to_segments(points: &[Vec3], closed: bool) -> Vec<QuadraticSegment> {
    if points.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(points.len());
    let mut prev = v2(points[0]);
    for p in &points[1..] {
        let cur = v2(*p);
        out.push(QuadraticSegment {
            p0: prev,
            p1: midpoint(prev, cur),
            p2: cur,
        });
        prev = cur;
    }
    if closed {
        let start = v2(points[0]);
        if prev != start {
            out.push(QuadraticSegment {
                p0: prev,
                p1: midpoint(prev, start),
                p2: start,
            });
        }
    }
    out
}

/// Fill the interior of a closed polyline. Returns an empty mesh for open
/// polylines — fill is meaningless without closure.
pub(crate) fn tessellate_polyline_fill(
    tess: &mut FillTessellator,
    points: &[Vec3],
    closed: bool,
) -> FillMesh {
    if !closed || points.len() < 3 {
        return FillMesh {
            vertices: Vec::new(),
            indices: Vec::new(),
        };
    }
    let path = polyline_to_path(points, true);
    tessellate_fill_path(tess, &path)
}

/// Fill a `BezPath` interior. Manimgl uses non-zero fill; we match that.
pub(crate) fn tessellate_bezpath_fill(tess: &mut FillTessellator, verbs: &[PathVerb]) -> FillMesh {
    if verbs.is_empty() {
        return FillMesh {
            vertices: Vec::new(),
            indices: Vec::new(),
        };
    }
    let path = verbs_to_path(verbs);
    tessellate_fill_path(tess, &path)
}

/// Curve-flattening tolerance for fill tessellation, in world units. lyon's
/// `FillOptions::DEFAULT` is 0.25, which is wildly too coarse at our scale —
/// `WORLD_UNITS_PER_EM = 1.0` means a glyph spans ~1 world unit, so 0.25
/// turns each cubic into ~4 line segments and leaves visible polygon-corner
/// "hooks" at curve terminals (Tex glyphs especially). 0.001 (1/1000 em) is
/// well below a pixel at any reasonable render resolution and produces clean
/// outlines. Polylines pay nothing for this since they're already linear.
const FILL_TOLERANCE: f32 = 0.001;

fn tessellate_fill_path(tess: &mut FillTessellator, path: &Path) -> FillMesh {
    let mut buffers: VertexBuffers<FillVertex, u32> = VertexBuffers::new();
    let opts = FillOptions::tolerance(FILL_TOLERANCE).with_fill_rule(FillRule::NonZero);
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
    fn closed_polyline_appends_closing_segment() {
        let points: Vec<Vec3> = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]];
        let segs = polyline_to_segments(&points, true);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[2].p0, [1.0, 1.0]);
        assert_eq!(segs[2].p2, [0.0, 0.0]);
    }

    #[test]
    fn open_polyline_omits_closing_segment() {
        let points: Vec<Vec3> = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]];
        let segs = polyline_to_segments(&points, false);
        assert_eq!(segs.len(), 2);
    }
}
