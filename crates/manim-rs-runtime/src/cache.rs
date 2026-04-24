//! Per-frame snapshot cache — blake3 over the render-relevant inputs, raw RGBA
//! bytes on disk.
//!
//! The cache sits between eval/raster and the encoder: for each frame we
//! compute a content hash of the inputs that can affect the output pixels
//! (active `SceneState`, camera, resolution, background, and a version tag
//! that lets shader/tessellator changes invalidate every frame at once).
//! A hit skips raster entirely and streams the stored RGBA into the encoder;
//! a miss renders, writes, then encodes.
//!
//! Invariant: `SceneState` serialization must be deterministic. The eval
//! crate produces `active_objects_at` in a fixed order; anything in the
//! hashed input that iterates a non-deterministic container (HashMap,
//! HashSet) would break this guarantee.
//!
//! Not handled here:
//! - Eviction / size caps / LRU — the cache grows unbounded. A separate CLI
//!   tool is the cheapest way to manage this without coupling it to renders.
//! - Cross-machine sharing. `MANIM_RS_CACHE_DIR` can point at a shared path,
//!   but we don't co-ordinate writes beyond atomic rename.
//! - In-memory layer. The OS page cache is already doing this for warm reruns.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use blake3::Hasher;
use manim_rs_eval::SceneState;
use manim_rs_ir::SceneMetadata;
use manim_rs_raster::Camera;
use serde::Serialize;

/// Bumping this number invalidates every cache entry ever written. Bump on:
/// shader edits that change output pixels, tessellator/sampler rewrites,
/// raster-side changes to MSAA / blending / format, anything that alters
/// `(state, camera, metadata) -> pixels` semantics without showing up in
/// the hashed inputs.
pub const CACHE_KEY_VERSION: u32 = 1;

const CACHE_DIR_ENV: &str = "MANIM_RS_CACHE_DIR";
const DEFAULT_CACHE_DIR: &str = ".manim-rs-cache";

/// Errors from cache directory I/O or canonical-JSON encoding of the inputs.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cache I/O: {0}")]
    Io(#[from] io::Error),
    #[error("canonical JSON encode failed: {0}")]
    Encode(#[from] serde_json::Error),
}

/// Per-render constants folded into the hash prefix so `frame_key` only
/// streams the (changing) `SceneState` through a cloned hasher per frame.
/// Field order is the serialization order → also the hash order, so
/// **do not reorder** without bumping `CACHE_KEY_VERSION`.
#[derive(Serialize)]
struct KeyPrefix<'a> {
    version: u32,
    metadata: &'a SceneMetadata,
    camera: CameraHashable,
}

#[derive(Serialize)]
struct CameraHashable {
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
}

impl From<&Camera> for CameraHashable {
    fn from(c: &Camera) -> Self {
        Self {
            left: c.left,
            right: c.right,
            bottom: c.bottom,
            top: c.top,
        }
    }
}

/// Seed a hasher with the per-render constants. Clone the result once per
/// frame and stream the changing `SceneState` into the clone — avoids
/// re-serializing metadata/camera/version every frame.
pub fn key_hasher(
    camera: &Camera,
    metadata: &SceneMetadata,
    version: u32,
) -> Result<Hasher, CacheError> {
    let prefix = KeyPrefix {
        version,
        metadata,
        camera: CameraHashable::from(camera),
    };
    let json = serde_json::to_vec(&prefix)?;
    let mut h = Hasher::new();
    h.update(&json);
    Ok(h)
}

/// Finalize a per-frame hash by streaming `state` into a clone of the
/// prefix-seeded hasher.
pub fn frame_key(prefix: &Hasher, state: &SceneState) -> Result<blake3::Hash, CacheError> {
    let json = serde_json::to_vec(state)?;
    let mut h = prefix.clone();
    h.update(&json);
    Ok(h.finalize())
}

/// On-disk frame cache. Owns a directory; each entry is one file named
/// `<hex>.rgba` containing raw RGBA bytes.
#[derive(Debug, Clone)]
pub struct FrameCache {
    dir: PathBuf,
}

impl FrameCache {
    /// Open (or create) a cache at `dir`. Idempotent.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, CacheError> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// Open the cache at `$MANIM_RS_CACHE_DIR` if set, else at the default
    /// `.manim-rs-cache/` in CWD.
    pub fn open_default() -> Result<Self, CacheError> {
        let dir = std::env::var_os(CACHE_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CACHE_DIR));
        Self::open(dir)
    }

    fn path_for(&self, key: &blake3::Hash) -> PathBuf {
        self.dir.join(format!("{}.rgba", key.to_hex()))
    }

    /// Read cached bytes for `key`. Returns `None` on miss, missing file,
    /// or a size mismatch (truncated / corrupted entry treated as a miss).
    pub fn get(&self, key: &blake3::Hash, expected_len: usize) -> Option<Vec<u8>> {
        let path = self.path_for(key);
        let bytes = fs::read(&path).ok()?;
        if bytes.len() != expected_len {
            return None;
        }
        Some(bytes)
    }

    /// Write `bytes` for `key` atomically (tmp file + rename). Errors are
    /// surfaced but non-fatal at call sites — a failed write just means the
    /// next run will miss again.
    pub fn put(&self, key: &blake3::Hash, bytes: &[u8]) -> Result<(), CacheError> {
        let final_path = self.path_for(key);
        // NamedTempFile::new_in keeps the temp file on the same filesystem
        // as the target so `persist`'s rename is atomic.
        let mut tmp = tempfile::NamedTempFile::new_in(&self.dir)?;
        tmp.as_file_mut().write_all(bytes)?;
        tmp.as_file_mut().sync_all()?;
        tmp.persist(&final_path)
            .map_err(|e| CacheError::Io(e.error))?;
        Ok(())
    }
}

/// Running counters updated by `render_to_mp4`. Exposed for tests so they
/// can assert hit-rate behaviour end-to-end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub write_errors: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_meta() -> SceneMetadata {
        SceneMetadata {
            schema_version: 1,
            fps: 30,
            duration: 1.0,
            resolution: manim_rs_ir::Resolution {
                width: 16,
                height: 16,
            },
            background: [0.0, 0.0, 0.0, 1.0],
        }
    }

    fn compute(state: &SceneState, meta: &SceneMetadata, version: u32) -> blake3::Hash {
        let prefix = key_hasher(&Camera::SLICE_B_DEFAULT, meta, version).unwrap();
        frame_key(&prefix, state).unwrap()
    }

    #[test]
    fn key_stable_across_calls_with_identical_inputs() {
        let meta = sample_meta();
        let state = SceneState::default();
        assert_eq!(
            compute(&state, &meta, CACHE_KEY_VERSION),
            compute(&state, &meta, CACHE_KEY_VERSION)
        );
    }

    #[test]
    fn version_bump_changes_the_key() {
        let meta = sample_meta();
        let state = SceneState::default();
        assert_ne!(compute(&state, &meta, 1), compute(&state, &meta, 2));
    }

    #[test]
    fn get_rejects_size_mismatched_files() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = FrameCache::open(tmp.path()).unwrap();
        let key = blake3::hash(b"test key");
        cache.put(&key, &[0u8; 16]).unwrap();
        assert!(cache.get(&key, 16).is_some());
        assert!(cache.get(&key, 32).is_none());
    }

    #[test]
    fn put_is_atomic_even_when_overwriting() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = FrameCache::open(tmp.path()).unwrap();
        let key = blake3::hash(b"another");
        cache.put(&key, &[1u8; 8]).unwrap();
        cache.put(&key, &[2u8; 8]).unwrap();
        assert_eq!(cache.get(&key, 8).unwrap(), vec![2u8; 8]);
    }
}
