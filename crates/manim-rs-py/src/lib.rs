//! Manimax pyo3 binding — exposes the Rust runtime as `manim_rs._rust`.
//!
//! Slice C swapped the IR wire format from a JSON string to a Python value
//! depythonized via `pythonize`. The Python side calls `msgspec.to_builtins`
//! before handing the IR across the FFI, giving us a plain dict/list/scalar
//! tree that pythonize turns into the serde-typed `Scene` in one hop.
//!
//! `roundtrip_ir` retains its JSON-string signature: it is a schema drift
//! guard, independent of the runtime FFI path.

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::OnceLock;

use manim_rs_eval::{Evaluator, SceneState};
use manim_rs_ir::Scene;
use manim_rs_runtime::{
    EncoderBackend, EncoderOptions, RenderOptions, RuntimeError, render_frame_to_png,
    render_to_mp4_with_options,
};
use manim_rs_tex::tex_to_display_list;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pythonize::{depythonize, pythonize};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::writer::BoxMakeWriter;

/// Minimal build-probe so the module always has something trivially importable.
#[pyfunction]
fn __build_probe() -> &'static str {
    "manim_rs._rust: slice-c step 1"
}

/// Tracking flag for `install_trace_json`. The global default subscriber can
/// only be set once per process; subsequent calls are silent no-ops.
static TRACE_INSTALLED: OnceLock<()> = OnceLock::new();

/// Convert a `RuntimeError` into a `PyErr` that preserves the full
/// `std::error::Error::source` chain via Python's `__cause__`. Replaces
/// the previous `format!("...: {e}")` flattening so users see e.g.
/// `RuntimeError: encoder failed: ... → FileNotFoundError: ffmpeg` instead
/// of a single string with the inner io::Error swallowed.
///
/// Orphan rules forbid `impl From<RuntimeError> for PyErr` directly (both
/// types live outside this crate), so this is the explicit-call form.
fn runtime_err_to_pyerr(err: RuntimeError) -> PyErr {
    // Collect the chain top-down: outermost message first, root cause last.
    let mut messages: Vec<String> = vec![err.to_string()];
    let mut src: Option<&dyn std::error::Error> = std::error::Error::source(&err);
    while let Some(e) = src {
        messages.push(e.to_string());
        src = e.source();
    }

    Python::with_gil(|py| {
        // Build inside-out so `__cause__` points from outer → inner.
        let mut iter = messages.into_iter().rev();
        let root_msg = iter
            .next()
            .expect("messages is non-empty: pushed err first");
        let mut current = PyRuntimeError::new_err(root_msg);
        for msg in iter {
            let parent = PyRuntimeError::new_err(msg);
            parent.set_cause(py, Some(current));
            current = parent;
        }
        current
    })
}

/// Install a JSON `tracing` subscriber that writes per-stage spans (eval_at,
/// raster::render, readback, encoder::push_frame, frame, render_to_mp4) to
/// `path`, one JSON object per event.
///
/// Idempotent at the process level — only the first call wins. Filter level
/// honors the `RUST_LOG` env var if set, otherwise defaults to `info`.
///
/// The output is the standard `tracing-subscriber` JSON format. Ingest with
/// `jq`, or convert to chrome://tracing format with a small post-processor.
#[pyfunction]
fn install_trace_json(path: &str) -> PyResult<bool> {
    if TRACE_INSTALLED.get().is_some() {
        return Ok(false);
    }

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| PyRuntimeError::new_err(format!("trace file open failed: {e}")))?;

    let writer = BoxMakeWriter::new(std::sync::Mutex::new(file));
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let installed = tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .try_init()
        .is_ok();

    if installed {
        TRACE_INSTALLED.set(()).ok();
    }
    Ok(installed)
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
/// without rewriting the Python scene. The GIL is released for the duration
/// of the render.
#[pyfunction]
#[pyo3(signature = (ir, out, fps=None, crf=None, encoder_backend=None, progress=None))]
fn render_to_mp4(
    py: Python<'_>,
    ir: &Bound<'_, PyAny>,
    out: &str,
    fps: Option<u32>,
    crf: Option<u8>,
    encoder_backend: Option<&str>,
    progress: Option<Py<PyAny>>,
) -> PyResult<()> {
    let mut scene = depythonize_scene(ir)?;

    if let Some(fps) = fps {
        if fps == 0 {
            return Err(PyValueError::new_err("fps must be positive"));
        }
        scene.metadata.fps = fps;
    }

    let backend = match encoder_backend {
        None | Some("software") => EncoderBackend::Software,
        Some("hardware") => EncoderBackend::Hardware,
        Some(other) => {
            return Err(PyValueError::new_err(format!(
                "encoder_backend must be 'software' or 'hardware', got {other:?}",
            )));
        }
    };

    let out_path = PathBuf::from(out);
    let render_options = RenderOptions {
        encoder: EncoderOptions { crf, backend },
    };
    py.allow_threads(move || -> Result<(), manim_rs_runtime::RuntimeError> {
        // Wrap the optional Python callable into a closure the Rust runtime
        // can drive. Each invocation re-acquires the GIL — cheap relative to
        // per-frame render cost (~10–30 ms vs. microseconds), and only
        // happens once per frame.
        let mut cb = progress.map(|callable| {
            move |idx: u32, total: u32| {
                Python::with_gil(|py| {
                    let _ = callable.call1(py, (idx, total));
                });
            }
        });
        let progress_ref: Option<manim_rs_runtime::ProgressFn<'_>> =
            cb.as_mut().map(|f| f as manim_rs_runtime::ProgressFn<'_>);
        render_to_mp4_with_options(scene, &out_path, &render_options, progress_ref)
    })
    .map_err(runtime_err_to_pyerr)?;

    Ok(())
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
        .map_err(runtime_err_to_pyerr)?;
    Ok(())
}

/// Parse `src` as Tex source and return on success, raising `ValueError`
/// with the parse error message on failure. Slice E Step 5 entry point —
/// lets `Tex(...)` constructors fail loudly at author time rather than
/// silently rendering a blank frame at render time. Cheaper than a full
/// `compile_tex` round trip: stops at `tex_to_display_list`, which runs
/// parse + layout but not the kurbo BezPath conversion.
#[pyfunction]
fn tex_validate(py: Python<'_>, src: &str) -> PyResult<()> {
    // Copy out of the &str (which borrows from a Python string and so
    // requires the GIL) before releasing it. Parse+layout itself touches
    // no Python state.
    let owned = src.to_owned();
    py.allow_threads(|| tex_to_display_list(&owned).map(|_| ()))
        .map_err(|e| PyValueError::new_err(format!("invalid Tex source: {e}")))
}

#[pymodule]
fn _rust(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(__build_probe, m)?)?;
    m.add_function(wrap_pyfunction!(roundtrip_ir, m)?)?;
    m.add_function(wrap_pyfunction!(render_to_mp4, m)?)?;
    m.add_function(wrap_pyfunction!(eval_at, m)?)?;
    m.add_function(wrap_pyfunction!(tex_validate, m)?)?;
    m.add_function(wrap_pyfunction!(render_frame, m)?)?;
    m.add_function(wrap_pyfunction!(install_trace_json, m)?)?;
    Ok(())
}
