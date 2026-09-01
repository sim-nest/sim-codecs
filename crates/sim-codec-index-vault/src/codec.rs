//! Deterministic vault bundle encoding and decoding.

use super::*;

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
pub(super) fn row_family(row: &IndexRow) -> &'static str {
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
