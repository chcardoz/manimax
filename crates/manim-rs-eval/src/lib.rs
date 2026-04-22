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

use manim_rs_ir::{
    ColorSegment, Easing, Object, ObjectId, OpacitySegment, PositionSegment, RgbaSrgb,
    RotationSegment, ScaleSegment, Scene, Time, Track, Vec3,
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
    pub object: Object,
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
    pub fn with_defaults(id: ObjectId, object: Object, position: Vec3) -> Self {
        Self {
            id,
            object,
            position,
            opacity: 1.0,
            rotation: 0.0,
            scale: 1.0,
            color_override: None,
        }
    }
}

/// The whole scene's state at a given time.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SceneState {
    pub objects: Vec<ObjectState>,
}

/// Evaluate the scene at absolute time `t`. Clones geometry into the state.
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
    let mut objects = Vec::new();
    for id in active_ids_at(scene, t) {
        let object = match latest_add_object(scene, id, t) {
            Some(o) => o.clone(),
            None => continue,
        };
        objects.push(ObjectState {
            id,
            object,
            position: sum_position_tracks(scene, id, t),
            opacity: product_opacity_tracks(scene, id, t),
            rotation: sum_rotation_tracks(scene, id, t),
            scale: product_scale_tracks(scene, id, t),
            color_override: latest_color_track(scene, id, t),
        });
    }
    SceneState { objects }
}

/// Ids whose most recent timeline event at `t' <= t` is an `Add`.
/// Preserves first-add order so the render order is deterministic.
fn active_ids_at(scene: &Scene, t: Time) -> Vec<ObjectId> {
    use manim_rs_ir::TimelineOp::{Add, Remove};

    let mut first_seen: Vec<ObjectId> = Vec::new();
    let mut active: std::collections::HashMap<ObjectId, bool> = std::collections::HashMap::new();

    for op in &scene.timeline {
        match op {
            Add { t: op_t, id, .. } if *op_t <= t => {
                if !active.contains_key(id) {
                    first_seen.push(*id);
                }
                active.insert(*id, true);
            }
            Remove { t: op_t, id } if *op_t <= t => {
                active.insert(*id, false);
            }
            _ => {}
        }
    }

    first_seen
        .into_iter()
        .filter(|id| active.get(id).copied().unwrap_or(false))
        .collect()
}

fn latest_add_object<'a>(scene: &'a Scene, id: ObjectId, t: Time) -> Option<&'a Object> {
    use manim_rs_ir::TimelineOp::Add;
    let mut last: Option<&Object> = None;
    for op in &scene.timeline {
        if let Add {
            t: op_t,
            id: op_id,
            object,
        } = op
        {
            if *op_id == id && *op_t <= t {
                last = Some(object);
            }
        }
    }
    last
}

/// Sum the contributions of every position track that references `id`. If no
/// track covers `t`, the object sits at origin; gaps clamp to the last `to`.
fn sum_position_tracks(scene: &Scene, id: ObjectId, t: Time) -> Vec3 {
    let mut out = [0.0_f32; 3];
    for track in &scene.tracks {
        match track {
            Track::Position {
                id: track_id,
                segments,
            } if *track_id == id => {
                let contribution = evaluate_position_track(segments, t);
                out[0] += contribution[0];
                out[1] += contribution[1];
                out[2] += contribution[2];
            }
            _ => {}
        }
    }
    out
}

/// Piecewise evaluation of a sorted, non-overlapping list of position segments.
/// Before every segment: zero. Inside a segment: `lerp(from, to, ease(alpha))`.
/// In a gap or past the last segment: the `to` of the most recently completed
/// segment (i.e. the one whose `t1 <= t`).
fn evaluate_position_track(segments: &[PositionSegment], t: Time) -> Vec3 {
    let mut held: Vec3 = [0.0, 0.0, 0.0];
    for seg in segments {
        if t >= seg.t0 && t <= seg.t1 {
            let alpha = segment_alpha(seg.t0, seg.t1, t);
            let eased = apply_easing(&seg.easing, alpha);
            return lerp_vec3(seg.from, seg.to, eased);
        }
        if seg.t1 < t {
            held = seg.to;
        }
    }
    held
}

// ---------------------------------------------------------------------------
// Opacity / Rotation / Scale / Color — track aggregation.
//
// Each new track type duplicates the per-segment scan from `evaluate_position_track`
// rather than going through a generic helper. Three scalar segment types is
// below the abstraction threshold; an associated-type-or-trait extraction
// would be more code than the duplication it removes.
// ---------------------------------------------------------------------------

fn segment_alpha(t0: Time, t1: Time, t: Time) -> f32 {
    if t1 == t0 {
        1.0
    } else {
        ((t - t0) / (t1 - t0)) as f32
    }
}

fn evaluate_opacity_track(segments: &[OpacitySegment], t: Time) -> Option<f32> {
    let mut held: Option<f32> = None;
    for seg in segments {
        if t >= seg.t0 && t <= seg.t1 {
            let alpha = segment_alpha(seg.t0, seg.t1, t);
            let eased = apply_easing(&seg.easing, alpha);
            return Some(seg.from + (seg.to - seg.from) * eased);
        }
        if seg.t1 < t {
            held = Some(seg.to);
        }
    }
    held
}

fn evaluate_rotation_track(segments: &[RotationSegment], t: Time) -> Option<f32> {
    let mut held: Option<f32> = None;
    for seg in segments {
        if t >= seg.t0 && t <= seg.t1 {
            let alpha = segment_alpha(seg.t0, seg.t1, t);
            let eased = apply_easing(&seg.easing, alpha);
            return Some(seg.from + (seg.to - seg.from) * eased);
        }
        if seg.t1 < t {
            held = Some(seg.to);
        }
    }
    held
}

fn evaluate_scale_track(segments: &[ScaleSegment], t: Time) -> Option<f32> {
    let mut held: Option<f32> = None;
    for seg in segments {
        if t >= seg.t0 && t <= seg.t1 {
            let alpha = segment_alpha(seg.t0, seg.t1, t);
            let eased = apply_easing(&seg.easing, alpha);
            return Some(seg.from + (seg.to - seg.from) * eased);
        }
        if seg.t1 < t {
            held = Some(seg.to);
        }
    }
    held
}

fn evaluate_color_track(segments: &[ColorSegment], t: Time) -> Option<RgbaSrgb> {
    let mut held: Option<RgbaSrgb> = None;
    for seg in segments {
        if t >= seg.t0 && t <= seg.t1 {
            let alpha = segment_alpha(seg.t0, seg.t1, t);
            let eased = apply_easing(&seg.easing, alpha);
            return Some(lerp_rgba(seg.from, seg.to, eased));
        }
        if seg.t1 < t {
            held = Some(seg.to);
        }
    }
    held
}

/// Multiply every active Opacity track's contribution. Default 1.0 when none.
fn product_opacity_tracks(scene: &Scene, id: ObjectId, t: Time) -> f32 {
    let mut out = 1.0_f32;
    for track in &scene.tracks {
        if let Track::Opacity {
            id: track_id,
            segments,
        } = track
        {
            if *track_id == id {
                if let Some(v) = evaluate_opacity_track(segments, t) {
                    out *= v;
                }
            }
        }
    }
    out
}

/// Sum every active Rotation track's contribution (radians). Default 0.0.
fn sum_rotation_tracks(scene: &Scene, id: ObjectId, t: Time) -> f32 {
    let mut out = 0.0_f32;
    for track in &scene.tracks {
        if let Track::Rotation {
            id: track_id,
            segments,
        } = track
        {
            if *track_id == id {
                if let Some(v) = evaluate_rotation_track(segments, t) {
                    out += v;
                }
            }
        }
    }
    out
}

/// Multiply every active Scale track's contribution. Default 1.0.
fn product_scale_tracks(scene: &Scene, id: ObjectId, t: Time) -> f32 {
    let mut out = 1.0_f32;
    for track in &scene.tracks {
        if let Track::Scale {
            id: track_id,
            segments,
        } = track
        {
            if *track_id == id {
                if let Some(v) = evaluate_scale_track(segments, t) {
                    out *= v;
                }
            }
        }
    }
    out
}

/// Override semantics: the last Color track in declaration order with an
/// active or held value at `t` wins. `None` ⇒ no override; rasterizer uses
/// the geometry's authored color.
fn latest_color_track(scene: &Scene, id: ObjectId, t: Time) -> Option<RgbaSrgb> {
    let mut out: Option<RgbaSrgb> = None;
    for track in &scene.tracks {
        if let Track::Color {
            id: track_id,
            segments,
        } = track
        {
            if *track_id == id {
                if let Some(v) = evaluate_color_track(segments, t) {
                    out = Some(v);
                }
            }
        }
    }
    out
}

fn lerp_rgba(a: RgbaSrgb, b: RgbaSrgb, alpha: f32) -> RgbaSrgb {
    [
        a[0] + (b[0] - a[0]) * alpha,
        a[1] + (b[1] - a[1]) * alpha,
        a[2] + (b[2] - a[2]) * alpha,
        a[3] + (b[3] - a[3]) * alpha,
    ]
}

/// All 15 manimgl rate functions.
///
/// Ported from `reference/manimgl/manimlib/utils/rate_functions.py` (Slice C).
/// Formulas are 1:1 with the Python source so that Python-authored easings
/// are pixel-equivalent in Rust.
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

fn lerp_vec3(a: Vec3, b: Vec3, alpha: f32) -> Vec3 {
    [
        a[0] + (b[0] - a[0]) * alpha,
        a[1] + (b[1] - a[1]) * alpha,
        a[2] + (b[2] - a[2]) * alpha,
    ]
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
            stroke: Some(Stroke {
                color: [1.0, 1.0, 1.0, 1.0],
                width: 0.04,
            }),
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
        // Two Color tracks on the same id: declaration order decides the
        // winner (last declared wins). The first track is fully covered by
        // the second's segment, so the second's value is what surfaces.
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
