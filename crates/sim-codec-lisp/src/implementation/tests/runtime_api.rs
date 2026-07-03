use super::*;

use sim_codec::{
    DecodePosition, DecodedForm, decode_datum_with_codec, decode_default_with_codec,
    decode_term_with_codec, encode_datum_with_codec,
};
use sim_kernel::{Datum, Ref, Term};

fn symbol() -> Symbol {
    Symbol::qualified("codec", "lisp")
}

fn sample_datum() -> Datum {
    Datum::Node {
        tag: Symbol::qualified("demo", "wire"),
        fields: vec![
            (Symbol::new("name"), Datum::String("sample".to_owned())),
            (
                Symbol::new("payload"),
                Datum::List(vec![
                    Datum::Symbol(Symbol::qualified("math", "pi")),
                    Datum::Bytes(vec![1, 2, 3]),
                ]),
            ),
        ],
    }
}

#[test]
fn lisp_datum_roundtrip_preserves_content_id() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let datum = sample_datum();
    let content_id = datum.content_id().unwrap();

    let output = encode_datum_with_codec(&mut cx, &symbol(), &datum, Default::default())
        .unwrap()
        .into_text()
        .unwrap();
    let decoded = decode_datum_with_codec(
        &mut cx,
        &symbol(),
        Input::Text(output),
        ReadPolicy::default(),
    )
    .unwrap();

    assert_eq!(decoded, datum);
    assert_eq!(decoded.content_id().unwrap(), content_id);
}

#[test]
fn lisp_default_decode_uses_term_in_eval_and_datum_in_data() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);

    let eval = decode_default_with_codec(
        &mut cx,
        &symbol(),
        Input::Text("(math/add 1 2)".to_owned()),
        ReadPolicy::default(),
        DecodePosition::Eval,
    )
    .unwrap();
    assert!(matches!(eval, DecodedForm::Term(Term::Call { .. })));

    let data = decode_default_with_codec(
        &mut cx,
        &symbol(),
        Input::Text("(math/add 1 2)".to_owned()),
        ReadPolicy::default(),
        DecodePosition::Data,
    )
    .unwrap();
    assert!(matches!(data, DecodedForm::Datum(Datum::List(_))));

    let quote = decode_default_with_codec(
        &mut cx,
        &symbol(),
        Input::Text("(math/add 1 2)".to_owned()),
        ReadPolicy::default(),
        DecodePosition::Quote,
    )
    .unwrap();
    assert!(matches!(quote, DecodedForm::Datum(Datum::List(_))));
}

#[test]
fn lisp_term_decode_lowers_list_call_surface() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);

    let term = decode_term_with_codec(
        &mut cx,
        &symbol(),
        Input::Text("(math/add 1 2)".to_owned()),
        ReadPolicy::default(),
    )
    .unwrap();

    let Term::Call { target, args } = term else {
        panic!("expected call term");
    };
    assert_eq!(
        *target,
        Term::Ref(Ref::Symbol(Symbol::qualified("math", "add")))
    );
    assert_eq!(args.len(), 2);
}
