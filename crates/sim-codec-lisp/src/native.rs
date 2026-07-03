#![allow(missing_docs)]

use std::sync::Arc;

use crate::LispCodecLib;

#[sim::sim_lib(id = "codec/lisp", version = "0.1.0", native_export = true)]
mod lisp_native {
    use super::*;
    #[allow(unused_imports)]
    use sim::{
        codec::{Input, decode_with_codec, encode_with_codec},
        kernel::{
            Cx, DefaultFactory, EncodeOptions, Expr, NoopEvalPolicy, ReadPolicy, Result, Symbol,
        },
        sim_codec,
    };

    #[sim_codec(
        symbol = "codec/lisp",
        decode = "decode_lisp_native",
        encode = "encode_lisp_native"
    )]
    pub fn lisp_codec() {}

    pub fn decode_lisp_native(text: String) -> Result<Expr> {
        with_lisp_context(|cx, codec| {
            decode_with_codec(cx, codec, Input::Text(text), ReadPolicy::default())
        })
    }

    pub fn encode_lisp_native(expr: Expr) -> Result<String> {
        with_lisp_context(|cx, codec| {
            encode_with_codec(cx, codec, &expr, EncodeOptions::default())?.into_text()
        })
    }

    fn with_lisp_context<T>(f: impl FnOnce(&mut Cx, &Symbol) -> Result<T>) -> Result<T> {
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let codec = Symbol::qualified("codec", "lisp");
        let lib = LispCodecLib::new(cx.registry_mut().fresh_codec_id())?;
        cx.load_lib(&lib)?;
        f(&mut cx, &codec)
    }
}
