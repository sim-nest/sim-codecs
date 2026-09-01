//! Expression projection for SIM Index records.

use super::*;

pub(super) fn source_unit_expr(unit: &SourceUnit) -> Expr {
    map(vec![
        field("subject", text(unit.subject.as_str())),
        field("path", text(&unit.path)),
        field(
            "reachability",
            text(match unit.reachability {
                SourceReachability::Reachable => "reachable",
                SourceReachability::Unreachable => "unreachable",
            }),
        ),
        field("completeness", text(unit.completeness.as_str())),
        field("reason", text(&unit.reason)),
        field("retained-bound", syntax_bound_expr(unit.retained_bound)),
        field("declaration-count", unsigned(unit.declaration_count)),
    ])
}

pub(super) fn source_unit_from_expr(expr: &Expr) -> Result<SourceUnit, CodecError> {
    let entries = expect_map(expr, "source unit")?;
    let reachability = match string_field(entries, "reachability")? {
        "reachable" => SourceReachability::Reachable,
        "unreachable" => SourceReachability::Unreachable,
        other => {
            return Err(CodecError::Shape(format!(
                "unsupported source reachability {other:?}"
            )));
        }
    };
    let completeness = match string_field(entries, "completeness")? {
        "complete" => SourceCompleteness::Complete,
        "malformed" => SourceCompleteness::Malformed,
        "unreadable" => SourceCompleteness::Unreadable,
        "truncated" => SourceCompleteness::Truncated,
        "unsupported" => SourceCompleteness::Unsupported,
        "unresolved" => SourceCompleteness::Unresolved,
        other => {
            return Err(CodecError::Shape(format!(
                "unsupported source completeness {other:?}"
            )));
        }
    };
    Ok(SourceUnit {
        subject: SubjectId::new(string_field(entries, "subject")?),
        path: string_field(entries, "path")?.to_owned(),
        reachability,
        completeness,
        reason: string_field(entries, "reason")?.to_owned(),
        retained_bound: syntax_bound_from_expr(required(entries, "retained-bound")?)?,
        declaration_count: usize_field(entries, "declaration-count")?,
    })
}

pub(super) fn declaration_expr(fact: &DeclarationFact) -> Expr {
    map(vec![
        field("anchor", text(fact.anchor.as_str())),
        field("role", text(fact.role.as_str())),
        field("module-path", text(&fact.module_path)),
        field("generics", text(&fact.generics)),
        field("members", strings(fact.members.iter())),
        field("location", source_location_expr(&fact.location)),
        field("syntax-bound", syntax_bound_expr(fact.syntax_bound)),
    ])
}

pub(super) fn source_location_expr(location: &SourceLocation) -> Expr {
    map(vec![
        field("file", text(&location.file)),
        field("declaration", unsigned(location.declaration)),
    ])
}

pub(super) fn syntax_bound_expr(bound: SyntaxBound) -> Expr {
    map(vec![
        field("max-bytes", unsigned(bound.max_bytes)),
        field("truncated", Expr::Bool(bound.truncated)),
    ])
}

pub(super) fn protocol_relation_expr(relation: &ProtocolRelation) -> Expr {
    map(vec![
        field("anchor", text(relation.anchor.as_str())),
        field("implementor", text(&relation.implementor)),
        field("source-spelling", text(&relation.source_spelling)),
        field("body-fingerprint", text(&relation.body_fingerprint)),
        field("body-bound", syntax_bound_expr(relation.body_bound)),
        field("resolution", resolution_expr(&relation.resolution)),
    ])
}

pub(super) fn resolution_expr(resolution: &ProtocolResolution) -> Expr {
    match resolution {
        ProtocolResolution::Resolved { protocol } => map(vec![
            field("state", text("resolved")),
            field("protocol", text(protocol)),
        ]),
        ProtocolResolution::Unresolved { reason, candidates } => map(vec![
            field("state", text("unresolved")),
            field("reason", text(unresolved_reason_name(*reason))),
            field("candidates", strings(candidates.iter())),
        ]),
    }
}

pub(super) fn subject_expr(subject: &SubjectRecord) -> Expr {
    map(vec![
        field("id", text(subject.id.as_str())),
        field("kind", text(&subject.kind)),
        field("title", text(&subject.title)),
    ])
}

pub(super) fn anchor_expr(anchor: &DiscoveredAnchor) -> Expr {
    map(vec![
        field("id", text(anchor.id.as_str())),
        field("subject", text(anchor.subject.as_str())),
        field("kind", text(&anchor.kind)),
    ])
}

pub(super) fn surface_expr(surface: &DiscoveredSurface) -> Expr {
    map(vec![
        field("id", text(surface.id.as_str())),
        field("subject", text(surface.subject.as_str())),
        field("kind", text(&surface.kind)),
    ])
}

pub(super) fn specimen_expr(specimen: &DiscoveredSpecimen) -> Expr {
    map(vec![
        field("id", text(specimen.id.as_str())),
        field("subject", text(specimen.subject.as_str())),
        field("kind", text(&specimen.kind)),
        field("path", text(&specimen.path)),
        field("language", optional_text(specimen.language.as_ref())),
        field("runnable", Expr::Bool(specimen.runnable)),
        field("checked", Expr::Bool(specimen.checked)),
        field("checked-by", optional_text(specimen.checked_by.as_ref())),
        field("doc-anchor", optional_id(specimen.doc_anchor.as_ref())),
    ])
}

pub(super) fn draft_expr(draft: &FeatureDraft) -> Expr {
    map(vec![
        field("id", text(draft.id.as_str())),
        field("subject", text(draft.subject.as_str())),
        field("title", text(&draft.title)),
        field("summary", text(&draft.summary)),
        field("claims-anchors", ids(draft.claims_anchors.iter())),
        field("claims-surfaces", ids(draft.claims_surfaces.iter())),
        field("claims-specimens", ids(draft.claims_specimens.iter())),
        field("literal-anchors", strings(draft.literal_anchors.iter())),
        field("literal-surfaces", strings(draft.literal_surfaces.iter())),
        field("literal-specimens", strings(draft.literal_specimens.iter())),
        field(
            "grammar-contracts",
            list(draft.grammar_contracts.iter().map(grammar_expr)),
        ),
        field("doc-anchor", optional_id(draft.doc_anchor.as_ref())),
    ])
}

pub(super) fn feature_expr(feature: &FeatureRecord) -> Expr {
    map(vec![
        field("id", text(feature.id.as_str())),
        field("key", text(feature.key.as_str())),
        field("subject", text(feature.subject.as_str())),
        field("title", text(&feature.title)),
        field("summary", text(&feature.summary)),
        field("anchors", ids(feature.anchors.iter())),
        field("surfaces", ids(feature.surfaces.iter())),
        field("specimens", ids(feature.specimens.iter())),
        field(
            "grammar-contracts",
            list(feature.grammar_contracts.iter().map(grammar_expr)),
        ),
        field("doc-anchor", optional_id(feature.doc_anchor.as_ref())),
    ])
}

pub(super) fn grammar_expr(grammar: &GrammarContract) -> Expr {
    map(vec![
        field("id", text(&grammar.id)),
        field("decoder", optional_id(grammar.decoder.as_ref())),
        field("encoder", optional_id(grammar.encoder.as_ref())),
        field("surface", optional_id(grammar.surface.as_ref())),
        field("round-trip", Expr::Bool(grammar.round_trip)),
    ])
}

pub(super) fn route_expr(route: &RouteRecord) -> Expr {
    map(vec![
        field("id", text(route.id.as_str())),
        field("title", text(&route.title)),
        field("audiences", strings(route.audiences.iter())),
        field("steps", list(route.steps.iter().map(step_expr))),
        field("doc-anchor", optional_id(route.doc_anchor.as_ref())),
    ])
}

pub(super) fn step_expr(step: &RouteStep) -> Expr {
    match step {
        RouteStep::Feature { id, why } => map(vec![
            field("kind", text("feature")),
            field("id", text(id.as_str())),
            field("why", text(why)),
        ]),
        RouteStep::Specimen { id, why } => map(vec![
            field("kind", text("specimen")),
            field("id", text(id.as_str())),
            field("why", text(why)),
        ]),
    }
}

pub(super) fn edge_expr(edge: &IndexEdge) -> Expr {
    map(vec![
        field("from", text(&edge.from)),
        field("rel", text(&edge.rel)),
        field("to", text(&edge.to)),
    ])
}
