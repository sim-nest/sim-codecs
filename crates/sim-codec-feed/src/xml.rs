//! Bounded RSS and Atom XML decoding.

use super::*;

pub(super) fn decode_xml(s: &str, l: &FeedLimits) -> Result<FeedDoc, FeedError> {
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
        let authors = elements(c, "author")
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
        if let Some(gt) = x.find('>')
            && let Some(b) = x[gt + 1..].find(&close)
        {
            out.push(&x[gt + 1..gt + 1 + b]);
            rest = &x[gt + 1 + b + close.len()..];
            continue;
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
        if (required.is_empty() || attr(tag, b).as_deref() == Some(required))
            && let Some(v) = attr(tag, a)
        {
            o.push((v, attr(tag, b)))
        }
        r = &x[e + 1..]
    }
    o
}
fn attr(s: &str, n: &str) -> Option<String> {
    for p in s.split_whitespace() {
        if let Some((k, v)) = p.split_once('=')
            && k.trim_start_matches('<').eq_ignore_ascii_case(n)
        {
            return Some(v.trim_matches(['\'', '"', '>', '/']).to_owned());
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
pub(super) fn depth(v: &serde_json::Value, d: usize, m: usize) -> Result<(), FeedError> {
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
