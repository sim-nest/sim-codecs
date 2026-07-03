//! Frame value types and wire constants.
//!
//! Defines the magic/version and flag constants, the `BinaryTag` body tags,
//! the `BinaryFrame` and `FrameTables` carriers, and the `DecodeLimits` bounds
//! that keep decoding fail-closed.

use sim_codec::DecodeLimits as SharedDecodeLimits;
use sim_kernel::Symbol;

pub(crate) const MAGIC: &[u8; 4] = b"SLB8";
pub(crate) const VERSION: u64 = 1;
pub(crate) const FLAG_NONE: u64 = 0;
pub(crate) const FLAG_ORIGIN: u64 = 1;
pub(crate) const FLAG_TREE_ORIGIN: u64 = 2;

/// Fail-closed bounds applied while decoding an untrusted binary frame.
///
/// Every count and length read from a frame is checked against these limits
/// before any allocation, so a malformed or hostile frame cannot exhaust
/// memory or recurse without bound. Construct via [`DecodeLimits::default`] or
/// from the shared [`sim_codec::DecodeLimits`] (see the [`From`] impl).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeLimits {
    /// Maximum total size, in bytes, of the frame accepted by the reader.
    pub max_frame_bytes: usize,
    /// Maximum length, in bytes, of any single decoded string.
    pub max_string_bytes: usize,
    /// Maximum length, in bytes, of any single decoded byte blob.
    pub max_blob_bytes: usize,
    /// Maximum number of entries in any side table or collection.
    pub max_table_entries: usize,
    /// Maximum number of `Expr` nodes decoded from one frame.
    pub max_expr_nodes: usize,
    /// Maximum nesting depth of the decoded `Expr` graph.
    pub max_depth: usize,
    /// Maximum number of trivia items carried by a single origin.
    pub max_trivia_items: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        SharedDecodeLimits::default().into()
    }
}

impl From<SharedDecodeLimits> for DecodeLimits {
    fn from(shared: SharedDecodeLimits) -> Self {
        Self {
            max_frame_bytes: shared.max_input_bytes,
            max_string_bytes: shared.max_string_bytes,
            max_blob_bytes: shared.max_blob_bytes,
            max_table_entries: shared.max_collection_len,
            max_expr_nodes: shared.max_expr_nodes,
            max_depth: shared.max_depth,
            max_trivia_items: shared.max_trivia_items,
        }
    }
}

/// A complete encoded binary frame: a magic/version header, side tables, and
/// the tag-prefixed `Expr` body, owned as a single byte buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryFrame(
    /// The raw frame bytes, ready to be written to a byte transport.
    pub Vec<u8>,
);

/// The interning side tables carried in a frame header.
///
/// Symbols, number-domain symbols, and their namespaces are interned once in
/// these tables and referenced by index from the body, keeping the frame
/// compact. Decoding reconstructs the same tables and validates every body
/// index against them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameTables {
    /// Interned namespace (lib) strings, referenced by index from symbols.
    pub libs: Vec<String>,
    /// Interned symbols referenced by index from the body.
    pub symbols: Vec<Symbol>,
    /// Interned number-domain symbols referenced by [`crate::BinaryTag::Number`].
    pub number_domains: Vec<Symbol>,
}

/// The one-byte tag that prefixes each node in a frame body.
///
/// Each variant selects the `Expr` shape that follows and fixes its wire
/// layout. The discriminant is the on-wire byte; decoding rejects any byte
/// without a matching variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BinaryTag {
    /// The nil value; no payload.
    Nil = 0x00,
    /// The boolean `false`; no payload.
    False = 0x01,
    /// The boolean `true`; no payload.
    True = 0x02,
    /// A number: a number-domain table index then the canonical text.
    Number = 0x03,
    /// A symbol, given as an index into the symbol table.
    Symbol = 0x04,
    /// A UTF-8 string, length-prefixed.
    String = 0x05,
    /// A raw byte blob, length-prefixed.
    Bytes = 0x06,
    /// A list: a count then that many body nodes.
    List = 0x07,
    /// A vector: a count then that many body nodes.
    Vector = 0x08,
    /// A map: a count then that many canonically ordered key/value node pairs.
    Map = 0x09,
    /// A set: a count then that many canonically ordered body nodes.
    Set = 0x0a,
    /// A call: an operator node, a count, then that many argument nodes.
    Call = 0x0b,
    /// An infix application: an operator symbol index then left and right nodes.
    Infix = 0x0c,
    /// A prefix application: an operator symbol index then the argument node.
    Prefix = 0x0d,
    /// A postfix application: an operator symbol index then the argument node.
    Postfix = 0x0e,
    /// A block: a count then that many body nodes.
    Block = 0x0f,
    /// A quote: a quote-mode byte then the quoted node.
    Quote = 0x10,
    /// An annotated node: the inner node then a count of symbol/value pairs.
    Annotated = 0x11,
    /// An extension: a tag symbol index then the payload node.
    Extension = 0x12,
    /// A local binding reference, given as an index into the symbol table.
    Local = 0x13,
}

impl BinaryTag {
    pub(crate) fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Self::Nil),
            0x01 => Some(Self::False),
            0x02 => Some(Self::True),
            0x03 => Some(Self::Number),
            0x04 => Some(Self::Symbol),
            0x05 => Some(Self::String),
            0x06 => Some(Self::Bytes),
            0x07 => Some(Self::List),
            0x08 => Some(Self::Vector),
            0x09 => Some(Self::Map),
            0x0a => Some(Self::Set),
            0x0b => Some(Self::Call),
            0x0c => Some(Self::Infix),
            0x0d => Some(Self::Prefix),
            0x0e => Some(Self::Postfix),
            0x0f => Some(Self::Block),
            0x10 => Some(Self::Quote),
            0x11 => Some(Self::Annotated),
            0x12 => Some(Self::Extension),
            0x13 => Some(Self::Local),
            _ => None,
        }
    }
}
