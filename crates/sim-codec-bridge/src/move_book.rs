use std::collections::BTreeMap;

use sim_kernel::{Error, Result, Symbol};

/// Parent-intent rule for a BRIDGE move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplyRule {
    /// The move opens a new thread and may not cite parents.
    Opens,
    /// The move may answer any parent intent.
    Any,
    /// The move may answer any parent intent except `receipt`.
    AnyNonReceipt,
    /// The move may answer only the listed parent intents.
    Only(Vec<Symbol>),
}

/// Registered dialogue move specification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeMoveSpec {
    /// Move intent symbol.
    pub intent: Symbol,
    /// Parent intent legality.
    pub replies_to: ReplyRule,
    /// Required part kinds.
    pub requires_parts: Vec<Symbol>,
    /// Alternative part-kind groups; every group must have at least one member present.
    pub requires_any_parts: Vec<Vec<Symbol>>,
    /// Whether this move is terminal.
    pub terminal: bool,
}

/// Registry of legal BRIDGE moves.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BridgeMoveBook {
    moves: BTreeMap<Symbol, BridgeMoveSpec>,
}

impl BridgeMoveBook {
    /// Builds an empty move book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a move spec.
    pub fn register(&mut self, spec: BridgeMoveSpec) {
        self.moves.insert(spec.intent.clone(), spec);
    }

    /// Returns the registered spec for `intent`.
    pub fn spec(&self, intent: &Symbol) -> Option<&BridgeMoveSpec> {
        self.moves.get(intent)
    }

    /// Returns all registered move specs.
    pub fn specs(&self) -> impl Iterator<Item = &BridgeMoveSpec> {
        self.moves.values()
    }

    /// Returns registered intents that may reply to `parents`.
    pub fn legal_reply_intents(&self, parents: &[Symbol]) -> Vec<Symbol> {
        self.moves
            .values()
            .filter(|spec| check_reply_rule(&spec.intent, &spec.replies_to, parents).is_ok())
            .map(|spec| spec.intent.clone())
            .collect()
    }

    /// Checks dialogue legality for a move, its parent intents, and its part kinds.
    pub fn check_move(&self, intent: &Symbol, parents: &[Symbol], parts: &[Symbol]) -> Result<()> {
        let spec = self
            .moves
            .get(intent)
            .ok_or_else(|| Error::Eval(format!("unknown BRIDGE move {intent}")))?;
        check_reply_rule(intent, &spec.replies_to, parents)?;
        for required in &spec.requires_parts {
            if !parts.contains(required) {
                return Err(Error::Eval(format!("{intent} requires a {required} part")));
            }
        }
        for alternatives in &spec.requires_any_parts {
            if alternatives.iter().any(|required| parts.contains(required)) {
                continue;
            }
            let names = alternatives
                .iter()
                .map(Symbol::as_qualified_str)
                .collect::<Vec<_>>()
                .join(" or ");
            return Err(Error::Eval(format!("{intent} requires {names}")));
        }
        Ok(())
    }
}

/// Builds the standard BRIDGE move book.
pub fn standard_move_book() -> BridgeMoveBook {
    let mut book = BridgeMoveBook::new();
    for spec in [
        move_spec_with_any(
            "request",
            ReplyRule::Opens,
            &["Return"],
            &[&["Frame", "Call", "Weave"]],
            false,
        ),
        move_spec(
            "offer",
            ReplyRule::Only(vec![intent("request")]),
            &["Return"],
            false,
        ),
        move_spec(
            "reply",
            ReplyRule::Only(vec![intent("request")]),
            &["Return"],
            false,
        ),
        move_spec(
            "review",
            ReplyRule::Only(vec![intent("offer"), intent("reply"), intent("patch")]),
            &["Review"],
            false,
        ),
        move_spec(
            "vote",
            ReplyRule::Only(vec![intent("offer"), intent("reply")]),
            &["Vote"],
            false,
        ),
        move_spec(
            "patch",
            ReplyRule::Only(vec![intent("receipt"), intent("review"), intent("reply")]),
            &["Patch"],
            false,
        ),
        move_spec("fetch", ReplyRule::AnyNonReceipt, &["Fetch"], false),
        move_spec("receipt", ReplyRule::AnyNonReceipt, &["Receipt"], true),
        move_spec(
            "attest",
            ReplyRule::Only(vec![intent("reply"), intent("receipt")]),
            &["Attest"],
            true,
        ),
        move_spec("error", ReplyRule::Any, &["Receipt"], true),
    ] {
        book.register(spec);
    }
    book
}

fn check_reply_rule(current_intent: &Symbol, rule: &ReplyRule, parents: &[Symbol]) -> Result<()> {
    match rule {
        ReplyRule::Opens => {
            if parents.is_empty() {
                Ok(())
            } else {
                Err(Error::Eval(format!(
                    "{current_intent} opens a thread; it cannot reply"
                )))
            }
        }
        ReplyRule::Any => require_parent(current_intent, parents),
        ReplyRule::AnyNonReceipt => {
            require_parent(current_intent, parents)?;
            for parent in parents {
                if parent == &intent("receipt") {
                    return Err(Error::Eval(format!(
                        "{current_intent} may not answer {parent}"
                    )));
                }
            }
            Ok(())
        }
        ReplyRule::Only(allowed) => {
            require_parent(current_intent, parents)?;
            for parent in parents {
                if !allowed.contains(parent) {
                    return Err(Error::Eval(format!(
                        "{current_intent} may not answer {parent}"
                    )));
                }
            }
            Ok(())
        }
    }
}

fn require_parent(intent: &Symbol, parents: &[Symbol]) -> Result<()> {
    if parents.is_empty() {
        Err(Error::Eval(format!("{intent} requires a parent move")))
    } else {
        Ok(())
    }
}

fn move_spec(
    name: &str,
    replies_to: ReplyRule,
    required_parts: &[&str],
    terminal: bool,
) -> BridgeMoveSpec {
    move_spec_with_any(name, replies_to, required_parts, &[], terminal)
}

fn move_spec_with_any(
    name: &str,
    replies_to: ReplyRule,
    required_parts: &[&str],
    required_any_parts: &[&[&str]],
    terminal: bool,
) -> BridgeMoveSpec {
    BridgeMoveSpec {
        intent: intent(name),
        replies_to,
        requires_parts: required_parts
            .iter()
            .map(|name| Symbol::qualified("bridge", *name))
            .collect(),
        requires_any_parts: required_any_parts
            .iter()
            .map(|group| {
                group
                    .iter()
                    .map(|name| Symbol::qualified("bridge", *name))
                    .collect()
            })
            .collect(),
        terminal,
    }
}

fn intent(name: &str) -> Symbol {
    Symbol::new(name)
}
