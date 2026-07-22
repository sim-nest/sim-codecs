use sim_kernel::{Error, Expr, Result};
use sim_shape::{GrammarPosition, GrammarRenderer, parse_shape_expr, shape_grammar_graph};

use sim_codec_json::JsonGrammarRenderer;

pub(crate) const OUTPUT_GRAMMAR_EXTRA: &str = "output-grammar";
pub(crate) const OUTPUT_GRAMMAR_DIALECT_EXTRA: &str = "output-grammar-dialect";
const OUTPUT_GRAMMAR_REQUIRED_EXTRA: &str = "output-grammar-required";
const RETURN_CODEC_EXTRA: &str = "return-codec";
const RETURN_SHAPE_EXTRA: &str = "return-shape";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputGrammarDialect {
    JsonSchema,
    Gbnf,
    SExpr,
}

pub(crate) struct OutputGrammar<'a> {
    pub(crate) dialect: OutputGrammarDialect,
    pub(crate) text: &'a str,
}

pub(crate) fn output_grammar(entries: &[(Expr, Expr)]) -> Result<Option<OutputGrammar<'_>>> {
    let Some(value) = field(entries, OUTPUT_GRAMMAR_EXTRA) else {
        return Ok(None);
    };
    let Expr::String(text) = value else {
        return Err(Error::Eval(
            "model request output-grammar must be a string".to_owned(),
        ));
    };
    let dialect =
        selected_output_grammar_dialect(entries)?.unwrap_or(OutputGrammarDialect::JsonSchema);
    Ok(Some(OutputGrammar { dialect, text }))
}

pub(crate) fn output_grammar_text(
    entries: &[(Expr, Expr)],
    dialect: OutputGrammarDialect,
) -> Result<Option<String>> {
    if let Some(grammar) = output_grammar(entries)? {
        if grammar.dialect != dialect {
            return Err(Error::Eval(format!(
                "provider cannot use {:?} output grammar",
                grammar.dialect
            )));
        }
        return Ok(Some(grammar.text.to_owned()));
    }
    if let Some(selected) = selected_output_grammar_dialect(entries)?
        && selected != dialect
    {
        return Err(Error::Eval(format!(
            "provider cannot use {selected:?} output grammar"
        )));
    }
    let Some(shape_expr) = field(entries, RETURN_SHAPE_EXTRA) else {
        return Ok(None);
    };
    if return_codec(entries).as_ref() != Some(&sim_kernel::Symbol::qualified("codec", "json")) {
        return Ok(None);
    }
    let shape = parse_shape_expr(shape_expr)
        .map_err(|err| Error::Eval(format!("output grammar shape does not parse: {err}")))?;
    let graph = shape_grammar_graph(shape.as_ref())
        .map_err(|err| Error::Eval(format!("output grammar shape does not lower: {err}")))?;
    let renderer = match dialect {
        OutputGrammarDialect::JsonSchema => JsonGrammarRenderer::json_schema(),
        OutputGrammarDialect::Gbnf => JsonGrammarRenderer::gbnf(),
        OutputGrammarDialect::SExpr => {
            return Err(Error::Eval(
                "codec/json does not support sexpr output grammar".to_owned(),
            ));
        }
    };
    renderer.render(&graph, GrammarPosition::Data).map(Some)
}

pub(crate) fn output_grammar_required(entries: &[(Expr, Expr)]) -> Result<bool> {
    match field(entries, OUTPUT_GRAMMAR_REQUIRED_EXTRA) {
        Some(Expr::Bool(flag)) => Ok(*flag),
        Some(other) => Err(Error::Eval(format!(
            "output-grammar-required must be a boolean, found {other:?}"
        ))),
        None => Ok(true),
    }
}

pub(crate) fn reject_output_grammar(entries: &[(Expr, Expr)], provider: &str) -> Result<()> {
    if field(entries, OUTPUT_GRAMMAR_EXTRA).is_some()
        || field(entries, OUTPUT_GRAMMAR_DIALECT_EXTRA).is_some()
    {
        return Err(Error::Eval(format!(
            "{provider} codec does not support output grammar"
        )));
    }
    Ok(())
}

fn selected_output_grammar_dialect(
    entries: &[(Expr, Expr)],
) -> Result<Option<OutputGrammarDialect>> {
    match field(entries, OUTPUT_GRAMMAR_DIALECT_EXTRA) {
        Some(Expr::Symbol(symbol)) => match symbol.name.as_ref() {
            "json-schema" if symbol.namespace.is_none() => {
                Ok(Some(OutputGrammarDialect::JsonSchema))
            }
            "gbnf" if symbol.namespace.is_none() => Ok(Some(OutputGrammarDialect::Gbnf)),
            "sexpr" if symbol.namespace.is_none() => Ok(Some(OutputGrammarDialect::SExpr)),
            other => Err(Error::Eval(format!(
                "unsupported output grammar dialect {other}"
            ))),
        },
        Some(other) => Err(Error::Eval(format!(
            "output-grammar-dialect must be a symbol, found {other:?}"
        ))),
        None => Ok(None),
    }
}

fn return_codec(entries: &[(Expr, Expr)]) -> Option<sim_kernel::Symbol> {
    match field(entries, RETURN_CODEC_EXTRA) {
        Some(Expr::Symbol(symbol)) => Some(symbol.clone()),
        _ => None,
    }
}

fn field<'a>(entries: &'a [(Expr, Expr)], name: &str) -> Option<&'a Expr> {
    entries.iter().find_map(|(key, value)| {
        matches!(key, Expr::Symbol(symbol) if symbol.namespace.is_none() && symbol.name.as_ref() == name)
            .then_some(value)
    })
}
