#[cfg(test)]
mod tests {
// conformance: schema documents parse, adapt, and validate canonical JSON forms.

use super::*;
    use serde_json::json;

    fn doc(value: Value) -> SchemaDocument {
        SchemaDocument::from_value(
            value,
            ResourceIdentity {
                base_uri: "sim://fixture/schema".into(),
                source: "fixture".into(),
            },
            SchemaLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn recursive_composition_annotations_and_pointers_are_stable() {
        let schema = json!({"$schema":DRAFT_2020_12,"$defs":{"node":{"type":"object","x-owner":"kept","required":["value"],"properties":{"value":{"type":"integer","minimum":0},"next":{"anyOf":[{"$ref":"#/$defs/node"},{"type":"null"}]}},"unevaluatedProperties":false}},"$ref":"#/$defs/node"});
        let document = doc(schema.clone());
        assert_eq!(document.value()["$defs"]["node"]["x-owner"], "kept");
        assert!(
            document
                .validate(&json!({"value":1,"next":{"value":2,"next":null}}))
                .is_ok()
        );
        let errors = document.validate(&json!({"value":-1})).unwrap_err();
        assert_eq!(errors[0].instance_pointer, "/value");
        assert_eq!(errors[0].keyword, "minimum");
        assert_eq!(document.semantic_digest(), doc(schema).semantic_digest());
    }

    #[test]
    fn hostile_ref_fails_closed_without_io_and_budgets_are_enforced() {
        let errors = doc(json!({"$ref":"https://example.invalid/secret"}))
            .validate(&json!(1))
            .unwrap_err();
        assert!(errors[0].message.contains("injected retriever"));
        let limits = SchemaLimits {
            evaluated_branches: 1,
            ..SchemaLimits::default()
        };
        let d = SchemaDocument::from_value(
            json!({"anyOf":[{"type":"string"},{"type":"number"}]}),
            ResourceIdentity {
                base_uri: "sim://budget".into(),
                source: "budget".into(),
            },
            limits,
        )
        .unwrap();
        assert_eq!(d.validate(&json!(1)).unwrap_err()[0].keyword, "budget");
    }

    struct FixtureRetriever {
        resource: RetrievedResource,
    }

    impl SchemaRetriever for FixtureRetriever {
        fn retrieve(&self, uri: &str) -> Result<RetrievedResource, SchemaError> {
            assert_eq!(uri, "sim://schema/external");
            Ok(self.resource.clone())
        }
    }

    #[test]
    fn injected_retriever_validates_digest_and_resource_schema() {
        let external = doc(
            json!({"type":"object","required":["id"],"properties":{"id":{"type":"string","pattern":"^[a-z]+-[0-9]+$"}}}),
        );
        let retriever = FixtureRetriever {
            resource: RetrievedResource {
                bytes: serde_json::to_vec(external.value()).unwrap(),
                media_type: "application/schema+json".to_owned(),
                base_uri: "sim://schema/external".to_owned(),
                digest: external.semantic_digest().to_owned(),
                redirects: Vec::new(),
            },
        };
        let root = doc(json!({"$ref":"sim://schema/external"}));

        assert!(
            root.validate_with(&json!({"id":"case-1"}), Some(&retriever))
                .is_ok()
        );
        let errors = root
            .validate_with(&json!({"id":"CASE"}), Some(&retriever))
            .unwrap_err();
        assert_eq!(errors[0].keyword, "pattern");
    }
}
