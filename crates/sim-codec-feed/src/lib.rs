//! Bounded, transport-free RSS 2.0, Atom 1.0, and JSON Feed projection.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::collections::BTreeMap;

/// Stable runtime codec symbol.
pub const CODEC_SYMBOL: &str = "codec/feed";
/// Stable accepted media-type aliases.
pub const MEDIA_TYPES: &[&str] = &[
    "application/rss+xml",
    "application/atom+xml",
    "application/feed+json",
];
/// Embedded pure recipes.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

/// Supported source dialect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedDialect {
    /// RSS 2.0.
    Rss20,
    /// Atom 1.0.
    Atom10,
    /// JSON Feed.
    JsonFeed,
}
/// A retained attachment claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    /// Claimed URL.
    pub url: String,
    /// Optional media type.
    pub media_type: Option<String>,
}
/// One normalized feed entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedEntry {
    /// Stable source identifier.
    pub id: String,
    /// Claimed source URL.
    pub source_url: Option<String>,
    /// Title.
    pub title: Option<String>,
    /// Authors.
    pub authors: Vec<String>,
    /// Published time text.
    pub published: Option<String>,
    /// Modified time text.
    pub modified: Option<String>,
    /// Summary text.
    pub summary: Option<String>,
    /// Content text or markup retained as data.
    pub content: Option<String>,
    /// Attachments.
    pub attachments: Vec<Attachment>,
    /// Unknown extension fields.
    pub extensions: BTreeMap<String, String>,
}
/// One normalized feed document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedDoc {
    /// Dialect.
    pub dialect: FeedDialect,
    /// Optional title.
    pub title: Option<String>,
    /// Entries.
    pub entries: Vec<FeedEntry>,
    /// Unknown top-level fields.
    pub extensions: BTreeMap<String, String>,
    /// Non-fatal decode warnings.
    pub warnings: Vec<String>,
}
/// Decode allocation limits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedLimits {
    /// Input byte ceiling.
    pub max_input_bytes: usize,
    /// Entry ceiling.
    pub max_entries: usize,
    /// Text byte ceiling.
    pub max_text_bytes: usize,
    /// Structural depth ceiling.
    pub max_depth: usize,
}
impl Default for FeedLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 2 * 1024 * 1024,
            max_entries: 10_000,
            max_text_bytes: 1024 * 1024,
            max_depth: 128,
        }
    }
}
/// Feed decode failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedError(pub String);
impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for FeedError {}

/// Decode a complete saved feed without resolving any referenced resource.
pub fn decode_feed(input: &[u8], limits: &FeedLimits) -> Result<FeedDoc, FeedError> {
    if input.len() > limits.max_input_bytes {
        return Err(FeedError("feed input byte limit exceeded".into()));
    }
    let text = String::from_utf8_lossy(input);
    let trimmed = text.trim_start();
    let mut doc = if trimmed.starts_with('{') {
        decode_json(trimmed, limits)?
    } else {
        decode_xml(trimmed, limits)?
    };
    if std::str::from_utf8(input).is_err() {
        doc.warnings
            .push("invalid UTF-8 replaced during decode".into())
    }
    Ok(doc)
}
fn decode_json(s: &str, l: &FeedLimits) -> Result<FeedDoc, FeedError> {
    let v: serde_json::Value = serde_json::from_str(s).map_err(|e| FeedError(e.to_string()))?;
    depth(&v, 0, l.max_depth)?;
    let o = v
        .as_object()
        .ok_or_else(|| FeedError("JSON Feed root must be an object".into()))?;
    let items = o
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.len() > l.max_entries {
        return Err(FeedError("feed entry limit exceeded".into()));
    }
    let mut total = 0;
    let mut entries = Vec::new();
    for item in items {
        let x = item
            .as_object()
            .ok_or_else(|| FeedError("JSON Feed item must be object".into()))?;
        let get = |k: &str| x.get(k).and_then(|v| v.as_str()).map(str::to_owned);
        let id = get("id")
            .or_else(|| get("url"))
            .ok_or_else(|| FeedError("JSON Feed item needs id or url".into()))?;
        let authors = x
            .get("authors")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|a| a.get("name").and_then(|v| v.as_str()).map(str::to_owned))
            .collect();
        let attachments = x
            .get("attachments")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|a| {
                Some(Attachment {
                    url: a.get("url")?.as_str()?.to_owned(),
                    media_type: a
                        .get("mime_type")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned),
                })
            })
            .collect();
        let known = [
            "id",
            "url",
            "external_url",
            "title",
            "content_text",
            "content_html",
            "summary",
            "date_published",
            "date_modified",
            "authors",
            "attachments",
        ];
        let extensions = x
            .iter()
            .filter(|(k, _)| !known.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();
        let e = FeedEntry {
            id,
            source_url: get("url"),
            title: get("title"),
            authors,
            published: get("date_published"),
            modified: get("date_modified"),
            summary: get("summary"),
            content: get("content_text").or_else(|| get("content_html")),
            attachments,
            extensions,
        };
        total += e.id.len()
            + e.title.as_deref().map_or(0, str::len)
            + e.content.as_deref().map_or(0, str::len);
        if total > l.max_text_bytes {
            return Err(FeedError("feed text limit exceeded".into()));
        }
        entries.push(e);
    }
    Ok(FeedDoc {
        dialect: FeedDialect::JsonFeed,
        title: o.get("title").and_then(|v| v.as_str()).map(str::to_owned),
        entries,
        extensions: BTreeMap::new(),
        warnings: Vec::new(),
    })
}
fn decode_xml(s: &str, l: &FeedLimits) -> Result<FeedDoc, FeedError> {
    let atom = s[..s.len().min(1024)]
        .to_ascii_lowercase()
        .contains("<feed");
    let container = if atom { "entry" } else { "item" };
    let chunks = elements(s, container);
    if chunks.len() > l.max_entries {
        return Err(FeedError("feed entry limit exceeded".into()));
    }
    let mut total = 0;
    let mut entries = Vec::new();
    for c in chunks {
        let get = |n: &str| element(c, n).map(unescape);
        let id = get(if atom { "id" } else { "guid" })
            .or_else(|| get("link"))
            .ok_or_else(|| FeedError("feed entry needs stable id or link".into()))?;
        let link = if atom {
            opening_attr(c, "link", "href").or_else(|| get("link"))
        } else {
            get("link")
        };
        let authors = elements(c, if atom { "author" } else { "author" })
            .into_iter()
            .filter_map(|a| element(a, if atom { "name" } else { "author" }).or(Some(a)))
            .map(unescape)
            .collect();
        let attachments = if atom {
            opening_attrs(c, "link", "href", "rel", "enclosure")
        } else {
            opening_attrs(c, "enclosure", "url", "type", "")
        }
        .into_iter()
        .map(|(url, mt)| Attachment {
            url,
            media_type: mt,
        })
        .collect();
        let e = FeedEntry {
            id,
            source_url: link,
            title: get("title"),
            authors,
            published: get(if atom { "published" } else { "pubDate" }),
            modified: get("updated"),
            summary: get(if atom { "summary" } else { "description" }),
            content: get(if atom { "content" } else { "content:encoded" }),
            attachments,
            extensions: BTreeMap::new(),
        };
        total += e.id.len() + e.content.as_deref().map_or(0, str::len);
        if total > l.max_text_bytes {
            return Err(FeedError("feed text limit exceeded".into()));
        }
        entries.push(e);
    }
    Ok(FeedDoc {
        dialect: if atom {
            FeedDialect::Atom10
        } else {
            FeedDialect::Rss20
        },
        title: element(s, "title").map(unescape),
        entries,
        extensions: BTreeMap::new(),
        warnings: Vec::new(),
    })
}
fn elements<'a>(s: &'a str, n: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let open = format!("<{n}");
    let close = format!("</{n}>");
    let mut rest = s;
    while let Some(a) = rest.find(&open) {
        let x = &rest[a..];
        if let Some(gt) = x.find('>') {
            if let Some(b) = x[gt + 1..].find(&close) {
                out.push(&x[gt + 1..gt + 1 + b]);
                rest = &x[gt + 1 + b + close.len()..];
                continue;
            }
        }
        break;
    }
    out
}
fn element<'a>(s: &'a str, n: &str) -> Option<&'a str> {
    elements(s, n).into_iter().next()
}
fn opening_attr(s: &str, n: &str, a: &str) -> Option<String> {
    let start = s.find(&format!("<{n}"))?;
    let end = s[start..].find('>')? + start;
    attr(&s[start..=end], a)
}
fn opening_attrs(
    s: &str,
    n: &str,
    a: &str,
    b: &str,
    required: &str,
) -> Vec<(String, Option<String>)> {
    let mut o = Vec::new();
    let mut r = s;
    while let Some(i) = r.find(&format!("<{n}")) {
        let x = &r[i..];
        let Some(e) = x.find('>') else { break };
        let tag = &x[..=e];
        if required.is_empty() || attr(tag, b).as_deref() == Some(required) {
            if let Some(v) = attr(tag, a) {
                o.push((v, attr(tag, b)))
            }
        }
        r = &x[e + 1..]
    }
    o
}
fn attr(s: &str, n: &str) -> Option<String> {
    for p in s.split_whitespace() {
        if let Some((k, v)) = p.split_once('=') {
            if k.trim_start_matches('<').eq_ignore_ascii_case(n) {
                return Some(v.trim_matches(['\'', '"', '>', '/']).to_owned());
            }
        }
    }
    None
}
fn unescape(s: &str) -> String {
    s.replace("<![CDATA[", "")
        .replace("]]>", "")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .trim()
        .to_owned()
}
fn depth(v: &serde_json::Value, d: usize, m: usize) -> Result<(), FeedError> {
    if d > m {
        return Err(FeedError("feed depth limit exceeded".into()));
    }
    match v {
        serde_json::Value::Array(a) => {
            for x in a {
                depth(x, d + 1, m)?
            }
        }
        serde_json::Value::Object(o) => {
            for x in o.values() {
                depth(x, d + 1, m)?
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dialects_and_extensions() {
        let rss=br#"<rss version='2.0'><channel><title>T</title><item><guid>1</guid><title>A</title><x:v>z</x:v></item></channel></rss>"#;
        assert_eq!(
            decode_feed(rss, &Default::default()).unwrap().entries[0].id,
            "1"
        );
        let json=br#"{"version":"https://jsonfeed.org/version/1.1","title":"T","items":[{"id":"x","_vendor":7}]}"#;
        assert!(
            decode_feed(json, &Default::default()).unwrap().entries[0]
                .extensions
                .contains_key("_vendor")
        );
    }
}
