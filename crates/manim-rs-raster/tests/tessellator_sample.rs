//! `sample_bezpath`: BezPath verbs → flat list of 2D quadratic Bézier
//! segments. One fixture per verb variant; endpoint continuity asserted
//! across the whole stream.

use manim_rs_ir::PathVerb;
use manim_rs_raster::{QuadraticSegment, sample_bezpath};

fn approx_eq(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
}

fn assert_continuous(segments: &[QuadraticSegment]) {
    for w in segments.windows(2) {
        assert!(
            approx_eq(w[0].p2, w[1].p0),
            "gap between segments: {:?} → {:?}",
            w[0].p2,
            w[1].p0,
        );
    }
}

#[test]
fn empty_verbs_yield_no_segments() {
    assert!(sample_bezpath(&[]).is_empty());
}

#[test]
fn move_to_alone_emits_nothing() {
    let verbs = vec![PathVerb::MoveTo {
        to: [1.0, 2.0, 0.0],
    }];
    assert!(sample_bezpath(&verbs).is_empty());
}

#[test]
fn line_to_emits_degenerate_quadratic_with_midpoint_control() {
    let verbs = vec![
        PathVerb::MoveTo {
            to: [0.0, 0.0, 0.0],
        },
        PathVerb::LineTo {
            to: [2.0, 0.0, 0.0],
        },
    ];
    let segs = sample_bezpath(&verbs);
    assert_eq!(segs.len(), 1);
    assert!(approx_eq(segs[0].p0, [0.0, 0.0]));
    assert!(approx_eq(segs[0].p1, [1.0, 0.0]));
    assert!(approx_eq(segs[0].p2, [2.0, 0.0]));
}

#[test]
fn quad_to_passes_through_unchanged() {
    let verbs = vec![
        PathVerb::MoveTo {
            to: [0.0, 0.0, 0.0],
        },
        PathVerb::QuadTo {
            ctrl: [1.0, 2.0, 0.0],
            to: [2.0, 0.0, 0.0],
        },
    ];
    let segs = sample_bezpath(&verbs);
    assert_eq!(segs.len(), 1);
    assert!(approx_eq(segs[0].p0, [0.0, 0.0]));
    assert!(approx_eq(segs[0].p1, [1.0, 2.0]));
    assert!(approx_eq(segs[0].p2, [2.0, 0.0]));
}

#[test]
fn cubic_to_splits_to_four_quadratics() {
    let verbs = vec![
        PathVerb::MoveTo {
            to: [0.0, 0.0, 0.0],
        },
        PathVerb::CubicTo {
            ctrl1: [1.0, 3.0, 0.0],
            ctrl2: [3.0, 3.0, 0.0],
            to: [4.0, 0.0, 0.0],
        },
    ];
    let segs = sample_bezpath(&verbs);
    assert_eq!(segs.len(), 4, "fixed subdivision depth 2 → 4 quadratics");
    assert!(approx_eq(segs[0].p0, [0.0, 0.0]));
    assert!(approx_eq(segs.last().unwrap().p2, [4.0, 0.0]));
    assert_continuous(&segs);
}

#[test]
fn close_connects_back_to_subpath_start() {
    let verbs = vec![
        PathVerb::MoveTo {
            to: [0.0, 0.0, 0.0],
        },
        PathVerb::LineTo {
            to: [1.0, 0.0, 0.0],
        },
        PathVerb::LineTo {
            to: [1.0, 1.0, 0.0],
        },
        PathVerb::Close {},
    ];
    let segs = sample_bezpath(&verbs);
    assert_eq!(segs.len(), 3, "2 line_to + 1 closing line = 3");
    assert!(approx_eq(segs[2].p0, [1.0, 1.0]));
    assert!(approx_eq(segs[2].p2, [0.0, 0.0]));
    assert_continuous(&segs);
}

#[test]
fn close_is_noop_when_cursor_already_at_subpath_start() {
    let verbs = vec![
        PathVerb::MoveTo {
            to: [0.0, 0.0, 0.0],
        },
        PathVerb::LineTo {
            to: [1.0, 0.0, 0.0],
        },
        PathVerb::LineTo {
            to: [0.0, 0.0, 0.0],
        },
        PathVerb::Close {},
    ];
    let segs = sample_bezpath(&verbs);
    assert_eq!(segs.len(), 2, "Close must not emit when already at start");
}

#[test]
fn multiple_subpaths_each_reset_close_target() {
    let verbs = vec![
        PathVerb::MoveTo {
            to: [0.0, 0.0, 0.0],
        },
        PathVerb::LineTo {
            to: [1.0, 0.0, 0.0],
        },
        PathVerb::Close {},
        PathVerb::MoveTo {
            to: [5.0, 5.0, 0.0],
        },
        PathVerb::LineTo {
            to: [6.0, 5.0, 0.0],
        },
        PathVerb::Close {},
    ];
    let segs = sample_bezpath(&verbs);
    assert_eq!(segs.len(), 4);
    assert!(approx_eq(segs[1].p2, [0.0, 0.0]));
    assert!(approx_eq(segs[2].p0, [5.0, 5.0]));
    assert!(approx_eq(segs[3].p2, [5.0, 5.0]));
}

#[test]
fn mixed_verbs_maintain_endpoint_continuity() {
    let verbs = vec![
        PathVerb::MoveTo {
            to: [0.0, 0.0, 0.0],
        },
        PathVerb::LineTo {
            to: [1.0, 0.0, 0.0],
        },
        PathVerb::QuadTo {
            ctrl: [2.0, 1.0, 0.0],
            to: [3.0, 0.0, 0.0],
        },
        PathVerb::CubicTo {
            ctrl1: [4.0, 1.0, 0.0],
            ctrl2: [5.0, -1.0, 0.0],
            to: [6.0, 0.0, 0.0],
        },
        PathVerb::Close {},
    ];
    let segs = sample_bezpath(&verbs);
    // 1 line + 1 quad + 4 cubic-derived + 1 closing = 7
    assert_eq!(segs.len(), 7);
    assert!(approx_eq(segs[0].p0, [0.0, 0.0]));
    assert!(approx_eq(segs.last().unwrap().p2, [0.0, 0.0]));
    assert_continuous(&segs);
}
