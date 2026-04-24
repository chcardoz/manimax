//! Edge-case coverage for `Runtime::render` branches that aren't exercised by
//! the canonical scene.
//!
//! - Empty-scene clear path (`begin_and_end_clear_pass`).
//! - Degenerate polyline → empty mesh → `continue` in the per-object loop.
//! - `RuntimeError::GeometryOverflow` when a single polyline's tessellated
//!   vertex buffer exceeds 64 KiB.

use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{Object, Stroke, Vec3};
use manim_rs_raster::{Camera, Runtime, RuntimeError};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 72;

fn small_square(cx: f32, cy: f32, half: f32) -> Object {
    Object::Polyline {
        points: vec![
            [cx - half, cy - half, 0.0],
            [cx + half, cy - half, 0.0],
            [cx + half, cy + half, 0.0],
            [cx - half, cy + half, 0.0],
        ] as Vec<Vec3>,
        closed: true,
        stroke: Some(Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.2)),
        fill: None,
    }
}

#[test]
fn empty_scene_is_flat_background() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    // Slice B stores the clear color as linear floats that wgpu writes through
    // an Rgba8UnormSrgb target — the on-wire pixels are sRGB-encoded. We use
    // mid-grey here because pure black/white would land on gamma curve
    // singularities and make a drift easy to hide.
    let bg = [0.5_f64, 0.5, 0.5, 1.0];

    let pixels = runtime
        .render(
            &SceneState { objects: vec![] },
            &Camera::SLICE_B_DEFAULT,
            bg,
        )
        .expect("render");

    assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);

    // With an all-background frame, every pixel in the buffer should match the
    // first pixel. Any drift (e.g. an uninitialized readback region) shows up
    // immediately.
    let first = &pixels[..4];
    for (i, chunk) in pixels.chunks_exact(4).enumerate() {
        assert_eq!(
            chunk, first,
            "pixel {i} differs from reference — empty-scene clear is not flat"
        );
    }
    // And the pixel must actually be in the background family — not
    // accidentally black because the clear pass didn't run.
    assert!(
        first[0] > 100 && first[1] > 100 && first[2] > 100,
        "background pixel looks black ({first:?}) — clear pass likely skipped",
    );
    assert_eq!(first[3], 255, "alpha should be opaque");
}

#[test]
fn degenerate_polyline_is_skipped_siblings_still_render() {
    // Middle object has two collinear identical points → lyon tessellates to
    // an empty mesh → `continue` in the render loop. The two flanking squares
    // must still appear.
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    // 2-point "polyline" with identical endpoints. lyon produces no geometry
    // for a zero-length stroke path.
    let degenerate = Object::Polyline {
        points: {
            let pts: Vec<Vec3> = vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
            pts
        },
        closed: false,
        stroke: Some(Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.2)),
        fill: None,
    };

    let state = SceneState {
        objects: vec![
            ObjectState::with_defaults(1, small_square(-3.0, 0.0, 0.4), [0.0, 0.0, 0.0]),
            ObjectState::with_defaults(2, degenerate, [0.0, 0.0, 0.0]),
            ObjectState::with_defaults(3, small_square(3.0, 0.0, 0.4), [0.0, 0.0, 0.0]),
        ],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    // Map scene x=-3 / x=3 under SLICE_B_DEFAULT at 128×72:
    // pixel = 64 + (x * 128 / 16) = 64 + 8x → 40 / 88.
    let bright = |x0: u32, x1: u32| -> usize {
        let mut n = 0;
        for y in 20..52 {
            for x in x0..=x1 {
                let i = ((y * WIDTH + x) * 4) as usize;
                if pixels[i] > 40 || pixels[i + 1] > 40 || pixels[i + 2] > 40 {
                    n += 1;
                }
            }
        }
        n
    };
    assert!(bright(30, 50) > 5, "left square missing");
    assert!(bright(78, 98) > 5, "right square missing");
}

#[test]
fn oversized_polyline_returns_geometry_overflow() {
    // Vertex cap is MAX_VERTICES_PER_OBJECT = 4096. lyon emits several
    // vertices per segment; a proper zigzag (non-coincident points lyon
    // can't collapse) reliably overflows the cap above ~1K segments.
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    let n = 3_000usize;
    let mut points: Vec<Vec3> = Vec::with_capacity(n);
    for i in 0..n {
        let x = -5.0 + 10.0 * (i as f32) / (n as f32);
        // Alternating y so each segment is a distinct diagonal — lyon cannot
        // merge these the way it merges sub-epsilon circular arc points.
        let y = if i % 2 == 0 { -1.0 } else { 1.0 };
        points.push([x, y, 0.0]);
    }

    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::Polyline {
                points,
                closed: true,
                stroke: Some(Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.2)),
                fill: None,
            },
            [0.0, 0.0, 0.0],
        )],
    };

    match runtime.render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0]) {
        Err(RuntimeError::GeometryOverflow { kind, needed, cap }) => {
            assert!(
                needed > cap,
                "overflow reported nonsense: needed={needed} cap={cap}",
            );
            assert!(
                kind == "vertex" || kind == "index",
                "unexpected overflow kind: {kind}",
            );
        }
        Ok(_) => panic!("expected GeometryOverflow for {n}-point polyline, but render succeeded",),
        Err(e) => panic!("expected GeometryOverflow, got {e:?}"),
    }
}
