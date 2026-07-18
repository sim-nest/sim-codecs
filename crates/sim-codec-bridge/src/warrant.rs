use std::collections::BTreeSet;

use sim_kernel::{ContentId, Datum, Error, Expr, NumberLiteral, Result, Symbol};

use crate::{
    AuthorityClass, BridgeBook, BridgeFrameBook, BridgeMoveBook, BridgePacket, BridgePartSpec,
    FrameHoleKind, FrameKind, FrameSpec, RenderClass, ReplyRule, UnknownPolicy,
};

/// Policy for checking packet warrants at receive time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeWarrantPolicy {
    /// The receiver already trusts that it shares the sender's books.
    SharedTrust,
    /// The receiver checks packet warrant content ids against its local books.
    Verify,
}

/// Content ids of protocol books and part specs that build a packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeWarrant {
    /// Content id of the dialogue move book.
    pub moves: ContentId,
    /// Content id of the fluent frame book.
    pub frames: ContentId,
    /// Content ids of the part specs used by this packet, keyed by part kind.
    pub parts: Vec<(Symbol, ContentId)>,
}

/// Builds the warrant for `packet` from the local bridge book.
pub fn warrant_for_packet(book: &BridgeBook, packet: &BridgePacket) -> Result<BridgeWarrant> {
    let mut used_parts = BTreeSet::new();
    for part in &packet.body {
        used_parts.insert(part.kind.clone());
    }
    let mut parts = Vec::new();
    for kind in used_parts {
        let spec = book
            .parts
            .spec(&kind)
            .ok_or_else(|| Error::Eval(format!("unknown BRIDGE part kind {kind}")))?;
        parts.push((kind, part_spec_content_id(spec)?));
    }
    Ok(BridgeWarrant {
        moves: move_book_content_id(&book.moves)?,
        frames: frame_book_content_id(&book.frames)?,
        parts,
    })
}

/// Computes the content id for a dialogue move book registry record.
pub fn move_book_content_id(book: &BridgeMoveBook) -> Result<ContentId> {
    Datum::Node {
        tag: Symbol::qualified("bridge", "MoveBook"),
        fields: vec![(
            Symbol::new("moves"),
            Datum::Vector(book.specs().map(move_spec_datum).collect()),
        )],
    }
    .content_id()
}

/// Computes the content id for a fluent frame book registry record.
pub fn frame_book_content_id(book: &BridgeFrameBook) -> Result<ContentId> {
    Datum::Node {
        tag: Symbol::qualified("bridge", "FrameBook"),
        fields: vec![(
            Symbol::new("frames"),
            Datum::Vector(book.specs().map(frame_spec_datum).collect()),
        )],
    }
    .content_id()
}

/// Computes the content id for one part-kind registry record.
pub fn part_spec_content_id(spec: &BridgePartSpec) -> Result<ContentId> {
    part_spec_datum(spec).content_id()
}

pub(crate) fn content_id_datum(id: &ContentId) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("core", "ContentId"),
        fields: vec![
            (
                Symbol::new("algorithm"),
                Datum::Symbol(id.algorithm.clone()),
            ),
            (Symbol::new("bytes"), Datum::Bytes(id.bytes.to_vec())),
        ],
    }
}

pub(crate) fn parse_content_id_string(text: &str) -> Result<ContentId> {
    let (algorithm, hex) = text
        .split_once(':')
        .ok_or_else(|| Error::Eval(format!("content id must contain algorithm:hex: {text}")))?;
    if hex.len() != 64 {
        return Err(Error::Eval(format!(
            "content id digest must be 64 hex digits: {text}"
        )));
    }
    let mut bytes = [0u8; 32];
    for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(pair)
            .map_err(|err| Error::Eval(format!("content id digest is not UTF-8: {err}")))?;
        bytes[index] = u8::from_str_radix(pair, 16)
            .map_err(|err| Error::Eval(format!("content id digest is not hex: {err}")))?;
    }
    Ok(ContentId::from_bytes(parse_symbol(algorithm), bytes))
}

fn move_spec_datum(spec: &crate::BridgeMoveSpec) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("bridge", "MoveSpec"),
        fields: vec![
            (Symbol::new("intent"), Datum::Symbol(spec.intent.clone())),
            (
                Symbol::new("replies-to"),
                reply_rule_datum(&spec.replies_to),
            ),
            (
                Symbol::new("requires-parts"),
                Datum::Vector(
                    spec.requires_parts
                        .iter()
                        .cloned()
                        .map(Datum::Symbol)
                        .collect(),
                ),
            ),
            (
                Symbol::new("requires-any-parts"),
                Datum::Vector(
                    spec.requires_any_parts
                        .iter()
                        .map(|group| {
                            Datum::Vector(group.iter().cloned().map(Datum::Symbol).collect())
                        })
                        .collect(),
                ),
            ),
            (Symbol::new("terminal"), Datum::Bool(spec.terminal)),
        ],
    }
}

fn reply_rule_datum(rule: &ReplyRule) -> Datum {
    match rule {
        ReplyRule::Opens => rule_node("Opens", Vec::new()),
        ReplyRule::Any => rule_node("Any", Vec::new()),
        ReplyRule::AnyNonReceipt => rule_node("AnyNonReceipt", Vec::new()),
        ReplyRule::Only(intents) => rule_node(
            "Only",
            vec![(
                Symbol::new("intents"),
                Datum::Vector(intents.iter().cloned().map(Datum::Symbol).collect()),
            )],
        ),
    }
}

fn rule_node(name: &str, fields: Vec<(Symbol, Datum)>) -> Datum {
    let mut fields = fields;
    fields.insert(
        0,
        (
            Symbol::new("rule"),
            Datum::Symbol(Symbol::qualified("bridge", name)),
        ),
    );
    Datum::Node {
        tag: Symbol::qualified("bridge", "ReplyRule"),
        fields,
    }
}

fn frame_spec_datum(spec: &FrameSpec) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("bridge", "FrameSpec"),
        fields: vec![
            (Symbol::new("id"), Datum::Symbol(spec.id.clone())),
            (
                Symbol::new("kind"),
                Datum::Symbol(frame_kind_symbol(spec.kind)),
            ),
            (Symbol::new("prefix"), Datum::String(spec.prefix.to_owned())),
            (
                Symbol::new("template"),
                Datum::String(spec.template.to_owned()),
            ),
            (
                Symbol::new("holes"),
                Datum::Vector(
                    spec.holes
                        .iter()
                        .map(|hole| Datum::Node {
                            tag: Symbol::qualified("bridge", "FrameHoleSpec"),
                            fields: vec![
                                (Symbol::new("name"), Datum::Symbol(hole.name.clone())),
                                (
                                    Symbol::new("kind"),
                                    Datum::Symbol(frame_hole_kind_symbol(hole.kind)),
                                ),
                            ],
                        })
                        .collect(),
                ),
            ),
            (
                Symbol::new("grammar-priority"),
                Datum::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "u8"),
                    canonical: spec.grammar_priority.to_string(),
                }),
            ),
        ],
    }
}

fn part_spec_datum(spec: &BridgePartSpec) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("bridge", "PartSpec"),
        fields: vec![
            (Symbol::new("kind"), Datum::Symbol(spec.kind.clone())),
            (Symbol::new("shape"), expr_datum(&spec.shape_expr)),
            (
                Symbol::new("render-class"),
                Datum::Symbol(render_class_symbol(&spec.render_class)),
            ),
            (
                Symbol::new("authority-class"),
                Datum::Symbol(authority_class_symbol(&spec.authority_class)),
            ),
            (
                Symbol::new("unknown-policy"),
                Datum::Symbol(unknown_policy_symbol(&spec.unknown_policy)),
            ),
        ],
    }
}

fn expr_datum(expr: &Expr) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("bridge", "ExprJson"),
        fields: vec![(
            Symbol::new("json"),
            Datum::String(
                serde_json::to_string(&sim_codec_json::expr_to_json(expr))
                    .expect("JSON value encodes"),
            ),
        )],
    }
}

fn frame_kind_symbol(kind: FrameKind) -> Symbol {
    let name = match kind {
        FrameKind::Use => "Use",
        FrameKind::Inform => "Inform",
        FrameKind::Task => "Task",
        FrameKind::Require => "Require",
        FrameKind::Forbid => "Forbid",
        FrameKind::Prefer => "Prefer",
        FrameKind::Return => "Return",
        FrameKind::Check => "Check",
    };
    Symbol::qualified("bridge", name)
}

fn frame_hole_kind_symbol(kind: FrameHoleKind) -> Symbol {
    let name = match kind {
        FrameHoleKind::Ref => "Ref",
        FrameHoleKind::Term => "Term",
        FrameHoleKind::Choice => "Choice",
        FrameHoleKind::Path => "Path",
        FrameHoleKind::Number => "Number",
        FrameHoleKind::Prose => "Prose",
    };
    Symbol::qualified("bridge", name)
}

fn render_class_symbol(class: &RenderClass) -> Symbol {
    let name = match class {
        RenderClass::Structural => "Structural",
        RenderClass::Frame => "Frame",
        RenderClass::Data => "Data",
        RenderClass::Evidence => "Evidence",
        RenderClass::Review => "Review",
        RenderClass::Vote => "Vote",
        RenderClass::Patch => "Patch",
        RenderClass::Fetch => "Fetch",
        RenderClass::Return => "Return",
        RenderClass::Receipt => "Receipt",
        RenderClass::Extension => "Extension",
    };
    Symbol::qualified("bridge", name)
}

fn authority_class_symbol(class: &AuthorityClass) -> Symbol {
    let name = match class {
        AuthorityClass::Data => "Data",
        AuthorityClass::Normative => "Normative",
        AuthorityClass::Callable => "Callable",
        AuthorityClass::Evidence => "Evidence",
    };
    Symbol::qualified("bridge", name)
}

fn unknown_policy_symbol(policy: &UnknownPolicy) -> Symbol {
    let name = match policy {
        UnknownPolicy::Reject => "Reject",
        UnknownPolicy::PreserveDataOnly => "PreserveDataOnly",
    };
    Symbol::qualified("bridge", name)
}

fn parse_symbol(text: &str) -> Symbol {
    match text.split_once('/') {
        Some((namespace, name)) if !namespace.is_empty() && !name.is_empty() => {
            Symbol::qualified(namespace.to_owned(), name.to_owned())
        }
        _ => Symbol::new(text.to_owned()),
    }
}
