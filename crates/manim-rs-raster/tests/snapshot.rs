//! Tolerance-based snapshot of the canonical Slice B/C scene.
//!
//! MSAA makes byte-exact equality brittle — sub-pixel coverage depends on the
//! rasterizer's sample pattern, which varies between wgpu backends and
//! driver revisions. Instead we pin expected sum/nonzero counts with a ±5%
//! tolerance band. That still catches the wide regressions the byte-exact
//! version did (color-space drift, sign-flipped MVP, stroke-width changes,
//! tessellator output reshuffles, background off-by-one) without tripping on
//! a benign MSAA-pattern change.
//!
//! If this fails after a deliberate rendering change, update `EXPECTED_*`
//! constants to match the failure message. If it fails *without* a deliberate
//! change, you've found a regression — investigate before bumping.

use manim_rs_eval::{ObjectState, SceneState};
use manim_rs_ir::{Object, Stroke, Vec3};
use manim_rs_raster::{Camera, Runtime};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 72;

/// Sum of every byte in the returned RGBA buffer for the canonical scene.
/// Captured on macOS arm64, Metal, wgpu 29, MSAA 4×.
const EXPECTED_SUM: u64 = 2_422_104;

/// Number of non-background bytes (>0) in the returned RGBA buffer.
/// MSAA introduces partially-covered edge pixels, so this is slightly higher
/// than the pre-MSAA count (9216 → ~9600 range).
const EXPECTED_NONZERO: u64 = 9_600;

/// ±5% band. Wide enough to absorb MSAA sample-pattern drift between backends,
/// tight enough to still flag real regressions.
const TOLERANCE: f64 = 0.05;

#[test]
fn canonical_scene_snapshot_pixel_checksum() {
    let runtime = Runtime::new(WIDTH, HEIGHT).expect("runtime");

    let state = SceneState {
        objects: vec![ObjectState::with_defaults(
            1,
            Object::Polyline {
                points: {
                    let pts: Vec<Vec3> = vec![
                        [-1.0, -1.0, 0.0],
                        [1.0, -1.0, 0.0],
                        [1.0, 1.0, 0.0],
                        [-1.0, 1.0, 0.0],
                    ];
                    pts
                },
                closed: true,
                stroke: Some(Stroke {
                    color: [1.0, 1.0, 1.0, 1.0],
                    width: 0.1,
                }),
                fill: None,
            },
            [0.0, 0.0, 0.0],
        )],
    };

    let pixels = runtime
        .render(&state, &Camera::SLICE_B_DEFAULT, [0.0, 0.0, 0.0, 1.0])
        .expect("render");

    assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);

    let sum: u64 = pixels.iter().map(|&b| b as u64).sum();
    let nonzero: u64 = pixels.iter().filter(|&&b| b != 0).count() as u64;

    assert_within(sum, EXPECTED_SUM, TOLERANCE, "byte sum");
    assert_within(nonzero, EXPECTED_NONZERO, TOLERANCE, "nonzero count");
}

fn assert_within(actual: u64, expected: u64, tolerance: f64, label: &str) {
    let delta = (actual as f64 - expected as f64).abs();
    let allowed = expected as f64 * tolerance;
    assert!(
        delta <= allowed,
        "{label} drift outside ±{:.0}% — got {actual}, expected {expected} (±{allowed:.0})",
        tolerance * 100.0,
    );
}
