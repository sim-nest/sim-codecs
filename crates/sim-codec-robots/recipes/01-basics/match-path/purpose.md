# Match a path

Parse caller-supplied bytes, select the most specific user-agent group, then apply longest-path matching with allow winning equal lengths.

This is a sandbox descriptor because robots parsing and matching are typed Rust
APIs rather than loadable runtime operations. Crate tests execute the saved-file
case and RFC precedence rules.
