//! Pratt parsing root for the Algol codec, aggregating the `parser`
//! (precedence-climbing engine) and `table` (operator definitions) submodules
//! and re-exporting `PrattParser`, `default_pratt_table`, and `supports_pratt`.

mod parser;
mod table;

pub use parser::{AlgolTokenSource, PrattParser};
pub use sim_codec_pratt::{PrattCodecParser, PrattTokenSource, SpannedPrattToken};
pub use table::{default_pratt_table, supports_pratt};
