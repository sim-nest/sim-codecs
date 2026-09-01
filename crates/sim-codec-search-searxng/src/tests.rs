// conformance: SearXNG results preserve provider claims without constructing citations.

use super::*;
use sim_lib_search_core::SearchQuery;
use sim_lib_web_core::DecodeLimits;
fn q(s: &str) -> SearchQuery {
    SearchQuery::checked(s.into(), vec![], None, 10).unwrap()
}
#[test]
fn post_and_sensitive_get() {
    let c = SearxngCodec;
    let w = c
        .encode(
            &q("rust privacy"),
            &RequestOptions::default(),
            DecodeLimits::default(),
        )
        .unwrap();
    assert_eq!(w.body, b"q=rust+privacy&format=json");
    let o = RequestOptions {
        method: SearchMethod::Get,
        ..Default::default()
    };
    assert!(c.encode(&q("secret"), &o, DecodeLimits::default()).is_err())
}
#[test]
fn bang_receipt() {
    let c = SearxngCodec;
    assert!(
        c.encode(
            &q("!!google x"),
            &RequestOptions::default(),
            DecodeLimits::default()
        )
        .is_err()
    );
    let o = RequestOptions {
        bang_policy: BangPolicy::Permit {
            policy: "operator".into(),
            receipt: "r:1".into(),
        },
        ..Default::default()
    };
    assert_eq!(
        c.encode(&q("!!google x"), &o, DecodeLimits::default())
            .unwrap()
            .bang_receipt
            .as_deref(),
        Some("r:1")
    )
}
#[test]
fn config_is_observation_only() {
    let c = SearxngCodec
        .config(
            include_bytes!("../fixtures/v1/config.json"),
            DecodeLimits::default(),
        )
        .unwrap();
    assert_eq!(c.engines, ["alpha", "beta"]);
    assert_eq!(c.json_search, JsonSupport::Unknown)
}
#[test]
fn partial_page_survives() {
    let d = SearxngCodec
        .response(
            200,
            &[],
            include_bytes!("../fixtures/v1/partial.json"),
            &q("sim kernel"),
            DecodeLimits::default(),
        )
        .unwrap();
    assert_eq!(d.results.len(), 2);
    assert_eq!(
        d.notices
            .iter()
            .find(|n| n.code == "row-decode")
            .and_then(|n| n.index),
        Some(1)
    );
    assert_eq!(
        d.raw_response,
        include_bytes!("../fixtures/v1/partial.json")
    )
}
#[test]
fn status_mapping_no_html() {
    assert_eq!(
        SearxngCodec.response(
            403,
            &[],
            include_bytes!("../fixtures/v1/disabled-json.html"),
            &q("x"),
            DecodeLimits::default()
        ),
        Err(ResponseError::FormatDisabled)
    );
    assert_eq!(
        SearxngCodec.response(
            429,
            &[("Retry-After".into(), "120".into())],
            b"",
            &q("x"),
            DecodeLimits::default()
        ),
        Err(ResponseError::RateLimited {
            retry_after: Some(120)
        })
    );
    assert_eq!(
        SearxngCodec.response(401, &[], b"", &q("x"), DecodeLimits::default()),
        Err(ResponseError::PrincipalRejected)
    );
    assert_eq!(
        SearxngCodec.response(503, &[], b"", &q("x"), DecodeLimits::default()),
        Err(ResponseError::SiteUnavailable)
    )
}
#[test]
fn correction_and_open_fields() {
    let d = SearxngCodec
        .response(
            200,
            &[],
            include_bytes!("../fixtures/v1/correction-only.json"),
            &q("searxngg"),
            DecodeLimits::default(),
        )
        .unwrap();
    assert_eq!(d.supplemental.len(), 1);
    let d = SearxngCodec
        .response(
            200,
            &[],
            include_bytes!("../fixtures/v1/open-fields.json"),
            &q("open result"),
            DecodeLimits::default(),
        )
        .unwrap();
    assert!(d.results[0].extra.contains_key("future_field"));
    assert_eq!(
        d.page.observations[0]
            .claim
            .as_ref()
            .unwrap()
            .snippet
            .as_deref(),
        Some("provider text")
    )
}
#[test]
fn bounds_invalid_url_and_snapshots() {
    let l = DecodeLimits {
        max_items: 2,
        ..Default::default()
    };
    assert!(
        SearxngCodec
            .response(
                200,
                &[],
                include_bytes!("../fixtures/v1/oversized.json"),
                &q("many"),
                l
            )
            .is_err()
    );
    assert!(
        SearxngCodec
            .response(
                200,
                &[],
                include_bytes!("../fixtures/v1/invalid-url.json"),
                &q("bad"),
                DecodeLimits::default()
            )
            .is_err()
    );
    let w = SearxngCodec
        .encode(
            &q("rust privacy"),
            &RequestOptions::default(),
            DecodeLimits::default(),
        )
        .unwrap();
    assert_eq!(
        std::str::from_utf8(&w.body).unwrap(),
        include_str!("../fixtures/v1/request.snapshot").trim()
    )
}
