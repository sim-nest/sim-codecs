//! Pratt parser root, aggregating the `core` driver loop and the `builders`
//! node-construction helpers and re-exporting `PrattParser`.

mod builders;
mod core;

pub use core::PrattParser;
