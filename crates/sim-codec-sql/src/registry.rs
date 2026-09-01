/// Codec output/input position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecPosition {
    /// Ordinary inert data.
    Data,
    /// Human/provider display text.
    Display,
}
/// One SQL codec registration descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SqlCodecRegistration {
    /// Stable codec name.
    pub name: &'static str,
    /// Whether decoding is available.
    pub decoder: bool,
    /// Allowed position.
    pub position: CodecPosition,
}
/// Returns the closed SQL codec registrations. Statements are encode-only;
/// bounded DDL is data-decodable and display-encodable.
pub const fn sql_codec_registrations() -> [SqlCodecRegistration; 3] {
    [
        SqlCodecRegistration {
            name: "codec/sql-statement",
            decoder: false,
            position: CodecPosition::Display,
        },
        SqlCodecRegistration {
            name: "codec/sql-ddl",
            decoder: false,
            position: CodecPosition::Display,
        },
        SqlCodecRegistration {
            name: "codec/sql-ddl",
            decoder: true,
            position: CodecPosition::Data,
        },
    ]
}
