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

use manim_rs_ir::{Easing, Object, ObjectId, PositionSegment, Scene, Time, Track, Vec3};

/// A single object's state at a given time — what the rasterizer draws.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectState {
    pub id: ObjectId,
    pub object: Object,
    pub position: Vec3,
}

/// The whole scene's state at a given time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SceneState {
    pub objects: Vec<ObjectState>,
}

/// Evaluate the scene at absolute time `t`. Clones geometry into the state.
///
/// Slice B assumptions (trusted, not validated):
/// - `scene.timeline` is sorted non-decreasing by `t`.
/// - Every `Track::Position.id` matches some `TimelineOp::Add`.
/// - Segments within a track are non-overlapping.
pub fn eval_at(scene: &Scene, t: Time) -> SceneState {
    let mut objects = Vec::new();
    for id in active_ids_at(scene, t) {
        let object = match latest_add_object(scene, id, t) {
            Some(o) => o.clone(),
            None => continue,
        };
        let position = sum_position_tracks(scene, id, t);
        objects.push(ObjectState { id, object, position });
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

    first_seen.into_iter().filter(|id| active.get(id).copied().unwrap_or(false)).collect()
}

fn latest_add_object<'a>(scene: &'a Scene, id: ObjectId, t: Time) -> Option<&'a Object> {
    use manim_rs_ir::TimelineOp::Add;
    let mut last: Option<&Object> = None;
    for op in &scene.timeline {
        if let Add { t: op_t, id: op_id, object } = op {
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
            Track::Position { id: track_id, segments } if *track_id == id => {
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
            let alpha = if seg.t1 == seg.t0 {
                1.0
            } else {
                ((t - seg.t0) / (seg.t1 - seg.t0)) as f32
            };
            let eased = apply_easing(&seg.easing, alpha);
            return lerp_vec3(seg.from, seg.to, eased);
        }
        if seg.t1 < t {
            held = seg.to;
        }
    }
    held
}

fn apply_easing(easing: &Easing, alpha: f32) -> f32 {
    match easing {
        Easing::Linear {} => alpha,
    }
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
        Resolution, SCHEMA_VERSION, SceneMetadata, TimelineOp,
    };

    fn square_points() -> Vec<Vec3> {
        vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]]
    }

    fn make_scene(timeline: Vec<TimelineOp>, tracks: Vec<Track>, duration: Time) -> Scene {
        Scene {
            metadata: SceneMetadata {
                schema_version: SCHEMA_VERSION,
                fps: 30,
                duration,
                resolution: Resolution { width: 480, height: 270 },
                background: [0.0, 0.0, 0.0, 1.0],
            },
            timeline,
            tracks,
        }
    }

    fn polyline() -> Object {
        Object::Polyline {
            points: square_points(),
            stroke_color: [1.0, 1.0, 1.0, 1.0],
            stroke_width: 0.04,
            closed: true,
        }
    }

    fn slice_b_scene() -> Scene {
        make_scene(
            vec![TimelineOp::Add { t: 0.0, id: 1, object: polyline() }],
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
                TimelineOp::Add { t: 0.0, id: 1, object: polyline() },
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
            vec![TimelineOp::Add { t: 0.0, id: 1, object: polyline() }],
            vec![],
            1.0,
        );
        let state = eval_at(&scene, 0.5);
        assert_eq!(state.objects[0].position, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn not_yet_added_object_absent() {
        let scene = make_scene(
            vec![TimelineOp::Add { t: 1.0, id: 1, object: polyline() }],
            vec![],
            2.0,
        );
        assert_eq!(eval_at(&scene, 0.5).objects.len(), 0);
        assert_eq!(eval_at(&scene, 1.0).objects.len(), 1);
    }

    #[test]
    fn two_parallel_position_tracks_sum() {
        let scene = make_scene(
            vec![TimelineOp::Add { t: 0.0, id: 1, object: polyline() }],
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
            vec![TimelineOp::Add { t: 0.0, id: 1, object: polyline() }],
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
            vec![TimelineOp::Add { t: 0.0, id: 1, object: polyline() }],
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

    #[test]
    fn multiple_add_remove_cycles_track_liveness() {
        // Four alternating ops on the same id — `active_ids_at` must update
        // liveness per op, not latch on the first Add.
        let scene = make_scene(
            vec![
                TimelineOp::Add    { t: 0.0, id: 1, object: polyline() },
                TimelineOp::Remove { t: 1.0, id: 1 },
                TimelineOp::Add    { t: 2.0, id: 1, object: polyline() },
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
