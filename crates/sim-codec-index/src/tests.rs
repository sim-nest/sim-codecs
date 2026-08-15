use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_index_core::{
    AnchorId, DeclarationFact, DeclarationRole, DiscoveredAnchor, DiscoveredSpecimen,
    DiscoveredSurface, FeatureId, FeatureRecord, GrammarContract, IndexDoc, IndexEdge, IndexError,
    ProtocolRelation, ProtocolResolution, RouteId, RouteRecord, RouteStep, SourceLocation,
    SpecimenId, SubjectId, SubjectRecord, SurfaceId, SyntaxBound, UnresolvedReason, Visibility,
    key::canonical_feature_key,
};
use sim_kernel::{EncodeOptions, EncodePosition, Expr, ReadPolicy, Symbol};

use crate::{
    CodecError, IndexCodec, IndexCodecLib, IndexForm, encode_index_expr, expr_from_index_doc,
    index_doc_from_expr, index_shape,
};

fn valid_doc() -> IndexDoc {
    let repo_subject = SubjectId::new("repo/sim-run");
    let subject = SubjectId::new("crate/sim-run");
    let feature_id = FeatureId::new("feature/sim-run/repl");
    IndexDoc {
        schema: "sim.index".to_owned(),
        generated_by: "sim-codec-index-tests".to_owned(),
        visibility: Visibility::Public,
        subjects: vec![
            SubjectRecord {
                id: repo_subject.clone(),
                kind: "repo".to_owned(),
                title: "sim-run".to_owned(),
            },
            SubjectRecord {
                id: subject.clone(),
                kind: "crate".to_owned(),
                title: "sim-run".to_owned(),
            },
        ],
        anchors: vec![
            DiscoveredAnchor {
                id: AnchorId::new("export/sim-run/repl"),
                subject: subject.clone(),
                kind: "export".to_owned(),
            },
            DiscoveredAnchor {
                id: AnchorId::new("doc/sim-run/repl"),
                subject: subject.clone(),
                kind: "doc".to_owned(),
            },
        ],
        declarations: Vec::new(),
        protocol_relations: Vec::new(),
        surfaces: vec![DiscoveredSurface {
            id: SurfaceId::new("cli/repl"),
            subject: subject.clone(),
            kind: "cli".to_owned(),
        }],
        specimens: vec![DiscoveredSpecimen {
            id: SpecimenId::new("recipe/sim-run/repl"),
            subject: subject.clone(),
            kind: "recipe".to_owned(),
            path: "recipes/01-basics/repl/recipe.toml".to_owned(),
            language: Some("cli-transcript".to_owned()),
            runnable: true,
            checked: true,
            checked_by: Some("xtask check-recipes".to_owned()),
            doc_anchor: Some(AnchorId::new("doc/sim-run/repl")),
        }],
        drafts: Vec::new(),
        features: vec![FeatureRecord {
            id: feature_id.clone(),
            key: canonical_feature_key(&subject, feature_id.as_str()),
            subject: subject.clone(),
            title: "REPL".to_owned(),
            summary: "Interactive command loop for SIM sessions.".to_owned(),
            anchors: vec![AnchorId::new("export/sim-run/repl")],
            surfaces: vec![SurfaceId::new("cli/repl")],
            specimens: vec![SpecimenId::new("recipe/sim-run/repl")],
            grammar_contracts: vec![GrammarContract {
                id: "grammar/repl".to_owned(),
                decoder: Some(AnchorId::new("export/sim-run/repl")),
                encoder: None,
                surface: Some(SurfaceId::new("cli/repl")),
                round_trip: true,
            }],
            doc_anchor: Some(AnchorId::new("doc/sim-run/repl")),
        }],
        routes: vec![RouteRecord {
            id: RouteId::new("route/use-repl"),
            title: "Use the REPL".to_owned(),
            audiences: vec!["user".to_owned(), "code".to_owned()],
            steps: vec![
                RouteStep::Feature {
                    id: feature_id.clone(),
                    why: "Start with the interactive entry point.".to_owned(),
                },
                RouteStep::Specimen {
                    id: SpecimenId::new("recipe/sim-run/repl"),
                    why: "Run the checked REPL recipe.".to_owned(),
                },
            ],
            doc_anchor: Some(AnchorId::new("doc/sim-run/repl")),
        }],
        edges: vec![
            IndexEdge::relates(feature_id.clone(), "supports", feature_id),
            IndexEdge::contains(repo_subject, subject),
        ],
    }
}

fn source_fact_doc() -> IndexDoc {
    let mut doc = valid_doc();
    doc.declarations.push(DeclarationFact {
        anchor: AnchorId::new("export/sim-run/repl"),
        role: DeclarationRole::Struct,
        module_path: "repl".to_owned(),
        generics: "<T>".to_owned(),
        members: vec!["value:T".to_owned()],
        location: SourceLocation {
            file: "src/repl.rs".to_owned(),
            declaration: 3,
        },
        syntax_bound: SyntaxBound {
            max_bytes: 64,
            truncated: false,
        },
    });
    doc.protocol_relations.push(ProtocolRelation {
        anchor: AnchorId::new("export/sim-run/repl"),
        implementor: "Repl".to_owned(),
        source_spelling: "Function".to_owned(),
        body_fingerprint: "call(&self)".to_owned(),
        body_bound: SyntaxBound {
            max_bytes: 64,
            truncated: false,
        },
        resolution: ProtocolResolution::Unresolved {
            reason: UnresolvedReason::ExternalMetadataAbsent,
            candidates: Vec::new(),
        },
    });
    doc
}

fn large_doc() -> IndexDoc {
    let mut doc = IndexDoc::public("sim-codec-index-tests/large");
    doc.subjects = (0..20_000)
        .map(|index| SubjectRecord {
            id: SubjectId::new(format!("crate/subject-{index:05}")),
            kind: "crate".to_owned(),
            title: format!("subject-{index:05}"),
        })
        .collect();
    doc
}

#[test]
fn roundtrip_sx_and_json_include_specimens() {
    let codec = IndexCodec;
    let doc = valid_doc();
    let sx = codec
        .encode(&doc, EncodePosition::Data, IndexForm::Sx)
        .expect("encode sx");
    let json = codec
        .encode(&doc, EncodePosition::Data, IndexForm::Json)
        .expect("encode json");

    let from_sx = codec.decode(IndexForm::Sx, &sx).expect("decode sx");
    let from_json = codec.decode(IndexForm::Json, &json).expect("decode json");

    assert_eq!(from_sx, doc);
    assert_eq!(from_json, doc);
    assert_eq!(from_sx.specimens[0].id.as_str(), "recipe/sim-run/repl");
    assert_eq!(
        from_sx.specimens[0].path,
        "recipes/01-basics/repl/recipe.toml"
    );
    assert_eq!(
        from_sx.specimens[0].checked_by.as_deref(),
        Some("xtask check-recipes")
    );
    assert!(sx.contains("specimens"));
    assert!(sx.contains("checked-by"));
    assert!(sx.contains("audiences"));
    assert!(sx.contains("why"));
    assert!(json.contains("specimens"));
    assert!(json.contains("audiences"));
}

#[test]
fn source_facts_roundtrip_canonically_in_both_forms() {
    let codec = IndexCodec;
    let doc = source_fact_doc();
    for form in [IndexForm::Sx, IndexForm::Json] {
        let first = codec
            .encode(&doc, EncodePosition::Data, form)
            .expect("encode source facts");
        let decoded = codec.decode(form, &first).expect("decode source facts");
        let second = codec
            .encode(&decoded, EncodePosition::Data, form)
            .expect("re-encode source facts");
        assert_eq!(decoded, doc);
        assert_eq!(second, first);
    }
}

#[test]
fn old_graph_encoding_is_byte_identical() {
    let codec = IndexCodec;
    let source = codec
        .encode(&valid_doc(), EncodePosition::Data, IndexForm::Sx)
        .expect("encode old graph");
    assert!(!source.contains("declarations"));
    assert!(!source.contains("protocol-relations"));
    assert_eq!(
        codec
            .encode(
                &codec
                    .decode(IndexForm::Sx, &source)
                    .expect("decode old graph"),
                EncodePosition::Data,
                IndexForm::Sx,
            )
            .expect("re-encode old graph"),
        source
    );
}

#[test]
fn malformed_source_fact_variants_and_fields_fail_with_location() {
    let mut expr = expr_from_index_doc(&source_fact_doc());
    let Expr::Map(root) = &mut expr else {
        panic!("root map")
    };
    let (_, Expr::List(relations)) = root
        .iter_mut()
        .find(|(key, _)| matches!(key, Expr::Symbol(symbol) if symbol.name.as_ref() == "protocol-relations"))
        .expect("relations")
    else { panic!("relation list") };
    let Expr::Map(relation) = &mut relations[0] else {
        panic!("relation map")
    };
    let (_, Expr::Map(resolution)) = relation
        .iter_mut()
        .find(
            |(key, _)| matches!(key, Expr::Symbol(symbol) if symbol.name.as_ref() == "resolution"),
        )
        .expect("resolution")
    else {
        panic!("resolution map")
    };
    resolution.push(resolution[0].clone());
    assert!(matches!(
        index_shape().check(&expr),
        Err(CodecError::Shape(message)) if message.contains("protocol resolution repeats field state")
    ));

    let mut unknown_variant = expr_from_index_doc(&source_fact_doc());
    let Expr::Map(root) = &mut unknown_variant else {
        panic!("root map")
    };
    let (_, Expr::List(relations)) = root
        .iter_mut()
        .find(|(key, _)| matches!(key, Expr::Symbol(symbol) if symbol.name.as_ref() == "protocol-relations"))
        .expect("relations")
    else { panic!("relation list") };
    let Expr::Map(relation) = &mut relations[0] else {
        panic!("relation map")
    };
    let (_, Expr::Map(resolution)) = relation
        .iter_mut()
        .find(
            |(key, _)| matches!(key, Expr::Symbol(symbol) if symbol.name.as_ref() == "resolution"),
        )
        .expect("resolution")
    else {
        panic!("resolution map")
    };
    let (_, state) = resolution
        .iter_mut()
        .find(|(key, _)| matches!(key, Expr::Symbol(symbol) if symbol.name.as_ref() == "state"))
        .expect("state");
    *state = Expr::String("partially-resolved".to_owned());
    assert!(matches!(
        index_shape().check(&unknown_variant),
        Err(CodecError::Shape(message)) if message.contains("unsupported protocol resolution state")
    ));

    let mut unknown = source_fact_doc();
    unknown.protocol_relations[0].resolution = ProtocolResolution::Resolved {
        protocol: String::new(),
    };
    let encoded = encode_index_expr(
        &expr_from_index_doc(&unknown),
        EncodePosition::Data,
        IndexForm::Sx,
    )
    .expect("encode malformed bound");
    assert!(matches!(
        IndexCodec.decode(IndexForm::Sx, &encoded),
        Err(CodecError::Index(
            IndexError::InvalidProtocolResolution { .. }
        ))
    ));

    let mut invalid_bound = source_fact_doc();
    invalid_bound.declarations[0].syntax_bound.max_bytes = 1;
    assert!(matches!(
        IndexCodec.encode(&invalid_bound, EncodePosition::Data, IndexForm::Sx),
        Err(CodecError::Index(IndexError::InvalidSourceBound { .. }))
    ));
}

#[test]
fn roundtrip_large_index_docs_over_default_expr_limit() {
    let codec = IndexCodec;
    let doc = large_doc();
    let sx = codec
        .encode(&doc, EncodePosition::Data, IndexForm::Sx)
        .expect("encode sx");
    let json = codec
        .encode(&doc, EncodePosition::Data, IndexForm::Json)
        .expect("encode json");

    let from_sx = codec.decode(IndexForm::Sx, &sx).expect("decode sx");
    let from_json = codec.decode(IndexForm::Json, &json).expect("decode json");

    assert_eq!(from_sx.subjects.len(), doc.subjects.len());
    assert_eq!(from_json.subjects.len(), doc.subjects.len());
}

#[test]
fn expr_conversion_is_one_checked_model() {
    let doc = valid_doc();
    let expr = expr_from_index_doc(&doc);

    index_shape().check(&expr).expect("shape");
    assert_eq!(index_doc_from_expr(&expr).expect("doc"), doc);
}

#[test]
fn malformed_ids_fail_closed_after_shape() {
    let codec = IndexCodec;
    let mut doc = valid_doc();
    doc.features[0].id = FeatureId::new("Feature/Bad");
    let source = codec
        .encode(&doc, EncodePosition::Data, IndexForm::Sx)
        .expect_err("invalid id should not encode");

    assert!(matches!(
        source,
        CodecError::Index(IndexError::InvalidId {
            kind: "feature",
            id
        }) if id == "Feature/Bad"
    ));
}

#[test]
fn duplicate_keys_fail_closed() {
    let codec = IndexCodec;
    let mut doc = valid_doc();
    let mut duplicate = doc.features[0].clone();
    duplicate.id = FeatureId::new("feature/sim-run/repl-copy");
    doc.features.push(duplicate);

    assert!(matches!(
        codec.encode(&doc, EncodePosition::Data, IndexForm::Json),
        Err(CodecError::Index(IndexError::DuplicateCanonicalKey { key }))
            if key == "crate/sim-run/feature-sim-run-repl"
    ));
}

#[test]
fn malformed_index_forms_fail_closed() {
    let codec = IndexCodec;
    let mut doc = valid_doc();
    doc.features[0].grammar_contracts[0].round_trip = false;
    let bad_grammar = encode_index_expr(
        &expr_from_index_doc(&doc),
        EncodePosition::Data,
        IndexForm::Sx,
    )
    .expect("encode unchecked sx");

    assert!(matches!(
        codec.decode(IndexForm::Sx, &bad_grammar),
        Err(CodecError::Index(IndexError::InvalidGrammarContract { .. }))
    ));
}

#[test]
fn literal_claims_and_missing_specimens_fail_closed() {
    let codec = IndexCodec;
    let mut literal = valid_doc();
    literal.drafts.push(sim_index_core::FeatureDraft {
        id: FeatureId::new("feature/sim-run/literal"),
        subject: SubjectId::new("crate/sim-run"),
        title: "Literal".to_owned(),
        summary: "Invalid literal claim.".to_owned(),
        claims_anchors: Vec::new(),
        claims_surfaces: Vec::new(),
        claims_specimens: Vec::new(),
        literal_anchors: vec!["export/sim-run/literal".to_owned()],
        literal_surfaces: Vec::new(),
        literal_specimens: Vec::new(),
        grammar_contracts: Vec::new(),
        doc_anchor: None,
    });
    let literal_sx = encode_index_expr(
        &expr_from_index_doc(&literal),
        EncodePosition::Data,
        IndexForm::Sx,
    )
    .expect("encode unchecked literal");

    let mut missing = valid_doc();
    missing.features[0]
        .specimens
        .push(SpecimenId::new("recipe/sim-run/missing"));
    let missing_sx = encode_index_expr(
        &expr_from_index_doc(&missing),
        EncodePosition::Data,
        IndexForm::Sx,
    )
    .expect("encode unchecked missing specimen");

    assert!(matches!(
        codec.decode(IndexForm::Sx, &literal_sx),
        Err(CodecError::Index(IndexError::LiteralClaim {
            kind: "anchor",
            ..
        }))
    ));
    assert!(matches!(
        codec.decode(IndexForm::Sx, &missing_sx),
        Err(CodecError::Index(IndexError::UnresolvedClaim {
            kind: "specimen",
            id,
            ..
        })) if id == "recipe/sim-run/missing"
    ));
}

#[test]
fn shape_rejects_wrong_field_type() {
    let mut expr = expr_from_index_doc(&valid_doc());
    let Expr::Map(entries) = &mut expr else {
        panic!("doc expr should be a map");
    };
    let (_, value) = entries
        .iter_mut()
        .find(|(key, _)| matches!(key, Expr::Symbol(symbol) if symbol.name.as_ref() == "subjects"))
        .expect("subjects field");
    *value = Expr::String("not-list".to_owned());

    assert!(matches!(
        index_shape().check(&expr),
        Err(CodecError::Shape(message)) if message.contains("subjects")
    ));
}

#[test]
fn runtime_codec_normalizes_to_index_expr() {
    let mut cx = sim_test_support::core_cx();
    let codec_id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&IndexCodecLib::new(codec_id))
        .expect("load index codec");
    let symbol = Symbol::qualified("codec", "index");
    let source = IndexCodec
        .encode(&valid_doc(), EncodePosition::Data, IndexForm::Sx)
        .expect("encode sx");

    let decoded = decode_with_codec(&mut cx, &symbol, Input::Text(source), ReadPolicy::default())
        .expect("runtime decode");
    let encoded = encode_with_codec(&mut cx, &symbol, &decoded, EncodeOptions::default())
        .expect("runtime encode")
        .into_text()
        .expect("text output");

    assert!(encoded.contains("features"));
    assert_eq!(
        IndexCodec
            .decode(IndexForm::Sx, &encoded)
            .expect("decode encoded"),
        valid_doc()
    );
}
