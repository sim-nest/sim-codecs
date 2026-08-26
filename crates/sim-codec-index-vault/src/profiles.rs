//! Vault profile definitions and legacy compatibility verification.

use super::*;

/// Versioned profile identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum VaultProfileId {
    /// Portable CommonMark.
    PortableMarkdownV2,
    /// Obsidian file vault.
    ObsidianMarkdownV2,
    /// Seqlog plain Markdown.
    SeqlogMarkdownV2,
    /// Logseq file graph.
    LogseqFileV2,
}

impl VaultProfileId {
    /// Stable profile id.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PortableMarkdownV2 => "portable-markdown-v2",
            Self::ObsidianMarkdownV2 => "obsidian-markdown-v2",
            Self::SeqlogMarkdownV2 => "seqlog-markdown-v2",
            Self::LogseqFileV2 => "logseq-file-v2",
        }
    }
}

/// Semantic outline choice; Markdown spelling remains owned by `sim-codec-doc`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutlineMapping {
    /// Headings and ordinary lists.
    HeadingsAndLists,
    /// A property prelude and indented list body.
    IndentedLists,
}

/// Closed profile descriptor containing syntax choices only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VaultProfile {
    /// Versioned id.
    pub id: VaultProfileId,
    /// Metadata envelope.
    pub attributes: AttributeEnvelope,
    /// Link spelling.
    pub links: LinkDialect,
    /// Outline mapping.
    pub outline: OutlineMapping,
    /// Evidence boundary identifier.
    pub compatibility_evidence: &'static str,
}

/// All four writable v2 profiles.
pub const PROFILES: [VaultProfile; 4] = [
    profile(
        VaultProfileId::PortableMarkdownV2,
        AttributeEnvelope::JsonFrontMatter,
        LinkDialect::CommonMark,
        OutlineMapping::HeadingsAndLists,
        "commonmark-0.31.2-json-envelope",
    ),
    profile(
        VaultProfileId::ObsidianMarkdownV2,
        AttributeEnvelope::JsonFrontMatter,
        LinkDialect::WikiLink,
        OutlineMapping::HeadingsAndLists,
        "obsidian-properties-internal-links",
    ),
    profile(
        VaultProfileId::SeqlogMarkdownV2,
        AttributeEnvelope::JsonFrontMatter,
        LinkDialect::CommonMark,
        OutlineMapping::HeadingsAndLists,
        "seqlog-plain-commonmark-files",
    ),
    profile(
        VaultProfileId::LogseqFileV2,
        AttributeEnvelope::DoubleColon,
        LinkDialect::WikiLink,
        OutlineMapping::IndentedLists,
        "logseq-file-graph-markdown",
    ),
];

/// Exact identities of the four historical, decode-only vault profiles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegacyVaultProfileId {
    /// July portable Markdown.
    PortableMarkdownV1,
    /// July Obsidian Markdown.
    ObsidianMarkdownV1,
    /// July Seqlog Markdown.
    SeqlogMarkdownV1,
    /// July Logseq file graph.
    LogseqFileV1,
}

impl LegacyVaultProfileId {
    /// Stable historical profile id.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PortableMarkdownV1 => "portable-markdown-v1",
            Self::ObsidianMarkdownV1 => "obsidian-markdown-v1",
            Self::SeqlogMarkdownV1 => "seqlog-markdown-v1",
            Self::LogseqFileV1 => "logseq-file-v1",
        }
    }
}

/// Frozen syntax descriptor for a historical decode-only profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LegacyVaultProfile {
    /// Exact historical identity.
    pub id: LegacyVaultProfileId,
    /// Historical metadata envelope.
    pub attributes: AttributeEnvelope,
    /// Historical link spelling.
    pub links: LinkDialect,
    /// Historical outline mapping.
    pub outline: OutlineMapping,
}

/// Closed set of readable v1 profiles. No encoder accepts this type.
pub const LEGACY_PROFILES: [LegacyVaultProfile; 4] = [
    legacy_profile(
        LegacyVaultProfileId::PortableMarkdownV1,
        AttributeEnvelope::LegacyYamlStringFrontMatter,
        LinkDialect::CommonMark,
        OutlineMapping::HeadingsAndLists,
    ),
    legacy_profile(
        LegacyVaultProfileId::ObsidianMarkdownV1,
        AttributeEnvelope::LegacyYamlStringFrontMatter,
        LinkDialect::WikiLink,
        OutlineMapping::HeadingsAndLists,
    ),
    legacy_profile(
        LegacyVaultProfileId::SeqlogMarkdownV1,
        AttributeEnvelope::LegacyYamlStringFrontMatter,
        LinkDialect::CommonMark,
        OutlineMapping::HeadingsAndLists,
    ),
    legacy_profile(
        LegacyVaultProfileId::LogseqFileV1,
        AttributeEnvelope::LegacyDoubleColonStrings,
        LinkDialect::WikiLink,
        OutlineMapping::IndentedLists,
    ),
];

const fn legacy_profile(
    id: LegacyVaultProfileId,
    attributes: AttributeEnvelope,
    links: LinkDialect,
    outline: OutlineMapping,
) -> LegacyVaultProfile {
    LegacyVaultProfile {
        id,
        attributes,
        links,
        outline,
    }
}

/// Resolves only an exact historical id; friendly aliases always select v2.
pub fn resolve_legacy_profile(name: &str) -> Result<LegacyVaultProfile, VaultCodecError> {
    LEGACY_PROFILES
        .into_iter()
        .find(|profile| profile.id.as_str() == name)
        .ok_or_else(|| VaultCodecError::UnknownProfile(name.into()))
}

/// Row families deliberately absent from every delivered v1 vault.
pub const LEGACY_V1_KNOWN_ABSENT_FAMILIES: [&str; 2] = ["declaration", "protocol"];

/// One caller-supplied file from a read-only v1 bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyVaultEntry {
    /// Normalized vault-relative path.
    pub path: String,
    /// Exact historical bytes.
    pub bytes: Vec<u8>,
}

/// Bounded caller-supplied v1 bundle. It has deliberately no encoder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyVaultBundle {
    /// Exact historical profile descriptor.
    pub profile: LegacyVaultProfile,
    /// Historical projection density.
    pub granularity: VaultGranularity,
    /// Sorted managed files, excluding the ownership manifest.
    pub entries: Vec<LegacyVaultEntry>,
}

/// Result of semantic v1 verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyVaultVerification {
    /// Stable identities found in the historical notes.
    pub note_identities: BTreeSet<(String, String)>,
    /// Row families intentionally not represented by v1.
    pub known_absent_families: &'static [&'static str],
}

/// Decode a historical bundle and compare its note identities to the explicit
/// incomplete legacy projection. This accepts bytes only from the caller and
/// performs no filesystem or reverse-import operation.
pub fn verify_legacy_v1(
    bundle: &LegacyVaultBundle,
    expected: &VaultProjection,
) -> Result<LegacyVaultVerification, VaultCodecError> {
    if bundle.entries.len() > MAX_NOTES + 1 {
        return Err(VaultCodecError::BoundExceeded("legacy notes"));
    }
    let total = bundle.entries.iter().try_fold(0usize, |total, entry| {
        total
            .checked_add(entry.bytes.len())
            .ok_or(VaultCodecError::BoundExceeded("legacy bundle bytes"))
    })?;
    if total > MAX_BUNDLE_BYTES {
        return Err(VaultCodecError::BoundExceeded("legacy bundle bytes"));
    }
    let backend = DialectMarkdownBackend::new(MarkdownDialect {
        attributes: bundle.profile.attributes,
        links: bundle.profile.links,
        ..MarkdownDialect::default()
    })
    .map_err(VaultCodecError::Markup)?;
    let mut paths = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let mut readme = false;
    for entry in &bundle.entries {
        if entry.bytes.len() > MAX_NOTE_BYTES {
            return Err(VaultCodecError::BoundExceeded("legacy note bytes"));
        }
        if entry.path.starts_with('/')
            || entry.path.contains('\\')
            || entry
                .path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || !paths.insert(entry.path.clone())
        {
            return Err(VaultCodecError::PathConflict(entry.path.clone()));
        }
        let text = std::str::from_utf8(&entry.bytes)
            .map_err(|_| VaultCodecError::InvalidUtf8(entry.path.clone()))?;
        let (doc, fidelity) = backend
            .decode(
                text,
                &MarkupDecodeOptions {
                    preserve_source: false,
                    preserve_raw: false,
                },
            )
            .map_err(VaultCodecError::Markup)?;
        if !fidelity.dropped.is_empty() || !fidelity.warnings.is_empty() {
            return Err(VaultCodecError::MarkupLoss);
        }
        let profile = string_attr(&doc, "sim_profile")?;
        if profile != bundle.profile.id.as_str() {
            return Err(VaultCodecError::ProfileDisagreement);
        }
        let granularity = string_attr(&doc, "granularity")?;
        if granularity != granularity_name(bundle.granularity) {
            return Err(VaultCodecError::GranularityDisagreement);
        }
        if entry.path == "README.md" {
            if readme || doc.title.as_deref() != Some("SIM Index Vault") {
                return Err(VaultCodecError::StrayNavigation);
            }
            readme = true;
            continue;
        }
        let id = string_attr(&doc, "sim_id")?.to_owned();
        let kind = string_attr(&doc, "kind")?.to_owned();
        if doc.title.as_deref().is_none() || !identities.insert((kind, id)) {
            return Err(VaultCodecError::DuplicateRow);
        }
    }
    if !readme {
        return Err(VaultCodecError::MissingMetadata("README"));
    }
    let expected_identities = expected
        .notes()
        .iter()
        .map(|note| (kind_name(note.kind).to_owned(), note.id.as_str().to_owned()))
        .collect::<BTreeSet<_>>();
    if identities != expected_identities {
        return Err(VaultCodecError::LegacySemanticDrift);
    }
    Ok(LegacyVaultVerification {
        note_identities: identities,
        known_absent_families: &LEGACY_V1_KNOWN_ABSENT_FAMILIES,
    })
}

fn string_attr<'a>(doc: &'a MarkupDoc, name: &'static str) -> Result<&'a str, VaultCodecError> {
    match doc.attrs.get(name) {
        Some(Expr::String(value)) => Ok(value),
        _ => Err(VaultCodecError::MissingMetadata(name)),
    }
}

/// Builds the exact incomplete July projection used to judge legacy semantics.
///
/// This value is comparison evidence only. It cannot be encoded by this crate.
pub fn legacy_projection_v1(
    doc: &IndexDoc,
    granularity: VaultGranularity,
) -> Result<VaultProjection, VaultCodecError> {
    let mut legacy = doc.clone();
    legacy.declarations.clear();
    legacy.protocol_relations.clear();
    VaultProjection::from_complete(&legacy, granularity)
        .map_err(|error| VaultCodecError::Reconstruction(error.to_string()))
}
const fn profile(
    id: VaultProfileId,
    attributes: AttributeEnvelope,
    links: LinkDialect,
    outline: OutlineMapping,
    compatibility_evidence: &'static str,
) -> VaultProfile {
    VaultProfile {
        id,
        attributes,
        links,
        outline,
        compatibility_evidence,
    }
}

/// Resolves an exact v2 id or its friendly alias.
pub fn resolve_profile(name: &str) -> Result<VaultProfile, VaultCodecError> {
    let id = match name {
        "portable" | "portable-markdown-v2" => VaultProfileId::PortableMarkdownV2,
        "obsidian" | "obsidian-markdown-v2" => VaultProfileId::ObsidianMarkdownV2,
        "seqlog" | "seqlog-markdown-v2" => VaultProfileId::SeqlogMarkdownV2,
        "logseq" | "logseq-file-v2" => VaultProfileId::LogseqFileV2,
        _ => return Err(VaultCodecError::UnknownProfile(name.into())),
    };
    Ok(PROFILES
        .into_iter()
        .find(|p| p.id == id)
        .expect("complete profile table"))
}
