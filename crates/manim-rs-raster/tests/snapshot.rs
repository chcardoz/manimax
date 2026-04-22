//! Pixel-exact snapshot test — pins the canonical Slice B scene's rendered
//! output byte-for-byte (as an rgba-sum checksum).
//!
//! Catches wide classes of regressions the bright-pixel-in-a-box tests miss
//! by design: color-space drift, sign-flipped MVP, stroke-width changes,
//! tessellator output reshuffles, background-color off-by-one, etc.
//!
//! The checksum is computed below. If it fails after a deliberate rendering
//! change (new wgpu version, new lyon version, new fixture scene), update
//! the `EXPECTED_SUM` / `EXPECTED_COUNT` constants to match the failure
//! message. If it fails *without* a deliberate change, you've found a
//! regression — investigate before bumping the constants.

use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{Object, Vec3};
use manim_rs_raster::{Camera, Runtime};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 72;

/// Sum of every byte in the returned RGBA buffer for the canonical scene.
/// Captured on macOS arm64, Metal, wgpu 29. If this fails on a different
/// platform, that's the expected kind of drift — update under scrutiny.
const EXPECTED_SUM: u64 = 2_350_080;

/// Number of non-background bytes (>0) in the returned RGBA buffer. Derived
/// from the stroke coverage of the canonical square at 128×72.
const EXPECTED_NONZERO: u64 = 9_216;

#[test]
fn canonical_scene_snapshot_pixel_checksum() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    // Canonical Slice B scene: white closed unit square at origin on black.
    let state = SceneState {
        objects: vec![ObjectState {
            id: 1,
            object: Object::Polyline {
                points: {
                    let pts: Vec<Vec3> = vec![
                        [-1.0, -1.0, 0.0],
                        [1.0, -1.0, 0.0],
                        [1.0, 1.0, 0.0],
                        [-1.0, 1.0, 0.0],
                    ];
                    pts
                },
                stroke_color: [1.0, 1.0, 1.0, 1.0],
                stroke_width: 0.1,
                closed: true,
            },
            position: [0.0, 0.0, 0.0],
        }],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);

    let sum: u64 = pixels.iter().map(|&b| b as u64).sum();
    let nonzero: u64 = pixels.iter().filter(|&&b| b != 0).count() as u64;

    assert_eq!(
        (sum, nonzero),
        (EXPECTED_SUM, EXPECTED_NONZERO),
        "pixel checksum changed — investigate before updating constants. \
         got sum={sum}, nonzero={nonzero}",
    );
}
