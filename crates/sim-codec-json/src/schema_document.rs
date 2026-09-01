//! Bounded JSON Schema Draft 2020-12 documents and validation.
//!
//! Parsing retains the complete JSON value, including unknown annotations and
//! extension keywords. It intentionally does not retain lexical whitespace or
//! the original byte layout.

include!("schema_document/document.rs");
include!("schema_document/validation.rs");
include!("schema_document/adaptation.rs");
include!("schema_document/pattern.rs");
include!("schema_document/pattern_parser.rs");
include!("schema_document/tests.rs");
