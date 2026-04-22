//! Slice C Step 5 — MSAA: diagonal strokes produce partially-covered edge
//! pixels (intermediate intensities), proving 4× multi-sampling is wired up
//! end-to-end from render pass → resolve → readback.
//!
//! Without MSAA, a 1-bit rasterizer would paint every edge pixel either at
//! background (0,0,0) or at the stroke color (255,255,255). With MSAA we
//! expect a meaningful count of pixels whose RGB channels sit somewhere
//! between those extremes.

use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{Object, Stroke, Vec3};
use manim_rs_raster::{Camera, Runtime};

const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;

#[test]
fn diagonal_stroke_has_antialiased_edge_pixels() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    // 45° diagonal line — worst case for sample coverage on a grid.
    let pts: Vec<Vec3> = vec![[-2.0, -1.5, 0.0], [2.0, 1.5, 0.0]];
    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::Polyline {
                points: pts,
                closed: false,
                stroke: Some(Stroke {
                    color: [1.0, 1.0, 1.0, 1.0],
                    width: 0.05,
                }),
                fill: None,
            },
            [0.0, 0.0, 0.0],
        )],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    // Count pixels whose green channel is in the "partial coverage" band.
    // With a pure 1-bit rasterizer, the green channel would be either 0 or
    // 255 — any intermediate value is direct evidence of MSAA resolve.
    let (mut full, mut partial) = (0usize, 0usize);
    for px in pixels.chunks_exact(4) {
        let g = px[1];
        if g > 250 {
            full += 1;
        } else if g > 30 && g < 220 {
            partial += 1;
        }
    }

    assert!(full > 0, "expected some fully-covered stroke pixels");
    assert!(
        partial > 50,
        "expected intermediate-intensity edge pixels from MSAA resolve, got {partial}"
    );
    // Sanity: partial pixels should roughly keep pace with fully-covered ones
    // along a diagonal stroke. If partial << full, we'd be looking at a
    // single-sample rasterizer.
    assert!(
        partial as f64 > (full as f64) * 0.25,
        "partial-coverage pixel ratio too low — MSAA may be disabled. full={full}, partial={partial}"
    );
}
