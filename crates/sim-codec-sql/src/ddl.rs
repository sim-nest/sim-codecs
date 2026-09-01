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
    /// Whether the column declaration carries an inline primary key.
    pub primary_key: bool,
}
/// One parsed, untrusted table declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftTable {
    /// Identifier after SQL unquoting.
    pub name: String,
    /// Declared columns in source order.
    pub columns: Vec<DraftColumn>,
    /// Primary-key column names in declaration order.
    pub primary_key: Vec<String>,
    /// HSQLDB identity high-water mark, when declared by `RESTART WITH`.
    pub restart_with: Option<i64>,
    /// HSQLDB cache index roots. The trailing row-count marker is excluded.
    pub index_roots: Vec<i64>,
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
        let statements = match grammar {
            LegacyDdl::Sqlite => split_statements(source)?,
            LegacyDdl::Hsqldb => source.lines().map(ToOwned::to_owned).collect(),
        };
        for raw in statements {
            let statement = raw.trim();
            if statement.is_empty() {
                continue;
            }
            let upper = statement.to_ascii_uppercase();
            if grammar == LegacyDdl::Hsqldb && upper.starts_with("ALTER TABLE ") {
                apply_restart(&mut tables, statement)?;
                continue;
            }
            if grammar == LegacyDdl::Hsqldb && upper.starts_with("SET TABLE ") {
                apply_index_roots(&mut tables, statement)?;
                continue;
            }
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
            let mut primary_key = vec![];
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
                    if first == "PRIMARY"
                        || (first == "CONSTRAINT"
                            && item.to_ascii_uppercase().contains(" PRIMARY KEY"))
                    {
                        primary_key = parse_primary_key(&item)?;
                    }
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
                let inline_primary = item.to_ascii_uppercase().contains(" PRIMARY KEY");
                let column_name = unquote(&words[0])?;
                if inline_primary {
                    primary_key.push(column_name.clone());
                }
                columns.push(DraftColumn {
                    name: column_name,
                    storage_type,
                    nullable: !item.to_ascii_uppercase().contains("NOT NULL"),
                    primary_key: inline_primary,
                });
            }
            if columns.is_empty() {
                return Err(SqlError::Ddl("table has no decodable columns".into()));
            }
            tables.push((
                DraftTable {
                    name,
                    columns,
                    primary_key,
                    restart_with: None,
                    index_roots: vec![],
                },
                diagnostics,
            ));
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
                            "{} {}{}{}",
                            quote(&c.name),
                            c.storage_type,
                            if c.nullable { "" } else { " NOT NULL" },
                            if c.primary_key { " PRIMARY KEY" } else { "" }
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join("\n"))
    }
}

fn apply_restart(
    tables: &mut [(DraftTable, Vec<DdlDiagnostic>)],
    statement: &str,
) -> Result<(), SqlError> {
    let words = words(statement)?;
    if words.len() != 9
        || !words[0].eq_ignore_ascii_case("ALTER")
        || !words[1].eq_ignore_ascii_case("TABLE")
        || !words[3].eq_ignore_ascii_case("ALTER")
        || !words[4].eq_ignore_ascii_case("COLUMN")
        || !words[6].eq_ignore_ascii_case("RESTART")
        || !words[7].eq_ignore_ascii_case("WITH")
    {
        return Err(SqlError::Ddl(
            "unsupported HSQLDB ALTER TABLE; expected ALTER COLUMN ... RESTART WITH integer".into(),
        ));
    }
    let table = unquote(&words[2])?;
    let column = unquote(&words[5])?;
    let next = words[8]
        .trim_end_matches(';')
        .parse::<i64>()
        .map_err(|_| SqlError::Ddl("HSQLDB RESTART WITH requires an i64 integer".into()))?;
    let draft = find_table_mut(tables, &table)?;
    if !draft
        .columns
        .iter()
        .any(|candidate| candidate.name == column)
    {
        return Err(SqlError::Ddl(
            "HSQLDB RESTART column is not declared by its table".into(),
        ));
    }
    draft.restart_with = Some(next);
    Ok(())
}

fn apply_index_roots(
    tables: &mut [(DraftTable, Vec<DdlDiagnostic>)],
    statement: &str,
) -> Result<(), SqlError> {
    let rest = statement
        .get("SET TABLE ".len()..)
        .ok_or_else(|| SqlError::Ddl("invalid HSQLDB SET TABLE".into()))?
        .trim_start();
    let (table, rest) = take_identifier(rest)?;
    let payload = rest
        .strip_prefix("INDEX'")
        .or_else(|| rest.strip_prefix("index'"))
        .ok_or_else(|| {
            SqlError::Ddl("unsupported HSQLDB SET TABLE; expected INDEX roots".into())
        })?;
    let payload = payload
        .strip_suffix('\'')
        .ok_or_else(|| SqlError::Ddl("unterminated HSQLDB INDEX roots".into()))?;
    let mut roots = payload
        .split_whitespace()
        .map(str::parse::<i64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SqlError::Ddl("HSQLDB INDEX roots require i64 integers".into()))?;
    if roots.pop().is_none() {
        return Err(SqlError::Ddl(
            "HSQLDB INDEX roots require a trailing row-count marker".into(),
        ));
    }
    find_table_mut(tables, &table)?.index_roots = roots;
    Ok(())
}

fn find_table_mut<'a>(
    tables: &'a mut [(DraftTable, Vec<DdlDiagnostic>)],
    name: &str,
) -> Result<&'a mut DraftTable, SqlError> {
    tables
        .iter_mut()
        .find(|(table, _)| table.name == name)
        .map(|(table, _)| table)
        .ok_or_else(|| {
            SqlError::Ddl("HSQLDB metadata precedes or names an undeclared table".into())
        })
}

fn parse_primary_key(item: &str) -> Result<Vec<String>, SqlError> {
    let upper = item.to_ascii_uppercase();
    let key = upper
        .find("PRIMARY KEY")
        .ok_or_else(|| SqlError::Ddl("invalid primary key constraint".into()))?;
    let tail = &item[key + "PRIMARY KEY".len()..];
    let open = tail
        .find('(')
        .ok_or_else(|| SqlError::Ddl("primary key requires columns".into()))?;
    let close = tail
        .rfind(')')
        .ok_or_else(|| SqlError::Ddl("primary key requires closing parenthesis".into()))?;
    split_commas(&tail[open + 1..close])?
        .iter()
        .map(|name| unquote(name))
        .collect()
}

fn take_identifier(input: &str) -> Result<(String, &str), SqlError> {
    if let Some(quoted) = input.strip_prefix('"') {
        let end = quoted
            .find('"')
            .ok_or_else(|| SqlError::Ddl("unterminated identifier".into()))?
            + 1;
        Ok((unquote(&input[..=end])?, input[end + 1..].trim_start()))
    } else {
        let end = input.find(char::is_whitespace).unwrap_or(input.len());
        Ok((unquote(&input[..end])?, input[end..].trim_start()))
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
