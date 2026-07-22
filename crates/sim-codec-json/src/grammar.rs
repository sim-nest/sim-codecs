//! JSON codec grammar rendering over the neutral Shape grammar graph.

use serde_json::{Map, Value as JsonValue};
use sim_kernel::{Error, Expr, Result, Symbol};
use sim_shape::{
    GrammarDialect, GrammarGraph, GrammarPosition, GrammarRenderer, Production, TerminalAtom,
};

use crate::{expr_to_json, json_escape};

/// Renders neutral Shape grammars for `codec:json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonGrammarRenderer {
    dialect: GrammarDialect,
}

impl JsonGrammarRenderer {
    /// Builds a renderer for `dialect`.
    pub fn new(dialect: GrammarDialect) -> Self {
        Self { dialect }
    }

    /// Builds a JSON Schema renderer.
    pub fn json_schema() -> Self {
        Self::new(GrammarDialect::JsonSchema)
    }

    /// Builds a JSON-shaped GBNF renderer.
    pub fn gbnf() -> Self {
        Self::new(GrammarDialect::Gbnf)
    }
}

impl GrammarRenderer for JsonGrammarRenderer {
    fn codec_symbol(&self) -> Symbol {
        Symbol::qualified("codec", "json")
    }

    fn dialect(&self) -> GrammarDialect {
        self.dialect
    }

    fn render(&self, graph: &GrammarGraph, position: GrammarPosition) -> Result<String> {
        match self.dialect {
            GrammarDialect::JsonSchema => render_json_schema_graph(graph, position),
            GrammarDialect::Gbnf => render_json_gbnf_graph(graph, position),
            unsupported => Err(grammar_error(format!(
                "codec/json does not support {unsupported:?} grammar dialect"
            ))),
        }
    }
}

fn render_json_schema_graph(graph: &GrammarGraph, position: GrammarPosition) -> Result<String> {
    let mut root = match render_json_schema(&graph.root)? {
        JsonValue::Object(object) => object,
        scalar => {
            let mut object = Map::new();
            object.insert("allOf".to_owned(), JsonValue::Array(vec![scalar]));
            object
        }
    };
    root.insert(
        "$comment".to_owned(),
        JsonValue::String(format!(
            "codec/json position={} target={}",
            position_name(position),
            json_decode_target(position)
        )),
    );
    if !graph.defs.is_empty() {
        let mut defs = Map::new();
        for (name, production) in &graph.defs {
            defs.insert(name.to_string(), render_json_schema(production)?);
        }
        root.insert("$defs".to_owned(), JsonValue::Object(defs));
    }
    serde_json::to_string(&JsonValue::Object(root)).map_err(|err| grammar_error(err.to_string()))
}

fn render_json_schema(production: &Production) -> Result<JsonValue> {
    match production {
        Production::Terminal(atom) => render_json_terminal(atom),
        Production::Seq(items) => render_json_seq(items),
        Production::Alt(choices) => {
            if choices.len() == 1
                && matches!(
                    choices.first(),
                    Some(Production::Terminal(TerminalAtom::Any))
                )
            {
                return Ok(JsonValue::Bool(true));
            }
            object_with_array("anyOf", choices.iter().map(render_json_schema))
        }
        Production::Repeat { inner, at_least } => {
            let mut object = typed_schema("array");
            object.insert("items".to_owned(), render_json_schema(inner)?);
            if *at_least > 0 {
                object.insert(
                    "minItems".to_owned(),
                    JsonValue::Number(serde_json::Number::from(*at_least)),
                );
            }
            Ok(JsonValue::Object(object))
        }
        Production::Call { head, args } => render_json_call(head, args),
        Production::Ref(name) => {
            let mut object = Map::new();
            object.insert(
                "$ref".to_owned(),
                JsonValue::String(format!(
                    "#/$defs/{}",
                    json_pointer_escape(&name.to_string())
                )),
            );
            Ok(JsonValue::Object(object))
        }
    }
}

fn render_json_terminal(atom: &TerminalAtom) -> Result<JsonValue> {
    Ok(match atom {
        TerminalAtom::Any => JsonValue::Bool(true),
        TerminalAtom::Nil => JsonValue::Object(typed_schema("null")),
        TerminalAtom::Bool => JsonValue::Object(typed_schema("boolean")),
        TerminalAtom::Number => JsonValue::Object(typed_schema("number")),
        TerminalAtom::String => JsonValue::Object(typed_schema("string")),
        TerminalAtom::List => JsonValue::Object(typed_schema("array")),
        TerminalAtom::Map => JsonValue::Object(typed_schema("object")),
        TerminalAtom::Symbol => {
            let mut object = typed_schema("string");
            object.insert(
                "description".to_owned(),
                JsonValue::String("symbol".to_owned()),
            );
            JsonValue::Object(object)
        }
        TerminalAtom::Exact(expr) => {
            let mut object = Map::new();
            object.insert("const".to_owned(), expr_to_json(expr));
            JsonValue::Object(object)
        }
    })
}

fn render_json_seq(items: &[Production]) -> Result<JsonValue> {
    let (prefix, rest) = match items.split_last() {
        Some((Production::Repeat { inner, at_least: 0 }, prefix)) => (prefix, Some(inner)),
        _ => (items, None),
    };
    let mut object = typed_schema("array");
    object.insert(
        "prefixItems".to_owned(),
        JsonValue::Array(
            prefix
                .iter()
                .map(render_json_schema)
                .collect::<Result<Vec<_>>>()?,
        ),
    );
    object.insert(
        "items".to_owned(),
        match rest {
            Some(rest) => render_json_schema(rest)?,
            None => JsonValue::Bool(false),
        },
    );
    if rest.is_none() {
        object.insert(
            "minItems".to_owned(),
            JsonValue::Number(serde_json::Number::from(prefix.len())),
        );
        object.insert(
            "maxItems".to_owned(),
            JsonValue::Number(serde_json::Number::from(prefix.len())),
        );
    }
    Ok(JsonValue::Object(object))
}

fn render_json_call(head: &Production, args: &[Production]) -> Result<JsonValue> {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        let (name, value) = call_arg_schema(index, arg)?;
        required.push(JsonValue::String(name.clone()));
        properties.insert(name, value);
    }
    let mut object = typed_schema("object");
    object.insert("properties".to_owned(), JsonValue::Object(properties));
    object.insert("required".to_owned(), JsonValue::Array(required));
    object.insert("additionalProperties".to_owned(), JsonValue::Bool(false));
    if let Some(head) = exact_symbol(head) {
        object.insert(
            "$comment".to_owned(),
            JsonValue::String(format!("call {}", head)),
        );
    }
    Ok(JsonValue::Object(object))
}

fn call_arg_schema(index: usize, arg: &Production) -> Result<(String, JsonValue)> {
    if let Production::Seq(parts) = arg
        && let [
            Production::Terminal(TerminalAtom::Exact(Expr::Symbol(name))),
            value,
        ] = parts.as_slice()
    {
        return Ok((name.name.to_string(), render_json_schema(value)?));
    }
    Ok((format!("arg{index}"), render_json_schema(arg)?))
}

fn render_json_gbnf_graph(graph: &GrammarGraph, position: GrammarPosition) -> Result<String> {
    let mut lines = vec![
        format!(
            "# codec/json position={} target={}",
            position_name(position),
            json_decode_target(position)
        ),
        format!("root ::= {}", render_json_gbnf(&graph.root)?),
    ];
    for (name, production) in &graph.defs {
        lines.push(format!(
            "{} ::= {}",
            rule_name(name),
            render_json_gbnf(production)?
        ));
    }
    Ok(lines.join("\n"))
}

fn render_json_gbnf(production: &Production) -> Result<String> {
    match production {
        Production::Terminal(atom) => render_json_gbnf_terminal(atom),
        Production::Seq(items) => {
            let rendered = items
                .iter()
                .map(render_json_gbnf)
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("[ {} ]", rendered.join(" \",\" ")))
        }
        Production::Alt(choices) => {
            let rendered = choices
                .iter()
                .map(render_json_gbnf)
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("({})", rendered.join(" | ")))
        }
        Production::Repeat { inner, .. } => Ok(format!("({})*", render_json_gbnf(inner)?)),
        Production::Call { head: _, args } => {
            let fields = args
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    let (name, value) = call_arg_gbnf(index, arg)?;
                    Ok(format!("{} \":\" {}", gbnf_literal(&name), value))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("{{ {} }}", fields.join(" \",\" ")))
        }
        Production::Ref(name) => Ok(rule_name(name)),
    }
}

fn call_arg_gbnf(index: usize, arg: &Production) -> Result<(String, String)> {
    if let Production::Seq(parts) = arg
        && let [
            Production::Terminal(TerminalAtom::Exact(Expr::Symbol(name))),
            value,
        ] = parts.as_slice()
    {
        return Ok((name.name.to_string(), render_json_gbnf(value)?));
    }
    Ok((format!("arg{index}"), render_json_gbnf(arg)?))
}

fn render_json_gbnf_terminal(atom: &TerminalAtom) -> Result<String> {
    Ok(match atom {
        TerminalAtom::Any => "json-value".to_owned(),
        TerminalAtom::Nil => "\"null\"".to_owned(),
        TerminalAtom::Bool => "(\"true\" | \"false\")".to_owned(),
        TerminalAtom::Number => "json-number".to_owned(),
        TerminalAtom::String | TerminalAtom::Symbol => "json-string".to_owned(),
        TerminalAtom::List => "json-array".to_owned(),
        TerminalAtom::Map => "json-object".to_owned(),
        TerminalAtom::Exact(expr) => {
            let text = serde_json::to_string(&expr_to_json(expr))
                .map_err(|err| grammar_error(err.to_string()))?;
            gbnf_literal(&text)
        }
    })
}

fn exact_symbol(production: &Production) -> Option<&Symbol> {
    let Production::Terminal(TerminalAtom::Exact(Expr::Symbol(symbol))) = production else {
        return None;
    };
    Some(symbol)
}

fn object_with_array(
    key: &str,
    values: impl Iterator<Item = Result<JsonValue>>,
) -> Result<JsonValue> {
    let mut object = Map::new();
    object.insert(
        key.to_owned(),
        JsonValue::Array(values.collect::<Result<Vec<_>>>()?),
    );
    Ok(JsonValue::Object(object))
}

fn typed_schema(kind: &str) -> Map<String, JsonValue> {
    let mut object = Map::new();
    object.insert("type".to_owned(), JsonValue::String(kind.to_owned()));
    object
}

fn json_pointer_escape(text: &str) -> String {
    text.replace('~', "~0").replace('/', "~1")
}

fn gbnf_literal(text: &str) -> String {
    format!("\"{}\"", json_escape(text))
}

fn rule_name(symbol: &Symbol) -> String {
    let mut out = String::new();
    for ch in symbol.to_string().chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    if out
        .chars()
        .next()
        .is_none_or(|ch| !ch.is_ascii_alphabetic())
    {
        out.insert_str(0, "r-");
    }
    out
}

fn position_name(position: GrammarPosition) -> &'static str {
    match position {
        GrammarPosition::Eval => "eval",
        GrammarPosition::Quote => "quote",
        GrammarPosition::Data => "data",
        GrammarPosition::Pattern => "pattern",
        GrammarPosition::Surface => "surface",
    }
}

fn json_decode_target(_position: GrammarPosition) -> &'static str {
    "datum"
}

fn grammar_error(message: impl Into<String>) -> Error {
    Error::Eval(format!("codec/json grammar renderer: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_codec_lisp::LispGrammarRenderer;
    use sim_kernel::Symbol;
    use sim_shape::{
        ExprKind, ExprKindShape, FieldShape, FieldSpec, GrammarDialect, GrammarPosition,
        GrammarTarget, OneOfShape, Shape, ShapeDefRef, ShapeDefs, shape_grammar,
    };

    use super::JsonGrammarRenderer;

    #[test]
    fn json_and_lisp_renderers_share_fields_and_refs_but_not_text() {
        let shape = recursive_node_shape();
        let json = shape_grammar(
            shape.as_ref(),
            GrammarTarget {
                codec: Symbol::qualified("codec", "json"),
                dialect: GrammarDialect::JsonSchema,
                position: GrammarPosition::Data,
            },
            &JsonGrammarRenderer::json_schema(),
        )
        .unwrap();
        let lisp = shape_grammar(
            shape.as_ref(),
            GrammarTarget {
                codec: Symbol::qualified("codec", "lisp"),
                dialect: GrammarDialect::SExpr,
                position: GrammarPosition::Data,
            },
            &LispGrammarRenderer::sexpr(),
        )
        .unwrap();

        assert_ne!(json.text, lisp.text);
        for token in ["name", "next", "Node"] {
            assert!(json.text.contains(token), "missing {token} in JSON grammar");
            assert!(lisp.text.contains(token), "missing {token} in Lisp grammar");
        }
        assert!(json.text.contains(r##""$ref":"#/$defs/Node""##));
        assert!(lisp.text.contains("(ref Node)"));
    }

    #[test]
    fn json_gbnf_uses_named_rules_for_refs() {
        let shape = recursive_node_shape();
        let grammar = shape_grammar(
            shape.as_ref(),
            GrammarTarget {
                codec: Symbol::qualified("codec", "json"),
                dialect: GrammarDialect::Gbnf,
                position: GrammarPosition::Eval,
            },
            &JsonGrammarRenderer::gbnf(),
        )
        .unwrap();

        assert!(grammar.text.contains("target=datum"));
        assert!(grammar.text.contains("Node ::="));
        assert!(grammar.text.contains("Node"));
        assert!(grammar.text.contains("\"name\""));
    }

    #[test]
    fn json_renderer_rejects_unsupported_dialect() {
        let err = shape_grammar(
            recursive_node_shape().as_ref(),
            GrammarTarget {
                codec: Symbol::qualified("codec", "json"),
                dialect: GrammarDialect::SExpr,
                position: GrammarPosition::Data,
            },
            &JsonGrammarRenderer::new(GrammarDialect::SExpr),
        )
        .unwrap_err();

        assert!(err.to_string().contains("does not support SExpr"));
    }

    fn recursive_node_shape() -> Arc<dyn Shape> {
        let node = Symbol::new("Node");
        Arc::new(ShapeDefs::new(
            Arc::new(ShapeDefRef::new(node.clone())),
            vec![(
                node.clone(),
                Arc::new(FieldShape::anonymous(vec![
                    FieldSpec::required(
                        Symbol::new("name"),
                        Arc::new(ExprKindShape::new(ExprKind::String)),
                    ),
                    FieldSpec::required(
                        Symbol::new("next"),
                        Arc::new(OneOfShape::new(vec![
                            Arc::new(ExprKindShape::new(ExprKind::Nil)),
                            Arc::new(ShapeDefRef::new(node)),
                        ])),
                    ),
                ])),
            )],
        ))
    }
}
