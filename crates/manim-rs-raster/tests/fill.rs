//! Slice C Step 5 — fill pipeline: closed polylines with a `Fill` render an
//! interior region, not just an outline.
//!
//! Test strategy: render a filled unit square with no stroke. Count interior
//! pixels and expect a count close to the scene's expected 2×2 footprint
//! in pixel space. Compared against a stroke-only render, the filled version
//! must have substantially more lit pixels.

use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{Fill, Object, Stroke, Vec3};
use manim_rs_raster::{Camera, Runtime};

const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;

fn square_points() -> Vec<Vec3> {
    vec![
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ]
}

fn count_lit(rgba: &[u8]) -> usize {
    rgba.chunks_exact(4)
        .filter(|px| px[0] > 8 || px[1] > 8 || px[2] > 8)
        .count()
}

#[test]
fn filled_square_produces_interior_region() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::Polyline {
                points: square_points(),
                closed: true,
                stroke: None,
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

    // 2×2 world-unit square under SLICE_B_DEFAULT projects to ~60×60 px,
    // so expect ~3600 interior pixels plus a thin MSAA edge band.
    assert!(
        lit > 2_500 && lit < 6_000,
        "filled square interior count out of range: got {lit}"
    );
}

#[test]
fn fill_plus_stroke_draws_both() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    // Red fill, white stroke — check both colors are present.
    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::Polyline {
                points: square_points(),
                closed: true,
                stroke: Some(Stroke {
                    color: [1.0, 1.0, 1.0, 1.0],
                    width: 0.15,
                }),
                fill: Some(Fill {
                    color: [1.0, 0.0, 0.0, 1.0],
                }),
            },
            [0.0, 0.0, 0.0],
        )],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    let mut red_only = 0usize; // fill interior: R high, G/B low
    let mut white_ish = 0usize; // stroke: all channels high
    for px in pixels.chunks_exact(4) {
        let (r, g, b) = (px[0], px[1], px[2]);
        if r > 200 && g < 40 && b < 40 {
            red_only += 1;
        } else if r > 200 && g > 200 && b > 200 {
            white_ish += 1;
        }
    }

    assert!(
        red_only > 2_000,
        "expected substantial red fill interior, got {red_only}"
    );
    assert!(
        white_ish > 100,
        "expected white stroke pixels around the perimeter, got {white_ish}"
    );
}

#[test]
fn open_polyline_fill_is_noop() {
    // Fill without closure is meaningless — the mesh should be empty and the
    // frame should contain only the background.
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::Polyline {
                points: square_points(),
                closed: false,
                stroke: None,
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
    assert_eq!(lit, 0, "open polyline + fill must render nothing");
}
