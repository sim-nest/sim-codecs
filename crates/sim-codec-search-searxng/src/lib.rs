//! Pure bounded SearXNG `/config` and JSON `/search` wire translation.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod codec;
pub use codec::*;

#[cfg(test)]
mod tests;
