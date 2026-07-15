use std::collections::BTreeSet;

use sim_kernel::{Error, Expr, Result, Symbol};
use sim_value::build::entry;

/// One operation-led LOOM row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeWeaveRow {
    /// Slot this row defines for later rows.
    pub slot: String,
    /// Operation or frontier head selected by this row.
    pub head: Symbol,
    /// Role bindings carried by this row.
    pub roles: Vec<(Symbol, Expr)>,
}

impl BridgeWeaveRow {
    /// Builds a weave row.
    pub fn new(slot: impl Into<String>, head: Symbol, roles: Vec<(Symbol, Expr)>) -> Self {
        Self {
            slot: slot.into(),
            head,
            roles,
        }
    }

    /// Decodes a weave row expression.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/WeaveRow")?;
        reject_unknown(fields, &["slot", "head", "roles"])?;
        Ok(Self::new(
            required_string(fields, "slot")?.to_owned(),
            required_symbol(fields, "head")?.clone(),
            required_roles(fields, "roles")?,
        ))
    }

    /// Encodes this row as a canonical expression map.
    pub fn to_expr(&self) -> Expr {
        Expr::Map(vec![
            entry("slot", Expr::String(self.slot.clone())),
            entry("head", Expr::Symbol(self.head.clone())),
            entry(
                "roles",
                Expr::Map(
                    self.roles
                        .iter()
                        .map(|(role, value)| (Expr::Symbol(role.clone()), value.clone()))
                        .collect(),
                ),
            ),
        ])
    }
}

/// Typed payload for a `bridge/Weave` part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeWeavePayload {
    /// Ordered LOOM rows.
    pub rows: Vec<BridgeWeaveRow>,
    /// Checker-owned result Shape derived from `rows`.
    pub result_shape: Expr,
}

impl BridgeWeavePayload {
    /// Builds a weave payload, deriving the result Shape from its rows.
    pub fn new(rows: Vec<BridgeWeaveRow>) -> Self {
        let result_shape = derive_weave_result_shape(&rows);
        Self { rows, result_shape }
    }

    /// Decodes a weave payload expression and rejects any handwritten result
    /// Shape that disagrees with the rows.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/Weave payload")?;
        reject_unknown(fields, &["rows", "result-shape"])?;
        let rows = required_vector(fields, "rows")?
            .iter()
            .map(BridgeWeaveRow::from_expr)
            .collect::<Result<Vec<_>>>()?;
        let result_shape = required_field(fields, "result-shape")?.clone();
        let derived = derive_weave_result_shape(&rows);
        if result_shape != derived {
            return Err(Error::Eval(
                "BRIDGE weave result-shape disagrees with derived row Shape".to_owned(),
            ));
        }
        Ok(Self { rows, result_shape })
    }

    /// Encodes this payload as a canonical expression map.
    pub fn to_expr(&self) -> Expr {
        Expr::Map(vec![
            entry(
                "rows",
                Expr::Vector(self.rows.iter().map(BridgeWeaveRow::to_expr).collect()),
            ),
            entry("result-shape", derive_weave_result_shape(&self.rows)),
        ])
    }
}

/// Roadmap-facing alias for a typed weave part payload.
pub type WeavePart = BridgeWeavePayload;

/// Derives the public result Shape descriptor for a sequence of weave rows.
pub fn derive_weave_result_shape(rows: &[BridgeWeaveRow]) -> Expr {
    Expr::Map(vec![
        entry(
            "shape",
            Expr::Symbol(Symbol::qualified("bridge", "WeaveResult")),
        ),
        entry(
            "slots",
            Expr::Vector(
                rows.iter()
                    .map(|row| {
                        Expr::Map(vec![
                            entry("slot", Expr::String(row.slot.clone())),
                            entry("head", Expr::Symbol(row.head.clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

/// Parses and validates a `bridge/Weave` payload.
pub fn validate_weave_payload(payload: &Expr) -> Result<BridgeWeavePayload> {
    let payload = BridgeWeavePayload::from_expr(payload)?;
    if payload.rows.is_empty() {
        return Err(Error::Eval(
            "BRIDGE weave payload must contain at least one row".to_owned(),
        ));
    }
    let mut slots = BTreeSet::new();
    for row in &payload.rows {
        if !is_token(&row.slot) {
            return Err(Error::Eval(format!(
                "BRIDGE weave slot {} must be a token",
                row.slot
            )));
        }
        if !slots.insert(row.slot.clone()) {
            return Err(Error::Eval(format!(
                "BRIDGE weave slot {} is duplicated",
                row.slot
            )));
        }
    }
    Ok(payload)
}

fn map_fields<'a>(expr: &'a Expr, label: &str) -> Result<&'a [(Expr, Expr)]> {
    match expr {
        Expr::Map(fields) => Ok(fields),
        _ => Err(Error::Eval(format!("{label} must be a map"))),
    }
}

fn reject_unknown(fields: &[(Expr, Expr)], allowed: &[&str]) -> Result<()> {
    for (key, _) in fields {
        let Some(name) = field_name(key) else {
            return Err(Error::Eval(
                "BRIDGE weave field keys must be symbols".to_owned(),
            ));
        };
        if !allowed.contains(&name.as_str()) {
            return Err(Error::Eval(format!("unknown BRIDGE weave field {name}")));
        }
    }
    Ok(())
}

fn required_field<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a Expr> {
    fields
        .iter()
        .find_map(|(key, value)| (field_name(key).as_deref() == Some(name)).then_some(value))
        .ok_or_else(|| Error::Eval(format!("BRIDGE weave record is missing {name}")))
}

fn required_symbol<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a Symbol> {
    match required_field(fields, name)? {
        Expr::Symbol(symbol) => Ok(symbol),
        _ => Err(Error::TypeMismatch {
            expected: "symbol",
            found: "non-symbol",
        }),
    }
}

fn required_string<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a str> {
    match required_field(fields, name)? {
        Expr::String(value) => Ok(value),
        _ => Err(Error::TypeMismatch {
            expected: "string",
            found: "non-string",
        }),
    }
}

fn required_vector<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a [Expr]> {
    match required_field(fields, name)? {
        Expr::Vector(items) | Expr::List(items) => Ok(items),
        _ => Err(Error::Eval(format!(
            "BRIDGE weave {name} field must be a vector"
        ))),
    }
}

fn required_roles(fields: &[(Expr, Expr)], name: &str) -> Result<Vec<(Symbol, Expr)>> {
    let Expr::Map(entries) = required_field(fields, name)? else {
        return Err(Error::Eval(format!(
            "BRIDGE weave {name} field must be a map"
        )));
    };
    entries
        .iter()
        .map(|(key, value)| match key {
            Expr::Symbol(symbol) => Ok((symbol.clone(), value.clone())),
            _ => Err(Error::Eval(
                "BRIDGE weave role keys must be symbols".to_owned(),
            )),
        })
        .collect()
}

fn field_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Symbol(symbol) => Some(symbol.name.to_string()),
        Expr::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_ascii() && !ch.is_whitespace() && !matches!(ch, '{' | '}'))
}
