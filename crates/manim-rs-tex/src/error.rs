//! Error type wrapping RaTeX failures with optional source-location info.

use thiserror::Error;

/// Failure surface for Tex source compilation.
///
/// Currently a thin shell around RaTeX's `ParseError`. Layout and
/// `to_display_list` are infallible in RaTeX (they never return `Result`),
/// so anything that goes wrong there is a panic upstream — not our error
/// to surface. If that ever changes, add variants here.
#[derive(Debug, Error)]
pub enum TexError {
    /// Parser rejected the source. `loc` is the byte offset RaTeX reported,
    /// when available.
    #[error("Tex parse error{}: {message}", .loc.map(|p| format!(" at byte {p}")).unwrap_or_default())]
    Parse { message: String, loc: Option<usize> },
}

impl From<ratex_parser::ParseError> for TexError {
    fn from(err: ratex_parser::ParseError) -> Self {
        TexError::Parse {
            message: err.message,
            loc: err.loc.map(|l| l.start),
        }
    }
}
