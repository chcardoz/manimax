//! Manimax runtime glue — orchestrates eval → raster → encode.
//!
//! `render_to_mp4(scene, out)` reads metadata off the IR, stands up a wgpu
//! `Runtime` and an ffmpeg `Encoder`, evaluates every frame, rasterizes, and
//! streams the pixels into the encoder. Nothing clever: the evaluator is pure,
//! so frames are independent and this loop is the thinnest possible driver.

use std::path::Path;

use manim_rs_encode::{EncodeError, Encoder};
use manim_rs_eval::eval_at;
use manim_rs_ir::Scene;
use manim_rs_raster::{Camera, Runtime, RuntimeError as RasterError};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("rasterizer init/render failed: {0}")]
    Raster(#[from] RasterError),
    #[error("encoder failed: {0}")]
    Encode(#[from] EncodeError),
}

/// Render a scene to an mp4 at `out`. Frame count = round(fps * duration).
pub fn render_to_mp4(scene: &Scene, out: &Path) -> Result<(), RuntimeError> {
    let meta = &scene.metadata;
    let total_frames = (f64::from(meta.fps) * meta.duration).round().max(0.0) as u32;

    let runtime = Runtime::new(meta.resolution.width, meta.resolution.height)?;
    let mut encoder = Encoder::start(out, meta.resolution.width, meta.resolution.height, meta.fps)?;

    let camera = Camera::SLICE_B_DEFAULT;
    let background: [f64; 4] = [
        meta.background[0] as f64,
        meta.background[1] as f64,
        meta.background[2] as f64,
        meta.background[3] as f64,
    ];

    for frame_idx in 0..total_frames {
        let t = f64::from(frame_idx) / f64::from(meta.fps);
        let state = eval_at(scene, t);
        let pixels = runtime.render(&state, &camera, background)?;
        encoder.push_frame(&pixels)?;
    }

    encoder.finish()?;
    Ok(())
}
