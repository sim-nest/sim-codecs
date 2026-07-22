//! Algol compatibility wrapper over the shared Pratt parser substrate.

use crate::parse::tokenize_algol_spanned_with_budget;
use sim_codec::{DecodeBudget, DecodeLimits};
use sim_codec_pratt::{PrattCodecParser, PrattTokenSource, SpannedPrattToken};
use sim_kernel::{CodecId, LocatedExprTree, PrattTable, Result};

/// Token source that adapts the Algol lexer to the shared Pratt driver.
#[derive(Clone, Copy, Debug, Default)]
pub struct AlgolTokenSource;

impl PrattTokenSource for AlgolTokenSource {
    fn tokenize_pratt(
        &self,
        _codec: CodecId,
        source: &str,
        budget: &mut DecodeBudget,
    ) -> Result<Vec<SpannedPrattToken>> {
        tokenize_algol_spanned_with_budget(source, budget).map(|tokens| {
            tokens
                .into_iter()
                .map(|token| {
                    SpannedPrattToken::with_leading_trivia(
                        token.token,
                        token.start,
                        token.end,
                        token.leading_trivia,
                    )
                })
                .collect()
        })
    }
}

/// Precedence-climbing parser for the Algol surface.
///
/// This wrapper preserves the public Algol parser API while delegating the
/// language-neutral driver work to [`PrattCodecParser`].
pub struct PrattParser {
    inner: PrattCodecParser<AlgolTokenSource>,
}

impl PrattParser {
    /// Creates a parser driven by the given operator table. Use
    /// [`crate::default_pratt_table`] for the standard arithmetic operators.
    pub fn new(operators: PrattTable) -> Self {
        Self {
            inner: PrattCodecParser::new(operators, AlgolTokenSource).with_surface_name("algol"),
        }
    }

    /// Returns the parser's operator table.
    pub fn operators(&self) -> &PrattTable {
        self.inner.operators()
    }

    /// Parses `source` into a located expression tree under a default decode
    /// budget. `source_id` names the input for origin tracking.
    pub fn parse_text_tree(
        &self,
        codec: CodecId,
        source_id: impl Into<String>,
        source: &str,
    ) -> Result<LocatedExprTree> {
        let mut budget = DecodeBudget::new(DecodeLimits::default());
        self.parse_text_tree_with_budget(codec, source_id, source, &mut budget)
    }

    /// Parses `source` into a located expression tree under an explicit
    /// `budget`, erroring if any tokens remain after a complete expression.
    pub fn parse_text_tree_with_budget(
        &self,
        codec: CodecId,
        source_id: impl Into<String>,
        source: &str,
        budget: &mut DecodeBudget,
    ) -> Result<LocatedExprTree> {
        self.inner.parse_tree_with_source_and_budget(
            codec,
            sim_kernel::SourceId(source_id.into()),
            source,
            budget,
        )
    }
}
