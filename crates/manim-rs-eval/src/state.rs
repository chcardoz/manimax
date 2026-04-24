//! Per-frame snapshot types — the values passed from the evaluator to the
//! rasterizer. Kept in their own module so the evaluator's logic file stays
//! focused on time-varying composition.

use std::sync::Arc;

use manim_rs_ir::{Object, ObjectId, RgbaSrgb, Vec3};
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
