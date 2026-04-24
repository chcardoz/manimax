//! Slice B step 7 integration test — render a short scene through the full
//! eval → raster → encode pipeline and ffprobe the result.

use std::path::PathBuf;
use std::process::Command;

use manim_rs_runtime::render_to_mp4;

mod common;
use common::short_slice_b_scene;

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

    let probe = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(["-select_streams", "v:0"])
        .args(["-count_frames"])
        .args([
            "-show_entries",
            "stream=width,height,avg_frame_rate,codec_name,nb_read_frames",
        ])
        .args(["-of", "default=noprint_wrappers=1"])
        .arg(&path)
        .output()
        .expect("run ffprobe");
    assert!(probe.status.success(), "ffprobe failed: {probe:?}");
    let out = String::from_utf8_lossy(&probe.stdout);

    assert!(out.contains("width=128"), "{out}");
    assert!(out.contains("height=72"), "{out}");
    assert!(out.contains("codec_name=h264"), "{out}");
    assert!(out.contains("avg_frame_rate=15/1"), "{out}");
    assert!(out.contains("nb_read_frames=6"), "{out}");
}
