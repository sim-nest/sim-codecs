use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy, Symbol};
use sim_shape::{ExprKind, ExprKindShape, Shape, TableExtraPolicy, TableFieldSpec, TableShape};

use super::domain_form::{DomainFormError, DomainValue, format_domain_form, parse_domain_form};

#[test]
fn projects_domain_values_to_expr_tree() {
    let form = parse_domain_form(
        "#(Chord 60 \"lead\" pitches=[60,\"64\",#(Note dur=1/4)] inner=#(Rest dur=1/8))",
    )
    .expect("parse");

    assert_eq!(
        form.to_expr_map(),
        Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("form")),
                Expr::String("Chord".to_owned()),
            ),
            (
                Expr::Symbol(Symbol::new("args")),
                Expr::List(vec![
                    Expr::String("60".to_owned()),
                    Expr::String("lead".to_owned()),
                ]),
            ),
            (
                Expr::Symbol(Symbol::new("pitches")),
                Expr::List(vec![
                    Expr::String("60".to_owned()),
                    Expr::String("64".to_owned()),
                    Expr::Map(vec![
                        (
                            Expr::Symbol(Symbol::new("form")),
                            Expr::String("Note".to_owned()),
                        ),
                        (
                            Expr::Symbol(Symbol::new("dur")),
                            Expr::String("1/4".to_owned()),
                        ),
                    ]),
                ]),
            ),
            (
                Expr::Symbol(Symbol::new("inner")),
                Expr::Map(vec![
                    (
                        Expr::Symbol(Symbol::new("form")),
                        Expr::String("Rest".to_owned()),
                    ),
                    (
                        Expr::Symbol(Symbol::new("dur")),
                        Expr::String("1/8".to_owned()),
                    ),
                ]),
            ),
        ])
    );
}

#[test]
fn projected_domain_form_is_accepted_by_table_shape() {
    let form = parse_domain_form("#(Note dur=1/4)").expect("parse");
    let expr = form.to_expr_map();
    let string_shape = || Arc::new(ExprKindShape::new(ExprKind::String));
    let shape = TableShape::new(
        vec![
            TableFieldSpec {
                key: Symbol::new("form"),
                shape: string_shape(),
                required: true,
            },
            TableFieldSpec {
                key: Symbol::new("dur"),
                shape: string_shape(),
                required: true,
            },
        ],
        TableExtraPolicy::Reject,
    );
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));

    assert!(shape.check_expr(&mut cx, &expr).unwrap().accepted);
}

#[test]
fn parses_keyed_form_with_list_and_nested_form() {
    let form = parse_domain_form("#(Note dur=4/4 pitch=C4 tags=[a,b] inner=#(Rest dur=1/4))")
        .expect("parse");
    assert_eq!(form.name, "Note");
    assert_eq!(form.atom("dur").unwrap(), "4/4");
    assert_eq!(form.atom("pitch").unwrap(), "C4");
    assert_eq!(
        form.list("tags").unwrap(),
        &[
            DomainValue::Atom("a".to_owned()),
            DomainValue::Atom("b".to_owned())
        ]
    );
    assert_eq!(form.form("inner").unwrap().name, "Rest");
}

#[test]
fn parses_positional_values() {
    let form = parse_domain_form("#(Chord 60 64 67)").expect("parse");
    assert_eq!(form.positional.len(), 3);
    assert!(form.fields.is_empty());
}

#[test]
fn round_trips_through_format() {
    let source = "#(Note dur=4/4 sym=\"a\\\"b\" pitches=[60,64])";
    let form = parse_domain_form(source).expect("parse");
    let rendered = format_domain_form(&form);
    assert_eq!(parse_domain_form(&rendered).unwrap(), form);
}

#[test]
fn missing_close_paren_is_unexpected_eof() {
    assert_eq!(
        parse_domain_form("#(Note dur=4/4"),
        Err(DomainFormError::UnexpectedEof)
    );
}

#[test]
fn duplicate_field_is_rejected() {
    assert_eq!(
        parse_domain_form("#(Note dur=4/4 dur=1/2)"),
        Err(DomainFormError::DuplicateField("dur".to_owned()))
    );
}

#[test]
fn trailing_input_is_rejected() {
    assert_eq!(
        parse_domain_form("#(Note dur=4/4) extra"),
        Err(DomainFormError::TrailingInput)
    );
}

#[test]
fn non_form_input_is_rejected() {
    assert_eq!(
        parse_domain_form("Note"),
        Err(DomainFormError::ExpectedForm)
    );
}

#[test]
fn escaped_quotes_round_trip() {
    let form = parse_domain_form("#(S v=\"he said \\\"hi\\\"\")").expect("parse");
    assert_eq!(form.string("v").unwrap(), "he said \"hi\"");
}

#[test]
fn field_atom_or_string_accepts_both_kinds() {
    let form = parse_domain_form("#(N a=bare s=\"quoted\")").expect("parse");
    assert_eq!(form.field_atom_or_string("a").unwrap(), "bare");
    assert_eq!(form.field_atom_or_string("s").unwrap(), "quoted");
}

#[test]
fn field_atom_or_string_rejects_list_and_missing() {
    let form = parse_domain_form("#(N tags=[a,b])").expect("parse");
    assert_eq!(
        form.field_atom_or_string("tags"),
        Err(DomainFormError::WrongFieldKind("tags".to_owned()))
    );
    assert_eq!(
        form.field_atom_or_string("nope"),
        Err(DomainFormError::MissingField("nope".to_owned()))
    );
}

#[test]
fn field_text_renders_every_value_kind() {
    let form = parse_domain_form("#(N a=bare s=\"q\\\"x\" tags=[a,b] inner=#(Rest dur=1/4))")
        .expect("parse");
    assert_eq!(form.field_text("a").unwrap(), "bare");
    assert_eq!(form.field_text("s").unwrap(), "\"q\\\"x\"");
    assert_eq!(form.field_text("tags").unwrap(), "[a,b]");
    assert_eq!(form.field_text("inner").unwrap(), "#(Rest dur=1/4)");
    assert_eq!(
        form.field_text("nope"),
        Err(DomainFormError::MissingField("nope".to_owned()))
    );
}

#[test]
fn value_as_form_and_atom_or_string() {
    let form = parse_domain_form("#(N inner=#(Rest dur=1/4) a=bare s=\"q\")").expect("parse");
    assert_eq!(form.field("inner").unwrap().as_form().unwrap().name, "Rest");
    assert_eq!(
        form.field("a").unwrap().as_form(),
        Err(DomainFormError::WrongValueKind)
    );
    assert_eq!(form.field("a").unwrap().atom_or_string().unwrap(), "bare");
    assert_eq!(form.field("s").unwrap().atom_or_string().unwrap(), "q");
    assert_eq!(
        form.field("inner").unwrap().atom_or_string(),
        Err(DomainFormError::WrongValueKind)
    );
}

#[test]
fn render_text_round_trips_a_form_value() {
    let source = "#(Note dur=4/4 sym=\"a\\\"b\" pitches=[60,64])";
    let form = parse_domain_form(source).expect("parse");
    let value = DomainValue::Form(form.clone());
    let rendered = value.render_text();
    assert_eq!(parse_domain_form(&rendered).unwrap(), form);
}

#[test]
fn render_text_formats_list_and_string_and_atom() {
    assert_eq!(DomainValue::Atom("60".to_owned()).render_text(), "60");
    assert_eq!(
        DomainValue::String("a\"b\\c".to_owned()).render_text(),
        "\"a\\\"b\\\\c\""
    );
    let list = DomainValue::List(vec![
        DomainValue::Atom("a".to_owned()),
        DomainValue::String("b".to_owned()),
    ]);
    assert_eq!(list.render_text(), "[a,\"b\"]");
}
