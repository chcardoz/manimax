//! Compiled scene evaluator. Turns an authored `Scene` into a fast-to-query
//! representation (timeline of add/remove ops + per-id `TrackBundle`) and
//! evaluates it at any `t`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use manim_rs_ir::{
    ColorSegment, Object, ObjectId, OpacitySegment, PositionSegment, RgbaSrgb, RotationSegment,
    ScaleSegment, Scene, Time, TimelineOp, Track, Vec3,
};

use crate::easing::apply_easing;
use crate::lerp::Lerp;
use crate::state::{ObjectState, SceneState};
use crate::tracks::{Segment, evaluate_track, segment_alpha};

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
