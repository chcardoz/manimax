//! Slice E Step 4 integration test — render a Tex-only scene end-to-end and
//! confirm a warm rerun hits the on-disk snapshot cache for every frame.
//! The eval-time fan-out (`Object::Tex` → N `Object::BezPath` `ObjectState`s)
//! is exercised implicitly: the rasterizer panics on a surviving `Object::Tex`,
//! so a green render here is itself proof that fan-out happened.

use std::collections::BTreeMap;
use std::path::PathBuf;

use manim_rs_ir::{Object, Resolution, SCHEMA_VERSION, Scene, SceneMetadata, TimelineOp};
use manim_rs_runtime::{FrameCache, render_to_mp4_with_cache};

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
    let tmp = tempfile::tempdir().unwrap();
    let cache = FrameCache::open(tmp.path()).unwrap();
    let out = mp4_path("render");
    let _ = std::fs::remove_file(&out);

    // 15 fps × 0.4 s = 6 frames.
    let scene = one_tex_scene(15, 0.4);
    render_to_mp4_with_cache(scene, &out, &cache).expect("Tex render");
    assert!(out.exists(), "mp4 not written");

    assert_mp4_stream(&out, 128, 72, "h264", "15/1", 6);
}

#[test]
fn tex_warm_rerun_hits_frame_cache() {
    // Tex content is time-invariant and there are no animation tracks, so a
    // warm rerun must hit the on-disk cache for every frame. This also
    // indirectly confirms that the per-Evaluator Tex-compile cache produces
    // deterministic geometry: any nondeterminism in the fan-out output
    // would change the SceneState hash and miss the cache.
    let tmp = tempfile::tempdir().unwrap();
    let cache = FrameCache::open(tmp.path()).unwrap();
    let out = mp4_path("cache");
    let _ = std::fs::remove_file(&out);

    render_to_mp4_with_cache(one_tex_scene(15, 0.4), &out, &cache).expect("cold");
    let stats = render_to_mp4_with_cache(one_tex_scene(15, 0.4), &out, &cache).expect("warm");

    assert_eq!(stats.hits, 6, "warm Tex rerun should hit every frame");
    assert_eq!(stats.misses, 0);
}
