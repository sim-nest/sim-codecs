//! Bounded, lossless TypeScript 7 and TSX syntax layered on JavaScript.
//!
//! ECMAScript productions remain [`sim_codec_javascript::Node`] values. This
//! crate adds only TypeScript/TSX notation and deliberately has no checker,
//! compiler, lowering pipeline, module resolver, or runtime model.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod parser;
mod types;

pub use parser::{parse_module, parse_module_with_limits, parse_tsx, parse_tsx_with_limits};
pub use types::{Diagnostic, DiagnosticCode, Language, Limits, SyntaxKind, SyntaxNode, SyntaxTree};

/// Frozen language release.
pub const TYPESCRIPT_VERSION: &str = "7.0.2";
/// Immutable upstream syntax authority for the frozen release.
pub const TYPESCRIPT_AUTHORITY: &str =
    "https://github.com/microsoft/TypeScript/tree/v7.0.2/src/compiler";
/// Frozen syntax/test identity.
pub const TYPESCRIPT_TEST_IDENTITY: &str =
    "microsoft/TypeScript v7.0.2 parser and conformance tests";
/// Date on which the syntax identity was frozen.
pub const TYPESCRIPT_FROZEN_ON: &str = "2026-08-09";

#[cfg(test)]
mod tests;
