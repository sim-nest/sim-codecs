//! Origin and trivia helpers for Algol parsing: build and attach `Origin`
//! spans and surrounding trivia to located expression trees so source layout
//! round-trips.

use sim_kernel::{Error, Origin, Result, SourceId, Span, Trivia};

pub(crate) fn origin_from_algol_source(
    codec: sim_kernel::CodecId,
    source_id: SourceId,
    source: &str,
) -> Result<Origin> {
    let (start, leading) = scan_algol_prefix_trivia(source)?;
    let (end, trailing) = scan_algol_suffix_trivia(source, start)?;
    let mut trivia = leading;
    trivia.extend(trailing);
    Ok(Origin {
        codec,
        source: source_id,
        span: Span { start, end },
        trivia,
    })
}

fn scan_algol_prefix_trivia(source: &str) -> Result<(usize, Vec<Trivia>)> {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut trivia = Vec::new();
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            trivia.push(Trivia::Whitespace(source[start..index].to_owned()));
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            let start = index;
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            trivia.push(Trivia::LineComment(source[start..index].to_owned()));
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let start = index;
            index += 2;
            while index + 1 < bytes.len() {
                if bytes[index] == b'*' && bytes[index + 1] == b'/' {
                    index += 2;
                    break;
                }
                index += 1;
            }
            if index > bytes.len() {
                return Err(Error::Eval("unterminated block comment".to_owned()));
            }
            trivia.push(Trivia::BlockComment(source[start..index].to_owned()));
            continue;
        }
        break;
    }
    Ok((index, trivia))
}

fn scan_algol_suffix_trivia(source: &str, start: usize) -> Result<(usize, Vec<Trivia>)> {
    let mut end = source.len();
    let mut trivia = Vec::new();
    loop {
        let mut changed = false;

        let bytes = source.as_bytes();
        let mut ws_start = end;
        while ws_start > start && bytes[ws_start - 1].is_ascii_whitespace() {
            ws_start -= 1;
        }
        if ws_start != end {
            trivia.push(Trivia::Whitespace(source[ws_start..end].to_owned()));
            end = ws_start;
            changed = true;
        }

        if end >= 2 {
            let prefix = &source[..end];
            let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
            if source[line_start..end].starts_with("//") {
                trivia.push(Trivia::LineComment(source[line_start..end].to_owned()));
                end = line_start;
                continue;
            }
            if let Some(block_start) = prefix.rfind("/*")
                && prefix[block_start..end].ends_with("*/")
                && !prefix[block_start + 2..end - 2].contains("/*")
            {
                trivia.push(Trivia::BlockComment(source[block_start..end].to_owned()));
                end = block_start;
                continue;
            }
        }

        if !changed {
            break;
        }
    }
    trivia.reverse();
    Ok((end, trivia))
}
