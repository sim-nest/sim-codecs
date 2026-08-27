# Translate a SearXNG exchange

Encode a checked query as form data and decode caller-supplied JSON bytes. The
codec opens no socket and provider snippets remain unverified claims.

This is a sandbox descriptor because the codec is a typed Rust wire boundary,
not a loadable runtime operation. Crate tests execute the request and response
translation against saved bytes.
