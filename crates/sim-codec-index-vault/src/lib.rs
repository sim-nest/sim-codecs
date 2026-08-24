//! Pure v2 bundle encoding for complete SIM Index vault projections.
//!
//! ```
//! use sim_codec_index_vault::{resolve_profile, VaultEncoder};
//! let encoder = VaultEncoder::new(resolve_profile("portable")?);
//! # let _ = encoder;
//! # Ok::<(), sim_codec_index_vault::VaultCodecError>(())
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use sha2::{Digest, Sha256};
use sim_codec_doc::{
    AttributeEnvelope, DialectMarkdownBackend, Inline, LinkDialect, MarkdownDialect, MarkupBackend,
    MarkupBlock, MarkupDecodeOptions, MarkupDoc, MarkupEncodeOptions,
};
use sim_codec_index::{
    IndexForm, decode_index_expr, encode_index_expr, expr_from_index_doc, index_doc_from_expr,
};
use sim_index_core::IndexDoc;
use sim_index_vault_core::{
    IndexRow, VaultGranularity, VaultNoteKind, VaultNotePlan, VaultProjection,
};
use sim_kernel::{ContentId, Expr, Symbol};

const MAX_NOTES: usize = 50_000;
const MAX_ROWS: usize = 100_000;
const MAX_NOTE_BYTES: usize = 1024 * 1024;
const MAX_BUNDLE_BYTES: usize = 128 * 1024 * 1024;
const ROOT: &str = "SIM-Index";

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

/// One sorted in-memory artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultEntry {
    /// Normalized vault-relative path.
    pub path: String,
    /// Encoded Markdown bytes.
    pub bytes: Vec<u8>,
    /// Note identity (`README` for navigation).
    pub note_id: String,
    /// Semantic kind.
    pub note_kind: Option<VaultNoteKind>,
    /// Row-family counts claimed here.
    pub claim_families: BTreeMap<String, usize>,
    /// Content digest.
    pub content_digest: ContentId,
}

/// Complete, deterministic in-memory bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultBundle {
    /// Selected profile.
    pub profile: VaultProfileId,
    /// Projection density.
    pub granularity: VaultGranularity,
    /// Semantic projection identity.
    pub projection_digest: ContentId,
    /// Ordered artifact-root identity.
    pub bundle_root: ContentId,
    /// Sorted entries, including README navigation.
    pub entries: Vec<VaultEntry>,
}

/// Pure configured encoder.
#[derive(Clone, Copy, Debug)]
pub struct VaultEncoder {
    profile: VaultProfile,
}

/// Pure decoder for caller-supplied in-memory bundle values.
#[derive(Clone, Copy, Debug)]
pub struct VaultDecoder {
    profile: VaultProfile,
}

/// A semantically reconstructed v2 vault.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedVault {
    /// Reconstructed complete projection with a newly closed certificate.
    pub projection: VaultProjection,
    /// Exact document-codec fidelity for every decoded entry.
    pub fidelity_exact: bool,
    /// Whether reconstructed semantics equal the declared projection identity.
    pub declared_projection_equal: bool,
}

/// One bounded semantic difference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultMismatch {
    /// Stable row or verification field path.
    pub path: String,
    /// Bounded expected value rendering.
    pub expected: String,
    /// Bounded decoded value rendering.
    pub actual: String,
}

/// Bounded verification result which retains the unbounded mismatch count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultVerification {
    /// Retained mismatch details.
    pub mismatches: Vec<VaultMismatch>,
    /// Total mismatches before retention bounds.
    pub total_mismatches: usize,
    /// Whether any mismatch or value was truncated.
    pub truncated: bool,
    /// Both projections have exactly the same semantic identity.
    pub projection_identity_equal: bool,
    /// The decoded claim certificate closed exactly.
    pub claims_closed: bool,
    /// The generic document codec reported exact fidelity.
    pub document_codec_exact: bool,
}

impl VaultVerification {
    /// Semantic success; byte equality alone is deliberately insufficient.
    pub fn is_success(&self) -> bool {
        self.projection_identity_equal
            && self.mismatches.is_empty()
            && self.total_mismatches == 0
            && self.claims_closed
            && self.document_codec_exact
    }
}

/// Decodes and compares a v2 bundle against an expected complete projection.
pub fn verify_v2(
    bundle: &VaultBundle,
    expected: &VaultProjection,
    max_mismatches: usize,
    max_value_bytes: usize,
) -> Result<VaultVerification, VaultCodecError> {
    let profile = PROFILES
        .into_iter()
        .find(|p| p.id == bundle.profile)
        .ok_or(VaultCodecError::ProfileDisagreement)?;
    let decoded = VaultDecoder::new(profile).decode(bundle)?;
    let expected_rows = expected.certificate().primary_rows();
    let actual_rows = decoded.projection.certificate().primary_rows();
    let mut all = Vec::new();
    for row in expected_rows.difference(actual_rows) {
        all.push((
            format!("rows.{}.missing", row_family(row)),
            format!("{row:?}"),
            "<absent>".into(),
        ));
    }
    for row in actual_rows.difference(expected_rows) {
        all.push((
            format!("rows.{}.unexpected", row_family(row)),
            "<absent>".into(),
            format!("{row:?}"),
        ));
    }
    if expected.granularity() != decoded.projection.granularity() {
        all.push((
            "granularity".into(),
            granularity_name(expected.granularity()).into(),
            granularity_name(decoded.projection.granularity()).into(),
        ));
    }
    let total_mismatches = all.len();
    let mut truncated = total_mismatches > max_mismatches;
    let mismatches = all
        .into_iter()
        .take(max_mismatches)
        .map(|(path, expected, actual)| {
            let (expected, a) = truncate_value(expected, max_value_bytes);
            truncated |= a;
            let (actual, b) = truncate_value(actual, max_value_bytes);
            truncated |= b;
            VaultMismatch {
                path,
                expected,
                actual,
            }
        })
        .collect();
    Ok(VaultVerification {
        mismatches,
        total_mismatches,
        truncated,
        projection_identity_equal: decoded.declared_projection_equal
            && projection_digest(expected) == projection_digest(&decoded.projection),
        claims_closed: decoded.projection.certificate().is_closed(),
        document_codec_exact: decoded.fidelity_exact,
    })
}

fn truncate_value(mut value: String, max: usize) -> (String, bool) {
    if value.len() <= max {
        return (value, false);
    }
    let mut end = max.min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    (value, true)
}

impl VaultDecoder {
    /// Creates a decoder for one exact descriptor.
    pub const fn new(profile: VaultProfile) -> Self {
        Self { profile }
    }

    /// Decodes and reconstructs a v2 bundle without filesystem or application access.
    pub fn decode(&self, bundle: &VaultBundle) -> Result<DecodedVault, VaultCodecError> {
        if bundle.profile != self.profile.id {
            return Err(VaultCodecError::ProfileDisagreement);
        }
        let backend = markdown_backend(self.profile)?;
        let mut rebuilt = IndexDoc::public("sim-codec-index-vault/decode-v2");
        let mut seen_paths = BTreeSet::new();
        let mut seen_rows = BTreeSet::new();
        let fidelity_exact = true;
        let mut readme_seen = false;
        for entry in &bundle.entries {
            if !seen_paths.insert(entry.path.clone()) {
                return Err(VaultCodecError::PathConflict(entry.path.clone()));
            }
            if content_id(b"sim.index-vault.content.v2\0", &entry.bytes) != entry.content_digest {
                return Err(VaultCodecError::ContentDigest(entry.path.clone()));
            }
            let text = std::str::from_utf8(&entry.bytes)
                .map_err(|_| VaultCodecError::InvalidUtf8(entry.path.clone()))?;
            let (doc, _fidelity) = backend
                .decode(
                    text,
                    &MarkupDecodeOptions {
                        preserve_source: false,
                        preserve_raw: false,
                    },
                )
                .map_err(VaultCodecError::Markup)?;
            validate_metadata(&doc, self.profile, bundle)?;
            if entry.path == "README.md" {
                if readme_seen || entry.note_kind.is_some() || entry.note_id != "README" {
                    return Err(VaultCodecError::StrayNavigation);
                }
                readme_seen = true;
                validate_readme(&doc)?;
                continue;
            }
            let kind = entry
                .note_kind
                .ok_or(VaultCodecError::MissingMetadata("note kind"))?;
            if note_path_parts(kind, &entry.note_id)? != entry.path {
                return Err(VaultCodecError::PathReversal(entry.path.clone()));
            }
            validate_note_heading(&doc, &entry.note_id)?;
            for row in semantic_rows(&doc)? {
                if !seen_rows.insert(row.clone()) {
                    return Err(VaultCodecError::DuplicateRow);
                }
                push_row(&mut rebuilt, row);
            }
        }
        if !readme_seen {
            return Err(VaultCodecError::MissingMetadata("README"));
        }
        let projection = VaultProjection::from_complete(&rebuilt, bundle.granularity)
            .map_err(|e| VaultCodecError::Reconstruction(e.to_string()))?;
        let declared_projection_equal = projection_digest(&projection) == bundle.projection_digest;
        if bundle_digest(&bundle.entries) != bundle.bundle_root {
            return Err(VaultCodecError::BundleDigest);
        }
        Ok(DecodedVault {
            projection,
            fidelity_exact,
            declared_projection_equal,
        })
    }
}
impl VaultEncoder {
    /// Creates an encoder for one descriptor.
    pub const fn new(profile: VaultProfile) -> Self {
        Self { profile }
    }
    /// Encodes a complete projection without filesystem access.
    pub fn encode(&self, projection: &VaultProjection) -> Result<VaultBundle, VaultCodecError> {
        if !projection.certificate().is_closed() {
            return Err(VaultCodecError::UnclosedClaims);
        }
        let row_count: usize = projection.notes().iter().map(|n| n.rows.len()).sum();
        if row_count > MAX_ROWS || projection.notes().len() > MAX_NOTES {
            return Err(VaultCodecError::BoundExceeded("projection"));
        }
        let projection_digest = projection_digest(projection);
        let mut entries = Vec::with_capacity(projection.notes().len() + 1);
        let paths = projection
            .notes()
            .iter()
            .map(note_path)
            .collect::<Result<Vec<_>, _>>()?;
        validate_paths(&paths)?;
        for (note, path) in projection.notes().iter().zip(paths.iter()) {
            let doc = note_doc(
                note,
                projection,
                self.profile,
                &projection_digest,
                path,
                &paths,
            )?;
            let bytes = self.encode_doc(&doc)?;
            if bytes.len() > MAX_NOTE_BYTES {
                return Err(VaultCodecError::BoundExceeded("note bytes"));
            }
            entries.push(VaultEntry {
                path: path.clone(),
                bytes: bytes.clone(),
                note_id: note.id.as_str().into(),
                note_kind: Some(note.kind),
                claim_families: family_counts(note),
                content_digest: content_id(b"sim.index-vault.content.v2\0", &bytes),
            });
        }
        let readme = readme_doc(projection, self.profile, &projection_digest, &paths);
        let bytes = self.encode_doc(&readme)?;
        entries.push(VaultEntry {
            path: "README.md".into(),
            bytes: bytes.clone(),
            note_id: "README".into(),
            note_kind: None,
            claim_families: BTreeMap::new(),
            content_digest: content_id(b"sim.index-vault.content.v2\0", &bytes),
        });
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        let total: usize = entries.iter().map(|e| e.bytes.len()).sum();
        if total > MAX_BUNDLE_BYTES {
            return Err(VaultCodecError::BoundExceeded("bundle bytes"));
        }
        let bundle_root = bundle_digest(&entries);
        Ok(VaultBundle {
            profile: self.profile.id,
            granularity: projection.granularity(),
            projection_digest,
            bundle_root,
            entries,
        })
    }
    fn encode_doc(&self, doc: &MarkupDoc) -> Result<Vec<u8>, VaultCodecError> {
        let backend = DialectMarkdownBackend::new(MarkdownDialect {
            attributes: self.profile.attributes,
            links: self.profile.links,
            ..MarkdownDialect::default()
        })
        .map_err(VaultCodecError::Markup)?;
        let (text, fidelity) = backend
            .encode(
                doc,
                &MarkupEncodeOptions {
                    fail_on_loss: true,
                    preserve_raw: false,
                },
            )
            .map_err(VaultCodecError::Markup)?;
        if !fidelity.dropped.is_empty()
            || !fidelity.warnings.is_empty()
            || !fidelity.preserved_raw.is_empty()
        {
            return Err(VaultCodecError::MarkupLoss);
        }
        Ok(text.into_bytes())
    }
}

fn note_doc(
    note: &VaultNotePlan,
    projection: &VaultProjection,
    profile: VaultProfile,
    digest: &ContentId,
    source_path: &str,
    all_paths: &[String],
) -> Result<MarkupDoc, VaultCodecError> {
    let mut attrs = common_attrs(profile, projection.granularity(), digest);
    attrs.insert("sim_note_id".into(), Expr::String(note.id.as_str().into()));
    attrs.insert(
        "sim_note_kind".into(),
        Expr::String(kind_name(note.kind).into()),
    );
    let mut blocks = vec![heading(1, note.id.as_str())];
    for row in &note.rows {
        blocks.push(heading(2, family_name(row)));
        blocks.push(row_block(row)?);
    }
    if projection.granularity() == VaultGranularity::Full {
        let targets = all_paths
            .iter()
            .filter(|p| p.as_str() != source_path)
            .map(|target| {
                vec![MarkupBlock::Paragraph {
                    content: vec![Inline::Link {
                        label: vec![Inline::Text(target.clone())],
                        target: link_target(source_path, target, profile.links),
                    }],
                    span: None,
                }]
            })
            .collect();
        blocks.push(MarkupBlock::List {
            ordered: false,
            items: targets,
            span: None,
        });
    }
    Ok(MarkupDoc {
        title: Some(note.id.as_str().into()),
        blocks,
        attrs,
        source: None,
    })
}
fn readme_doc(
    projection: &VaultProjection,
    profile: VaultProfile,
    digest: &ContentId,
    paths: &[String],
) -> MarkupDoc {
    let attrs = common_attrs(profile, projection.granularity(), digest);
    let items = paths
        .iter()
        .map(|p| {
            vec![MarkupBlock::Paragraph {
                content: vec![Inline::Link {
                    label: vec![Inline::Text(p.clone())],
                    target: link_target("README.md", p, profile.links),
                }],
                span: None,
            }]
        })
        .collect();
    MarkupDoc {
        title: Some("SIM Index Vault".into()),
        blocks: vec![
            heading(1, "SIM Index Vault"),
            MarkupBlock::List {
                ordered: false,
                items,
                span: None,
            },
        ],
        attrs,
        source: None,
    }
}
fn common_attrs(
    profile: VaultProfile,
    granularity: VaultGranularity,
    digest: &ContentId,
) -> BTreeMap<String, Expr> {
    BTreeMap::from([
        (
            "sim_profile".into(),
            Expr::String(profile.id.as_str().into()),
        ),
        (
            "sim_granularity".into(),
            Expr::String(granularity_name(granularity).into()),
        ),
        (
            "sim_projection_digest".into(),
            Expr::String(content_text(digest)),
        ),
        (
            "sim_compatibility_evidence".into(),
            Expr::String(profile.compatibility_evidence.into()),
        ),
    ])
}
fn heading(level: u8, text: &str) -> MarkupBlock {
    MarkupBlock::Heading {
        level,
        text: vec![Inline::Text(text.into())],
        id: None,
        span: None,
    }
}
fn row_block(row: &IndexRow) -> Result<MarkupBlock, VaultCodecError> {
    // Exhaustive matching is intentional: a new canonical family cannot be silently omitted.
    let family = row_family(row);
    let mut doc = IndexDoc::public("sim-codec-index-vault/row-v2");
    push_row(&mut doc, row.clone());
    let value = encode_index_expr(
        &expr_from_index_doc(&doc),
        sim_kernel::EncodePosition::Data,
        IndexForm::Json,
    )
    .map_err(|e| VaultCodecError::RowCodec(e.to_string()))?;
    Ok(MarkupBlock::CodeBlock {
        lang: Some(format!("sim-index-row-{family}")),
        code: value,
        span: None,
    })
}
fn row_family(row: &IndexRow) -> &'static str {
    match row {
        IndexRow::Subject(_) => "subject",
        IndexRow::Anchor(_) => "anchor",
        IndexRow::SourceUnit(_) => "source-unit",
        IndexRow::Declaration(_) => "declaration",
        IndexRow::ProtocolRelation(_) => "protocol-relation",
        IndexRow::Surface(_) => "surface",
        IndexRow::Specimen(_) => "specimen",
        IndexRow::Draft(_) => "draft",
        IndexRow::Feature(_) => "feature",
        IndexRow::Route(_) => "route",
        IndexRow::Edge(_) => "edge",
    }
}

fn push_row(doc: &mut IndexDoc, row: IndexRow) {
    match row {
        IndexRow::Subject(v) => doc.subjects.push(v),
        IndexRow::Anchor(v) => doc.anchors.push(v),
        IndexRow::SourceUnit(v) => doc.source_units.push(v),
        IndexRow::Declaration(v) => doc.declarations.push(v),
        IndexRow::ProtocolRelation(v) => doc.protocol_relations.push(v),
        IndexRow::Surface(v) => doc.surfaces.push(v),
        IndexRow::Specimen(v) => doc.specimens.push(v),
        IndexRow::Draft(v) => doc.drafts.push(v),
        IndexRow::Feature(v) => doc.features.push(v),
        IndexRow::Route(v) => doc.routes.push(v),
        IndexRow::Edge(v) => doc.edges.push(v),
    }
}

fn markdown_backend(profile: VaultProfile) -> Result<DialectMarkdownBackend, VaultCodecError> {
    DialectMarkdownBackend::new(MarkdownDialect {
        attributes: profile.attributes,
        links: profile.links,
        ..MarkdownDialect::default()
    })
    .map_err(VaultCodecError::Markup)
}

fn attr_text<'a>(doc: &'a MarkupDoc, key: &'static str) -> Result<&'a str, VaultCodecError> {
    match doc.attrs.get(key) {
        Some(Expr::String(v)) => Ok(v),
        Some(_) => Err(VaultCodecError::InvalidMetadata(key)),
        None => Err(VaultCodecError::MissingMetadata(key)),
    }
}
fn validate_metadata(
    doc: &MarkupDoc,
    profile: VaultProfile,
    bundle: &VaultBundle,
) -> Result<(), VaultCodecError> {
    if attr_text(doc, "sim_profile")? != profile.id.as_str() {
        return Err(VaultCodecError::ProfileDisagreement);
    }
    if attr_text(doc, "sim_granularity")? != granularity_name(bundle.granularity) {
        return Err(VaultCodecError::GranularityDisagreement);
    }
    if attr_text(doc, "sim_projection_digest")? != content_text(&bundle.projection_digest) {
        return Err(VaultCodecError::ProjectionDigest);
    }
    if attr_text(doc, "sim_compatibility_evidence")? != profile.compatibility_evidence {
        return Err(VaultCodecError::ProfileDisagreement);
    }
    Ok(())
}
fn plain(inlines: &[Inline]) -> Option<String> {
    let mut out = String::new();
    for inline in inlines {
        if let Inline::Text(v) = inline {
            out.push_str(v);
        } else {
            return None;
        }
    }
    Some(out)
}
fn validate_note_heading(doc: &MarkupDoc, id: &str) -> Result<(), VaultCodecError> {
    match doc.blocks.first() {
        Some(MarkupBlock::Heading { level: 1, text, .. }) if plain(text).as_deref() == Some(id) => {
            Ok(())
        }
        _ => Err(VaultCodecError::MisplacedPrimaryContent),
    }
}
fn validate_readme(doc: &MarkupDoc) -> Result<(), VaultCodecError> {
    match doc.blocks.first() {
        Some(MarkupBlock::Heading { level: 1, text, .. })
            if plain(text).as_deref() == Some("SIM Index Vault") =>
        {
            Ok(())
        }
        _ => Err(VaultCodecError::StrayNavigation),
    }
}
fn semantic_rows(doc: &MarkupDoc) -> Result<Vec<IndexRow>, VaultCodecError> {
    let mut rows = Vec::new();
    let mut expected_family = None;
    for block in doc.blocks.iter().skip(1) {
        match block {
            MarkupBlock::Heading { level: 2, text, .. } => {
                expected_family = Some(plain(text).ok_or(VaultCodecError::UnknownSection)?);
            }
            MarkupBlock::CodeBlock {
                lang: Some(lang),
                code,
                ..
            } if lang.starts_with("sim-index-row-") => {
                let heading_family = expected_family
                    .take()
                    .ok_or(VaultCodecError::MisplacedPrimaryContent)?;
                let expr = decode_index_expr(IndexForm::Json, code)
                    .map_err(|e| VaultCodecError::RowCodec(e.to_string()))?;
                let row_doc = index_doc_from_expr(&expr)
                    .map_err(|e| VaultCodecError::RowCodec(e.to_string()))?;
                let (_, inventory) = row_doc.inventory();
                if inventory.len() != 1 {
                    return Err(VaultCodecError::MisplacedPrimaryContent);
                }
                let row = inventory[0].to_owned();
                if lang.as_str() != format!("sim-index-row-{}", row_family(&row))
                    || heading_family != family_name(&row)
                {
                    return Err(VaultCodecError::UnknownSection);
                }
                rows.push(row);
            }
            MarkupBlock::List { .. } => {
                if expected_family.is_some() {
                    return Err(VaultCodecError::MisplacedPrimaryContent);
                }
            }
            _ => return Err(VaultCodecError::UnknownSection),
        }
    }
    if expected_family.is_some() {
        return Err(VaultCodecError::MisplacedPrimaryContent);
    }
    Ok(rows)
}
fn note_path_parts(kind: VaultNoteKind, id: &str) -> Result<String, VaultCodecError> {
    let note = VaultNotePlan {
        id: sim_index_vault_core::VaultNoteId::new(id),
        kind,
        rows: vec![],
    };
    note_path(&note)
}
fn family_name(row: &IndexRow) -> &'static str {
    match row {
        IndexRow::Subject(_) => "Subject",
        IndexRow::Anchor(_) => "Anchor",
        IndexRow::SourceUnit(_) => "Source unit",
        IndexRow::Declaration(_) => "Declaration",
        IndexRow::ProtocolRelation(_) => "Protocol relation",
        IndexRow::Surface(_) => "Surface",
        IndexRow::Specimen(_) => "Specimen",
        IndexRow::Draft(_) => "Draft",
        IndexRow::Feature(_) => "Feature",
        IndexRow::Route(_) => "Route",
        IndexRow::Edge(_) => "Edge",
    }
}
fn family_counts(note: &VaultNotePlan) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for row in &note.rows {
        *out.entry(family_name(row).into()).or_default() += 1;
    }
    out
}
fn kind_name(kind: VaultNoteKind) -> &'static str {
    match kind {
        VaultNoteKind::Index => "index",
        VaultNoteKind::Subject => "subject",
        VaultNoteKind::Anchor => "anchor",
        VaultNoteKind::Surface => "surface",
        VaultNoteKind::Specimen => "specimen",
        VaultNoteKind::Draft => "draft",
        VaultNoteKind::Feature => "feature",
        VaultNoteKind::Route => "route",
    }
}
fn kind_dir(kind: VaultNoteKind) -> &'static str {
    match kind {
        VaultNoteKind::Index => "index",
        VaultNoteKind::Subject => "subjects",
        VaultNoteKind::Anchor => "anchors",
        VaultNoteKind::Surface => "surfaces",
        VaultNoteKind::Specimen => "specimens",
        VaultNoteKind::Draft => "drafts",
        VaultNoteKind::Feature => "features",
        VaultNoteKind::Route => "routes",
    }
}
fn note_path(note: &VaultNotePlan) -> Result<String, VaultCodecError> {
    let id = note.id.as_str();
    if id.is_empty()
        || id.contains('~')
        || id.contains(['\\', '\0'])
        || id.split('/').any(|s| s.is_empty() || s == "." || s == "..")
    {
        return Err(VaultCodecError::InvalidPath(id.into()));
    }
    Ok(format!(
        "{}/{}.md",
        kind_dir(note.kind),
        id.replace('/', "~")
    ))
}
fn validate_paths(paths: &[String]) -> Result<(), VaultCodecError> {
    let mut exact = BTreeSet::new();
    let mut folded = BTreeSet::new();
    for path in paths {
        if path.starts_with('/')
            || path.contains("/../")
            || !exact.insert(path.clone())
            || !folded.insert(path.to_lowercase())
        {
            return Err(VaultCodecError::PathConflict(path.clone()));
        }
    }
    Ok(())
}
fn link_target(source: &str, target: &str, links: LinkDialect) -> String {
    match links {
        LinkDialect::WikiLink => format!("{ROOT}/{}", target.trim_end_matches(".md")),
        LinkDialect::CommonMark => {
            let depth = source.matches('/').count();
            format!("{}{}", "../".repeat(depth), target)
        }
    }
}
fn projection_digest(projection: &VaultProjection) -> ContentId {
    let mut h = Sha256::new();
    h.update(b"sim.index-vault.projection.v2\0");
    h.update(granularity_name(projection.granularity()));
    for note in projection.notes() {
        h.update(kind_name(note.kind));
        h.update([0]);
        h.update(note.id.as_str());
        for row in &note.rows {
            h.update([0]);
            h.update(format!("{row:#?}"));
        }
    }
    finish(h)
}
fn bundle_digest(entries: &[VaultEntry]) -> ContentId {
    let mut h = Sha256::new();
    h.update(b"sim.index-vault.bundle.v2\0");
    for entry in entries {
        h.update(entry.path.as_bytes());
        h.update([0]);
        h.update(entry.content_digest.bytes);
    }
    finish(h)
}
fn content_id(domain: &[u8], bytes: &[u8]) -> ContentId {
    let mut h = Sha256::new();
    h.update(domain);
    h.update(bytes);
    finish(h)
}
fn finish(h: Sha256) -> ContentId {
    ContentId::from_bytes(Symbol::qualified("core", "sha256"), h.finalize().into())
}
fn content_text(id: &ContentId) -> String {
    format!(
        "sha256:{}",
        id.bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
fn granularity_name(value: VaultGranularity) -> &'static str {
    match value {
        VaultGranularity::Compact => "compact",
        VaultGranularity::Full => "full",
    }
}

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
