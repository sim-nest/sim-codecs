//! Fair, uniform encoded-size measurement for the two codecs.

use sim_kernel::Expr;

/// Encoded wire size (bytes) for one value under each codec/mode.
#[derive(Clone, Copy, Debug)]
pub struct Sizes {
    /// `sim-codec-binary` frame bytes.
    pub binary: usize,
    /// `sim-codec-bitwise` plain (canonical) frame bytes.
    pub bitwise: usize,
    /// `sim-codec-bitwise` dense (`Ref` back-reference) frame bytes.
    pub bitwise_dense: usize,
}

/// Measure all three encodings of `expr`.
///
/// Panics only on a genuine encode error (a corpus bug): every corpus value must
/// be encodable by both codecs.
pub fn measure_size(expr: &Expr) -> Sizes {
    Sizes {
        binary: sim_codec_binary::encode_frame(expr)
            .expect("binary encodes")
            .0
            .len(),
        bitwise: sim_codec_bitwise::encode_frame(expr)
            .expect("bitwise encodes")
            .0
            .len(),
        bitwise_dense: sim_codec_bitwise::encode_dense(expr)
            .expect("dense encodes")
            .0
            .len(),
    }
}

#[cfg(test)]
mod tests {
    use crate::corpus::corpus;
    use sim_kernel::CodecId;

    // Every corpus value must round-trip under BOTH codecs, so no size number is
    // ever reported for a lossy encoding.
    #[test]
    fn every_sample_round_trips_under_both_codecs() {
        for s in corpus() {
            let bin = sim_codec_binary::encode_frame(&s.expr)
                .expect("binary encode")
                .0;
            let (_t, back) =
                sim_codec_binary::decode_frame(CodecId(1), &bin).expect("binary decode");
            assert_eq!(back, s.expr, "binary round-trip failed for {}", s.label);

            let bw = sim_codec_bitwise::encode_frame(&s.expr)
                .expect("bitwise encode")
                .0;
            let (_t, back) =
                sim_codec_bitwise::decode_frame(CodecId(1), &bw).expect("bitwise decode");
            assert_eq!(back, s.expr, "bitwise round-trip failed for {}", s.label);

            let dense = sim_codec_bitwise::encode_dense(&s.expr)
                .expect("dense encode")
                .0;
            let (_t, back) =
                sim_codec_bitwise::decode_frame(CodecId(1), &dense).expect("dense decode");
            assert_eq!(back, s.expr, "dense round-trip failed for {}", s.label);
        }
    }
}
