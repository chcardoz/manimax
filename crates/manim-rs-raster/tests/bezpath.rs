//! Slice C Step 5 — BezPath rendering: a verb-stream path with quadratic
//! and cubic segments tessellates to both stroke and fill geometry.

use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{Fill, Object, PathVerb, Stroke};
use manim_rs_raster::{Camera, Runtime};

const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;

/// A teardrop: MoveTo → QuadTo → CubicTo → Close. Exercises every non-trivial
/// `PathVerb` variant in a single small fixture.
fn teardrop_verbs() -> Vec<PathVerb> {
    vec![
        PathVerb::MoveTo {
            to: [0.0, -1.0, 0.0],
        },
        PathVerb::QuadTo {
            ctrl: [1.5, -0.5, 0.0],
            to: [0.5, 1.0, 0.0],
        },
        PathVerb::CubicTo {
            ctrl1: [0.0, 1.5, 0.0],
            ctrl2: [-1.5, 1.0, 0.0],
            to: [-0.5, 0.0, 0.0],
        },
        PathVerb::Close {},
    ]
}

fn count_lit(rgba: &[u8]) -> usize {
    rgba.chunks_exact(4)
        .filter(|px| px[0] > 8 || px[1] > 8 || px[2] > 8)
        .count()
}

#[test]
fn bezpath_stroke_renders() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::BezPath {
                verbs: teardrop_verbs(),
                stroke: Some(Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.1)),
                fill: None,
            },
            [0.0, 0.0, 0.0],
        )],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    let lit = count_lit(&pixels);
    assert!(
        lit > 500,
        "BezPath stroke should render visible outline, got {lit}"
    );
}

#[test]
fn bezpath_fill_renders_interior() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::BezPath {
                verbs: teardrop_verbs(),
                stroke: None,
                fill: Some(Fill {
                    color: [0.0, 1.0, 0.0, 1.0],
                }),
            },
            [0.0, 0.0, 0.0],
        )],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    // Interior of the teardrop must contain substantially more lit pixels
    // than just a stroked outline would.
    let lit = count_lit(&pixels);
    assert!(
        lit > 1_500,
        "BezPath fill interior should be large, got {lit}"
    );

    // And it should be dominantly green.
    let green_dominant = pixels
        .chunks_exact(4)
        .filter(|px| px[1] > 150 && px[0] < 50 && px[2] < 50)
        .count();
    assert!(
        green_dominant > 1_000,
        "fill color should dominate the interior, got {green_dominant}"
    );
}

#[test]
fn bezpath_with_stroke_and_fill_draws_both() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::BezPath {
                verbs: teardrop_verbs(),
                stroke: Some(Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.1)),
                fill: Some(Fill {
                    color: [0.0, 0.0, 1.0, 1.0],
                }),
            },
            [0.0, 0.0, 0.0],
        )],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    let blue_only = pixels
        .chunks_exact(4)
        .filter(|px| px[2] > 200 && px[0] < 40 && px[1] < 40)
        .count();
    let white_ish = pixels
        .chunks_exact(4)
        .filter(|px| px[0] > 200 && px[1] > 200 && px[2] > 200)
        .count();

    assert!(blue_only > 1_000, "expected blue interior, got {blue_only}");
    assert!(
        white_ish > 100,
        "expected white stroke pixels, got {white_ish}"
    );
}

#[test]
fn empty_bezpath_renders_nothing() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::BezPath {
                verbs: vec![],
                stroke: Some(Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.1)),
                fill: Some(Fill {
                    color: [1.0, 1.0, 1.0, 1.0],
                }),
            },
            [0.0, 0.0, 0.0],
        )],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    let lit = count_lit(&pixels);
    assert_eq!(lit, 0, "empty bezpath must not render any geometry");
}
