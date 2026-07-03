//! Parse cursor state for the Algol codec: `ParseCx` holds the spanned token
//! stream and offers peek/advance/lookahead operations over it.

use sim_kernel::{Error, Result};

use super::tokenize::SpannedToken;

/// Cursor over a spanned Algol token stream, driving the Pratt parser with
/// peek, advance, and lookahead operations.
pub struct ParseCx {
    tokens: Vec<SpannedToken>,
    index: usize,
}

impl ParseCx {
    /// Creates a cursor positioned at the first of `tokens`.
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Self { tokens, index: 0 }
    }

    /// Returns the current token without consuming it, or `None` at end of
    /// input.
    pub fn peek(&self) -> Option<&SpannedToken> {
        self.tokens.get(self.index)
    }

    /// Consumes and returns the current token, or `None` at end of input.
    pub fn advance(&mut self) -> Option<SpannedToken> {
        let token = self.tokens.get(self.index).cloned()?;
        self.index += 1;
        Some(token)
    }

    /// Consumes the current token, erroring if the stream is exhausted.
    pub fn next_required(&mut self) -> Result<SpannedToken> {
        self.advance()
            .ok_or_else(|| Error::Eval("unexpected end of algol input".to_owned()))
    }

    /// Returns `true` once every token has been consumed.
    pub fn is_empty(&self) -> bool {
        self.index >= self.tokens.len()
    }
}
