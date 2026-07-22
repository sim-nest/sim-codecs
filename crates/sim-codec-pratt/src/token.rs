use sim_codec::DecodeBudget;
use sim_kernel::{CodecId, PrattToken, Result, Trivia};

/// A Pratt token with its source span, produced by a language-specific lexer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpannedPrattToken {
    /// The scanned Pratt token.
    pub token: PrattToken,
    /// Byte offset where the token starts in the source.
    pub start: usize,
    /// Byte offset just past the end of the token.
    pub end: usize,
    /// Whitespace and comment trivia immediately preceding the token.
    pub leading_trivia: Vec<Trivia>,
}

impl SpannedPrattToken {
    /// Builds a token span with no attached trivia.
    pub fn new(token: PrattToken, start: usize, end: usize) -> Self {
        Self {
            token,
            start,
            end,
            leading_trivia: Vec::new(),
        }
    }

    /// Builds a token span with explicit leading trivia.
    pub fn with_leading_trivia(
        token: PrattToken,
        start: usize,
        end: usize,
        leading_trivia: Vec<Trivia>,
    ) -> Self {
        Self {
            token,
            start,
            end,
            leading_trivia,
        }
    }
}

/// A lexer that turns source text into Pratt tokens for the shared driver.
pub trait PrattTokenSource: Send + Sync {
    /// Tokenizes `source` under `budget`, using `codec` for budget diagnostics.
    fn tokenize_pratt(
        &self,
        codec: CodecId,
        source: &str,
        budget: &mut DecodeBudget,
    ) -> Result<Vec<SpannedPrattToken>>;
}
