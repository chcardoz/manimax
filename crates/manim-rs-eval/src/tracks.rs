//! Generic segment evaluation. The five `*Segment` types from `manim-rs-ir`
//! all share `(t0, t1, from, to, easing)`; the [`Segment`] trait abstracts
//! over them so [`evaluate_track`] only has to be written once.

use manim_rs_ir::{
    ColorSegment, Easing, OpacitySegment, PositionSegment, RgbaSrgb, RotationSegment, ScaleSegment,
    Time, Vec3,
};

use crate::easing::apply_easing;
use crate::lerp::Lerp;

/// Uniform shape across segment types: each has `t0`, `t1`, `from`, `to`,
/// `easing`. Implemented for all five `*Segment` IR types via a macro.
pub(crate) trait Segment {
    type V: Lerp;
    fn t0(&self) -> Time;
    fn t1(&self) -> Time;
    fn from(&self) -> Self::V;
    fn to(&self) -> Self::V;
    fn easing(&self) -> &Easing;
}

macro_rules! impl_segment {
    ($seg:ty, $v:ty) => {
        impl Segment for $seg {
            type V = $v;
            fn t0(&self) -> Time {
                self.t0
            }
            fn t1(&self) -> Time {
                self.t1
            }
            fn from(&self) -> $v {
                self.from
            }
            fn to(&self) -> $v {
                self.to
            }
            fn easing(&self) -> &Easing {
                &self.easing
            }
        }
    };
}

impl_segment!(PositionSegment, Vec3);
impl_segment!(OpacitySegment, f32);
impl_segment!(RotationSegment, f32);
impl_segment!(ScaleSegment, f32);
impl_segment!(ColorSegment, RgbaSrgb);

/// Piecewise evaluation of a sorted, non-overlapping list of segments.
/// Before every segment: `None`. Inside a segment: `lerp(from, to, ease(alpha))`.
/// In a gap or past the last segment: the `to` of the most recently completed
/// segment (i.e. the one whose `t1 <= t`).
pub(crate) fn evaluate_track<S: Segment>(segments: &[S], t: Time) -> Option<S::V> {
    let mut held: Option<S::V> = None;
    for seg in segments {
        let (t0, t1) = (seg.t0(), seg.t1());
        if t >= t0 && t <= t1 {
            let alpha = segment_alpha(t0, t1, t);
            let eased = apply_easing(seg.easing(), alpha);
            return Some(Lerp::lerp(seg.from(), seg.to(), eased));
        }
        if t1 < t {
            held = Some(seg.to());
        }
    }
    held
}

pub(crate) fn segment_alpha(t0: Time, t1: Time, t: Time) -> f32 {
    if t1 == t0 {
        1.0
    } else {
        ((t - t0) / (t1 - t0)) as f32
    }
}
