use sim_kernel::Symbol;
use std::{error::Error, fmt};

/// Inspectable SQL behavior capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialectCaps {
    /// Supports mutation `RETURNING`.
    pub returning: bool,
    /// Supports conflict clauses.
    pub conflict: bool,
    /// Supports the codec's bounded DDL family.
    pub ddl: bool,
    /// Supports database attachment.
    pub attach: bool,
    /// Supports deferred transactions.
    pub transaction_deferred: bool,
    /// Supports immediate transactions.
    pub transaction_immediate: bool,
    /// Supports serializable transactions.
    pub transaction_serializable: bool,
}

/// Capability named in a fail-closed diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// Mutation returning rows.
    Returning,
    /// Conflict handling.
    Conflict,
    /// DDL.
    Ddl,
    /// Database attachment.
    Attach,
    /// Deferred transactions.
    DeferredTransaction,
    /// Immediate transactions.
    ImmediateTransaction,
    /// Serializable transactions.
    SerializableTransaction,
}

/// SQL projection failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlError {
    /// The selected behavior does not implement a required form.
    Unsupported {
        /// Selected dialect id.
        dialect: Symbol,
        /// Required unavailable capability.
        capability: Capability,
    },
    /// A plan invariant was violated after admission.
    InvalidPlan(&'static str),
    /// DDL is outside the bounded decoder domain.
    Ddl(String),
}
impl fmt::Display for SqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl Error for SqlError {}

/// An identifier passed to dialect behavior rather than interpolated by callers.
#[derive(Clone, Copy, Debug)]
pub struct SqlIdentifier<'a>(&'a Symbol);
impl<'a> SqlIdentifier<'a> {
    /// Wraps a validated relational name's symbol for dialect quoting.
    pub const fn from_symbol(symbol: &'a Symbol) -> Self {
        Self(symbol)
    }
    /// Returns the logical symbol.
    pub const fn symbol(self) -> &'a Symbol {
        self.0
    }
}

/// A fragment minted only by this crate's dialect behavior and emitter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedSqlFragment(pub(crate) String);
impl CheckedSqlFragment {
    /// Inspects the checked text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Executable SQL syntax behavior. Implementations cannot manufacture checked
/// fragments because the fragment field is private to this crate.
pub trait SqlDialect: Send + Sync {
    /// Open stable dialect id.
    fn id(&self) -> Symbol;
    /// Inspectable capabilities.
    fn caps(&self) -> DialectCaps;
    /// Quotes one logical identifier as a single SQL identifier.
    fn quote_ident(&self, name: SqlIdentifier<'_>) -> Result<CheckedSqlFragment, SqlError>;
    /// Emits a binding placeholder for a one-based ordinal.
    fn placeholder(&self, ordinal: usize) -> CheckedSqlFragment;
}

fn quoted(symbol: &Symbol) -> CheckedSqlFragment {
    let logical = match &symbol.namespace {
        Some(ns) => format!("{ns}/{}", symbol.name),
        None => symbol.name.to_string(),
    };
    CheckedSqlFragment(format!("\"{}\"", logical.replace('"', "\"\"")))
}

/// SQLite SQL behavior.
#[derive(Clone, Copy, Debug, Default)]
pub struct SqliteDialect;
impl SqlDialect for SqliteDialect {
    fn id(&self) -> Symbol {
        Symbol::qualified("sql-dialect", "sqlite")
    }
    fn caps(&self) -> DialectCaps {
        DialectCaps {
            returning: true,
            conflict: true,
            ddl: true,
            attach: true,
            transaction_deferred: true,
            transaction_immediate: true,
            transaction_serializable: false,
        }
    }
    fn quote_ident(&self, name: SqlIdentifier<'_>) -> Result<CheckedSqlFragment, SqlError> {
        Ok(quoted(name.symbol()))
    }
    fn placeholder(&self, ordinal: usize) -> CheckedSqlFragment {
        CheckedSqlFragment(format!("?{ordinal}"))
    }
}

/// Compile-only PostgreSQL SQL behavior.
#[derive(Clone, Copy, Debug, Default)]
pub struct PostgreSqlDialect;
impl SqlDialect for PostgreSqlDialect {
    fn id(&self) -> Symbol {
        Symbol::qualified("sql-dialect", "postgresql")
    }
    fn caps(&self) -> DialectCaps {
        DialectCaps {
            returning: true,
            conflict: true,
            ddl: true,
            attach: false,
            transaction_deferred: false,
            transaction_immediate: false,
            transaction_serializable: true,
        }
    }
    fn quote_ident(&self, name: SqlIdentifier<'_>) -> Result<CheckedSqlFragment, SqlError> {
        Ok(quoted(name.symbol()))
    }
    fn placeholder(&self, ordinal: usize) -> CheckedSqlFragment {
        CheckedSqlFragment(format!("${ordinal}"))
    }
}

pub(crate) fn require(
    dialect: &dyn SqlDialect,
    capability: Capability,
    available: bool,
) -> Result<(), SqlError> {
    if available {
        Ok(())
    } else {
        Err(SqlError::Unsupported {
            dialect: dialect.id(),
            capability,
        })
    }
}
