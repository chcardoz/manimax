//! Shared fixtures for the manim-rs-runtime integration tests.

use manim_rs_ir::{
    Easing, Object, PositionSegment, Resolution, SCHEMA_VERSION, Scene, SceneMetadata, Stroke,
    TimelineOp, Track,
};

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
