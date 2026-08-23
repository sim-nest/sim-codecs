use sim_kernel::Symbol;
use sim_relation_core::{Cell, ParameterName, RelationId, RowType};

/// Purpose of a prepared artifact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatementRole {
    /// Row-producing query.
    Query,
    /// Data mutation.
    Mutation,
    /// Schema migration statement.
    Migration,
    /// Bounded DDL display/data projection.
    Ddl,
}

/// One ordered placeholder source. Values are never rendered into SQL text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlBinding {
    /// A typed literal cell.
    Literal(Cell),
    /// A caller-supplied named parameter.
    Parameter(ParameterName),
}

/// Cache identity containing every semantic input to preparation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedCacheKey {
    /// Domain catalog identity.
    pub catalog_id: RelationId,
    /// Schema identity.
    pub schema_id: RelationId,
    /// Plan or migration identity.
    pub plan_id: RelationId,
    /// Dialect behavior identity.
    pub dialect_id: Symbol,
    /// Ordered input row contract.
    pub parameter_row_type: RowType,
    /// Output row contract.
    pub output_row_type: RowType,
    /// Statement purpose.
    pub role: StatementRole,
}

/// Sealed prepared SQL artifact. Only lowering in this crate constructs it.
#[derive(Clone, Debug)]
pub struct PreparedSql {
    pub(crate) text: String,
    pub(crate) bindings: Vec<SqlBinding>,
    pub(crate) key: PreparedCacheKey,
}
impl PreparedSql {
    /// Checked SQL text for display or provider execution.
    pub fn text(&self) -> &str {
        &self.text
    }
    /// Ordered placeholder sources.
    pub fn bindings(&self) -> &[SqlBinding] {
        &self.bindings
    }
    /// Complete cache identity.
    pub fn cache_key(&self) -> &PreparedCacheKey {
        &self.key
    }
    /// Statement role.
    pub fn role(&self) -> StatementRole {
        self.key.role
    }
}

/// Ordered statements comprising one checked migration program.
#[derive(Clone, Debug)]
pub struct StatementSet(Vec<PreparedSql>);
impl StatementSet {
    /// Inspects statements in execution order.
    pub fn statements(&self) -> &[PreparedSql] {
        &self.0
    }
    pub(crate) fn new(statements: Vec<PreparedSql>) -> Self {
        Self(statements)
    }
}
