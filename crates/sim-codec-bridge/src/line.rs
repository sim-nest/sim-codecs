use std::collections::BTreeMap;

use serde_json::Value as JsonValue;
use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{CodecId, Error, Expr, Result, Symbol};

use crate::identity::content_id_string;
use crate::warrant::parse_content_id_string;
use crate::{BridgeBook, BridgeHeader, BridgePacket, BridgePart, BridgeProvenance, BridgeWarrant};

/// Encodes a BRIDGE packet to the strict `BRIDGE/1` line face.
pub fn encode_bridge_text(packet: &BridgePacket, book: &BridgeBook) -> Result<String> {
    book.validate_packet(packet)?;
    let mut lines = vec![
        "BRIDGE/1".to_owned(),
        format!("CID {}", packet.header.cid.as_deref().unwrap_or("nil")),
        format!("MOVE {}", symbol_text(&packet.header.move_kind)),
        format!("FROM {}", checked_token("FROM", &packet.header.from)?),
        format!("TO {}", string_list(&packet.header.to)?),
        format!("ROLE {}", symbol_text(&packet.header.role)),
        format!("PARENTS {}", string_list(&packet.header.parents)?),
        format!("TASK {}", symbol_text(&packet.header.task)),
        format!("OUTPUT {}", symbol_text(&packet.header.output)),
        format!("CEIL {}", symbol_list(&packet.header.ceiling)),
        format!("CONTEXT {}", symbol_list(&packet.header.context)),
        format!(
            "PROV author={} card={}",
            symbol_text(&packet.header.provenance.author),
            packet.header.provenance.card.as_deref().unwrap_or("nil")
        ),
    ];
    if let Some(warrant) = &packet.warrant {
        lines.push(format!("WARRANT {}", warrant_text(warrant)));
    }
    lines.push("BODY".to_owned());
    for part in &packet.body {
        lines.push(format!(
            "{} {} payload={}",
            part_keyword(&part.kind),
            symbol_text(&part.id),
            payload_text(&part.payload)?
        ));
    }
    lines.push("END".to_owned());
    Ok(format!("{}\n", lines.join("\n")))
}

/// Decodes the strict `BRIDGE/1` line face to a BRIDGE packet.
pub fn decode_bridge_text(text: &str, book: &BridgeBook) -> Result<BridgePacket> {
    let mut lines = text.lines();
    match lines.next() {
        Some("BRIDGE/1") => {}
        _ => {
            return Err(Error::Eval(
                "BRIDGE packet must start with BRIDGE/1".to_owned(),
            ));
        }
    }

    let mut headers = BTreeMap::new();
    let mut body_lines = Vec::new();
    let mut in_body = false;
    let mut ended = false;
    for line in lines {
        if ended {
            if !line.trim().is_empty() {
                return Err(Error::Eval("BRIDGE packet has text after END".to_owned()));
            }
            continue;
        }
        if line == "BODY" {
            if in_body {
                return Err(Error::Eval("duplicate BRIDGE BODY marker".to_owned()));
            }
            in_body = true;
            continue;
        }
        if line == "END" {
            if !in_body {
                return Err(Error::Eval("BRIDGE END before BODY".to_owned()));
            }
            ended = true;
            continue;
        }
        if in_body {
            body_lines.push(line.to_owned());
        } else {
            let (key, value) = split_header(line)?;
            if !is_known_header(key) {
                return Err(Error::Eval(format!("unknown BRIDGE header {key}")));
            }
            if headers.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(Error::Eval(format!("duplicate BRIDGE header {key}")));
            }
        }
    }
    if !ended {
        return Err(Error::Eval("BRIDGE packet is missing END".to_owned()));
    }
    let packet = BridgePacket {
        header: BridgeHeader {
            cid: header(&headers, "CID").and_then(parse_cid)?,
            move_kind: parse_symbol(header(&headers, "MOVE")?),
            from: header(&headers, "FROM")?.to_owned(),
            to: parse_string_list(header(&headers, "TO")?)?,
            role: parse_symbol(header(&headers, "ROLE")?),
            parents: parse_string_list(header(&headers, "PARENTS")?)?,
            task: parse_symbol(header(&headers, "TASK")?),
            output: parse_symbol(header(&headers, "OUTPUT")?),
            ceiling: parse_symbol_list(header(&headers, "CEIL")?)?,
            context: parse_symbol_list(header(&headers, "CONTEXT")?)?,
            provenance: parse_provenance(header(&headers, "PROV")?)?,
        },
        body: body_lines
            .iter()
            .map(|line| parse_part(line, book))
            .collect::<Result<Vec<_>>>()?,
        warrant: match headers.get("WARRANT") {
            Some(value) => Some(parse_warrant(value)?),
            None => None,
        },
    };
    book.validate_packet(&packet)?;
    Ok(packet)
}

fn split_header(line: &str) -> Result<(&str, &str)> {
    line.split_once(' ')
        .ok_or_else(|| Error::Eval(format!("malformed BRIDGE header line {line:?}")))
}

fn is_known_header(header: &str) -> bool {
    matches!(
        header,
        "CID"
            | "MOVE"
            | "FROM"
            | "TO"
            | "ROLE"
            | "PARENTS"
            | "TASK"
            | "OUTPUT"
            | "CEIL"
            | "CONTEXT"
            | "PROV"
            | "WARRANT"
    )
}

fn header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str> {
    headers
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| Error::Eval(format!("BRIDGE packet is missing {name} header")))
}

fn parse_cid(value: &str) -> Result<Option<String>> {
    if value == "nil" {
        Ok(None)
    } else {
        Ok(Some(value.to_owned()))
    }
}

fn parse_provenance(value: &str) -> Result<BridgeProvenance> {
    let mut author = None;
    let mut card = None;
    for item in value.split(' ') {
        let Some((key, value)) = item.split_once('=') else {
            return Err(Error::Eval(format!("malformed BRIDGE provenance {item}")));
        };
        match key {
            "author" => author = Some(parse_symbol(value)),
            "card" => {
                card = Some(if value == "nil" {
                    None
                } else {
                    Some(value.to_owned())
                })
            }
            _ => {
                return Err(Error::Eval(format!(
                    "unknown BRIDGE provenance field {key}"
                )));
            }
        }
    }
    Ok(BridgeProvenance {
        author: author.ok_or_else(|| Error::Eval("BRIDGE provenance missing author".to_owned()))?,
        card: card.ok_or_else(|| Error::Eval("BRIDGE provenance missing card".to_owned()))?,
    })
}

fn warrant_text(warrant: &BridgeWarrant) -> String {
    format!(
        "moves={} frames={} parts={}",
        content_id_string(&warrant.moves),
        content_id_string(&warrant.frames),
        warrant_parts_text(&warrant.parts)
    )
}

fn warrant_parts_text(parts: &[(Symbol, sim_kernel::ContentId)]) -> String {
    format!(
        "[{}]",
        parts
            .iter()
            .map(|(kind, id)| format!("{}={}", symbol_text(kind), content_id_string(id)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn parse_warrant(value: &str) -> Result<BridgeWarrant> {
    let mut moves = None;
    let mut frames = None;
    let mut parts = None;
    for item in value.split(' ') {
        let Some((key, value)) = item.split_once('=') else {
            return Err(Error::Eval(format!("malformed BRIDGE warrant {item}")));
        };
        match key {
            "moves" => moves = Some(parse_content_id_string(value)?),
            "frames" => frames = Some(parse_content_id_string(value)?),
            "parts" => parts = Some(parse_warrant_parts(value)?),
            _ => return Err(Error::Eval(format!("unknown BRIDGE warrant field {key}"))),
        }
    }
    Ok(BridgeWarrant {
        moves: moves.ok_or_else(|| Error::Eval("BRIDGE warrant missing moves".to_owned()))?,
        frames: frames.ok_or_else(|| Error::Eval("BRIDGE warrant missing frames".to_owned()))?,
        parts: parts.ok_or_else(|| Error::Eval("BRIDGE warrant missing parts".to_owned()))?,
    })
}

fn parse_warrant_parts(text: &str) -> Result<Vec<(Symbol, sim_kernel::ContentId)>> {
    parse_list(text)?
        .into_iter()
        .map(|item| {
            let (kind, cid) = item
                .split_once('=')
                .ok_or_else(|| Error::Eval(format!("malformed BRIDGE warrant part {item}")))?;
            Ok((parse_symbol(kind), parse_content_id_string(cid)?))
        })
        .collect()
}

fn parse_part(line: &str, book: &BridgeBook) -> Result<BridgePart> {
    let mut fields = line.splitn(3, ' ');
    let keyword = fields
        .next()
        .ok_or_else(|| Error::Eval("empty BRIDGE part line".to_owned()))?;
    let id = fields
        .next()
        .ok_or_else(|| Error::Eval(format!("BRIDGE part {keyword} missing id")))?;
    let rest = fields
        .next()
        .ok_or_else(|| Error::Eval(format!("BRIDGE part {keyword} missing payload")))?;
    let payload = rest
        .strip_prefix("payload=")
        .ok_or_else(|| Error::Eval(format!("BRIDGE part {keyword} has unknown field")))?;
    let kind = kind_from_keyword(keyword);
    book.parts.require_registered(&kind)?;
    let payload = parse_payload(payload)?;
    match &kind {
        kind if *kind == Symbol::qualified("bridge", "Frame") => {
            book.frames.validate_payload(&payload)?;
        }
        kind if *kind == Symbol::qualified("bridge", "Call") => {
            crate::validate_call_payload(&payload)?;
        }
        kind if *kind == Symbol::qualified("bridge", "Weave") => {
            crate::validate_weave_payload(&payload)?;
        }
        kind if collab_part(kind) => {
            crate::validate_collab_payload(kind, &payload)?;
        }
        _ => {}
    }
    Ok(BridgePart {
        id: parse_symbol(id),
        kind,
        payload,
    })
}

fn payload_text(expr: &Expr) -> Result<String> {
    serde_json::to_string(&sim_codec_json::expr_to_json(expr))
        .map_err(|err| Error::Eval(format!("encode BRIDGE payload JSON: {err}")))
}

fn parse_payload(text: &str) -> Result<Expr> {
    let value = serde_json::from_str::<JsonValue>(text)
        .map_err(|err| Error::Eval(format!("parse BRIDGE payload JSON: {err}")))?;
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    sim_codec_json::json_to_expr(CodecId(0), &value, &mut budget, 0)
}

fn symbol_text(symbol: &Symbol) -> String {
    symbol.as_qualified_str()
}

fn parse_symbol(text: &str) -> Symbol {
    match text.split_once('/') {
        Some((namespace, name)) if !namespace.is_empty() && !name.is_empty() => {
            Symbol::qualified(namespace.to_owned(), name.to_owned())
        }
        _ => Symbol::new(text.to_owned()),
    }
}

fn string_list(items: &[String]) -> Result<String> {
    let tokens = items
        .iter()
        .map(|item| checked_token("list item", item))
        .collect::<Result<Vec<_>>>()?;
    Ok(format!("[{}]", tokens.join(",")))
}

fn symbol_list(items: &[Symbol]) -> String {
    format!(
        "[{}]",
        items.iter().map(symbol_text).collect::<Vec<_>>().join(",")
    )
}

fn parse_string_list(text: &str) -> Result<Vec<String>> {
    parse_list(text).map(|items| items.into_iter().map(str::to_owned).collect())
}

fn parse_symbol_list(text: &str) -> Result<Vec<Symbol>> {
    parse_list(text).map(|items| items.into_iter().map(parse_symbol).collect())
}

fn parse_list(text: &str) -> Result<Vec<&str>> {
    let inner = text
        .strip_prefix('[')
        .and_then(|text| text.strip_suffix(']'))
        .ok_or_else(|| Error::Eval(format!("BRIDGE list must use brackets: {text}")))?;
    if inner.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(inner.split(',').collect())
    }
}

fn checked_token<'a>(label: &str, value: &'a str) -> Result<&'a str> {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '[' | ']' | ','))
    {
        Err(Error::Eval(format!(
            "BRIDGE {label} must be a non-empty token"
        )))
    } else {
        Ok(value)
    }
}

fn part_keyword(kind: &Symbol) -> String {
    kind.name.to_ascii_uppercase()
}

fn kind_from_keyword(keyword: &str) -> Symbol {
    let mut chars = keyword.chars();
    let name = match chars.next() {
        Some(first) => format!(
            "{}{}",
            first.to_ascii_uppercase(),
            chars.as_str().to_ascii_lowercase()
        ),
        None => String::new(),
    };
    Symbol::qualified("bridge", name)
}

fn collab_part(kind: &Symbol) -> bool {
    matches!(
        kind.name.as_ref(),
        "Review" | "Vote" | "Patch" | "Evidence" | "Receipt" | "Attest"
    )
}
