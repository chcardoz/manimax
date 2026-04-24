//! Render-side snapshot cache — hit/miss accounting and locality of
//! invalidation. Tests live outside the crate to exercise the public
//! surface (`render_to_mp4_with_cache` + `CacheStats`) exactly the way a
//! caller would. Each test uses a fresh `TempDir` as the cache so they
//! never see state from one another.

use std::path::PathBuf;

use manim_rs_ir::{Easing, PositionSegment, Track};
use manim_rs_runtime::{FrameCache, render_to_mp4_with_cache};

mod common;
use common::short_slice_b_scene;

fn mp4_path(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("manim_rs_cache_{name}.mp4"));
    p
}

/// A 15 fps × 0.4 s scene = 6 frames. The position track runs for the full
/// duration so every frame's evaluated state differs from every other —
/// useful for proving locality in `test_local_invalidation`.
fn six_frame_scene() -> manim_rs_ir::Scene {
    short_slice_b_scene(15, 0.4)
}

#[test]
fn cold_run_populates_cache_every_frame_misses() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = FrameCache::open(tmp.path()).unwrap();
    let out = mp4_path("cold");
    let _ = std::fs::remove_file(&out);

    let stats = render_to_mp4_with_cache(six_frame_scene(), &out, &cache).expect("cold render");

    assert_eq!(stats.misses, 6);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.write_errors, 0);

    let entries = std::fs::read_dir(tmp.path()).unwrap().count();
    assert_eq!(entries, 6, "one cache entry per frame");
}

#[test]
fn warm_rerun_hits_every_frame() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = FrameCache::open(tmp.path()).unwrap();
    let out = mp4_path("warm");
    let _ = std::fs::remove_file(&out);

    render_to_mp4_with_cache(six_frame_scene(), &out, &cache).expect("cold");
    let stats = render_to_mp4_with_cache(six_frame_scene(), &out, &cache).expect("warm");

    assert_eq!(stats.hits, 6, "warm rerun should hit every frame");
    assert_eq!(stats.misses, 0);
}

#[test]
fn local_track_edit_invalidates_only_affected_frames() {
    // Edit strategy: replace the Position track's segment so the object ends
    // at a different x. Every frame is inside `[t0, t1]` of the position
    // track and each frame's evaluated position differs, so this should
    // invalidate every frame. We then separately prove locality by narrowing
    // the edited segment to cover only part of the timeline.
    let tmp = tempfile::tempdir().unwrap();
    let cache = FrameCache::open(tmp.path()).unwrap();
    let out = mp4_path("local");
    let _ = std::fs::remove_file(&out);

    // Use a scene whose position track only animates during t ∈ [0.2, 0.4].
    // Frames 0/1/2/3 at t=0.0, 0.066, 0.133, 0.2 all evaluate to position
    // [0,0,0] (no active segment, or alpha=0 at t0 with from=[0,0,0]).
    // Frames 4/5 at t=0.266, 0.333 fall inside the segment with alpha > 0.
    //
    // We pick `edited.to` so that no edited frame's evaluated position
    // coincidentally collides with any base frame's — the cache is
    // content-addressed, so a collision would count as a hit regardless of
    // which frame produced it.
    let mut base = six_frame_scene();
    base.tracks = vec![Track::Position {
        id: 1,
        segments: vec![PositionSegment {
            t0: 0.2,
            t1: 0.4,
            from: [0.0, 0.0, 0.0],
            to: [1.0, 0.0, 0.0],
            easing: Easing::Linear {},
        }],
    }];

    render_to_mp4_with_cache(base.clone(), &out, &cache).expect("cold");

    let mut edited = base.clone();
    edited.tracks = vec![Track::Position {
        id: 1,
        segments: vec![PositionSegment {
            t0: 0.2,
            t1: 0.4,
            from: [0.0, 0.0, 0.0],
            to: [3.0, 0.0, 0.0], // chosen to avoid coincidental collisions
            easing: Easing::Linear {},
        }],
    }];

    let stats = render_to_mp4_with_cache(edited, &out, &cache).expect("warm edit");

    // Frames 0-3 evaluate to [0,0,0] in both runs → hit. Frames 4, 5
    // evaluate to [0.333,0,0] / [0.666,0,0] in base and [1.0,0,0] /
    // [2.0,0,0] in edited → distinct states, miss.
    assert_eq!(
        stats.hits, 4,
        "pre-segment frames + alpha-zero frame should hit"
    );
    assert_eq!(
        stats.misses, 2,
        "inside-segment frames with alpha > 0 should miss"
    );
}

#[test]
fn corrupted_cache_entry_is_re_rendered() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = FrameCache::open(tmp.path()).unwrap();
    let out = mp4_path("corrupt");
    let _ = std::fs::remove_file(&out);

    render_to_mp4_with_cache(six_frame_scene(), &out, &cache).expect("cold");

    // Truncate one cache file to half its size.
    let target = std::fs::read_dir(tmp.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = std::fs::read(&target).unwrap();
    std::fs::write(&target, &bytes[..bytes.len() / 2]).unwrap();

    let stats = render_to_mp4_with_cache(six_frame_scene(), &out, &cache).expect("retry");
    assert_eq!(stats.misses, 1, "the corrupted entry should re-render");
    assert_eq!(stats.hits, 5, "other five should still hit");
}
