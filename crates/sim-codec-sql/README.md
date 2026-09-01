# sim-codec-sql

`sim-codec-sql` is the bounded, inspectable SQL projection for admitted SIM
relational plans. Dialects are behavior, identifiers are always quoted by that
behavior, and values are always ordered bindings. Its DDL decoder produces an
untrusted `SchemaDraft`; only product admission against an explicit domain
catalog can produce trusted metadata. Its HSQLDB dialect is deliberately
limited to `CREATE [CACHED|MEMORY] TABLE`, inline or table primary keys,
`ALTER COLUMN ... RESTART WITH`, and `SET TABLE ... INDEX` cache roots. Every
other statement is a named refusal and is never executed or retained as SQL.
