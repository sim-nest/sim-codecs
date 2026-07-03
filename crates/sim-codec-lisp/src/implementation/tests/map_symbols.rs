use super::*;

#[test]
fn map_values_escape_operator_only_symbols() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let codec = Symbol::qualified("codec", "lisp");
    let expr = Expr::Map(vec![(
        Expr::Symbol(Symbol::new("symbol")),
        Expr::Symbol(Symbol::new("+")),
    )]);

    let encoded = encode_with_codec(&mut cx, &codec, &expr, Default::default())
        .unwrap()
        .into_text()
        .unwrap();
    assert_eq!(encoded, "(expr:map [symbol (expr:symbol nil \"+\")])");

    let decoded =
        decode_with_codec(&mut cx, &codec, Input::Text(encoded), ReadPolicy::default()).unwrap();
    assert_eq!(decoded, expr);
}
