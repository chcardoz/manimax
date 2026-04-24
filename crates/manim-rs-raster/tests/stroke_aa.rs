//! Analytic SDF AA at the stroke edge: renders a diagonal stroke and asserts
//! that edge rows contain intermediate (non-saturated) alpha values —
//! evidence the fragment shader fades via `smoothstep` rather than hard
//! cutting. Without AA every pixel would be either 0 or 255.

use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{Object, Stroke};
use manim_rs_raster::{Camera, Runtime};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 144;

#[test]
fn stroke_edge_has_intermediate_alpha() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::Polyline {
                // Diagonal line — sub-pixel coverage along the slope produces
                // a broad spread of intermediate alpha values.
                points: vec![[-3.0, -2.0, 0.0], [3.0, 2.0, 0.0]],
                closed: false,
                stroke: Some(Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.15)),
                fill: None,
            },
            [0.0, 0.0, 0.0],
        )],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    // Count pixels in the fade band. A hard-cut stroke has none; a smoothstep
    // fade + MSAA partial coverage along a diagonal produces many.
    let mut fade_band = 0usize;
    let mut saturated = 0usize;
    for row in 0..HEIGHT as usize {
        for col in 0..WIDTH as usize {
            let i = (row * WIDTH as usize + col) * 4;
            let r = pixels[i];
            if (16..=230).contains(&r) {
                fade_band += 1;
            } else if r > 230 {
                saturated += 1;
            }
        }
    }
    assert!(saturated > 0, "stroke should produce some saturated pixels");
    assert!(
        fade_band >= 32,
        "expected many fade-band pixels along diagonal stroke, got {fade_band}"
    );
}
