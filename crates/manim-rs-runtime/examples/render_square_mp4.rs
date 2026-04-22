//! Slice B step 7 eyeball — render a translating-square scene to mp4.
//!
//! Produces a 2-second 480×270 mp4 at `/tmp/slice_b_square.mp4`. Square outline
//! translates from centered to 2 scene units right of center over 2 seconds.
//!
//! ```sh
//! cargo run -p manim-rs-runtime --example render_square_mp4
//! open /tmp/slice_b_square.mp4
//! ```

use std::path::Path;

use manim_rs_ir::{
    Easing, Object, PositionSegment, Resolution, SCHEMA_VERSION, Scene, SceneMetadata, TimelineOp,
    Track,
};
use manim_rs_runtime::render_to_mp4;

fn main() {
    let scene = Scene {
        metadata: SceneMetadata {
            schema_version: SCHEMA_VERSION,
            fps: 30,
            duration: 2.0,
            resolution: Resolution {
                width: 480,
                height: 270,
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
                stroke_color: [1.0, 1.0, 1.0, 1.0],
                stroke_width: 0.08,
                closed: true,
            },
        }],
        tracks: vec![Track::Position {
            id: 1,
            segments: vec![PositionSegment {
                t0: 0.0,
                t1: 2.0,
                from: [0.0, 0.0, 0.0],
                to: [2.0, 0.0, 0.0],
                easing: Easing::Linear {},
            }],
        }],
    };

    let out = Path::new("/tmp/slice_b_square.mp4");
    render_to_mp4(&scene, out).expect("render_to_mp4");
    println!("wrote {}", out.display());
}
