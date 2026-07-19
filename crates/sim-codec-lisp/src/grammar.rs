//! Lisp codec grammar rendering over the neutral Shape grammar graph.

use sim_codec::encode_string_literal;
use sim_kernel::{Error, Expr, Result, Symbol};
use sim_shape::{
    GrammarDialect, GrammarGraph, GrammarPosition, GrammarRenderer, Production, TerminalAtom,
};

/// Renders neutral Shape grammars for `codec:lisp`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LispGrammarRenderer {
    dialect: GrammarDialect,
}

impl LispGrammarRenderer {
    /// Builds a renderer for `dialect`.
    pub fn new(dialect: GrammarDialect) -> Self {
        Self { dialect }
    }

    /// Builds an S-expression grammar renderer.
    pub fn sexpr() -> Self {
        Self::new(GrammarDialect::SExpr)
    }

    /// Builds a Lisp-shaped GBNF renderer.
    pub fn gbnf() -> Self {
        Self::new(GrammarDialect::Gbnf)
    }
}

impl GrammarRenderer for LispGrammarRenderer {
    fn codec_symbol(&self) -> Symbol {
        Symbol::qualified("codec", "lisp")
    }

    fn dialect(&self) -> GrammarDialect {
        self.dialect
    }

    fn render(&self, graph: &GrammarGraph, position: GrammarPosition) -> Result<String> {
        match self.dialect {
            GrammarDialect::SExpr => render_lisp_sexpr_graph(graph, position),
            GrammarDialect::Gbnf => render_lisp_gbnf_graph(graph, position),
            unsupported => Err(grammar_error(format!(
                "codec/lisp does not support {unsupported:?} grammar dialect"
            ))),
        }
    }
}

fn render_lisp_sexpr_graph(graph: &GrammarGraph, position: GrammarPosition) -> Result<String> {
    let mut forms = vec![
        format!("(codec {})", Symbol::qualified("codec", "lisp")),
        format!("(position {})", position_name(position)),
        format!("(decode-target {})", lisp_decode_target(position)),
        format!("(root {})", render_lisp_sexpr(&graph.root)?),
    ];
    for (name, production) in &graph.defs {
        forms.push(format!("(def {} {})", name, render_lisp_sexpr(production)?));
    }
    Ok(format!("(grammar {})", forms.join(" ")))
}

fn render_lisp_sexpr(production: &Production) -> Result<String> {
    match production {
        Production::Terminal(atom) => render_lisp_terminal(atom),
        Production::Seq(items) => render_wrapped("seq", items.iter().map(render_lisp_sexpr)),
        Production::Alt(choices) => render_wrapped("alt", choices.iter().map(render_lisp_sexpr)),
        Production::Repeat { inner, at_least } => Ok(format!(
            "(repeat {} {})",
            at_least,
            render_lisp_sexpr(inner)?
        )),
        Production::Call { head, args } => {
            let mut rendered = Vec::with_capacity(args.len() + 1);
            rendered.push(render_lisp_sexpr(head)?);
            for arg in args {
                rendered.push(render_lisp_sexpr(arg)?);
            }
            Ok(format!("({})", rendered.join(" ")))
        }
        Production::Ref(name) => Ok(format!("(ref {})", name)),
    }
}

fn render_lisp_terminal(atom: &TerminalAtom) -> Result<String> {
    Ok(match atom {
        TerminalAtom::Any => "_".to_owned(),
        TerminalAtom::Nil => "nil".to_owned(),
        TerminalAtom::Bool => "Bool".to_owned(),
        TerminalAtom::Number => "Number".to_owned(),
        TerminalAtom::String => "String".to_owned(),
        TerminalAtom::List => "List".to_owned(),
        TerminalAtom::Map => "Map".to_owned(),
        TerminalAtom::Symbol => "Symbol".to_owned(),
        TerminalAtom::Exact(expr) => render_exact_lisp(expr)?,
    })
}

fn render_lisp_gbnf_graph(graph: &GrammarGraph, position: GrammarPosition) -> Result<String> {
    let mut lines = vec![
        format!(
            "# codec/lisp position={} target={}",
            position_name(position),
            lisp_decode_target(position)
        ),
        format!("root ::= {}", render_lisp_gbnf(&graph.root)?),
    ];
    for (name, production) in &graph.defs {
        lines.push(format!(
            "{} ::= {}",
            rule_name(name),
            render_lisp_gbnf(production)?
        ));
    }
    Ok(lines.join("\n"))
}

fn render_lisp_gbnf(production: &Production) -> Result<String> {
    match production {
        Production::Terminal(atom) => render_lisp_gbnf_terminal(atom),
        Production::Seq(items) => {
            let rendered = items
                .iter()
                .map(render_lisp_gbnf)
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("({})", rendered.join(" ")))
        }
        Production::Alt(choices) => {
            let rendered = choices
                .iter()
                .map(render_lisp_gbnf)
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("({})", rendered.join(" | ")))
        }
        Production::Repeat { inner, .. } => Ok(format!("({})*", render_lisp_gbnf(inner)?)),
        Production::Call { head, args } => {
            let mut rendered = Vec::with_capacity(args.len() + 1);
            rendered.push(render_lisp_gbnf(head)?);
            for arg in args {
                rendered.push(render_lisp_gbnf(arg)?);
            }
            Ok(format!("\"(\" {} \")\"", rendered.join(" ")))
        }
        Production::Ref(name) => Ok(rule_name(name)),
    }
}

fn render_lisp_gbnf_terminal(atom: &TerminalAtom) -> Result<String> {
    Ok(match atom {
        TerminalAtom::Any => "sexpr".to_owned(),
        TerminalAtom::Nil => "\"nil\"".to_owned(),
        TerminalAtom::Bool => "(\"true\" | \"false\")".to_owned(),
        TerminalAtom::Number => "number".to_owned(),
        TerminalAtom::String => "string".to_owned(),
        TerminalAtom::List => "list".to_owned(),
        TerminalAtom::Map => "map".to_owned(),
        TerminalAtom::Symbol => "symbol".to_owned(),
        TerminalAtom::Exact(expr) => gbnf_literal(&render_exact_lisp(expr)?),
    })
}

fn render_wrapped(head: &str, values: impl Iterator<Item = Result<String>>) -> Result<String> {
    let rendered = values.collect::<Result<Vec<_>>>()?;
    Ok(format!("({} {})", head, rendered.join(" ")))
}

fn render_exact_lisp(expr: &Expr) -> Result<String> {
    Ok(match expr {
        Expr::Nil => "nil".to_owned(),
        Expr::Bool(true) => "true".to_owned(),
        Expr::Bool(false) => "false".to_owned(),
        Expr::Number(number) => number.canonical.clone(),
        Expr::String(text) => encode_string_literal(text),
        Expr::Symbol(symbol) => symbol.to_string(),
        Expr::List(items) | Expr::Vector(items) => {
            let items = items
                .iter()
                .map(render_exact_lisp)
                .collect::<Result<Vec<_>>>()?;
            format!("({})", items.join(" "))
        }
        Expr::Map(entries) => {
            let entries = entries
                .iter()
                .map(|(key, value)| {
                    Ok(format!(
                        "({} {})",
                        render_exact_lisp(key)?,
                        render_exact_lisp(value)?
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            format!("(map {})", entries.join(" "))
        }
        _ => {
            return Err(grammar_error(
                "exact Lisp grammar terminals support data-like expressions",
            ));
        }
    })
}

fn gbnf_literal(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
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

fn lisp_decode_target(position: GrammarPosition) -> &'static str {
    match position {
        GrammarPosition::Eval => "term",
        GrammarPosition::Quote
        | GrammarPosition::Data
        | GrammarPosition::Pattern
        | GrammarPosition::Surface => "datum",
    }
}

fn grammar_error(message: impl Into<String>) -> Error {
    Error::Eval(format!("codec/lisp grammar renderer: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_kernel::Symbol;
    use sim_shape::{
        ExprKind, ExprKindShape, FieldShape, FieldSpec, GrammarDialect, GrammarPosition,
        GrammarTarget, OneOfShape, Shape, ShapeDefRef, ShapeDefs, shape_grammar,
    };

    use super::LispGrammarRenderer;

    #[test]
    fn lisp_sexpr_renders_calls_and_refs() {
        let grammar = shape_grammar(
            recursive_node_shape().as_ref(),
            GrammarTarget {
                codec: Symbol::qualified("codec", "lisp"),
                dialect: GrammarDialect::SExpr,
                position: GrammarPosition::Eval,
            },
            &LispGrammarRenderer::sexpr(),
        )
        .unwrap();

        assert!(grammar.text.contains("(decode-target term)"));
        assert!(grammar.text.contains("(shape/fields"));
        assert!(grammar.text.contains("(ref Node)"));
        assert!(grammar.text.contains("name"));
        assert!(grammar.text.contains("next"));
    }

    #[test]
    fn lisp_gbnf_uses_named_rules_for_refs() {
        let grammar = shape_grammar(
            recursive_node_shape().as_ref(),
            GrammarTarget {
                codec: Symbol::qualified("codec", "lisp"),
                dialect: GrammarDialect::Gbnf,
                position: GrammarPosition::Quote,
            },
            &LispGrammarRenderer::gbnf(),
        )
        .unwrap();

        assert!(grammar.text.contains("target=datum"));
        assert!(grammar.text.contains("Node ::="));
        assert!(grammar.text.contains("Node"));
    }

    #[test]
    fn lisp_renderer_rejects_unsupported_dialect() {
        let err = shape_grammar(
            recursive_node_shape().as_ref(),
            GrammarTarget {
                codec: Symbol::qualified("codec", "lisp"),
                dialect: GrammarDialect::JsonSchema,
                position: GrammarPosition::Data,
            },
            &LispGrammarRenderer::new(GrammarDialect::JsonSchema),
        )
        .unwrap_err();

        assert!(err.to_string().contains("does not support JsonSchema"));
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
