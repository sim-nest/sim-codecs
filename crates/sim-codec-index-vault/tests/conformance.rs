// conformance: every vault profile round-trips semantically and rejects corrupted or false legacy claims.

use sim_codec_index_vault::{
    LegacyVaultBundle, LegacyVaultEntry, PROFILES, VaultDecoder, VaultEncoder,
    legacy_projection_v1, resolve_legacy_profile, resolve_profile, verify_legacy_v1, verify_v2,
};
use sim_index_core::{
    AnchorId, DeclarationFact, DeclarationRole, DiscoveredAnchor, DiscoveredSpecimen,
    DiscoveredSurface, FeatureDraft, FeatureId, FeatureRecord, GrammarContract, IndexDoc,
    IndexEdge, ProtocolRelation, ProtocolResolution, RouteId, RouteRecord, RouteStep,
    SourceCompleteness, SourceLocation, SourceReachability, SourceUnit, SpecimenId, SubjectId,
    SubjectRecord, SurfaceId, SyntaxBound, UnresolvedReason, Visibility, canonical_feature_key,
};
use sim_index_vault_core::IndexRow;
use sim_index_vault_core::{VaultGranularity, VaultProjection};

fn fixture() -> IndexDoc {
    let subject = SubjectId::new("crate/edge-case");
    let anchor = AnchorId::new("anchor/edge-case/value");
    let surface = SurfaceId::new("syntax/edge-case");
    let specimen = SpecimenId::new("recipe/edge-case/value");
    let feature = FeatureId::new("feature/edge-case/value");
    IndexDoc {
        schema: "sim.index".into(),
        generated_by: "vault-v2-test".into(),
        visibility: Visibility::Public,
        subjects: vec![SubjectRecord {
            id: subject.clone(),
            kind: "crate".into(),
            title: "Empty differs from absent".into(),
        }],
        anchors: vec![
            DiscoveredAnchor {
                id: anchor.clone(),
                subject: subject.clone(),
                kind: "export".into(),
            },
            DiscoveredAnchor {
                id: AnchorId::new("doc/edge-case/value"),
                subject: subject.clone(),
                kind: "doc".into(),
            },
        ],
        source_units: vec![SourceUnit {
            subject: subject.clone(),
            path: "src/lib.rs".into(),
            reachability: SourceReachability::Reachable,
            completeness: SourceCompleteness::Complete,
            reason: String::new(),
            retained_bound: SyntaxBound {
                max_bytes: 32,
                truncated: false,
            },
            declaration_count: 1,
        }],
        declarations: vec![DeclarationFact {
            anchor: anchor.clone(),
            role: DeclarationRole::Struct,
            module_path: "edge::Value".into(),
            generics: String::new(),
            members: vec![],
            location: SourceLocation {
                file: "src/lib.rs".into(),
                declaration: 0,
            },
            syntax_bound: SyntaxBound {
                max_bytes: 32,
                truncated: true,
            },
        }],
        protocol_relations: vec![
            ProtocolRelation {
                anchor: anchor.clone(),
                implementor: "Value".into(),
                source_spelling: "Display".into(),
                body_fingerprint: String::new(),
                body_bound: SyntaxBound {
                    max_bytes: 32,
                    truncated: false,
                },
                resolution: ProtocolResolution::Resolved {
                    protocol: "core::fmt::Display".into(),
                },
            },
            ProtocolRelation {
                anchor: anchor.clone(),
                implementor: "Value".into(),
                source_spelling: "Mystery".into(),
                body_fingerprint: "body".into(),
                body_bound: SyntaxBound {
                    max_bytes: 32,
                    truncated: false,
                },
                resolution: ProtocolResolution::Unresolved {
                    reason: UnresolvedReason::ExternalMetadataAbsent,
                    candidates: vec![],
                },
            },
        ],
        surfaces: vec![DiscoveredSurface {
            id: surface.clone(),
            subject: subject.clone(),
            kind: "syntax".into(),
        }],
        specimens: vec![DiscoveredSpecimen {
            id: specimen.clone(),
            subject: subject.clone(),
            kind: "recipe".into(),
            path: "recipes/value".into(),
            language: None,
            runnable: true,
            checked: true,
            checked_by: Some("cargo test".into()),
            doc_anchor: None,
        }],
        drafts: vec![FeatureDraft {
            id: FeatureId::new("feature/edge-case/draft"),
            subject: subject.clone(),
            title: "Draft".into(),
            summary: String::new(),
            claims_anchors: vec![],
            claims_surfaces: vec![],
            claims_specimens: vec![],
            literal_anchors: vec![],
            literal_surfaces: vec![],
            literal_specimens: vec![],
            grammar_contracts: vec![],
            doc_anchor: None,
        }],
        features: vec![FeatureRecord {
            id: feature.clone(),
            key: canonical_feature_key(&subject, feature.as_str()),
            subject: subject.clone(),
            title: "Value".into(),
            summary: "all families".into(),
            anchors: vec![anchor.clone()],
            surfaces: vec![surface.clone()],
            specimens: vec![specimen.clone()],
            grammar_contracts: vec![GrammarContract {
                id: "grammar/edge-case".into(),
                decoder: Some(anchor),
                encoder: None,
                surface: Some(surface),
                round_trip: true,
            }],
            doc_anchor: None,
        }],
        routes: vec![RouteRecord {
            id: RouteId::new("route/edge-case/value"),
            title: "Use value".into(),
            audiences: vec!["framework".into()],
            steps: vec![
                RouteStep::Feature {
                    id: feature.clone(),
                    why: "learn".into(),
                },
                RouteStep::Specimen {
                    id: specimen,
                    why: "run".into(),
                },
            ],
            doc_anchor: None,
        }],
        edges: vec![IndexEdge::relates(feature.clone(), "supports", feature)],
    }
}

#[test]
fn all_four_profiles_encode_both_complete_granularities_exactly_once() {
    let doc = fixture();
    for profile in PROFILES {
        assert_eq!(resolve_profile(profile.id.as_str()).unwrap(), profile);
        for granularity in [VaultGranularity::Compact, VaultGranularity::Full] {
            let projection = VaultProjection::from_complete(&doc, granularity).unwrap();
            let bundle = VaultEncoder::new(profile).encode(&projection).unwrap();
            let decoded = VaultDecoder::new(profile).decode(&bundle).unwrap();
            assert_eq!(decoded.projection, projection);
            assert!(decoded.fidelity_exact);
            assert!(decoded.declared_projection_equal);
            let verification = verify_v2(&bundle, &projection, 16, 256).unwrap();
            assert!(verification.is_success());
            assert_eq!(bundle.profile, profile.id);
            assert_eq!(
                bundle
                    .entries
                    .iter()
                    .filter(|e| e.note_id == "README")
                    .count(),
                1
            );
            let claimed: usize = bundle
                .entries
                .iter()
                .flat_map(|e| e.claim_families.values())
                .sum();
            assert_eq!(claimed, doc.inventory().1.len());
            assert_eq!(
                bundle.entries.iter().map(|e| &e.path).collect::<Vec<_>>(),
                {
                    let mut p = bundle.entries.iter().map(|e| &e.path).collect::<Vec<_>>();
                    p.sort();
                    p
                }
            );
            let text = bundle
                .entries
                .iter()
                .map(|e| String::from_utf8_lossy(&e.bytes))
                .collect::<String>();
            assert!(text.contains("truncated") && text.contains("true"));
            assert!(text.to_lowercase().contains("unresolved"));
            assert!(text.to_lowercase().contains("resolved"));
        }
    }
}

#[test]
fn decode_rejects_digest_path_profile_and_valid_markdown_semantic_corruption() {
    let projection = VaultProjection::from_complete(&fixture(), VaultGranularity::Compact).unwrap();
    let bundle = VaultEncoder::new(PROFILES[0]).encode(&projection).unwrap();

    let mut content = bundle.clone();
    let entry = content
        .entries
        .iter_mut()
        .find(|e| e.note_kind.is_some())
        .unwrap();
    let text = String::from_utf8(entry.bytes.clone()).unwrap();
    entry.bytes = text.replacen("Draft", "Mystery", 1).into_bytes();
    assert_ne!(entry.bytes, text.as_bytes());
    assert!(VaultDecoder::new(PROFILES[0]).decode(&content).is_err());

    let mut path = bundle.clone();
    let entry = path
        .entries
        .iter_mut()
        .find(|e| e.note_kind.is_some())
        .unwrap();
    entry.path = "wrong/place.md".into();
    assert!(VaultDecoder::new(PROFILES[0]).decode(&path).is_err());

    assert!(VaultDecoder::new(PROFILES[1]).decode(&bundle).is_err());
}

#[test]
fn bytes_and_semantic_identity_ignore_input_permutation() {
    for granularity in [VaultGranularity::Compact, VaultGranularity::Full] {
        let doc = fixture();
        let expected = VaultEncoder::new(PROFILES[0])
            .encode(&VaultProjection::from_complete(&doc, granularity).unwrap())
            .unwrap();
        let mut permuted = doc.clone();
        permuted.anchors.reverse();
        let actual = VaultEncoder::new(PROFILES[0])
            .encode(&VaultProjection::from_complete(&permuted, granularity).unwrap())
            .unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn every_canonical_row_variant_has_one_semantic_site() {
    let projection = VaultProjection::from_complete(&fixture(), VaultGranularity::Full).unwrap();
    let mut families = std::collections::BTreeSet::new();
    for row in projection.certificate().primary_rows() {
        families.insert(match row {
            IndexRow::Subject(_) => "subject",
            IndexRow::Anchor(_) => "anchor",
            IndexRow::SourceUnit(_) => "source",
            IndexRow::Declaration(_) => "declaration",
            IndexRow::ProtocolRelation(_) => "protocol",
            IndexRow::Surface(_) => "surface",
            IndexRow::Specimen(_) => "specimen",
            IndexRow::Draft(_) => "draft",
            IndexRow::Feature(_) => "feature",
            IndexRow::Route(_) => "route",
            IndexRow::Edge(_) => "edge",
        });
    }
    assert_eq!(families.len(), 11);
    assert_eq!(
        projection.certificate().primary().len(),
        fixture().inventory().1.len()
    );
}

#[test]
fn v2_deliberate_spelling_changes_are_documented() {
    let readme = include_str!("../README.md");
    assert!(readme.contains("not byte-compatible"));
    for legacy in [
        "portable-markdown-v1",
        "obsidian-markdown-v1",
        "seqlog-markdown-v1",
        "logseq-file-v1",
    ] {
        assert!(!PROFILES.iter().any(|p| p.id.as_str() == legacy));
    }
}

#[test]
fn legacy_v1_is_bounded_decode_only_and_semantically_checked() {
    let doc = IndexDoc::public("legacy-v1-fixture");
    let expected = legacy_projection_v1(&doc, VaultGranularity::Compact).unwrap();
    let bytes = b"---\nsim_profile: \"portable-markdown-v1\"\ngranularity: \"compact\"\nschema: \"sim.index/v1\"\ngenerated-by: \"fixture\"\n---\n\n# SIM Index Vault\n\n## Navigation\n".to_vec();
    let bundle = LegacyVaultBundle {
        profile: resolve_legacy_profile("portable-markdown-v1").unwrap(),
        granularity: VaultGranularity::Compact,
        entries: vec![LegacyVaultEntry {
            path: "README.md".into(),
            bytes,
        }],
    };
    let verified = verify_legacy_v1(&bundle, &expected).unwrap();
    assert!(verified.note_identities.is_empty());
    assert_eq!(verified.known_absent_families, &["declaration", "protocol"]);

    let mut drift = bundle;
    drift.entries[0].bytes = String::from_utf8(drift.entries[0].bytes.clone())
        .unwrap()
        .replace("portable-markdown-v1", "foreign-markdown-v1")
        .into_bytes();
    assert!(verify_legacy_v1(&drift, &expected).is_err());
}
