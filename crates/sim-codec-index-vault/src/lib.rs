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
    MarkupBlock, MarkupDoc, MarkupEncodeOptions,
};
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
        blocks.push(row_block(row));
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
fn row_block(row: &IndexRow) -> MarkupBlock {
    // Exhaustive matching is intentional: a new canonical family cannot be silently omitted.
    let (family, value) = match row {
        IndexRow::Subject(v) => ("subject", format!("{v:#?}")),
        IndexRow::Anchor(v) => ("anchor", format!("{v:#?}")),
        IndexRow::SourceUnit(v) => ("source-unit", format!("{v:#?}")),
        IndexRow::Declaration(v) => ("declaration", format!("{v:#?}")),
        IndexRow::ProtocolRelation(v) => ("protocol-relation", format!("{v:#?}")),
        IndexRow::Surface(v) => ("surface", format!("{v:#?}")),
        IndexRow::Specimen(v) => ("specimen", format!("{v:#?}")),
        IndexRow::Draft(v) => ("draft", format!("{v:#?}")),
        IndexRow::Feature(v) => ("feature", format!("{v:#?}")),
        IndexRow::Route(v) => ("route", format!("{v:#?}")),
        IndexRow::Edge(v) => ("edge", format!("{v:#?}")),
    };
    MarkupBlock::CodeBlock {
        lang: Some(format!("sim-index-row-{family}")),
        code: value,
        span: None,
    }
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
        }
    }
}
impl Error for VaultCodecError {}
