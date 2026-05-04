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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use manim_rs_encode::{EncodeError, Encoder};
use manim_rs_eval::Evaluator;
use manim_rs_ir::{Scene, SceneMetadata};
use manim_rs_raster::{Camera, Runtime, RuntimeError as RasterError};

pub use manim_rs_encode::{EncoderBackend, EncoderOptions};

/// Anything that can go wrong while rendering a scene to disk.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("rasterizer init/render failed: {0}")]
    Raster(#[from] RasterError),
    #[error("encoder failed: {0}")]
    Encode(#[from] EncodeError),
    #[error("png write failed: {0}")]
    Png(#[from] png::EncodingError),
    #[error("ffmpeg concat failed: {0}")]
    Concat(String),
    #[error("invalid frame range {start}..{end} for {total} total frames")]
    InvalidFrameRange { start: u32, end: u32, total: u32 },
    #[error("chunk worker panicked")]
    ChunkWorkerPanicked,
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Render config that flows from the CLI/Python boundary into the runtime.
/// Currently just encoder knobs; future fields (camera overrides, MSAA
/// sample count) belong here.
#[derive(Debug, Default, Clone)]
pub struct RenderOptions {
    pub encoder: EncoderOptions,
    /// Number of local frame-range workers. `0` and `1` both mean the
    /// historical single-pass render path.
    pub workers: usize,
}

/// Per-frame progress callback: `(frame_idx, total_frames)`. Called *after*
/// the frame has been pushed to the encoder. `frame_idx` runs `0..total`.
pub type ProgressFn<'a> = &'a mut dyn FnMut(u32, u32);

/// Bundle the eval + raster + view state both render entry points need.
/// Building these from a `Scene` consumes the scene (the evaluator owns it).
struct RenderSetup {
    runtime: Runtime,
    evaluator: Evaluator,
    camera: Camera,
    background: [f64; 4],
    meta: SceneMetadata,
}

impl RenderSetup {
    fn new(scene: Scene) -> Result<Self, RuntimeError> {
        let meta = scene.metadata.clone();
        let runtime = Runtime::new(meta.resolution.width, meta.resolution.height)?;
        let evaluator = Evaluator::new(scene);
        let background = meta.background.map(f64::from);
        Ok(Self {
            runtime,
            evaluator,
            camera: Camera::SLICE_B_DEFAULT,
            background,
            meta,
        })
    }
}

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
    progress: Option<ProgressFn<'_>>,
) -> Result<(), RuntimeError> {
    if options.workers > 1 {
        return render_to_mp4_chunked(scene, out, options, progress);
    }

    let total_frames = total_frames(&scene.metadata);
    render_frame_range_to_mp4_with_options(scene, out, 0, total_frames, &options.encoder, progress)
}

/// Render a half-open frame range `[start_frame, end_frame)` into its own mp4.
///
/// Frame indices are absolute within the scene, so chunk `100..200` evaluates
/// `t = 100 / fps` for its first frame. The resulting mp4 is still a
/// standalone segment whose timestamps start at zero; concat places it in
/// sequence later.
pub fn render_frame_range_to_mp4(
    scene: Scene,
    out: &Path,
    start_frame: u32,
    end_frame: u32,
) -> Result<(), RuntimeError> {
    render_frame_range_to_mp4_with_options(
        scene,
        out,
        start_frame,
        end_frame,
        &EncoderOptions::default(),
        None,
    )
}

pub fn render_frame_range_to_mp4_with_options(
    scene: Scene,
    out: &Path,
    start_frame: u32,
    end_frame: u32,
    encoder_options: &EncoderOptions,
    mut progress: Option<ProgressFn<'_>>,
) -> Result<(), RuntimeError> {
    let setup = RenderSetup::new(scene)?;
    let total_frames = total_frames(&setup.meta);
    if start_frame > end_frame || end_frame > total_frames {
        return Err(RuntimeError::InvalidFrameRange {
            start: start_frame,
            end: end_frame,
            total: total_frames,
        });
    }
    let range_frames = end_frame - start_frame;

    let mut encoder = Encoder::start_with_options(
        out,
        setup.meta.resolution.width,
        setup.meta.resolution.height,
        setup.meta.fps,
        encoder_options,
    )?;

    for frame_idx in start_frame..end_frame {
        let _frame_span = tracing::info_span!("frame", idx = frame_idx).entered();
        let t = f64::from(frame_idx) / f64::from(setup.meta.fps);
        let state = setup.evaluator.eval_at(t);
        let pixels = setup
            .runtime
            .render(&state, &setup.camera, setup.background)?;
        encoder.push_frame(pixels)?;

        if let Some(cb) = progress.as_mut() {
            cb(frame_idx - start_frame, range_frames);
        }
    }

    encoder.finish()?;
    Ok(())
}

fn total_frames(meta: &SceneMetadata) -> u32 {
    (f64::from(meta.fps) * meta.duration).round().max(0.0) as u32
}

fn render_to_mp4_chunked(
    scene: Scene,
    out: &Path,
    options: &RenderOptions,
    mut progress: Option<ProgressFn<'_>>,
) -> Result<(), RuntimeError> {
    let total = total_frames(&scene.metadata);
    if total == 0 {
        return render_frame_range_to_mp4_with_options(
            scene,
            out,
            0,
            0,
            &options.encoder,
            progress,
        );
    }

    let local_cap = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let worker_count = options.workers.min(total as usize).min(local_cap).max(1);
    if worker_count == 1 {
        return render_frame_range_to_mp4_with_options(
            scene,
            out,
            0,
            total,
            &options.encoder,
            progress,
        );
    }

    let tempdir = tempfile::Builder::new()
        .prefix("manimax-chunks-")
        .tempdir()?;
    let ranges = split_ranges(total, worker_count);
    let chunk_paths: Vec<PathBuf> = (0..ranges.len())
        .map(|idx| tempdir.path().join(format!("chunk-{idx:03}.mp4")))
        .collect();

    let progress_channel = progress.is_some().then(mpsc::channel::<()>);

    let render_result = thread::scope(|scope| {
        let mut handles = Vec::with_capacity(ranges.len());
        for (idx, (start, end)) in ranges.iter().copied().enumerate() {
            let scene = scene.clone();
            let path = chunk_paths[idx].clone();
            let encoder = options.encoder.clone();
            let progress_tx = progress_channel.as_ref().map(|(tx, _)| tx.clone());
            handles.push(scope.spawn(move || {
                let mut worker_progress = progress_tx.map(|tx| {
                    move |_idx: u32, _total: u32| {
                        let _ = tx.send(());
                    }
                });
                let progress_ref = worker_progress.as_mut().map(|f| f as ProgressFn<'_>);
                render_frame_range_to_mp4_with_options(
                    scene,
                    &path,
                    start,
                    end,
                    &encoder,
                    progress_ref,
                )
            }));
        }

        if let Some((tx, rx)) = progress_channel {
            drop(tx);
            let mut completed = 0u32;
            for () in rx {
                completed += 1;
                if let Some(cb) = progress.as_mut() {
                    cb(completed - 1, total);
                }
            }
        }

        for handle in handles {
            handle
                .join()
                .map_err(|_| RuntimeError::ChunkWorkerPanicked)??;
        }
        Ok::<(), RuntimeError>(())
    });

    render_result?;

    concat_chunks(&chunk_paths, tempdir.path(), out)?;

    Ok(())
}

fn split_ranges(total_frames: u32, chunks: usize) -> Vec<(u32, u32)> {
    let chunks = chunks.min(total_frames as usize).max(1);
    let base = total_frames / chunks as u32;
    let extra = total_frames % chunks as u32;
    let mut ranges = Vec::with_capacity(chunks);
    let mut start = 0;
    for idx in 0..chunks as u32 {
        let len = base + u32::from(idx < extra);
        let end = start + len;
        ranges.push((start, end));
        start = end;
    }
    ranges
}

fn concat_chunks(chunks: &[PathBuf], tempdir: &Path, out: &Path) -> Result<(), RuntimeError> {
    use std::fmt::Write as _;
    let list_path = tempdir.join("chunks.txt");
    let mut list = String::new();
    for chunk in chunks {
        let name = chunk.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
            RuntimeError::Concat(format!("chunk path has no UTF-8 file name: {chunk:?}"))
        })?;
        // Single-quoted, with internal `'` escaped as `'\''` per ffmpeg concat syntax.
        let escaped = name.replace('\'', "'\\''");
        writeln!(list, "file '{escaped}'").expect("write to String never fails");
    }
    std::fs::write(&list_path, list)?;

    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-y"])
        .args(["-f", "concat", "-safe", "0"])
        .arg("-i")
        .arg(&list_path)
        .args(["-c", "copy"])
        .arg(out)
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(RuntimeError::Concat(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
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
    let setup = RenderSetup::new(scene)?;
    let state = setup.evaluator.eval_at(t);
    let pixels = setup
        .runtime
        .render(&state, &setup.camera, setup.background)?;
    write_rgba_png(
        out,
        setup.meta.resolution.width,
        setup.meta.resolution.height,
        &pixels,
    )
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
