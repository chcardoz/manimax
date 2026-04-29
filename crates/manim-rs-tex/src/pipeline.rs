//! parse → layout → DisplayList.

use crate::error::TexError;
use ratex_layout::{LayoutOptions, layout, to_display_list};
use ratex_parser::parse;
use ratex_types::DisplayList;

/// Compile a Tex source string into a RaTeX `DisplayList`.
///
/// Uses default `LayoutOptions` (Display math style, black default color).
/// Color overrides happen later, per-item, in `display_list_to_bezpath`'s
/// caller.
pub fn tex_to_display_list(src: &str) -> Result<DisplayList, TexError> {
    let nodes = parse(src)?;
    let opts = LayoutOptions::default();
    let layout_box = layout(&nodes, &opts);
    Ok(to_display_list(&layout_box))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_compiles() {
        let dl = tex_to_display_list("").expect("empty parses");
        assert!(dl.items.is_empty());
    }

    #[test]
    fn frac_produces_glyphs_and_a_bar() {
        let dl = tex_to_display_list(r"\frac{a}{b}").expect("parses");
        assert!(!dl.items.is_empty());
        // A fraction must have at least one Line (the bar).
        let has_line = dl
            .items
            .iter()
            .any(|it| matches!(it, ratex_types::DisplayItem::Line { .. }));
        assert!(has_line, "\\frac{{a}}{{b}} should emit a Line for the bar");
    }

    #[test]
    fn unparseable_source_returns_error() {
        // Unmatched braces — definitely a parse error.
        let err = tex_to_display_list(r"\frac{a").unwrap_err();
        let TexError::Parse { message, .. } = err;
        assert!(!message.is_empty());
    }
}
