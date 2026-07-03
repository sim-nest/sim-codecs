use super::*;

#[test]
fn located_decode_captures_top_level_origin_and_trivia() {
    let mut host = cx();
    let mut read = ReadCx {
        cx: &mut host,
        codec: sim_kernel::CodecId(1),
        read_policy: ReadPolicy::default(),
        limits: sim_codec::DecodeLimits::default(),
    };
    let located = decode_lisp_located(
        &mut read,
        "test.lisp",
        Input::Text(" \n; lead\n(+ 1 2)\n; tail\n".to_owned()),
    )
    .unwrap();

    assert!(matches!(located.expr, Expr::List(_)));
    let origin = located.origin.unwrap();
    assert_eq!(origin.source, SourceId("test.lisp".to_owned()));
    assert_eq!(origin.span.start, 9);
    assert_eq!(origin.span.end, 16);
    assert!(!origin.trivia.is_empty());
}

#[test]
fn tree_decode_captures_child_spans() {
    let mut host = cx();
    let mut read = ReadCx {
        cx: &mut host,
        codec: sim_kernel::CodecId(1),
        read_policy: ReadPolicy::default(),
        limits: sim_codec::DecodeLimits::default(),
    };
    let tree = decode_lisp_tree(
        &mut read,
        "tree.lisp",
        Input::Text("(f [1 2] true)".to_owned()),
    )
    .unwrap();

    assert_eq!(tree.origin.as_ref().unwrap().span.start, 0);
    assert_eq!(tree.origin.as_ref().unwrap().span.end, 14);
    assert_eq!(tree.children.len(), 3);
    assert_eq!(tree.children[0].origin.as_ref().unwrap().span.start, 1);
    assert_eq!(tree.children[0].origin.as_ref().unwrap().span.end, 2);
    assert_eq!(tree.children[1].origin.as_ref().unwrap().span.start, 3);
    assert_eq!(tree.children[1].origin.as_ref().unwrap().span.end, 8);
    assert_eq!(
        tree.children[1].children[0]
            .origin
            .as_ref()
            .unwrap()
            .span
            .start,
        4
    );
    assert_eq!(
        tree.children[1].children[0]
            .origin
            .as_ref()
            .unwrap()
            .span
            .end,
        5
    );
    assert_eq!(
        tree.children[1].children[1]
            .origin
            .as_ref()
            .unwrap()
            .span
            .start,
        6
    );
    assert_eq!(
        tree.children[1].children[1]
            .origin
            .as_ref()
            .unwrap()
            .span
            .end,
        7
    );
}

#[test]
fn located_read_time_eval_dot_is_capability_gated() {
    // The located reader (proc-macro token path) has its own `#.` arm; it must
    // gate on read_eval exactly like the flat reader, failing closed under the
    // default policy. proc-macro lexing keeps `.` a distinct punct, so `#.1` is
    // the right form here.
    let mut host = cx();
    let mut read = ReadCx {
        cx: &mut host,
        codec: sim_kernel::CodecId(1),
        read_policy: ReadPolicy::default(),
        limits: sim_codec::DecodeLimits::default(),
    };
    let denied = decode_lisp_located(&mut read, "dot.lisp", Input::Text("#.1".to_owned()));
    assert!(matches!(
        denied,
        Err(sim_kernel::Error::CapabilityDenied { .. })
    ));
}

#[test]
fn tree_decode_attaches_leading_trivia_to_child_nodes() {
    let mut host = cx();
    let mut read = ReadCx {
        cx: &mut host,
        codec: sim_kernel::CodecId(1),
        read_policy: ReadPolicy::default(),
        limits: sim_codec::DecodeLimits::default(),
    };
    let tree = decode_lisp_tree(
        &mut read,
        "tree.lisp",
        Input::Text("(f ; note\n [1 2])".to_owned()),
    )
    .unwrap();

    let trivia = &tree.children[1].origin.as_ref().unwrap().trivia;
    assert!(!trivia.is_empty());
    assert!(
        trivia
            .iter()
            .any(|item| matches!(item, Trivia::LineComment(_)))
    );
}

#[test]
fn tree_decode_duplicates_internal_and_closing_trivia_for_group_context() {
    let mut host = cx();
    let mut read = ReadCx {
        cx: &mut host,
        codec: sim_kernel::CodecId(1),
        read_policy: ReadPolicy::default(),
        limits: sim_codec::DecodeLimits::default(),
    };
    let tree = decode_lisp_tree(
        &mut read,
        "tree.lisp",
        Input::Text("(outer (f ; between\n x ; tail\n))".to_owned()),
    )
    .unwrap();

    let inner = &tree.children[1];
    let last_child_trivia = &inner.children[1].origin.as_ref().unwrap().trivia;
    assert!(
        last_child_trivia
            .iter()
            .any(|item| matches!(item, Trivia::LineComment(text) if text.contains("tail")))
    );
    let parent_trivia = &inner.origin.as_ref().unwrap().trivia;
    assert!(
        parent_trivia
            .iter()
            .any(|item| matches!(item, Trivia::LineComment(text) if text.contains("tail")))
    );
}

#[test]
fn lisp_decode_rejects_excessive_tokens() {
    let mut host = cx();
    register_lisp_codec(&mut host);
    let limits = DecodeLimits {
        max_tokens: 4,
        ..DecodeLimits::default()
    };
    let err = decode_tree_with_codec_and_limits(
        &mut host,
        &Symbol::qualified("codec", "lisp"),
        Input::Text("(a b c d e)".to_owned()),
        ReadPolicy::default(),
        "tree.lisp",
        limits,
    )
    .unwrap_err();
    assert!(
        matches!(err, sim_kernel::Error::CodecError { message, .. } if message.contains("tokens"))
    );
}

#[test]
fn lisp_decode_rejects_excessive_depth() {
    let mut host = cx();
    register_lisp_codec(&mut host);
    let nested = "(".repeat(12) + "nil" + &")".repeat(12);
    let limits = DecodeLimits {
        max_depth: 4,
        ..DecodeLimits::default()
    };
    let err = decode_with_codec_and_limits(
        &mut host,
        &Symbol::qualified("codec", "lisp"),
        Input::Text(nested),
        ReadPolicy::default(),
        limits,
    )
    .unwrap_err();
    assert!(
        matches!(err, sim_kernel::Error::CodecError { message, .. } if message.contains("recursion depth"))
    );
}
