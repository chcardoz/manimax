//! Slice B step 5 eyeball test — render a hardcoded square through the stroke
//! pipeline, save a PNG.
//!
//! ```sh
//! cargo run -p manim-rs-raster --example stroke_square_png
//! open /tmp/stroke_square.png
//! ```
//!
//! Expect: white square outline on black, centered, ~middle third of frame.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{Object, Stroke};
use manim_rs_raster::{Camera, Runtime};

const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;
const BACKGROUND: [f64; 4] = [0.0, 0.0, 0.0, 1.0];

fn main() {
    let square = Object::Polyline {
        points: vec![
            [-2.0, -2.0, 0.0],
            [2.0, -2.0, 0.0],
            [2.0, 2.0, 0.0],
            [-2.0, 2.0, 0.0],
        ],
        closed: true,
        stroke: Some(Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.08)),
        fill: None,
    };

    let state = SceneState {
        objects: vec![ObjectState::with_defaults(1, square, [0.0, 0.0, 0.0])],
    };

    let runtime = Runtime::new(WIDTH, HEIGHT).expect("Runtime init");
    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, BACKGROUND)
        .expect("render");
    assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);

    let out = Path::new("/tmp/stroke_square.png");
    write_png(out, WIDTH, HEIGHT, &pixels);
    println!("wrote {}", out.display());
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
    let file = File::create(path).expect("create output");
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(rgba).expect("png data");
}
