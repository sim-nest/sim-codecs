//! RFC 9309 robots.txt records and deterministic path matching, without HTTP policy.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Stable runtime codec symbol.
pub const CODEC_SYMBOL: &str = "codec/robots";
/// Stable accepted media-type alias.
pub const MEDIA_TYPES: &[&str] = &["text/plain"];
/// RFC 9309 requires parsers to accept at least this many octets.
pub const RFC_MINIMUM_BYTES: usize = 500 * 1024;
/// Embedded pure recipes.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));
/// Redirect observations supplied by a transport owner, retained only as metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedirectMetadata {
    /// Redirect count.
    pub hops: usize,
    /// Final claimed URL.
    pub final_url: Option<String>,
}
/// Rule disposition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleKind {
    /// Permit matching paths.
    Allow,
    /// Deny matching paths.
    Disallow,
}
/// One path rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// Kind.
    pub kind: RuleKind,
    /// Original pattern.
    pub pattern: String,
}
/// A group of user-agent products and rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// Case-insensitive product tokens.
    pub user_agents: Vec<String>,
    /// Ordered source rules.
    pub rules: Vec<Rule>,
}
/// Parsed robots data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RobotsDoc {
    /// Groups.
    pub groups: Vec<Group>,
    /// Sitemap URL claims.
    pub sitemaps: Vec<String>,
    /// Optional externally supplied redirect metadata.
    pub redirect: Option<RedirectMetadata>,
    /// Decode warnings.
    pub warnings: Vec<String>,
}
/// Parse limits. The default input ceiling honors the RFC floor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RobotsLimits {
    /// Byte ceiling, never less than 500 KiB.
    pub max_input_bytes: usize,
    /// Line ceiling.
    pub max_lines: usize,
    /// Rule ceiling.
    pub max_rules: usize,
}
impl Default for RobotsLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: RFC_MINIMUM_BYTES,
            max_lines: 100_000,
            max_rules: 50_000,
        }
    }
}
/// Parse failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RobotsError(pub String);
impl std::fmt::Display for RobotsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for RobotsError {}

/// Parse robots bytes as inert records. HTTP status and redirect policy remain outside this crate.
pub fn parse_robots(
    input: &[u8],
    limits: &RobotsLimits,
    redirect: Option<RedirectMetadata>,
) -> Result<RobotsDoc, RobotsError> {
    let ceiling = limits.max_input_bytes.max(RFC_MINIMUM_BYTES);
    if input.len() > ceiling {
        return Err(RobotsError("robots input byte limit exceeded".into()));
    }
    let text = String::from_utf8_lossy(input);
    let mut groups = Vec::new();
    let mut agents = Vec::new();
    let mut rules = Vec::new();
    let mut sitemaps = Vec::new();
    let mut warnings = Vec::new();
    let mut count = 0;
    for (line_no, raw) in text.lines().enumerate() {
        if line_no >= limits.max_lines {
            return Err(RobotsError("robots line limit exceeded".into()));
        }
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((field, value)) = line.split_once(':') else {
            warnings.push(format!("line {} has no field separator", line_no + 1));
            continue;
        };
        let field = field.trim().to_ascii_lowercase();
        let value = value.trim();
        match field.as_str() {
            "user-agent" => {
                if !rules.is_empty() {
                    groups.push(Group {
                        user_agents: std::mem::take(&mut agents),
                        rules: std::mem::take(&mut rules),
                    });
                }
                agents.push(value.to_ascii_lowercase())
            }
            "allow" | "disallow" => {
                if agents.is_empty() {
                    warnings.push(format!("line {} rule precedes user-agent", line_no + 1));
                    continue;
                }
                if field == "disallow" && value.is_empty() {
                    continue;
                }
                count += 1;
                if count > limits.max_rules {
                    return Err(RobotsError("robots rule limit exceeded".into()));
                }
                rules.push(Rule {
                    kind: if field == "allow" {
                        RuleKind::Allow
                    } else {
                        RuleKind::Disallow
                    },
                    pattern: value.to_owned(),
                })
            }
            "sitemap" => sitemaps.push(value.to_owned()),
            _ => warnings.push(format!("unknown robots field {field}")),
        }
    }
    if !agents.is_empty() {
        groups.push(Group {
            user_agents: agents,
            rules,
        });
    }
    if std::str::from_utf8(input).is_err() {
        warnings.push("invalid UTF-8 replaced during decode".into())
    }
    Ok(RobotsDoc {
        groups,
        sitemaps,
        redirect,
        warnings,
    })
}
impl RobotsDoc {
    /// Decide access for a product token and URL path. No matching rule means allow.
    pub fn allows(&self, product: &str, path: &str) -> bool {
        let product = product.to_ascii_lowercase();
        let mut best_agent = 0;
        let mut chosen = Vec::new();
        for g in &self.groups {
            let specificity = g
                .user_agents
                .iter()
                .filter_map(|a| {
                    if a == "*" {
                        Some(0)
                    } else if product.contains(a) {
                        Some(a.len())
                    } else {
                        None
                    }
                })
                .max();
            if let Some(n) = specificity {
                if n > best_agent {
                    best_agent = n;
                    chosen.clear();
                }
                if n == best_agent {
                    chosen.push(g)
                }
            }
        }
        let normalized = normalize_percent(path);
        let mut winner: Option<(usize, RuleKind)> = None;
        for g in chosen {
            for r in &g.rules {
                if pattern_matches(&normalize_percent(&r.pattern), &normalized) {
                    let n = match_len(&r.pattern);
                    match winner {
                        None => winner = Some((n, r.kind)),
                        Some((old, _)) if n > old => winner = Some((n, r.kind)),
                        Some((old, RuleKind::Disallow))
                            if n == old && r.kind == RuleKind::Allow =>
                        {
                            winner = Some((n, r.kind))
                        }
                        _ => {}
                    }
                }
            }
        }
        winner.is_none_or(|(_, k)| k == RuleKind::Allow)
    }
}
fn normalize_percent(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                if v.is_ascii_alphanumeric() || matches!(v, b'-' | b'.' | b'_' | b'~') {
                    out.push(v as char)
                } else {
                    out.push_str(&format!("%{v:02X}"))
                }
                i += 3;
                continue;
            }
        }
        out.push(b[i] as char);
        i += 1
    }
    out
}
fn match_len(p: &str) -> usize {
    p.as_bytes()
        .iter()
        .filter(|&&b| b != b'*' && b != b'$')
        .count()
}
fn pattern_matches(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        if p.is_empty() {
            return true;
        }
        if p == b"$" {
            return s.is_empty();
        }
        if p[0] == b'*' {
            return (0..=s.len()).any(|n| go(&p[1..], &s[n..]));
        }
        s.first() == p.first() && go(&p[1..], &s[1..])
    }
    go(pattern.as_bytes(), path.as_bytes())
}

#[cfg(test)]
mod tests {
    // conformance: robots rules apply RFC precedence and matching boundaries.
    use super::*;
    #[test]
    fn precedence_table() {
        let d=parse_robots(b"User-agent: *\nDisallow: /fish\nAllow: /fish$\nDisallow: /fish*heads\nAllow: /fishheads\n",&Default::default(),None).unwrap();
        let cases = [
            ("/", true),
            ("/fish", true),
            ("/fish/", false),
            ("/fishheads", true),
            ("/fishXYZheads", false),
        ];
        for (c, want) in cases {
            assert_eq!(d.allows("bot", c), want, "{c}")
        }
    }
    #[test]
    fn groups_case_and_percent() {
        let d=parse_robots(b"User-agent: Bot\nDisallow: /a%2fb\nUser-agent: *\nAllow: /\nSitemap: https://e/s.xml\n",&Default::default(),None).unwrap();
        assert!(!d.allows("MyBOT", "/a%2Fb"));
        assert!(d.allows("other", "/a%2Fb"));
        assert_eq!(d.sitemaps.len(), 1)
    }
}
