//! cosmic-text shaping + layout → per-glyph `kurbo::BezPath`.
//!
//! Slice E Step 7 / S7b. Mirrors the Tex pipeline's adapter shape
//! (`crates/manim-rs-tex/src/adapter.rs`): cosmic-text gives us a
//! sequence of laid-out glyphs in y-down "shape" space, swash gives us
//! each glyph's outline in y-up font space, and this module composes the
//! two into Manimax y-up world coordinates with the **first line's
//! baseline at world y = 0**.
//!
//! Why the `OnceLock<Mutex<FontSystem>>` singleton:
//! `FontSystem::new_with_locale_and_db` is fine, but seeding a shared
//! one once amortizes Inter Regular's parse + index across every Text
//! call in the process. Tests are hermetic because we hand in our own
//! `fontdb::Database` containing only Inter — no system-font scan.
//! Slice E §6 gotcha #7 (cosmic-text font db init cost) becomes a
//! one-time, deterministic init.
//!
//! Why we shape at SHAPE_PPEM = 1024:
//! Same reasoning as ADR 0008 §C — extracting glyph outlines via swash
//! at low ppem activates TrueType hinting and snaps control points to
//! the integer grid, which causes visible staircase scallops once the
//! result is rescaled. We ask cosmic-text to lay out at ppem 1024, then
//! post-multiply the entire result by `size / 1024`. The scale is
//! linear and the layout is hinting-immune. cosmic-text's own pixel
//! grid alignment via `LayoutGlyph::physical()` is bypassed; we use the
//! `x_offset` / `y_offset` sub-pixel offsets directly because we render
//! to vectors, not pixels.

use std::sync::{Mutex, OnceLock};

use cosmic_text::fontdb;
use cosmic_text::{Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, Wrap};
use kurbo::{Affine, BezPath};
use swash::FontRef;
use swash::scale::ScaleContext;

use crate::font::default_text_font;
use crate::glyph::glyph_to_bezpath_with_ctx;

/// Internal shaping ppem. Layout runs at this size; the final affine
/// post-multiplies by `size / SHAPE_PPEM` to land at the caller's units.
const SHAPE_PPEM: f32 = 1024.0;

/// Default line-height multiplier. Matches the typographic convention used by
/// most editors and by manimgl's Pango defaults. Surfaced as a knob if a
/// future caller needs to override; not exposed yet.
const LINE_HEIGHT_FACTOR: f32 = 1.2;

/// Family name we register Inter under inside our private `fontdb`. The TTF's
/// own metadata reports "Inter" as the family; we hard-code the same string
/// so resolution is deterministic regardless of fontdb version.
const INTER_FAMILY: &str = "Inter";

/// Public weight enum exposed to callers. Slice E ships only Inter Regular
/// bundled, so `Bold` requires the user to supply font bytes through a
/// future `font=...` parameter (S7c / S7f). Until then `Bold` resolves to
/// the closest match in the registered `fontdb`, which without a real bold
/// face means cosmic-text falls back to synthesized bold or to Regular.
/// Documented as a known coverage gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextWeight {
    Regular,
    Bold,
}

impl From<TextWeight> for Weight {
    fn from(w: TextWeight) -> Self {
        match w {
            TextWeight::Regular => Weight::NORMAL,
            TextWeight::Bold => Weight::BOLD,
        }
    }
}

/// Public alignment enum. Justified is intentionally omitted — Slice E
/// declares justification out of scope (slice plan §4) and exposing it
/// would advertise a feature we don't intend to verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl From<TextAlign> for Align {
    fn from(a: TextAlign) -> Self {
        match a {
            TextAlign::Left => Align::Left,
            TextAlign::Center => Align::Center,
            TextAlign::Right => Align::Right,
        }
    }
}

/// Process-wide cosmic-text engine seeded with Inter Regular. Lazy-init.
fn font_system() -> &'static Mutex<FontSystem> {
    static FS: OnceLock<Mutex<FontSystem>> = OnceLock::new();
    FS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_font_data(default_text_font().to_vec());
        // Empty locale string is fine here — Slice E only ships Latin and
        // we don't rely on locale-driven font fallback.
        let fs = FontSystem::new_with_locale_and_db(String::new(), db);
        Mutex::new(fs)
    })
}

/// Shape `src` with cosmic-text and emit one `kurbo::BezPath` per glyph in
/// Manimax world units. The first line's baseline lands at world y = 0;
/// ascenders sit above (positive y), descenders below (negative y), and
/// subsequent lines stack downward (more negative y).
///
/// `size` is in world units (= em). `weight` and `align` affect shaping +
/// layout but not the output color (which is always `[1.0; 4]` here — the
/// eval-time fan-out applies the parent `Text`'s color override uniformly,
/// mirroring how `compile_tex` ships per-item colors and lets `ObjectState`
/// recolor at render time).
///
/// Returns an empty vec when `src` is empty or whitespace-only.
pub fn text_to_bezpaths(
    src: &str,
    size: f32,
    weight: TextWeight,
    align: TextAlign,
) -> Vec<(BezPath, [f32; 4])> {
    if src.is_empty() {
        return Vec::new();
    }

    let mut fs = font_system().lock().expect("text font system poisoned");

    let metrics = Metrics::new(SHAPE_PPEM, SHAPE_PPEM * LINE_HEIGHT_FACTOR);
    let mut buffer = Buffer::new(&mut fs, metrics);
    // Wrap::None: no wrapping. Slice E §4 declared justification + complex
    // line-breaking out of scope; the user can break lines explicitly with
    // '\n' which cosmic-text honors natively.
    buffer.set_wrap(Wrap::None);
    // Empty bounds with Wrap::None is fine; layout runs unconstrained.
    buffer.set_size(Some(f32::INFINITY), Some(f32::INFINITY));

    let attrs = Attrs::new()
        .family(Family::Name(INTER_FAMILY))
        .weight(weight.into());
    buffer.set_text(src, &attrs, Shaping::Advanced, Some(align.into()));
    buffer.shape_until_scroll(&mut fs, false);

    // Anchor: translate the entire layout so the first line's baseline lands
    // at world y = 0. cosmic-text emits y-down with line_y at the baseline of
    // each line; the first line's line_y equals its max_ascent (ascenders
    // above baseline, baseline below the line top by max_ascent).
    let baseline_anchor = buffer
        .layout_runs()
        .next()
        .map(|run| run.line_y)
        .unwrap_or(0.0);

    let post_scale = Affine::scale(f64::from(size) / f64::from(SHAPE_PPEM));

    let mut out: Vec<(BezPath, [f32; 4])> = Vec::new();
    let mut scale_ctx = ScaleContext::new();

    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            // Resolve the font bytes for this glyph through cosmic-text's
            // own font cache. Today we only have Inter, but going through
            // `fs.get_font` keeps the indirection honest for the future
            // when S7c/S7f add user-supplied fonts.
            let Some(font) = fs.get_font(glyph.font_id, glyph.font_weight) else {
                continue;
            };
            let Some(font_ref) = FontRef::from_index(font.data(), 0) else {
                continue;
            };
            let mut path =
                glyph_to_bezpath_with_ctx(font_ref, &mut scale_ctx, glyph.glyph_id, SHAPE_PPEM);
            if path.is_empty() {
                continue;
            }

            // y-down → y-up flip happens in the translate.
            // pos_x = glyph.x + glyph.font_size * glyph.x_offset (in y-down units)
            // pos_y = glyph.y + run.line_y - glyph.font_size * glyph.y_offset
            //         (cosmic-text's `physical` impl subtracts y_offset, matching the
            //          y-down convention; ours mirrors it.)
            let pos_x = glyph.x + glyph.font_size * glyph.x_offset;
            let pos_y_down =
                glyph.y + run.line_y - glyph.font_size * glyph.y_offset - baseline_anchor;
            let glyph_xform =
                post_scale * Affine::translate((f64::from(pos_x), -f64::from(pos_y_down)));
            path.apply_affine(glyph_xform);

            out.push((path, [1.0, 1.0, 1.0, 1.0]));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    fn render_default(src: &str) -> Vec<(BezPath, [f32; 4])> {
        text_to_bezpaths(src, 1.0, TextWeight::Regular, TextAlign::Left)
    }

    #[test]
    fn empty_string_yields_no_paths() {
        assert!(render_default("").is_empty());
    }

    #[test]
    fn ascii_word_yields_one_path_per_visible_glyph() {
        let paths = render_default("Hello");
        // 5 visible glyphs; cosmic-text may emit a trailing zero-width run
        // for some inputs, so accept a small range rather than `==`.
        assert!(
            paths.len() >= 5,
            "expected at least 5 glyph paths, got {}",
            paths.len()
        );
        for (i, (path, _)) in paths.iter().enumerate() {
            assert!(!path.is_empty(), "glyph {i} produced an empty path");
        }
    }

    #[test]
    fn glyphs_advance_left_to_right() {
        let paths = render_default("abc");
        let xs: Vec<f64> = paths
            .iter()
            .map(|(p, _)| p.bounding_box().min_x())
            .collect();
        for window in xs.windows(2) {
            assert!(
                window[1] > window[0],
                "expected strictly-increasing glyph x positions, got {xs:?}"
            );
        }
    }

    #[test]
    fn newline_starts_a_new_line_below() {
        // Two single-glyph lines — second glyph sits below the first
        // (y-up world; "below" means smaller y).
        let paths = render_default("a\nb");
        assert!(
            paths.len() >= 2,
            "expected at least 2 glyphs, got {}",
            paths.len()
        );
        let first = paths[0].0.bounding_box();
        let second = paths[paths.len() - 1].0.bounding_box();
        assert!(
            second.max_y() < first.min_y(),
            "second-line glyph should sit below first; got first={first:?} second={second:?}"
        );
    }

    #[test]
    fn descender_glyph_extends_below_baseline() {
        // 'g' has a descender. With first-line baseline anchored at y=0,
        // its bounding box should dip below 0.
        let paths = render_default("g");
        assert!(!paths.is_empty(), "expected at least one path for 'g'");
        let bbox = paths[0].0.bounding_box();
        assert!(
            bbox.min_y() < 0.0,
            "'g' should descend below baseline (y < 0); got bbox={bbox:?}"
        );
    }

    #[test]
    fn ascender_glyph_sits_above_baseline() {
        // 'A' has no descender — entire bbox should sit on or above y=0
        // when the first-line baseline is anchored at 0.
        let paths = render_default("A");
        assert!(!paths.is_empty());
        let bbox = paths[0].0.bounding_box();
        // Allow a tiny epsilon for floating-point round-off through the
        // SHAPE_PPEM rescale.
        assert!(
            bbox.min_y() >= -0.01,
            "'A' should not descend below baseline; got bbox={bbox:?}"
        );
        assert!(
            bbox.max_y() > 0.3,
            "'A' should reach above baseline; got bbox={bbox:?}"
        );
    }

    #[test]
    fn size_scales_glyph_extent_linearly() {
        let small = text_to_bezpaths("M", 1.0, TextWeight::Regular, TextAlign::Left);
        let large = text_to_bezpaths("M", 4.0, TextWeight::Regular, TextAlign::Left);
        let small_h = small[0].0.bounding_box().height();
        let large_h = large[0].0.bounding_box().height();
        let ratio = large_h / small_h;
        assert!(
            (ratio - 4.0).abs() < 0.01,
            "expected 4x scaling, got ratio = {ratio} (small={small_h}, large={large_h})"
        );
    }

    #[test]
    fn singleton_is_reusable_across_calls() {
        // Hammers the OnceLock + Mutex pair. If init were not idempotent,
        // a second call would either deadlock or produce different glyph
        // bytes (e.g. font reloaded with different fontdb id).
        let first = render_default("ab");
        let second = render_default("ab");
        assert_eq!(first.len(), second.len());
        for ((p1, _), (p2, _)) in first.iter().zip(second.iter()) {
            assert_eq!(p1.bounding_box(), p2.bounding_box());
        }
    }
}
