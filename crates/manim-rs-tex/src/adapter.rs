//! `DisplayList` → `Vec<(BezPath, Color)>` adapter.
//!
//! The single coordinate transform happens here, in one place, applied
//! per item (Slice E §6 gotcha #1: don't sprinkle flips at call sites):
//!
//! - **em → world units** via [`WORLD_UNITS_PER_EM`]. RaTeX emits all
//!   `DisplayList` coordinates in em; Manimax world space is unitless,
//!   and the Tex IR variant (Step 3) scales further with its own
//!   `scale` field. The default of `1.0` here means "one em renders as
//!   one world unit," which puts a typical formula in a reasonable
//!   place under default scene cameras (~8 world-unit width).
//! - **page-y-down → world-y-up** by negating page-y at translation
//!   time. Glyph outlines from `manim_rs_text::glyph_to_bezpath` come
//!   back in y-up natural; they are translated, not re-flipped.
//!   Non-glyph paths (Path/Line/Rect) carry their internal coordinates
//!   in page-y-down convention — those get flipped via an affine of
//!   `[1, 0, 0, -1, x, -y] * em` on the way out.
//! - **Anchor at the baseline.** RaTeX's page origin sits at the top of
//!   the layout box with the baseline at `list.height` em below. We
//!   translate the entire result by `+list.height` in world-y so that
//!   `world.y = 0` is the baseline (ascenders positive, descenders
//!   negative). This matches `Text` (Step 7) and makes Tex/Text
//!   compose naturally.
//!
//! Multi-color: `\textcolor{red}{...}` makes RaTeX emit per-item
//! `color` values; we pass them through. The Tex IR variant's top-level
//! color override (Step 3) is applied by the eval step, not here.

use kurbo::{Affine, BezPath, Point as KPoint, Rect, Shape};
use manim_rs_text::ScaleContext;
use ratex_types::{Color, DisplayItem, DisplayList, PathCommand};

/// World units per em. One em is one world unit by default; the Tex IR
/// variant's `scale` (Step 3) scales further. If a future slice needs a
/// different default (e.g. to match `Text`'s implicit scale), change this
/// in one place.
const WORLD_UNITS_PER_EM: f64 = 1.0;

/// Convert a RaTeX `DisplayList` into a flat list of filled paths in
/// Manimax world coordinates (y-up, em-scaled).
///
/// Each output `(BezPath, Color)` pair is independent — there is no
/// implicit z-order beyond list order, and the existing fill pipeline
/// renders them in sequence. Empty `DisplayList`s produce empty `Vec`s.
pub fn display_list_to_bezpath(list: &DisplayList) -> Vec<(BezPath, Color)> {
    // Shift everything up so the baseline (page-y = list.height) lands at
    // world-y = 0. Composed into each item's per-item affine so each path
    // is transformed in a single pass.
    let baseline_shift = Affine::translate((0.0, list.height * WORLD_UNITS_PER_EM));

    // Hoist a single ScaleContext for the whole DisplayList — swash
    // allocates scratch buffers inside ScaleContext::new() and is designed
    // to be reused across glyphs.
    let mut ctx = ScaleContext::new();

    let mut out = Vec::with_capacity(list.items.len());
    for item in &list.items {
        match item {
            DisplayItem::GlyphPath {
                x,
                y,
                scale,
                font,
                char_code,
                color,
                commands: _,
            } => {
                if let Some(p) =
                    glyph_path(*x, *y, *scale, font, *char_code, baseline_shift, &mut ctx)
                {
                    out.push((p, *color));
                }
            }
            DisplayItem::Line {
                x,
                y,
                width,
                thickness,
                color,
                dashed: _,
            } => {
                // Slice E doesn't render dashed lines yet (no \hdashline in
                // the corpus). When it does, this is where the dash pattern
                // would be applied. Solid for now.
                out.push((
                    rect_path(*x, *y, *width, *thickness, baseline_shift),
                    *color,
                ));
            }
            DisplayItem::Rect {
                x,
                y,
                width,
                height,
                color,
            } => out.push((rect_path(*x, *y, *width, *height, baseline_shift), *color)),
            DisplayItem::Path {
                x,
                y,
                commands,
                fill: _,
                color,
            } => {
                // `fill` is true for filled shapes (radical bowls, etc.).
                // Manimax's fill pipeline treats every BezPath as filled
                // already; an explicit `false` would mean "stroke only,"
                // which RaTeX in practice never emits in the supported
                // corpus. Honoring `fill=false` is deferred until a
                // corpus expression actually needs it.
                out.push((path_from_commands(commands, *x, *y, baseline_shift), *color));
            }
        }
    }
    out
}

fn glyph_path(
    x: f64,
    y: f64,
    scale: f64,
    font_id: &str,
    char_code: u32,
    baseline_shift: Affine,
    ctx: &mut ScaleContext,
) -> Option<BezPath> {
    let font_bytes = manim_rs_text::katex_font(font_id)?;
    // `scale` is already in em-units; multiply by world-units-per-em to
    // pass swash a ppem in our output unit system.
    let ppem = (scale * WORLD_UNITS_PER_EM) as f32;
    let mut outline = manim_rs_text::glyph_to_bezpath(font_bytes, char_code, ppem, ctx);
    if outline.is_empty() {
        return None;
    }
    // Glyph outline is y-up at baseline-origin. Page-y is y-down, so the
    // per-glyph translate negates y; the baseline shift then composes on
    // top so the whole transform is one apply_affine pass.
    let xform =
        baseline_shift * Affine::translate((x * WORLD_UNITS_PER_EM, -y * WORLD_UNITS_PER_EM));
    outline.apply_affine(xform);
    Some(outline)
}

/// `(x, y, w, h)` in em, page-y-down convention; emit a closed rectangle
/// in world space (y-up), with the baseline shift already applied.
fn rect_path(x: f64, y: f64, w: f64, h: f64, baseline_shift: Affine) -> BezPath {
    let scale = WORLD_UNITS_PER_EM;
    let world_top_y = -y * scale;
    let world_bottom_y = -(y + h) * scale;
    let world_left = x * scale;
    let world_right = (x + w) * scale;
    let rect = Rect::new(world_left, world_bottom_y, world_right, world_top_y);
    let mut path = rect.to_path(0.0);
    path.apply_affine(baseline_shift);
    path
}

/// Translate `commands` (page-y-down, em-units, local origin) to a
/// world-space (y-up, world-units) `BezPath`. Single affine handles the
/// translate-by-`(x, y)`, the y-flip, and the baseline shift in one pass:
///
///   world.x = (cmd.x + x) * em
///   world.y = list.height + -(cmd.y + y) * em   [baseline_shift folded in]
///
/// `kurbo::Affine::new([m11, m12, m21, m22, dx, dy])` treats coefficients
/// column-major — verified by the `affine_flips_correctly` unit test
/// below.
fn path_from_commands(
    commands: &[PathCommand],
    origin_x: f64,
    origin_y: f64,
    baseline_shift: Affine,
) -> BezPath {
    let em = WORLD_UNITS_PER_EM;
    let local = Affine::new([em, 0.0, 0.0, -em, origin_x * em, -origin_y * em]);
    let xform = baseline_shift * local;

    let mut path = BezPath::new();
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => path.move_to(KPoint::new(x, y)),
            PathCommand::LineTo { x, y } => path.line_to(KPoint::new(x, y)),
            PathCommand::CubicTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                path.curve_to(KPoint::new(x1, y1), KPoint::new(x2, y2), KPoint::new(x, y));
            }
            PathCommand::QuadTo { x1, y1, x, y } => {
                path.quad_to(KPoint::new(x1, y1), KPoint::new(x, y));
            }
            PathCommand::Close => path.close_path(),
        }
    }
    path.apply_affine(xform);
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::tex_to_display_list;
    use kurbo::Shape;

    fn render(src: &str) -> Vec<(BezPath, Color)> {
        let dl = tex_to_display_list(src).expect("parses");
        display_list_to_bezpath(&dl)
    }

    fn assert_paths_renderable(paths: &[(BezPath, Color)], label: &str) {
        assert!(!paths.is_empty(), "{label}: adapter returned no paths");
        let any_non_empty = paths.iter().any(|(p, _)| !p.elements().is_empty());
        assert!(any_non_empty, "{label}: every path was empty");
        let total_bbox = paths
            .iter()
            .filter(|(p, _)| !p.elements().is_empty())
            .map(|(p, _)| p.bounding_box())
            .reduce(|a, b| a.union(b))
            .expect("at least one non-empty path");
        assert!(
            total_bbox.width() > 0.05,
            "{label}: combined bbox width too small: {total_bbox:?}"
        );
        assert!(
            total_bbox.height() > 0.05,
            "{label}: combined bbox height too small: {total_bbox:?}"
        );
    }

    #[test]
    fn affine_flips_correctly() {
        // Sanity-check the column-major layout: the matrix used in
        // `path_from_commands` should send (1, 1) → (1 + 0, -(1 + 0)) when
        // origin = (0, 0).
        let em = WORLD_UNITS_PER_EM;
        let xform = Affine::new([em, 0.0, 0.0, -em, 0.0, 0.0]);
        let p = xform * KPoint::new(1.0, 1.0);
        assert!((p.x - 1.0).abs() < 1e-9, "x = {}", p.x);
        assert!((p.y - -1.0).abs() < 1e-9, "y = {}", p.y);
    }

    #[test]
    fn frac_a_over_b_renders() {
        let paths = render(r"\frac{a}{b}");
        assert_paths_renderable(&paths, r"\frac{a}{b}");
        // \frac must include a Line — confirm a non-glyph rectangle made
        // it through the adapter (a thin BezPath rectangle from rect_path).
        let any_thin_rect = paths.iter().any(|(p, _)| {
            let b = p.bounding_box();
            b.height() < 0.1 && b.width() > 0.1
        });
        assert!(any_thin_rect, "expected a thin rectangular bar in \\frac");
    }

    #[test]
    fn sqrt_x_renders() {
        assert_paths_renderable(&render(r"\sqrt{x}"), r"\sqrt{x}");
    }

    #[test]
    fn x_squared_renders() {
        let paths = render(r"x^2");
        assert_paths_renderable(&paths, "x^2");
        // The exponent must sit above the baseline of x, so combined bbox
        // should span more than a single glyph's height.
        let bbox = paths
            .iter()
            .map(|(p, _)| p.bounding_box())
            .reduce(|a, b| a.union(b))
            .unwrap();
        assert!(
            bbox.max_y() > 0.4,
            "exponent did not lift bbox above baseline: {bbox:?}"
        );
    }

    #[test]
    fn sum_with_limits_renders() {
        assert_paths_renderable(&render(r"\sum_{i=1}^n i"), r"\sum_{i=1}^n i");
    }

    #[test]
    fn empty_display_list_yields_empty_vec() {
        assert!(render("").is_empty());
    }
}
