//! Camera — orthographic 2D for Slice B.
//!
//! Matches manimgl's 16:9 default frame. 3D camera with phi/theta lands in
//! Slice F; this module will grow a `Perspective` variant then.

use glam::Mat4;

/// Orthographic viewport in scene units. Slice B hardcodes this to the 16:9
/// frame at `[-8, 8] × [-4.5, 4.5]`, matching `docs/slices/slice-b.md` §2.
#[derive(Copy, Clone, Debug)]
pub struct Camera {
    pub left: f32,
    pub right: f32,
    pub bottom: f32,
    pub top: f32,
}

impl Camera {
    /// 16:9 frame at `[-8, 8] × [-4.5, 4.5]` — the only camera Slice B uses.
    pub const SLICE_B_DEFAULT: Camera = Camera {
        left: -8.0,
        right: 8.0,
        bottom: -4.5,
        top: 4.5,
    };

    /// Build the orthographic projection matrix for this viewport.
    pub fn projection(&self) -> Mat4 {
        // near=-1, far=1: Slice B is planar; z is squashed without discarding.
        Mat4::orthographic_rh(self.left, self.right, self.bottom, self.top, -1.0, 1.0)
    }
}
