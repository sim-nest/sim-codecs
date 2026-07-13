use sim_codec::{Input, decode_with_codec};
use sim_kernel::ReadPolicy;

use crate::{
    BackendId, BackendStatus, MarkupError, backend_catalog, default_backend_registry,
    install_doc_codec, markup_codec_symbol,
};

fn cx() -> sim_kernel::Cx {
    let mut cx = sim_test_support::core_cx();
    sim_test_support::register_f64_number_domain(&mut cx);
    install_doc_codec(&mut cx).unwrap();
    cx
}

#[test]
fn implemented_backends_are_registered() {
    let registry = default_backend_registry();
    let mut implemented = 0;

    for info in backend_catalog()
        .into_iter()
        .filter(|info| info.status == BackendStatus::Implemented)
    {
        implemented += 1;
        assert!(info.can_read, "{} should read", info.id);
        assert!(info.can_write, "{} should write", info.id);
        assert!(
            registry.backend(&info.id).is_ok(),
            "{} should be registered",
            info.id
        );
    }

    assert_eq!(implemented, 4);
    for id in registry.ids() {
        assert!(
            backend_catalog()
                .into_iter()
                .any(|info| info.id == id && info.status == BackendStatus::Implemented),
            "registered backend {id} should be implemented in the catalog"
        );
    }
}

#[test]
fn tracked_backends_fail_closed() {
    let registry = default_backend_registry();
    let mut cx = cx();
    let mut tracked = 0;
    let mut external_candidates = 0;

    for info in backend_catalog()
        .into_iter()
        .filter(|info| info.status != BackendStatus::Implemented)
    {
        match info.status {
            BackendStatus::Tracked => tracked += 1,
            BackendStatus::ExternalSiteCandidate => external_candidates += 1,
            BackendStatus::Implemented => unreachable!(),
        }
        assert!(!info.can_read, "{} should not read", info.id);
        assert!(!info.can_write, "{} should not write", info.id);
        let Err(MarkupError::UnknownBackend(id)) = registry.backend(&info.id) else {
            panic!("expected {} to fail closed", info.id);
        };
        assert_eq!(id, info.id);

        let symbol = markup_codec_symbol(&info.id);
        let symbol_text = symbol.to_string();
        let err = decode_with_codec(
            &mut cx,
            &symbol,
            Input::Text("= Catalog\n".to_owned()),
            ReadPolicy::default(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains(&symbol_text),
            "{symbol} should fail closed by codec lookup"
        );
    }

    assert_eq!(tracked, 8);
    assert_eq!(external_candidates, 1);
}

#[test]
fn catalog_contains_texinfo_as_external_candidate() {
    let texinfo = backend_catalog()
        .into_iter()
        .find(|info| info.id == BackendId::new("texinfo"))
        .expect("texinfo is cataloged");

    assert_eq!(texinfo.status, BackendStatus::ExternalSiteCandidate);
    assert!(!texinfo.can_read);
    assert!(!texinfo.can_write);
    assert!(texinfo.notes.contains("texi2any"));
}
