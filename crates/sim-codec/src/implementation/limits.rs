//! Decode resource limits and budgets shared by every codec decode path.
//!
//! Defines `DecodeLimits` (the resource ceilings applied to untrusted input),
//! the per-decode `DecodeBudget` counters, and the `ReadCx` decode context.

use sim_kernel::{CodecId, Cx, Error, ReadPolicy, Result};

/// Resource ceiling shared by every codec decode path.
///
/// These bounds are applied to untrusted input before bulk allocation where
/// possible. They are deliberately generous for normal data and only reject
/// pathological input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeLimits {
    /// Maximum total input size, in bytes.
    pub max_input_bytes: usize,
    /// Maximum number of tokens the decoder may produce.
    pub max_tokens: usize,
    /// Maximum number of `Expr` nodes in the decoded result.
    pub max_expr_nodes: usize,
    /// Maximum nesting depth.
    pub max_depth: usize,
    /// Maximum length of a single decoded string, in bytes.
    pub max_string_bytes: usize,
    /// Maximum length of a single decoded byte blob.
    pub max_blob_bytes: usize,
    /// Maximum length of a single decoded collection (list, vector, map, set).
    pub max_collection_len: usize,
    /// Maximum number of trivia items (comments, whitespace) retained.
    pub max_trivia_items: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 8 * 1024 * 1024,
            max_tokens: 1_000_000,
            max_expr_nodes: 200_000,
            max_depth: 512,
            max_string_bytes: 256 * 1024,
            max_blob_bytes: 8 * 1024 * 1024,
            max_collection_len: 65_536,
            max_trivia_items: 16_384,
        }
    }
}

/// Running counters for a single decode. Construct one per decode call.
pub struct DecodeBudget {
    limits: DecodeLimits,
    nodes: usize,
    trivia: usize,
}

impl DecodeBudget {
    /// Create a fresh budget with zeroed counters for the given `limits`.
    pub fn new(limits: DecodeLimits) -> Self {
        Self {
            limits,
            nodes: 0,
            trivia: 0,
        }
    }

    /// The [`DecodeLimits`] this budget enforces.
    pub fn limits(&self) -> DecodeLimits {
        self.limits
    }

    /// Check input size against [`DecodeLimits::max_input_bytes`].
    pub fn check_input_bytes(&self, codec: CodecId, len: usize) -> Result<()> {
        self.check(codec, "input bytes", len, self.limits.max_input_bytes)
    }

    /// Check token count against [`DecodeLimits::max_tokens`].
    pub fn check_tokens(&self, codec: CodecId, count: usize) -> Result<()> {
        self.check(codec, "tokens", count, self.limits.max_tokens)
    }

    /// Check collection length against [`DecodeLimits::max_collection_len`].
    pub fn check_collection_len(&self, codec: CodecId, len: usize) -> Result<()> {
        self.check(
            codec,
            "collection length",
            len,
            self.limits.max_collection_len,
        )
    }

    /// Check string length against [`DecodeLimits::max_string_bytes`].
    pub fn check_string_bytes(&self, codec: CodecId, len: usize) -> Result<()> {
        self.check(codec, "string bytes", len, self.limits.max_string_bytes)
    }

    /// Check blob length against [`DecodeLimits::max_blob_bytes`].
    pub fn check_blob_bytes(&self, codec: CodecId, len: usize) -> Result<()> {
        self.check(codec, "blob bytes", len, self.limits.max_blob_bytes)
    }

    /// Charge one trivia item and check the running total against
    /// [`DecodeLimits::max_trivia_items`].
    pub fn add_trivia(&mut self, codec: CodecId) -> Result<()> {
        self.trivia += 1;
        self.check(
            codec,
            "trivia items",
            self.trivia,
            self.limits.max_trivia_items,
        )
    }

    /// Charge one `Expr` node and check both the running node count against
    /// [`DecodeLimits::max_expr_nodes`] and `depth` against
    /// [`DecodeLimits::max_depth`].
    pub fn enter_node(&mut self, codec: CodecId, depth: usize) -> Result<()> {
        self.nodes += 1;
        self.check(codec, "expr nodes", self.nodes, self.limits.max_expr_nodes)?;
        self.check(codec, "recursion depth", depth, self.limits.max_depth)
    }

    fn check(&self, codec: CodecId, what: &str, got: usize, max: usize) -> Result<()> {
        if got > max {
            return Err(Error::CodecError {
                codec,
                message: format!("decode {what} limit exceeded: {got} > {max}"),
            });
        }
        Ok(())
    }
}

/// The decode context threaded through every [`Decoder`](crate::Decoder): the
/// kernel context plus the active codec id, read policy, and resource limits.
pub struct ReadCx<'a> {
    /// The kernel context the decode runs against.
    pub cx: &'a mut Cx,
    /// Id of the codec performing the decode (used to tag errors).
    pub codec: CodecId,
    /// The read policy governing what the decode may admit.
    pub read_policy: ReadPolicy,
    /// Resource ceilings applied to this decode.
    pub limits: DecodeLimits,
}
