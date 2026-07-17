use std::sync::Arc;

use sim_codec::{DecodeLimits, Input, decode_with_codec_and_limits, encode_with_codec};
use sim_kernel::{CodecId, ReadPolicy};
use sim_kernel::{DefaultFactory, EagerPolicy, EncodeOptions, Expr, Symbol};

use crate::{
    BridgeBook, BridgeCodecLib, BridgeFramePayload, BridgeHeader, BridgePacket, BridgePart,
    BridgeProvenance, BridgeReceiptPayload, BridgeWarrantPolicy, decode_bridge_text,
    decode_bridge_text_with_limits, encode_bridge_text, frame_book_content_id, packet_to_expr,
    warrant_for_packet,
};

fn brief_packet() -> BridgePacket {
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
                payload: BridgeFramePayload::new(Symbol::qualified("bridge", "proposal")).to_expr(),
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

#[test]
fn bridge_book_accepts_standard_packet() {
    let book = BridgeBook::standard();
    let packet = brief_packet();
    let text = encode_bridge_text(&packet, &book).unwrap();
    let decoded = decode_bridge_text(&text, &book).unwrap();

    assert_eq!(decoded, packet);
}

#[test]
fn bridge_payload_json_honors_decode_collection_limit() {
    let book = BridgeBook::standard();
    let packet = brief_packet();
    let text = encode_bridge_text(&packet, &book).unwrap();
    let limits = DecodeLimits {
        max_collection_len: 0,
        ..DecodeLimits::default()
    };

    let line_err = decode_bridge_text_with_limits(&text, &book, CodecId(0), limits).unwrap_err();
    assert!(
        line_err.to_string().contains("collection length"),
        "expected collection-length budget error, got {line_err:?}"
    );

    let mut cx = cx();
    let codec_err = decode_with_codec_and_limits(
        &mut cx,
        &Symbol::qualified("codec", "bridge"),
        Input::Text(text),
        ReadPolicy::default(),
        limits,
    )
    .unwrap_err();
    assert!(
        codec_err.to_string().contains("collection length"),
        "expected collection-length budget error, got {codec_err:?}"
    );
}

#[test]
fn move_legality_rejects_missing_required_return() {
    let book = BridgeBook::standard();
    let mut packet = brief_packet();
    packet.body.pop();
    let err = encode_bridge_text(&packet, &book).unwrap_err();

    assert!(err.to_string().contains("requires"));
}

#[test]
fn move_legality_rejects_illegal_reply_for_opening_move() {
    let book = BridgeBook::standard();
    let mut packet = brief_packet();
    packet.header.parents = vec!["core/sha256-bridge-v1:parent#move=reply".to_owned()];
    let err = encode_bridge_text(&packet, &book).unwrap_err();

    assert!(err.to_string().contains("cannot reply"));
}

#[test]
fn parent_move_evidence_is_required_for_reply_packets() {
    let book = BridgeBook::standard();
    let mut packet = brief_packet();
    packet.header.parents = vec!["core/sha256-bridge-v1:parent".to_owned()];
    let err = encode_bridge_text(&packet, &book).unwrap_err();

    assert!(err.to_string().contains("missing #move=<intent> evidence"));
}

#[test]
fn unprofiled_body_fails_line_and_codec_encode() {
    let book = BridgeBook::standard();
    let mut packet = brief_packet();
    packet.body.push(BridgePart {
        id: Symbol::new("Rc1"),
        kind: Symbol::qualified("bridge", "Receipt"),
        payload: BridgeReceiptPayload::new(Symbol::new("accepted"), vec!["O1".to_owned()])
            .to_expr(),
    });
    let err = encode_bridge_text(&packet, &book).unwrap_err();

    assert!(err.to_string().contains("matches no standard profile"));

    let mut cx = cx();
    let expr = packet_to_expr(&packet);
    let codec_err = encode_with_codec(
        &mut cx,
        &Symbol::qualified("codec", "bridge"),
        &expr,
        EncodeOptions::default(),
    )
    .unwrap_err();

    assert!(
        codec_err
            .to_string()
            .contains("matches no standard profile")
    );
}

#[test]
fn verify_policy_rejects_missing_warrant() {
    let book = BridgeBook::standard().with_warrant_policy(BridgeWarrantPolicy::Verify);
    let err = book.validate_packet(&brief_packet()).unwrap_err();

    assert!(err.to_string().contains("warrant is required"));
}

#[test]
fn verify_policy_rejects_bad_warrant() {
    let book = BridgeBook::standard().with_warrant_policy(BridgeWarrantPolicy::Verify);
    let mut packet = brief_packet();
    let mut warrant = warrant_for_packet(&book, &packet).unwrap();
    warrant.moves = frame_book_content_id(&book.frames).unwrap();
    packet.warrant = Some(warrant);
    let err = book.validate_packet(&packet).unwrap_err();

    assert!(err.to_string().contains("warrant does not match"));
}

#[test]
fn verify_policy_accepts_matching_warrant() {
    let book = BridgeBook::standard().with_warrant_policy(BridgeWarrantPolicy::Verify);
    let mut packet = brief_packet();
    packet.warrant = Some(warrant_for_packet(&book, &packet).unwrap());
    let text = encode_bridge_text(&packet, &book).unwrap();
    let decoded = decode_bridge_text(&text, &book).unwrap();

    assert_eq!(decoded, packet);
}
