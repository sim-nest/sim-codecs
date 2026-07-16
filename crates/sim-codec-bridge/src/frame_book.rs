use std::collections::{BTreeMap, BTreeSet};

use sim_kernel::{Error, Expr, Result, Symbol};
use sim_value::build::entry;

/// Illocutionary kind for a BRIDGE frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    /// A frame that tells the receiver which context to use.
    Use,
    /// A frame that supplies information.
    Inform,
    /// A frame that assigns work.
    Task,
    /// A frame that declares a hard requirement.
    Require,
    /// A frame that declares forbidden behavior.
    Forbid,
    /// A frame that declares a preference.
    Prefer,
    /// A frame that describes return obligations.
    Return,
    /// A frame that asks for a check.
    Check,
}

impl FrameKind {
    /// Deterministic sentence prefix for this frame kind.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Use => "Use ",
            Self::Inform => "Note ",
            Self::Task => "You MUST ",
            Self::Require => "You MUST ",
            Self::Forbid => "You must NEVER ",
            Self::Prefer => "Prefer to ",
            Self::Return => "Return ",
            Self::Check => "Check ",
        }
    }

    /// Grammar priority used when frames become constrained-generation choices.
    pub fn grammar_priority(self) -> u8 {
        match self {
            Self::Require | Self::Forbid => 0,
            Self::Task => 1,
            Self::Return | Self::Check => 2,
            Self::Use | Self::Prefer => 3,
            Self::Inform => 4,
        }
    }
}

/// Hole kind for a typed BRIDGE frame slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameHoleKind {
    /// A reference to another packet part or external object.
    Ref,
    /// A symbolic term.
    Term,
    /// A closed choice token.
    Choice,
    /// A structured path.
    Path,
    /// A numeric value.
    Number,
    /// Prose data that must be rendered through a fence.
    Prose,
}

impl FrameHoleKind {
    /// Shape descriptor for values accepted by this hole kind.
    pub fn shape_expr(self) -> Expr {
        let shape = match self {
            Self::Ref | Self::Term | Self::Choice | Self::Prose => {
                Symbol::qualified("core", "String")
            }
            Self::Path => Symbol::qualified("core", "List"),
            Self::Number => Symbol::qualified("core", "Number"),
        };
        Expr::Map(vec![
            entry("hole-kind", Expr::Symbol(self.symbol())),
            entry("shape", Expr::Symbol(shape)),
        ])
    }

    fn symbol(self) -> Symbol {
        match self {
            Self::Ref => Symbol::qualified("bridge", "Ref"),
            Self::Term => Symbol::qualified("bridge", "Term"),
            Self::Choice => Symbol::qualified("bridge", "Choice"),
            Self::Path => Symbol::qualified("bridge", "Path"),
            Self::Number => Symbol::qualified("bridge", "Number"),
            Self::Prose => Symbol::qualified("bridge", "Prose"),
        }
    }
}

/// One named typed hole in a frame template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameHoleSpec {
    /// Hole name used in the payload and template.
    pub name: Symbol,
    /// Hole kind.
    pub kind: FrameHoleKind,
}

impl FrameHoleSpec {
    /// Builds a hole spec.
    pub fn new(name: Symbol, kind: FrameHoleKind) -> Self {
        Self { name, kind }
    }
}

/// Registered frame specification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameSpec {
    /// Frame id carried in `bridge/Frame` payloads.
    pub id: Symbol,
    /// Illocutionary frame kind.
    pub kind: FrameKind,
    /// Deterministic sentence prefix.
    pub prefix: String,
    /// Deterministic template body using `{hole}` placeholders.
    pub template: String,
    /// Typed holes accepted by this frame.
    pub holes: Vec<FrameHoleSpec>,
    /// Priority for grammar menus.
    pub grammar_priority: u8,
}

impl FrameSpec {
    /// Builds a frame spec, deriving prefix and priority from `kind`.
    pub fn new(
        id: Symbol,
        kind: FrameKind,
        template: impl Into<String>,
        holes: Vec<FrameHoleSpec>,
    ) -> Self {
        Self {
            id,
            kind,
            prefix: kind.prefix().to_owned(),
            template: template.into(),
            holes,
            grammar_priority: kind.grammar_priority(),
        }
    }
}

/// Typed payload for a `bridge/Frame` part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeFramePayload {
    /// Registered frame id.
    pub frame: Symbol,
    /// Slot values keyed by hole name.
    pub slots: BTreeMap<Symbol, Expr>,
}

impl BridgeFramePayload {
    /// Builds a payload for `frame` with no slots.
    pub fn new(frame: Symbol) -> Self {
        Self {
            frame,
            slots: BTreeMap::new(),
        }
    }

    /// Adds one slot value.
    pub fn with_slot(mut self, name: Symbol, value: Expr) -> Self {
        self.slots.insert(name, value);
        self
    }

    /// Decodes a payload expression.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/Frame payload")?;
        reject_unknown(fields, &["frame", "slots"])?;
        Ok(Self {
            frame: required_symbol(fields, "frame")?,
            slots: optional_slots(fields, "slots")?,
        })
    }

    /// Encodes this payload as a canonical expression map.
    pub fn to_expr(&self) -> Expr {
        let mut fields = vec![entry("frame", Expr::Symbol(self.frame.clone()))];
        if !self.slots.is_empty() {
            fields.push(entry(
                "slots",
                Expr::Map(
                    self.slots
                        .iter()
                        .map(|(name, value)| (Expr::Symbol(name.clone()), value.clone()))
                        .collect(),
                ),
            ));
        }
        Expr::Map(fields)
    }
}

/// Registry of BRIDGE frame specifications.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BridgeFrameBook {
    specs: BTreeMap<Symbol, FrameSpec>,
}

impl BridgeFrameBook {
    /// Builds an empty frame book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a frame spec.
    pub fn register(&mut self, spec: FrameSpec) {
        self.specs.insert(spec.id.clone(), spec);
    }

    /// Returns the registered spec for `id`.
    pub fn spec(&self, id: &Symbol) -> Option<&FrameSpec> {
        self.specs.get(id)
    }

    /// Requires a registered frame spec.
    pub fn require_spec(&self, id: &Symbol) -> Result<&FrameSpec> {
        self.spec(id)
            .ok_or_else(|| Error::Eval(format!("unknown BRIDGE frame {id}")))
    }

    /// Returns all registered specs.
    pub fn specs(&self) -> impl Iterator<Item = &FrameSpec> {
        self.specs.values()
    }

    /// Parses and validates a frame payload.
    pub fn validate_payload(&self, payload: &Expr) -> Result<BridgeFramePayload> {
        let payload = BridgeFramePayload::from_expr(payload)?;
        let spec = self.require_spec(&payload.frame)?;
        validate_frame_payload(spec, &payload)?;
        Ok(payload)
    }
}

/// Builds the standard BRIDGE frame book.
pub fn standard_frame_book() -> BridgeFrameBook {
    let mut book = BridgeFrameBook::new();
    for spec in [
        frame(
            "use",
            FrameKind::Use,
            "the referenced context {resource}.",
            &[("resource", FrameHoleKind::Ref)],
        ),
        frame(
            "inform",
            FrameKind::Inform,
            "{fact}.",
            &[("fact", FrameHoleKind::Term)],
        ),
        frame(
            "proposal",
            FrameKind::Task,
            "produce the requested proposal.",
            &[],
        ),
        frame(
            "answer",
            FrameKind::Inform,
            "answer the parent packet.",
            &[],
        ),
        frame(
            "produce-artifact",
            FrameKind::Task,
            "produce {what} for {target}.",
            &[
                ("what", FrameHoleKind::Term),
                ("target", FrameHoleKind::Term),
            ],
        ),
        frame(
            "require",
            FrameKind::Require,
            "{rule}.",
            &[("rule", FrameHoleKind::Term)],
        ),
        frame(
            "forbid",
            FrameKind::Forbid,
            "{rule}.",
            &[("rule", FrameHoleKind::Term)],
        ),
        frame(
            "prefer",
            FrameKind::Prefer,
            "{choice}.",
            &[("choice", FrameHoleKind::Choice)],
        ),
        frame(
            "return",
            FrameKind::Return,
            "{shape}.",
            &[("shape", FrameHoleKind::Term)],
        ),
        frame(
            "check",
            FrameKind::Check,
            "{path}.",
            &[("path", FrameHoleKind::Path)],
        ),
        frame(
            "explain",
            FrameKind::Inform,
            "{text}.",
            &[("text", FrameHoleKind::Prose)],
        ),
    ] {
        book.register(spec);
    }
    book
}

pub(crate) fn validate_frame_payload(spec: &FrameSpec, payload: &BridgeFramePayload) -> Result<()> {
    let expected = spec
        .holes
        .iter()
        .map(|hole| hole.name.clone())
        .collect::<BTreeSet<_>>();
    for name in payload.slots.keys() {
        if !expected.contains(name) {
            return Err(Error::Eval(format!(
                "BRIDGE frame {} has unknown hole {}",
                spec.id, name
            )));
        }
    }
    for hole in &spec.holes {
        let value = payload.slots.get(&hole.name).ok_or_else(|| {
            Error::Eval(format!(
                "BRIDGE frame {} missing hole {}",
                spec.id, hole.name
            ))
        })?;
        validate_hole(hole, value)?;
    }
    Ok(())
}

fn validate_hole(hole: &FrameHoleSpec, value: &Expr) -> Result<()> {
    match hole.kind {
        FrameHoleKind::Ref | FrameHoleKind::Term | FrameHoleKind::Choice => match value {
            Expr::Symbol(_) => Ok(()),
            Expr::String(text) if is_token(text) => Ok(()),
            Expr::String(_) => Err(Error::Eval(format!(
                "BRIDGE prose is only allowed in Prose hole {}",
                hole.name
            ))),
            _ => Err(Error::Eval(format!(
                "BRIDGE hole {} expects a symbol or token",
                hole.name
            ))),
        },
        FrameHoleKind::Path => match value {
            Expr::Vector(items) | Expr::List(items) if !items.is_empty() => {
                for item in items {
                    match item {
                        Expr::Symbol(_) => {}
                        Expr::String(text) if is_token(text) => {}
                        _ => {
                            return Err(Error::Eval(format!(
                                "BRIDGE path hole {} expects token path items",
                                hole.name
                            )));
                        }
                    }
                }
                Ok(())
            }
            _ => Err(Error::Eval(format!(
                "BRIDGE hole {} expects a non-empty path",
                hole.name
            ))),
        },
        FrameHoleKind::Number => match value {
            Expr::Number(_) => Ok(()),
            _ => Err(Error::Eval(format!(
                "BRIDGE hole {} expects a number",
                hole.name
            ))),
        },
        FrameHoleKind::Prose => match value {
            Expr::String(_) => Ok(()),
            _ => Err(Error::Eval(format!(
                "BRIDGE hole {} expects prose text",
                hole.name
            ))),
        },
    }
}

fn frame(
    name: &str,
    kind: FrameKind,
    template: &'static str,
    holes: &[(&str, FrameHoleKind)],
) -> FrameSpec {
    FrameSpec::new(
        Symbol::qualified("bridge", name),
        kind,
        template,
        holes
            .iter()
            .map(|(name, kind)| FrameHoleSpec::new(Symbol::new(*name), *kind))
            .collect(),
    )
}

fn map_fields<'a>(expr: &'a Expr, label: &str) -> Result<&'a [(Expr, Expr)]> {
    match expr {
        Expr::Map(fields) => Ok(fields),
        _ => Err(Error::Eval(format!("{label} must be a map"))),
    }
}

fn reject_unknown(fields: &[(Expr, Expr)], allowed: &[&str]) -> Result<()> {
    for (key, _) in fields {
        let Some(symbol) = key_symbol(key) else {
            return Err(Error::Eval(
                "BRIDGE frame field keys must be symbols".to_owned(),
            ));
        };
        if !allowed.contains(&symbol.name.as_ref()) {
            return Err(Error::Eval(format!("unknown BRIDGE frame field {symbol}")));
        }
    }
    Ok(())
}

fn required_symbol(fields: &[(Expr, Expr)], name: &str) -> Result<Symbol> {
    match field_value(fields, name) {
        Some(Expr::Symbol(symbol)) => Ok(symbol.clone()),
        Some(_) => Err(Error::Eval(format!(
            "BRIDGE frame field {name} must be a symbol"
        ))),
        None => Err(Error::Eval(format!("BRIDGE frame payload missing {name}"))),
    }
}

fn optional_slots(fields: &[(Expr, Expr)], name: &str) -> Result<BTreeMap<Symbol, Expr>> {
    let Some(value) = field_value(fields, name) else {
        return Ok(BTreeMap::new());
    };
    let Expr::Map(entries) = value else {
        return Err(Error::Eval("BRIDGE frame slots must be a map".to_owned()));
    };
    let mut slots = BTreeMap::new();
    for (key, value) in entries {
        let Some(symbol) = key_symbol(key) else {
            return Err(Error::Eval(
                "BRIDGE frame slot keys must be symbols".to_owned(),
            ));
        };
        slots.insert(symbol.clone(), value.clone());
    }
    Ok(slots)
}

fn field_value<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Option<&'a Expr> {
    fields.iter().find_map(|(key, value)| {
        let symbol = key_symbol(key)?;
        (symbol.name.as_ref() == name).then_some(value)
    })
}

fn key_symbol(key: &Expr) -> Option<&Symbol> {
    match key {
        Expr::Symbol(symbol) => Some(symbol),
        _ => None,
    }
}

fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_ascii() && !ch.is_whitespace() && !matches!(ch, '{' | '}'))
}
