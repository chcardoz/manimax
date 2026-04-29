//! Tex source → `kurbo::BezPath` outlines via RaTeX.
//!
//! Slice E Step 2 boundary: this crate owns the bridge from a string of
//! Tex source to a flat list of `(BezPath, Color)` ready for the existing
//! fill pipeline. The two halves:
//!
//! 1. [`tex_to_display_list`] — `parse → layout → DisplayList`. Wraps
//!    RaTeX's `ratex_parser::parse`, `ratex_layout::layout`, and
//!    `ratex_layout::to_display_list` into a single error-friendly call.
//! 2. [`display_list_to_bezpath`] — the adapter from RaTeX's
//!    `DisplayItem` variants to `kurbo::BezPath`s in Manimax world space.
//!
//! Coordinate convention: RaTeX's `DisplayList` is laid out with positive
//! y pointing **down** the page (standard typographic convention), in
//! whatever units RaTeX's `LayoutOptions` produces (its size unit is em,
//! scaled by `MathStyle::size_multiplier`). The adapter applies a single
//! y-flip per item at translation time so callers always see Manimax
//! y-up coordinates. Glyph outlines themselves come back from
//! `manim_rs_text::glyph_to_bezpath` in y-up natural — they are not
//! flipped a second time.
//!
//! Color handling: each emitted `(BezPath, Color)` carries the
//! `DisplayItem`'s color as RaTeX produced it. Per-item colors set by
//! `\textcolor{red}{...}` ride through unchanged. The Tex IR variant's
//! top-level `color` field (Step 3) overrides items whose color is the
//! default black, leaving explicitly-colored items alone — see
//! `docs/slices/slice-e.md` §6 gotcha #10.

mod adapter;
mod error;
mod pipeline;

pub use adapter::display_list_to_bezpath;
pub use error::TexError;
pub use pipeline::tex_to_display_list;

// Re-export the `Color` type so callers (eval, tests) don't need to depend
// on `ratex-types` directly.
pub use ratex_types::Color;
