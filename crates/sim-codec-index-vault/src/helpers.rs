//! Shared semantic, path, and digest helpers.

use super::*;

pub(super) fn markdown_backend(
    profile: VaultProfile,
) -> Result<DialectMarkdownBackend, VaultCodecError> {
    DialectMarkdownBackend::new(MarkdownDialect {
        attributes: profile.attributes,
        links: profile.links,
        ..MarkdownDialect::default()
    })
    .map_err(VaultCodecError::Markup)
}

pub(super) fn attr_text<'a>(
    doc: &'a MarkupDoc,
    key: &'static str,
) -> Result<&'a str, VaultCodecError> {
    match doc.attrs.get(key) {
        Some(Expr::String(v)) => Ok(v),
        Some(_) => Err(VaultCodecError::InvalidMetadata(key)),
        None => Err(VaultCodecError::MissingMetadata(key)),
    }
}
pub(super) fn validate_metadata(
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
pub(super) fn plain(inlines: &[Inline]) -> Option<String> {
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
pub(super) fn validate_note_heading(doc: &MarkupDoc, id: &str) -> Result<(), VaultCodecError> {
    match doc.blocks.first() {
        Some(MarkupBlock::Heading { level: 1, text, .. }) if plain(text).as_deref() == Some(id) => {
            Ok(())
        }
        _ => Err(VaultCodecError::MisplacedPrimaryContent),
    }
}
pub(super) fn validate_readme(doc: &MarkupDoc) -> Result<(), VaultCodecError> {
    match doc.blocks.first() {
        Some(MarkupBlock::Heading { level: 1, text, .. })
            if plain(text).as_deref() == Some("SIM Index Vault") =>
        {
            Ok(())
        }
        _ => Err(VaultCodecError::StrayNavigation),
    }
}
pub(super) fn semantic_rows(doc: &MarkupDoc) -> Result<Vec<IndexRow>, VaultCodecError> {
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
pub(super) fn note_path_parts(kind: VaultNoteKind, id: &str) -> Result<String, VaultCodecError> {
    let note = VaultNotePlan {
        id: sim_index_vault_core::VaultNoteId::new(id),
        kind,
        rows: vec![],
    };
    note_path(&note)
}
pub(super) fn family_name(row: &IndexRow) -> &'static str {
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
pub(super) fn family_counts(note: &VaultNotePlan) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for row in &note.rows {
        *out.entry(family_name(row).into()).or_default() += 1;
    }
    out
}
pub(super) fn kind_name(kind: VaultNoteKind) -> &'static str {
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
pub(super) fn kind_dir(kind: VaultNoteKind) -> &'static str {
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
pub(super) fn note_path(note: &VaultNotePlan) -> Result<String, VaultCodecError> {
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
pub(super) fn validate_paths(paths: &[String]) -> Result<(), VaultCodecError> {
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
pub(super) fn link_target(source: &str, target: &str, links: LinkDialect) -> String {
    match links {
        LinkDialect::WikiLink => format!("{ROOT}/{}", target.trim_end_matches(".md")),
        LinkDialect::CommonMark => {
            let depth = source.matches('/').count();
            format!("{}{}", "../".repeat(depth), target)
        }
    }
}
pub(super) fn projection_digest(projection: &VaultProjection) -> ContentId {
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
pub(super) fn bundle_digest(entries: &[VaultEntry]) -> ContentId {
    let mut h = Sha256::new();
    h.update(b"sim.index-vault.bundle.v2\0");
    for entry in entries {
        h.update(entry.path.as_bytes());
        h.update([0]);
        h.update(entry.content_digest.bytes);
    }
    finish(h)
}
pub(super) fn content_id(domain: &[u8], bytes: &[u8]) -> ContentId {
    let mut h = Sha256::new();
    h.update(domain);
    h.update(bytes);
    finish(h)
}
pub(super) fn finish(h: Sha256) -> ContentId {
    ContentId::from_bytes(Symbol::qualified("core", "sha256"), h.finalize().into())
}
pub(super) fn content_text(id: &ContentId) -> String {
    format!(
        "sha256:{}",
        id.bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
pub(super) fn granularity_name(value: VaultGranularity) -> &'static str {
    match value {
        VaultGranularity::Compact => "compact",
        VaultGranularity::Full => "full",
    }
}
