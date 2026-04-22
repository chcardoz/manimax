//! Slice C Step 4 — rasterizer honors `ObjectState.{opacity, rotation,
//! scale, color_override}`.
//!
//! Each test renders a small fixture and inspects the framebuffer. Together
//! they prove the per-object MVP composition (T·R·S) and the color/alpha
//! pipeline are correctly wired between `eval_at` output and the GPU.

use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{Object, Stroke, Vec3};
use manim_rs_raster::{Camera, Runtime};

const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;

fn unit_square() -> Object {
    let pts: Vec<Vec3> = vec![
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ];
    Object::Polyline {
        points: pts,
        closed: true,
        stroke: Some(Stroke {
            color: [1.0, 1.0, 1.0, 1.0],
            width: 0.15,
        }),
        fill: None,
    }
}

fn bright_centroid_and_count(rgba: &[u8], threshold: u8) -> (usize, f64, f64) {
    let mut n = 0usize;
    let mut sx = 0u64;
    let mut sy = 0u64;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let i = ((y * WIDTH + x) * 4) as usize;
            if rgba[i] > threshold || rgba[i + 1] > threshold || rgba[i + 2] > threshold {
                n += 1;
                sx += x as u64;
                sy += y as u64;
            }
        }
    }
    if n == 0 {
        (0, 0.0, 0.0)
    } else {
        (n, sx as f64 / n as f64, sy as f64 / n as f64)
    }
}

fn render_one(state: ObjectState) -> Vec<u8> {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");
    runtime
        .render(
            &SceneState {
                objects: vec![state],
            },
            &Camera::SLICE_B_DEFAULT,
            [0.0, 0.0, 0.0, 1.0],
        )
        .expect("render")
}

#[test]
fn opacity_dims_stroke_color() {
    let opaque = render_one(ObjectState {
        id: 1,
        object: unit_square(),
        position: [0.0, 0.0, 0.0],
        opacity: 1.0,
        rotation: 0.0,
        scale: 1.0,
        color_override: None,
    });
    let dim = render_one(ObjectState {
        id: 1,
        object: unit_square(),
        position: [0.0, 0.0, 0.0],
        opacity: 0.5,
        rotation: 0.0,
        scale: 1.0,
        color_override: None,
    });

    // Same geometry → same number of touched pixels (above a low threshold).
    let (n_opaque, _, _) = bright_centroid_and_count(&opaque, 20);
    let (n_dim, _, _) = bright_centroid_and_count(&dim, 20);
    assert!(n_opaque > 100 && n_dim > 100, "no stroke drawn");
    assert!(
        (n_opaque as i64 - n_dim as i64).abs() < (n_opaque as i64) / 4,
        "opacity changed pixel count drastically: {n_opaque} vs {n_dim}",
    );

    // Mean R channel inside touched pixels should drop with opacity. Use a
    // strict white-ish threshold for the opaque image; the dim version must
    // fail to clear the same threshold for most of those pixels.
    let bright_rgb_count = |rgba: &[u8], min: u8| -> usize {
        rgba.chunks_exact(4)
            .filter(|p| p[0] >= min && p[1] >= min && p[2] >= min)
            .count()
    };
    let opaque_white = bright_rgb_count(&opaque, 200);
    let dim_white = bright_rgb_count(&dim, 200);
    assert!(opaque_white > 100, "opaque didn't render bright white");
    assert!(
        dim_white * 4 < opaque_white,
        "opacity=0.5 didn't dim: {dim_white} vs {opaque_white}",
    );
}

#[test]
fn color_override_replaces_authored_color() {
    let pixels = render_one(ObjectState {
        id: 1,
        object: unit_square(),
        position: [0.0, 0.0, 0.0],
        opacity: 1.0,
        rotation: 0.0,
        scale: 1.0,
        color_override: Some([1.0, 0.0, 0.0, 1.0]),
    });

    // The authored color is white; an override to red must produce pixels
    // dominated by the R channel.
    let mut n = 0usize;
    let mut sum_r = 0u64;
    let mut sum_g = 0u64;
    let mut sum_b = 0u64;
    for chunk in pixels.chunks_exact(4) {
        if chunk[0] > 40 || chunk[1] > 40 || chunk[2] > 40 {
            n += 1;
            sum_r += chunk[0] as u64;
            sum_g += chunk[1] as u64;
            sum_b += chunk[2] as u64;
        }
    }
    assert!(n > 100, "override didn't render");
    let avg_r = sum_r / n as u64;
    let avg_g = sum_g / n as u64;
    let avg_b = sum_b / n as u64;
    assert!(
        avg_r > avg_g * 4 && avg_r > avg_b * 4,
        "color override not red-dominant: r={avg_r} g={avg_g} b={avg_b}",
    );
}

#[test]
fn scale_dilates_bounding_box() {
    let unit = render_one(ObjectState {
        id: 1,
        object: unit_square(),
        position: [0.0, 0.0, 0.0],
        opacity: 1.0,
        rotation: 0.0,
        scale: 1.0,
        color_override: None,
    });
    let doubled = render_one(ObjectState {
        id: 1,
        object: unit_square(),
        position: [0.0, 0.0, 0.0],
        opacity: 1.0,
        rotation: 0.0,
        scale: 2.0,
        color_override: None,
    });

    // Bounding box of touched pixels (rough proxy for stroke extent).
    let bbox = |rgba: &[u8], threshold: u8| -> (u32, u32, u32, u32) {
        let mut x_min = WIDTH;
        let mut y_min = HEIGHT;
        let mut x_max = 0;
        let mut y_max = 0;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let i = ((y * WIDTH + x) * 4) as usize;
                if rgba[i] > threshold || rgba[i + 1] > threshold || rgba[i + 2] > threshold {
                    if x < x_min {
                        x_min = x;
                    }
                    if y < y_min {
                        y_min = y;
                    }
                    if x > x_max {
                        x_max = x;
                    }
                    if y > y_max {
                        y_max = y;
                    }
                }
            }
        }
        (x_min, y_min, x_max, y_max)
    };

    let (ux0, uy0, ux1, uy1) = bbox(&unit, 40);
    let (dx0, dy0, dx1, dy1) = bbox(&doubled, 40);
    let unit_w = (ux1 - ux0) as f64;
    let unit_h = (uy1 - uy0) as f64;
    let doubled_w = (dx1 - dx0) as f64;
    let doubled_h = (dy1 - dy0) as f64;

    // Doubling scale should ~double both axes of the bbox. Allow ±15% slack
    // for stroke-width contributions to the bbox edges.
    assert!(
        (doubled_w / unit_w - 2.0).abs() < 0.15,
        "scale=2 width ratio: {} (unit={unit_w}, doubled={doubled_w})",
        doubled_w / unit_w,
    );
    assert!(
        (doubled_h / unit_h - 2.0).abs() < 0.15,
        "scale=2 height ratio: {} (unit={unit_h}, doubled={doubled_h})",
        doubled_h / unit_h,
    );
}

#[test]
fn rotation_keeps_centroid_centered_for_symmetric_shape() {
    // A unit square rotated about its own center stays centered. This proves
    // rotation is applied before translation (T·R·S order), not after — if
    // we had `T · S · R`, an off-origin object would orbit instead of spin
    // in place. The scene origin maps to pixel (240, 135) at SLICE_B_DEFAULT
    // and 480×270.
    let pixels = render_one(ObjectState {
        id: 1,
        object: unit_square(),
        position: [0.0, 0.0, 0.0],
        opacity: 1.0,
        rotation: std::f32::consts::FRAC_PI_4,
        scale: 1.0,
        color_override: None,
    });

    let (n, cx, cy) = bright_centroid_and_count(&pixels, 40);
    assert!(n > 100, "no stroke drawn");
    assert!(
        (cx - 240.0).abs() < 3.0,
        "rotated centroid x={cx} drifted from 240",
    );
    assert!(
        (cy - 135.0).abs() < 3.0,
        "rotated centroid y={cy} drifted from 135",
    );
}

#[test]
fn rotation_with_translation_orbits_correctly() {
    // Translate then rotate composition (T · R) means the rotation pivots
    // around the object's *local* origin, not the scene origin. So an object
    // translated to (+2, 0) then rotated stays at (+2, 0); only its body
    // spins. SLICE_B_DEFAULT camera maps scene x=+2 to pixel 240 + 30·2 = 300.
    let pixels = render_one(ObjectState {
        id: 1,
        object: unit_square(),
        position: [2.0, 0.0, 0.0],
        opacity: 1.0,
        rotation: std::f32::consts::FRAC_PI_4,
        scale: 1.0,
        color_override: None,
    });

    let (n, cx, cy) = bright_centroid_and_count(&pixels, 40);
    assert!(n > 100, "no stroke drawn");
    assert!(
        (cx - 300.0).abs() < 5.0,
        "translated+rotated centroid x={cx} expected ≈300",
    );
    assert!(
        (cy - 135.0).abs() < 5.0,
        "translated+rotated centroid y={cy} expected ≈135",
    );
}
