use crate::{
    Capability, PreparedCacheKey, PreparedSql, SqlBinding, SqlDialect, SqlError, SqlIdentifier,
    StatementRole, StatementSet,
};
use sim_relation_core::{RelationId, RowType};
use sim_relation_migrate::{CheckedProgram, OperationKind};
use sim_relation_plan::{
    Aggregate, CheckedMutation, CheckedQuery, ConflictAction, ConflictTarget, JoinKind, Mutation,
    OrderDirection, Rel, Scalar, ScalarOp, SetOp,
};

struct Emitter<'a> {
    dialect: &'a dyn SqlDialect,
    bindings: Vec<SqlBinding>,
}
impl<'a> Emitter<'a> {
    fn ident(&self, symbol: &sim_kernel::Symbol) -> Result<String, SqlError> {
        Ok(self
            .dialect
            .quote_ident(SqlIdentifier::from_symbol(symbol))?
            .0)
    }
    fn bind(&mut self, binding: SqlBinding) -> String {
        self.bindings.push(binding);
        self.dialect.placeholder(self.bindings.len()).0
    }
    fn scalar(&mut self, value: &Scalar) -> Result<String, SqlError> {
        Ok(match value {
            Scalar::Field(field) => format!(
                "{}.{}",
                self.ident(field.binding.symbol())?,
                self.ident(field.field.symbol())?
            ),
            Scalar::Literal(cell) => self.bind(SqlBinding::Literal(cell.clone())),
            Scalar::Param(name) => self.bind(SqlBinding::Parameter(name.clone())),
            Scalar::Call(op, args) => {
                let parts = args
                    .iter()
                    .map(|v| self.scalar(v))
                    .collect::<Result<Vec<_>, _>>()?;
                match op {
                    ScalarOp::Not => format!("(NOT {})", one(&parts)?),
                    ScalarOp::IsNull => format!("({} IS NULL)", one(&parts)?),
                    ScalarOp::Coalesce => format!("COALESCE({})", parts.join(", ")),
                    _ => format!(
                        "({})",
                        parts.join(match op {
                            ScalarOp::And => " AND ",
                            ScalarOp::Or => " OR ",
                            ScalarOp::Eq => " = ",
                            ScalarOp::Ne => " <> ",
                            ScalarOp::Lt => " < ",
                            ScalarOp::Le => " <= ",
                            ScalarOp::Gt => " > ",
                            ScalarOp::Ge => " >= ",
                            ScalarOp::Add => " + ",
                            ScalarOp::Sub => " - ",
                            ScalarOp::Mul => " * ",
                            ScalarOp::Div => " / ",
                            _ => unreachable!(),
                        })
                    ),
                }
            }
            Scalar::Case {
                branches,
                otherwise,
            } => {
                let mut out = String::from("CASE");
                for (when, then) in branches {
                    out.push_str(&format!(
                        " WHEN {} THEN {}",
                        self.scalar(when)?,
                        self.scalar(then)?
                    ));
                }
                if let Some(value) = otherwise {
                    out.push_str(&format!(" ELSE {}", self.scalar(value)?));
                }
                out.push_str(" END");
                out
            }
            Scalar::Exists(query) => format!("EXISTS ({})", self.rel(query)?),
            Scalar::InQuery { value, query } => {
                format!("({} IN ({}))", self.scalar(value)?, self.rel(query)?)
            }
            Scalar::ScalarQuery(query) => format!("({})", self.rel(query)?),
        })
    }
    fn aggregate(&mut self, aggregate: &Aggregate) -> Result<String, SqlError> {
        Ok(match aggregate {
            Aggregate::CountAll => "COUNT(*)".into(),
            Aggregate::Count(v) => format!("COUNT({})", self.scalar(v)?),
            Aggregate::Sum(v) => format!("SUM({})", self.scalar(v)?),
            Aggregate::Min(v) => format!("MIN({})", self.scalar(v)?),
            Aggregate::Max(v) => format!("MAX({})", self.scalar(v)?),
        })
    }
    fn rel(&mut self, rel: &Rel) -> Result<String, SqlError> {
        Ok(match rel {
            Rel::Scan {
                source,
                table,
                bind,
            } => format!(
                "SELECT * FROM {}.{} AS {}",
                self.ident(source.symbol())?,
                self.ident(table.symbol())?,
                self.ident(bind.symbol())?
            ),
            Rel::Values {
                bind,
                row_type,
                rows,
            } => {
                let mut rendered = Vec::new();
                for row in rows {
                    rendered.push(format!(
                        "({})",
                        row.cells()
                            .iter()
                            .cloned()
                            .map(|c| self.bind(SqlBinding::Literal(c)))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                let columns = row_type
                    .fields()
                    .iter()
                    .map(|f| self.ident(f.name.symbol()))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                format!(
                    "SELECT * FROM (VALUES {}) AS {} ({})",
                    rendered.join(", "),
                    self.ident(bind.symbol())?,
                    columns
                )
            }
            Rel::Project {
                input,
                bind,
                fields,
            } => format!(
                "SELECT {} FROM ({}) AS {}",
                fields
                    .iter()
                    .map(|f| Ok(format!(
                        "{} AS {}",
                        self.scalar(&f.scalar)?,
                        self.ident(f.name.symbol())?
                    )))
                    .collect::<Result<Vec<_>, SqlError>>()?
                    .join(", "),
                self.rel(input)?,
                self.ident(bind.symbol())?
            ),
            Rel::Filter { input, predicate } => format!(
                "SELECT * FROM ({}) AS _filter WHERE {}",
                self.rel(input)?,
                self.scalar(predicate)?
            ),
            Rel::Join {
                left,
                right,
                kind,
                on,
            } => format!(
                "SELECT * FROM ({}) AS _left {} JOIN ({}) AS _right{}",
                self.rel(left)?,
                match kind {
                    JoinKind::Inner => "INNER",
                    JoinKind::Left => "LEFT",
                    JoinKind::Cross => "CROSS",
                },
                self.rel(right)?,
                if *kind == JoinKind::Cross {
                    String::new()
                } else {
                    format!(" ON {}", self.scalar(on)?)
                }
            ),
            Rel::Group {
                input,
                bind,
                keys,
                aggregates,
                having,
            } => {
                let key_sql = keys
                    .iter()
                    .map(|v| self.scalar(&v.scalar))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut fields = keys
                    .iter()
                    .zip(&key_sql)
                    .map(|(v, sql)| Ok(format!("{} AS {}", sql, self.ident(v.name.symbol())?)))
                    .collect::<Result<Vec<_>, SqlError>>()?;
                fields.extend(
                    aggregates
                        .iter()
                        .map(|v| {
                            Ok(format!(
                                "{} AS {}",
                                self.aggregate(&v.aggregate)?,
                                self.ident(v.name.symbol())?
                            ))
                        })
                        .collect::<Result<Vec<_>, SqlError>>()?,
                );
                format!(
                    "SELECT {} FROM ({}) AS {}{}{}",
                    fields.join(", "),
                    self.rel(input)?,
                    self.ident(bind.symbol())?,
                    if key_sql.is_empty() {
                        String::new()
                    } else {
                        format!(" GROUP BY {}", key_sql.join(", "))
                    },
                    having
                        .as_ref()
                        .map(|v| self.scalar(v).map(|v| format!(" HAVING {v}")))
                        .transpose()?
                        .unwrap_or_default()
                )
            }
            Rel::Set { op, inputs } => inputs
                .iter()
                .map(|v| self.rel(v).map(|v| format!("({v})")))
                .collect::<Result<Vec<_>, _>>()?
                .join(match op {
                    SetOp::Union => " UNION ",
                    SetOp::UnionAll => " UNION ALL ",
                    SetOp::Intersect => " INTERSECT ",
                    SetOp::Except => " EXCEPT ",
                }),
            Rel::Distinct(input) => {
                format!("SELECT DISTINCT * FROM ({}) AS _distinct", self.rel(input)?)
            }
            Rel::Order { input, keys } => format!(
                "SELECT * FROM ({}) AS _ordered ORDER BY {}",
                self.rel(input)?,
                keys.iter()
                    .map(|v| Ok(format!(
                        "{} {}",
                        self.scalar(&v.scalar)?,
                        match v.direction {
                            OrderDirection::Asc => "ASC",
                            OrderDirection::Desc => "DESC",
                        }
                    )))
                    .collect::<Result<Vec<_>, SqlError>>()?
                    .join(", ")
            ),
            Rel::Limit {
                input,
                count,
                offset,
            } => format!(
                "SELECT * FROM ({}) AS _limited LIMIT {} OFFSET {}",
                self.rel(input)?,
                count.map_or_else(|| "ALL".into(), |v| v.to_string()),
                offset
            ),
        })
    }
}
fn one(parts: &[String]) -> Result<&str, SqlError> {
    if let [value] = parts {
        Ok(value)
    } else {
        Err(SqlError::InvalidPlan("unary operator arity"))
    }
}

struct PreparedContext<'a> {
    catalog_id: &'a RelationId,
    schema_id: &'a RelationId,
    plan_id: &'a RelationId,
    parameters: &'a RowType,
    output: &'a RowType,
    role: StatementRole,
}
fn prepared(emit: Emitter<'_>, text: String, context: PreparedContext<'_>) -> PreparedSql {
    PreparedSql {
        text,
        bindings: emit.bindings,
        key: PreparedCacheKey {
            catalog_id: context.catalog_id.clone(),
            schema_id: context.schema_id.clone(),
            plan_id: context.plan_id.clone(),
            dialect_id: emit.dialect.id(),
            parameter_row_type: context.parameters.clone(),
            output_row_type: context.output.clone(),
            role: context.role,
        },
    }
}

/// Lowers an admitted query deterministically.
pub fn prepare_query(
    query: &CheckedQuery,
    dialect: &dyn SqlDialect,
) -> Result<PreparedSql, SqlError> {
    let mut emit = Emitter {
        dialect,
        bindings: vec![],
    };
    let text = emit.rel(query.plan())?;
    Ok(prepared(
        emit,
        text,
        PreparedContext {
            catalog_id: query.catalog_id(),
            schema_id: query.schema_id(),
            plan_id: query.plan_id(),
            parameters: query.parameters(),
            output: query.output(),
            role: StatementRole::Query,
        },
    ))
}

/// Lowers an admitted mutation deterministically.
pub fn prepare_mutation(
    value: &CheckedMutation,
    dialect: &dyn SqlDialect,
) -> Result<PreparedSql, SqlError> {
    let mut e = Emitter {
        dialect,
        bindings: vec![],
    };
    let text = match value.plan() {
        Mutation::Insert {
            table,
            columns,
            input,
            conflict,
            returning,
        } => {
            if !matches!(conflict, ConflictAction::Fail) {
                crate::dialect::require(dialect, Capability::Conflict, dialect.caps().conflict)?;
            }
            let conflict = conflict_sql(&mut e, conflict)?;
            let returning = returning_sql(&mut e, returning)?;
            format!(
                "INSERT INTO {} ({}) {}{}{}",
                e.ident(table.symbol())?,
                columns
                    .iter()
                    .map(|v| e.ident(v.symbol()))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", "),
                e.rel(input)?,
                conflict,
                returning
            )
        }
        Mutation::Update {
            table,
            bind,
            assignments,
            predicate,
            returning,
        } => format!(
            "UPDATE {} AS {} SET {}{}{}",
            e.ident(table.symbol())?,
            e.ident(bind.symbol())?,
            assignments
                .iter()
                .map(|(n, v)| Ok(format!("{} = {}", e.ident(n.symbol())?, e.scalar(v)?)))
                .collect::<Result<Vec<_>, SqlError>>()?
                .join(", "),
            predicate
                .as_ref()
                .map(|v| e.scalar(v).map(|v| format!(" WHERE {v}")))
                .transpose()?
                .unwrap_or_default(),
            returning_sql(&mut e, returning)?
        ),
        Mutation::Delete {
            table,
            bind,
            predicate,
            returning,
        } => format!(
            "DELETE FROM {} AS {}{}{}",
            e.ident(table.symbol())?,
            e.ident(bind.symbol())?,
            predicate
                .as_ref()
                .map(|v| e.scalar(v).map(|v| format!(" WHERE {v}")))
                .transpose()?
                .unwrap_or_default(),
            returning_sql(&mut e, returning)?
        ),
    };
    Ok(prepared(
        e,
        text,
        PreparedContext {
            catalog_id: value.catalog_id(),
            schema_id: value.schema_id(),
            plan_id: value.plan_id(),
            parameters: value.parameters(),
            output: value.output(),
            role: StatementRole::Mutation,
        },
    ))
}
fn returning_sql(
    e: &mut Emitter<'_>,
    values: &[sim_relation_plan::NamedScalar],
) -> Result<String, SqlError> {
    if values.is_empty() {
        return Ok(String::new());
    }
    crate::dialect::require(e.dialect, Capability::Returning, e.dialect.caps().returning)?;
    Ok(format!(
        " RETURNING {}",
        values
            .iter()
            .map(|v| Ok(format!(
                "{} AS {}",
                e.scalar(&v.scalar)?,
                e.ident(v.name.symbol())?
            )))
            .collect::<Result<Vec<_>, SqlError>>()?
            .join(", ")
    ))
}
fn conflict_sql(e: &mut Emitter<'_>, value: &ConflictAction) -> Result<String, SqlError> {
    let target = |e: &Emitter<'_>, t: &ConflictTarget| -> Result<String, SqlError> {
        Ok(match t {
            ConflictTarget::PrimaryKey => String::new(),
            ConflictTarget::UniqueConstraint(n) => {
                format!(" ON CONSTRAINT {}", e.ident(n.symbol())?)
            }
            ConflictTarget::Columns(v) => format!(
                " ({})",
                v.iter()
                    .map(|n| e.ident(n.symbol()))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ")
            ),
        })
    };
    Ok(match value {
        ConflictAction::Fail => String::new(),
        ConflictAction::DoNothing { target: t } => {
            format!(" ON CONFLICT{} DO NOTHING", target(e, t)?)
        }
        ConflictAction::DoUpdate {
            target: t,
            assignments,
            predicate,
        } => format!(
            " ON CONFLICT{} DO UPDATE SET {}{}",
            target(e, t)?,
            assignments
                .iter()
                .map(|(n, v)| Ok(format!("{} = {}", e.ident(n.symbol())?, e.scalar(v)?)))
                .collect::<Result<Vec<_>, SqlError>>()?
                .join(", "),
            predicate
                .as_ref()
                .map(|v| e.scalar(v).map(|v| format!(" WHERE {v}")))
                .transpose()?
                .unwrap_or_default()
        ),
    })
}

/// Lowers an admitted migration into ordered, individually prepared statements.
pub fn prepare_migration(
    program: &CheckedProgram,
    catalog_id: &RelationId,
    dialect: &dyn SqlDialect,
) -> Result<StatementSet, SqlError> {
    crate::dialect::require(dialect, Capability::Ddl, dialect.caps().ddl)?;
    let p = program.program();
    let mut statements = vec![];
    let empty = RowType::new([]).map_err(|_| SqlError::InvalidPlan("empty row"))?;
    for revision in &p.revisions {
        for operation in revision.operations() {
            let schema_id = operation.before();
            let plan_id = revision.id();
            if let OperationKind::Backfill(value) = operation.kind() {
                let mut statement = prepare_mutation(value, dialect)?;
                statement.key.catalog_id = catalog_id.clone();
                statement.key.schema_id = schema_id.clone();
                statement.key.plan_id = plan_id.clone();
                statement.key.role = StatementRole::Migration;
                statements.push(statement);
                continue;
            }
            let text = migration_operation(operation.kind(), dialect)?;
            statements.push(PreparedSql {
                text,
                bindings: vec![],
                key: PreparedCacheKey {
                    catalog_id: catalog_id.clone(),
                    schema_id: schema_id.clone(),
                    plan_id: plan_id.clone(),
                    dialect_id: dialect.id(),
                    parameter_row_type: empty.clone(),
                    output_row_type: empty.clone(),
                    role: StatementRole::Migration,
                },
            });
        }
    }
    Ok(StatementSet::new(statements))
}
fn migration_operation(op: &OperationKind, d: &dyn SqlDialect) -> Result<String, SqlError> {
    let q = |s: &sim_kernel::Symbol| d.quote_ident(SqlIdentifier::from_symbol(s)).map(|v| v.0);
    Ok(match op {
        OperationKind::CreateTable(t) => format!(
            "CREATE TABLE {} ({})",
            q(t.name().symbol())?,
            t.columns()
                .iter()
                .map(|c| Ok(format!(
                    "{} BLOB{}",
                    q(c.name().symbol())?,
                    if c.nullable() { "" } else { " NOT NULL" }
                )))
                .collect::<Result<Vec<_>, SqlError>>()?
                .join(", ")
        ),
        OperationKind::DropTable(t) => format!("DROP TABLE {}", q(t.symbol())?),
        OperationKind::RenameTable { from, to } => format!(
            "ALTER TABLE {} RENAME TO {}",
            q(from.symbol())?,
            q(to.symbol())?
        ),
        OperationKind::AddColumn { table, column } => format!(
            "ALTER TABLE {} ADD COLUMN {} BLOB{}",
            q(table.symbol())?,
            q(column.name().symbol())?,
            if column.nullable() { "" } else { " NOT NULL" }
        ),
        OperationKind::DropColumn { table, column } => format!(
            "ALTER TABLE {} DROP COLUMN {}",
            q(table.symbol())?,
            q(column.symbol())?
        ),
        OperationKind::RenameColumn { table, from, to } => format!(
            "ALTER TABLE {} RENAME COLUMN {} TO {}",
            q(table.symbol())?,
            q(from.symbol())?,
            q(to.symbol())?
        ),
        OperationKind::AlterColumn { .. }
        | OperationKind::AddConstraint { .. }
        | OperationKind::DropConstraint { .. } => {
            return Err(SqlError::InvalidPlan(
                "dialect-specific table rebuild required",
            ));
        }
        OperationKind::AddIndex { table, index } => format!(
            "CREATE {}INDEX {} ON {} ({})",
            if index.unique { "UNIQUE " } else { "" },
            q(index.name.symbol())?,
            q(table.symbol())?,
            index
                .columns
                .iter()
                .map(|v| q(v.symbol()))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        ),
        OperationKind::DropIndex { index, .. } => format!("DROP INDEX {}", q(index.symbol())?),
        OperationKind::Backfill(_) => {
            unreachable!("backfills retain bindings in prepare_migration")
        }
    })
}
