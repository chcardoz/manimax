//! Shared `kurbo::BezPath` → IR `Vec<PathVerb>` conversion. Used by both
//! the Tex and Text compilation pipelines, which receive shaped outlines
//! as `BezPath` and need them as the IR's flat verb representation.

use kurbo::{BezPath, PathEl};
use manim_rs_ir::PathVerb;

pub(crate) fn bezpath_to_verbs(path: &BezPath) -> Vec<PathVerb> {
    path.elements()
        .iter()
        .map(|el| match *el {
            PathEl::MoveTo(p) => PathVerb::MoveTo {
                to: [p.x as f32, p.y as f32, 0.0],
            },
            PathEl::LineTo(p) => PathVerb::LineTo {
                to: [p.x as f32, p.y as f32, 0.0],
            },
            PathEl::QuadTo(c, p) => PathVerb::QuadTo {
                ctrl: [c.x as f32, c.y as f32, 0.0],
                to: [p.x as f32, p.y as f32, 0.0],
            },
            PathEl::CurveTo(c1, c2, p) => PathVerb::CubicTo {
                ctrl1: [c1.x as f32, c1.y as f32, 0.0],
                ctrl2: [c2.x as f32, c2.y as f32, 0.0],
                to: [p.x as f32, p.y as f32, 0.0],
            },
            PathEl::ClosePath => PathVerb::Close {},
        })
        .collect()
}
