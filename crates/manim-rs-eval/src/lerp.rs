//! Linear interpolation between values used by track segments. The three
//! `Lerp` impls are the only thing that used to differ between the per-kind
//! `evaluate_*_track` copies — extracting the trait collapsed five copies
//! into one generic `evaluate_track`.

/// Componentwise linear interpolation. `alpha` is already eased (in `[0, 1]`).
pub(crate) trait Lerp: Copy {
    fn lerp(from: Self, to: Self, alpha: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(a: f32, b: f32, alpha: f32) -> f32 {
        a + (b - a) * alpha
    }
}

impl Lerp for [f32; 3] {
    fn lerp(a: [f32; 3], b: [f32; 3], alpha: f32) -> [f32; 3] {
        [
            f32::lerp(a[0], b[0], alpha),
            f32::lerp(a[1], b[1], alpha),
            f32::lerp(a[2], b[2], alpha),
        ]
    }
}

impl Lerp for [f32; 4] {
    fn lerp(a: [f32; 4], b: [f32; 4], alpha: f32) -> [f32; 4] {
        [
            f32::lerp(a[0], b[0], alpha),
            f32::lerp(a[1], b[1], alpha),
            f32::lerp(a[2], b[2], alpha),
            f32::lerp(a[3], b[3], alpha),
        ]
    }
}
