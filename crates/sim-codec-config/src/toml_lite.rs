use std::collections::{BTreeSet, HashSet};

use sim_kernel::Expr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Document {
    pub(crate) root: Table,
    pub(crate) sections: Vec<Section>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Table {
    pub(crate) entries: Vec<(String, TomlValue)>,
}

impl Table {
    fn push(&mut self, key: String, value: TomlValue) -> Result<(), String> {
        if self.entries.iter().any(|(existing, _)| existing == &key) {
            return Err(format!("duplicate config key {key:?}"));
        }
        self.entries.push((key, value));
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Section {
    pub(crate) path: Vec<String>,
    pub(crate) repeated: bool,
    pub(crate) table: Table,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TomlValue {
    String(String),
    Integer(i64),
    Bool(bool),
    Array(Vec<TomlValue>),
}

pub(crate) fn parse(source: &str) -> Result<Document, String> {
    let mut document = Document {
        root: Table::default(),
        sections: Vec::new(),
    };
    let mut current_section = None;
    let mut regular_sections = HashSet::<String>::new();

    for (index, raw_line) in source.lines().enumerate() {
        let line_number = index + 1;
        let stripped =
            strip_comment(raw_line).map_err(|message| format!("line {line_number}: {message}"))?;
        let line = stripped.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            let (path, repeated) =
                parse_header(line).map_err(|message| format!("line {line_number}: {message}"))?;
            if !repeated {
                let canonical = path.join(".");
                if !regular_sections.insert(canonical.clone()) {
                    return Err(format!(
                        "line {line_number}: duplicate config section {canonical:?}"
                    ));
                }
            }
            document.sections.push(Section {
                path,
                repeated,
                table: Table::default(),
            });
            current_section = Some(document.sections.len() - 1);
            continue;
        }
        let (key, value) =
            parse_assignment(line).map_err(|message| format!("line {line_number}: {message}"))?;
        match current_section {
            Some(section) => document.sections[section].table.push(key, value),
            None => document.root.push(key, value),
        }
        .map_err(|message| format!("line {line_number}: {message}"))?;
    }

    Ok(document)
}

pub(crate) fn encode_map(entries: &[(Expr, Expr)]) -> Result<String, String> {
    let mut output = String::new();
    encode_table(entries, &[], &mut output)?;
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

fn encode_table(
    entries: &[(Expr, Expr)],
    path: &[String],
    output: &mut String,
) -> Result<(), String> {
    let mut scalars = Vec::<(String, &Expr)>::new();
    let mut tables = Vec::<(String, &[(Expr, Expr)])>::new();
    let mut repeated = Vec::<(String, Vec<&[(Expr, Expr)]>)>::new();

    for (key, value) in entries {
        let key = key_name(key)?;
        match value {
            Expr::Map(table) => tables.push((key, table)),
            Expr::List(items)
                if !items.is_empty() && items.iter().all(|item| matches!(item, Expr::Map(_))) =>
            {
                let mut maps = Vec::new();
                for item in items {
                    let Expr::Map(map) = item else {
                        unreachable!("checked list item shape");
                    };
                    maps.push(map.as_slice());
                }
                repeated.push((key, maps));
            }
            _ => scalars.push((key, value)),
        }
    }

    scalars.sort_by(|left, right| left.0.cmp(&right.0));
    tables.sort_by(|left, right| left.0.cmp(&right.0));
    repeated.sort_by(|left, right| left.0.cmp(&right.0));

    for (key, value) in scalars {
        output.push_str(&key);
        output.push_str(" = ");
        output.push_str(&encode_value(value)?);
        output.push('\n');
    }

    for (key, maps) in repeated {
        let section_path = child_path(path, &key);
        for map in maps {
            section_break(output);
            output.push_str("[[");
            output.push_str(&section_path.join("."));
            output.push_str("]]\n");
            encode_table(map, &section_path, output)?;
        }
    }

    for (key, table) in tables {
        let section_path = child_path(path, &key);
        section_break(output);
        output.push('[');
        output.push_str(&section_path.join("."));
        output.push_str("]\n");
        encode_table(table, &section_path, output)?;
    }

    Ok(())
}

fn child_path(path: &[String], child: &str) -> Vec<String> {
    path.iter()
        .cloned()
        .chain(std::iter::once(child.to_owned()))
        .collect()
}

fn section_break(output: &mut String) {
    if output.is_empty() {
        return;
    }
    if output.ends_with("\n\n") {
        return;
    }
    if output.ends_with('\n') {
        output.push('\n');
    } else {
        output.push_str("\n\n");
    }
}

fn encode_value(value: &Expr) -> Result<String, String> {
    match value {
        Expr::String(value) => Ok(format!("\"{}\"", escape_string(value))),
        Expr::Bool(value) => Ok(value.to_string()),
        Expr::Number(number) if is_integer_text(&number.canonical) => Ok(number.canonical.clone()),
        Expr::Number(number) => Err(format!(
            "config numbers must be integer literals, got {:?}",
            number.canonical
        )),
        Expr::List(items) => {
            let mut values = Vec::new();
            for item in items {
                if matches!(item, Expr::Map(_)) {
                    return Err("config arrays cannot mix table values with scalars".to_owned());
                }
                values.push(encode_value(item)?);
            }
            Ok(format!("[{}]", values.join(", ")))
        }
        _ => Err(format!("config value is not encodable: {value:?}")),
    }
}

fn escape_string(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn is_integer_text(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit())
}

fn key_name(key: &Expr) -> Result<String, String> {
    let name = match key {
        Expr::Symbol(symbol) => symbol.as_qualified_str(),
        Expr::String(value) => value.clone(),
        _ => {
            return Err(format!(
                "config map keys must be symbols or strings, got {key:?}"
            ));
        }
    };
    validate_key(&name)?;
    Ok(name)
}

fn parse_header(line: &str) -> Result<(Vec<String>, bool), String> {
    let (inner, repeated) = if let Some(inner) = line.strip_prefix("[[") {
        let inner = inner
            .strip_suffix("]]")
            .ok_or("malformed repeated section; expected closing ]]")?;
        (inner, true)
    } else {
        let inner = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .ok_or("malformed section; expected closing ]")?;
        (inner, false)
    };
    if inner.trim() != inner || inner.is_empty() {
        return Err("section names must not be empty or padded".to_owned());
    }
    let mut path = Vec::new();
    for segment in inner.split('.') {
        validate_key(segment)?;
        path.push(segment.to_owned());
    }
    Ok((path, repeated))
}

fn parse_assignment(line: &str) -> Result<(String, TomlValue), String> {
    let position = find_assignment(line).ok_or("expected key = value assignment")?;
    let key = line[..position].trim();
    validate_key(key)?;
    let value = line[position + 1..].trim();
    if value.is_empty() {
        return Err(format!("config key {key:?} has no value"));
    }
    Ok((key.to_owned(), parse_value(value)?))
}

fn find_assignment(line: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
        } else if ch == '=' {
            return Some(index);
        }
    }
    None
}

fn parse_value(value: &str) -> Result<TomlValue, String> {
    if value.starts_with('"') {
        parse_string(value).map(TomlValue::String)
    } else if value == "true" {
        Ok(TomlValue::Bool(true))
    } else if value == "false" {
        Ok(TomlValue::Bool(false))
    } else if value.starts_with('[') {
        parse_array(value)
    } else if is_integer_text(value) {
        value
            .parse::<i64>()
            .map(TomlValue::Integer)
            .map_err(|err| format!("integer literal {value:?} is out of range: {err}"))
    } else {
        Err(format!("unsupported config value {value:?}"))
    }
}

fn parse_array(value: &str) -> Result<TomlValue, String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or("malformed array; expected closing ]")?;
    if inner.trim().is_empty() {
        return Ok(TomlValue::Array(Vec::new()));
    }
    let mut items = Vec::new();
    for item in split_array_items(inner)? {
        let parsed = parse_value(item.trim())?;
        if matches!(parsed, TomlValue::Array(_)) {
            return Err("nested arrays are not supported in config text".to_owned());
        }
        items.push(parsed);
    }
    Ok(TomlValue::Array(items))
}

fn split_array_items(inner: &str) -> Result<Vec<&str>, String> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in inner.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
        } else if ch == ',' {
            let item = inner[start..index].trim();
            if item.is_empty() {
                return Err("empty array item".to_owned());
            }
            items.push(item);
            start = index + 1;
        }
    }
    if in_string {
        return Err("unterminated string in array".to_owned());
    }
    let item = inner[start..].trim();
    if item.is_empty() {
        return Err("trailing comma in array".to_owned());
    }
    items.push(item);
    Ok(items)
}

fn parse_string(value: &str) -> Result<String, String> {
    let mut chars = value.char_indices();
    if chars.next().map(|(_, ch)| ch) != Some('"') {
        return Err("expected quoted string".to_owned());
    }
    let mut output = String::new();
    let mut escaped = false;
    for (index, ch) in chars {
        if escaped {
            match ch {
                '"' => output.push('"'),
                '\\' => output.push('\\'),
                'n' => output.push('\n'),
                'r' => output.push('\r'),
                't' => output.push('\t'),
                _ => return Err(format!("unsupported string escape \\{ch}")),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                let rest = value[index + ch.len_utf8()..].trim();
                if rest.is_empty() {
                    return Ok(output);
                }
                return Err(format!("unexpected content after string literal: {rest:?}"));
            }
            other => output.push(other),
        }
    }
    Err("unterminated string literal".to_owned())
}

fn strip_comment(line: &str) -> Result<&str, String> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
        } else if ch == '#' {
            return Ok(&line[..index]);
        }
    }
    if in_string {
        Err("unterminated string literal".to_owned())
    } else {
        Ok(line)
    }
}

fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("config keys and section names cannot be empty".to_owned());
    }
    if !key.is_ascii() {
        return Err(format!("config key {key:?} must be ASCII"));
    }
    if key
        .chars()
        .any(|ch| ch.is_ascii_whitespace() || matches!(ch, '[' | ']' | '"' | '\'' | '='))
    {
        return Err(format!(
            "config key {key:?} contains an unsupported character"
        ));
    }
    let allowed = key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/'));
    if !allowed {
        return Err(format!(
            "config key {key:?} contains an unsupported character"
        ));
    }
    Ok(())
}

#[allow(dead_code)]
fn sorted_keys(entries: &[(Expr, Expr)]) -> Result<BTreeSet<String>, String> {
    entries.iter().map(|(key, _)| key_name(key)).collect()
}
