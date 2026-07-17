//! Runtime wiring for the Algol codec: `AlgolCodec` implements the decoder and
//! encoder over a Pratt parser, and `AlgolCodecLib` is the `Lib` that builds the
//! manifest and registers the codec with the linker.

use crate::encode::{decode_escape, encode_algol, encode_algol_tree};
use crate::parse::{decode_algol_located_with_budget, raw_number_tag};
use crate::pratt::{PrattParser, default_pratt_table};
use sim_codec::{
    CodecDefaultDecode, CodecRuntime, DecodeBudget, Decoder, Encoder, Input, LocatedDecoder,
    Output, ReadCx, TreeDecoder, TreeEncoder, codec_value,
};
use sim_kernel::{
    AbiVersion, DefaultFactory, Dependency, Error, Export, Expr, Lib, LibManifest, LibTarget,
    Linker, LocatedExprTree, Result, SourceId, Symbol, Version, WriteCx, pratt_table_value,
};
use std::sync::Arc;

/// Runtime decoder and encoder for the Algol surface, wrapping a [`PrattParser`]
/// over [`crate::default_pratt_table`].
///
/// Implements the `Decoder`, `LocatedDecoder`, `TreeDecoder`, `Encoder`, and
/// `TreeEncoder` traits so the codec participates in every decode/encode lane:
/// it parses infix text into checked `Expr` forms and renders any `Expr` back to
/// infix text.
pub struct AlgolCodec {
    parser: PrattParser,
}

impl Default for AlgolCodec {
    fn default() -> Self {
        Self {
            parser: PrattParser::new(default_pratt_table()),
        }
    }
}

impl Decoder for AlgolCodec {
    fn decode(&self, cx: &mut ReadCx<'_>, input: Input) -> Result<Expr> {
        let source = input.into_string_for(cx.codec)?;
        let mut budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        let mut tree =
            self.parser
                .parse_text_tree_with_budget(cx.codec, "<algol>", &source, &mut budget)?;
        rewrite_number_domains_tree(cx, &mut tree)?;
        decode_escape(cx, tree.expr)
    }
}

impl LocatedDecoder for AlgolCodec {
    fn decode_located(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<sim_kernel::LocatedExpr> {
        let source = input.into_string_for(cx.codec)?;
        let mut budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        cx.cx
            .sources_mut()
            .intern_text(SourceId(source_id.clone()), &source);
        let mut located =
            decode_algol_located_with_budget(cx.codec, source_id, &source, &mut budget)?;
        located.expr = rewrite_number_domains_expr(cx, located.expr)?;
        Ok(located)
    }
}

impl TreeDecoder for AlgolCodec {
    fn decode_tree(
        &self,
        cx: &mut ReadCx<'_>,
        input: Input,
        source_id: String,
    ) -> Result<LocatedExprTree> {
        let source = input.into_string_for(cx.codec)?;
        let mut budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        cx.cx
            .sources_mut()
            .intern_text(SourceId(source_id.clone()), &source);
        let mut tree =
            self.parser
                .parse_text_tree_with_budget(cx.codec, source_id, &source, &mut budget)?;
        rewrite_number_domains_tree(cx, &mut tree)?;
        Ok(tree)
    }
}

fn rewrite_number_domains_expr(cx: &mut ReadCx<'_>, expr: Expr) -> Result<Expr> {
    Ok(match expr {
        Expr::Extension { tag, payload } if tag == raw_number_tag() => {
            let Expr::String(raw) = *payload else {
                return Err(Error::CodecError {
                    codec: cx.codec,
                    message: "algol number literal payload must be a string".to_owned(),
                });
            };
            match cx.cx.parse_number_literal(&raw)? {
                Some(number) => Expr::Number(number),
                None => {
                    return Err(Error::CodecError {
                        codec: cx.codec,
                        message: format!("no number domain accepted literal {raw}"),
                    });
                }
            }
        }
        Expr::List(items) => Expr::List(
            items
                .into_iter()
                .map(|item| rewrite_number_domains_expr(cx, item))
                .collect::<Result<Vec<_>>>()?,
        ),
        Expr::Vector(items) => Expr::Vector(
            items
                .into_iter()
                .map(|item| rewrite_number_domains_expr(cx, item))
                .collect::<Result<Vec<_>>>()?,
        ),
        Expr::Map(entries) => Expr::Map(
            entries
                .into_iter()
                .map(|(key, value)| {
                    Ok((
                        rewrite_number_domains_expr(cx, key)?,
                        rewrite_number_domains_expr(cx, value)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        Expr::Set(items) => Expr::Set(
            items
                .into_iter()
                .map(|item| rewrite_number_domains_expr(cx, item))
                .collect::<Result<Vec<_>>>()?,
        ),
        Expr::Call { operator, args } => Expr::Call {
            operator: Box::new(rewrite_number_domains_expr(cx, *operator)?),
            args: args
                .into_iter()
                .map(|item| rewrite_number_domains_expr(cx, item))
                .collect::<Result<Vec<_>>>()?,
        },
        Expr::Infix {
            operator,
            left,
            right,
        } => Expr::Infix {
            operator,
            left: Box::new(rewrite_number_domains_expr(cx, *left)?),
            right: Box::new(rewrite_number_domains_expr(cx, *right)?),
        },
        Expr::Prefix { operator, arg } => Expr::Prefix {
            operator,
            arg: Box::new(rewrite_number_domains_expr(cx, *arg)?),
        },
        Expr::Postfix { operator, arg } => Expr::Postfix {
            operator,
            arg: Box::new(rewrite_number_domains_expr(cx, *arg)?),
        },
        Expr::Block(items) => Expr::Block(
            items
                .into_iter()
                .map(|item| rewrite_number_domains_expr(cx, item))
                .collect::<Result<Vec<_>>>()?,
        ),
        Expr::Quote { mode, expr } => Expr::Quote { mode, expr },
        Expr::Annotated { expr, annotations } => Expr::Annotated {
            expr: Box::new(rewrite_number_domains_expr(cx, *expr)?),
            annotations: annotations
                .into_iter()
                .map(|(name, value)| Ok((name, rewrite_number_domains_expr(cx, value)?)))
                .collect::<Result<Vec<_>>>()?,
        },
        Expr::Extension { tag, payload } => Expr::Extension {
            tag,
            payload: Box::new(rewrite_number_domains_expr(cx, *payload)?),
        },
        other => other,
    })
}

fn rewrite_number_domains_tree(cx: &mut ReadCx<'_>, tree: &mut LocatedExprTree) -> Result<()> {
    tree.expr = rewrite_number_domains_expr(cx, tree.expr.clone())?;
    if matches!(tree.expr, Expr::Quote { .. }) {
        return Ok(());
    }
    for child in &mut tree.children {
        rewrite_number_domains_tree(cx, child)?;
    }
    Ok(())
}

impl Encoder for AlgolCodec {
    fn encode(&self, cx: &mut WriteCx<'_>, expr: &Expr) -> Result<Output> {
        Ok(Output::Text(encode_algol(
            expr,
            &self.parser.operators,
            0,
            cx,
        )?))
    }
}

impl TreeEncoder for AlgolCodec {
    fn encode_tree(&self, cx: &mut WriteCx<'_>, expr: &LocatedExprTree) -> Result<Output> {
        Ok(Output::Text(encode_algol_tree(
            expr,
            &self.parser.operators,
            0,
            cx,
        )?))
    }
}

/// The [`Lib`] that registers the Algol codec (`codec:algol`) with the runtime.
///
/// Its manifest depends on `codec:lisp` (used by the `expr.lisp(...)` escape for
/// forms outside the infix grammar) and exports the codec value plus the
/// `pratt:arithmetic` operator table value.
pub struct AlgolCodecLib {
    symbol: Symbol,
    codec_id: sim_kernel::CodecId,
}

impl AlgolCodecLib {
    /// Creates the lib bound to the given runtime-assigned codec id.
    pub fn new(id: sim_kernel::CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "algol"),
            codec_id: id,
        }
    }
}

impl Lib for AlgolCodecLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: self.symbol.clone(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: vec![Dependency {
                id: Symbol::qualified("codec", "lisp"),
                minimum_version: None,
            }],
            capabilities: Vec::new(),
            exports: vec![
                Export::Codec {
                    symbol: self.symbol.clone(),
                    codec_id: Some(self.codec_id),
                },
                Export::Value {
                    symbol: Symbol::qualified("pratt", "arithmetic"),
                },
            ],
        }
    }

    fn load(&self, _cx: &mut sim_kernel::LoadCx, linker: &mut Linker) -> Result<()> {
        let _factory = DefaultFactory;
        let expr_shape =
            sim_codec::resolve_expr_shape(linker, &Symbol::qualified("codec", "AlgolSurface"))?;
        let options_shape = sim_codec::resolve_options_shape(linker)?;

        linker.codec_value(
            self.symbol.clone(),
            codec_value(CodecRuntime {
                id: self.codec_id,
                symbol: self.symbol.clone(),
                decoder: Some(Arc::new(AlgolCodec::default())),
                located_decoder: Some(Arc::new(AlgolCodec::default())),
                tree_decoder: Some(Arc::new(AlgolCodec::default())),
                encoder: Some(Arc::new(AlgolCodec::default())),
                located_encoder: None,
                tree_encoder: Some(Arc::new(AlgolCodec::default())),
                expr_shape,
                options_shape,
                default_decode: CodecDefaultDecode::TermInEvalDatumOtherwise,
            }),
        )?;
        linker.value(
            Symbol::qualified("pratt", "arithmetic"),
            pratt_table_value(
                Symbol::qualified("pratt", "arithmetic"),
                default_pratt_table(),
            ),
        )?;
        Ok(())
    }
}
