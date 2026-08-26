//! Header projection helpers and authority conformance tests.

use super::*;

pub(super) fn extension_owner(key: &str) -> Option<&str> {
    let (owner, leaf) = key.split_once('/')?;
    (!owner.is_empty() && owner.contains('.') && !leaf.is_empty()).then_some(owner)
}

pub(super) fn json_depth(value: &Value) -> usize {
    match value {
        Value::Array(v) => 1 + v.iter().map(json_depth).max().unwrap_or(0),
        Value::Object(v) => 1 + v.values().map(json_depth).max().unwrap_or(0),
        _ => 1,
    }
}

pub(super) fn encode_header_json(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).expect("JSON value serializes");
    if bytes.iter().all(|b| matches!(b, 0x21..=0x7e)) && !bytes.starts_with(b":base64:") {
        String::from_utf8(bytes).expect("safe bytes are UTF-8 JSON")
    } else {
        format!(":base64:{}", base64_no_pad(&bytes))
    }
}

pub(super) fn validate_header_value(value: &str) -> Result<(), HeaderError> {
    if let Some(encoded) = value.strip_prefix(":base64:") {
        if encoded.is_empty()
            || encoded.len() % 4 == 1
            || !encoded
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
        {
            return Err(HeaderError::UnsafeValue);
        }
    } else if value.is_empty() || !value.bytes().all(|b| matches!(b, 0x21..=0x7e)) {
        return Err(HeaderError::UnsafeValue);
    }
    Ok(())
}

pub(super) fn base64_no_pad(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[((bits >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((bits >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((bits >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(bits & 63) as usize] as char);
        }
    }
    out
}

pub(super) fn check_header(
    seen: &BTreeMap<String, &String>,
    key: &str,
    expected: Option<&str>,
) -> Result<(), HeaderError> {
    match (seen.get(key), expected) {
        (None, Some(_)) => Err(HeaderError::Missing(key.to_owned())),
        (Some(actual), Some(expected)) if actual.as_str() != expected => {
            Err(HeaderError::Conflict(key.to_owned()))
        }
        (Some(_), None) => Err(HeaderError::Conflict(key.to_owned())),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    // conformance: wire authority accepts only the declared MCP protocol surface.
    use super::*;
    use sim_codec_json::{ResourceIdentity, SchemaLimits};

    fn info() -> ClientInfo {
        ClientInfo {
            name: "fixture".into(),
            version: "1".into(),
        }
    }
    fn meta() -> RequestMeta {
        RequestMeta {
            protocol_version: MODERN.into(),
            client_capabilities: serde_json::json!({"tools": {}}),
            client_info: Some(info()),
            trace: TraceContext::default(),
            extensions: ExtensionSettings::default(),
        }
    }
    fn schema(value: Value) -> SchemaDocument {
        SchemaDocument::from_value(
            value,
            ResourceIdentity {
                base_uri: "urn:fixture".into(),
                source: "fixture".into(),
            },
            SchemaLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn profiles_report_exact_requested_and_supported_data() {
        assert_eq!(
            ProtocolProfileId::parse(MODERN),
            Ok(ProtocolProfileId::Modern20260728)
        );
        assert_eq!(
            ProtocolProfileId::parse("future"),
            Err(ProtocolError::UnsupportedVersion {
                requested: "future".into(),
                supported: [MODERN, LEGACY]
            })
        );
    }

    #[test]
    fn extension_json_round_trips_and_fails_closed() {
        let original = serde_json::json!({"com.example/setting":{"unknown":[1,{"x":true}]}});
        let settings: ExtensionSettings = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(serde_json::to_value(&settings).unwrap(), original);
        assert_eq!(
            settings.validate_negotiated(&BTreeSet::new()),
            Err(WireError::UnnegotiatedExtension("com.example".into()))
        );
        assert!(
            serde_json::from_value::<ExtensionSettings>(serde_json::json!({
                "protocolVersion": "collision"
            }))
            .is_err()
        );
    }

    #[test]
    fn result_type_payload_and_cache_are_checked() {
        let owners = BTreeSet::from(["com.example".to_owned()]);
        assert_eq!(
            ResultType::parse("com.example/review", &owners),
            Ok(ResultType::Extension("com.example/review".into()))
        );
        assert_eq!(
            ResultType::for_profile(ProtocolProfileId::Legacy20250326, None, &owners),
            Ok(ResultType::Complete)
        );
        assert_eq!(
            ResultType::for_profile(ProtocolProfileId::Modern20260728, None, &owners),
            Err(WireError::MissingProjection("resultType"))
        );
        assert!(matches!(
            ResultType::parse("org.other/review", &owners),
            Err(WireError::UnnegotiatedExtension(_))
        ));
        let input = InputRequired {
            input_requests: BTreeMap::from([(
                "answer".into(),
                schema(serde_json::json!({"type":"string","title":"kept"})),
            )]),
            request_state: serde_json::json!("opaque"),
        };
        assert!(
            ModernResult::new(
                Method::ToolsCall,
                ResultType::InputRequired,
                ResultPayload::InputRequired(input),
                info(),
                Some(CacheHint {
                    cacheable: true,
                    max_age: Some(60)
                })
            )
            .is_err()
        );
    }

    #[test]
    fn header_projection_is_pure_complete_and_conflict_checked() {
        let projection = HeaderProjection::from_body(
            ProtocolProfileId::Modern20260728,
            Method::ToolsCall,
            Some("weather"),
            &meta(),
        )
        .unwrap();
        let mut headers = vec![
            ("MCP-Protocol-Version".into(), MODERN.into()),
            ("MCP-Method".into(), "tools/call".into()),
            ("MCP-Name".into(), "weather".into()),
        ];
        headers.extend(
            projection
                .x_mcp_headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        assert_eq!(projection.check(&headers), Ok(()));
        headers.push(("mcp-name".into(), "other".into()));
        assert!(matches!(
            projection.check(&headers),
            Err(HeaderError::Duplicate(_))
        ));
    }

    #[test]
    fn tracing_and_full_schema_are_not_lossy() {
        let invalid = TraceContext {
            tracestate: Some("vendor=value".into()),
            ..TraceContext::default()
        };
        assert_eq!(
            invalid.validate(),
            Err(WireError::MalformedTrace("tracestate requires traceparent"))
        );
        let schemas = ToolSchemas {
            input: schema(serde_json::json!({"type":"object","x-vendor":{"kept":true}})),
            output: None,
            structured_content: None,
        };
        assert_eq!(schemas.input.value()["x-vendor"]["kept"], true);
    }

    #[test]
    fn coverage_vectors_have_one_typed_authority_path() {
        let vectors: Value =
            serde_json::from_str(include_str!("../../fixtures/mcp/2026-07-28/vectors.json"))
                .unwrap();
        let ids: BTreeSet<_> = vectors["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|case| case["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            BTreeSet::from([
                "required-metadata",
                "legacy-absent-result-type",
                "extension-result-type",
                "server-info",
                "header-safe-base64",
                "header-mismatch",
                "unsupported-protocol-version",
                "missing-capability",
                "tracing",
                "mrtr",
                "subscriptions",
                "caching",
                "discovery",
            ])
        );
        let all_methods = [
            Method::ServerDiscover,
            Method::ToolsList,
            Method::ToolsCall,
            Method::PromptsList,
            Method::PromptsGet,
            Method::ResourcesList,
            Method::ResourcesRead,
            Method::SubscriptionsListen,
            Method::InputResponses,
            Method::Acknowledgement,
            Method::Event,
            Method::Cancel,
            Method::Terminal,
        ];
        assert!(
            all_methods
                .iter()
                .all(|method| !method_registry(*method).name.is_empty())
        );
    }
}
