//! Eval-facing API that drives codecs through the kernel.
//!
//! Provides the `DecodedForm` result type, codec lookup/value helpers, and the
//! decode/encode entry points that locate a codec by symbol and run it with the
//! configured decode limits and read policy.

use std::sync::Arc;

use sim_kernel::{
    Cx, Datum, DefaultFactory, Factory, LocatedExpr, LocatedExprTree, ReadPolicy, Result, Symbol,
    Term, Value, WriteCx,
};

use crate::{
    CodecDefaultDecode, CodecRuntime, DecodeLimits, DecodePosition, DecodeTarget, Input, Output,
    ReadCx, encode_value_expr,
};

/// The position-resolved result of a default decode: inert data or an evaluable
/// term.
///
/// Returned by [`decode_default_with_codec`]; the codec's
/// [`CodecDefaultDecode`] policy and the requested [`DecodePosition`] together
/// select which variant is produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodedForm {
    /// Decoded as inert data.
    Datum(Datum),
    /// Decoded as an evaluable term.
    Term(Term),
}

/// Wrap a [`CodecRuntime`] as an opaque runtime [`Value`] for registry storage.
pub fn codec_value(codec: CodecRuntime) -> Value {
    DefaultFactory
        .opaque(Arc::new(codec))
        .expect("codec runtime should always be boxable")
}

/// Decode `input` with the codec named `symbol`, using default [`DecodeLimits`].
///
/// Looks the codec up in the kernel registry, runs its decoder under
/// `read_policy`, and returns the raw `Expr`.
pub fn decode_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
) -> Result<sim_kernel::Expr> {
    decode_with_codec_and_limits(cx, symbol, input, read_policy, DecodeLimits::default())
}

/// Decode `input` with the codec named `symbol` under explicit [`DecodeLimits`].
pub fn decode_with_codec_and_limits(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
    limits: DecodeLimits,
) -> Result<sim_kernel::Expr> {
    decode_expr_with_codec_and_limits(cx, symbol, input, read_policy, limits).map(|(expr, _)| expr)
}

/// Decode `input` to a `Datum` with the codec named `symbol`, using default
/// [`DecodeLimits`].
pub fn decode_datum_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
) -> Result<Datum> {
    decode_datum_with_codec_and_limits(cx, symbol, input, read_policy, DecodeLimits::default())
}

/// Decode `input` to a `Datum` under explicit [`DecodeLimits`], failing if the
/// decoded `Expr` is not inert data.
pub fn decode_datum_with_codec_and_limits(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
    limits: DecodeLimits,
) -> Result<Datum> {
    let (expr, _) = decode_expr_with_codec_and_limits(cx, symbol, input, read_policy, limits)?;
    Datum::try_from(expr)
}

/// Decode `input` to an evaluable `Term` with the codec named `symbol`, using
/// default [`DecodeLimits`].
pub fn decode_term_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
) -> Result<Term> {
    decode_term_with_codec_and_limits(cx, symbol, input, read_policy, DecodeLimits::default())
}

/// Decode `input` to a `Term` under explicit [`DecodeLimits`], lowering the
/// decoded `Expr` to a term per the codec's [`CodecDefaultDecode`] policy.
pub fn decode_term_with_codec_and_limits(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
    limits: DecodeLimits,
) -> Result<Term> {
    let (expr, default_decode) =
        decode_expr_with_codec_and_limits(cx, symbol, input, read_policy, limits)?;
    term_from_expr(default_decode, expr)
}

/// Decode `input` and resolve it to data or a term per the codec's policy and
/// the requested `position`, using default [`DecodeLimits`].
///
/// This is the position-aware entry point: the codec's [`CodecDefaultDecode`]
/// and `position` jointly pick the [`DecodedForm`] variant.
pub fn decode_default_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
    position: DecodePosition,
) -> Result<DecodedForm> {
    decode_default_with_codec_and_limits(
        cx,
        symbol,
        input,
        read_policy,
        position,
        DecodeLimits::default(),
    )
}

/// Position-aware decode under explicit [`DecodeLimits`]; see
/// [`decode_default_with_codec`].
pub fn decode_default_with_codec_and_limits(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
    position: DecodePosition,
    limits: DecodeLimits,
) -> Result<DecodedForm> {
    let (expr, default_decode) =
        decode_expr_with_codec_and_limits(cx, symbol, input, read_policy, limits)?;
    match default_decode.target_for(position) {
        DecodeTarget::Datum => Datum::try_from(expr).map(DecodedForm::Datum),
        DecodeTarget::Term => term_from_expr(default_decode, expr).map(DecodedForm::Term),
    }
}

fn decode_expr_with_codec_and_limits(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
    limits: DecodeLimits,
) -> Result<(sim_kernel::Expr, CodecDefaultDecode)> {
    let value = cx.resolve_codec(symbol)?;
    let codec =
        value
            .object()
            .downcast_ref::<CodecRuntime>()
            .ok_or(sim_kernel::Error::TypeMismatch {
                expected: "codec",
                found: "non-codec",
            })?;
    let mut read_cx = ReadCx {
        cx,
        codec: codec.id,
        read_policy,
        limits,
    };
    codec
        .decode(&mut read_cx, input)
        .map(|expr| (expr, codec.default_decode))
}

/// Encode `expr` with the codec named `symbol` under `options`.
///
/// Looks the codec up, builds a `WriteCx` from `options` (which fix the output
/// position and fidelity), and runs the codec's encoder.
pub fn encode_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    expr: &sim_kernel::Expr,
    options: sim_kernel::EncodeOptions,
) -> Result<Output> {
    let value = cx.resolve_codec(symbol)?;
    let codec =
        value
            .object()
            .downcast_ref::<CodecRuntime>()
            .ok_or(sim_kernel::Error::TypeMismatch {
                expected: "codec",
                found: "non-codec",
            })?;
    let mut write_cx = WriteCx {
        cx,
        codec: codec.id,
        options,
    };
    codec.encode(&mut write_cx, expr)
}

/// Encode a `Datum` with the codec named `symbol`, lifting it to an `Expr`
/// first.
pub fn encode_datum_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    datum: &Datum,
    options: sim_kernel::EncodeOptions,
) -> Result<Output> {
    encode_with_codec(cx, symbol, &sim_kernel::Expr::from(datum.clone()), options)
}

/// Encode a `Term` with the codec named `symbol`, lifting it to an `Expr` first.
pub fn encode_term_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    term: &Term,
    options: sim_kernel::EncodeOptions,
) -> Result<Output> {
    encode_with_codec(cx, symbol, &sim_kernel::Expr::from(term.clone()), options)
}

/// Encode a runtime `Value` with the codec named `symbol`, forcing lists and
/// tables into an `Expr` via [`encode_value_expr`] before encoding.
pub fn encode_value_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    value: &Value,
    options: sim_kernel::EncodeOptions,
) -> Result<Output> {
    let codec_value = cx.resolve_codec(symbol)?;
    let codec = codec_value.object().downcast_ref::<CodecRuntime>().ok_or(
        sim_kernel::Error::TypeMismatch {
            expected: "codec",
            found: "non-codec",
        },
    )?;
    let mut write_cx = WriteCx {
        cx,
        codec: codec.id,
        options,
    };
    let expr = encode_value_expr(&mut write_cx, value)?;
    codec.encode(&mut write_cx, &expr)
}

/// Decode `input` preserving source origin, attributing spans to `source_id`,
/// using default [`DecodeLimits`].
pub fn decode_located_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
    source_id: impl Into<String>,
) -> Result<LocatedExpr> {
    decode_located_with_codec_and_limits(
        cx,
        symbol,
        input,
        read_policy,
        source_id,
        DecodeLimits::default(),
    )
}

/// Origin-preserving decode under explicit [`DecodeLimits`]; see
/// [`decode_located_with_codec`].
pub fn decode_located_with_codec_and_limits(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
    source_id: impl Into<String>,
    limits: DecodeLimits,
) -> Result<LocatedExpr> {
    let value = cx.resolve_codec(symbol)?;
    let codec =
        value
            .object()
            .downcast_ref::<CodecRuntime>()
            .ok_or(sim_kernel::Error::TypeMismatch {
                expected: "codec",
                found: "non-codec",
            })?;
    let mut read_cx = ReadCx {
        cx,
        codec: codec.id,
        read_policy,
        limits,
    };
    codec.decode_located(&mut read_cx, input, source_id.into())
}

/// Encode a [`LocatedExpr`] with the codec named `symbol`, using its origin for
/// fidelity when `options` request lossless-origin output.
pub fn encode_located_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    expr: &LocatedExpr,
    options: sim_kernel::EncodeOptions,
) -> Result<Output> {
    let value = cx.resolve_codec(symbol)?;
    let codec =
        value
            .object()
            .downcast_ref::<CodecRuntime>()
            .ok_or(sim_kernel::Error::TypeMismatch {
                expected: "codec",
                found: "non-codec",
            })?;
    let mut write_cx = WriteCx {
        cx,
        codec: codec.id,
        options,
    };
    codec.encode_located(&mut write_cx, expr)
}

/// Encode a [`LocatedExprTree`] with the codec named `symbol`, reproducing
/// layout and trivia when `options` request lossless-origin output.
pub fn encode_tree_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    expr: &LocatedExprTree,
    options: sim_kernel::EncodeOptions,
) -> Result<Output> {
    let value = cx
        .registry()
        .codec_by_symbol(symbol)
        .cloned()
        .ok_or_else(|| sim_kernel::Error::Eval(format!("unknown codec {}", symbol)))?;
    let codec = value
        .object()
        .as_any()
        .downcast_ref::<CodecRuntime>()
        .ok_or_else(|| sim_kernel::Error::Eval(format!("{} is not a codec runtime", symbol)))?;
    let mut write_cx = WriteCx {
        cx,
        codec: codec.id,
        options,
    };
    codec.encode_tree(&mut write_cx, expr)
}

/// Decode `input` into a full [`LocatedExprTree`] (trivia and layout retained),
/// attributing spans to `source_id`, using default [`DecodeLimits`].
pub fn decode_tree_with_codec(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
    source_id: impl Into<String>,
) -> Result<LocatedExprTree> {
    decode_tree_with_codec_and_limits(
        cx,
        symbol,
        input,
        read_policy,
        source_id,
        DecodeLimits::default(),
    )
}

/// Full-tree decode under explicit [`DecodeLimits`]; see
/// [`decode_tree_with_codec`].
pub fn decode_tree_with_codec_and_limits(
    cx: &mut Cx,
    symbol: &Symbol,
    input: Input,
    read_policy: ReadPolicy,
    source_id: impl Into<String>,
    limits: DecodeLimits,
) -> Result<LocatedExprTree> {
    let value = cx.resolve_codec(symbol)?;
    let codec =
        value
            .object()
            .downcast_ref::<CodecRuntime>()
            .ok_or(sim_kernel::Error::TypeMismatch {
                expected: "codec",
                found: "non-codec",
            })?;
    let mut read_cx = ReadCx {
        cx,
        codec: codec.id,
        read_policy,
        limits,
    };
    codec.decode_tree(&mut read_cx, input, source_id.into())
}

fn term_from_expr(default_decode: CodecDefaultDecode, expr: sim_kernel::Expr) -> Result<Term> {
    match default_decode {
        CodecDefaultDecode::Datum => Term::lower(expr),
        CodecDefaultDecode::TermInEvalDatumOtherwise => Term::lower(lower_eval_surface(expr)),
    }
}

fn lower_eval_surface(expr: sim_kernel::Expr) -> sim_kernel::Expr {
    use sim_kernel::Expr;

    match expr {
        Expr::List(items) if items.len() > 1 => {
            let mut items = items
                .into_iter()
                .map(lower_eval_surface)
                .collect::<Vec<_>>();
            let operator = Box::new(items.remove(0));
            Expr::Call {
                operator,
                args: items,
            }
        }
        Expr::List(items) => Expr::List(items.into_iter().map(lower_eval_surface).collect()),
        Expr::Vector(items) => Expr::Vector(items.into_iter().map(lower_eval_surface).collect()),
        Expr::Map(entries) => Expr::Map(
            entries
                .into_iter()
                .map(|(key, value)| (lower_eval_surface(key), lower_eval_surface(value)))
                .collect(),
        ),
        Expr::Set(items) => Expr::Set(items.into_iter().map(lower_eval_surface).collect()),
        Expr::Block(items) => Expr::Block(items.into_iter().map(lower_eval_surface).collect()),
        Expr::Quote { mode, expr } => Expr::Quote { mode, expr },
        Expr::Annotated { expr, annotations } => Expr::Annotated {
            expr: Box::new(lower_eval_surface(*expr)),
            annotations: annotations
                .into_iter()
                .map(|(name, value)| (name, lower_eval_surface(value)))
                .collect(),
        },
        Expr::Extension { tag, payload } => Expr::Extension {
            tag,
            payload: Box::new(lower_eval_surface(*payload)),
        },
        other => other,
    }
}
