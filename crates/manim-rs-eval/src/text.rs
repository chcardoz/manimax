//! Compile an `Object::Text` IR node into a list of `Object::BezPath`s.
//!
//! Mirrors `tex.rs` in shape: time-invariant compilation of source +
//! styling into per-glyph fill-only paths, with caching layered on by
//! `Evaluator`. The split between this module (compilation) and
//! `evaluator.rs` (cache + fan-out) matches the Tex pipeline.
//!
//! Color: `text_to_bezpaths` always emits white (`[1.0; 4]`) — recoloring
//! happens here so the cached result already carries the IR's color, and
//! the eval-time fan-out is a straight ref-count bump. This is simpler
//! than Tex (no `\textcolor`-style per-item override yet); a future
//! markup variant can introduce per-item color the same way Tex does.
//!
//! `font: Option<String>`: today only `None` is honored — Inter Regular
//! bundled. A non-`None` family name is reserved for S7c/S7f and currently
//! ignored by the shaper. We carry it through the cache key so a future
//! shaper that resolves user-supplied fonts won't share entries with the
//! Inter-only path.

use manim_rs_ir::{Fill, Object, RgbaSrgb, TextAlign as IrAlign, TextWeight as IrWeight};
use manim_rs_text::{TextAlign, TextWeight, text_to_bezpaths};

use crate::bezpath::bezpath_to_verbs;

/// Compile a text source + style into a list of fill-only `Object::BezPath`s,
/// each colored to the IR's `color`. `font` is reserved (see module doc).
pub(crate) fn compile_text(
    src: &str,
    _font: Option<&str>,
    weight: IrWeight,
    size: f32,
    color: RgbaSrgb,
    align: IrAlign,
) -> Vec<Object> {
    let path_color_pairs = text_to_bezpaths(src, size, ir_weight(weight), ir_align(align));

    path_color_pairs
        .into_iter()
        .map(|(path, _)| Object::BezPath {
            verbs: bezpath_to_verbs(&path),
            stroke: None,
            fill: Some(Fill { color }),
        })
        .collect()
}

fn ir_weight(w: IrWeight) -> TextWeight {
    match w {
        IrWeight::Regular => TextWeight::Regular,
        IrWeight::Bold => TextWeight::Bold,
    }
}

fn ir_align(a: IrAlign) -> TextAlign {
    match a {
        IrAlign::Left => TextAlign::Left,
        IrAlign::Center => TextAlign::Center,
        IrAlign::Right => TextAlign::Right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_word_compiles_to_filled_bezpaths() {
        let objs = compile_text(
            "Hello",
            None,
            IrWeight::Regular,
            1.0,
            [1.0, 1.0, 1.0, 1.0],
            IrAlign::Left,
        );
        assert!(
            objs.len() >= 5,
            "expected at least one glyph per visible char, got {}",
            objs.len()
        );
        for obj in &objs {
            match obj {
                Object::BezPath {
                    verbs,
                    stroke,
                    fill,
                } => {
                    assert!(stroke.is_none(), "Text paths are fill-only");
                    assert!(fill.is_some(), "Text paths must have a fill");
                    assert!(!verbs.is_empty());
                }
                _ => panic!("compile_text emitted non-BezPath object"),
            }
        }
    }

    #[test]
    fn ir_color_lands_on_every_glyph() {
        let red: RgbaSrgb = [1.0, 0.0, 0.0, 1.0];
        let objs = compile_text("ab", None, IrWeight::Regular, 1.0, red, IrAlign::Left);
        assert!(!objs.is_empty());
        for obj in &objs {
            if let Object::BezPath {
                fill: Some(Fill { color }),
                ..
            } = obj
            {
                assert_eq!(*color, red);
            }
        }
    }

    #[test]
    fn empty_string_yields_no_objects() {
        let objs = compile_text("", None, IrWeight::Regular, 1.0, [1.0; 4], IrAlign::Left);
        assert!(objs.is_empty());
    }
}
