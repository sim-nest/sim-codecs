//! Frame value types and wire constants.
//!
//! Defines the wire version and flag constants, the 6-bit [`BitwiseTag`]
//! alphabet, the [`BitwiseFrame`] and [`FrameTables`] carriers, and the
//! [`DecodeLimits`] bounds that keep decoding fail-closed.

use sim_codec::DecodeLimits as SharedDecodeLimits;
use sim_kernel::Symbol;

/// The current wire-format version, carried as the first `vbits` of a frame.
pub(crate) const VERSION: u128 = 1;
/// No optional frame sections are present.
pub(crate) const FLAG_NONE: u128 = 0;
/// The frame carries a single top-level origin (located form).
pub(crate) const FLAG_ORIGIN: u128 = 1;
/// The frame carries a per-node origin tree (tree form).
pub(crate) const FLAG_TREE_ORIGIN: u128 = 2;
/// The frame body uses dense mode: a repeated, value-equal subtree is written
/// once and later occurrences are back-references ([`BitwiseTag::Ref`]).
///
/// Dense mode is opt-in (see [`crate::encode_dense`]); the plain body, and
/// therefore [`crate::canonical_bytes`], never sets this bit and never emits a
/// `Ref`.
pub(crate) const FLAG_DENSE: u128 = 4;
/// The set of flag bits this version understands; any other bit is rejected.
pub(crate) const FLAG_KNOWN: u128 = FLAG_ORIGIN | FLAG_TREE_ORIGIN | FLAG_DENSE;

/// Fail-closed bounds applied while decoding an untrusted bitwise frame.
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

/// A complete encoded bitwise frame: the self-delimiting header, side tables,
/// and the bit-packed `Expr` body, owned as a single byte buffer.
///
/// The final carrier byte is zero-padded in its unused low bits; decode rejects
/// any nonzero trailing bit and any trailing whole byte, so the plain-mode
/// buffer is the smallest canonical byte string for its `Expr` value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitwiseFrame(
    /// The raw frame bytes, ready to be written to a byte transport.
    pub Vec<u8>,
);

/// The interning side tables carried in a frame header.
///
/// Symbols, number-domain symbols, and their namespaces are interned once in
/// these tables and referenced by index from the body, keeping the frame
/// compact. Decoding reconstructs the same tables and validates every body
/// index against them. The collection order matches `sim-codec-binary` so the
/// two codecs agree on interning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameTables {
    /// Interned namespace (lib) strings, referenced by index from symbols.
    pub libs: Vec<String>,
    /// Interned symbols referenced by index from the body.
    pub symbols: Vec<Symbol>,
    /// Interned number-domain symbols referenced by number-body nodes.
    pub number_domains: Vec<Symbol>,
}

/// The fixed-width 6-bit tag that prefixes each node in a frame body.
///
/// The first sixteen variants are inline small unsigned integer literals
/// carrying only a domain index; the next twenty are the structural `Expr`
/// kinds (one generic tag each -- lengths ride `vbits`); [`BitwiseTag::Ref`] is
/// emitted only in dense mode (a back-reference to an earlier subtree). Raw
/// values `37..=63` are reserved and rejected on decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum BitwiseTag {
    UInt0 = 0,
    UInt1 = 1,
    UInt2 = 2,
    UInt3 = 3,
    UInt4 = 4,
    UInt5 = 5,
    UInt6 = 6,
    UInt7 = 7,
    UInt8 = 8,
    UInt9 = 9,
    UInt10 = 10,
    UInt11 = 11,
    UInt12 = 12,
    UInt13 = 13,
    UInt14 = 14,
    UInt15 = 15,
    Nil = 16,
    False = 17,
    True = 18,
    Number = 19,
    Symbol = 20,
    Local = 21,
    String = 22,
    Bytes = 23,
    List = 24,
    Vector = 25,
    Map = 26,
    Set = 27,
    Call = 28,
    Infix = 29,
    Prefix = 30,
    Postfix = 31,
    Block = 32,
    Quote = 33,
    Annotated = 34,
    Extension = 35,
    /// Dense-mode back-reference to an earlier subtree. The plain codec never
    /// emits it; only [`crate::encode_dense`] does.
    Ref = 36,
}

impl BitwiseTag {
    /// The fixed on-wire width of a tag, in bits.
    pub(crate) const WIDTH_BITS: usize = 6;

    /// Maps a raw 6-bit value to a tag, or `None` for a reserved/invalid slot.
    ///
    /// Written as an exhaustive `match` (the crate forbids `unsafe`), so no
    /// out-of-range or reserved value can ever transmute into a tag.
    pub(crate) fn from_u6(value: u8) -> Option<Self> {
        let tag = match value {
            0 => Self::UInt0,
            1 => Self::UInt1,
            2 => Self::UInt2,
            3 => Self::UInt3,
            4 => Self::UInt4,
            5 => Self::UInt5,
            6 => Self::UInt6,
            7 => Self::UInt7,
            8 => Self::UInt8,
            9 => Self::UInt9,
            10 => Self::UInt10,
            11 => Self::UInt11,
            12 => Self::UInt12,
            13 => Self::UInt13,
            14 => Self::UInt14,
            15 => Self::UInt15,
            16 => Self::Nil,
            17 => Self::False,
            18 => Self::True,
            19 => Self::Number,
            20 => Self::Symbol,
            21 => Self::Local,
            22 => Self::String,
            23 => Self::Bytes,
            24 => Self::List,
            25 => Self::Vector,
            26 => Self::Map,
            27 => Self::Set,
            28 => Self::Call,
            29 => Self::Infix,
            30 => Self::Prefix,
            31 => Self::Postfix,
            32 => Self::Block,
            33 => Self::Quote,
            34 => Self::Annotated,
            35 => Self::Extension,
            36 => Self::Ref,
            _ => return None,
        };
        Some(tag)
    }

    /// The inline literal value `0..=15` for a `UInt*` tag, else `None`.
    pub(crate) fn small_uint(self) -> Option<u8> {
        let value = self as u8;
        (value < 16).then_some(value)
    }
}
