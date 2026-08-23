use crate::SqlError;

/// Legacy grammar selected for bounded DDL lifting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyDdl {
    /// SQLite `CREATE TABLE` forms.
    Sqlite,
    /// HSQLDB 1.8 `CREATE ... TABLE` forms.
    Hsqldb,
}

/// One parsed, untrusted column declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftColumn {
    /// Identifier after SQL unquoting.
    pub name: String,
    /// Retained normalized storage type spelling.
    pub storage_type: String,
    /// Whether the declaration permits NULL.
    pub nullable: bool,
}
/// One parsed, untrusted table declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftTable {
    /// Identifier after SQL unquoting.
    pub name: String,
    /// Declared columns in source order.
    pub columns: Vec<DraftColumn>,
}
/// Diagnostic attached to a bounded DDL draft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DdlDiagnostic {
    /// Byte offset at or before the problem.
    pub offset: usize,
    /// Stable human-readable explanation.
    pub message: String,
}
/// Parsed DDL candidate. This type deliberately has no conversion to trusted
/// `sim_relation_schema::Schema`; callers must perform domain-aware admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaDraft {
    /// Parsed table declarations.
    pub tables: Vec<DraftTable>,
    /// Non-fatal source diagnostics.
    pub diagnostics: Vec<DdlDiagnostic>,
    /// Grammar that admitted the source.
    pub grammar: LegacyDdl,
}

/// Bounded DDL data codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct DdlCodec;
impl DdlCodec {
    /// Decodes exactly the emitted/SQLite or HSQLDB legacy `CREATE TABLE`
    /// subset. Any other statement fails closed.
    pub fn decode(&self, source: &str, grammar: LegacyDdl) -> Result<SchemaDraft, SqlError> {
        if source.len() > 1_048_576 {
            return Err(SqlError::Ddl("DDL byte budget exceeded".into()));
        }
        let mut tables = vec![];
        for raw in split_statements(source)? {
            let statement = raw.trim();
            if statement.is_empty() {
                continue;
            }
            let upper = statement.to_ascii_uppercase();
            let prefix = match grammar {
                LegacyDdl::Sqlite if upper.starts_with("CREATE TABLE ") => "CREATE TABLE ",
                LegacyDdl::Sqlite if upper.starts_with("CREATE TEMP TABLE ") => {
                    "CREATE TEMP TABLE "
                }
                LegacyDdl::Hsqldb if upper.starts_with("CREATE CACHED TABLE ") => {
                    "CREATE CACHED TABLE "
                }
                LegacyDdl::Hsqldb if upper.starts_with("CREATE MEMORY TABLE ") => {
                    "CREATE MEMORY TABLE "
                }
                LegacyDdl::Hsqldb if upper.starts_with("CREATE TABLE ") => "CREATE TABLE ",
                _ => {
                    return Err(SqlError::Ddl(
                        "statement is outside bounded CREATE TABLE domain".into(),
                    ));
                }
            };
            let body = statement
                .get(prefix.len()..)
                .ok_or_else(|| SqlError::Ddl("missing table body".into()))?
                .trim();
            let open = find_unquoted(body, '(')
                .ok_or_else(|| SqlError::Ddl("missing column list".into()))?;
            if !body.ends_with(')') {
                return Err(SqlError::Ddl("trailing text after column list".into()));
            }
            let name = unquote(body[..open].trim())?;
            let mut columns = vec![];
            let mut diagnostics = vec![];
            for item in split_commas(&body[open + 1..body.len() - 1])? {
                let words = words(&item)?;
                if words.is_empty() {
                    return Err(SqlError::Ddl("empty table item".into()));
                }
                let first = words[0].to_ascii_uppercase();
                if matches!(
                    first.as_str(),
                    "PRIMARY" | "UNIQUE" | "CONSTRAINT" | "FOREIGN" | "CHECK"
                ) {
                    diagnostics.push(DdlDiagnostic {
                        offset: source.find(&item).unwrap_or(0),
                        message: format!("retained table constraint: {item}"),
                    });
                    continue;
                }
                if words.len() < 2 {
                    return Err(SqlError::Ddl("column requires a storage type".into()));
                }
                let storage_type = words[1].to_ascii_uppercase();
                if !storage_type
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '(' | ')' | ','))
                {
                    return Err(SqlError::Ddl("unsupported storage type spelling".into()));
                }
                columns.push(DraftColumn {
                    name: unquote(&words[0])?,
                    storage_type,
                    nullable: !item.to_ascii_uppercase().contains("NOT NULL"),
                });
            }
            if columns.is_empty() {
                return Err(SqlError::Ddl("table has no decodable columns".into()));
            }
            tables.push((DraftTable { name, columns }, diagnostics));
        }
        if tables.is_empty() {
            return Err(SqlError::Ddl("DDL contains no tables".into()));
        }
        let diagnostics = tables
            .iter_mut()
            .flat_map(|(_, d)| std::mem::take(d))
            .collect();
        Ok(SchemaDraft {
            tables: tables.into_iter().map(|(t, _)| t).collect(),
            diagnostics,
            grammar,
        })
    }
    /// Encodes a draft to the canonical bounded DDL subset. Decoding this text
    /// returns the same table and column data.
    pub fn encode(&self, draft: &SchemaDraft) -> Result<String, SqlError> {
        if draft.tables.is_empty() {
            return Err(SqlError::Ddl("draft contains no tables".into()));
        }
        draft
            .tables
            .iter()
            .map(|table| {
                if table.columns.is_empty() {
                    return Err(SqlError::Ddl("draft table contains no columns".into()));
                }
                Ok(format!(
                    "CREATE TABLE {} ({});",
                    quote(&table.name),
                    table
                        .columns
                        .iter()
                        .map(|c| format!(
                            "{} {}{}",
                            quote(&c.name),
                            c.storage_type,
                            if c.nullable { "" } else { " NOT NULL" }
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join("\n"))
    }
}
fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
fn unquote(value: &str) -> Result<String, SqlError> {
    let value = value.trim();
    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        Ok(value[1..value.len() - 1].replace("\"\"", "\""))
    } else if value.starts_with('[') && value.ends_with(']') {
        Ok(value[1..value.len() - 1].replace("]]", "]"))
    } else if !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(value.to_owned())
    } else {
        Err(SqlError::Ddl("invalid identifier".into()))
    }
}
fn split_statements(source: &str) -> Result<Vec<String>, SqlError> {
    split_delimited(source, ';')
}
fn split_commas(source: &str) -> Result<Vec<String>, SqlError> {
    split_delimited(source, ',')
}
fn split_delimited(source: &str, delimiter: char) -> Result<Vec<String>, SqlError> {
    let mut out = vec![];
    let mut start = 0;
    let mut quote = false;
    let mut depth = 0;
    let chars: Vec<_> = source.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (offset, c) = chars[i];
        if c == '\'' {
            return Err(SqlError::Ddl(
                "string literals are outside bounded DDL".into(),
            ));
        }
        if c == '"' {
            if quote && chars.get(i + 1).is_some_and(|(_, n)| *n == '"') {
                i += 2;
                continue;
            }
            quote = !quote;
        } else if !quote {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                if depth == 0 {
                    return Err(SqlError::Ddl("unbalanced parenthesis".into()));
                }
                depth -= 1;
            } else if c == delimiter && depth == 0 {
                out.push(source[start..offset].to_owned());
                start = offset + c.len_utf8();
            }
        }
        i += 1;
    }
    if quote || depth != 0 {
        return Err(SqlError::Ddl(
            "unterminated quoted identifier or parenthesis".into(),
        ));
    }
    out.push(source[start..].to_owned());
    Ok(out)
}
fn find_unquoted(source: &str, needle: char) -> Option<usize> {
    let mut quoted = false;
    for (i, c) in source.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if !quoted && c == needle {
            return Some(i);
        }
    }
    None
}
fn words(source: &str) -> Result<Vec<String>, SqlError> {
    let mut out = vec![];
    let mut current = String::new();
    let mut quoted = false;
    for c in source.chars() {
        if c == '"' {
            quoted = !quoted;
            current.push(c);
        } else if c.is_whitespace() && !quoted {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if quoted {
        return Err(SqlError::Ddl("unterminated identifier".into()));
    }
    if !current.is_empty() {
        out.push(current);
    }
    Ok(out)
}
