//! Manimax IR — v1.
//!
//! Spec: `docs/ir-schema.md`. This module is the Rust side of the Python↔Rust
//! contract; `python/manim_rs/ir.py` is the msgspec mirror. Keep them in sync.
//!
//! All structs use `#[serde(deny_unknown_fields)]` so schema drift between the
//! two sides fails loudly at deserialize time rather than silently dropping
//! fields. All enums are internally tagged because msgspec only supports that
//! shape natively.
//!
//! Optionality note: `stroke`/`fill` on geometry are `Option<T>` because a
//! shape can have either, both, or neither. On the wire they are required
//! fields whose value may be `null`. This matches the "all fields required"
//! principle while expressing absence cleanly.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Wire-format version. Bump when the schema changes shape; the Python
/// mirror's `SCHEMA_VERSION` must move in lockstep.
///
/// - v1 (Slice B–D): `Polyline` + `BezPath` objects.
/// - v2 (Slice E Step 3): adds `Object::Tex` carrying source string +
///   user-supplied macros, expanded to BezPaths at eval time.
pub const SCHEMA_VERSION: u32 = 2;

/// Scene-time in seconds. `f64` matches msgspec's number type.
pub type Time = f64;
/// Stable per-scene id assigned by the Python frontend.
pub type ObjectId = u32;
/// Position / control-point coordinate in scene units.
pub type Vec3 = [f32; 3];
/// sRGB color with straight (non-premultiplied) alpha. Components in `[0, 1]`.
pub type RgbaSrgb = [f32; 4];

/// Output frame dimensions in pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// Scene-level invariants needed by the runtime before stepping any track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneMetadata {
    pub schema_version: u32,
    pub fps: u32,
    pub duration: Time,
    pub resolution: Resolution,
    pub background: RgbaSrgb,
}

// ---------------------------------------------------------------------------
// Stroke / Fill — shared by every geometry variant.
// ---------------------------------------------------------------------------

/// Stroke width is either a single scalar (uniform across the stroke) or a
/// per-vertex list. The wire format is untagged: a bare number or a JSON
/// array. Both shapes round-trip cleanly through serde + msgspec.
///
/// Per-vertex length must equal `points.len()` for `Polyline` or
/// `segment_count + 1` for `BezPath` (the per-segment endpoint count produced
/// by `sample_bezpath`). The rasterizer falls back to the first element if
/// the length check fails, so an IR-level invariant breach degrades to
/// uniform-width rather than panicking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StrokeWidth {
    Scalar(f32),
    PerVertex(Vec<f32>),
}

impl From<f32> for StrokeWidth {
    fn from(v: f32) -> Self {
        Self::Scalar(v)
    }
}

/// Joint strategy for corners between consecutive stroke segments. Serializes
/// as a lowercase string (`"miter"` / `"bevel"` / `"auto"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JointKind {
    Miter,
    Bevel,
    #[default]
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub color: RgbaSrgb,
    /// Scene units, not pixels. Camera is hardcoded at `[-8, 8] × [-4.5, 4.5]`.
    pub width: StrokeWidth,
    /// Corner strategy. `Auto` matches manimgl's cosine threshold.
    #[serde(default)]
    pub joint: JointKind,
}

impl Stroke {
    /// Convenience for call-sites that just want a uniform-width, auto-joint
    /// stroke — keeps test fixtures terse.
    pub fn solid(color: RgbaSrgb, width: f32) -> Self {
        Self {
            color,
            width: StrokeWidth::Scalar(width),
            joint: JointKind::Auto,
        }
    }
}

/// Solid interior fill. Currently color-only; gradients are out of scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fill {
    pub color: RgbaSrgb,
}

// ---------------------------------------------------------------------------
// Path verbs for BezPath geometry. Mirrors SVG / lyon `PathEvent`.
// ---------------------------------------------------------------------------

/// Single verb in a Bézier path. Mirrors SVG / lyon `PathEvent`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum PathVerb {
    MoveTo { to: Vec3 },
    LineTo { to: Vec3 },
    QuadTo { ctrl: Vec3, to: Vec3 },
    CubicTo { ctrl1: Vec3, ctrl2: Vec3, to: Vec3 },
    Close {},
}

// ---------------------------------------------------------------------------
// Object — every geometry can be stroked, filled, or both (null ⇒ absent).
// ---------------------------------------------------------------------------

/// A renderable shape. Every variant can carry a stroke, a fill, both, or
/// neither (`null` on the wire ⇒ `None`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Object {
    Polyline {
        points: Vec<Vec3>,
        closed: bool,
        stroke: Option<Stroke>,
        fill: Option<Fill>,
    },
    BezPath {
        verbs: Vec<PathVerb>,
        stroke: Option<Stroke>,
        fill: Option<Fill>,
    },
    /// LaTeX-flavored math source. The Rust eval layer compiles this once
    /// to a list of filled `BezPath`s via `manim-rs-tex`. Time-invariant:
    /// the same `(src, macros, color, scale)` always produces the same
    /// outlines, so the cache key is content-only.
    ///
    /// Macro expansion is **Python-side** — by the time a `Tex` reaches
    /// Rust, `src` is already-expanded source. `macros` rides through
    /// for cache-key stability and roundtrip fidelity, not because Rust
    /// re-expands.
    ///
    /// `BTreeMap` (not `HashMap`) for canonical key ordering: an
    /// insertion-order map would invalidate the cache on cosmetic Python
    /// dict reordering. Slice D §5 / Slice E §6 gotcha #4.
    Tex {
        src: String,
        macros: BTreeMap<String, String>,
        /// Default color applied to items RaTeX emits in plain black
        /// (i.e. anything not explicitly `\textcolor{...}`'d). Per-item
        /// colors from the source ride through unchanged. Slice E §6
        /// gotcha #10.
        color: RgbaSrgb,
        /// Multiplier applied on top of the adapter's
        /// `WORLD_UNITS_PER_EM`. `1.0` is identity.
        scale: f32,
    },
}

/// Add-or-remove event keyed by `(t, id)`. The evaluator replays these to
/// decide which objects are alive at any time `t`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum TimelineOp {
    Add {
        t: Time,
        id: ObjectId,
        object: Object,
    },
    Remove {
        t: Time,
        id: ObjectId,
    },
}

// ---------------------------------------------------------------------------
// Easing. All 15 manimgl rate functions. Two are recursive combinators
// (`NotQuiteThere`, `SquishRateFunc`) wrapping an inner easing.
// ---------------------------------------------------------------------------

/// All 15 manimgl rate functions. `NotQuiteThere` and `SquishRateFunc` are
/// recursive combinators wrapping an inner easing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Easing {
    // Empty-struct variants (not unit) so `deny_unknown_fields` actually
    // rejects extra fields. Serde's unit-variant handling under an internal
    // tag ignores extras silently. Wire format is identical.
    Linear {},
    Smooth {},
    RushInto {},
    RushFrom {},
    SlowInto {},
    DoubleSmooth {},
    ThereAndBack {},
    Lingering {},
    ThereAndBackWithPause { pause_ratio: f32 },
    RunningStart { pull_factor: f32 },
    Overshoot { pull_factor: f32 },
    Wiggle { wiggles: f32 },
    ExponentialDecay { half_life: f32 },
    NotQuiteThere { inner: Box<Easing>, proportion: f32 },
    SquishRateFunc { inner: Box<Easing>, a: f32, b: f32 },
}

// ---------------------------------------------------------------------------
// Track segments — one shape per value type. `t0/t1/easing` are common.
// ---------------------------------------------------------------------------

/// `from → to` over `[t0, t1]` for object position, eased by `easing`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionSegment {
    pub t0: Time,
    pub t1: Time,
    pub from: Vec3,
    pub to: Vec3,
    pub easing: Easing,
}

/// `from → to` over `[t0, t1]` for object opacity in `[0, 1]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpacitySegment {
    pub t0: Time,
    pub t1: Time,
    pub from: f32,
    pub to: f32,
    pub easing: Easing,
}

/// `from → to` over `[t0, t1]` for rotation, in radians.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotationSegment {
    pub t0: Time,
    pub t1: Time,
    /// Radians. Matches manimgl/numpy convention.
    pub from: f32,
    pub to: f32,
    pub easing: Easing,
}

/// `from → to` over `[t0, t1]` for uniform scale. `1.0` is identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleSegment {
    pub t0: Time,
    pub t1: Time,
    /// Uniform scale factor. `1.0` is identity. Per-axis scale deferred.
    pub from: f32,
    pub to: f32,
    pub easing: Easing,
}

/// `from → to` over `[t0, t1]` for stroke/fill color, eased componentwise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorSegment {
    pub t0: Time,
    pub t1: Time,
    pub from: RgbaSrgb,
    pub to: RgbaSrgb,
    pub easing: Easing,
}

/// All segments for one property of one object. Each variant is a separate
/// list so the evaluator can pick a typed segment slice without dispatch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Track {
    Position {
        id: ObjectId,
        segments: Vec<PositionSegment>,
    },
    Opacity {
        id: ObjectId,
        segments: Vec<OpacitySegment>,
    },
    Rotation {
        id: ObjectId,
        segments: Vec<RotationSegment>,
    },
    Scale {
        id: ObjectId,
        segments: Vec<ScaleSegment>,
    },
    Color {
        id: ObjectId,
        segments: Vec<ColorSegment>,
    },
}

/// Top-level IR document: scene metadata, the add/remove timeline, and all
/// per-property tracks. Round-trips byte-for-byte through serde + msgspec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub metadata: SceneMetadata,
    pub timeline: Vec<TimelineOp>,
    pub tracks: Vec<Track>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_scene() -> Scene {
        Scene {
            metadata: SceneMetadata {
                schema_version: SCHEMA_VERSION,
                fps: 30,
                duration: 2.0,
                resolution: Resolution {
                    width: 480,
                    height: 270,
                },
                background: [0.0, 0.0, 0.0, 1.0],
            },
            timeline: vec![TimelineOp::Add {
                t: 0.0,
                id: 1,
                object: Object::Polyline {
                    points: vec![
                        [-1.0, -1.0, 0.0],
                        [1.0, -1.0, 0.0],
                        [1.0, 1.0, 0.0],
                        [-1.0, 1.0, 0.0],
                    ],
                    closed: true,
                    stroke: Some(Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.04)),
                    fill: None,
                },
            }],
            tracks: vec![Track::Position {
                id: 1,
                segments: vec![PositionSegment {
                    t0: 0.0,
                    t1: 2.0,
                    from: [0.0, 0.0, 0.0],
                    to: [2.0, 0.0, 0.0],
                    easing: Easing::Linear {},
                }],
            }],
        }
    }

    #[test]
    fn roundtrip_preserves_scene() {
        let s = sample_scene();
        let json = serde_json::to_string(&s).expect("serialize");
        let back: Scene = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(s, back);
    }

    #[test]
    fn unknown_fields_rejected() {
        let bad = r#"{"schema_version":1,"fps":30,"duration":2.0,
            "resolution":{"width":480,"height":270},
            "background":[0,0,0,1],"extra":"nope"}"#;
        let err = serde_json::from_str::<SceneMetadata>(bad);
        assert!(err.is_err(), "deny_unknown_fields must reject `extra`");
    }

    #[test]
    fn timeline_op_uses_internal_tag() {
        let op = TimelineOp::Remove { t: 1.5, id: 7 };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains(r#""op":"Remove""#), "got {json}");
        assert!(json.contains(r#""id":7"#));
    }

    #[test]
    fn bezpath_roundtrips() {
        let obj = Object::BezPath {
            verbs: vec![
                PathVerb::MoveTo {
                    to: [0.0, 0.0, 0.0],
                },
                PathVerb::LineTo {
                    to: [1.0, 0.0, 0.0],
                },
                PathVerb::QuadTo {
                    ctrl: [1.0, 1.0, 0.0],
                    to: [0.0, 1.0, 0.0],
                },
                PathVerb::CubicTo {
                    ctrl1: [-1.0, 1.0, 0.0],
                    ctrl2: [-1.0, 0.0, 0.0],
                    to: [0.0, 0.0, 0.0],
                },
                PathVerb::Close {},
            ],
            stroke: Some(Stroke::solid([0.0, 1.0, 0.0, 1.0], 0.05)),
            fill: Some(Fill {
                color: [0.2, 0.2, 0.8, 0.5],
            }),
        };
        let json = serde_json::to_string(&obj).unwrap();
        let back: Object = serde_json::from_str(&json).unwrap();
        assert_eq!(obj, back);
    }

    #[test]
    fn tex_object_roundtrips() {
        let mut macros = BTreeMap::new();
        macros.insert("RR".to_string(), r"\mathbb{R}".to_string());
        macros.insert("NN".to_string(), r"\mathbb{N}".to_string());
        let obj = Object::Tex {
            src: r"\sum_{i=1}^n i = \frac{n(n+1)}{2}".to_string(),
            macros,
            color: [1.0, 1.0, 1.0, 1.0],
            scale: 2.5,
        };
        let json = serde_json::to_string(&obj).unwrap();
        assert!(json.contains(r#""kind":"Tex""#), "got {json}");
        // BTreeMap serializes with sorted keys — cosmetic Python dict
        // reordering must not change cache identity.
        let nn_pos = json.find(r#""NN""#).unwrap();
        let rr_pos = json.find(r#""RR""#).unwrap();
        assert!(nn_pos < rr_pos, "macros not key-sorted in {json}");
        let back: Object = serde_json::from_str(&json).unwrap();
        assert_eq!(obj, back);
    }

    #[test]
    fn schema_version_is_two() {
        // Wire-format guard: bumping forgets to update the Python mirror
        // unless someone reads this test failure first.
        assert_eq!(SCHEMA_VERSION, 2);
    }

    #[test]
    fn stroke_width_scalar_serializes_as_number() {
        let s = Stroke::solid([1.0, 1.0, 1.0, 1.0], 0.04);
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""width":0.04"#), "got {json}");
        let back: Stroke = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn stroke_width_per_vertex_serializes_as_array() {
        let s = Stroke {
            color: [1.0, 0.0, 0.0, 1.0],
            width: StrokeWidth::PerVertex(vec![0.02, 0.08, 0.02]),
            joint: JointKind::Miter,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""width":[0.02,0.08,0.02]"#), "got {json}");
        assert!(json.contains(r#""joint":"miter""#), "got {json}");
        let back: Stroke = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn stroke_joint_defaults_to_auto_when_absent() {
        let json = r#"{"color":[1,1,1,1],"width":0.04}"#;
        let s: Stroke = serde_json::from_str(json).unwrap();
        assert_eq!(s.joint, JointKind::Auto);
    }

    #[test]
    fn recursive_easing_roundtrips() {
        let e = Easing::SquishRateFunc {
            inner: Box::new(Easing::NotQuiteThere {
                inner: Box::new(Easing::Smooth {}),
                proportion: 0.7,
            }),
            a: 0.2,
            b: 0.8,
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: Easing = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn every_easing_serializes_with_tag() {
        let cases = [
            Easing::Linear {},
            Easing::Smooth {},
            Easing::RushInto {},
            Easing::RushFrom {},
            Easing::SlowInto {},
            Easing::DoubleSmooth {},
            Easing::ThereAndBack {},
            Easing::Lingering {},
            Easing::ThereAndBackWithPause { pause_ratio: 0.33 },
            Easing::RunningStart { pull_factor: -0.5 },
            Easing::Overshoot { pull_factor: 1.5 },
            Easing::Wiggle { wiggles: 2.0 },
            Easing::ExponentialDecay { half_life: 0.1 },
            Easing::NotQuiteThere {
                inner: Box::new(Easing::Smooth {}),
                proportion: 0.7,
            },
            Easing::SquishRateFunc {
                inner: Box::new(Easing::Linear {}),
                a: 0.4,
                b: 0.6,
            },
        ];
        assert_eq!(cases.len(), 15, "all 15 manimgl rate functions present");
        for e in cases {
            let json = serde_json::to_string(&e).unwrap();
            assert!(json.contains(r#""kind""#), "missing tag in {json}");
            let back: Easing = serde_json::from_str(&json).unwrap();
            assert_eq!(e, back);
        }
    }

    #[test]
    fn every_track_variant_roundtrips() {
        let common_easing = Easing::Linear {};
        let cases = vec![
            Track::Position {
                id: 1,
                segments: vec![PositionSegment {
                    t0: 0.0,
                    t1: 1.0,
                    from: [0.0; 3],
                    to: [1.0, 0.0, 0.0],
                    easing: common_easing.clone(),
                }],
            },
            Track::Opacity {
                id: 1,
                segments: vec![OpacitySegment {
                    t0: 0.0,
                    t1: 1.0,
                    from: 0.0,
                    to: 1.0,
                    easing: common_easing.clone(),
                }],
            },
            Track::Rotation {
                id: 1,
                segments: vec![RotationSegment {
                    t0: 0.0,
                    t1: 1.0,
                    from: 0.0,
                    to: std::f32::consts::PI,
                    easing: common_easing.clone(),
                }],
            },
            Track::Scale {
                id: 1,
                segments: vec![ScaleSegment {
                    t0: 0.0,
                    t1: 1.0,
                    from: 1.0,
                    to: 2.0,
                    easing: common_easing.clone(),
                }],
            },
            Track::Color {
                id: 1,
                segments: vec![ColorSegment {
                    t0: 0.0,
                    t1: 1.0,
                    from: [1.0, 0.0, 0.0, 1.0],
                    to: [0.0, 0.0, 1.0, 1.0],
                    easing: common_easing,
                }],
            },
        ];
        for t in cases {
            let json = serde_json::to_string(&t).unwrap();
            let back: Track = serde_json::from_str(&json).unwrap();
            assert_eq!(t, back);
        }
    }
}
