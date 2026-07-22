use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{EncodeOptions, Expr, NumberLiteral, Symbol};

use crate::{
    AuthorityClass, BridgeBook, BridgeFramePayload, BridgePart, BridgePartSpec, BridgePatchPayload,
    BridgeProfileSpec, BridgeVotePayload, BridgeWarrantPolicy, FrameHoleKind, FrameHoleSpec,
    FrameKind, FrameSpec, ProfilePartCount, ProfilePartRule, RenderClass, UnknownPolicy,
    ask_profile_symbol, assert_roundtrip, assert_total_ownership, bridge_profile_shape_expr,
    brief_profile_symbol, collab_profile_symbol, decode_bridge_text, encode_bridge_text,
    expr_to_packet, frame_book_content_id, loom_profile_symbol, packet_content_id, packet_to_expr,
    render_frame_part, stamp_packet_cid, verify_packet_cid, warrant_for_packet,
};

mod support;

use support::{ask_packet, codec_symbol, collab_packet, cx, loom_packet, packet};

#[test]
fn codec_registers() {
    let cx = cx();
    assert!(cx.registry().codec_by_symbol(&codec_symbol()).is_some());
}

#[test]
fn packet_roundtrips_and_cid_is_stable() {
    let book = BridgeBook::standard();
    let packet = stamp_packet_cid(&packet()).unwrap();
    let first_id = packet_content_id(&packet).unwrap();
    let text = encode_bridge_text(&packet, &book).unwrap();
    let decoded = decode_bridge_text(&text, &book).unwrap();
    let second_text = encode_bridge_text(&decoded, &book).unwrap();

    assert_eq!(decoded, packet);
    assert_eq!(second_text, text);
    assert_eq!(packet_content_id(&decoded).unwrap(), first_id);
    assert_roundtrip(&packet, &book).unwrap();
    verify_packet_cid(&decoded).unwrap();
}

#[test]
fn packet_warrant_roundtrips_through_expr_and_line_face() {
    let book = BridgeBook::standard().with_warrant_policy(BridgeWarrantPolicy::Verify);
    let mut packet = packet();
    packet.warrant = Some(warrant_for_packet(&book, &packet).unwrap());
    let packet = stamp_packet_cid(&packet).unwrap();
    let warrant = packet.warrant.as_ref().unwrap();

    assert_eq!(warrant.parts.len(), 2);
    assert_eq!(warrant.parts[0].0, Symbol::qualified("bridge", "Frame"));
    assert_eq!(warrant.parts[1].0, Symbol::qualified("bridge", "Return"));

    let expr = packet_to_expr(&packet);
    assert_eq!(expr_to_packet(&expr).unwrap(), packet);

    let text = encode_bridge_text(&packet, &book).unwrap();
    assert!(text.contains("WARRANT moves="));
    assert!(text.contains("parts=[bridge/Frame="));
    let decoded = decode_bridge_text(&text, &book).unwrap();

    assert_eq!(decoded, packet);
    verify_packet_cid(&decoded).unwrap();
    assert_roundtrip(&packet, &book).unwrap();
}

#[test]
fn codec_roundtrips_packet_expression() {
    let mut cx = cx();
    let packet = stamp_packet_cid(&packet()).unwrap();
    let expr = packet_to_expr(&packet);
    let output = encode_with_codec(&mut cx, &codec_symbol(), &expr, EncodeOptions::default())
        .unwrap()
        .into_text()
        .unwrap();
    let decoded = decode_with_codec(
        &mut cx,
        &codec_symbol(),
        Input::Text(output),
        Default::default(),
    )
    .unwrap();

    assert_eq!(expr_to_packet(&decoded).unwrap(), packet);
}

#[test]
fn mutated_body_fails_cid_verification() {
    let mut packet = stamp_packet_cid(&packet()).unwrap();
    packet.body.push(BridgePart {
        id: Symbol::new("X1"),
        kind: Symbol::qualified("bridge", "Evidence"),
        payload: Expr::String("changed".to_owned()),
    });

    assert!(verify_packet_cid(&packet).is_err());
}

#[test]
fn unknown_header_rejects() {
    let book = BridgeBook::standard();
    let packet = stamp_packet_cid(&packet()).unwrap();
    let text = encode_bridge_text(&packet, &book)
        .unwrap()
        .replace("ROLE implementer", "UNKNOWN implementer");

    assert!(decode_bridge_text(&text, &book).is_err());
}

#[test]
fn unknown_normative_part_rejects() {
    let book = BridgeBook::standard();
    let custom_kind = Symbol::qualified("bridge", "Custom");
    let custom_profile = Symbol::qualified("bridge", "BRIEF-CUSTOM");
    let mut packet = packet();
    packet.body.push(BridgePart {
        id: Symbol::new("X1"),
        kind: custom_kind.clone(),
        payload: Expr::Nil,
    });

    assert!(encode_bridge_text(&packet, &book).is_err());

    let book = book
        .with_part(BridgePartSpec::new(
            custom_kind.clone(),
            Expr::Symbol(custom_kind.clone()),
            RenderClass::Extension,
            AuthorityClass::Data,
            UnknownPolicy::PreserveDataOnly,
        ))
        .with_profile(BridgeProfileSpec::new(
            custom_profile,
            vec![
                ProfilePartRule::new(
                    Symbol::qualified("bridge", "Frame"),
                    ProfilePartCount::OneOrMore,
                ),
                ProfilePartRule::new(
                    Symbol::qualified("bridge", "Return"),
                    ProfilePartCount::Optional,
                ),
                ProfilePartRule::new(custom_kind, ProfilePartCount::Optional),
            ],
        ));
    let text = encode_bridge_text(&packet, &book).unwrap();
    let decoded = decode_bridge_text(&text, &book).unwrap();

    assert_eq!(decoded, packet);
}

#[test]
fn unowned_span_fails_total_ownership() {
    assert_total_ownership("abcdef", &[crate::OwnedSpan::Structural("abc".to_owned())])
        .unwrap_err();
}

#[test]
fn vote_may_not_answer_request() {
    let book = BridgeBook::standard();
    let result = book.moves.check_move(
        &Symbol::new("vote"),
        &[Symbol::new("request")],
        &[Symbol::qualified("bridge", "Vote")],
    );

    assert!(result.is_err());
}

#[test]
fn receipt_requires_receipt_part() {
    let book = BridgeBook::standard();
    let result = book
        .moves
        .check_move(&Symbol::new("receipt"), &[Symbol::new("reply")], &[]);
    assert!(result.is_err());

    book.moves
        .check_move(
            &Symbol::new("receipt"),
            &[Symbol::new("reply")],
            &[Symbol::qualified("bridge", "Receipt")],
        )
        .unwrap();
}

#[test]
fn attest_requires_attest_part() {
    let book = BridgeBook::standard();
    book.moves
        .check_move(
            &Symbol::new("attest"),
            &[Symbol::new("reply")],
            &[Symbol::qualified("bridge", "Attest")],
        )
        .unwrap();
    assert!(
        book.moves
            .check_move(
                &Symbol::new("attest"),
                &[Symbol::new("reply")],
                &[Symbol::qualified("bridge", "Evidence")],
            )
            .is_err()
    );
}

#[test]
fn request_requires_frame_and_return_parts() {
    let book = BridgeBook::standard();
    book.moves
        .check_move(
            &Symbol::new("request"),
            &[],
            &[
                Symbol::qualified("bridge", "Frame"),
                Symbol::qualified("bridge", "Return"),
            ],
        )
        .unwrap();
    book.moves
        .check_move(
            &Symbol::new("request"),
            &[],
            &[
                Symbol::qualified("bridge", "Call"),
                Symbol::qualified("bridge", "Return"),
            ],
        )
        .unwrap();
    book.moves
        .check_move(
            &Symbol::new("request"),
            &[],
            &[
                Symbol::qualified("bridge", "Weave"),
                Symbol::qualified("bridge", "Return"),
            ],
        )
        .unwrap();
    assert!(
        book.moves
            .check_move(
                &Symbol::new("request"),
                &[],
                &[Symbol::qualified("bridge", "Return")],
            )
            .is_err()
    );
}

#[test]
fn registered_frame_renders_canonical_and_fluent_faces() {
    let book = BridgeBook::standard();
    let part = BridgePart {
        id: Symbol::new("T1"),
        kind: Symbol::qualified("bridge", "Frame"),
        payload: BridgeFramePayload::new(Symbol::qualified("bridge", "produce-artifact"))
            .with_slot(
                Symbol::new("what"),
                Expr::Symbol(Symbol::qualified("bridge", "proposal")),
            )
            .with_slot(
                Symbol::new("target"),
                Expr::String("sim-human-model".to_owned()),
            )
            .to_expr(),
    };
    let mut packet = packet();
    packet.body[0] = part.clone();
    let packet = stamp_packet_cid(&packet).unwrap();
    let line = encode_bridge_text(&packet, &book).unwrap();
    let fluent = render_frame_part(&book, &part).unwrap();

    assert!(line.contains("FRAME T1 payload="));
    assert_eq!(
        fluent,
        "[T1] You MUST produce bridge/proposal for sim-human-model."
    );
    assert_eq!(decode_bridge_text(&line, &book).unwrap(), packet);
}

#[test]
fn owned_template_frame_spec_registers_and_changes_book_id() {
    let mut book = BridgeBook::standard();
    let before = frame_book_content_id(&book.frames).unwrap();
    book.frames.register(FrameSpec::new(
        Symbol::qualified("bridge", "summarize-transcript"),
        FrameKind::Task,
        "summarize {target}.".to_owned(),
        vec![FrameHoleSpec::new(
            Symbol::new("target"),
            FrameHoleKind::Term,
        )],
    ));
    let after = frame_book_content_id(&book.frames).unwrap();
    let part = BridgePart {
        id: Symbol::new("T1"),
        kind: Symbol::qualified("bridge", "Frame"),
        payload: BridgeFramePayload::new(Symbol::qualified("bridge", "summarize-transcript"))
            .with_slot(
                Symbol::new("target"),
                Expr::Symbol(Symbol::new("transcript")),
            )
            .to_expr(),
    };
    let fluent = render_frame_part(&book, &part).unwrap();

    assert_ne!(before, after);
    assert_eq!(fluent, "[T1] You MUST summarize transcript.");
}

#[test]
fn unknown_frame_id_rejects_at_decode_time() {
    let book = BridgeBook::standard();
    let packet = stamp_packet_cid(&packet()).unwrap();
    let text = encode_bridge_text(&packet, &book)
        .unwrap()
        .replace(r#""name":"proposal""#, r#""name":"unknown""#);
    let err = decode_bridge_text(&text, &book).unwrap_err();

    assert!(err.to_string().contains("unknown BRIDGE frame"));
}

#[test]
fn brief_profile_is_registered_as_profile_choice() {
    let book = BridgeBook::standard();
    let packet = packet();
    let profiles = book.profiles.matching_profiles(&packet);
    let shape = bridge_profile_shape_expr();

    assert_eq!(profiles, vec![brief_profile_symbol()]);
    assert!(format!("{shape:?}").contains("BRIEF"));
    assert!(format!("{shape:?}").contains("ASK"));
    assert!(format!("{shape:?}").contains("LOOM"));
    assert!(format!("{shape:?}").contains("COLLAB"));
}

#[test]
fn ask_profile_is_registered_as_call_shape() {
    let book = BridgeBook::standard();
    let packet = ask_packet();
    let profiles = book.profiles.matching_profiles(&packet);

    assert_eq!(profiles, vec![ask_profile_symbol()]);
}

#[test]
fn loom_profile_is_registered_as_weave_shape() {
    let book = BridgeBook::standard();
    let packet = loom_packet();
    let profiles = book.profiles.matching_profiles(&packet);
    let text = encode_bridge_text(&stamp_packet_cid(&packet).unwrap(), &book).unwrap();

    assert_eq!(profiles, vec![loom_profile_symbol()]);
    assert!(text.contains("WEAVE W1 payload="));
    assert!(text.contains("result-shape"));
}

#[test]
fn collab_profile_is_registered_as_any_collab_shape() {
    let book = BridgeBook::standard();
    let packet = collab_packet();
    let profiles = book.profiles.matching_profiles(&packet);
    let text = encode_bridge_text(&stamp_packet_cid(&packet).unwrap(), &book).unwrap();

    assert_eq!(profiles, vec![collab_profile_symbol()]);
    assert!(text.contains("REVIEW R1 payload="));
    assert!(text.contains("VOTE V1 payload="));
    assert!(text.contains("PATCH P1 payload="));
}

#[test]
fn collab_payloads_roundtrip_and_validate() {
    let book = BridgeBook::standard();
    let packet = stamp_packet_cid(&collab_packet()).unwrap();
    let text = encode_bridge_text(&packet, &book).unwrap();
    let decoded = decode_bridge_text(&text, &book).unwrap();
    let patch = BridgePatchPayload::from_expr(&decoded.body[2].payload).unwrap();
    let vote = BridgeVotePayload::from_expr(&decoded.body[1].payload).unwrap();

    assert_eq!(decoded, packet);
    assert_eq!(patch.target, "body/0/payload");
    assert_eq!(patch.parent_cid, "core/sha256-bridge-v1:parent");
    assert_eq!(vote.scores[0].axis, Symbol::new("correctness"));
    verify_packet_cid(&decoded).unwrap();
}

#[test]
fn empty_collab_vote_rejects() {
    let mut packet = collab_packet();
    packet.body[1].payload = BridgeVotePayload::new("body/0/payload", Vec::new()).to_expr();
    let err = encode_bridge_text(&packet, &BridgeBook::standard()).unwrap_err();

    assert!(err.to_string().contains("at least one score"));
}

#[test]
fn hand_written_weave_result_shape_that_disagrees_rejects() {
    let book = BridgeBook::standard();
    let mut packet = loom_packet();
    let Expr::Map(fields) = &mut packet.body[0].payload else {
        panic!("weave payload must be a map");
    };
    for (key, value) in fields {
        if matches!(key, Expr::Symbol(symbol) if symbol.name.as_ref() == "result-shape") {
            *value = Expr::Symbol(Symbol::qualified("core", "String"));
        }
    }
    let err = encode_bridge_text(&packet, &book).unwrap_err();

    assert!(err.to_string().contains("result-shape disagrees"));
}

#[test]
fn unfenced_call_argument_rejects_at_decode_time() {
    let book = BridgeBook::standard();
    let packet = stamp_packet_cid(&ask_packet()).unwrap();
    let text = encode_bridge_text(&packet, &book)
        .unwrap()
        .replace("<sim-data-", "<raw-data-");
    let err = decode_bridge_text(&text, &book).unwrap_err();

    assert!(err.to_string().contains("must be fence-wrapped"));
}

#[test]
fn line_payload_roundtrips_non_datum_expr() {
    let mut packet = packet();
    packet.body[1].payload = Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::new("make"))),
        args: vec![Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "i64"),
            canonical: "1".to_owned(),
        })],
    };
    let packet = stamp_packet_cid(&packet).unwrap();
    let text = encode_bridge_text(&packet, &BridgeBook::standard()).unwrap();
    let decoded = decode_bridge_text(&text, &BridgeBook::standard()).unwrap();

    assert_eq!(decoded, packet);
    verify_packet_cid(&decoded).unwrap();
}
