//! Text-form decode and encode helpers for `codec/index`.

use std::sync::Arc;

use sim_codec::{DecodeBudget, DecodeLimits, Decoder, Encoder, Input, Output, ReadCx};
use sim_codec_json::{expr_to_json, json_to_expr};
use sim_codec_lisp::{LispProcMacroDecoder, LispProcMacroEncoder};
use sim_index_core::{IndexDoc, check_index_doc};
use sim_kernel::{
    CodecId, Cx, DefaultFactory, EncodeOptions, EncodePosition, Expr, NoopEvalPolicy, ReadPolicy,
    WriteCx,
};

use crate::{CodecError, expr_from_index_doc, index_doc_from_expr, index_shape};

/// Text forms emitted by `codec/index`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexForm {
    /// Canonical s-expression form, rendered through `codec:lisp`.
    Sx,
    /// Tagged JSON expression form, rendered through `codec:json`.
    Json,
}

/// Decodes one index text form into the shared expression grammar.
pub fn decode_index_expr(form: IndexForm, source: &str) -> Result<Expr, CodecError> {
    match form {
        IndexForm::Sx => decode_sx_expr(source),
        IndexForm::Json => decode_json_expr(source),
    }
}

/// Encodes one index expression into a text form.
pub fn encode_index_expr(
    expr: &Expr,
    position: EncodePosition,
    form: IndexForm,
) -> Result<String, CodecError> {
    match form {
        IndexForm::Sx => encode_sx_expr(expr, position),
        IndexForm::Json => encode_json_expr(expr),
    }
}

pub(crate) fn doc_from_form(form: IndexForm, source: &str) -> Result<IndexDoc, CodecError> {
    let expr = decode_index_expr(form, source)?;
    doc_from_expr(&expr)
}

pub(crate) fn doc_from_expr(expr: &Expr) -> Result<IndexDoc, CodecError> {
    index_shape().check(expr)?;
    let doc = index_doc_from_expr(expr)?;
    check_index_doc(&doc)?;
    Ok(doc)
}

pub(crate) fn encode_doc(
    doc: &IndexDoc,
    position: EncodePosition,
    form: IndexForm,
) -> Result<String, CodecError> {
    check_index_doc(doc)?;
    let expr = expr_from_index_doc(doc);
    encode_index_expr(&expr, position, form)
}

fn decode_sx_expr(source: &str) -> Result<Expr, CodecError> {
    let mut cx = bare_cx();
    let mut read_cx = ReadCx {
        cx: &mut cx,
        codec: CodecId(0),
        read_policy: ReadPolicy::default(),
        limits: DecodeLimits::default(),
    };
    LispProcMacroDecoder
        .decode(&mut read_cx, Input::Text(source.to_owned()))
        .map_err(|err| CodecError::Decode(err.to_string()))
}

fn decode_json_expr(source: &str) -> Result<Expr, CodecError> {
    let value = serde_json::from_str(source).map_err(|err| CodecError::Decode(err.to_string()))?;
    let mut budget = DecodeBudget::new(DecodeLimits::default());
    json_to_expr(CodecId(0), &value, &mut budget, 0)
        .map_err(|err| CodecError::Decode(err.to_string()))
}

fn encode_sx_expr(expr: &Expr, position: EncodePosition) -> Result<String, CodecError> {
    let mut cx = bare_cx();
    let mut write_cx = WriteCx {
        cx: &mut cx,
        codec: CodecId(0),
        options: EncodeOptions {
            position,
            ..EncodeOptions::default()
        },
    };
    match LispProcMacroEncoder
        .encode(&mut write_cx, expr)
        .map_err(|err| CodecError::Encode(err.to_string()))?
    {
        Output::Text(text) => Ok(text),
        Output::Bytes(_) => Err(CodecError::Encode(
            "codec/lisp emitted bytes for index text".to_owned(),
        )),
    }
}

fn encode_json_expr(expr: &Expr) -> Result<String, CodecError> {
    serde_json::to_string(&expr_to_json(expr)).map_err(|err| CodecError::Encode(err.to_string()))
}

fn bare_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}
