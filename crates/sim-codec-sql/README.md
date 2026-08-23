# sim-codec-sql

`sim-codec-sql` is the bounded, inspectable SQL projection for admitted SIM
relational plans. Dialects are behavior, identifiers are always quoted by that
behavior, and values are always ordered bindings. Its DDL decoder produces an
untrusted `SchemaDraft`; only `sim-relation-schema` admission can produce a
trusted schema.
