//! Slice E Step 4 integration test — render a Tex-only scene end-to-end.
//! The eval-time fan-out (`Object::Tex` → N `Object::BezPath` `ObjectState`s)
//! is exercised implicitly: the rasterizer panics on a surviving `Object::Tex`,
//! so a green render here is itself proof that fan-out happened.

use std::collections::BTreeMap;
use std::path::PathBuf;

use manim_rs_ir::{Object, Resolution, SCHEMA_VERSION, Scene, SceneMetadata, TimelineOp};
use manim_rs_runtime::render_to_mp4;

mod common;
use common::assert_mp4_stream;

fn mp4_path(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("manim_rs_tex_{name}.mp4"));
    p
}

fn one_tex_scene(fps: u32, duration: f64) -> Scene {
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
            object: Object::Tex {
                src: r"x^2 + y^2".to_string(),
                macros: BTreeMap::new(),
                color: [1.0, 1.0, 1.0, 1.0],
                scale: 1.0,
            },
        }],
        tracks: vec![],
    }
}

#[test]
fn tex_scene_renders_to_mp4() {
    let out = mp4_path("render");
    let _ = std::fs::remove_file(&out);

    // 15 fps × 0.4 s = 6 frames.
    let scene = one_tex_scene(15, 0.4);
    render_to_mp4(scene, &out).expect("Tex render");
    assert!(out.exists(), "mp4 not written");

    assert_mp4_stream(&out, 128, 72, "h264", "15/1", 6);
}
