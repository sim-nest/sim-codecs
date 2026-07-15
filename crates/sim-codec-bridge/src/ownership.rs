use sim_kernel::{Error, Result};

use crate::{BridgeBook, BridgePacket, decode_bridge_text, encode_bridge_text};

/// A rendered byte span owned by one BRIDGE packet component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedSpan {
    /// Structural packet syntax.
    Structural(String),
    /// Fluent frame sentence text.
    Frame {
        /// Frame part id.
        id: String,
        /// Rendered text.
        text: String,
    },
    /// Registered part line.
    Part {
        /// Part id.
        id: String,
        /// Part kind.
        kind: String,
        /// Rendered text.
        text: String,
    },
    /// Fenced data block.
    Fence {
        /// Fence id.
        id: String,
        /// Rendered text.
        text: String,
    },
}

impl OwnedSpan {
    /// Returns the rendered text owned by this span.
    pub fn rendered_text(&self) -> &str {
        match self {
            Self::Structural(text) => text,
            Self::Frame { text, .. } | Self::Part { text, .. } | Self::Fence { text, .. } => text,
        }
    }
}

/// Asserts that encoding then decoding reproduces the same packet.
pub fn assert_roundtrip(packet: &BridgePacket, book: &BridgeBook) -> Result<()> {
    let text = encode_bridge_text(packet, book)?;
    let decoded = decode_bridge_text(&text, book)?;
    if decoded != *packet {
        return Err(Error::Eval("bridge roundtrip mismatch".to_owned()));
    }
    Ok(())
}

/// Asserts that spans exactly cover the full rendered face in order.
pub fn assert_total_ownership(rendered: &str, spans: &[OwnedSpan]) -> Result<()> {
    let mut cursor = 0usize;
    for span in spans {
        let text = span.rendered_text();
        if !rendered[cursor..].starts_with(text) {
            return Err(Error::Eval(format!(
                "bridge render has unowned bytes at {cursor}"
            )));
        }
        cursor += text.len();
    }
    if cursor != rendered.len() {
        return Err(Error::Eval(
            "bridge render has unowned trailing bytes".to_owned(),
        ));
    }
    Ok(())
}
