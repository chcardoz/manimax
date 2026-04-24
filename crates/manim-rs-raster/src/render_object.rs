//! Per-object render prep — tessellation, paint resolution, MVP build, and
//! geometry-size guard. The runtime calls [`tessellate_object`] and
//! [`build_mvp`] for each `ObjectState` before submitting a render pass.

use glam::Mat4;
use lyon::tessellation::VertexBuffers;
use manim_rs_eval::ObjectState;
use manim_rs_ir::{Object, RgbaSrgb, StrokeWidth};

use crate::tessellator::{
    FillMesh, StrokeVertex, expand_stroke, polyline_to_segments, sample_bezpath,
    tessellate_bezpath_fill, tessellate_polyline_fill,
};
use crate::{INDEX_BUFFER_SIZE, RuntimeError};

/// Paint an `ObjectState` resolves into for one frame. Either side may be
/// absent (no stroke / no fill, or zero-index mesh after tessellation).
pub(crate) struct ObjectDraw {
    pub(crate) fill: Option<(FillMesh, RgbaSrgb)>,
    pub(crate) stroke: Option<VertexBuffers<StrokeVertex, u32>>,
}

impl ObjectDraw {
    pub(crate) fn is_empty(&self) -> bool {
        self.fill.is_none() && self.stroke.is_none()
    }
}

/// Tessellate the object's geometry and resolve its paint colors. Returns
/// an empty `ObjectDraw` (both sides `None`) if no paint produces drawable
/// indices — the caller skips empty draws.
pub(crate) fn tessellate_object(state: &ObjectState) -> ObjectDraw {
    let (fill_raw, stroke_segments, stroke_info) = match &*state.object {
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
                .map(|_| polyline_to_segments(points, *closed)),
            stroke.as_ref(),
        ),
        Object::BezPath {
            verbs,
            stroke,
            fill,
        } => (
            fill.as_ref()
                .map(|f| (tessellate_bezpath_fill(verbs), f.color)),
            stroke.as_ref().map(|_| sample_bezpath(verbs)),
            stroke.as_ref(),
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

    let stroke = match (stroke_segments, stroke_info) {
        (Some(segs), Some(stroke)) if !segs.is_empty() => {
            let resolved = with_opacity(
                resolve_color(stroke.color, state.color_override),
                state.opacity,
            );
            let widths = resolve_stroke_widths(&stroke.width, segs.len());
            let bufs = expand_stroke(&segs, &widths, resolved, stroke.joint);
            (!bufs.indices.is_empty()).then_some(bufs)
        }
        _ => None,
    };

    ObjectDraw { fill, stroke }
}

/// Produce the widths slice `expand_stroke` expects. Scalar → length 1;
/// per-vertex matching `segments+1` → pass-through; closed-polyline
/// off-by-one (N points, N+1 endpoints after the closing edge) → pad by
/// re-using `widths[0]`; any other mismatch falls back to the first width,
/// so an IR-level invariant breach degrades to uniform width rather than
/// panicking.
pub(crate) fn resolve_stroke_widths(width: &StrokeWidth, segment_count: usize) -> Vec<f32> {
    let expected = segment_count + 1;
    match width {
        StrokeWidth::Scalar(v) => vec![*v],
        StrokeWidth::PerVertex(v) if v.len() == expected => v.clone(),
        StrokeWidth::PerVertex(v) if v.len() + 1 == expected && !v.is_empty() => {
            let mut w = v.clone();
            w.push(v[0]);
            w
        }
        StrokeWidth::PerVertex(v) => vec![v.first().copied().unwrap_or(0.0)],
    }
}

fn resolve_color(base: RgbaSrgb, color_override: Option<RgbaSrgb>) -> RgbaSrgb {
    color_override.unwrap_or(base)
}

fn with_opacity(mut color: RgbaSrgb, opacity: f32) -> RgbaSrgb {
    color[3] *= opacity;
    color
}

/// Compose the object's MVP matrix from its position, rotation, and scale.
pub(crate) fn build_mvp(projection: &Mat4, state: &ObjectState) -> Mat4 {
    let translation = Mat4::from_translation(glam::Vec3::new(
        state.position[0],
        state.position[1],
        state.position[2],
    ));
    let rotation = Mat4::from_rotation_z(state.rotation);
    let scale = Mat4::from_scale(glam::Vec3::splat(state.scale));
    *projection * translation * rotation * scale
}

/// Build a `LoadOp::Clear` for a background color expressed as an `[f64; 4]`.
pub(crate) fn clear_load(background: [f64; 4]) -> wgpu::LoadOp<wgpu::Color> {
    wgpu::LoadOp::Clear(wgpu::Color {
        r: background[0],
        g: background[1],
        b: background[2],
        a: background[3],
    })
}

/// Reject geometry that would overflow the per-object vertex / index caps.
pub(crate) fn check_geometry_size(
    v_len: u64,
    i_len: u64,
    vertex_cap: u64,
) -> Result<(), RuntimeError> {
    if v_len > vertex_cap {
        return Err(RuntimeError::GeometryOverflow {
            kind: "vertex",
            needed: v_len,
            cap: vertex_cap,
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
