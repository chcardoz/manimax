//! Regression test for the Slice B multi-object render bug.
//!
//! Bug: `Runtime::render` writes every object's vertices/indices/uniforms to
//! the *same* reusable buffers and records N render passes into one encoder
//! that is submitted once at the end. wgpu schedules all `queue.write_buffer`
//! calls to happen before any command buffer executes, so every pass sees
//! only the **last** object's data. Result: only the last-added object is
//! ever visible.
//!
//! This test proves that by rendering two polylines in well-separated regions
//! of the frame and asserting that both regions contain non-background pixels.

use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{Object, Vec3};
use manim_rs_raster::{Camera, Runtime};

const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;

fn square(cx: f32, cy: f32, half: f32) -> Object {
    Object::Polyline {
        points: vec![
            [cx - half, cy - half, 0.0],
            [cx + half, cy - half, 0.0],
            [cx + half, cy + half, 0.0],
            [cx - half, cy + half, 0.0],
        ] as Vec<Vec3>,
        stroke_color: [1.0, 1.0, 1.0, 1.0],
        stroke_width: 0.15,
        closed: true,
    }
}

/// Count non-background pixels in an inclusive pixel rectangle.
fn bright_pixels_in_box(
    rgba: &[u8],
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    threshold: u8,
) -> usize {
    let mut n = 0;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let i = ((y * WIDTH + x) * 4) as usize;
            if rgba[i] > threshold || rgba[i + 1] > threshold || rgba[i + 2] > threshold {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn both_objects_in_multi_object_scene_are_visible() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    // Object A: small square on the LEFT (scene x ≈ -4). At the default Slice B
    // camera ([-8,8] × [-4.5,4.5]) and 480×270, scene (-4, 0, 0) maps to pixel
    // x ≈ 120, centered vertically.
    //
    // Object B: small square on the RIGHT (scene x ≈ +4) → pixel x ≈ 360.
    //
    // The bug drops object A; it would render object B only. An all-background
    // region where A should be is the failure signal.
    let state = SceneState {
        objects: vec![
            ObjectState {
                id: 1,
                object: square(-4.0, 0.0, 0.5),
                position: [0.0, 0.0, 0.0],
            },
            ObjectState {
                id: 2,
                object: square(4.0, 0.0, 0.5),
                position: [0.0, 0.0, 0.0],
            },
        ],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    // Region where object A (left square, pixel x ≈ 120, y ≈ 135) should be.
    let left = bright_pixels_in_box(&pixels, 90, 105, 150, 165, 40);
    // Region where object B (right square, pixel x ≈ 360, y ≈ 135) should be.
    let right = bright_pixels_in_box(&pixels, 330, 105, 390, 165, 40);

    assert!(right > 50, "right object missing: only {right} bright px");
    // This is the assertion the bug makes fail:
    assert!(left > 50, "left object missing: only {left} bright px (multi-object render bug)");
}

/// Strengthened version: three objects. Two-object coverage proved the
/// `queue.write_buffer` bug was fixed; three-object proves there's no
/// off-by-one in the per-object submit loop (e.g. a `first` flag that
/// mis-fires on iteration 2) and pins centroid placement so "both render but
/// at the wrong positions" also fails.
#[test]
fn three_objects_all_visible_at_expected_centroids() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    // Pixel mapping at SLICE_B_DEFAULT + 480×270: scene (x,y) → pixel
    // (240 + 30x, 135 − 30y).
    //   x = -4 → pixel 120
    //   x =  0 → pixel 240
    //   x =  4 → pixel 360
    let state = SceneState {
        objects: vec![
            ObjectState {
                id: 1,
                object: square(-4.0, 0.0, 0.5),
                position: [0.0, 0.0, 0.0],
            },
            ObjectState {
                id: 2,
                object: square(0.0, 0.0, 0.5),
                position: [0.0, 0.0, 0.0],
            },
            ObjectState {
                id: 3,
                object: square(4.0, 0.0, 0.5),
                position: [0.0, 0.0, 0.0],
            },
        ],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    for (expected_px, label) in [(120u32, "left"), (240u32, "center"), (360u32, "right")] {
        let x0 = expected_px.saturating_sub(30);
        let x1 = (expected_px + 30).min(WIDTH - 1);
        let y0 = 105;
        let y1 = 165;
        let (n, cx, cy) = bright_centroid_in_box(&pixels, x0, y0, x1, y1, 40);
        assert!(n > 50, "{label} object missing: only {n} bright px");
        let dx = (cx as i64 - expected_px as i64).abs();
        let dy = (cy as i64 - 135).abs();
        assert!(dx <= 3, "{label} centroid drift: x={cx} expected ≈{expected_px} (dx={dx})");
        assert!(dy <= 3, "{label} centroid drift: y={cy} expected ≈135 (dy={dy})");
    }
}

fn bright_centroid_in_box(
    rgba: &[u8],
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    threshold: u8,
) -> (usize, u32, u32) {
    let mut n = 0usize;
    let mut sx = 0u64;
    let mut sy = 0u64;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let i = ((y * WIDTH + x) * 4) as usize;
            if rgba[i] > threshold || rgba[i + 1] > threshold || rgba[i + 2] > threshold {
                n += 1;
                sx += x as u64;
                sy += y as u64;
            }
        }
    }
    if n == 0 {
        (0, 0, 0)
    } else {
        (n, (sx / n as u64) as u32, (sy / n as u64) as u32)
    }
}
