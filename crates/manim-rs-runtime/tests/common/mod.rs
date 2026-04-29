//! Shared fixtures for the manim-rs-runtime integration tests.
//!
//! Each integration test file (`end_to_end.rs`, `tex_render.rs`, etc.)
//! compiles `mod common` into its own test binary. Items not used by a
//! given binary trip the dead-code lint there, hence the module-wide
//! allow.
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

use manim_rs_ir::{
    Easing, Object, PositionSegment, Resolution, SCHEMA_VERSION, Scene, SceneMetadata, Stroke,
    TimelineOp, Track,
};

/// Run `ffprobe` on `path` and assert the stream metadata matches the
/// expected width / height / codec / framerate / frame count. Used by the
/// end-to-end render tests across slices to keep the assertion shape one
/// place. `expected_codec` is e.g. `"h264"`; `expected_fps_str` is
/// `"15/1"` (ffprobe's avg_frame_rate format).
pub fn assert_mp4_stream(
    path: &Path,
    expected_width: u32,
    expected_height: u32,
    expected_codec: &str,
    expected_fps_str: &str,
    expected_frames: u32,
) {
    let probe = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(["-select_streams", "v:0"])
        .args(["-count_frames"])
        .args([
            "-show_entries",
            "stream=width,height,avg_frame_rate,codec_name,nb_read_frames",
        ])
        .args(["-of", "default=noprint_wrappers=1"])
        .arg(path)
        .output()
        .expect("run ffprobe");
    assert!(probe.status.success(), "ffprobe failed: {probe:?}");
    let out = String::from_utf8_lossy(&probe.stdout);

    assert!(out.contains(&format!("width={expected_width}")), "{out}");
    assert!(out.contains(&format!("height={expected_height}")), "{out}");
    assert!(
        out.contains(&format!("codec_name={expected_codec}")),
        "{out}"
    );
    assert!(
        out.contains(&format!("avg_frame_rate={expected_fps_str}")),
        "{out}"
    );
    assert!(
        out.contains(&format!("nb_read_frames={expected_frames}")),
        "{out}"
    );
}

/// Unit square translated +1 on x over `duration` seconds at `fps` fps on a
/// 128×72 canvas. At 15 fps × 0.4 s this is the six-frame scene both the
/// end-to-end render test and the cache behaviour tests share.
pub fn short_slice_b_scene(fps: u32, duration: f64) -> Scene {
    Scene {
        metadata: SceneMetadata {
            schema_version: SCHEMA_VERSION,
            fps,
            duration,
            resolution: Resolution {
                width: 128,
                height: 72,
            },
            background: [0.0, 0.0, 0.0, 1.0],
        },
        timeline: vec![TimelineOp::Add {
            t: 0.0,
            id: 1,
            object: Object::Polyline {
                points: vec![
                    [-1.0, -1.0, 0.0],
                    [1.0, -1.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [-1.0, 1.0, 0.0],
                ],
                closed: true,
                stroke: Some(Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.1)),
                fill: None,
            },
        }],
        tracks: vec![Track::Position {
            id: 1,
            segments: vec![PositionSegment {
                t0: 0.0,
                t1: duration,
                from: [0.0, 0.0, 0.0],
                to: [1.0, 0.0, 0.0],
                easing: Easing::Linear {},
            }],
        }],
    }
}
