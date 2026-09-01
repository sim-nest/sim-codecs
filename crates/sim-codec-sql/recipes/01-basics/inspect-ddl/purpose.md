# Inspect DDL without trusting it

Decode a bounded `CREATE TABLE` statement into a diagnostic `SchemaDraft`.
Parsing does not admit logical domains or produce a trusted schema.
