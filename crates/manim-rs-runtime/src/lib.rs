//! Manimax runtime glue — orchestrates eval → raster → encode with a
//! per-frame blake3-keyed snapshot cache.
//!
//! `render_to_mp4(scene, out)` reads metadata off the IR, stands up a wgpu
//! `Runtime` and an ffmpeg `Encoder`, and for every frame:
//!   1. Evaluates the IR at `t` → `SceneState`.
//!   2. Hashes `(version, metadata, camera, state)` → 32-byte key.
//!   3. Reads cached RGBA for that key if present; otherwise rasterizes and
//!      writes the bytes atomically.
//!   4. Feeds the RGBA into the encoder.
//!
//! Re-renders of a scene that has only changed locally pay only for the
//! frames whose evaluated state actually differs — other frames hash to the
//! same key and skip the rasterizer entirely. See `cache.rs` for the
//! invariants that keep the hash deterministic.

pub mod cache;

use std::path::Path;

use manim_rs_encode::{EncodeError, Encoder};
use manim_rs_eval::Evaluator;
use manim_rs_ir::Scene;
use manim_rs_raster::{Camera, Runtime, RuntimeError as RasterError};

pub use cache::{CACHE_KEY_VERSION, CacheError, CacheStats, FrameCache};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("rasterizer init/render failed: {0}")]
    Raster(#[from] RasterError),
    #[error("encoder failed: {0}")]
    Encode(#[from] EncodeError),
    #[error("cache: {0}")]
    Cache(#[from] CacheError),
}

/// Convenience: render with the default cache location (`$MANIM_RS_CACHE_DIR`
/// or `.manim-rs-cache/`). Discards stats.
pub fn render_to_mp4(scene: Scene, out: &Path) -> Result<(), RuntimeError> {
    let cache = FrameCache::open_default()?;
    render_to_mp4_with_cache(scene, out, &cache).map(|_| ())
}

/// Render a scene to an mp4 at `out`, routing every frame through `cache`.
/// Returns the hit/miss counters so callers (tests, perf logging) can observe
/// how much work the cache actually skipped.
pub fn render_to_mp4_with_cache(
    scene: Scene,
    out: &Path,
    cache: &FrameCache,
) -> Result<CacheStats, RuntimeError> {
    let meta = scene.metadata.clone();
    let total_frames = (f64::from(meta.fps) * meta.duration).round().max(0.0) as u32;

    let runtime = Runtime::new(meta.resolution.width, meta.resolution.height)?;
    let mut encoder = Encoder::start(out, meta.resolution.width, meta.resolution.height, meta.fps)?;
    let evaluator = Evaluator::new(scene);

    let camera = Camera::SLICE_B_DEFAULT;
    let background: [f64; 4] = [
        meta.background[0] as f64,
        meta.background[1] as f64,
        meta.background[2] as f64,
        meta.background[3] as f64,
    ];
    let expected_frame_len =
        (meta.resolution.width as usize) * (meta.resolution.height as usize) * 4;

    let key_prefix = cache::key_hasher(&camera, &meta, CACHE_KEY_VERSION)?;
    let mut stats = CacheStats::default();

    for frame_idx in 0..total_frames {
        let t = f64::from(frame_idx) / f64::from(meta.fps);
        let state = evaluator.eval_at(t);

        let key = cache::frame_key(&key_prefix, &state)?;
        let pixels = match cache.get(&key, expected_frame_len) {
            Some(bytes) => {
                stats.hits += 1;
                bytes
            }
            None => {
                stats.misses += 1;
                let rendered = runtime.render(&state, &camera, background)?;
                // A failed write is non-fatal — the next run just misses again.
                if cache.put(&key, &rendered).is_err() {
                    stats.write_errors += 1;
                }
                rendered
            }
        };
        encoder.push_frame(&pixels)?;
    }

    encoder.finish()?;
    Ok(stats)
}
