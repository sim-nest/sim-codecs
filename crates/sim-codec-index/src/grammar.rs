//! Structural grammar check for the index expression form.

use std::collections::BTreeSet;

use sim_kernel::Expr;

use crate::CodecError;

/// Shape-like checker for the expression grammar accepted by `codec/index`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IndexExprShape;

/// Returns the structural checker for `codec/index` expressions.
pub fn index_shape() -> IndexExprShape {
    IndexExprShape
}

impl IndexExprShape {
    /// Checks that `expr` has the map/list/string/bool shape of an index
    /// document. Id validity and graph semantics are checked later by
    /// `sim-index-core`.
    pub fn check(&self, expr: &Expr) -> Result<(), CodecError> {
        let root = map(expr, "index")?;
        fields(
            root,
            &[
                Field::string("schema"),
                Field::string("generated-by"),
                Field::string("visibility"),
                Field::list("subjects", subject),
                Field::list("anchors", anchor),
                Field::optional_list("source-units", source_unit),
                Field::optional_list("declarations", declaration),
                Field::optional_list("protocol-relations", protocol_relation),
                Field::list("surfaces", surface),
                Field::list("specimens", specimen),
                Field::list("drafts", draft),
                Field::list("features", feature),
                Field::list("routes", route),
                Field::list("edges", edge),
            ],
            "index",
        )
    }
}

fn source_unit(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "source unit")?,
        &[
            Field::string("subject"),
            Field::string("path"),
            Field::string("reachability"),
            Field::string("completeness"),
            Field::string("reason"),
            Field::object("retained-bound", syntax_bound),
            Field::number("declaration-count"),
        ],
        "source unit",
    )
}

type CheckFn = fn(&Expr) -> Result<(), CodecError>;

#[derive(Clone, Copy)]
struct Field {
    name: &'static str,
    kind: FieldKind,
}

#[derive(Clone, Copy)]
enum FieldKind {
    String,
    Number,
    Bool,
    Object(CheckFn),
    OptionalString,
    OptionalList(CheckFn),
    List(CheckFn),
}

impl Field {
    const fn string(name: &'static str) -> Self {
        Self {
            name,
            kind: FieldKind::String,
        }
    }

    const fn bool(name: &'static str) -> Self {
        Self {
            name,
            kind: FieldKind::Bool,
        }
    }

    const fn number(name: &'static str) -> Self {
        Self {
            name,
            kind: FieldKind::Number,
        }
    }

    const fn object(name: &'static str, check: CheckFn) -> Self {
        Self {
            name,
            kind: FieldKind::Object(check),
        }
    }

    const fn optional_string(name: &'static str) -> Self {
        Self {
            name,
            kind: FieldKind::OptionalString,
        }
    }

    const fn optional_list(name: &'static str, item: CheckFn) -> Self {
        Self {
            name,
            kind: FieldKind::OptionalList(item),
        }
    }

    const fn list(name: &'static str, item: CheckFn) -> Self {
        Self {
            name,
            kind: FieldKind::List(item),
        }
    }
}

fn declaration(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "declaration")?,
        &[
            Field::string("anchor"),
            Field::string("role"),
            Field::string("module-path"),
            Field::string("generics"),
            Field::list("members", string_item),
            Field::object("location", source_location),
            Field::object("syntax-bound", syntax_bound),
        ],
        "declaration",
    )
}

fn source_location(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "source location")?,
        &[Field::string("file"), Field::number("declaration")],
        "source location",
    )
}

fn syntax_bound(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "syntax bound")?,
        &[Field::number("max-bytes"), Field::bool("truncated")],
        "syntax bound",
    )
}

fn protocol_relation(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "protocol relation")?,
        &[
            Field::string("anchor"),
            Field::string("implementor"),
            Field::string("source-spelling"),
            Field::string("body-fingerprint"),
            Field::object("body-bound", syntax_bound),
            Field::object("resolution", protocol_resolution),
        ],
        "protocol relation",
    )
}

fn protocol_resolution(expr: &Expr) -> Result<(), CodecError> {
    let entries = map(expr, "protocol resolution")?;
    unique_symbol_fields(entries, "protocol resolution")?;
    let state = entries
        .iter()
        .find_map(|(key, value)| (symbol_name(key) == Some("state")).then_some(value));
    match state {
        Some(Expr::String(state)) if state == "resolved" => fields(
            entries,
            &[Field::string("state"), Field::string("protocol")],
            "protocol resolution",
        ),
        Some(Expr::String(state)) if state == "unresolved" => fields(
            entries,
            &[
                Field::string("state"),
                Field::string("reason"),
                Field::list("candidates", string_item),
            ],
            "protocol resolution",
        ),
        Some(Expr::String(state)) => Err(CodecError::Shape(format!(
            "unsupported protocol resolution state {state:?}"
        ))),
        Some(other) => wrong("protocol resolution", "state", "string", other),
        None => Err(CodecError::Shape(
            "protocol resolution missing field state".to_owned(),
        )),
    }
}

fn subject(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "subject")?,
        &[
            Field::string("id"),
            Field::string("kind"),
            Field::string("title"),
        ],
        "subject",
    )
}

fn anchor(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "anchor")?,
        &[
            Field::string("id"),
            Field::string("subject"),
            Field::string("kind"),
        ],
        "anchor",
    )
}

fn surface(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "surface")?,
        &[
            Field::string("id"),
            Field::string("subject"),
            Field::string("kind"),
        ],
        "surface",
    )
}

fn specimen(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "specimen")?,
        &[
            Field::string("id"),
            Field::string("subject"),
            Field::string("kind"),
            Field::string("path"),
            Field::optional_string("language"),
            Field::bool("runnable"),
            Field::bool("checked"),
            Field::optional_string("checked-by"),
            Field::optional_string("doc-anchor"),
        ],
        "specimen",
    )
}

fn draft(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "draft")?,
        &[
            Field::string("id"),
            Field::string("subject"),
            Field::string("title"),
            Field::string("summary"),
            Field::list("claims-anchors", string_item),
            Field::list("claims-surfaces", string_item),
            Field::list("claims-specimens", string_item),
            Field::list("literal-anchors", string_item),
            Field::list("literal-surfaces", string_item),
            Field::list("literal-specimens", string_item),
            Field::list("grammar-contracts", grammar_contract),
            Field::optional_string("doc-anchor"),
        ],
        "draft",
    )
}

fn feature(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "feature")?,
        &[
            Field::string("id"),
            Field::string("key"),
            Field::string("subject"),
            Field::string("title"),
            Field::string("summary"),
            Field::list("anchors", string_item),
            Field::list("surfaces", string_item),
            Field::list("specimens", string_item),
            Field::list("grammar-contracts", grammar_contract),
            Field::optional_string("doc-anchor"),
        ],
        "feature",
    )
}

fn grammar_contract(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "grammar contract")?,
        &[
            Field::string("id"),
            Field::optional_string("decoder"),
            Field::optional_string("encoder"),
            Field::optional_string("surface"),
            Field::bool("round-trip"),
        ],
        "grammar contract",
    )
}

fn route(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "route")?,
        &[
            Field::string("id"),
            Field::string("title"),
            Field::optional_list("audiences", string_item),
            Field::list("steps", step),
            Field::optional_string("doc-anchor"),
        ],
        "route",
    )
}

fn step(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "route step")?,
        &[
            Field::string("kind"),
            Field::string("id"),
            Field::optional_string("why"),
        ],
        "route step",
    )
}

fn edge(expr: &Expr) -> Result<(), CodecError> {
    fields(
        map(expr, "edge")?,
        &[
            Field::string("from"),
            Field::string("rel"),
            Field::string("to"),
        ],
        "edge",
    )
}

fn fields(entries: &[(Expr, Expr)], spec: &[Field], label: &str) -> Result<(), CodecError> {
    unique_symbol_fields(entries, label)?;
    for (key, _) in entries {
        let name = symbol_name(key).expect("unique_symbol_fields checked symbol keys");
        if !spec.iter().any(|field| field.name == name) {
            return Err(CodecError::Shape(format!(
                "{label} has unknown field {name}"
            )));
        }
    }
    for field in spec {
        let Some(value) = entries
            .iter()
            .find_map(|(key, value)| (symbol_name(key) == Some(field.name)).then_some(value))
        else {
            if field.is_optional() {
                continue;
            }
            return Err(CodecError::Shape(format!(
                "{label} missing field {}",
                field.name
            )));
        };
        check_field_kind(value, *field, label)?;
    }
    Ok(())
}

impl Field {
    fn is_optional(self) -> bool {
        matches!(
            self.kind,
            FieldKind::OptionalString | FieldKind::OptionalList(_)
        )
    }
}

fn check_field_kind(value: &Expr, field: Field, label: &str) -> Result<(), CodecError> {
    match field.kind {
        FieldKind::String => string_item(value),
        FieldKind::Number => match value {
            Expr::Number(_) => Ok(()),
            other => wrong(label, field.name, "number", other),
        },
        FieldKind::Bool => match value {
            Expr::Bool(_) => Ok(()),
            other => wrong(label, field.name, "bool", other),
        },
        FieldKind::Object(check) => match value {
            Expr::Map(_) => check(value),
            other => wrong(label, field.name, "map", other),
        },
        FieldKind::OptionalString => match value {
            Expr::Nil | Expr::String(_) => Ok(()),
            other => wrong(label, field.name, "nil or string", other),
        },
        FieldKind::OptionalList(item) => match value {
            Expr::List(items) => items.iter().try_for_each(item),
            other => wrong(label, field.name, "list", other),
        },
        FieldKind::List(item) => match value {
            Expr::List(items) => items.iter().try_for_each(item),
            other => wrong(label, field.name, "list", other),
        },
    }
}

fn string_item(expr: &Expr) -> Result<(), CodecError> {
    match expr {
        Expr::String(_) => Ok(()),
        other => Err(CodecError::Shape(format!(
            "expected string, found {other:?}"
        ))),
    }
}

fn map<'a>(expr: &'a Expr, label: &str) -> Result<&'a [(Expr, Expr)], CodecError> {
    match expr {
        Expr::Map(entries) => Ok(entries),
        other => Err(CodecError::Shape(format!(
            "{label} must be a map, found {other:?}"
        ))),
    }
}

fn unique_symbol_fields(entries: &[(Expr, Expr)], label: &str) -> Result<(), CodecError> {
    let mut seen = BTreeSet::new();
    for (key, _) in entries {
        let Some(name) = symbol_name(key) else {
            return Err(CodecError::Shape(format!(
                "{label} field key must be a symbol"
            )));
        };
        if !seen.insert(name) {
            return Err(CodecError::Shape(format!("{label} repeats field {name}")));
        }
    }
    Ok(())
}

fn symbol_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Symbol(symbol) if symbol.namespace.is_none() => Some(symbol.name.as_ref()),
        _ => None,
    }
}

fn wrong(label: &str, field: &str, expected: &str, actual: &Expr) -> Result<(), CodecError> {
    Err(CodecError::Shape(format!(
        "{label}.{field} must be {expected}, found {actual:?}"
    )))
}
