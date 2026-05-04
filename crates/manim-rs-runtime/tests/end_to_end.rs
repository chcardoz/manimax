//! Slice B step 7 integration test — render a short scene through the full
//! eval → raster → encode pipeline and ffprobe the result.

use std::path::PathBuf;

use manim_rs_runtime::{
    EncoderOptions, RenderOptions, render_frame_range_to_mp4, render_to_mp4,
    render_to_mp4_with_options,
};

mod common;
use common::{assert_mp4_stream, short_slice_b_scene};

fn scene_path() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push("manim_rs_runtime_e2e.mp4");
    p
}

fn path_with_name(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(name);
    p
}

fn has_command(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("-version")
        .output()
        .is_ok_and(|out| out.status.success())
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

#[test]
fn render_frame_range_to_mp4_writes_exact_range() {
    let path = path_with_name("manim_rs_runtime_range.mp4");
    let _ = std::fs::remove_file(&path);

    let scene = short_slice_b_scene(15, 0.4);
    render_frame_range_to_mp4(scene, &path, 2, 5).expect("render_frame_range_to_mp4");
    assert!(path.exists(), "range mp4 not written");

    assert_mp4_stream(&path, 128, 72, "h264", "15/1", 3);
}

#[test]
fn chunked_render_concats_worker_outputs() {
    if !has_command("ffmpeg") {
        eprintln!("skipping chunked render test: ffmpeg not on PATH");
        return;
    }

    let path = path_with_name("manim_rs_runtime_chunked.mp4");
    let _ = std::fs::remove_file(&path);

    let scene = short_slice_b_scene(15, 0.4);
    let options = RenderOptions {
        encoder: EncoderOptions::default(),
        workers: 2,
    };
    render_to_mp4_with_options(scene, &path, &options, None).expect("chunked render");
    assert!(path.exists(), "chunked mp4 not written");

    assert_mp4_stream(&path, 128, 72, "h264", "15/1", 6);
}
