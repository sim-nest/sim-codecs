use std::collections::BTreeMap;

use sim_kernel::{Expr, Symbol};
use sim_value::build::entry;

use crate::BridgePacket;

/// Repetition rule for a profile part kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfilePartCount {
    /// Zero or more parts.
    ZeroOrMore,
    /// One or more parts.
    OneOrMore,
    /// Zero or one part.
    Optional,
    /// Exactly one part.
    Required,
}

/// One part-kind rule in a BRIDGE profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfilePartRule {
    /// Required part kind.
    pub kind: Symbol,
    /// Repetition rule.
    pub count: ProfilePartCount,
}

impl ProfilePartRule {
    /// Builds a profile part rule.
    pub fn new(kind: Symbol, count: ProfilePartCount) -> Self {
        Self { kind, count }
    }
}

/// Registered BRIDGE profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeProfileSpec {
    /// Profile id.
    pub id: Symbol,
    /// Ordered part-kind rules.
    pub parts: Vec<ProfilePartRule>,
}

impl BridgeProfileSpec {
    /// Builds a profile spec.
    pub fn new(id: Symbol, parts: Vec<ProfilePartRule>) -> Self {
        Self { id, parts }
    }

    /// Returns true when `packet` matches this ordered profile.
    pub fn matches_packet(&self, packet: &BridgePacket) -> bool {
        let kinds = packet
            .body
            .iter()
            .map(|part| &part.kind)
            .collect::<Vec<_>>();
        let mut cursor = 0usize;
        for rule in &self.parts {
            let mut count = 0usize;
            while cursor < kinds.len() && kinds[cursor] == &rule.kind {
                count += 1;
                cursor += 1;
            }
            match rule.count {
                ProfilePartCount::ZeroOrMore => {}
                ProfilePartCount::OneOrMore if count == 0 => return false,
                ProfilePartCount::OneOrMore => {}
                ProfilePartCount::Optional if count > 1 => return false,
                ProfilePartCount::Optional => {}
                ProfilePartCount::Required if count != 1 => return false,
                ProfilePartCount::Required => {}
            }
        }
        cursor == kinds.len()
    }

    /// Encodes this profile as a public descriptor expression.
    pub fn to_expr(&self) -> Expr {
        Expr::Map(vec![
            entry("id", Expr::Symbol(self.id.clone())),
            entry(
                "parts",
                Expr::Vector(
                    self.parts
                        .iter()
                        .map(|rule| {
                            Expr::Map(vec![
                                entry("kind", Expr::Symbol(rule.kind.clone())),
                                entry("count", Expr::Symbol(rule.count.symbol())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

/// Registry of BRIDGE profile specifications.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BridgeProfileBook {
    specs: BTreeMap<Symbol, BridgeProfileSpec>,
}

impl BridgeProfileBook {
    /// Builds an empty profile book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a profile spec.
    pub fn register(&mut self, spec: BridgeProfileSpec) {
        self.specs.insert(spec.id.clone(), spec);
    }

    /// Returns the registered profile spec for `id`.
    pub fn spec(&self, id: &Symbol) -> Option<&BridgeProfileSpec> {
        self.specs.get(id)
    }

    /// Returns all registered profile specs.
    pub fn specs(&self) -> impl Iterator<Item = &BridgeProfileSpec> {
        self.specs.values()
    }

    /// Returns the matching profile ids for `packet`.
    pub fn matching_profiles(&self, packet: &BridgePacket) -> Vec<Symbol> {
        self.specs
            .values()
            .filter(|spec| spec.matches_packet(packet))
            .map(|spec| spec.id.clone())
            .collect()
    }
}

impl ProfilePartCount {
    fn symbol(self) -> Symbol {
        match self {
            Self::ZeroOrMore => Symbol::qualified("bridge", "ZeroOrMore"),
            Self::OneOrMore => Symbol::qualified("bridge", "OneOrMore"),
            Self::Optional => Symbol::qualified("bridge", "Optional"),
            Self::Required => Symbol::qualified("bridge", "Required"),
        }
    }
}

/// Profile id for BRIEF packets.
pub fn brief_profile_symbol() -> Symbol {
    Symbol::qualified("bridge", "BRIEF")
}

/// Shape descriptor for the registered BRIDGE profile catalog.
pub fn bridge_profile_shape_expr() -> Expr {
    Expr::Map(vec![
        entry("shape", Expr::Symbol(Symbol::qualified("shape", "OneOf"))),
        entry(
            "choices",
            Expr::Vector(vec![Expr::Symbol(brief_profile_symbol())]),
        ),
    ])
}

/// Builds the BRIEF profile spec: `Given* Frame+ Return?`.
pub fn brief_profile_spec() -> BridgeProfileSpec {
    BridgeProfileSpec::new(
        brief_profile_symbol(),
        vec![
            ProfilePartRule::new(part("Given"), ProfilePartCount::ZeroOrMore),
            ProfilePartRule::new(part("Frame"), ProfilePartCount::OneOrMore),
            ProfilePartRule::new(part("Return"), ProfilePartCount::Optional),
        ],
    )
}

/// Builds the standard BRIDGE profile book.
pub fn standard_profile_book() -> BridgeProfileBook {
    let mut book = BridgeProfileBook::new();
    book.register(brief_profile_spec());
    book
}

fn part(name: &str) -> Symbol {
    Symbol::qualified("bridge", name)
}
