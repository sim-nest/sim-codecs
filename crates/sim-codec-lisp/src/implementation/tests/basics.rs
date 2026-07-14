use super::*;

#[test]
fn decodes_lists_vectors_and_quotes() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let expr = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("(quote [1 2])".to_owned()),
        ReadPolicy::default(),
    )
    .unwrap();
    assert!(matches!(
        expr,
        Expr::Quote {
            mode: sim_kernel::QuoteMode::Quote,
            ..
        }
    ));
}

#[test]
fn codec_is_registered_as_lib_export() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    assert!(
        cx.registry()
            .codec_by_symbol(&Symbol::qualified("codec", "lisp"))
            .is_some()
    );
}

#[test]
fn quote_encoding_is_canonical_and_round_trips() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let expr = Expr::Quote {
        mode: sim_kernel::QuoteMode::Quote,
        expr: Box::new(Expr::Vector(vec![
            Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "1".to_owned(),
            }),
            Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "2".to_owned(),
            }),
        ])),
    };
    let encoded = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        &expr,
        Default::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();
    assert_eq!(encoded, "(quote [1 2])");

    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text(encoded),
        ReadPolicy::default(),
    )
    .unwrap();
    assert!(decoded.canonical_eq(&expr));
}

#[test]
fn qualified_symbols_with_dot_names_round_trip() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let codec = Symbol::qualified("codec", "lisp");
    for (expr, expected) in [
        (
            Expr::Symbol(Symbol::qualified("capability", "browse.internal")),
            "capability/browse.internal",
        ),
        (
            Expr::Symbol(Symbol::qualified("core/help", "args")),
            "core/help/args",
        ),
        (Expr::Symbol(Symbol::new("6")), "(expr:symbol nil \"6\")"),
        (Expr::Symbol(Symbol::new("+")), "+"),
        (
            Expr::Symbol(Symbol::qualified("numbers/quad", "central-3")),
            "(expr:symbol \"numbers/quad\" \"central-3\")",
        ),
    ] {
        let encoded = encode_with_codec(&mut cx, &codec, &expr, Default::default())
            .unwrap()
            .into_text()
            .unwrap();
        assert_eq!(encoded, expected);

        let decoded =
            decode_with_codec(&mut cx, &codec, Input::Text(encoded), ReadPolicy::default())
                .unwrap();
        assert_eq!(decoded, expr);
    }
}

#[test]
fn shared_string_escape_codec_roundtrips_common_escapes() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let expr = Expr::String("slash\\\\ quote\" tab\t cr\r lf\n bell\u{7}".to_owned());
    let encoded = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        &expr,
        Default::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();
    assert_eq!(
        encoded,
        "\"slash\\\\\\\\ quote\\\" tab\\t cr\\r lf\\n bell\\u{7}\""
    );
    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text(encoded),
        ReadPolicy::default(),
    )
    .unwrap();
    assert_eq!(decoded, expr);
}

#[test]
fn lower_eval_surface_preserves_quoted_payloads() {
    let quoted = Expr::Quote {
        mode: sim_kernel::QuoteMode::Quote,
        expr: Box::new(Expr::List(vec![
            Expr::Symbol(Symbol::new("+")),
            Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "1".to_owned(),
            }),
            Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "2".to_owned(),
            }),
        ])),
    };
    assert_eq!(lower_eval_surface(quoted.clone()), quoted);
}

#[test]
fn read_eval_is_capability_gated() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let denied = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("#eval(1)".to_owned()),
        ReadPolicy::default(),
    );
    assert!(matches!(
        denied,
        Err(sim_kernel::Error::CapabilityDenied { .. })
    ));

    let allowed = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("#eval(1)".to_owned()),
        policy_with(vec![read_eval_capability()]),
    )
    .unwrap();
    assert_eq!(
        allowed,
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "1".to_owned(),
        })
    );
}

#[test]
fn read_time_eval_dot_is_capability_gated() {
    // `#.` is CL read-time eval; it must be gated on read_eval like its `#eval`
    // sibling. Previously it evaluated untrusted input with no capability check.
    // (The space keeps the codec lexer from folding `.1` into one atom.)
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let denied = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("#. 1".to_owned()),
        ReadPolicy::default(),
    );
    assert!(matches!(
        denied,
        Err(sim_kernel::Error::CapabilityDenied { .. })
    ));

    let allowed = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("#. 1".to_owned()),
        policy_with(vec![read_eval_capability()]),
    )
    .unwrap();
    assert_eq!(
        allowed,
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "f64"),
            canonical: "1".to_owned(),
        })
    );
}

#[test]
fn read_eval_requires_trusted_policy_even_when_capability_is_present() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let denied = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("#eval(1)".to_owned()),
        ReadPolicy {
            trust: sim_kernel::TrustLevel::Untrusted,
            capabilities: sim_kernel::CapabilitySet::new().grant(read_eval_capability()),
        },
    );
    assert!(matches!(
        denied,
        Err(sim_kernel::Error::TrustDenied { capability, trust })
            if capability == read_eval_capability()
                && trust == sim_kernel::TrustLevel::Untrusted
    ));
}

#[test]
fn registered_number_domains_can_parse_non_f64_literals() {
    let mut cx = cx();
    let domain = cx.factory().opaque(Arc::new(RationalDomain)).unwrap();
    cx.registry_mut()
        .register_number_domain_value(Symbol::qualified("numbers", "rational"), domain)
        .unwrap();

    assert_eq!(
        cx.parse_number_literal("1/3").unwrap(),
        Some(NumberLiteral {
            domain: Symbol::qualified("numbers", "rational"),
            canonical: "1/3".to_owned(),
        })
    );
}

#[test]
fn lisp_codec_parses_rational_and_complex_literals_from_source_text() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let rational = cx.factory().opaque(Arc::new(RationalDomain)).unwrap();
    cx.registry_mut()
        .register_number_domain_value(Symbol::qualified("numbers", "rational"), rational)
        .unwrap();
    let complex = cx.factory().opaque(Arc::new(ComplexDomain)).unwrap();
    cx.registry_mut()
        .register_number_domain_value(Symbol::qualified("numbers", "complex"), complex)
        .unwrap();

    let rational = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("1/3".to_owned()),
        ReadPolicy::default(),
    )
    .unwrap();
    assert_eq!(
        rational,
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "rational"),
            canonical: "1/3".to_owned(),
        })
    );

    let complex = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("1+2i".to_owned()),
        ReadPolicy::default(),
    )
    .unwrap();
    assert_eq!(
        complex,
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "complex"),
            canonical: "1+2i".to_owned(),
        })
    );
}

#[test]
fn read_construct_is_capability_gated() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    install_point(&mut cx);

    let denied = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("#(Point 1 2)".to_owned()),
        ReadPolicy::default(),
    );
    assert!(matches!(
        denied,
        Err(sim_kernel::Error::CapabilityDenied { .. })
    ));

    cx.grant(read_construct_capability());
    let expr = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("#(Point 1 2)".to_owned()),
        policy_with(vec![read_construct_capability()]),
    )
    .unwrap();
    assert!(matches!(expr, Expr::Map(_)));
}

#[test]
fn read_construct_args_decode_as_data_by_default() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    install_point(&mut cx);
    cx.grant(read_construct_capability());

    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("#(Point (+ 1 2) 3)".to_owned()),
        policy_with(vec![read_construct_capability()]),
    )
    .unwrap();
    let Expr::Map(fields) = decoded else {
        panic!("expected read-construct result map");
    };
    assert!(fields.iter().any(|(key, value)| {
        key == &Expr::Symbol(Symbol::new("x"))
            && matches!(value, Expr::List(items) if items == &vec![
                Expr::Symbol(Symbol::new("+")),
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "1".to_owned(),
                }),
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "2".to_owned(),
                }),
            ])
    }));
}

#[test]
fn read_construct_explicit_eval_is_capability_gated() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    install_point(&mut cx);
    cx.grant(read_construct_capability());

    let denied = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("#(Point #eval(1) 3)".to_owned()),
        policy_with(vec![read_construct_capability()]),
    );
    assert!(matches!(
        denied,
        Err(sim_kernel::Error::CapabilityDenied { capability })
            if capability == read_eval_capability()
    ));
}

#[test]
fn read_construct_explicit_eval_runs_when_granted() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    install_point(&mut cx);
    cx.grant(read_construct_capability());
    cx.grant(read_eval_capability());

    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("#(Point #eval(1) 3)".to_owned()),
        policy_with(vec![read_construct_capability(), read_eval_capability()]),
    )
    .unwrap();
    let Expr::Map(fields) = decoded else {
        panic!("expected read-construct result map");
    };
    assert!(fields.iter().any(|(key, value)| {
        key == &Expr::Symbol(Symbol::new("x"))
            && value
                == &Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "1".to_owned(),
                })
    }));
}

#[test]
fn encoder_can_escape_non_native_exprs() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let encoded = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        &Expr::Infix {
            operator: Symbol::new("+"),
            left: Box::new(Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "1".to_owned(),
            })),
            right: Box::new(Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "2".to_owned(),
            })),
        },
        Default::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();
    assert_eq!(encoded, "(expr:infix \"+\" 1 2)");
}

#[test]
fn default_encoder_never_emits_read_eval_syntax() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let encoded = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        &Expr::Symbol(Symbol::new("#eval")),
        Default::default(),
    )
    .unwrap()
    .into_text()
    .unwrap();

    assert_eq!(encoded, "(expr:symbol nil \"#eval\")");
    assert!(!encoded.contains("#eval("));
    assert!(!encoded.contains("#."));
}

#[test]
fn constructor_values_encode_as_read_construct_in_quote_position() {
    let mut cx = cx();
    let point = cx
        .factory()
        .opaque(Arc::new(PointValue {
            args: vec![
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "1".to_owned(),
                }),
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "2".to_owned(),
                }),
            ],
            fields: Vec::new(),
        }))
        .unwrap();
    let mut write = KernelWriteCx {
        cx: &mut cx,
        codec: sim_kernel::CodecId(1),
        options: sim_kernel::EncodeOptions {
            position: EncodePosition::Quote,
            ..Default::default()
        },
    };
    let encoded = encode_object_lisp(&mut write, point).unwrap();
    assert_eq!(encoded, "#(Point 1 2)");
}

#[test]
fn constructor_values_encode_as_read_construct_in_data_position() {
    let mut cx = cx();
    let point = cx
        .factory()
        .opaque(Arc::new(PointValue {
            args: vec![
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "1".to_owned(),
                }),
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "2".to_owned(),
                }),
            ],
            fields: Vec::new(),
        }))
        .unwrap();
    let mut write = KernelWriteCx {
        cx: &mut cx,
        codec: sim_kernel::CodecId(1),
        options: sim_kernel::EncodeOptions {
            position: EncodePosition::Data,
            ..Default::default()
        },
    };
    let encoded = encode_object_lisp(&mut write, point).unwrap();
    assert_eq!(encoded, "#(Point 1 2)");
}

#[test]
fn constructor_encoding_round_trips_through_lisp_codec() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    install_point(&mut cx);
    cx.grant(read_construct_capability());

    let point = cx
        .factory()
        .opaque(Arc::new(PointValue {
            args: vec![
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "1".to_owned(),
                }),
                Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "f64"),
                    canonical: "2".to_owned(),
                }),
            ],
            fields: vec![
                (
                    Symbol::new("x"),
                    cx.factory()
                        .number_literal(Symbol::qualified("numbers", "f64"), "1".to_owned())
                        .unwrap(),
                ),
                (
                    Symbol::new("y"),
                    cx.factory()
                        .number_literal(Symbol::qualified("numbers", "f64"), "2".to_owned())
                        .unwrap(),
                ),
            ],
        }))
        .unwrap();
    let mut write = KernelWriteCx {
        cx: &mut cx,
        codec: sim_kernel::CodecId(1),
        options: sim_kernel::EncodeOptions {
            position: EncodePosition::Quote,
            ..Default::default()
        },
    };
    let encoded = encode_object_lisp(&mut write, point).unwrap();
    let decoded = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text(encoded),
        policy_with(vec![read_construct_capability()]),
    )
    .unwrap();
    assert!(matches!(decoded, Expr::Map(_)));
}

#[test]
fn malformed_dispatch_is_rejected() {
    let mut cx = cx();
    register_lisp_codec(&mut cx);
    let error = decode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("#wat".to_owned()),
        ReadPolicy::default(),
    )
    .unwrap_err();
    assert!(matches!(error, sim_kernel::Error::CodecError { .. }));
}
