# Lua decode roundtrip descriptor

This records the Lua codec case where a chunk is decoded through the plain,
located, and tree lanes while preserving the same root form and source identity.
The crate's test suite checks the parser and encoder behavior.
