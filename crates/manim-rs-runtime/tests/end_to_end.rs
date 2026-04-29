//! Slice B step 7 integration test — render a short scene through the full
//! eval → raster → encode pipeline and ffprobe the result.

use std::path::PathBuf;

use manim_rs_runtime::render_to_mp4;

mod common;
use common::{assert_mp4_stream, short_slice_b_scene};

fn scene_path() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push("manim_rs_runtime_e2e.mp4");
    p
}

#[test]
fn render_short_scene_to_mp4() {
    let path = scene_path();
    let _ = std::fs::remove_file(&path);

    // 15 fps × 0.4s = 6 frames. Small enough to run fast in CI.
    let scene = short_slice_b_scene(15, 0.4);
    render_to_mp4(scene, &path).expect("render_to_mp4");
    assert!(path.exists(), "mp4 not written");

    assert_mp4_stream(&path, 128, 72, "h264", "15/1", 6);
}
