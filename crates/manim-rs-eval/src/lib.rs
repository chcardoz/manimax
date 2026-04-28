//! Manimax IR evaluator.
//!
//! `eval_at(scene, t)` returns a `SceneState` — a flat, render-ready snapshot
//! of the scene at time `t`. The function is pure: same input, same output, no
//! internal caching, no history carried across calls. This is what makes
//! random-access frame rendering, parallel chunked rendering, and memoization
//! by `(ir_hash, t)` all possible downstream.
//!
//! Pipeline (top-down view of one `Evaluator::eval_at(t)` call):
//!
//! ```text
//!   Scene ── Evaluator::new ──▶ compiled timeline + indexed tracks
//!                                            │
//!                                            ▼
//!   index_tracks    builds  HashMap<ObjectId, TrackBundle> at compile time
//!   active_objects_at(t)    walks the compiled timeline → live (id, Arc<Object>)s
//!                                            │
//!                                            ▼
//!   for each live object:
//!     evaluate_track(segments, t)            picks the active segment (or held value)
//!     apply_easing(easing, alpha)            maps linear alpha → eased alpha
//!     Lerp::lerp(from, to, eased)            produces the per-property value
//!     compose by kind                        sum / product / override per docs above
//!                                            │
//!                                            ▼
//!                                     SceneState { objects }
//! ```
//!
//! Module map:
//! - [`state`] — `ObjectState` / `SceneState` snapshot types.
//! - [`evaluator`] — `Evaluator`, `eval_at`, timeline + track composition.
//! - [`tracks`] — `Segment` trait + generic `evaluate_track`.
//! - [`lerp`] — `Lerp` trait + the three impls used by tracks.
//! - [`easing`] — `apply_easing` and the rate-function ports from manimgl.
//!
//! Replaces the role of `manimlib/animation/animation.py`'s `interpolate`.
//! Reimplemented, not ported.

mod easing;
mod evaluator;
mod lerp;
mod state;
mod tex;
mod tracks;

pub use evaluator::{Evaluator, eval_at};
pub use state::{ObjectState, SceneState};
pub use tex::compile_tex;

#[cfg(test)]
mod tests {
    use super::*;
    use manim_rs_ir::{
        ColorSegment, Easing, Object, OpacitySegment, PositionSegment, Resolution, RgbaSrgb,
        RotationSegment, SCHEMA_VERSION, ScaleSegment, Scene, SceneMetadata, Stroke, Time,
        TimelineOp, Track, Vec3,
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

    // ----------------------------------------------------------------
    // Slice E Step 4: Tex fan-out at eval time.
    // ----------------------------------------------------------------

    #[test]
    fn tex_eval_at_fans_out_into_bezpath_states() {
        // Eval-time fan-out replaces a single Object::Tex with N
        // ObjectStates, each carrying a fill-only Object::BezPath. The
        // rasterizer never sees Object::Tex.
        use manim_rs_ir::Object;
        use std::collections::BTreeMap;

        let tex_object = Object::Tex {
            src: r"x^2 + y^2 = r^2".to_string(),
            macros: BTreeMap::new(),
            color: [1.0, 1.0, 1.0, 1.0],
            scale: 1.0,
        };
        let scene = make_scene(
            vec![TimelineOp::Add {
                t: 0.0,
                id: 1,
                object: tex_object,
            }],
            vec![],
            1.0,
        );

        let state = eval_at(&scene, 0.0);
        assert!(
            state.objects.len() > 1,
            "Tex must fan out into multiple BezPath states (got {})",
            state.objects.len()
        );
        for s in &state.objects {
            assert_eq!(s.id, 1, "fan-out children share the parent ObjectId");
            match &*s.object {
                Object::BezPath {
                    verbs,
                    stroke,
                    fill,
                } => {
                    assert!(!verbs.is_empty());
                    assert!(stroke.is_none());
                    assert!(fill.is_some());
                }
                Object::Tex { .. } => panic!("Tex must not survive eval"),
                _ => panic!("fan-out must emit only Object::BezPath"),
            }
        }
    }

    #[test]
    fn tex_scale_multiplies_into_object_state_scale() {
        // Tex.scale is applied at fan-out, multiplied with the parent
        // ObjectState.scale (which the Track::Scale composition already
        // resolved). Tex.scale=4 with no Scale track ⇒ child.scale=4.
        use manim_rs_ir::Object;
        use std::collections::BTreeMap;

        let scene = make_scene(
            vec![TimelineOp::Add {
                t: 0.0,
                id: 1,
                object: Object::Tex {
                    src: r"x".to_string(),
                    macros: BTreeMap::new(),
                    color: [1.0, 1.0, 1.0, 1.0],
                    scale: 4.0,
                },
            }],
            vec![],
            1.0,
        );

        let state = eval_at(&scene, 0.0);
        assert!(!state.objects.is_empty());
        for s in &state.objects {
            assert_eq!(
                s.scale, 4.0,
                "Tex.scale=4 must surface as ObjectState.scale=4"
            );
        }
    }

    #[test]
    fn tex_scale_composes_with_scale_track() {
        // Tex.scale=2 plus a Scale track of constant 3 ⇒ child.scale = 6
        // (the tracked scale × the IR scale). This is the composition
        // that motivated decision (b): one apply site, no double-bake.
        use manim_rs_ir::Object;
        use std::collections::BTreeMap;

        let scene = make_scene(
            vec![TimelineOp::Add {
                t: 0.0,
                id: 1,
                object: Object::Tex {
                    src: r"x".to_string(),
                    macros: BTreeMap::new(),
                    color: [1.0, 1.0, 1.0, 1.0],
                    scale: 2.0,
                },
            }],
            vec![Track::Scale {
                id: 1,
                segments: vec![ScaleSegment {
                    t0: 0.0,
                    t1: 1.0,
                    from: 3.0,
                    to: 3.0,
                    easing: Easing::Linear {},
                }],
            }],
            1.0,
        );

        let state = eval_at(&scene, 0.5);
        assert!(!state.objects.is_empty());
        for s in &state.objects {
            assert!(
                (s.scale - 6.0).abs() < 1e-6,
                "expected 2.0 * 3.0 = 6.0, got {}",
                s.scale
            );
        }
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
