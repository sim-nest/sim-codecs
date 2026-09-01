# Decode saved feed bytes

The feed codec admits bounded bytes and returns inert feed records. Referenced URLs remain claims.

This is a sandbox descriptor because feed decoding is a typed Rust API rather
than a loadable runtime operation. Crate tests execute RSS and JSON Feed
specimens with explicit limits.
