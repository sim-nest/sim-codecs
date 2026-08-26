//! RFC 9309 percent normalization and wildcard matching.

pub(super) fn normalize_percent(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            if v.is_ascii_alphanumeric() || matches!(v, b'-' | b'.' | b'_' | b'~') {
                out.push(v as char)
            } else {
                out.push_str(&format!("%{v:02X}"))
            }
            i += 3;
            continue;
        }
        out.push(b[i] as char);
        i += 1
    }
    out
}
pub(super) fn match_len(p: &str) -> usize {
    p.as_bytes()
        .iter()
        .filter(|&&b| b != b'*' && b != b'$')
        .count()
}
pub(super) fn pattern_matches(pattern: &str, path: &str) -> bool {
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
