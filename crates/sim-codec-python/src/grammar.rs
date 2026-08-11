//! Frozen Python syntax authority and coverage inventory.

/// Exact Python patch release targeted by this frontend.
pub const PYTHON_VERSION: &str = "3.14.6";
/// SHA-256 of `grammar/python-3.14.6.gram`.
pub const GRAMMAR_SHA256: &str = "e202af7e205b898d38676e6ce5aa211bc80cdb27635efc5d386bd571fefb50c1";
/// SHA-256 of `grammar/corpus-3.14.6.txt`.
pub const CORPUS_SHA256: &str = "628c172d99c245b204c8a2891e996e50624f8c372a0248d3a3d18e445dd49392";

/// Frozen production names checked by the syntax corpus.
#[must_use]
pub fn frozen_productions() -> &'static [&'static str] {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/grammar/productions.rs"
    ))
}
