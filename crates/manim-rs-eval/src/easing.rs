//! Easings — ported from `reference/manimgl/manimlib/utils/rate_functions.py`.
//! Formulas are 1:1 with the Python source so Python-authored easings are
//! pixel-equivalent in Rust.

use manim_rs_ir::Easing;

/// Map a linear `alpha` in `[0, 1]` through `easing` and return the eased
/// alpha. Used by track evaluation immediately before `Lerp::lerp`.
pub(crate) fn apply_easing(easing: &Easing, alpha: f32) -> f32 {
    match easing {
        Easing::Linear {} => alpha,
        Easing::Smooth {} => smooth(alpha),
        Easing::RushInto {} => 2.0 * smooth(0.5 * alpha),
        Easing::RushFrom {} => 2.0 * smooth(0.5 * (alpha + 1.0)) - 1.0,
        Easing::SlowInto {} => (1.0 - (1.0 - alpha) * (1.0 - alpha)).sqrt(),
        Easing::DoubleSmooth {} => {
            if alpha < 0.5 {
                0.5 * smooth(2.0 * alpha)
            } else {
                0.5 * (1.0 + smooth(2.0 * alpha - 1.0))
            }
        }
        Easing::ThereAndBack {} => {
            let h = if alpha < 0.5 {
                2.0 * alpha
            } else {
                2.0 * (1.0 - alpha)
            };
            smooth(h)
        }
        Easing::Lingering {} => squish(alpha, 0.0, 0.8, &Easing::Linear {}),
        Easing::ThereAndBackWithPause { pause_ratio } => {
            let p = *pause_ratio;
            let a = 2.0 / (1.0 - p);
            if alpha < 0.5 - p / 2.0 {
                smooth(a * alpha)
            } else if alpha < 0.5 + p / 2.0 {
                1.0
            } else {
                smooth(a - a * alpha)
            }
        }
        Easing::RunningStart { pull_factor } => {
            let p = *pull_factor;
            bezier_scalar(&[0.0, 0.0, p, p, 1.0, 1.0, 1.0], alpha)
        }
        Easing::Overshoot { pull_factor } => {
            let p = *pull_factor;
            bezier_scalar(&[0.0, 0.0, p, p, 1.0, 1.0], alpha)
        }
        Easing::Wiggle { wiggles } => {
            let h = if alpha < 0.5 {
                2.0 * alpha
            } else {
                2.0 * (1.0 - alpha)
            };
            smooth(h) * (wiggles * std::f32::consts::PI * alpha).sin()
        }
        Easing::ExponentialDecay { half_life } => 1.0 - (-alpha / *half_life).exp(),
        Easing::NotQuiteThere { inner, proportion } => *proportion * apply_easing(inner, alpha),
        Easing::SquishRateFunc { inner, a, b } => squish(alpha, *a, *b, inner),
    }
}

fn smooth(t: f32) -> f32 {
    // bezier([0, 0, 0, 1, 1, 1]) — zero first and second derivatives at t=0 and t=1.
    let s = 1.0 - t;
    t.powi(3) * (10.0 * s * s + 5.0 * s * t + t * t)
}

fn squish(t: f32, a: f32, b: f32, inner: &Easing) -> f32 {
    if a == b {
        a
    } else if t < a {
        apply_easing(inner, 0.0)
    } else if t > b {
        apply_easing(inner, 1.0)
    } else {
        apply_easing(inner, (t - a) / (b - a))
    }
}

/// Evaluate a scalar Bezier at `t` using Bernstein basis.
/// `coeffs.len()` control points = degree `coeffs.len() - 1` curve.
fn bezier_scalar(coeffs: &[f32], t: f32) -> f32 {
    let n = coeffs.len() - 1;
    let mut acc = 0.0_f32;
    let mut binom = 1.0_f32;
    let s = 1.0 - t;
    for (k, &c) in coeffs.iter().enumerate() {
        let term = binom * t.powi(k as i32) * s.powi((n - k) as i32) * c;
        acc += term;
        // Update binomial C(n, k+1) = C(n, k) * (n - k) / (k + 1).
        if k < n {
            binom = binom * (n - k) as f32 / (k + 1) as f32;
        }
    }
    acc
}
