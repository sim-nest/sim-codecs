//! Conversion between `IndexDoc` records and the codec expression grammar.

use sim_index_core::{
    AnchorId, CanonicalFeatureKey, DiscoveredAnchor, DiscoveredSpecimen, DiscoveredSurface,
    FeatureDraft, FeatureId, FeatureRecord, GrammarContract, IndexDoc, IndexEdge, RouteId,
    RouteRecord, RouteStep, SpecimenId, SubjectId, SubjectRecord, SurfaceId, Visibility,
};
use sim_kernel::{Expr, Symbol};

use crate::CodecError;

/// Projects an index document to the single expression grammar used by all
/// index text forms.
pub fn expr_from_index_doc(doc: &IndexDoc) -> Expr {
    map(vec![
        field("schema", text(&doc.schema)),
        field("generated-by", text(&doc.generated_by)),
        field("visibility", text(visibility_name(doc.visibility))),
        field("subjects", list(doc.subjects.iter().map(subject_expr))),
        field("anchors", list(doc.anchors.iter().map(anchor_expr))),
        field("surfaces", list(doc.surfaces.iter().map(surface_expr))),
        field("specimens", list(doc.specimens.iter().map(specimen_expr))),
        field("drafts", list(doc.drafts.iter().map(draft_expr))),
        field("features", list(doc.features.iter().map(feature_expr))),
        field("routes", list(doc.routes.iter().map(route_expr))),
        field("edges", list(doc.edges.iter().map(edge_expr))),
    ])
}

/// Builds an index document from the checked index expression grammar.
pub fn index_doc_from_expr(expr: &Expr) -> Result<IndexDoc, CodecError> {
    let root = expect_map(expr, "index")?;
    Ok(IndexDoc {
        schema: string_field(root, "schema")?.to_owned(),
        generated_by: string_field(root, "generated-by")?.to_owned(),
        visibility: visibility_from_name(string_field(root, "visibility")?)?,
        subjects: map_list_field(root, "subjects", subject_from_expr)?,
        anchors: map_list_field(root, "anchors", anchor_from_expr)?,
        surfaces: map_list_field(root, "surfaces", surface_from_expr)?,
        specimens: map_list_field(root, "specimens", specimen_from_expr)?,
        drafts: map_list_field(root, "drafts", draft_from_expr)?,
        features: map_list_field(root, "features", feature_from_expr)?,
        routes: map_list_field(root, "routes", route_from_expr)?,
        edges: map_list_field(root, "edges", edge_from_expr)?,
    })
}

fn subject_expr(subject: &SubjectRecord) -> Expr {
    map(vec![
        field("id", text(subject.id.as_str())),
        field("kind", text(&subject.kind)),
        field("title", text(&subject.title)),
    ])
}

fn anchor_expr(anchor: &DiscoveredAnchor) -> Expr {
    map(vec![
        field("id", text(anchor.id.as_str())),
        field("subject", text(anchor.subject.as_str())),
        field("kind", text(&anchor.kind)),
    ])
}

fn surface_expr(surface: &DiscoveredSurface) -> Expr {
    map(vec![
        field("id", text(surface.id.as_str())),
        field("subject", text(surface.subject.as_str())),
        field("kind", text(&surface.kind)),
    ])
}

fn specimen_expr(specimen: &DiscoveredSpecimen) -> Expr {
    map(vec![
        field("id", text(specimen.id.as_str())),
        field("subject", text(specimen.subject.as_str())),
        field("kind", text(&specimen.kind)),
        field("runnable", Expr::Bool(specimen.runnable)),
        field("checked", Expr::Bool(specimen.checked)),
        field("doc-anchor", optional_id(specimen.doc_anchor.as_ref())),
    ])
}

fn draft_expr(draft: &FeatureDraft) -> Expr {
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

fn feature_expr(feature: &FeatureRecord) -> Expr {
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

fn grammar_expr(grammar: &GrammarContract) -> Expr {
    map(vec![
        field("id", text(&grammar.id)),
        field("decoder", optional_id(grammar.decoder.as_ref())),
        field("encoder", optional_id(grammar.encoder.as_ref())),
        field("surface", optional_id(grammar.surface.as_ref())),
        field("round-trip", Expr::Bool(grammar.round_trip)),
    ])
}

fn route_expr(route: &RouteRecord) -> Expr {
    map(vec![
        field("id", text(route.id.as_str())),
        field("title", text(&route.title)),
        field("steps", list(route.steps.iter().map(step_expr))),
        field("doc-anchor", optional_id(route.doc_anchor.as_ref())),
    ])
}

fn step_expr(step: &RouteStep) -> Expr {
    match step {
        RouteStep::Feature(id) => map(vec![
            field("kind", text("feature")),
            field("id", text(id.as_str())),
        ]),
        RouteStep::Specimen(id) => map(vec![
            field("kind", text("specimen")),
            field("id", text(id.as_str())),
        ]),
    }
}

fn edge_expr(edge: &IndexEdge) -> Expr {
    map(vec![
        field("from", text(edge.from.as_str())),
        field("predicate", text(&edge.predicate)),
        field("to", text(edge.to.as_str())),
    ])
}

fn subject_from_expr(expr: &Expr) -> Result<SubjectRecord, CodecError> {
    let entries = expect_map(expr, "subject")?;
    Ok(SubjectRecord {
        id: SubjectId::new(string_field(entries, "id")?),
        kind: string_field(entries, "kind")?.to_owned(),
        title: string_field(entries, "title")?.to_owned(),
    })
}

fn anchor_from_expr(expr: &Expr) -> Result<DiscoveredAnchor, CodecError> {
    let entries = expect_map(expr, "anchor")?;
    Ok(DiscoveredAnchor {
        id: AnchorId::new(string_field(entries, "id")?),
        subject: SubjectId::new(string_field(entries, "subject")?),
        kind: string_field(entries, "kind")?.to_owned(),
    })
}

fn surface_from_expr(expr: &Expr) -> Result<DiscoveredSurface, CodecError> {
    let entries = expect_map(expr, "surface")?;
    Ok(DiscoveredSurface {
        id: SurfaceId::new(string_field(entries, "id")?),
        subject: SubjectId::new(string_field(entries, "subject")?),
        kind: string_field(entries, "kind")?.to_owned(),
    })
}

fn specimen_from_expr(expr: &Expr) -> Result<DiscoveredSpecimen, CodecError> {
    let entries = expect_map(expr, "specimen")?;
    Ok(DiscoveredSpecimen {
        id: SpecimenId::new(string_field(entries, "id")?),
        subject: SubjectId::new(string_field(entries, "subject")?),
        kind: string_field(entries, "kind")?.to_owned(),
        runnable: bool_field(entries, "runnable")?,
        checked: bool_field(entries, "checked")?,
        doc_anchor: optional_anchor(entries, "doc-anchor")?,
    })
}

fn draft_from_expr(expr: &Expr) -> Result<FeatureDraft, CodecError> {
    let entries = expect_map(expr, "draft")?;
    Ok(FeatureDraft {
        id: FeatureId::new(string_field(entries, "id")?),
        subject: SubjectId::new(string_field(entries, "subject")?),
        title: string_field(entries, "title")?.to_owned(),
        summary: string_field(entries, "summary")?.to_owned(),
        claims_anchors: id_list(entries, "claims-anchors", |id| AnchorId::new(id))?,
        claims_surfaces: id_list(entries, "claims-surfaces", |id| SurfaceId::new(id))?,
        claims_specimens: id_list(entries, "claims-specimens", |id| SpecimenId::new(id))?,
        literal_anchors: string_list_field(entries, "literal-anchors")?,
        literal_surfaces: string_list_field(entries, "literal-surfaces")?,
        literal_specimens: string_list_field(entries, "literal-specimens")?,
        grammar_contracts: map_list_field(entries, "grammar-contracts", grammar_from_expr)?,
        doc_anchor: optional_anchor(entries, "doc-anchor")?,
    })
}

fn feature_from_expr(expr: &Expr) -> Result<FeatureRecord, CodecError> {
    let entries = expect_map(expr, "feature")?;
    Ok(FeatureRecord {
        id: FeatureId::new(string_field(entries, "id")?),
        key: CanonicalFeatureKey::new(string_field(entries, "key")?),
        subject: SubjectId::new(string_field(entries, "subject")?),
        title: string_field(entries, "title")?.to_owned(),
        summary: string_field(entries, "summary")?.to_owned(),
        anchors: id_list(entries, "anchors", |id| AnchorId::new(id))?,
        surfaces: id_list(entries, "surfaces", |id| SurfaceId::new(id))?,
        specimens: id_list(entries, "specimens", |id| SpecimenId::new(id))?,
        grammar_contracts: map_list_field(entries, "grammar-contracts", grammar_from_expr)?,
        doc_anchor: optional_anchor(entries, "doc-anchor")?,
    })
}

fn grammar_from_expr(expr: &Expr) -> Result<GrammarContract, CodecError> {
    let entries = expect_map(expr, "grammar contract")?;
    Ok(GrammarContract {
        id: string_field(entries, "id")?.to_owned(),
        decoder: optional_anchor(entries, "decoder")?,
        encoder: optional_anchor(entries, "encoder")?,
        surface: optional_surface(entries, "surface")?,
        round_trip: bool_field(entries, "round-trip")?,
    })
}

fn route_from_expr(expr: &Expr) -> Result<RouteRecord, CodecError> {
    let entries = expect_map(expr, "route")?;
    Ok(RouteRecord {
        id: RouteId::new(string_field(entries, "id")?),
        title: string_field(entries, "title")?.to_owned(),
        steps: map_list_field(entries, "steps", step_from_expr)?,
        doc_anchor: optional_anchor(entries, "doc-anchor")?,
    })
}

fn step_from_expr(expr: &Expr) -> Result<RouteStep, CodecError> {
    let entries = expect_map(expr, "route step")?;
    match string_field(entries, "kind")? {
        "feature" => Ok(RouteStep::Feature(FeatureId::new(string_field(
            entries, "id",
        )?))),
        "specimen" => Ok(RouteStep::Specimen(SpecimenId::new(string_field(
            entries, "id",
        )?))),
        other => Err(CodecError::Shape(format!(
            "unsupported route step kind {other:?}"
        ))),
    }
}

fn edge_from_expr(expr: &Expr) -> Result<IndexEdge, CodecError> {
    let entries = expect_map(expr, "edge")?;
    Ok(IndexEdge {
        from: FeatureId::new(string_field(entries, "from")?),
        predicate: string_field(entries, "predicate")?.to_owned(),
        to: FeatureId::new(string_field(entries, "to")?),
    })
}

fn expect_map<'a>(expr: &'a Expr, label: &str) -> Result<&'a [(Expr, Expr)], CodecError> {
    match expr {
        Expr::Map(entries) => Ok(entries),
        other => Err(CodecError::Shape(format!(
            "{label} must be a map, found {other:?}"
        ))),
    }
}

fn map_list_field<T>(
    entries: &[(Expr, Expr)],
    name: &str,
    read: fn(&Expr) -> Result<T, CodecError>,
) -> Result<Vec<T>, CodecError> {
    let Expr::List(items) = required(entries, name)? else {
        return Err(CodecError::Shape(format!("{name} must be a list")));
    };
    items.iter().map(read).collect()
}

fn id_list<T>(
    entries: &[(Expr, Expr)],
    name: &str,
    build: impl FnMut(&str) -> T,
) -> Result<Vec<T>, CodecError> {
    Ok(string_list(entries, name)?.map(build).collect())
}

fn string_list_field(entries: &[(Expr, Expr)], name: &str) -> Result<Vec<String>, CodecError> {
    Ok(string_list(entries, name)?.map(str::to_owned).collect())
}

fn string_list<'a>(
    entries: &'a [(Expr, Expr)],
    name: &str,
) -> Result<impl Iterator<Item = &'a str>, CodecError> {
    let Expr::List(items) = required(entries, name)? else {
        return Err(CodecError::Shape(format!("{name} must be a list")));
    };
    items
        .iter()
        .map(|expr| match expr {
            Expr::String(value) => Ok(value.as_str()),
            other => Err(CodecError::Shape(format!(
                "{name} item must be a string, found {other:?}"
            ))),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_iter)
}

fn string_field<'a>(entries: &'a [(Expr, Expr)], name: &str) -> Result<&'a str, CodecError> {
    match required(entries, name)? {
        Expr::String(value) => Ok(value),
        other => Err(CodecError::Shape(format!(
            "{name} must be a string, found {other:?}"
        ))),
    }
}

fn bool_field(entries: &[(Expr, Expr)], name: &str) -> Result<bool, CodecError> {
    match required(entries, name)? {
        Expr::Bool(value) => Ok(*value),
        other => Err(CodecError::Shape(format!(
            "{name} must be a bool, found {other:?}"
        ))),
    }
}

fn optional_anchor(entries: &[(Expr, Expr)], name: &str) -> Result<Option<AnchorId>, CodecError> {
    optional_id_from_field(entries, name).map(|value| value.map(AnchorId::new))
}

fn optional_surface(entries: &[(Expr, Expr)], name: &str) -> Result<Option<SurfaceId>, CodecError> {
    optional_id_from_field(entries, name).map(|value| value.map(SurfaceId::new))
}

fn optional_id_from_field<'a>(
    entries: &'a [(Expr, Expr)],
    name: &str,
) -> Result<Option<&'a str>, CodecError> {
    match required(entries, name)? {
        Expr::Nil => Ok(None),
        Expr::String(value) => Ok(Some(value)),
        other => Err(CodecError::Shape(format!(
            "{name} must be nil or a string, found {other:?}"
        ))),
    }
}

fn required<'a>(entries: &'a [(Expr, Expr)], name: &str) -> Result<&'a Expr, CodecError> {
    entries
        .iter()
        .find_map(|(key, value)| (symbol_name(key) == Some(name)).then_some(value))
        .ok_or_else(|| CodecError::Shape(format!("missing field {name}")))
}

fn symbol_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Symbol(symbol) if symbol.namespace.is_none() => Some(symbol.name.as_ref()),
        _ => None,
    }
}

fn visibility_name(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "public",
        Visibility::PrivateLocal => "private-local",
    }
}

fn visibility_from_name(name: &str) -> Result<Visibility, CodecError> {
    match name {
        "public" => Ok(Visibility::Public),
        "private-local" => Ok(Visibility::PrivateLocal),
        other => Err(CodecError::Shape(format!(
            "unsupported visibility {other:?}"
        ))),
    }
}

fn field(name: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(name.to_owned())), value)
}

fn map(entries: Vec<(Expr, Expr)>) -> Expr {
    Expr::Map(entries)
}

fn list(items: impl Iterator<Item = Expr>) -> Expr {
    Expr::List(items.collect())
}

fn ids<'a, T>(items: impl Iterator<Item = &'a T>) -> Expr
where
    T: ToString + 'a,
{
    list(items.map(|item| text(item.to_string())))
}

fn strings<'a>(items: impl Iterator<Item = &'a String>) -> Expr {
    list(items.map(text))
}

fn optional_id<T: ToString>(value: Option<&T>) -> Expr {
    value.map_or(Expr::Nil, |id| text(id.to_string()))
}

fn text(value: impl AsRef<str>) -> Expr {
    Expr::String(value.as_ref().to_owned())
}
