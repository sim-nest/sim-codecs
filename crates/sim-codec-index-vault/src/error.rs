//! Typed vault codec failures.

use super::*;

/// Typed encoding failures.
#[derive(Debug)]
pub enum VaultCodecError {
    /// Unknown profile.
    UnknownProfile(String),
    /// Projection claims were not closed.
    UnclosedClaims,
    /// A public bound was exceeded.
    BoundExceeded(&'static str),
    /// Invalid reversible path input.
    InvalidPath(String),
    /// Duplicate, escaping, or case-fold-colliding path.
    PathConflict(String),
    /// Document codec failure.
    Markup(sim_codec_doc::MarkupError),
    /// Document codec reported non-exact fidelity.
    MarkupLoss,
    /// Bundle and decoder profiles differ.
    ProfileDisagreement,
    /// Bundle and document granularities differ.
    GranularityDisagreement,
    /// Required semantic metadata is absent.
    MissingMetadata(&'static str),
    /// Semantic metadata has the wrong value type.
    InvalidMetadata(&'static str),
    /// Entry is not UTF-8 Markdown.
    InvalidUtf8(String),
    /// An entry content digest is false.
    ContentDigest(String),
    /// The projection identity is false.
    ProjectionDigest,
    /// The ordered bundle identity is false.
    BundleDigest,
    /// A row content site did not use the canonical Index grammar.
    RowCodec(String),
    /// A section is unknown or disagrees with its typed row.
    UnknownSection,
    /// A primary row appeared outside its typed content site.
    MisplacedPrimaryContent,
    /// The same canonical row occurred twice.
    DuplicateRow,
    /// A note path did not reverse to its declared identity.
    PathReversal(String),
    /// README navigation was missing, duplicated, or used as primary content.
    StrayNavigation,
    /// Canonical reconstruction or claim closure failed.
    Reconstruction(String),
    /// Historical note identities differ from the explicit v1 projection.
    LegacySemanticDrift,
}
impl fmt::Display for VaultCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProfile(v) => write!(f, "unknown v2 vault profile {v:?}"),
            Self::UnclosedClaims => f.write_str("projection claim certificate is not closed"),
            Self::BoundExceeded(v) => write!(f, "vault {v} bound exceeded"),
            Self::InvalidPath(v) => write!(f, "invalid vault note path identity {v:?}"),
            Self::PathConflict(v) => write!(f, "vault path conflict at {v:?}"),
            Self::Markup(v) => write!(f, "document codec failed: {v}"),
            Self::MarkupLoss => f.write_str("document codec reported unexpected fidelity evidence"),
            Self::ProfileDisagreement => {
                f.write_str("vault profile metadata disagrees with the selected decoder")
            }
            Self::GranularityDisagreement => {
                f.write_str("vault granularity metadata disagrees with the bundle")
            }
            Self::MissingMetadata(v) => write!(f, "missing vault metadata {v}"),
            Self::InvalidMetadata(v) => write!(f, "invalid vault metadata {v}"),
            Self::InvalidUtf8(v) => write!(f, "vault entry {v:?} is not UTF-8"),
            Self::ContentDigest(v) => write!(f, "vault entry {v:?} has a false content digest"),
            Self::ProjectionDigest => {
                f.write_str("vault projection identity disagrees with semantic content")
            }
            Self::BundleDigest => f.write_str("vault bundle identity disagrees with its entries"),
            Self::RowCodec(v) => write!(f, "canonical Index row codec failed: {v}"),
            Self::UnknownSection => f.write_str("unknown or disagreeing semantic vault section"),
            Self::MisplacedPrimaryContent => f.write_str("primary content is missing or misplaced"),
            Self::DuplicateRow => f.write_str("duplicate canonical row in vault"),
            Self::PathReversal(v) => {
                write!(f, "vault path {v:?} does not reverse to its note identity")
            }
            Self::StrayNavigation => {
                f.write_str("missing, duplicate, or semantically invalid README navigation")
            }
            Self::Reconstruction(v) => write!(f, "vault projection reconstruction failed: {v}"),
            Self::LegacySemanticDrift => {
                f.write_str("legacy vault semantics disagree with the declared v1 projection")
            }
        }
    }
}
impl Error for VaultCodecError {}
