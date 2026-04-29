//! Manimax runtime glue — orchestrates eval → raster → encode.
//!
//! `render_to_mp4(scene, out)` reads metadata off the IR, stands up a wgpu
//! `Runtime` and an ffmpeg `Encoder`, and for every frame:
//!   1. Evaluates the IR at `t` → `SceneState`.
//!   2. Rasterizes the state to RGBA.
//!   3. Feeds the RGBA into the encoder.
//!
//! No on-disk pixel cache: the cold-render cost was dominated by
//! disk-write of raw RGBA frames (`docs/performance.md` N16 / ADR 0009),
//! and warm reruns were bottlenecked by the same pipe + readback work
//! that a cold render does anyway. Cheaper artifact caches (Tex compile,
//! glyph outlines) live inside their respective crates, keyed on the
//! source they derive from rather than per-frame `SceneState`.

use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use manim_rs_encode::{EncodeError, Encoder};
use manim_rs_eval::Evaluator;
use manim_rs_ir::Scene;
use manim_rs_raster::{Camera, Runtime, RuntimeError as RasterError};

pub use manim_rs_encode::EncoderOptions;

/// Anything that can go wrong while rendering a scene to disk.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("rasterizer init/render failed: {0}")]
    Raster(#[from] RasterError),
    #[error("encoder failed: {0}")]
    Encode(#[from] EncodeError),
    #[error("png write failed: {0}")]
    Png(String),
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

impl From<png::EncodingError> for RuntimeError {
    fn from(e: png::EncodingError) -> Self {
        RuntimeError::Png(e.to_string())
    }
}

/// Render config that flows from the CLI/Python boundary into the runtime.
/// Currently just encoder knobs; future fields (camera overrides, MSAA
/// sample count) belong here.
#[derive(Debug, Default, Clone)]
pub struct RenderOptions {
    pub encoder: EncoderOptions,
}

/// Per-frame progress callback: `(frame_idx, total_frames)`. Called *after*
/// the frame has been pushed to the encoder. `frame_idx` runs `0..total`.
pub type ProgressFn<'a> = &'a mut dyn FnMut(u32, u32);

/// Convenience: render with default options and no progress callback.
pub fn render_to_mp4(scene: Scene, out: &Path) -> Result<(), RuntimeError> {
    render_to_mp4_with_options(scene, out, &RenderOptions::default(), None)
}

/// Render a scene to an mp4 at `out`.
///
/// `progress`, when set, is invoked once per frame after the encoder push so
/// callers can render a progress bar (`docs/performance.md` O11). Errors
/// inside the callback do not propagate — the callback is fire-and-forget UI.
#[tracing::instrument(
    name = "render_to_mp4",
    skip_all,
    fields(
        width = scene.metadata.resolution.width,
        height = scene.metadata.resolution.height,
        fps = scene.metadata.fps,
        duration = scene.metadata.duration,
    ),
)]
pub fn render_to_mp4_with_options(
    scene: Scene,
    out: &Path,
    options: &RenderOptions,
    mut progress: Option<ProgressFn<'_>>,
) -> Result<(), RuntimeError> {
    let meta = scene.metadata.clone();
    let total_frames = (f64::from(meta.fps) * meta.duration).round().max(0.0) as u32;

    let runtime = Runtime::new(meta.resolution.width, meta.resolution.height)?;
    let mut encoder = Encoder::start_with_options(
        out,
        meta.resolution.width,
        meta.resolution.height,
        meta.fps,
        &options.encoder,
    )?;
    let evaluator = Evaluator::new(scene);

    let camera = Camera::SLICE_B_DEFAULT;
    let background: [f64; 4] = [
        meta.background[0] as f64,
        meta.background[1] as f64,
        meta.background[2] as f64,
        meta.background[3] as f64,
    ];

    for frame_idx in 0..total_frames {
        let _frame_span = tracing::info_span!("frame", idx = frame_idx).entered();
        let t = f64::from(frame_idx) / f64::from(meta.fps);
        let state = evaluator.eval_at(t);
        let pixels = runtime.render(&state, &camera, background)?;
        encoder.push_frame(pixels)?;

        if let Some(cb) = progress.as_mut() {
            cb(frame_idx, total_frames);
        }
    }

    encoder.finish()?;
    Ok(())
}

/// Render a single frame at time `t` and write it as a PNG at `out`.
///
/// Bypasses the ffmpeg encoder entirely — eval + raster only. Useful for
/// inspection (`python -m manim_rs frame ...`), snapshot tests (Slice E
/// Step 6 corpus baselines), and quick iteration where mp4 round-tripping
/// would dominate latency.
///
/// `t` is clamped silently to `[0, duration]` semantics by the evaluator —
/// `eval_at(t)` past the timeline holds the last reached state, so a
/// caller can ask for `t = +inf` to get the final frame without
/// special-casing the duration.
pub fn render_frame_to_png(scene: Scene, out: &Path, t: f64) -> Result<(), RuntimeError> {
    let meta = scene.metadata.clone();
    let runtime = Runtime::new(meta.resolution.width, meta.resolution.height)?;
    let evaluator = Evaluator::new(scene);
    let camera = Camera::SLICE_B_DEFAULT;
    let background: [f64; 4] = [
        meta.background[0] as f64,
        meta.background[1] as f64,
        meta.background[2] as f64,
        meta.background[3] as f64,
    ];

    let state = evaluator.eval_at(t);
    let pixels = runtime.render(&state, &camera, background)?;

    write_rgba_png(out, meta.resolution.width, meta.resolution.height, &pixels)
}

fn write_rgba_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), RuntimeError> {
    let _span = tracing::info_span!("png::write_rgba", width, height, bytes = rgba.len()).entered();
    let file = File::create(path)?;
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    Ok(())
}
