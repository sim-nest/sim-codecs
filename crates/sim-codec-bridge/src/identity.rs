use sim_codec_json::expr_to_json;
use sim_kernel::{ContentId, Datum, Expr, Result, Symbol};

use crate::{BridgePacket, BridgePart};

/// Builds the canonical datum used to hash a packet with `cid` cleared.
pub fn canonical_packet_datum(packet: &BridgePacket) -> Datum {
    let packet = packet.canonicalized();
    Datum::Node {
        tag: Symbol::qualified("bridge", "Packet"),
        fields: vec![
            (
                Symbol::new("header"),
                Datum::Node {
                    tag: Symbol::qualified("bridge", "Header"),
                    fields: vec![
                        (Symbol::new("cid"), Datum::Nil),
                        (Symbol::new("move"), Datum::Symbol(packet.header.move_kind)),
                        (Symbol::new("from"), Datum::String(packet.header.from)),
                        (
                            Symbol::new("to"),
                            Datum::Vector(
                                packet.header.to.into_iter().map(Datum::String).collect(),
                            ),
                        ),
                        (Symbol::new("role"), Datum::Symbol(packet.header.role)),
                        (
                            Symbol::new("parents"),
                            Datum::Vector(
                                packet
                                    .header
                                    .parents
                                    .into_iter()
                                    .map(Datum::String)
                                    .collect(),
                            ),
                        ),
                        (Symbol::new("task"), Datum::Symbol(packet.header.task)),
                        (Symbol::new("output"), Datum::Symbol(packet.header.output)),
                        (
                            Symbol::new("ceiling"),
                            Datum::Vector(
                                packet
                                    .header
                                    .ceiling
                                    .into_iter()
                                    .map(Datum::Symbol)
                                    .collect(),
                            ),
                        ),
                        (
                            Symbol::new("context"),
                            Datum::Vector(
                                packet
                                    .header
                                    .context
                                    .into_iter()
                                    .map(Datum::Symbol)
                                    .collect(),
                            ),
                        ),
                        (
                            Symbol::new("provenance"),
                            Datum::Node {
                                tag: Symbol::qualified("bridge", "Provenance"),
                                fields: vec![
                                    (
                                        Symbol::new("author"),
                                        Datum::Symbol(packet.header.provenance.author),
                                    ),
                                    (
                                        Symbol::new("card"),
                                        packet
                                            .header
                                            .provenance
                                            .card
                                            .map(Datum::String)
                                            .unwrap_or(Datum::Nil),
                                    ),
                                ],
                            },
                        ),
                    ],
                },
            ),
            (
                Symbol::new("body"),
                Datum::Vector(packet.body.iter().map(part_datum).collect()),
            ),
            (
                Symbol::new("warrant"),
                packet
                    .warrant
                    .map(|warrant| Datum::Node {
                        tag: Symbol::qualified("bridge", "Warrant"),
                        fields: vec![(
                            Symbol::new("content-ids"),
                            Datum::Vector(
                                warrant.content_ids.into_iter().map(Datum::String).collect(),
                            ),
                        )],
                    })
                    .unwrap_or(Datum::Nil),
            ),
        ],
    }
}

/// Computes the canonical content id for a packet with `cid` cleared.
pub fn packet_content_id(packet: &BridgePacket) -> Result<ContentId> {
    canonical_packet_datum(packet).content_id()
}

/// Returns the stable text form of a content id.
pub fn content_id_string(id: &ContentId) -> String {
    format!("{}:{}", id.algorithm.as_qualified_str(), hex(&id.bytes))
}

/// Returns a packet with its canonical cid stamped into the header.
pub fn stamp_packet_cid(packet: &BridgePacket) -> Result<BridgePacket> {
    let mut packet = packet.canonicalized();
    packet.header.cid = Some(content_id_string(&packet_content_id(&packet)?));
    Ok(packet)
}

/// Verifies that a packet's stamped cid matches its canonical content.
pub fn verify_packet_cid(packet: &BridgePacket) -> Result<()> {
    let expected = content_id_string(&packet_content_id(packet)?);
    match &packet.header.cid {
        Some(actual) if actual == &expected => Ok(()),
        Some(actual) => Err(sim_kernel::Error::Eval(format!(
            "BRIDGE packet cid mismatch: expected {expected}, found {actual}"
        ))),
        None => Err(sim_kernel::Error::Eval(
            "BRIDGE packet has no cid to verify".to_owned(),
        )),
    }
}

fn part_datum(part: &BridgePart) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("bridge", "Part"),
        fields: vec![
            (Symbol::new("id"), Datum::Symbol(part.id.clone())),
            (Symbol::new("kind"), Datum::Symbol(part.kind.clone())),
            (Symbol::new("payload"), expr_datum(&part.payload)),
        ],
    }
}

fn expr_datum(expr: &Expr) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("bridge", "ExprJson"),
        fields: vec![(
            Symbol::new("json"),
            Datum::String(serde_json::to_string(&expr_to_json(expr)).expect("JSON value encodes")),
        )],
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
