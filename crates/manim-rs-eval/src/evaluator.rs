//! Compiled scene evaluator. Turns an authored `Scene` into a fast-to-query
//! representation (timeline of add/remove ops + per-id `TrackBundle`) and
//! evaluates it at any `t`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use manim_rs_ir::{
    ColorSegment, Object, ObjectId, OpacitySegment, PositionSegment, RgbaSrgb, RotationSegment,
    ScaleSegment, Scene, TextAlign, TextWeight, Time, TimelineOp, Track, Vec3,
};

use crate::easing::apply_easing;
use crate::lerp::Lerp;
use crate::state::{ObjectState, SceneState};
use crate::tex::compile_tex;
use crate::text::compile_text;
use crate::tracks::{Segment, evaluate_track, segment_alpha};

/// Pre-Arc'd compiled glyph children indexed by content hash. Each entry is
/// the source-`compile_*` output with its `Object`s wrapped in `Arc` so the
/// per-frame fan-out only does ref-count bumps. Used for both Tex and Text.
type CompileCache = Arc<Mutex<HashMap<blake3::Hash, Arc<Vec<Arc<Object>>>>>>;

/// Runtime-friendly evaluator state compiled once from an authored `Scene`.
#[derive(Debug, Clone)]
pub struct Evaluator {
    timeline: Vec<CompiledTimelineOp>,
    tracks: HashMap<ObjectId, TrackBundle>,
    /// Per-Evaluator cache of compiled Tex outputs. Keyed by content hash so
    /// two scenes with the same Tex source share entries; carried across
    /// frames so a Tex that lives for N frames compiles once.
    tex_cache: CompileCache,
    /// Per-Evaluator cache of compiled Text outputs. Cache key intentionally
    /// excludes per-instance transforms — those apply later at the
    /// `ObjectState` level, not in the cached geometry.
    text_cache: CompileCache,
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
            tex_cache: CompileCache::default(),
            text_cache: CompileCache::default(),
        }
    }

    /// Probe the cache; on miss, run `compile`, Arc-wrap each child, and
    /// insert. The lock is dropped between probe and compile so two threads
    /// racing on the same key both compile — the loser's result is dropped
    /// when re-checking under the second lock. Acceptable until the
    /// renderer goes parallel; replace with `RwLock` or a smarter scheme then.
    fn cached_compile<F>(
        cache: &CompileCache,
        key: blake3::Hash,
        compile: F,
    ) -> Arc<Vec<Arc<Object>>>
    where
        F: FnOnce() -> Vec<Object>,
    {
        if let Some(hit) = cache.lock().unwrap().get(&key).cloned() {
            return hit;
        }
        let compiled: Vec<Arc<Object>> = compile().into_iter().map(Arc::new).collect();
        let arc = Arc::new(compiled);
        Arc::clone(cache.lock().unwrap().entry(key).or_insert(arc))
    }

    /// Look up (or compile and insert) the BezPath children for a Tex IR
    /// node. Key excludes `scale` (applied at fan-out) and `macros` (pre-
    /// expanded by Python). `compile_tex` is expected to succeed because the
    /// Python constructor pre-validates; direct Rust callers must pass valid
    /// sources or accept the panic.
    fn compile_tex_cached(&self, src: &str, color: RgbaSrgb) -> Arc<Vec<Arc<Object>>> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(src.as_bytes());
        for c in color {
            hasher.update(&c.to_le_bytes());
        }
        let key = hasher.finalize();
        Self::cached_compile(&self.tex_cache, key, || {
            compile_tex(src, color).expect("compile_tex on validated source")
        })
    }

    /// Look up (or compile and insert) the BezPath children for a Text IR
    /// node. Key covers every shape-determining input but excludes per-instance
    /// transforms (position / rotation / scale), which apply later at
    /// `ObjectState`.
    fn compile_text_cached(
        &self,
        src: &str,
        font: Option<&str>,
        weight: TextWeight,
        size: f32,
        color: RgbaSrgb,
        align: TextAlign,
    ) -> Arc<Vec<Arc<Object>>> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(src.as_bytes());
        // Tag font None vs Some so an empty Some("") stays distinct from None.
        match font {
            None => hasher.update(&[0u8]),
            Some(f) => {
                hasher.update(&[1u8]);
                hasher.update(f.as_bytes())
            }
        };
        hasher.update(&[match weight {
            TextWeight::Regular => 0,
            TextWeight::Bold => 1,
        }]);
        hasher.update(&size.to_le_bytes());
        for c in color {
            hasher.update(&c.to_le_bytes());
        }
        hasher.update(&[match align {
            TextAlign::Left => 0,
            TextAlign::Center => 1,
            TextAlign::Right => 2,
        }]);
        let key = hasher.finalize();
        Self::cached_compile(&self.text_cache, key, || {
            compile_text(src, font, weight, size, color, align)
        })
    }

    /// Convenience for one-off callers that only have `&Scene`.
    pub fn from_scene(scene: &Scene) -> Self {
        Self::new(scene.clone())
    }

    /// Evaluate the compiled scene at absolute time `t`.
    ///
    /// Tex fan-out (Slice E Step 4): an active `Object::Tex` is replaced
    /// with N `ObjectState`s — one per glyph/decoration BezPath — that
    /// share the parent's resolved transforms (position / rotation /
    /// opacity / color_override) and **multiply** the parent's
    /// track-resolved `scale` by the IR's `Tex.scale`. The rasterizer
    /// therefore never sees `Object::Tex`.
    #[tracing::instrument(level = "trace", name = "eval_at", skip_all, fields(t = t))]
    pub fn eval_at(&self, t: Time) -> SceneState {
        let mut objects = Vec::new();
        for (id, object) in active_objects_at(&self.timeline, t) {
            let bundle = self.tracks.get(&id);
            let position = bundle.map_or([0.0; 3], |b| sum_segments(&b.position, t));
            let opacity = bundle.map_or(1.0, |b| product_scalars(&b.opacity, t));
            let rotation = bundle.map_or(0.0, |b| sum_scalars(&b.rotation, t));
            let scale = bundle.map_or(1.0, |b| product_scalars(&b.scale, t));
            let color_override = bundle.and_then(|b| latest_segments(&b.color, t));

            let make_state = |obj: Arc<Object>, scale: f32| ObjectState {
                id,
                object: obj,
                position,
                opacity,
                rotation,
                scale,
                color_override,
            };

            // Exhaustive match — adding a new `Object` variant forces a
            // decision here between "passthrough" and "fan out", instead of
            // silently defaulting to passthrough.
            match &**object {
                Object::Tex {
                    src,
                    color,
                    scale: tex_scale,
                    ..
                } => {
                    let children = self.compile_tex_cached(src, *color);
                    let combined_scale = scale * *tex_scale;
                    for child in children.iter() {
                        objects.push(make_state(Arc::clone(child), combined_scale));
                    }
                }
                Object::Text {
                    src,
                    font,
                    weight,
                    size,
                    color,
                    align,
                } => {
                    // Text has no IR `scale` field — `size` is baked into
                    // the shaped geometry (cosmic-text adapter post-multiplies
                    // by `size / SHAPE_PPEM`). Track-resolved scale passes
                    // through unchanged.
                    let children = self.compile_text_cached(
                        src,
                        font.as_deref(),
                        *weight,
                        *size,
                        *color,
                        *align,
                    );
                    for child in children.iter() {
                        objects.push(make_state(Arc::clone(child), scale));
                    }
                }
                Object::Polyline { .. } | Object::BezPath { .. } => {
                    objects.push(make_state(Arc::clone(object), scale));
                }
            }
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
