use std::sync::Arc;

use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{DefaultFactory, EagerPolicy, EncodeOptions, Expr, NumberLiteral, Symbol};

use crate::{
    AuthorityClass, BridgeBook, BridgeCodecLib, BridgeHeader, BridgePacket, BridgePart,
    BridgePartSpec, BridgeProvenance, RenderClass, UnknownPolicy, assert_roundtrip,
    assert_total_ownership, decode_bridge_text, encode_bridge_text, expr_to_packet,
    packet_content_id, packet_to_expr, stamp_packet_cid, verify_packet_cid,
};

fn packet() -> BridgePacket {
    BridgePacket {
        header: BridgeHeader {
            cid: None,
            move_kind: Symbol::new("request"),
            from: "sim".to_owned(),
            to: vec!["model:drafter".to_owned()],
            role: Symbol::new("implementer"),
            parents: Vec::new(),
            task: Symbol::new("T1"),
            output: Symbol::new("O1"),
            ceiling: vec![Symbol::qualified("ai", "run")],
            context: vec![Symbol::new("C1")],
            provenance: BridgeProvenance::default(),
        },
        body: vec![
            BridgePart {
                id: Symbol::new("T1"),
                kind: Symbol::qualified("bridge", "Frame"),
                payload: Expr::Map(vec![sim_value::build::entry(
                    "frame",
                    Expr::Symbol(Symbol::qualified("bridge", "proposal")),
                )]),
            },
            BridgePart {
                id: Symbol::new("O1"),
                kind: Symbol::qualified("bridge", "Return"),
                payload: Expr::Map(vec![sim_value::build::entry(
                    "codec",
                    Expr::Symbol(Symbol::qualified("codec", "bridge")),
                )]),
            },
        ],
        warrant: None,
    }
}

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_kernel::Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    let lib = BridgeCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lib).unwrap();
    cx
}

fn codec_symbol() -> Symbol {
    Symbol::qualified("codec", "bridge")
}

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
    let text = "BRIDGE/1\nCID nil\nMOVE request\nFROM sim\nTO [model:drafter]\nROLE implementer\nPARENTS []\nTASK T1\nOUTPUT O1\nCEIL [ai/run]\nCONTEXT []\nPROV author=sim card=nil\nBODY\nCUSTOM X1 payload={\"$expr\":\"nil\"}\nEND\n";

    assert!(decode_bridge_text(text, &book).is_err());

    let book = book.with_part(BridgePartSpec::new(
        Symbol::qualified("bridge", "Custom"),
        Expr::Symbol(Symbol::qualified("bridge", "Custom")),
        RenderClass::Extension,
        AuthorityClass::Data,
        UnknownPolicy::PreserveDataOnly,
    ));
    assert!(decode_bridge_text(text, &book).is_ok());
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
}

#[test]
fn line_payload_roundtrips_non_datum_expr() {
    let mut packet = packet();
    packet.body[0].payload = Expr::Call {
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
