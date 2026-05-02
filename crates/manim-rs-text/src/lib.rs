//! Font plumbing + glyph outline → `kurbo::BezPath` for the Slice E
//! text/Tex pipelines. See `docs/slices/slice-e.md` Step 1.
//!
//! Two halves share this crate: plain text (`Text`) feeds Inter via
//! cosmic-text, and Tex feeds KaTeX TTFs via RaTeX's `DisplayList`. Both
//! end up in `glyph_to_bezpath` to produce Manimax-convention outlines
//! (y-up, fill-ready).

mod cosmic;
mod font;
mod glyph;

pub use cosmic::{TextAlign, TextWeight, text_to_bezpaths};
pub use font::katex_font;
pub use glyph::glyph_to_bezpath;
// Re-export `ScaleContext` so callers building many glyphs in a row can
// hoist one out of their hot loop and pass it into `glyph_to_bezpath`.
pub use swash::scale::ScaleContext;
