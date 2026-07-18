use std::sync::Arc;

use sim_kernel::{DefaultFactory, EagerPolicy, Expr, Symbol};

use crate::{
    BridgeAttestPayload, BridgeCallArgument, BridgeCallPayload, BridgeCodecLib,
    BridgeEvidencePayload, BridgeHeader, BridgePacket, BridgePart, BridgePatchPayload,
    BridgeProvenance, BridgeReceiptPayload, BridgeReviewPayload, BridgeScore, BridgeVotePayload,
    BridgeWeavePayload, BridgeWeaveRow, CallArgumentMedia,
};

pub(super) fn packet() -> BridgePacket {
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

fn call_payload() -> BridgeCallPayload {
    BridgeCallPayload::new(Symbol::qualified("bridge", "answer-question")).with_arg(
        BridgeCallArgument::new(
            Symbol::new("question"),
            Symbol::qualified("codec", "json"),
            CallArgumentMedia::Text,
            "core/sha256-datum-v1:fixture".to_owned(),
            "<sim-data-core-sha256-datum-v1-abcdef id=\"question\">\n{}\n</sim-data-core-sha256-datum-v1-abcdef>".to_owned(),
        ),
    )
}

pub(super) fn ask_packet() -> BridgePacket {
    BridgePacket {
        header: BridgeHeader {
            cid: None,
            move_kind: Symbol::new("request"),
            from: "sim".to_owned(),
            to: vec!["model:drafter".to_owned()],
            role: Symbol::new("implementer"),
            parents: Vec::new(),
            task: Symbol::new("C1"),
            output: Symbol::new("O1"),
            ceiling: vec![Symbol::qualified("ai", "run")],
            context: Vec::new(),
            provenance: BridgeProvenance::default(),
        },
        body: vec![
            BridgePart {
                id: Symbol::new("C1"),
                kind: Symbol::qualified("bridge", "Call"),
                payload: call_payload().to_expr(),
            },
            BridgePart {
                id: Symbol::new("O1"),
                kind: Symbol::qualified("bridge", "Return"),
                payload: Expr::Map(vec![sim_value::build::entry(
                    "codec",
                    Expr::Symbol(Symbol::qualified("codec", "json")),
                )]),
            },
        ],
        warrant: None,
    }
}

fn weave_payload() -> BridgeWeavePayload {
    BridgeWeavePayload::new(vec![BridgeWeaveRow::new(
        "answer",
        Symbol::new("reply"),
        vec![(Symbol::new("input"), Expr::Symbol(Symbol::new("T1")))],
    )])
}

pub(super) fn loom_packet() -> BridgePacket {
    BridgePacket {
        header: BridgeHeader {
            cid: None,
            move_kind: Symbol::new("request"),
            from: "sim".to_owned(),
            to: vec!["model:drafter".to_owned()],
            role: Symbol::new("implementer"),
            parents: Vec::new(),
            task: Symbol::new("W1"),
            output: Symbol::new("O1"),
            ceiling: vec![Symbol::qualified("ai", "run")],
            context: vec![Symbol::new("T1")],
            provenance: BridgeProvenance::default(),
        },
        body: vec![
            BridgePart {
                id: Symbol::new("W1"),
                kind: Symbol::qualified("bridge", "Weave"),
                payload: weave_payload().to_expr(),
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

pub(super) fn collab_packet() -> BridgePacket {
    BridgePacket {
        header: BridgeHeader {
            cid: None,
            move_kind: Symbol::new("review"),
            from: "human:reviewer".to_owned(),
            to: vec!["model:drafter".to_owned()],
            role: Symbol::new("reviewer"),
            parents: vec!["core/sha256-bridge-v1:parent#move=patch".to_owned()],
            task: Symbol::new("P1"),
            output: Symbol::new("Rc1"),
            ceiling: vec![Symbol::qualified("review", "comment")],
            context: Vec::new(),
            provenance: BridgeProvenance::default(),
        },
        body: vec![
            BridgePart {
                id: Symbol::new("R1"),
                kind: Symbol::qualified("bridge", "Review"),
                payload: BridgeReviewPayload::new("body/0/payload", "tighten wording").to_expr(),
            },
            BridgePart {
                id: Symbol::new("V1"),
                kind: Symbol::qualified("bridge", "Vote"),
                payload: BridgeVotePayload::new(
                    "body/0/payload",
                    vec![BridgeScore::new(
                        Symbol::new("correctness"),
                        1,
                        "keeps the contract",
                    )],
                )
                .to_expr(),
            },
            BridgePart {
                id: Symbol::new("P1"),
                kind: Symbol::qualified("bridge", "Patch"),
                payload: BridgePatchPayload::new(
                    "core/sha256-bridge-v1:parent",
                    "body/0/payload",
                    Expr::String("replacement".to_owned()),
                )
                .to_expr(),
            },
            BridgePart {
                id: Symbol::new("E1"),
                kind: Symbol::qualified("bridge", "Evidence"),
                payload: BridgeEvidencePayload::new("packet:P1", "checked locally").to_expr(),
            },
            BridgePart {
                id: Symbol::new("Rc1"),
                kind: Symbol::qualified("bridge", "Receipt"),
                payload: BridgeReceiptPayload::new(
                    Symbol::new("accepted"),
                    vec!["body/0/payload".to_owned()],
                )
                .to_expr(),
            },
            BridgePart {
                id: Symbol::new("A1"),
                kind: Symbol::qualified("bridge", "Attest"),
                payload: BridgeAttestPayload::new("packet:P1", "reviewed", vec!["E1".to_owned()])
                    .to_expr(),
            },
        ],
        warrant: None,
    }
}

pub(super) fn cx() -> sim_kernel::Cx {
    let mut cx = sim_kernel::Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    let lib = BridgeCodecLib::new(cx.registry_mut().fresh_codec_id());
    cx.load_lib(&lib).unwrap();
    cx
}

pub(super) fn codec_symbol() -> Symbol {
    Symbol::qualified("codec", "bridge")
}
