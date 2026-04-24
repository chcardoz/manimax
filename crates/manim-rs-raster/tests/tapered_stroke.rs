//! Per-vertex stroke width rasterizes as a visibly tapered ribbon. Author
//! supplies one width per point on a horizontal polyline; the middle pixel
//! column should be thicker than the endpoints.

use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{JointKind, Object, Stroke, StrokeWidth};
use manim_rs_raster::{Camera, Runtime};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 144;

fn lit_row_count_at(pixels: &[u8], col: usize) -> usize {
    let mut count = 0;
    for row in 0..HEIGHT as usize {
        let i = (row * WIDTH as usize + col) * 4;
        // Background is opaque black; count pixels whose red channel indicates
        // the white stroke shows through.
        if pixels[i] > 16 {
            count += 1;
        }
    }
    count
}

#[test]
fn tapered_polyline_renders_a_thicker_middle() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    // 5-point horizontal polyline spanning most of the frame. Widths taper
    // from 0.02 at the ends to 0.25 at the middle.
    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::Polyline {
                points: vec![
                    [-4.0, 0.0, 0.0],
                    [-2.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0],
                    [2.0, 0.0, 0.0],
                    [4.0, 0.0, 0.0],
                ],
                closed: false,
                stroke: Some(Stroke {
                    color: [1.0, 1.0, 1.0, 1.0],
                    width: StrokeWidth::PerVertex(vec![0.02, 0.1, 0.25, 0.1, 0.02]),
                    joint: JointKind::Auto,
                }),
                fill: None,
            },
            [0.0, 0.0, 0.0],
        )],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    // Columns mapping: x-span = 16, so 1 pixel = 0.0625 scene units.
    // Middle column = WIDTH/2 = 128; endpoints are near cols 32 and 224.
    let center_rows = lit_row_count_at(&pixels, 128);
    let quarter_rows = lit_row_count_at(&pixels, 80);
    let tail_rows = lit_row_count_at(&pixels, 40);

    assert!(
        center_rows > quarter_rows,
        "center ({center_rows}) should be thicker than the 1/4 point ({quarter_rows})",
    );
    assert!(
        quarter_rows > tail_rows,
        "1/4 point ({quarter_rows}) should be thicker than the tail ({tail_rows})",
    );
    // Absolute sanity: the tapered center with width 0.25 is ~4 pixels tall.
    assert!(
        center_rows >= 3,
        "center should cover >=3 rows, got {center_rows}"
    );
}
