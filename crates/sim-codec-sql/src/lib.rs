//! Bounded SQL projection for admitted relational plans and migrations.
//!
//! SQL is an output codec, never the provider-neutral execution IR. Callers
//! supply opaque admitted values and a behavior-backed [`SqlDialect`]. Every
//! identifier crosses [`SqlDialect::quote_ident`], while every literal or
//! parameter becomes an ordered [`SqlBinding`]. Neither checked SQL fragments
//! nor prepared statements have a public arbitrary-text constructor.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod ddl;
mod dialect;
mod emit;
mod prepared;
mod registry;

pub use ddl::{DdlCodec, DdlDiagnostic, DraftColumn, DraftTable, LegacyDdl, SchemaDraft};
pub use dialect::{
    Capability, CheckedSqlFragment, DialectCaps, PostgreSqlDialect, SqlDialect, SqlError,
    SqlIdentifier, SqliteDialect,
};
pub use emit::{prepare_migration, prepare_mutation, prepare_query};
pub use prepared::{PreparedCacheKey, PreparedSql, SqlBinding, StatementRole, StatementSet};
pub use registry::{CodecPosition, SqlCodecRegistration, sql_codec_registrations};

#[cfg(test)]
mod tests;
