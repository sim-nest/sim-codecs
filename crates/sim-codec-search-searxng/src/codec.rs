//! Pure bounded SearXNG `/config` and JSON `/search` wire translation.
use sim_codec_json::{JsonTree, parse_json_with_limits, render_json};
use sim_kernel::{CodecId, Datum, Symbol};
use sim_lib_search_core::{
    ProviderClaim, SearchError, SearchObservation, SearchPage, SearchQuery, SearchWireCodec,
};
use sim_lib_web_core::DecodeLimits;
use std::collections::{BTreeMap, BTreeSet};

/// Stable codec id.
pub const CODEC_ID: &str = "codec/search-searxng";
/// Stable mapping version.
pub const CODEC_VERSION: &str = "1";
/// Embedded pure recipes.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
const JSON_ID: CodecId = CodecId(1);

/// Search request method.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchMethod {
    /// Private form body.
    #[default]
    Post,
    /// Explicit URL query.
    Get,
}
/// URL-disclosure classification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QuerySensitivity {
    /// Do not place in URLs.
    #[default]
    Sensitive,
    /// Explicitly public.
    Public,
}
/// Named bang policy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BangPolicy {
    /// Reject bang tokens.
    #[default]
    Reject,
    /// Admit with retained decision evidence.
    Permit {
        /// Policy name.
        policy: String,
        /// Decision receipt.
        receipt: String,
    },
}
/// SearXNG time range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeRange {
    /// Day.
    Day,
    /// Month.
    Month,
    /// Year.
    Year,
}
impl TimeRange {
    fn wire(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Month => "month",
            Self::Year => "year",
        }
    }
}
/// SearXNG safe-search posture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafeSearch {
    /// Off.
    Off = 0,
    /// Moderate.
    Moderate = 1,
    /// Strict.
    Strict = 2,
}
/// Checked site request options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestOptions {
    /// Method.
    pub method: SearchMethod,
    /// Confidentiality.
    pub sensitivity: QuerySensitivity,
    /// Categories.
    pub categories: Vec<String>,
    /// Language.
    pub language: Option<String>,
    /// One-based page.
    pub page: Option<u32>,
    /// Time range.
    pub time_range: Option<TimeRange>,
    /// Safe search.
    pub safe_search: Option<SafeSearch>,
    /// Bang policy.
    pub bang_policy: BangPolicy,
}
/// Pure HTTP request projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireRequest {
    /// Method.
    pub method: SearchMethod,
    /// Always `/search`.
    pub path: &'static str,
    /// POST body.
    pub body: Vec<u8>,
    /// GET query.
    pub query: Option<String>,
    /// Bang decision receipt.
    pub bang_receipt: Option<String>,
}
/// Pure codec.
#[derive(Clone, Debug, Default)]
pub struct SearxngCodec;

impl SearxngCodec {
    /// Encode only admitted SearXNG parameters.
    pub fn encode(
        &self,
        q: &SearchQuery,
        o: &RequestOptions,
        l: DecodeLimits,
    ) -> Result<WireRequest, SearchError> {
        validate(q, o, l)?;
        let mut f = vec![("q", q.text.clone()), ("format", "json".into())];
        if !o.categories.is_empty() {
            f.push(("categories", o.categories.join(",")))
        }
        if let Some(v) = o.language.as_ref().or(q.language.as_ref()) {
            f.push(("language", v.clone()))
        }
        if let Some(v) = o.page {
            f.push(("pageno", v.to_string()))
        }
        if let Some(v) = o.time_range {
            f.push(("time_range", v.wire().into()))
        }
        if let Some(v) = o.safe_search {
            f.push(("safesearch", (v as u8).to_string()))
        }
        let encoded = f
            .into_iter()
            .map(|(k, v)| format!("{}={}", form(k), form(&v)))
            .collect::<Vec<_>>()
            .join("&");
        if encoded.len() > l.max_body_bytes {
            return Err(SearchError::BoundExceeded("encoded request"));
        }
        let receipt = match &o.bang_policy {
            BangPolicy::Permit { receipt, .. } if has_bang(&q.text) => Some(receipt.clone()),
            _ => None,
        };
        Ok(if o.method == SearchMethod::Post {
            WireRequest {
                method: o.method,
                path: "/search",
                body: encoded.into_bytes(),
                query: None,
                bang_receipt: receipt,
            }
        } else {
            WireRequest {
                method: o.method,
                path: "/search",
                body: vec![],
                query: Some(encoded),
                bang_receipt: receipt,
            }
        })
    }
    /// Decode `/config` without inferring JSON support.
    pub fn config(&self, input: &[u8], l: DecodeLimits) -> Result<SiteCapabilities, SearchError> {
        let root = parse(input, l)?;
        let o = obj(&root, "config root")?;
        let engines = array(o, "engines")
            .unwrap_or(&[])
            .iter()
            .filter_map(|v| obj(v, "engine").ok())
            .filter_map(|v| string(v, "name"))
            .collect();
        Ok(SiteCapabilities {
            instance_name: string(o, "instance_name"),
            engines,
            categories: string_list(value(o, "categories"), l.max_items)?,
            locales: keys_or_strings(value(o, "locales"), l.max_items)?,
            plugins: names(value(o, "plugins"), l.max_items)?,
            safe_search: value(o, "safe_search").and_then(number_u32),
            json_search: JsonSupport::Unknown,
        })
    }
    /// Decode one transport-owned HTTP outcome; HTML is never a fallback.
    pub fn response(
        &self,
        status: u16,
        headers: &[(String, String)],
        input: &[u8],
        q: &SearchQuery,
        l: DecodeLimits,
    ) -> Result<DecodedPage, ResponseError> {
        match status {
            200..=299 => decode(input, q, l).map_err(ResponseError::Decode),
            403 => Err(ResponseError::FormatDisabled),
            429 => Err(ResponseError::RateLimited {
                retry_after: retry(headers),
            }),
            401 | 407 => Err(ResponseError::PrincipalRejected),
            500..=599 => Err(ResponseError::SiteUnavailable),
            _ => Err(ResponseError::HttpStatus(status)),
        }
    }
}
impl SearchWireCodec for SearxngCodec {
    fn codec_id(&self) -> &str {
        CODEC_ID
    }
    fn codec_version(&self) -> &str {
        CODEC_VERSION
    }
    fn encode_request(&self, q: &SearchQuery, l: DecodeLimits) -> Result<Vec<u8>, SearchError> {
        Ok(self.encode(q, &RequestOptions::default(), l)?.body)
    }
    fn decode_config(&self, i: &[u8], l: DecodeLimits) -> Result<Datum, SearchError> {
        let c = self.config(i, l)?;
        Ok(Datum::Node {
            tag: Symbol::qualified("searxng", "site-capabilities"),
            fields: vec![
                (
                    Symbol::qualified("searxng", "instance-name"),
                    c.instance_name.map_or(Datum::Nil, Datum::String),
                ),
                (
                    Symbol::qualified("searxng", "json-search-established"),
                    Datum::Bool(false),
                ),
            ],
        })
    }
    fn decode_response(
        &self,
        i: &[u8],
        q: &SearchQuery,
        l: DecodeLimits,
    ) -> Result<SearchPage, SearchError> {
        Ok(decode(i, q, l)?.page)
    }
}

/// JSON support observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonSupport {
    /// Not established by config.
    Unknown,
    /// Established by successful JSON response.
    Established,
}
/// `/config` observations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteCapabilities {
    /// Instance name.
    pub instance_name: Option<String>,
    /// Engines.
    pub engines: Vec<String>,
    /// Categories.
    pub categories: Vec<String>,
    /// Locales.
    pub locales: Vec<String>,
    /// Plugins.
    pub plugins: Vec<String>,
    /// Safe-search value.
    pub safe_search: Option<u32>,
    /// JSON support evidence.
    pub json_search: JsonSupport,
}
/// Typed HTTP/decode failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResponseError {
    /// 403 for requested JSON.
    FormatDisabled,
    /// 429.
    RateLimited {
        /// Safe delta seconds.
        retry_after: Option<u64>,
    },
    /// 401/407.
    PrincipalRejected,
    /// 5xx.
    SiteUnavailable,
    /// Other status.
    HttpStatus(u16),
    /// Invalid body.
    Decode(SearchError),
}
/// Supplemental provider claim class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SupplementalClaimKind {
    /// Answer.
    Answer,
    /// Correction.
    Correction,
    /// Infobox.
    Infobox,
    /// Suggestion.
    Suggestion,
}
/// Supplemental provider claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupplementalClaim {
    /// Class.
    pub kind: SupplementalClaimKind,
    /// Open retained JSON.
    pub value: JsonTree,
}
/// Known and open result-row claims.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultClaim {
    /// Original order.
    pub index: usize,
    /// URL claim.
    pub url: String,
    /// Title claim.
    pub title: Option<String>,
    /// Snippet claim.
    pub snippet: Option<String>,
    /// Score claim.
    pub score: Option<String>,
    /// Engine claims.
    pub engines: Vec<String>,
    /// Category claim.
    pub category: Option<String>,
    /// Publication claim.
    pub published: Option<String>,
    /// Thumbnail claim.
    pub thumbnail: Option<String>,
    /// Unknown fields.
    pub extra: BTreeMap<String, JsonTree>,
}
/// Non-fatal decode notice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeNotice {
    /// Code.
    pub code: String,
    /// Row index.
    pub index: Option<usize>,
    /// Detail.
    pub message: String,
}
/// Rich provider decode plus generic projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedPage {
    /// Generic page.
    pub page: SearchPage,
    /// Result claims.
    pub results: Vec<ResultClaim>,
    /// Supplemental claims.
    pub supplemental: Vec<SupplementalClaim>,
    /// Partial failures.
    pub notices: Vec<DecodeNotice>,
    /// Raw response capture.
    pub raw_response: Vec<u8>,
    /// JSON support evidence.
    pub json_search: JsonSupport,
}

fn decode(input: &[u8], q: &SearchQuery, l: DecodeLimits) -> Result<DecodedPage, SearchError> {
    let root = parse(input, l)?;
    let o = obj(&root, "response root")?;
    if string(o, "query").as_deref() != Some(&q.text) {
        return Err(SearchError::InvalidRecord("query identity"));
    }
    let rows = array(o, "results").unwrap_or(&[]);
    if rows.len() > l.max_items {
        return Err(SearchError::BoundExceeded("result rows"));
    }
    let (mut results, mut observations, mut notices) = (vec![], vec![], vec![]);
    for (i, row) in rows.iter().enumerate() {
        match row_decode(i, row, l) {
            Ok((r, o)) => {
                results.push(r);
                observations.push(o)
            }
            Err(e) => notices.push(DecodeNotice {
                code: "row-decode".into(),
                index: Some(i),
                message: e.to_string(),
            }),
        }
    }
    if !rows.is_empty() && results.is_empty() {
        return Err(SearchError::InvalidRecord("all result identities"));
    }
    let mut supplemental = vec![];
    for (key, kind) in [
        ("answers", SupplementalClaimKind::Answer),
        ("corrections", SupplementalClaimKind::Correction),
        ("infoboxes", SupplementalClaimKind::Infobox),
        ("suggestions", SupplementalClaimKind::Suggestion),
    ] {
        if let Some(v) = array(o, key) {
            if v.len() > l.max_items {
                return Err(SearchError::BoundExceeded("supplemental claims"));
            }
            supplemental.extend(v.iter().cloned().map(|value| SupplementalClaim {
                kind: kind.clone(),
                value,
            }))
        }
    }
    if let Some(v) = array(o, "unresponsive_engines") {
        for x in v.iter().take(l.max_items) {
            notices.push(DecodeNotice {
                code: "unresponsive-engine".into(),
                index: None,
                message: render_json(JSON_ID, x).map_err(|e| SearchError::Wire(e.to_string()))?,
            })
        }
    }
    Ok(DecodedPage {
        page: SearchPage {
            query: q.clone(),
            observations,
            continuation: None,
        },
        results,
        supplemental,
        notices,
        raw_response: input.to_vec(),
        json_search: JsonSupport::Established,
    })
}
fn row_decode(
    i: usize,
    v: &JsonTree,
    l: DecodeLimits,
) -> Result<(ResultClaim, SearchObservation), SearchError> {
    let o = obj(v, "result row")?;
    let url = string(o, "url").ok_or(SearchError::InvalidRecord("result URL"))?;
    let title = string(o, "title");
    let snippet = string(o, "content");
    let engines = string_list(value(o, "engines"), l.max_items)?;
    let known: BTreeSet<_> = [
        "url",
        "title",
        "content",
        "score",
        "engines",
        "engine",
        "category",
        "publishedDate",
        "published_date",
        "thumbnail",
    ]
    .into_iter()
    .collect();
    let extra = o
        .iter()
        .filter(|(k, _)| !known.contains(k.as_str()))
        .cloned()
        .collect();
    let provider = if engines.is_empty() {
        string(o, "engine").unwrap_or_else(|| "searxng".into())
    } else {
        engines.join(",")
    };
    let observation = SearchObservation::checked(
        &url,
        Some(ProviderClaim {
            provider,
            uri: url.clone(),
            title: title.clone(),
            snippet: snippet.clone(),
            position: u32::try_from(i + 1).ok(),
        }),
        None,
    )?;
    Ok((
        ResultClaim {
            index: i,
            url,
            title,
            snippet,
            score: value(o, "score").and_then(number),
            engines,
            category: string(o, "category"),
            published: string(o, "publishedDate").or_else(|| string(o, "published_date")),
            thumbnail: string(o, "thumbnail"),
            extra,
        },
        observation,
    ))
}
fn validate(q: &SearchQuery, o: &RequestOptions, l: DecodeLimits) -> Result<(), SearchError> {
    if q.text.len() > l.max_text_bytes {
        return Err(SearchError::BoundExceeded("query text"));
    }
    if o.method == SearchMethod::Get && o.sensitivity != QuerySensitivity::Public {
        return Err(SearchError::Wire("GET rejected for sensitive query".into()));
    }
    if o.categories.len() > l.max_items
        || o.categories.iter().any(|v| {
            v.is_empty()
                || v.len() > 128
                || !v
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b' '))
        })
    {
        return Err(SearchError::InvalidRecord("categories"));
    }
    if o.page == Some(0) {
        return Err(SearchError::InvalidRecord("page"));
    }
    if has_bang(&q.text) {
        match &o.bang_policy {
            BangPolicy::Reject => {
                return Err(SearchError::Wire("bang syntax rejected by policy".into()));
            }
            BangPolicy::Permit { policy, receipt }
                if policy.trim().is_empty() || receipt.trim().is_empty() =>
            {
                return Err(SearchError::InvalidRecord("bang policy receipt"));
            }
            _ => {}
        }
    }
    Ok(())
}
fn parse(i: &[u8], l: DecodeLimits) -> Result<JsonTree, SearchError> {
    if i.len() > l.max_body_bytes {
        return Err(SearchError::BoundExceeded("response body"));
    }
    let s = std::str::from_utf8(i).map_err(|_| SearchError::InvalidRecord("UTF-8 JSON"))?;
    parse_json_with_limits(
        JSON_ID,
        s,
        sim_codec::DecodeLimits {
            max_input_bytes: l.max_body_bytes,
            max_tokens: l.max_items * 32,
            max_expr_nodes: l.max_items * 32,
            max_depth: 64,
            max_string_bytes: l.max_text_bytes,
            max_blob_bytes: l.max_body_bytes,
            max_collection_len: l.max_items,
            max_trivia_items: l.max_items,
        },
    )
    .map_err(|e| SearchError::Wire(e.to_string()))
}
fn obj<'a>(v: &'a JsonTree, n: &'static str) -> Result<&'a [(String, JsonTree)], SearchError> {
    if let JsonTree::Object(o) = v {
        Ok(o)
    } else {
        Err(SearchError::InvalidRecord(n))
    }
}
fn value<'a>(o: &'a [(String, JsonTree)], k: &str) -> Option<&'a JsonTree> {
    o.iter().find(|(n, _)| n == k).map(|(_, v)| v)
}
fn array<'a>(o: &'a [(String, JsonTree)], k: &str) -> Option<&'a [JsonTree]> {
    if let Some(JsonTree::Array(v)) = value(o, k) {
        Some(v)
    } else {
        None
    }
}
fn string(o: &[(String, JsonTree)], k: &str) -> Option<String> {
    if let Some(JsonTree::String(v)) = value(o, k) {
        Some(v.clone())
    } else {
        None
    }
}
fn number(v: &JsonTree) -> Option<String> {
    if let JsonTree::Number(v) = v {
        Some(v.clone())
    } else {
        None
    }
}
fn number_u32(v: &JsonTree) -> Option<u32> {
    number(v)?.parse().ok()
}
fn string_list(v: Option<&JsonTree>, limit: usize) -> Result<Vec<String>, SearchError> {
    let Some(JsonTree::Array(v)) = v else {
        return Ok(vec![]);
    };
    if v.len() > limit {
        return Err(SearchError::BoundExceeded("string array"));
    }
    Ok(v.iter()
        .filter_map(|v| {
            if let JsonTree::String(v) = v {
                Some(v.clone())
            } else {
                None
            }
        })
        .collect())
}
fn keys_or_strings(v: Option<&JsonTree>, l: usize) -> Result<Vec<String>, SearchError> {
    if let Some(JsonTree::Object(v)) = v {
        if v.len() > l {
            return Err(SearchError::BoundExceeded("locales"));
        }
        Ok(v.iter().map(|(k, _)| k.clone()).collect())
    } else {
        string_list(v, l)
    }
}
fn names(v: Option<&JsonTree>, l: usize) -> Result<Vec<String>, SearchError> {
    let Some(JsonTree::Array(v)) = v else {
        return Ok(vec![]);
    };
    if v.len() > l {
        return Err(SearchError::BoundExceeded("plugins"));
    }
    Ok(v.iter()
        .filter_map(|x| match x {
            JsonTree::String(s) => Some(s.clone()),
            JsonTree::Object(o) => string(o, "name"),
            _ => None,
        })
        .collect())
}
fn has_bang(q: &str) -> bool {
    q.split_whitespace().any(|v| v.starts_with('!'))
}
fn form(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                o.push(char::from(b))
            }
            b' ' => o.push('+'),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}
fn retry(h: &[(String, String)]) -> Option<u64> {
    h.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("retry-after"))
        .and_then(|(_, v)| {
            let v = v.trim();
            if v.len() > 10 || !v.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                v.parse().ok().filter(|x| *x <= 86400)
            }
        })
}
