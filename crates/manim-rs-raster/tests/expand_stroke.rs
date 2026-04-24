//! `expand_stroke`: flat quadratics → per-vertex widened ribbon of triangles
//! with miter/bevel joints. Fixtures cover straight line, miter L-bend,
//! bevel L-bend, tapered width.

use manim_rs_ir::JointKind;
use manim_rs_raster::{QuadraticSegment, StrokeVertex, expand_stroke};

fn straight_line(p0: [f32; 2], p2: [f32; 2]) -> QuadraticSegment {
    QuadraticSegment {
        p0,
        p1: [(p0[0] + p2[0]) * 0.5, (p0[1] + p2[1]) * 0.5],
        p2,
    }
}

fn min_max<F: Fn(&StrokeVertex) -> f32>(vs: &[StrokeVertex], f: F) -> (f32, f32) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for v in vs {
        let x = f(v);
        if x < lo {
            lo = x;
        }
        if x > hi {
            hi = x;
        }
    }
    (lo, hi)
}

#[test]
fn empty_input_yields_empty_buffers() {
    let bufs = expand_stroke(&[], &[1.0], [1.0, 1.0, 1.0, 1.0], JointKind::Auto);
    assert!(bufs.vertices.is_empty());
    assert!(bufs.indices.is_empty());
}

#[test]
fn straight_line_uniform_width_produces_parallel_edges() {
    let seg = straight_line([0.0, 0.0], [0.05, 0.0]);
    let bufs = expand_stroke(&[seg], &[0.2], [1.0, 0.0, 0.0, 1.0], JointKind::Auto);
    assert!(bufs.vertices.len() >= 4, "at least 2 sample pairs");
    assert_eq!(bufs.indices.len() % 3, 0);

    let (y_min, y_max) = min_max(&bufs.vertices, |v| v.position[1]);
    assert!(
        (y_max - 0.1).abs() < 1e-5,
        "top edge at +w/2, got {}",
        y_max
    );
    assert!(
        (y_min + 0.1).abs() < 1e-5,
        "bottom edge at -w/2, got {}",
        y_min
    );

    for v in &bufs.vertices {
        assert!((v.stroke_width - 0.2).abs() < 1e-5);
        assert_eq!(v.color, [1.0, 0.0, 0.0, 1.0]);
    }
}

#[test]
fn tapered_width_interpolates_across_segment() {
    let seg = straight_line([0.0, 0.0], [0.05, 0.0]);
    let bufs = expand_stroke(&[seg], &[0.5, 2.0], [1.0; 4], JointKind::Auto);

    let (w_min, w_max) = min_max(&bufs.vertices, |v| v.stroke_width);
    assert!(
        (w_min - 0.5).abs() < 1e-4,
        "narrow end stroke_width ~ 0.5, got {}",
        w_min
    );
    assert!(
        (w_max - 2.0).abs() < 1e-4,
        "wide end stroke_width ~ 2.0, got {}",
        w_max
    );

    let (y_min, y_max) = min_max(&bufs.vertices, |v| v.position[1]);
    assert!(
        y_max > 0.99 && y_max < 1.01,
        "wide end y ~ ±1.0, got {}",
        y_max
    );
    assert!(y_min < -0.99 && y_min > -1.01);
}

#[test]
fn miter_joint_on_90_degree_l_bend() {
    // L-shape: (0,0) -> (0.05,0) -> (0.05,0.05). Short segments to keep
    // step counts small; width 0.2 so miter points are visibly offset.
    let s1 = straight_line([0.0, 0.0], [0.05, 0.0]);
    let s2 = straight_line([0.05, 0.0], [0.05, 0.05]);
    let bufs = expand_stroke(&[s1, s2], &[0.2], [1.0; 4], JointKind::Miter);

    // Expected miter points at the corner (0.05, 0.0):
    //   miter_left  = (0.05 - 0.1, 0.0 + 0.1) = (-0.05, 0.1)
    //   miter_right = (0.05 + 0.1, 0.0 - 0.1) = ( 0.15, -0.1)
    let has_miter_left = bufs
        .vertices
        .iter()
        .any(|v| (v.position[0] + 0.05).abs() < 1e-4 && (v.position[1] - 0.1).abs() < 1e-4);
    let has_miter_right = bufs
        .vertices
        .iter()
        .any(|v| (v.position[0] - 0.15).abs() < 1e-4 && (v.position[1] + 0.1).abs() < 1e-4);
    assert!(has_miter_left, "miter left vertex missing");
    assert!(has_miter_right, "miter right vertex missing");

    let miter_vert = bufs
        .vertices
        .iter()
        .find(|v| (v.position[0] + 0.05).abs() < 1e-4 && (v.position[1] - 0.1).abs() < 1e-4)
        .unwrap();
    assert!(
        miter_vert.joint_angle.abs() > 0.1,
        "joint_angle should be non-zero at corner"
    );
}

#[test]
fn bevel_joint_on_90_degree_l_bend_does_not_emit_miter_vertices() {
    let s1 = straight_line([0.0, 0.0], [0.05, 0.0]);
    let s2 = straight_line([0.05, 0.0], [0.05, 0.05]);
    let bufs = expand_stroke(&[s1, s2], &[0.2], [1.0; 4], JointKind::Bevel);

    let has_miter_left = bufs
        .vertices
        .iter()
        .any(|v| (v.position[0] + 0.05).abs() < 1e-4 && (v.position[1] - 0.1).abs() < 1e-4);
    assert!(
        !has_miter_left,
        "bevel must not emit miter vertex at (-0.05, 0.1)"
    );

    // Every vertex is either on segment 1's ribbon (x in [0, 0.05], y in [-0.1, 0.1])
    // or segment 2's ribbon (x in [-0.05, 0.15], y in [0, 0.05]).
    for v in &bufs.vertices {
        let on_s1 = v.position[0] >= -1e-4
            && v.position[0] <= 0.05 + 1e-4
            && v.position[1].abs() <= 0.1 + 1e-4;
        let on_s2 = v.position[1] >= -1e-4
            && v.position[1] <= 0.05 + 1e-4
            && (v.position[0] - 0.05).abs() <= 0.1 + 1e-4;
        assert!(
            on_s1 || on_s2,
            "vertex outside the union of the two ribbons: {:?}",
            v.position
        );
    }
}

#[test]
fn triangle_indices_are_valid() {
    let s1 = straight_line([0.0, 0.0], [0.05, 0.0]);
    let s2 = straight_line([0.05, 0.0], [0.05, 0.05]);
    for joint in [JointKind::Miter, JointKind::Bevel, JointKind::Auto] {
        let bufs = expand_stroke(&[s1, s2], &[0.2], [1.0; 4], joint);
        assert_eq!(bufs.indices.len() % 3, 0);
        let n = bufs.vertices.len() as u32;
        for &idx in &bufs.indices {
            assert!(idx < n, "index {} >= vertex count {}", idx, n);
        }
    }
}
