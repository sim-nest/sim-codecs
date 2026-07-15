use std::collections::BTreeSet;

use sim_kernel::{Error, Expr, Result, Symbol};
use sim_value::build::entry as field;

/// One BRIDGE packet, carrying a header, ordered typed parts, and optional warrant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgePacket {
    /// Fixed packet header.
    pub header: BridgeHeader,
    /// Ordered packet body parts.
    pub body: Vec<BridgePart>,
    /// Optional cross-trust warrant, populated by later bridge layers.
    pub warrant: Option<BridgeWarrant>,
}

/// Fixed, fast-scan BRIDGE packet header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeHeader {
    /// Packet content id as rendered text, or `None` while canonicalizing.
    pub cid: Option<String>,
    /// Dialogue move intent.
    pub move_kind: Symbol,
    /// Sender seat.
    pub from: String,
    /// Receiver seats.
    pub to: Vec<String>,
    /// Sender role for this packet.
    pub role: Symbol,
    /// Parent packet content ids.
    pub parents: Vec<String>,
    /// Header task part id.
    pub task: Symbol,
    /// Header output/return part id.
    pub output: Symbol,
    /// Capability ceiling names, resolved by later bridge runtime code.
    pub ceiling: Vec<Symbol>,
    /// Context part ids available to this packet.
    pub context: Vec<Symbol>,
    /// Packet provenance.
    pub provenance: BridgeProvenance,
}

/// Provenance carried in the BRIDGE header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeProvenance {
    /// Author seat or system symbol.
    pub author: Symbol,
    /// Optional card id or descriptor.
    pub card: Option<String>,
}

/// One typed body part in a BRIDGE packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgePart {
    /// Part id, referenced by header fields and other parts.
    pub id: Symbol,
    /// Registered part kind.
    pub kind: Symbol,
    /// Part payload expression.
    pub payload: Expr,
}

/// Content ids of protocol books and specs used to build a packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeWarrant {
    /// Content ids of books and specs.
    pub content_ids: Vec<String>,
}

impl BridgePacket {
    /// Returns this packet with its header cid cleared for canonical hashing.
    pub fn canonicalized(&self) -> Self {
        let mut packet = self.clone();
        packet.header.cid = None;
        packet
    }
}

impl Default for BridgeProvenance {
    fn default() -> Self {
        Self {
            author: Symbol::new("sim"),
            card: None,
        }
    }
}

/// Project a packet to the canonical expression record used by `codec:bridge`.
pub fn packet_to_expr(packet: &BridgePacket) -> Expr {
    Expr::Map(vec![
        field("bridge", Expr::String("1".to_owned())),
        field("header", header_to_expr(&packet.header)),
        field(
            "body",
            Expr::Vector(packet.body.iter().map(part_to_expr).collect()),
        ),
        field(
            "warrant",
            packet
                .warrant
                .as_ref()
                .map(warrant_to_expr)
                .unwrap_or(Expr::Nil),
        ),
    ])
}

/// Validate a canonical packet expression back into a typed packet.
pub fn expr_to_packet(expr: &Expr) -> Result<BridgePacket> {
    let fields = map_fields(expr, "BRIDGE packet")?;
    reject_unknown(fields, &["bridge", "header", "body", "warrant"])?;
    match required_field(fields, "bridge")? {
        Expr::String(version) if version == "1" => {}
        _ => {
            return Err(Error::Eval(
                "BRIDGE packet must declare bridge version 1".to_owned(),
            ));
        }
    }
    Ok(BridgePacket {
        header: header_from_expr(required_field(fields, "header")?)?,
        body: vector_field(required_field(fields, "body")?, "body")?
            .iter()
            .map(part_from_expr)
            .collect::<Result<Vec<_>>>()?,
        warrant: match required_field(fields, "warrant")? {
            Expr::Nil => None,
            value => Some(warrant_from_expr(value)?),
        },
    })
}

fn header_to_expr(header: &BridgeHeader) -> Expr {
    Expr::Map(vec![
        field(
            "cid",
            header
                .cid
                .as_ref()
                .map(|cid| Expr::String(cid.clone()))
                .unwrap_or(Expr::Nil),
        ),
        field("move", Expr::Symbol(header.move_kind.clone())),
        field("from", Expr::String(header.from.clone())),
        field(
            "to",
            Expr::Vector(
                header
                    .to
                    .iter()
                    .map(|value| Expr::String(value.clone()))
                    .collect(),
            ),
        ),
        field("role", Expr::Symbol(header.role.clone())),
        field(
            "parents",
            Expr::Vector(
                header
                    .parents
                    .iter()
                    .map(|value| Expr::String(value.clone()))
                    .collect(),
            ),
        ),
        field("task", Expr::Symbol(header.task.clone())),
        field("output", Expr::Symbol(header.output.clone())),
        field(
            "ceiling",
            Expr::Vector(
                header
                    .ceiling
                    .iter()
                    .map(|symbol| Expr::Symbol(symbol.clone()))
                    .collect(),
            ),
        ),
        field(
            "context",
            Expr::Vector(
                header
                    .context
                    .iter()
                    .map(|symbol| Expr::Symbol(symbol.clone()))
                    .collect(),
            ),
        ),
        field("provenance", provenance_to_expr(&header.provenance)),
    ])
}

fn header_from_expr(expr: &Expr) -> Result<BridgeHeader> {
    let fields = map_fields(expr, "BRIDGE header")?;
    reject_unknown(
        fields,
        &[
            "cid",
            "move",
            "from",
            "to",
            "role",
            "parents",
            "task",
            "output",
            "ceiling",
            "context",
            "provenance",
        ],
    )?;
    Ok(BridgeHeader {
        cid: optional_string_or_nil(fields, "cid")?,
        move_kind: required_symbol(fields, "move")?,
        from: required_string(fields, "from")?,
        to: required_string_vector(fields, "to")?,
        role: required_symbol(fields, "role")?,
        parents: required_string_vector(fields, "parents")?,
        task: required_symbol(fields, "task")?,
        output: required_symbol(fields, "output")?,
        ceiling: required_symbol_vector(fields, "ceiling")?,
        context: required_symbol_vector(fields, "context")?,
        provenance: provenance_from_expr(required_field(fields, "provenance")?)?,
    })
}

fn part_to_expr(part: &BridgePart) -> Expr {
    Expr::Map(vec![
        field("id", Expr::Symbol(part.id.clone())),
        field("kind", Expr::Symbol(part.kind.clone())),
        field("payload", part.payload.clone()),
    ])
}

fn part_from_expr(expr: &Expr) -> Result<BridgePart> {
    let fields = map_fields(expr, "BRIDGE part")?;
    reject_unknown(fields, &["id", "kind", "payload"])?;
    Ok(BridgePart {
        id: required_symbol(fields, "id")?,
        kind: required_symbol(fields, "kind")?,
        payload: required_field(fields, "payload")?.clone(),
    })
}

fn provenance_to_expr(provenance: &BridgeProvenance) -> Expr {
    Expr::Map(vec![
        field("author", Expr::Symbol(provenance.author.clone())),
        field(
            "card",
            provenance
                .card
                .as_ref()
                .map(|card| Expr::String(card.clone()))
                .unwrap_or(Expr::Nil),
        ),
    ])
}

fn provenance_from_expr(expr: &Expr) -> Result<BridgeProvenance> {
    let fields = map_fields(expr, "BRIDGE provenance")?;
    reject_unknown(fields, &["author", "card"])?;
    Ok(BridgeProvenance {
        author: required_symbol(fields, "author")?,
        card: optional_string_or_nil(fields, "card")?,
    })
}

fn warrant_to_expr(warrant: &BridgeWarrant) -> Expr {
    Expr::Map(vec![field(
        "content-ids",
        Expr::Vector(
            warrant
                .content_ids
                .iter()
                .map(|id| Expr::String(id.clone()))
                .collect(),
        ),
    )])
}

fn warrant_from_expr(expr: &Expr) -> Result<BridgeWarrant> {
    let fields = map_fields(expr, "BRIDGE warrant")?;
    reject_unknown(fields, &["content-ids"])?;
    Ok(BridgeWarrant {
        content_ids: required_string_vector(fields, "content-ids")?,
    })
}

fn required_symbol(fields: &[(Expr, Expr)], name: &str) -> Result<Symbol> {
    match required_field(fields, name)? {
        Expr::Symbol(symbol) => Ok(symbol.clone()),
        _ => Err(Error::TypeMismatch {
            expected: "symbol",
            found: "non-symbol",
        }),
    }
}

fn required_string(fields: &[(Expr, Expr)], name: &str) -> Result<String> {
    match required_field(fields, name)? {
        Expr::String(value) => Ok(value.clone()),
        _ => Err(Error::TypeMismatch {
            expected: "string",
            found: "non-string",
        }),
    }
}

fn optional_string_or_nil(fields: &[(Expr, Expr)], name: &str) -> Result<Option<String>> {
    match required_field(fields, name)? {
        Expr::Nil => Ok(None),
        Expr::String(value) => Ok(Some(value.clone())),
        _ => Err(Error::TypeMismatch {
            expected: "string or nil",
            found: "invalid optional string",
        }),
    }
}

fn required_string_vector(fields: &[(Expr, Expr)], name: &str) -> Result<Vec<String>> {
    vector_field(required_field(fields, name)?, name)?
        .iter()
        .map(|value| match value {
            Expr::String(value) => Ok(value.clone()),
            _ => Err(Error::TypeMismatch {
                expected: "string vector",
                found: "non-string item",
            }),
        })
        .collect()
}

fn required_symbol_vector(fields: &[(Expr, Expr)], name: &str) -> Result<Vec<Symbol>> {
    vector_field(required_field(fields, name)?, name)?
        .iter()
        .map(|value| match value {
            Expr::Symbol(symbol) => Ok(symbol.clone()),
            _ => Err(Error::TypeMismatch {
                expected: "symbol vector",
                found: "non-symbol item",
            }),
        })
        .collect()
}

fn vector_field<'a>(expr: &'a Expr, name: &str) -> Result<&'a [Expr]> {
    match expr {
        Expr::Vector(items) | Expr::List(items) => Ok(items),
        _ => Err(Error::Eval(format!("BRIDGE {name} field must be a vector"))),
    }
}

fn required_field<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a Expr> {
    optional_field(fields, name)
        .ok_or_else(|| Error::Eval(format!("BRIDGE record is missing {name}")))
}

fn optional_field<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Option<&'a Expr> {
    fields
        .iter()
        .find_map(|(key, value)| (field_name(key).ok()?.as_str() == name).then_some(value))
}

fn reject_unknown(fields: &[(Expr, Expr)], allowed: &[&str]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (key, _) in fields {
        let name = field_name(key)?;
        if !seen.insert(name.clone()) {
            return Err(Error::Eval(format!("duplicate BRIDGE field {name}")));
        }
        if !allowed.contains(&name.as_str()) {
            return Err(Error::Eval(format!("unknown BRIDGE field {name}")));
        }
    }
    Ok(())
}

fn field_name(expr: &Expr) -> Result<String> {
    match expr {
        Expr::Symbol(symbol) if symbol.namespace.is_none() => Ok(symbol.name.to_string()),
        Expr::String(value) => Ok(value.clone()),
        _ => Err(Error::TypeMismatch {
            expected: "BRIDGE field symbol",
            found: "invalid field key",
        }),
    }
}

use sim_value::access::map_entries as map_fields;
