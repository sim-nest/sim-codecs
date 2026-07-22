//! Pratt parser root, re-exporting the Algol token source and compatibility
//! `PrattParser` wrapper over the shared Pratt codec parser.

mod core;

pub use core::{AlgolTokenSource, PrattParser};
