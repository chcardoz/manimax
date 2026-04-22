//! Manimax IR — v1.
//!
//! Spec: `docs/ir-schema.md`. This module is the Rust side of the Python↔Rust
//! contract; `python/manim_rs/ir.py` is the msgspec mirror. Keep them in sync.
//!
//! All structs use `#[serde(deny_unknown_fields)]` so schema drift between the
//! two sides fails loudly at deserialize time rather than silently dropping
//! fields. All enums are internally tagged because msgspec only supports that
//! shape natively.

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Object {
    Polyline {
        points: Vec<Vec3>,
        stroke_color: RgbaSrgb,
        stroke_width: f32,
        closed: bool,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Easing {
    // Empty-struct variant (not unit) so `deny_unknown_fields` actually
    // rejects extra fields. Serde's unit-variant handling under an internal
    // tag ignores extras silently. Wire format is identical.
    Linear {},
}

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
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Track {
    Position {
        id: ObjectId,
        segments: Vec<PositionSegment>,
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
                resolution: Resolution { width: 480, height: 270 },
                background: [0.0, 0.0, 0.0, 1.0],
            },
            timeline: vec![TimelineOp::Add {
                t: 0.0,
                id: 1,
                object: Object::Polyline {
                    points: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]],
                    stroke_color: [1.0, 1.0, 1.0, 1.0],
                    stroke_width: 0.04,
                    closed: true,
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
}
