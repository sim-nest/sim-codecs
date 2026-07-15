use std::collections::BTreeMap;

use sim_kernel::{Error, Expr, Result, Symbol};

/// Rendering class for a registered BRIDGE part kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderClass {
    /// Structural line in the packet face.
    Structural,
    /// Fluent frame sentence rendered from a typed frame.
    Frame,
    /// Non-instruction data rendered through a fence.
    Data,
    /// Evidence or attestation material.
    Evidence,
    /// Review text or structured review material.
    Review,
    /// Vote material.
    Vote,
    /// Patch material.
    Patch,
    /// Fetch request material.
    Fetch,
    /// Return contract material.
    Return,
    /// Receipt material.
    Receipt,
    /// Extension material.
    Extension,
}

/// Authority class for a registered BRIDGE part kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthorityClass {
    /// Data only; it carries no instruction authority.
    Data,
    /// Normative instruction or obligation.
    Normative,
    /// Callable tool or model request material.
    Callable,
    /// Evidence, receipt, or review material.
    Evidence,
}

/// Policy for preserving a part kind that is not in the standard normative book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnknownPolicy {
    /// Reject the part unless the kind is registered.
    Reject,
    /// Preserve the part as data only.
    PreserveDataOnly,
}

/// Registered BRIDGE part-kind specification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgePartSpec {
    /// Part kind symbol.
    pub kind: Symbol,
    /// Shape expression for the part payload.
    pub shape_expr: Expr,
    /// Rendering class.
    pub render_class: RenderClass,
    /// Authority class.
    pub authority_class: AuthorityClass,
    /// Unknown preservation policy.
    pub unknown_policy: UnknownPolicy,
}

impl BridgePartSpec {
    /// Builds a normative registered part spec.
    pub fn new(
        kind: Symbol,
        shape_expr: Expr,
        render_class: RenderClass,
        authority_class: AuthorityClass,
        unknown_policy: UnknownPolicy,
    ) -> Self {
        Self {
            kind,
            shape_expr,
            render_class,
            authority_class,
            unknown_policy,
        }
    }

    /// Builds a data-only preserving extension spec.
    pub fn preserve_data_only(kind: Symbol, shape_expr: Expr) -> Self {
        Self::new(
            kind,
            shape_expr,
            RenderClass::Extension,
            AuthorityClass::Data,
            UnknownPolicy::PreserveDataOnly,
        )
    }
}

/// Registry of BRIDGE part-kind specifications.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BridgePartBook {
    specs: BTreeMap<Symbol, BridgePartSpec>,
}

impl BridgePartBook {
    /// Builds an empty part book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a part spec, replacing any existing spec for the same kind.
    pub fn register(&mut self, spec: BridgePartSpec) {
        self.specs.insert(spec.kind.clone(), spec);
    }

    /// Returns the registered spec for `kind`.
    pub fn spec(&self, kind: &Symbol) -> Option<&BridgePartSpec> {
        self.specs.get(kind)
    }

    /// Checks that `kind` is registered for decoding.
    pub fn require_registered(&self, kind: &Symbol) -> Result<&BridgePartSpec> {
        self.spec(kind)
            .ok_or_else(|| Error::Eval(format!("unknown BRIDGE part kind {kind}")))
    }
}

/// A BRIDGE book bundles the part book and move book used for a packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeBook {
    /// Registered part-kind specs.
    pub parts: BridgePartBook,
    /// Registered dialogue move specs.
    pub moves: crate::BridgeMoveBook,
}

impl BridgeBook {
    /// Builds a book from explicit part and move books.
    pub fn new(parts: BridgePartBook, moves: crate::BridgeMoveBook) -> Self {
        Self { parts, moves }
    }

    /// Builds the standard BRIDGE book.
    pub fn standard() -> Self {
        Self::new(crate::standard_part_book(), crate::standard_move_book())
    }

    /// Returns a copy with one more part spec registered.
    pub fn with_part(mut self, spec: BridgePartSpec) -> Self {
        self.parts.register(spec);
        self
    }
}

/// Builds the standard BRIDGE part book.
pub fn standard_part_book() -> BridgePartBook {
    let mut book = BridgePartBook::new();
    for spec in [
        spec("Given", RenderClass::Data, AuthorityClass::Data),
        spec("Frame", RenderClass::Frame, AuthorityClass::Normative),
        spec("Call", RenderClass::Structural, AuthorityClass::Callable),
        spec("Weave", RenderClass::Structural, AuthorityClass::Normative),
        spec("Check", RenderClass::Structural, AuthorityClass::Normative),
        spec("Evidence", RenderClass::Evidence, AuthorityClass::Evidence),
        spec("Review", RenderClass::Review, AuthorityClass::Evidence),
        spec("Vote", RenderClass::Vote, AuthorityClass::Evidence),
        spec("Patch", RenderClass::Patch, AuthorityClass::Normative),
        spec("Fetch", RenderClass::Fetch, AuthorityClass::Callable),
        spec("Return", RenderClass::Return, AuthorityClass::Normative),
        spec("Receipt", RenderClass::Receipt, AuthorityClass::Evidence),
        spec("Attest", RenderClass::Evidence, AuthorityClass::Evidence),
        spec("Extension", RenderClass::Extension, AuthorityClass::Data),
    ] {
        book.register(spec);
    }
    book
}

fn spec(name: &str, render_class: RenderClass, authority_class: AuthorityClass) -> BridgePartSpec {
    let kind = Symbol::qualified("bridge", name);
    BridgePartSpec::new(
        kind.clone(),
        Expr::Symbol(kind),
        render_class,
        authority_class,
        UnknownPolicy::Reject,
    )
}
