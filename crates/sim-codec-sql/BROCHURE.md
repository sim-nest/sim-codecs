# Safe SQL without making SQL the execution model

Project admitted relational queries, mutations, and migrations to SQLite or
PostgreSQL with deterministic text, explicit dialect capabilities, sealed
fragments, ordered bindings, and cache-complete identities. Inspect bounded DDL
as an untrusted draft without turning arbitrary statement text back into plans.
