//! Public catalog of implemented and tracked markup backends.

use crate::BackendId;

/// Implementation state for a cataloged markup backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendStatus {
    /// The backend is installed by the default registry and supports read/write.
    Implemented,
    /// The format is tracked by name, but no parser or writer is registered.
    Tracked,
    /// The format is tracked as an external-site-backed backend candidate.
    ExternalSiteCandidate,
}

/// Catalog row describing one markup backend or tracked format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendInfo {
    /// Stable backend id, used in `codec:markup/<id>` when implemented.
    pub id: BackendId,
    /// Implementation state for this backend.
    pub status: BackendStatus,
    /// Whether the default runtime can decode this backend.
    pub can_read: bool,
    /// Whether the default runtime can encode this backend.
    pub can_write: bool,
    /// Human-readable catalog note.
    pub notes: &'static str,
}

/// Return the deterministic catalog of implemented and tracked markup backends.
pub fn backend_catalog() -> Vec<BackendInfo> {
    let mut catalog = vec![
        implemented(
            "asciidoc",
            "Safe AsciiDoc read/write backend over asciidork-parser.",
        ),
        implemented(
            "latex",
            "Safe LaTeX article-subset backend over tree-sitter.",
        ),
        implemented(
            "markdown",
            "CommonMark/GFM-compatible Markdown read/write backend.",
        ),
        implemented("typst", "Safe Typst read/write backend over typst-syntax."),
        external_candidate(
            "texinfo",
            "Texinfo is tracked as an external-site candidate; no local parser or texi2any site is registered.",
        ),
        tracked("bbcode", "Bulletin-board markup is tracked by name only."),
        tracked("creole", "Creole wiki markup is tracked by name only."),
        tracked("djot", "Djot is tracked by name only."),
        tracked("myst", "MyST Markdown is tracked by name only."),
        tracked("org", "Org markup is tracked by name only."),
        tracked("rest", "reStructuredText is tracked by name only."),
        tracked("textile", "Textile markup is tracked by name only."),
        tracked("wikitext", "WikiText is tracked by name only."),
    ];
    catalog.sort_by(|left, right| left.id.cmp(&right.id));
    catalog
}

fn implemented(id: &'static str, notes: &'static str) -> BackendInfo {
    BackendInfo {
        id: BackendId::new(id),
        status: BackendStatus::Implemented,
        can_read: true,
        can_write: true,
        notes,
    }
}

fn tracked(id: &'static str, notes: &'static str) -> BackendInfo {
    BackendInfo {
        id: BackendId::new(id),
        status: BackendStatus::Tracked,
        can_read: false,
        can_write: false,
        notes,
    }
}

fn external_candidate(id: &'static str, notes: &'static str) -> BackendInfo {
    BackendInfo {
        id: BackendId::new(id),
        status: BackendStatus::ExternalSiteCandidate,
        can_read: false,
        can_write: false,
        notes,
    }
}
