//! Compile an `Object::Tex` IR node into a list of `Object::BezPath`s.
//!
//! Tex is time-invariant: `(src, macros, color, scale)` always produces
//! the same outlines. Slice E Step 3 exposes the compilation as a free
//! function; per-Evaluator caching is added in Step 4 alongside the
//! raster wiring (no point caching what no one calls yet).
//!
//! Color override semantics — Slice E §6 gotcha #10:
//! - Items RaTeX emits with the default black (`Color::BLACK`) take the
//!   IR's top-level `color`. This covers the common `Tex(src, color=BLUE)`
//!   case, where the user's expression is uncolored Tex source.
//! - Items RaTeX colors explicitly via `\textcolor{red}{...}` keep that
//!   color and ignore the IR override. Matches manimgl's behavior.
//!
//! IR scale: `Object::Tex::scale` is **not** baked into the emitted
//! BezPath geometry. It's applied at the eval-time fan-out site by
//! multiplying into the child `ObjectState.scale`, alongside whatever the
//! Track::Scale composition produced. Pre-baking would double-apply with
//! the rasterizer's existing scale handling. The adapter's
//! `WORLD_UNITS_PER_EM` (= 1.0) gives a formula at scale=1.0 a width on
//! the order of one to a few world units.

use kurbo::{BezPath, PathEl};
use manim_rs_ir::{Fill, Object, PathVerb, RgbaSrgb};
use manim_rs_tex::TexError;

/// Compile a Tex source into a list of fill-only `Object::BezPath`s.
///
/// `src` is the post-macro-expansion LaTeX source carried on
/// `Object::Tex`; `color` is the IR-level override applied to RaTeX
/// items left at default-black. Returns the underlying `TexError` on
/// parse failure so the caller can decide between "panic on a contract
/// the constructor was supposed to enforce" and "surface to the user."
pub fn compile_tex(src: &str, color: RgbaSrgb) -> Result<Vec<Object>, TexError> {
    let display_list = manim_rs_tex::tex_to_display_list(src)?;
    let path_color_pairs = manim_rs_tex::display_list_to_bezpath(&display_list);

    Ok(path_color_pairs
        .into_iter()
        .map(|(path, ratex_color)| {
            let resolved = resolve_color(ratex_color, color);
            Object::BezPath {
                verbs: bezpath_to_verbs(&path),
                stroke: None,
                fill: Some(Fill { color: resolved }),
            }
        })
        .collect())
}

/// `\textcolor{...}` ⇒ keep the per-item color. Default-black ⇒ inherit
/// the IR-level override.
fn resolve_color(item: manim_rs_tex::Color, ir_color: RgbaSrgb) -> RgbaSrgb {
    if item == manim_rs_tex::Color::BLACK {
        ir_color
    } else {
        [item.r, item.g, item.b, item.a]
    }
}

fn bezpath_to_verbs(path: &BezPath) -> Vec<PathVerb> {
    path.elements()
        .iter()
        .map(|el| match *el {
            PathEl::MoveTo(p) => PathVerb::MoveTo {
                to: [p.x as f32, p.y as f32, 0.0],
            },
            PathEl::LineTo(p) => PathVerb::LineTo {
                to: [p.x as f32, p.y as f32, 0.0],
            },
            PathEl::QuadTo(c, p) => PathVerb::QuadTo {
                ctrl: [c.x as f32, c.y as f32, 0.0],
                to: [p.x as f32, p.y as f32, 0.0],
            },
            PathEl::CurveTo(c1, c2, p) => PathVerb::CubicTo {
                ctrl1: [c1.x as f32, c1.y as f32, 0.0],
                ctrl2: [c2.x as f32, c2.y as f32, 0.0],
                to: [p.x as f32, p.y as f32, 0.0],
            },
            PathEl::ClosePath => PathVerb::Close {},
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frac_compiles_to_filled_bezpaths() {
        let objs = compile_tex(r"\frac{a}{b}", [1.0, 1.0, 1.0, 1.0]).expect("parses");
        assert!(!objs.is_empty(), "expected at least one BezPath");
        for obj in &objs {
            match obj {
                Object::BezPath {
                    verbs,
                    stroke,
                    fill,
                } => {
                    assert!(stroke.is_none(), "Tex paths are fill-only");
                    assert!(fill.is_some(), "Tex paths must have a fill");
                    assert!(
                        !verbs.is_empty(),
                        "every emitted verb list must be non-empty"
                    );
                }
                _ => panic!("compile_tex emitted non-BezPath object"),
            }
        }
    }

    #[test]
    fn ir_color_overrides_default_black() {
        let red: RgbaSrgb = [1.0, 0.0, 0.0, 1.0];
        let objs = compile_tex(r"x", red).expect("parses");
        assert!(!objs.is_empty());
        for obj in &objs {
            if let Object::BezPath {
                fill: Some(Fill { color }),
                ..
            } = obj
            {
                assert_eq!(*color, red, "uncolored Tex item should pick up IR color");
            }
        }
    }

    #[test]
    fn explicit_textcolor_survives_ir_override() {
        let blue: RgbaSrgb = [0.0, 0.0, 1.0, 1.0];
        // `\textcolor{red}{x} + y`: x is per-item red; y is uncolored ⇒ blue.
        let objs = compile_tex(r"\textcolor{red}{x} + y", blue).expect("parses");
        let red_present = objs.iter().any(|o| {
            matches!(o, Object::BezPath { fill: Some(Fill { color }), .. } if color[0] > 0.5 && color[1] < 0.5 && color[2] < 0.5)
        });
        let blue_present = objs.iter().any(
            |o| matches!(o, Object::BezPath { fill: Some(Fill { color }), .. } if color == &blue),
        );
        assert!(red_present, "\\textcolor{{red}} should produce a red item");
        assert!(blue_present, "uncolored items should adopt IR color (blue)");
    }

    #[test]
    fn parse_error_surfaces_as_err() {
        // Garbage that RaTeX rejects. The exact message is RaTeX-private;
        // we just want to confirm compile_tex doesn't silently produce
        // an empty render on malformed source.
        let result = compile_tex(r"\notacommand{", [1.0; 4]);
        assert!(matches!(result, Err(TexError::Parse { .. })));
    }
}
