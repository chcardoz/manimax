//! Bundled font byte accessors.
//!
//! Inter Regular is embedded directly via `include_bytes!` and is naturally
//! `&'static [u8]`.
//!
//! KaTeX TTFs come from the upstream `ratex-katex-fonts` crate, which uses
//! `rust-embed`. In practice that crate hands us `Cow::Owned` for every
//! file (rust-embed's non-debug mode embeds as `&'static [u8]` constants
//! internally but exposes `Cow::Owned(Vec)` at the public boundary). To
//! keep the `&'static [u8]` API the slice plan calls for — and to avoid
//! re-decoding font bytes on every glyph lookup — each font is leaked
//! once into a process-wide `OnceLock` cache. The bundled set is small
//! and fixed (~20 files, ~2 MB total); leaking is the right tradeoff.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

const INTER_REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular.ttf");

/// Default text font (Inter Regular, OFL-1.1). Used when a `Text` mobject
/// doesn't supply an explicit font path.
pub fn default_text_font() -> &'static [u8] {
    INTER_REGULAR
}

fn katex_cache() -> &'static RwLock<HashMap<String, &'static [u8]>> {
    static CACHE: OnceLock<RwLock<HashMap<String, &'static [u8]>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Resolve a RaTeX `DisplayItem::GlyphPath::font` id (e.g. `"Main-Regular"`,
/// `"Math-Italic"`) to the embedded KaTeX TTF bytes. RaTeX stores font ids
/// without the `KaTeX_` prefix or the `.ttf` suffix; the TTF filenames in
/// `ratex-katex-fonts` carry both. This function bridges the two.
///
/// Returns `None` if the id is unknown to the bundled font set.
pub fn katex_font(font_id: &str) -> Option<&'static [u8]> {
    // Lock poisoning here means a previous caller panicked inside
    // Box::leak / into_owned — a real bug, not "font not found." Don't
    // mask it as None; let `.expect` crash this caller too.
    if let Some(bytes) = katex_cache()
        .read()
        .expect("katex font cache lock poisoned")
        .get(font_id)
        .copied()
    {
        return Some(bytes);
    }

    // Take the write lock *before* leaking so concurrent cold misses
    // can't both leak duplicate font allocations. Re-check under the
    // lock first; if a winner already inserted, return their entry.
    let mut cache = katex_cache()
        .write()
        .expect("katex font cache lock poisoned");
    if let Some(bytes) = cache.get(font_id).copied() {
        return Some(bytes);
    }

    let filename = format!("KaTeX_{font_id}.ttf");
    let bytes_cow = ratex_katex_fonts::ttf_bytes(&filename)?;
    let leaked: &'static [u8] = Box::leak(bytes_cow.into_owned().into_boxed_slice());
    cache.insert(font_id.to_string(), leaked);
    Some(leaked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inter_regular_is_a_real_ttf() {
        let bytes = default_text_font();
        assert!(bytes.len() > 1024, "font is suspiciously small");
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x00, 0x00]);
    }

    #[test]
    fn katex_main_regular_resolves() {
        let bytes = katex_font("Main-Regular").expect("Main-Regular bundled");
        assert!(bytes.len() > 1024);
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x00, 0x00]);
    }

    #[test]
    fn katex_font_lookup_is_cached() {
        let a = katex_font("Main-Regular").unwrap();
        let b = katex_font("Main-Regular").unwrap();
        // Same backing storage — the cache returns the leaked slice.
        assert_eq!(a.as_ptr(), b.as_ptr());
    }

    #[test]
    fn unknown_katex_font_id_is_none() {
        assert!(katex_font("Bogus-Variant").is_none());
    }

    /// Every font id RaTeX emits in its corpus must resolve. Slice E §6
    /// gotcha #3: a silent rename upstream would make all glyph lookups
    /// for that face miss with no test failure unless we pin the set.
    #[test]
    fn all_known_katex_font_ids_resolve() {
        let ids = [
            "Main-Regular",
            "Main-Bold",
            "Main-Italic",
            "Main-BoldItalic",
            "Math-Italic",
            "Math-BoldItalic",
            "AMS-Regular",
            "Caligraphic-Regular",
            "Caligraphic-Bold",
            "Fraktur-Regular",
            "Fraktur-Bold",
            "SansSerif-Regular",
            "SansSerif-Bold",
            "SansSerif-Italic",
            "Script-Regular",
            "Typewriter-Regular",
            "Size1-Regular",
            "Size2-Regular",
            "Size3-Regular",
            "Size4-Regular",
        ];
        for id in ids {
            assert!(
                katex_font(id).is_some(),
                "katex_font({id:?}) returned None — RaTeX may have renamed a face"
            );
        }
    }
}
