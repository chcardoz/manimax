//! Manimax pyo3 binding — exposes the Rust runtime as `manim_rs._rust`.
//!
//! Slice C swapped the IR wire format from a JSON string to a Python value
//! depythonized via `pythonize`. The Python side calls `msgspec.to_builtins`
//! before handing the IR across the FFI, giving us a plain dict/list/scalar
//! tree that pythonize turns into the serde-typed `Scene` in one hop.
//!
//! `roundtrip_ir` retains its JSON-string signature: it is a schema drift
//! guard, independent of the runtime FFI path.

use std::path::PathBuf;

use manim_rs_eval::{Evaluator, SceneState};
use manim_rs_ir::Scene;
use manim_rs_runtime::{CacheStats, FrameCache, render_frame_to_png, render_to_mp4_with_cache};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pythonize::{depythonize, pythonize};

/// Minimal build-probe so the module always has something trivially importable.
#[pyfunction]
fn __build_probe() -> &'static str {
    "manim_rs._rust: slice-c step 1"
}

/// Deserialize an IR JSON string into the Rust IR and reserialize it.
/// Schema drift guard used by `tests/python/test_ir_roundtrip.py`. Not on the
/// render hot path — that goes through `render_to_mp4` via pythonize.
#[pyfunction]
fn roundtrip_ir(json: &str) -> PyResult<String> {
    let scene: Scene = serde_json::from_str(json)
        .map_err(|e| PyValueError::new_err(format!("IR deserialize failed: {e}")))?;
    serde_json::to_string(&scene)
        .map_err(|e| PyValueError::new_err(format!("IR serialize failed: {e}")))
}

fn depythonize_scene(ir: &Bound<'_, PyAny>) -> PyResult<Scene> {
    depythonize(ir).map_err(|e| PyValueError::new_err(format!("IR depythonize failed: {e}")))
}

/// Render the scene described by `ir` to an mp4 at `out`.
///
/// `ir` is any Python value that pythonize can deserialize into a `Scene` —
/// in practice, the dict produced by `msgspec.to_builtins(scene.ir)` on the
/// Python side. `fps` overrides `metadata.fps` so the CLI can expose `--fps`
/// without rewriting the Python scene. `cache_dir`, when set, points the
/// frame cache at that directory; unset falls back to
/// `FrameCache::open_default` (`$MANIM_RS_CACHE_DIR` or `.manim-rs-cache/`).
/// Returns a dict with `hits`, `misses`, `write_errors` so callers (tests,
/// perf logging) can observe what the cache did. The GIL is released for
/// the duration of the render.
#[pyfunction]
#[pyo3(signature = (ir, out, fps=None, cache_dir=None))]
fn render_to_mp4(
    py: Python<'_>,
    ir: &Bound<'_, PyAny>,
    out: &str,
    fps: Option<u32>,
    cache_dir: Option<&str>,
) -> PyResult<PyObject> {
    let mut scene = depythonize_scene(ir)?;

    if let Some(fps) = fps {
        if fps == 0 {
            return Err(PyValueError::new_err("fps must be positive"));
        }
        scene.metadata.fps = fps;
    }

    let out_path = PathBuf::from(out);
    let cache_dir = cache_dir.map(PathBuf::from);
    let stats: CacheStats = py
        .allow_threads(
            move || -> Result<CacheStats, manim_rs_runtime::RuntimeError> {
                let cache = match cache_dir {
                    Some(dir) => FrameCache::open(dir)?,
                    None => FrameCache::open_default()?,
                };
                render_to_mp4_with_cache(scene, &out_path, &cache)
            },
        )
        .map_err(|e| PyRuntimeError::new_err(format!("render_to_mp4 failed: {e}")))?;

    let d = PyDict::new(py);
    d.set_item("hits", stats.hits)?;
    d.set_item("misses", stats.misses)?;
    d.set_item("write_errors", stats.write_errors)?;
    Ok(d.into())
}

/// Evaluate the scene at time `t` and return a plain Python representation of
/// the resulting `SceneState`. Pure function — no GPU, no `Runtime`.
#[pyfunction]
fn eval_at(py: Python<'_>, ir: &Bound<'_, PyAny>, t: f64) -> PyResult<PyObject> {
    let scene = depythonize_scene(ir)?;
    let state: SceneState = Evaluator::new(scene).eval_at(t);
    let obj = pythonize(py, &state)
        .map_err(|e| PyValueError::new_err(format!("state pythonize failed: {e}")))?;
    Ok(obj.into())
}

/// Render a single frame at time `t` to a PNG at `out`. Skips the ffmpeg
/// encoder — eval + raster + PNG. Useful for snapshot inspection and
/// per-frame baselines (Slice E Step 6).
#[pyfunction]
fn render_frame(py: Python<'_>, ir: &Bound<'_, PyAny>, out: &str, t: f64) -> PyResult<()> {
    let scene = depythonize_scene(ir)?;
    let out_path = PathBuf::from(out);
    py.allow_threads(move || render_frame_to_png(scene, &out_path, t))
        .map_err(|e| PyRuntimeError::new_err(format!("render_frame failed: {e}")))?;
    Ok(())
}

#[pymodule]
fn _rust(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(__build_probe, m)?)?;
    m.add_function(wrap_pyfunction!(roundtrip_ir, m)?)?;
    m.add_function(wrap_pyfunction!(render_to_mp4, m)?)?;
    m.add_function(wrap_pyfunction!(eval_at, m)?)?;
    m.add_function(wrap_pyfunction!(render_frame, m)?)?;
    Ok(())
}
