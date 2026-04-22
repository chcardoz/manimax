//! Manimax pyo3 binding — exposes the Rust runtime as `manim_rs._rust`.
//!
//! Slice B keeps the FFI surface intentionally small: we pass the IR across
//! as a JSON string. Slice C swaps to `pythonize` / `FromPyObject` once the
//! schema stabilizes.

use std::path::PathBuf;

use manim_rs_ir::Scene;
use manim_rs_runtime::render_to_mp4 as rust_render_to_mp4;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

/// Minimal build-probe so the module always has something trivially importable.
#[pyfunction]
fn __build_probe() -> &'static str {
    "manim_rs._rust: slice-b step 8"
}

/// Deserialize an IR JSON string into the Rust IR and reserialize it.
/// Used by `tests/python/test_ir_roundtrip.py`.
#[pyfunction]
fn roundtrip_ir(json: &str) -> PyResult<String> {
    let scene: Scene = serde_json::from_str(json)
        .map_err(|e| PyValueError::new_err(format!("IR deserialize failed: {e}")))?;
    serde_json::to_string(&scene)
        .map_err(|e| PyValueError::new_err(format!("IR serialize failed: {e}")))
}

/// Render the scene described by `ir_json` to an mp4 at `out`.
///
/// `fps` overrides `metadata.fps` so the CLI can expose `--fps` without
/// rewriting the Python scene. The GIL is released for the duration of the
/// render so a caller can, e.g., run progress UI on another thread.
#[pyfunction]
#[pyo3(signature = (ir_json, out, fps=None))]
fn render_to_mp4(py: Python<'_>, ir_json: &str, out: &str, fps: Option<u32>) -> PyResult<()> {
    let mut scene: Scene = serde_json::from_str(ir_json)
        .map_err(|e| PyValueError::new_err(format!("IR deserialize failed: {e}")))?;

    if let Some(fps) = fps {
        if fps == 0 {
            return Err(PyValueError::new_err("fps must be positive"));
        }
        scene.metadata.fps = fps;
    }

    let out_path = PathBuf::from(out);
    py.allow_threads(|| rust_render_to_mp4(&scene, &out_path))
        .map_err(|e| PyRuntimeError::new_err(format!("render_to_mp4 failed: {e}")))
}

#[pymodule]
fn _rust(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(__build_probe, m)?)?;
    m.add_function(wrap_pyfunction!(roundtrip_ir, m)?)?;
    m.add_function(wrap_pyfunction!(render_to_mp4, m)?)?;
    Ok(())
}
