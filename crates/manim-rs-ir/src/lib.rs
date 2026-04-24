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

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

pub type Time = f64;
pub type ObjectId = u32;
pub type Vec3 = [f32; 3];
pub type RgbaSrgb = [f32; 4];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fill {
    pub color: RgbaSrgb,
}

// ---------------------------------------------------------------------------
// Path verbs for BezPath geometry. Mirrors SVG / lyon `PathEvent`.
// ---------------------------------------------------------------------------

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
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionSegment {
    pub t0: Time,
    pub t1: Time,
    pub from: Vec3,
    pub to: Vec3,
    pub easing: Easing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpacitySegment {
    pub t0: Time,
    pub t1: Time,
    pub from: f32,
    pub to: f32,
    pub easing: Easing,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorSegment {
    pub t0: Time,
    pub t1: Time,
    pub from: RgbaSrgb,
    pub to: RgbaSrgb,
    pub easing: Easing,
}

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
