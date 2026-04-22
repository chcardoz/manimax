//! Slice B step 4 eyeball test — clear an offscreen target and write a PNG.
//!
//! Proves the full wgpu init + clear + readback path on this machine. Run:
//!
//! ```sh
//! cargo run -p manim-rs-raster --example clear_png
//! open /tmp/clear.png
//! ```

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use manim_rs_raster::Runtime;

const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;
// A recognizable red so "did the clear actually happen?" is obvious on sight.
// The framebuffer is Rgba8UnormSrgb, so wgpu gamma-encodes on write —
// these floats are treated as linear and come out looking like a vivid red.
const CLEAR_COLOR: [f64; 4] = [0.8, 0.1, 0.2, 1.0];

fn main() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("wgpu Runtime init");
    let pixels = runtime.render_clear(CLEAR_COLOR).expect("render_clear");
    assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);

    let out_path = Path::new("/tmp/clear.png");
    write_png(out_path, WIDTH, HEIGHT, &pixels);
    println!("wrote {} ({} bytes)", out_path.display(), pixels.len());
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
    let file = File::create(path).expect("create output file");
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(rgba).expect("png data");
}
