use sim_kernel::{Error, Expr, NumberLiteral, Result, Symbol};
use sim_value::build::entry;

/// One axis in a structured collaboration vote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeScore {
    /// Axis being scored, such as correctness or safety.
    pub axis: Symbol,
    /// Signed score for this axis.
    pub value: i64,
    /// Reviewer rationale for this axis.
    pub reason: String,
}

impl BridgeScore {
    /// Builds a vote score.
    pub fn new(axis: Symbol, value: i64, reason: impl Into<String>) -> Self {
        Self {
            axis,
            value,
            reason: reason.into(),
        }
    }

    /// Decodes a score expression.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/Score")?;
        reject_unknown(fields, &["axis", "value", "reason"])?;
        Ok(Self::new(
            required_symbol(fields, "axis")?.clone(),
            required_i64(fields, "value")?,
            required_string(fields, "reason")?.to_owned(),
        ))
    }

    /// Encodes this score as an expression map.
    pub fn to_expr(&self) -> Expr {
        Expr::Map(vec![
            entry("axis", Expr::Symbol(self.axis.clone())),
            entry("value", i64_expr(self.value)),
            entry("reason", Expr::String(self.reason.clone())),
        ])
    }
}

/// Review payload targeting one packet path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeReviewPayload {
    /// Target packet path reviewed by this payload.
    pub target: String,
    /// Review text.
    pub body: String,
}

impl BridgeReviewPayload {
    /// Builds a review payload.
    pub fn new(target: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            body: body.into(),
        }
    }

    /// Decodes a review payload expression.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/Review")?;
        reject_unknown(fields, &["target", "body"])?;
        Ok(Self::new(
            required_string(fields, "target")?,
            required_string(fields, "body")?,
        ))
    }

    /// Encodes this payload as an expression map.
    pub fn to_expr(&self) -> Expr {
        Expr::Map(vec![
            entry("target", Expr::String(self.target.clone())),
            entry("body", Expr::String(self.body.clone())),
        ])
    }
}

/// Vote payload with one or more structured score axes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeVotePayload {
    /// Target packet path or contribution id being voted on.
    pub target: String,
    /// Structured score axes.
    pub scores: Vec<BridgeScore>,
}

impl BridgeVotePayload {
    /// Builds a vote payload.
    pub fn new(target: impl Into<String>, scores: Vec<BridgeScore>) -> Self {
        Self {
            target: target.into(),
            scores,
        }
    }

    /// Decodes a vote payload expression.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/Vote")?;
        reject_unknown(fields, &["target", "scores"])?;
        let scores = required_vector(fields, "scores")?
            .iter()
            .map(BridgeScore::from_expr)
            .collect::<Result<Vec<_>>>()?;
        if scores.is_empty() {
            return Err(Error::Eval(
                "BRIDGE vote payload must carry at least one score".to_owned(),
            ));
        }
        Ok(Self::new(required_string(fields, "target")?, scores))
    }

    /// Encodes this payload as an expression map.
    pub fn to_expr(&self) -> Expr {
        Expr::Map(vec![
            entry("target", Expr::String(self.target.clone())),
            entry(
                "scores",
                Expr::Vector(self.scores.iter().map(BridgeScore::to_expr).collect()),
            ),
        ])
    }
}

/// Patch payload targeting an exact parent packet and path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgePatchPayload {
    /// Parent packet content id being patched.
    pub parent_cid: String,
    /// Target packet path.
    pub target: String,
    /// Replacement expression for the target path.
    pub replacement: Expr,
}

impl BridgePatchPayload {
    /// Builds a patch payload.
    pub fn new(
        parent_cid: impl Into<String>,
        target: impl Into<String>,
        replacement: Expr,
    ) -> Self {
        Self {
            parent_cid: parent_cid.into(),
            target: target.into(),
            replacement,
        }
    }

    /// Decodes a patch payload expression.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/Patch")?;
        reject_unknown(fields, &["parent-cid", "target", "replacement"])?;
        Ok(Self::new(
            required_string(fields, "parent-cid")?,
            required_string(fields, "target")?,
            required_field(fields, "replacement")?.clone(),
        ))
    }

    /// Encodes this payload as an expression map.
    pub fn to_expr(&self) -> Expr {
        Expr::Map(vec![
            entry("parent-cid", Expr::String(self.parent_cid.clone())),
            entry("target", Expr::String(self.target.clone())),
            entry("replacement", self.replacement.clone()),
        ])
    }
}

/// Evidence payload attached to a collaboration packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeEvidencePayload {
    /// Evidence source id or URI.
    pub source: String,
    /// Evidence body.
    pub body: String,
}

impl BridgeEvidencePayload {
    /// Builds an evidence payload.
    pub fn new(source: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            body: body.into(),
        }
    }

    /// Decodes an evidence payload expression.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/Evidence")?;
        reject_unknown(fields, &["source", "body"])?;
        Ok(Self::new(
            required_string(fields, "source")?,
            required_string(fields, "body")?,
        ))
    }

    /// Encodes this payload as an expression map.
    pub fn to_expr(&self) -> Expr {
        Expr::Map(vec![
            entry("source", Expr::String(self.source.clone())),
            entry("body", Expr::String(self.body.clone())),
        ])
    }
}

/// Receipt payload for an accepted or rejected collaboration step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeReceiptPayload {
    /// Receipt status.
    pub status: Symbol,
    /// Packet paths or content ids covered by the receipt.
    pub refs: Vec<String>,
}

impl BridgeReceiptPayload {
    /// Builds a receipt payload.
    pub fn new(status: Symbol, refs: Vec<String>) -> Self {
        Self { status, refs }
    }

    /// Decodes a receipt payload expression.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/Receipt")?;
        reject_unknown(fields, &["status", "refs"])?;
        Ok(Self::new(
            required_symbol(fields, "status")?.clone(),
            string_vector(fields, "refs")?,
        ))
    }

    /// Encodes this payload as an expression map.
    pub fn to_expr(&self) -> Expr {
        Expr::Map(vec![
            entry("status", Expr::Symbol(self.status.clone())),
            entry(
                "refs",
                Expr::Vector(self.refs.iter().cloned().map(Expr::String).collect()),
            ),
        ])
    }
}

/// Attestation payload citing evidence for a claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeAttestPayload {
    /// Subject packet path or content id.
    pub subject: String,
    /// Claim being attested.
    pub claim: String,
    /// Evidence ids supporting the claim.
    pub evidence: Vec<String>,
}

impl BridgeAttestPayload {
    /// Builds an attestation payload.
    pub fn new(
        subject: impl Into<String>,
        claim: impl Into<String>,
        evidence: Vec<String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            claim: claim.into(),
            evidence,
        }
    }

    /// Decodes an attestation payload expression.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/Attest")?;
        reject_unknown(fields, &["subject", "claim", "evidence"])?;
        Ok(Self::new(
            required_string(fields, "subject")?,
            required_string(fields, "claim")?,
            string_vector(fields, "evidence")?,
        ))
    }

    /// Encodes this payload as an expression map.
    pub fn to_expr(&self) -> Expr {
        Expr::Map(vec![
            entry("subject", Expr::String(self.subject.clone())),
            entry("claim", Expr::String(self.claim.clone())),
            entry(
                "evidence",
                Expr::Vector(self.evidence.iter().cloned().map(Expr::String).collect()),
            ),
        ])
    }
}

/// Validates a collaboration part payload for its registered part kind.
pub fn validate_collab_payload(kind: &Symbol, payload: &Expr) -> Result<()> {
    if kind == &part("Review") {
        BridgeReviewPayload::from_expr(payload).map(drop)
    } else if kind == &part("Vote") {
        BridgeVotePayload::from_expr(payload).map(drop)
    } else if kind == &part("Patch") {
        BridgePatchPayload::from_expr(payload).map(drop)
    } else if kind == &part("Evidence") {
        BridgeEvidencePayload::from_expr(payload).map(drop)
    } else if kind == &part("Receipt") {
        BridgeReceiptPayload::from_expr(payload).map(drop)
    } else if kind == &part("Attest") {
        BridgeAttestPayload::from_expr(payload).map(drop)
    } else {
        Ok(())
    }
}

fn map_fields<'a>(expr: &'a Expr, label: &str) -> Result<&'a [(Expr, Expr)]> {
    match expr {
        Expr::Map(fields) => Ok(fields),
        _ => Err(Error::Eval(format!("{label} payload must be a map"))),
    }
}

fn reject_unknown(fields: &[(Expr, Expr)], allowed: &[&str]) -> Result<()> {
    for (key, _) in fields {
        let Some(name) = field_name(key) else {
            return Err(Error::Eval(
                "BRIDGE collaboration field keys must be symbols".to_owned(),
            ));
        };
        if !allowed.contains(&name.as_str()) {
            return Err(Error::Eval(format!(
                "unknown BRIDGE collaboration field {name}"
            )));
        }
    }
    Ok(())
}

fn required_field<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a Expr> {
    fields
        .iter()
        .find_map(|(key, value)| (field_name(key).as_deref() == Some(name)).then_some(value))
        .ok_or_else(|| Error::Eval(format!("BRIDGE collaboration record is missing {name}")))
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

fn required_i64(fields: &[(Expr, Expr)], name: &str) -> Result<i64> {
    match required_field(fields, name)? {
        Expr::Number(number) => number.canonical.parse().map_err(|_| {
            Error::Eval(format!(
                "BRIDGE collaboration field {name} must be an i64 literal"
            ))
        }),
        _ => Err(Error::TypeMismatch {
            expected: "number",
            found: "non-number",
        }),
    }
}

fn required_vector<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a [Expr]> {
    match required_field(fields, name)? {
        Expr::Vector(items) | Expr::List(items) => Ok(items),
        _ => Err(Error::Eval(format!(
            "BRIDGE collaboration field {name} must be a vector"
        ))),
    }
}

fn string_vector(fields: &[(Expr, Expr)], name: &str) -> Result<Vec<String>> {
    required_vector(fields, name)?
        .iter()
        .map(|item| match item {
            Expr::String(value) => Ok(value.clone()),
            _ => Err(Error::TypeMismatch {
                expected: "string",
                found: "non-string",
            }),
        })
        .collect()
}

fn i64_expr(value: i64) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: value.to_string(),
    })
}

fn field_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Symbol(symbol) => Some(symbol.name.to_string()),
        Expr::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn part(name: &str) -> Symbol {
    Symbol::qualified("bridge", name)
}
