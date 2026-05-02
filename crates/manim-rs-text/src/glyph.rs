//! Glyph outline → `kurbo::BezPath`.
//!
//! `swash` produces an `Outline` of zeno verbs/points in pixel space at the
//! requested ppem, in **y-up** coordinates (positive y = above baseline,
//! matching TrueType's font-design convention). Manimax world space is
//! also y-up, so glyph outlines pass through unflipped here.
//!
//! The y-flip the slice plan referenced is in Step 2's RaTeX adapter:
//! RaTeX's `DisplayList` lays out glyphs in y-down page space, and the
//! adapter negates that y at translation time. Glyph-internal outlines
//! never get flipped twice.
//!
//! For multi-layer color glyphs (`COLR`-table) we currently flatten by
//! concatenating layer paths. KaTeX TTFs and Inter are monochrome, so this
//! is academic, but the behavior is documented in case a future font isn't.

use kurbo::{Affine, BezPath, Point as KPoint};
use swash::FontRef;
use swash::scale::{ScaleContext, outline::Outline};
use swash::zeno::{self, Verb};

/// Internal ppem for outline extraction. Glyph outlines are produced at this
/// resolution (effectively hinting-off, since hinting only kicks in at small
/// ppem) and the resulting coordinates are scaled down to the requested
/// `scale` afterwards via a uniform affine. This keeps the kurbo path
/// resolution-independent regardless of how big a `Tex.scale` blows it up
/// later — without it, ppem≈1 (which is what "1 em = 1 world unit" implies)
/// snaps coordinates to an integer pixel grid and the scaled-up outline
/// shows visible staircase scallops.
const OUTLINE_PPEM: f32 = 1024.0;

/// Build a `kurbo::BezPath` for the glyph mapped to `char_code`, scaled to
/// `scale` pixels-per-em, in Manimax y-up coordinates.
///
/// Returns an empty `BezPath` when:
/// - the font bytes don't parse as a usable TTF/OTF, or
/// - the scaler produces no outline (e.g. blank glyphs like `' '`).
///
/// If the codepoint has no charmap entry the font's `.notdef` glyph is
/// outlined instead — making missing-glyph rendering visible rather than
/// silently empty, which matches how typical text engines behave.
pub fn glyph_to_bezpath(font: &[u8], char_code: u32, scale: f32) -> BezPath {
    let Some(font_ref) = FontRef::from_index(font, 0) else {
        return BezPath::new();
    };
    let glyph_id = font_ref.charmap().map(char_code);
    let mut ctx = ScaleContext::new();
    glyph_to_bezpath_with_ctx(font_ref, &mut ctx, glyph_id, scale)
}

/// Like [`glyph_to_bezpath`] but reuses a caller-owned `ScaleContext` and a
/// pre-parsed `FontRef`. Hot paths shaping many glyphs in a row should hold
/// one `ScaleContext` for the whole batch — `ScaleContext::new()` allocates
/// scratch buffers, and swash's design intent is one-per-thread reuse.
pub(crate) fn glyph_to_bezpath_with_ctx(
    font_ref: FontRef<'_>,
    ctx: &mut ScaleContext,
    glyph_id: u16,
    scale: f32,
) -> BezPath {
    let mut scaler = ctx.builder(font_ref).size(OUTLINE_PPEM).build();
    let Some(outline) = scaler.scale_outline(glyph_id) else {
        return BezPath::new();
    };

    let mut path = outline_to_bezpath(&outline);
    // Scale from OUTLINE_PPEM down to the caller's requested ppem. Doing
    // this as a post-process affine keeps the high-resolution outline
    // smooth and ensures the result composes with downstream MVP scaling
    // without re-introducing hinting artifacts.
    let factor = f64::from(scale) / f64::from(OUTLINE_PPEM);
    path.apply_affine(Affine::scale(factor));
    path
}

fn outline_to_bezpath(outline: &Outline) -> BezPath {
    let mut path = BezPath::new();

    if outline.is_empty() {
        // Mono fonts: swash's `Outline` reports `len() == 0` but the
        // top-level points/verbs slices hold the full path. Walk those.
        push_verbs(&mut path, outline.points(), outline.verbs());
    } else {
        for i in 0..outline.len() {
            if let Some(layer) = outline.get(i) {
                push_verbs(&mut path, layer.points(), layer.verbs());
            }
        }
    }

    path
}

fn push_verbs(path: &mut BezPath, points: &[zeno::Point], verbs: &[Verb]) {
    let mut i = 0;
    let to_kurbo = |p: zeno::Point| KPoint::new(p.x as f64, p.y as f64);
    for verb in verbs {
        match verb {
            Verb::MoveTo => {
                let p = points[i];
                i += 1;
                path.move_to(to_kurbo(p));
            }
            Verb::LineTo => {
                let p = points[i];
                i += 1;
                path.line_to(to_kurbo(p));
            }
            Verb::QuadTo => {
                let p1 = points[i];
                let p2 = points[i + 1];
                i += 2;
                path.quad_to(to_kurbo(p1), to_kurbo(p2));
            }
            Verb::CurveTo => {
                let p1 = points[i];
                let p2 = points[i + 1];
                let p3 = points[i + 2];
                i += 3;
                path.curve_to(to_kurbo(p1), to_kurbo(p2), to_kurbo(p3));
            }
            Verb::Close => {
                path.close_path();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{default_text_font, katex_font};
    use kurbo::Shape;

    fn assert_glyph_renders(font: &[u8], ch: char, label: &str) {
        let path = glyph_to_bezpath(font, ch as u32, 64.0);
        assert!(
            !path.elements().is_empty(),
            "{label}: glyph {ch:?} produced an empty path"
        );
        let bbox = path.bounding_box();
        assert!(
            bbox.width() > 1.0 && bbox.height() > 1.0,
            "{label}: glyph {ch:?} bbox is degenerate ({bbox:?})"
        );
    }

    #[test]
    fn inter_outlines_an_uppercase_a() {
        assert_glyph_renders(default_text_font(), 'A', "Inter");
    }

    #[test]
    fn katex_main_regular_outlines_a_letter() {
        let bytes = katex_font("Main-Regular").expect("Main-Regular bundled");
        assert_glyph_renders(bytes, 'A', "KaTeX Main-Regular");
    }

    #[test]
    fn katex_math_italic_outlines_x() {
        let bytes = katex_font("Math-Italic").expect("Math-Italic bundled");
        assert_glyph_renders(bytes, 'x', "KaTeX Math-Italic");
    }

    /// 'A' has no descender, so its outline should sit on or above the
    /// baseline (y >= 0 in y-up world). A regression that re-introduces a
    /// y-flip would push 'A' below baseline and trip this.
    #[test]
    fn uppercase_a_sits_above_baseline() {
        let path = glyph_to_bezpath(default_text_font(), 'A' as u32, 64.0);
        let bbox = path.bounding_box();
        assert!(
            bbox.min_y() >= -2.0,
            "uppercase A unexpectedly descends below baseline: bbox.min_y = {}",
            bbox.min_y()
        );
        assert!(
            bbox.max_y() > 30.0,
            "uppercase A bbox didn't extend above baseline: bbox.max_y = {} (ppem = 64)",
            bbox.max_y()
        );
    }

    /// 'g' has a descender — bbox should extend below the baseline.
    /// Belt-and-braces against a hidden flip elsewhere.
    #[test]
    fn lowercase_g_descends_below_baseline() {
        let path = glyph_to_bezpath(default_text_font(), 'g' as u32, 64.0);
        let bbox = path.bounding_box();
        assert!(
            bbox.min_y() < -1.0,
            "lowercase g should have a descender: bbox.min_y = {}",
            bbox.min_y()
        );
    }
}
