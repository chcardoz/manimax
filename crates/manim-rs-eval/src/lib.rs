//! Manimax IR evaluator.
//!
//! `eval_at(scene, t)` returns a `SceneState` — a flat, render-ready snapshot
//! of the scene at time `t`. The function is pure: same input, same output, no
//! internal caching, no history carried across calls. This is what makes
//! random-access frame rendering, parallel chunked rendering, and memoization
//! by `(ir_hash, t)` all possible downstream.
//!
//! Replaces the role of `manimlib/animation/animation.py`'s `interpolate`.
//! Reimplemented, not ported.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use manim_rs_ir::{
    ColorSegment, Easing, Object, ObjectId, OpacitySegment, PositionSegment, RgbaSrgb,
    RotationSegment, ScaleSegment, Scene, Time, TimelineOp, Track, Vec3,
};
use serde::{Deserialize, Serialize};

/// A single object's state at a given time — what the rasterizer draws.
///
/// Track-derived fields use neutral defaults when no track of that kind
/// references the object: `opacity = 1.0`, `rotation = 0.0`, `scale = 1.0`,
/// `color_override = None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectState {
    pub id: ObjectId,
    // `Arc` so `eval_at` clones geometry by ref-count bump rather than a deep
    // copy of `Vec<Vec3>` / `Vec<PathVerb>` per frame.
    #[serde(with = "arc_object_serde")]
    pub object: Arc<Object>,
    pub position: Vec3,
    pub opacity: f32,
    /// Radians, additive across multiple Rotation tracks.
    pub rotation: f32,
    /// Uniform; multiplicative across multiple Scale tracks.
    pub scale: f32,
    /// `Some` ⇒ the rasterizer must replace the object's stroke and fill
    /// colors with this value. `None` ⇒ render with the geometry's authored
    /// stroke / fill. The most-recently-active Color segment wins; multiple
    /// Color tracks on the same id are not composed (override semantics).
    pub color_override: Option<RgbaSrgb>,
}

impl ObjectState {
    /// Construct an `ObjectState` with neutral track-derived fields. Useful
    /// for raster tests that bypass `eval_at` and build a state literal: they
    /// only care about position-and-geometry.
    pub fn with_defaults(id: ObjectId, object: impl Into<Arc<Object>>, position: Vec3) -> Self {
        Self {
            id,
            object: object.into(),
            position,
            opacity: 1.0,
            rotation: 0.0,
            scale: 1.0,
            color_override: None,
        }
    }
}

mod arc_object_serde {
    use std::sync::Arc;

    use manim_rs_ir::Object;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(object: &Arc<Object>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        object.as_ref().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<Object>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Arc::new(Object::deserialize(deserializer)?))
    }
}

/// The whole scene's state at a given time.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SceneState {
    pub objects: Vec<ObjectState>,
}

/// Runtime-friendly evaluator state compiled once from an authored `Scene`.
#[derive(Debug, Clone, Default)]
pub struct Evaluator {
    timeline: Vec<CompiledTimelineOp>,
    tracks: HashMap<ObjectId, TrackBundle>,
}

#[derive(Debug, Clone)]
enum CompiledTimelineOp {
    Add {
        t: Time,
        id: ObjectId,
        object: Arc<Object>,
    },
    Remove {
        t: Time,
        id: ObjectId,
    },
}

/// Evaluate the scene at absolute time `t`.
///
/// Track composition semantics (Slice C Step 3):
/// - Position: additive across Position tracks. Default `[0, 0, 0]`.
/// - Opacity: multiplicative across Opacity tracks. Default `1.0`.
/// - Rotation: additive across Rotation tracks (radians). Default `0.0`.
/// - Scale: multiplicative across Scale tracks. Default `1.0`.
/// - Color: override (last active wins). Default `None` ⇒ use authored color.
///
/// Trusted (not validated): timeline sorted by `t`, segment ids exist,
/// segments non-overlapping within a track.
pub fn eval_at(scene: &Scene, t: Time) -> SceneState {
    Evaluator::from_scene(scene).eval_at(t)
}

impl Evaluator {
    /// Compile an authored `Scene` into an evaluator that owns preindexed
    /// tracks and shared object geometry. This is the fast path for renderers
    /// that evaluate many times.
    pub fn new(scene: Scene) -> Self {
        let Scene {
            metadata: _,
            timeline,
            tracks,
        } = scene;

        let timeline = timeline
            .into_iter()
            .map(|op| match op {
                TimelineOp::Add { t, id, object } => CompiledTimelineOp::Add {
                    t,
                    id,
                    object: Arc::new(object),
                },
                TimelineOp::Remove { t, id } => CompiledTimelineOp::Remove { t, id },
            })
            .collect();

        Self {
            timeline,
            tracks: index_tracks(tracks),
        }
    }

    /// Convenience for one-off callers that only have `&Scene`.
    pub fn from_scene(scene: &Scene) -> Self {
        Self::new(scene.clone())
    }

    /// Evaluate the compiled scene at absolute time `t`.
    pub fn eval_at(&self, t: Time) -> SceneState {
        let mut objects = Vec::new();
        for (id, object) in active_objects_at(&self.timeline, t) {
            let bundle = self.tracks.get(&id);
            objects.push(ObjectState {
                id,
                object: Arc::clone(object),
                position: bundle.map_or([0.0; 3], |b| sum_segments(&b.position, t)),
                opacity: bundle.map_or(1.0, |b| product_scalars(&b.opacity, t)),
                rotation: bundle.map_or(0.0, |b| sum_scalars(&b.rotation, t)),
                scale: bundle.map_or(1.0, |b| product_scalars(&b.scale, t)),
                color_override: bundle.and_then(|b| latest_segments(&b.color, t)),
            });
        }
        SceneState { objects }
    }
}

/// Active objects whose most recent timeline event at `t' <= t` is an `Add`.
/// Preserves first-add order so render order remains deterministic.
fn active_objects_at<'a>(
    timeline: &'a [CompiledTimelineOp],
    t: Time,
) -> Vec<(ObjectId, &'a Arc<Object>)> {
    let mut first_seen: Vec<ObjectId> = Vec::new();
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut active: HashMap<ObjectId, &'a Arc<Object>> = HashMap::new();

    for op in timeline {
        let op_t = match op {
            CompiledTimelineOp::Add { t, .. } | CompiledTimelineOp::Remove { t, .. } => *t,
        };
        if op_t > t {
            break;
        }

        match op {
            CompiledTimelineOp::Add { id, object, .. } => {
                if seen.insert(*id) {
                    first_seen.push(*id);
                }
                active.insert(*id, object);
            }
            CompiledTimelineOp::Remove { id, .. } => {
                active.remove(id);
            }
        }
    }

    first_seen
        .into_iter()
        .filter_map(|id| active.remove(&id).map(|object| (id, object)))
        .collect()
}

// ---------------------------------------------------------------------------
// Track indexing + generic segment evaluation.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct TrackBundle {
    position: Vec<Vec<PositionSegment>>,
    opacity: Vec<Vec<OpacitySegment>>,
    rotation: Vec<Vec<RotationSegment>>,
    scale: Vec<Vec<ScaleSegment>>,
    color: Vec<Vec<ColorSegment>>,
}

fn index_tracks(tracks: Vec<Track>) -> HashMap<ObjectId, TrackBundle> {
    let mut index: HashMap<ObjectId, TrackBundle> = HashMap::new();
    for track in tracks {
        match track {
            Track::Position { id, segments } => {
                index.entry(id).or_default().position.push(segments);
            }
            Track::Opacity { id, segments } => {
                index.entry(id).or_default().opacity.push(segments);
            }
            Track::Rotation { id, segments } => {
                index.entry(id).or_default().rotation.push(segments);
            }
            Track::Scale { id, segments } => {
                index.entry(id).or_default().scale.push(segments);
            }
            Track::Color { id, segments } => {
                index.entry(id).or_default().color.push(segments);
            }
        }
    }
    index
}

/// Small linear-interpolation trait used to unify segment evaluation across
/// `f32` / `Vec3` / `RgbaSrgb`. The three `lerp` impls are the only thing that
/// used to differ between the five per-kind `evaluate_*_track` copies.
trait Lerp: Copy {
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

/// Uniform shape across segment types: each has `t0`, `t1`, `from`, `to`, `easing`.
trait Segment {
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
fn evaluate_track<S: Segment>(segments: &[S], t: Time) -> Option<S::V> {
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

fn segment_alpha(t0: Time, t1: Time, t: Time) -> f32 {
    if t1 == t0 {
        1.0
    } else {
        ((t - t0) / (t1 - t0)) as f32
    }
}

fn sum_segments<S>(tracks: &[Vec<S>], t: Time) -> Vec3
where
    S: Segment<V = Vec3>,
{
    let mut out = [0.0_f32; 3];
    for segs in tracks {
        if let Some(v) = evaluate_track(segs.as_slice(), t) {
            out[0] += v[0];
            out[1] += v[1];
            out[2] += v[2];
        }
    }
    out
}

fn sum_scalars<S>(tracks: &[Vec<S>], t: Time) -> f32
where
    S: Segment<V = f32>,
{
    let mut out = 0.0_f32;
    for segs in tracks {
        if let Some(v) = evaluate_track(segs.as_slice(), t) {
            out += v;
        }
    }
    out
}

fn product_scalars<S>(tracks: &[Vec<S>], t: Time) -> f32
where
    S: Segment<V = f32>,
{
    let mut out = 1.0_f32;
    for segs in tracks {
        if let Some(v) = evaluate_track(segs.as_slice(), t) {
            out *= v;
        }
    }
    out
}

/// Override semantics: among all parallel tracks with a contributing
/// segment at `t` (active or held), the one whose contributing segment has
/// the latest `t0` wins. Deterministic from timeline data alone — does not
/// depend on list ordering of tracks. `None` ⇒ no override.
fn latest_segments<S>(tracks: &[Vec<S>], t: Time) -> Option<S::V>
where
    S: Segment<V = RgbaSrgb>,
{
    let mut best: Option<(Time, S::V)> = None;
    for segs in tracks {
        let mut contributing: Option<(Time, S::V)> = None;
        for seg in segs {
            let (t0, t1) = (seg.t0(), seg.t1());
            if t >= t0 && t <= t1 {
                let alpha = segment_alpha(t0, t1, t);
                let eased = apply_easing(seg.easing(), alpha);
                contributing = Some((t0, Lerp::lerp(seg.from(), seg.to(), eased)));
            } else if t1 < t {
                contributing = Some((t0, seg.to()));
            }
        }
        if let Some((t0, v)) = contributing {
            if best.map_or(true, |(best_t0, _)| t0 >= best_t0) {
                best = Some((t0, v));
            }
        }
    }
    best.map(|(_, v)| v)
}

// ---------------------------------------------------------------------------
// Easings — ported from `reference/manimgl/manimlib/utils/rate_functions.py`.
// Formulas are 1:1 with the Python source so Python-authored easings are
// pixel-equivalent in Rust.
// ---------------------------------------------------------------------------

fn apply_easing(easing: &Easing, alpha: f32) -> f32 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use manim_rs_ir::{
        ColorSegment, OpacitySegment, Resolution, RotationSegment, SCHEMA_VERSION, ScaleSegment,
        SceneMetadata, Stroke, TimelineOp,
    };

    fn square_points() -> Vec<Vec3> {
        vec![
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ]
    }

    fn make_scene(timeline: Vec<TimelineOp>, tracks: Vec<Track>, duration: Time) -> Scene {
        Scene {
            metadata: SceneMetadata {
                schema_version: SCHEMA_VERSION,
                fps: 30,
                duration,
                resolution: Resolution {
                    width: 480,
                    height: 270,
                },
                background: [0.0, 0.0, 0.0, 1.0],
            },
            timeline,
            tracks,
        }
    }

    fn polyline() -> Object {
        Object::Polyline {
            points: square_points(),
            closed: true,
            stroke: Some(Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.04)),
            fill: None,
        }
    }

    fn slice_b_scene() -> Scene {
        make_scene(
            vec![TimelineOp::Add {
                t: 0.0,
                id: 1,
                object: polyline(),
            }],
            vec![Track::Position {
                id: 1,
                segments: vec![PositionSegment {
                    t0: 0.0,
                    t1: 2.0,
                    from: [0.0, 0.0, 0.0],
                    to: [2.0, 0.0, 0.0],
                    easing: Easing::Linear {},
                }],
            }],
            2.0,
        )
    }

    #[test]
    fn canonical_scene_at_zero() {
        let s = eval_at(&slice_b_scene(), 0.0);
        assert_eq!(s.objects.len(), 1);
        assert_eq!(s.objects[0].id, 1);
        assert_eq!(s.objects[0].position, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn canonical_scene_at_midpoint() {
        let s = eval_at(&slice_b_scene(), 1.0);
        assert_eq!(s.objects[0].position, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn canonical_scene_at_endpoint() {
        let s = eval_at(&slice_b_scene(), 2.0);
        assert_eq!(s.objects[0].position, [2.0, 0.0, 0.0]);
    }

    #[test]
    fn canonical_scene_past_endpoint_clamps() {
        let s = eval_at(&slice_b_scene(), 3.0);
        assert_eq!(s.objects[0].position, [2.0, 0.0, 0.0]);
    }

    #[test]
    fn removed_object_disappears() {
        let scene = make_scene(
            vec![
                TimelineOp::Add {
                    t: 0.0,
                    id: 1,
                    object: polyline(),
                },
                TimelineOp::Remove { t: 1.0, id: 1 },
            ],
            vec![],
            1.0,
        );
        assert_eq!(eval_at(&scene, 0.5).objects.len(), 1);
        // Remove is inclusive at its timestamp — by t=1.0 the object is gone.
        assert_eq!(eval_at(&scene, 1.0).objects.len(), 0);
        assert_eq!(eval_at(&scene, 2.0).objects.len(), 0);
    }

    #[test]
    fn object_without_track_sits_at_origin() {
        let scene = make_scene(
            vec![TimelineOp::Add {
                t: 0.0,
                id: 1,
                object: polyline(),
            }],
            vec![],
            1.0,
        );
        let state = eval_at(&scene, 0.5);
        assert_eq!(state.objects[0].position, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn not_yet_added_object_absent() {
        let scene = make_scene(
            vec![TimelineOp::Add {
                t: 1.0,
                id: 1,
                object: polyline(),
            }],
            vec![],
            2.0,
        );
        assert_eq!(eval_at(&scene, 0.5).objects.len(), 0);
        assert_eq!(eval_at(&scene, 1.0).objects.len(), 1);
    }

    #[test]
    fn two_parallel_position_tracks_sum() {
        let scene = make_scene(
            vec![TimelineOp::Add {
                t: 0.0,
                id: 1,
                object: polyline(),
            }],
            vec![
                Track::Position {
                    id: 1,
                    segments: vec![PositionSegment {
                        t0: 0.0,
                        t1: 2.0,
                        from: [0.0, 0.0, 0.0],
                        to: [2.0, 0.0, 0.0],
                        easing: Easing::Linear {},
                    }],
                },
                Track::Position {
                    id: 1,
                    segments: vec![PositionSegment {
                        t0: 0.0,
                        t1: 2.0,
                        from: [0.0, 0.0, 0.0],
                        to: [0.0, 1.0, 0.0],
                        easing: Easing::Linear {},
                    }],
                },
            ],
            2.0,
        );
        let state = eval_at(&scene, 1.0);
        assert_eq!(state.objects[0].position, [1.0, 0.5, 0.0]);
    }

    #[test]
    fn gap_between_segments_holds_last_to() {
        let scene = make_scene(
            vec![TimelineOp::Add {
                t: 0.0,
                id: 1,
                object: polyline(),
            }],
            vec![Track::Position {
                id: 1,
                segments: vec![
                    PositionSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: [0.0, 0.0, 0.0],
                        to: [1.0, 0.0, 0.0],
                        easing: Easing::Linear {},
                    },
                    PositionSegment {
                        t0: 2.0,
                        t1: 3.0,
                        from: [1.0, 0.0, 0.0],
                        to: [2.0, 0.0, 0.0],
                        easing: Easing::Linear {},
                    },
                ],
            }],
            3.0,
        );
        // In the gap (t=1.5) we hold the last reached `to`.
        assert_eq!(eval_at(&scene, 1.5).objects[0].position, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn zero_duration_segment_jumps_to_endpoint() {
        // Covers the explicit `if seg.t1 == seg.t0 { 1.0 }` branch — without
        // which this would divide by zero and produce NaN.
        let scene = make_scene(
            vec![TimelineOp::Add {
                t: 0.0,
                id: 1,
                object: polyline(),
            }],
            vec![Track::Position {
                id: 1,
                segments: vec![PositionSegment {
                    t0: 1.0,
                    t1: 1.0,
                    from: [0.0, 0.0, 0.0],
                    to: [2.0, 0.0, 0.0],
                    easing: Easing::Linear {},
                }],
            }],
            2.0,
        );
        assert_eq!(eval_at(&scene, 1.0).objects[0].position, [2.0, 0.0, 0.0]);
    }

    // ----------------------------------------------------------------
    // Step 3: Opacity / Rotation / Scale / Color track aggregation.
    // ----------------------------------------------------------------

    fn one_object_scene(tracks: Vec<Track>, duration: Time) -> Scene {
        make_scene(
            vec![TimelineOp::Add {
                t: 0.0,
                id: 1,
                object: polyline(),
            }],
            tracks,
            duration,
        )
    }

    #[test]
    fn defaults_when_no_tracks_present() {
        let s = eval_at(&one_object_scene(vec![], 1.0), 0.5);
        assert_eq!(s.objects[0].opacity, 1.0);
        assert_eq!(s.objects[0].rotation, 0.0);
        assert_eq!(s.objects[0].scale, 1.0);
        assert_eq!(s.objects[0].color_override, None);
    }

    #[test]
    fn opacity_track_lerps_and_holds() {
        let scene = one_object_scene(
            vec![Track::Opacity {
                id: 1,
                segments: vec![OpacitySegment {
                    t0: 0.0,
                    t1: 1.0,
                    from: 0.0,
                    to: 1.0,
                    easing: Easing::Linear {},
                }],
            }],
            2.0,
        );
        assert_eq!(eval_at(&scene, 0.0).objects[0].opacity, 0.0);
        assert_eq!(eval_at(&scene, 0.5).objects[0].opacity, 0.5);
        assert_eq!(eval_at(&scene, 1.0).objects[0].opacity, 1.0);
        // Past the segment, value holds at `to`.
        assert_eq!(eval_at(&scene, 1.5).objects[0].opacity, 1.0);
    }

    #[test]
    fn opacity_tracks_compose_multiplicatively() {
        let half = |t0, t1| OpacitySegment {
            t0,
            t1,
            from: 0.5,
            to: 0.5,
            easing: Easing::Linear {},
        };
        let scene = one_object_scene(
            vec![
                Track::Opacity {
                    id: 1,
                    segments: vec![half(0.0, 1.0)],
                },
                Track::Opacity {
                    id: 1,
                    segments: vec![half(0.0, 1.0)],
                },
            ],
            1.0,
        );
        assert_eq!(eval_at(&scene, 0.5).objects[0].opacity, 0.25);
    }

    #[test]
    fn rotation_tracks_sum_in_radians() {
        let scene = one_object_scene(
            vec![
                Track::Rotation {
                    id: 1,
                    segments: vec![RotationSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: 0.0,
                        to: std::f32::consts::PI,
                        easing: Easing::Linear {},
                    }],
                },
                Track::Rotation {
                    id: 1,
                    segments: vec![RotationSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: 0.0,
                        to: std::f32::consts::PI,
                        easing: Easing::Linear {},
                    }],
                },
            ],
            1.0,
        );
        // Two parallel half-turns at midpoint = 2 · π/2 = π.
        assert!((eval_at(&scene, 0.5).objects[0].rotation - std::f32::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn scale_tracks_compose_multiplicatively() {
        let scene = one_object_scene(
            vec![
                Track::Scale {
                    id: 1,
                    segments: vec![ScaleSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: 2.0,
                        to: 2.0,
                        easing: Easing::Linear {},
                    }],
                },
                Track::Scale {
                    id: 1,
                    segments: vec![ScaleSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: 3.0,
                        to: 3.0,
                        easing: Easing::Linear {},
                    }],
                },
            ],
            1.0,
        );
        assert_eq!(eval_at(&scene, 0.5).objects[0].scale, 6.0);
    }

    #[test]
    fn color_track_overrides_authored_color() {
        let red: RgbaSrgb = [1.0, 0.0, 0.0, 1.0];
        let blue: RgbaSrgb = [0.0, 0.0, 1.0, 1.0];
        let scene = one_object_scene(
            vec![Track::Color {
                id: 1,
                segments: vec![ColorSegment {
                    t0: 0.0,
                    t1: 1.0,
                    from: red,
                    to: blue,
                    easing: Easing::Linear {},
                }],
            }],
            2.0,
        );
        // At midpoint the override is the lerp.
        let mid = eval_at(&scene, 0.5).objects[0].color_override.unwrap();
        assert!((mid[0] - 0.5).abs() < 1e-6);
        assert!((mid[2] - 0.5).abs() < 1e-6);
        // Past the segment, value holds at `to`.
        assert_eq!(eval_at(&scene, 1.5).objects[0].color_override, Some(blue));
    }

    #[test]
    fn second_color_track_takes_over_override() {
        // Two Color tracks on the same id with identical timing (`t0` tie):
        // ties break to the later-iterated track, so the second-declared
        // track's value surfaces.
        let red: RgbaSrgb = [1.0, 0.0, 0.0, 1.0];
        let green: RgbaSrgb = [0.0, 1.0, 0.0, 1.0];
        let scene = one_object_scene(
            vec![
                Track::Color {
                    id: 1,
                    segments: vec![ColorSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: red,
                        to: red,
                        easing: Easing::Linear {},
                    }],
                },
                Track::Color {
                    id: 1,
                    segments: vec![ColorSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: green,
                        to: green,
                        easing: Easing::Linear {},
                    }],
                },
            ],
            1.0,
        );
        assert_eq!(eval_at(&scene, 0.5).objects[0].color_override, Some(green));
    }

    #[test]
    fn color_override_picks_latest_t0_not_list_order() {
        // Track A is declared second but started earlier; Track B is declared
        // first but its contributing segment starts later. The winner is
        // decided by segment `t0`, not list order — so B (blue) wins at a
        // time where both are held, even though A is later in the array.
        let red: RgbaSrgb = [1.0, 0.0, 0.0, 1.0];
        let blue: RgbaSrgb = [0.0, 0.0, 1.0, 1.0];
        let scene = one_object_scene(
            vec![
                // Declared FIRST, but its segment starts LATER (t0=2.0).
                Track::Color {
                    id: 1,
                    segments: vec![ColorSegment {
                        t0: 2.0,
                        t1: 3.0,
                        from: blue,
                        to: blue,
                        easing: Easing::Linear {},
                    }],
                },
                // Declared SECOND, but segment starts EARLIER (t0=0.0).
                Track::Color {
                    id: 1,
                    segments: vec![ColorSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: red,
                        to: red,
                        easing: Easing::Linear {},
                    }],
                },
            ],
            4.0,
        );
        // At t=2.5 both tracks contribute (red held from segment end, blue
        // active). Latest-t0 wins → blue, regardless of list order.
        assert_eq!(eval_at(&scene, 2.5).objects[0].color_override, Some(blue));
        // At t=3.5 both tracks are held. Latest-t0 still wins → blue.
        assert_eq!(eval_at(&scene, 3.5).objects[0].color_override, Some(blue));
    }

    #[test]
    fn composite_scene_evaluates_every_track_kind() {
        // One object with a track of every kind active. Asserts each output
        // field independently — proves the eval function isn't mixing them up.
        let scene = one_object_scene(
            vec![
                Track::Position {
                    id: 1,
                    segments: vec![PositionSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: [0.0, 0.0, 0.0],
                        to: [2.0, 0.0, 0.0],
                        easing: Easing::Linear {},
                    }],
                },
                Track::Opacity {
                    id: 1,
                    segments: vec![OpacitySegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: 0.0,
                        to: 1.0,
                        easing: Easing::Linear {},
                    }],
                },
                Track::Rotation {
                    id: 1,
                    segments: vec![RotationSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: 0.0,
                        to: 1.0,
                        easing: Easing::Linear {},
                    }],
                },
                Track::Scale {
                    id: 1,
                    segments: vec![ScaleSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: 1.0,
                        to: 3.0,
                        easing: Easing::Linear {},
                    }],
                },
                Track::Color {
                    id: 1,
                    segments: vec![ColorSegment {
                        t0: 0.0,
                        t1: 1.0,
                        from: [0.0, 0.0, 0.0, 1.0],
                        to: [1.0, 1.0, 1.0, 1.0],
                        easing: Easing::Linear {},
                    }],
                },
            ],
            1.0,
        );
        let s = &eval_at(&scene, 0.5).objects[0];
        assert_eq!(s.position, [1.0, 0.0, 0.0]);
        assert_eq!(s.opacity, 0.5);
        assert_eq!(s.rotation, 0.5);
        assert_eq!(s.scale, 2.0);
        let c = s.color_override.unwrap();
        assert!((c[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn multiple_add_remove_cycles_track_liveness() {
        // Four alternating ops on the same id — `active_ids_at` must update
        // liveness per op, not latch on the first Add.
        let scene = make_scene(
            vec![
                TimelineOp::Add {
                    t: 0.0,
                    id: 1,
                    object: polyline(),
                },
                TimelineOp::Remove { t: 1.0, id: 1 },
                TimelineOp::Add {
                    t: 2.0,
                    id: 1,
                    object: polyline(),
                },
                TimelineOp::Remove { t: 3.0, id: 1 },
            ],
            vec![],
            3.0,
        );
        assert_eq!(eval_at(&scene, 0.5).objects.len(), 1, "present in [0,1)");
        assert_eq!(eval_at(&scene, 1.0).objects.len(), 0, "removed at t=1");
        assert_eq!(eval_at(&scene, 1.5).objects.len(), 0, "absent in [1,2)");
        assert_eq!(eval_at(&scene, 2.0).objects.len(), 1, "re-added at t=2");
        assert_eq!(eval_at(&scene, 2.5).objects.len(), 1, "present in [2,3)");
        assert_eq!(eval_at(&scene, 3.0).objects.len(), 0, "removed at t=3");
    }
}
