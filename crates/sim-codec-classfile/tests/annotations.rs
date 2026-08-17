use sim_codec_classfile::{
    AnnotationDefaultAttribute, AnnotationsAttribute, AttributeErrorKind, ByteReader, ElementValue,
    ParameterAnnotationsAttribute, TypeAnnotationTarget, TypeAnnotationsAttribute,
};

#[test]
fn declaration_annotations_preserve_duplicate_names_order_and_origins() {
    let bytes = [
        0, 1, // annotations
        0, 7, 0, 2, // type, pairs
        0, 9, b'I', 0, 11, // first duplicate
        0, 9, b's', 0, 13, // second duplicate
    ];
    let decoded =
        AnnotationsAttribute::decode(&mut ByteReader::new(&bytes, bytes.len()), 8).unwrap();
    let annotation = &decoded.annotations[0];
    assert_eq!(
        annotation
            .elements
            .iter()
            .map(|pair| pair.name_index)
            .collect::<Vec<_>>(),
        [9, 9]
    );
    assert_eq!(annotation.origin.start, 2);
    assert_eq!(annotation.origin.end, bytes.len());
    assert_eq!(annotation.elements[0].origin.start, 6);
    assert_eq!(annotation.elements[1].value.origin().end, bytes.len());
    assert_eq!(decoded.encode(bytes.len()).unwrap(), bytes);
}

#[test]
fn deeply_nested_array_stops_at_budget_before_nested_allocation() {
    let mut bytes = Vec::new();
    for _ in 0..64 {
        bytes.extend_from_slice(&[b'[', 0, 1]);
    }
    bytes.extend_from_slice(&[b'I', 0, 1]);
    let error = AnnotationDefaultAttribute::decode(&mut ByteReader::new(&bytes, bytes.len()), 12)
        .unwrap_err();
    assert_eq!(error.kind, AttributeErrorKind::NestingBudgetExceeded);
    assert_eq!(error.offset, 36);
}

#[test]
fn parameter_type_and_default_annotation_forms_round_trip() {
    let parameters = [1, 0, 1, 0, 2, 0, 0];
    let decoded = ParameterAnnotationsAttribute::decode(
        &mut ByteReader::new(&parameters, parameters.len()),
        4,
    )
    .unwrap();
    assert_eq!(decoded.parameters.len(), 1);
    assert_eq!(decoded.encode(parameters.len()).unwrap(), parameters);

    // local-variable target, one path step selecting type argument 2, empty annotation body.
    let type_bytes = [0, 1, 0x40, 0, 1, 0, 3, 0, 5, 0, 2, 1, 3, 2, 0, 17, 0, 0];
    let decoded =
        TypeAnnotationsAttribute::decode(&mut ByteReader::new(&type_bytes, type_bytes.len()), 4)
            .unwrap();
    assert!(matches!(
        decoded.annotations[0].target,
        TypeAnnotationTarget::LocalVariable { .. }
    ));
    assert_eq!(decoded.annotations[0].path[0].origin.start, 12);
    assert_eq!(decoded.encode(type_bytes.len()).unwrap(), type_bytes);

    let default_bytes = [
        b'@', 0, 5, 0, 1, 0, 6, b'[', 0, 2, b'e', 0, 7, 0, 8, b'c', 0, 9,
    ];
    let decoded = AnnotationDefaultAttribute::decode(
        &mut ByteReader::new(&default_bytes, default_bytes.len()),
        4,
    )
    .unwrap();
    assert!(matches!(decoded.value, ElementValue::Annotation { .. }));
    assert_eq!(decoded.encode(default_bytes.len()).unwrap(), default_bytes);
}
