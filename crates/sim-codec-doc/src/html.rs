//! Bounded, inert HTML projection into the shared markup model.
//! conformance: bounded HTML decoding produces the shared document model.

use sim_kernel::Expr;

use crate::{
    BackendId, Inline, MarkupBackend, MarkupBlock, MarkupDecodeOptions, MarkupDoc,
    MarkupEncodeOptions, MarkupError, MarkupFidelity, SourceDoc, Span, SpanState,
};

/// Resource limits and an optional authoritative HTTP charset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HtmlDecodeOptions {
    /// Maximum accepted source bytes.
    pub max_input_bytes: usize,
    /// Maximum tags examined.
    pub max_nodes: usize,
    /// Maximum element nesting.
    pub max_depth: usize,
    /// Maximum normalized text bytes.
    pub max_text_bytes: usize,
    /// Charset supplied by HTTP, which takes precedence over a document declaration.
    pub http_charset: Option<String>,
}

impl Default for HtmlDecodeOptions {
    fn default() -> Self {
        Self {
            max_input_bytes: 2 * 1024 * 1024,
            max_nodes: 100_000,
            max_depth: 256,
            max_text_bytes: 1024 * 1024,
            http_charset: None,
        }
    }
}

/// Tolerant HTML backend. It never resolves URLs or executes active content.
#[derive(Clone, Debug, Default)]
pub struct HtmlBackend;

impl MarkupBackend for HtmlBackend {
    fn id(&self) -> BackendId {
        BackendId::new("html")
    }
    fn decode(
        &self,
        input: &str,
        opts: &MarkupDecodeOptions,
    ) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
        decode_html_text(
            input,
            opts.preserve_source,
            HtmlDecodeOptions::default(),
            Vec::new(),
        )
    }
    fn encode(
        &self,
        doc: &MarkupDoc,
        _opts: &MarkupEncodeOptions,
    ) -> Result<(String, MarkupFidelity), MarkupError> {
        if let Some(source) = &doc.source {
            if source.backend.as_str() == "html" {
                return Ok((source.text.clone(), MarkupFidelity::exact(self.id())));
            }
        }
        Err(MarkupError::Encode(
            "HTML is an extraction backend; encoding requires preserved HTML source".into(),
        ))
    }
}

/// Decode HTML bytes with HTTP/document charset precedence and replacement warnings.
pub fn decode_html_bytes(
    input: &[u8],
    opts: &HtmlDecodeOptions,
) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
    if input.len() > opts.max_input_bytes {
        return Err(MarkupError::Decode("HTML input byte limit exceeded".into()));
    }
    let declared = opts.http_charset.clone().or_else(|| sniff_charset(input));
    let mut warnings = Vec::new();
    let text = match declared.as_deref().map(|v| v.to_ascii_lowercase()) {
        Some(label) if label == "iso-8859-1" || label == "windows-1252" => {
            input.iter().map(|&b| char::from(b)).collect()
        }
        Some(label) if label != "utf-8" && label != "utf8" => {
            warnings.push(format!("unsupported charset {label}; decoded as UTF-8"));
            String::from_utf8_lossy(input).into_owned()
        }
        _ => String::from_utf8_lossy(input).into_owned(),
    };
    if std::str::from_utf8(input).is_err()
        && !matches!(declared.as_deref(), Some("iso-8859-1" | "windows-1252"))
    {
        warnings.push("invalid UTF-8 replaced during decode".into());
    }
    decode_html_text(&text, true, opts.clone(), warnings)
}

fn sniff_charset(input: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&input[..input.len().min(4096)]).to_ascii_lowercase();
    let at = head.find("charset=")? + 8;
    Some(
        head[at..]
            .trim_start_matches(['\'', '"'])
            .split(|c: char| c == '\'' || c == '"' || c == ';' || c.is_whitespace() || c == '>')
            .next()?
            .to_owned(),
    )
}

fn decode_html_text(
    input: &str,
    preserve_source: bool,
    limits: HtmlDecodeOptions,
    warnings: Vec<String>,
) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
    if input.len() > limits.max_input_bytes {
        return Err(MarkupError::Decode("HTML input byte limit exceeded".into()));
    }
    let mut p = Parser {
        source: input,
        pos: 0,
        nodes: 0,
        depth: 0,
        text_bytes: 0,
        limits,
        blocks: Vec::new(),
        stack: Vec::new(),
        title: None,
        attrs: Default::default(),
        warnings,
        suppressed: 0,
    };
    p.parse()?;
    p.extract_structures();
    let mut fidelity = MarkupFidelity::exact(BackendId::new("html"));
    fidelity.warnings = p.warnings;
    let doc = MarkupDoc {
        title: p.title,
        blocks: p.blocks,
        attrs: p.attrs,
        source: preserve_source.then(|| SourceDoc {
            backend: BackendId::new("html"),
            text: input.to_owned(),
        }),
    };
    Ok((doc, fidelity))
}

struct Frame {
    tag: String,
    start: usize,
    text: String,
    href: Option<String>,
    lang: Option<String>,
}
struct Parser<'a> {
    source: &'a str,
    pos: usize,
    nodes: usize,
    depth: usize,
    text_bytes: usize,
    limits: HtmlDecodeOptions,
    blocks: Vec<MarkupBlock>,
    stack: Vec<Frame>,
    title: Option<String>,
    attrs: std::collections::BTreeMap<String, Expr>,
    warnings: Vec<String>,
    suppressed: usize,
}
impl Parser<'_> {
    fn extract_structures(&mut self) {
        for (tag, ordered) in [("ul", false), ("ol", true)] {
            for list in html_elements(self.source, tag) {
                let items = html_elements(list, "li")
                    .into_iter()
                    .map(|item| {
                        vec![MarkupBlock::Paragraph {
                            content: vec![Inline::Text(normalize(&strip_tags(item)))],
                            span: None,
                        }]
                    })
                    .collect::<Vec<_>>();
                if !items.is_empty() {
                    self.blocks.push(MarkupBlock::List {
                        ordered,
                        items,
                        span: None,
                    });
                }
            }
        }
        for table in html_elements(self.source, "table") {
            let mut rows = html_elements(table, "tr")
                .into_iter()
                .map(|row| {
                    let mut cells = html_elements(row, "th");
                    if cells.is_empty() {
                        cells = html_elements(row, "td");
                    }
                    cells
                        .into_iter()
                        .map(|cell| vec![Inline::Text(normalize(&strip_tags(cell)))])
                        .collect::<Vec<_>>()
                })
                .filter(|r| !r.is_empty())
                .collect::<Vec<_>>();
            if !rows.is_empty() {
                let header = rows.remove(0);
                self.blocks.push(MarkupBlock::Table {
                    header,
                    rows,
                    span: None,
                });
            }
        }
        let readable = self
            .blocks
            .iter()
            .filter_map(|b| match b {
                MarkupBlock::Heading { text, .. }
                | MarkupBlock::Paragraph { content: text, .. } => Some(
                    text.iter()
                        .filter_map(|i| {
                            if let Inline::Text(v) = i {
                                Some(v.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.attrs
            .insert("readable-text".into(), Expr::String(readable));
    }
    fn parse(&mut self) -> Result<(), MarkupError> {
        while self.pos < self.source.len() {
            if self.source.as_bytes()[self.pos] == b'<' {
                self.tag()?;
            } else {
                self.text()?;
            }
        }
        while let Some(frame) = self.stack.pop() {
            self.finish(frame, self.source.len());
        }
        Ok(())
    }
    fn tag(&mut self) -> Result<(), MarkupError> {
        let start = self.pos;
        let Some(rel) = self.source[start..].find('>') else {
            self.pos = self.source.len();
            return Ok(());
        };
        let end = start + rel + 1;
        self.nodes += 1;
        if self.nodes > self.limits.max_nodes {
            return Err(MarkupError::Decode("HTML node limit exceeded".into()));
        }
        let raw = &self.source[start + 1..end - 1];
        self.pos = end;
        if raw.starts_with('!') || raw.starts_with('?') {
            return Ok(());
        }
        let closing = raw.trim_start().starts_with('/');
        let body = raw.trim().trim_start_matches('/').trim();
        let name = body
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches('/')
            .to_ascii_lowercase();
        if name.is_empty() {
            return Ok(());
        }
        if closing {
            if let Some(ix) = self.stack.iter().rposition(|f| f.tag == name) {
                while self.stack.len() > ix {
                    let f = self.stack.pop().unwrap();
                    self.finish(f, end);
                }
            }
            return Ok(());
        }
        if name == "meta" {
            if let Some(v) = attr(body, "name").zip(attr(body, "content")) {
                self.attrs.insert(
                    format!("meta:{}", v.0.to_ascii_lowercase()),
                    Expr::String(v.1),
                );
            }
        }
        if name == "link" && attr(body, "rel").is_some_and(|v| v.eq_ignore_ascii_case("canonical"))
        {
            if let Some(v) = attr(body, "href") {
                self.attrs.insert("canonical-link".into(), Expr::String(v));
            }
        }
        if name == "html" {
            if let Some(v) = attr(body, "lang") {
                self.attrs.insert("language".into(), Expr::String(v));
            }
        }
        let active = matches!(
            name.as_str(),
            "script" | "style" | "form" | "object" | "embed" | "iframe"
        );
        if active {
            self.suppressed += 1;
            self.warnings
                .push(format!("active or embedded <{name}> content omitted"));
        }
        if body.contains("on")
            && body
                .split_whitespace()
                .any(|a| a.to_ascii_lowercase().starts_with("on") && a.contains('='))
        {
            self.warnings
                .push(format!("event handler stripped from <{name}>"));
        }
        if !body.ends_with('/')
            && !matches!(
                name.as_str(),
                "meta" | "link" | "img" | "br" | "hr" | "input" | "source"
            )
        {
            self.depth += 1;
            if self.depth > self.limits.max_depth {
                return Err(MarkupError::Decode("HTML depth limit exceeded".into()));
            }
            self.stack.push(Frame {
                tag: name,
                start,
                text: String::new(),
                href: attr(body, "href"),
                lang: attr(body, "class")
                    .and_then(|v| v.strip_prefix("language-").map(str::to_owned)),
            });
        }
        Ok(())
    }
    fn text(&mut self) -> Result<(), MarkupError> {
        let end = self.source[self.pos..]
            .find('<')
            .map_or(self.source.len(), |v| self.pos + v);
        let raw = &self.source[self.pos..end];
        self.pos = end;
        if self.suppressed == 0 {
            let decoded = entities(raw);
            self.text_bytes += decoded.len();
            if self.text_bytes > self.limits.max_text_bytes {
                return Err(MarkupError::Decode("HTML text limit exceeded".into()));
            }
            for f in &mut self.stack {
                f.text.push_str(&decoded);
            }
        }
        Ok(())
    }
    fn finish(&mut self, f: Frame, end: usize) {
        self.depth = self.depth.saturating_sub(1);
        if matches!(
            f.tag.as_str(),
            "script" | "style" | "form" | "object" | "embed" | "iframe"
        ) {
            self.suppressed = self.suppressed.saturating_sub(1);
            return;
        }
        let text = normalize(&f.text);
        if text.is_empty() {
            return;
        }
        let span = Some(Span {
            start: f.start,
            end,
            state: SpanState::Preserved,
        });
        let inline = || {
            vec![if let Some(target) = &f.href {
                Inline::Link {
                    label: vec![Inline::Text(text.clone())],
                    target: target.clone(),
                }
            } else {
                Inline::Text(text.clone())
            }]
        };
        match f.tag.as_str() {
            "title" => self.title = Some(text),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.blocks.push(MarkupBlock::Heading {
                level: f.tag[1..].parse().unwrap_or(1),
                text: inline(),
                id: None,
                span,
            }),
            "pre" => self.blocks.push(MarkupBlock::CodeBlock {
                lang: f.lang,
                code: text,
                span,
            }),
            "blockquote" => self.blocks.push(MarkupBlock::Quote {
                blocks: vec![MarkupBlock::Paragraph {
                    content: inline(),
                    span: span.clone(),
                }],
                span,
            }),
            "p" | "li" | "td" | "th" => self.blocks.push(MarkupBlock::Paragraph {
                content: inline(),
                span,
            }),
            _ => {}
        }
    }
}
fn attr(body: &str, wanted: &str) -> Option<String> {
    for token in body.split_whitespace().skip(1) {
        let (k, v) = token.split_once('=')?;
        if k.eq_ignore_ascii_case(wanted) {
            return Some(v.trim_matches(['\'', '"', '>']).to_owned());
        }
    }
    None
}
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}
fn html_elements<'a>(s: &'a str, tag: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut rest = s;
    while let Some(a) = rest.to_ascii_lowercase().find(&open) {
        let x = &rest[a..];
        let Some(gt) = x.find('>') else { break };
        let Some(b) = x[gt + 1..].to_ascii_lowercase().find(&close) else {
            break;
        };
        out.push(&x[gt + 1..gt + 1 + b]);
        rest = &x[gt + 1 + b + close.len()..];
    }
    out
}
fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in s.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    entities(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inert_and_chunk_equivalent() {
        let html=b"<html lang='en'><head><link rel='canonical' href='https://e/x'><script>panic()</script></head><body><h1>Hello &amp; hi</h1><p>Safe <a href='/x'>link</a></p></body></html>";
        let (a, f) = decode_html_bytes(html, &Default::default()).unwrap();
        let joined = [&html[..31], &html[31..]].concat();
        let (b, _) = decode_html_bytes(&joined, &Default::default()).unwrap();
        assert_eq!(a, b);
        assert!(!format!("{:?}", a.blocks).contains("panic"));
        assert!(f.warnings.iter().any(|w| w.contains("omitted")));
    }
}
// conformance: bounded HTML decoding produces the shared document model.
