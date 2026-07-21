//! Error type for the index codec.

use std::{error::Error, fmt};

use sim_index_core::IndexError;

/// Failure while decoding, checking, or encoding an index document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodecError {
    /// Text failed before it could become an index expression.
    Decode(String),
    /// The expression did not match the index codec grammar.
    Shape(String),
    /// The expression matched the grammar but failed graph validation.
    Index(IndexError),
    /// The checked graph could not be rendered.
    Encode(String),
}

impl From<IndexError> for CodecError {
    fn from(error: IndexError) -> Self {
        Self::Index(error)
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(message) => write!(f, "index decode failed: {message}"),
            Self::Shape(message) => write!(f, "index shape check failed: {message}"),
            Self::Index(error) => write!(f, "index validation failed: {error}"),
            Self::Encode(message) => write!(f, "index encode failed: {message}"),
        }
    }
}

impl Error for CodecError {}

pub(crate) fn kernel_codec_error(
    codec: sim_kernel::CodecId,
    error: CodecError,
) -> sim_kernel::Error {
    sim_kernel::Error::CodecError {
        codec,
        message: error.to_string(),
    }
}
