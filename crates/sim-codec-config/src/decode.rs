//! Decoding support for config text.

use sim_codec::{DecodeBudget, Decoder, Input};
use sim_kernel::{CodecId, Error, Expr, NumberLiteral, Result, Symbol};

use crate::toml_lite::{self, Document, Section, TomlValue};

/// Selects the config document shape produced by [`ConfigDecoder`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeMode {
    /// Decode one per-library config file as a single table map.
    Table,
    /// Decode one shared config file as a directory of library id to table map.
    Dir,
}

/// Decoder for config text in either table or directory mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigDecoder {
    mode: DecodeMode,
}

impl ConfigDecoder {
    /// Creates a decoder for the selected mode.
    pub fn new(mode: DecodeMode) -> Self {
        Self { mode }
    }

    /// Creates a decoder that reads one per-library config table.
    pub fn table() -> Self {
        Self::new(DecodeMode::Table)
    }

    /// Creates a decoder that reads one shared directory file.
    pub fn dir() -> Self {
        Self::new(DecodeMode::Dir)
    }

    /// Decodes config text without a runtime context.
    pub fn decode_text(&self, source: &str) -> std::result::Result<Expr, String> {
        let document = parse_source(source)?;
        match self.mode {
            DecodeMode::Table => document_to_table(&document),
            DecodeMode::Dir => document_to_dir(&document),
        }
    }
}

impl Decoder for ConfigDecoder {
    fn decode(&self, cx: &mut sim_codec::ReadCx<'_>, input: Input) -> Result<Expr> {
        let source = input.into_string_for(cx.codec)?;
        let budget = DecodeBudget::new(cx.limits);
        budget.check_input_bytes(cx.codec, source.len())?;
        self.decode_text(&source)
            .map_err(|message| codec_error(cx.codec, message))
    }
}

pub(crate) fn decode_auto_text(codec: CodecId, source: &str) -> Result<Expr> {
    let document = parse_source(source).map_err(|message| codec_error(codec, message))?;
    if looks_like_dir(&document) {
        document_to_dir(&document).map_err(|message| codec_error(codec, message))
    } else {
        document_to_table(&document).map_err(|message| codec_error(codec, message))
    }
}

fn parse_source(source: &str) -> std::result::Result<Document, String> {
    if !source.is_ascii() {
        return Err("config input must be ASCII".to_owned());
    }
    toml_lite::parse(source)
}

fn looks_like_dir(document: &Document) -> bool {
    document.root.entries.is_empty()
        && !document.sections.is_empty()
        && document
            .sections
            .iter()
            .all(|section| section.path.first().is_some_and(|key| key.contains('/')))
}

fn document_to_table(document: &Document) -> std::result::Result<Expr, String> {
    let mut entries = table_entries(&document.root)?;
    for section in &document.sections {
        insert_section(
            &mut entries,
            &section.path,
            section_expr(section)?,
            section.repeated,
        )?;
    }
    Ok(Expr::Map(entries))
}

fn document_to_dir(document: &Document) -> std::result::Result<Expr, String> {
    if !document.root.entries.is_empty() {
        return Err("shared config entries must be inside a library section".to_owned());
    }
    let mut entries = Vec::<(Expr, Expr)>::new();
    for section in &document.sections {
        let Some((lib_id, rest)) = section.path.split_first() else {
            return Err("config section path cannot be empty".to_owned());
        };
        if section.repeated && rest.is_empty() {
            return Err(format!("library section {lib_id:?} cannot be repeated"));
        }
        let lib_table = section_expr(section)?;
        if rest.is_empty() {
            insert_regular_table(&mut entries, lib_id, lib_table)?;
        } else {
            let table = ensure_table(&mut entries, lib_id)?;
            insert_section(table, rest, lib_table, section.repeated)?;
        }
    }
    Ok(Expr::Map(entries))
}

fn table_entries(table: &toml_lite::Table) -> std::result::Result<Vec<(Expr, Expr)>, String> {
    table
        .entries
        .iter()
        .map(|(key, value)| Ok((key_expr(key), value_expr(value)?)))
        .collect()
}

fn section_expr(section: &Section) -> std::result::Result<Expr, String> {
    Ok(Expr::Map(table_entries(&section.table)?))
}

fn value_expr(value: &TomlValue) -> std::result::Result<Expr, String> {
    match value {
        TomlValue::String(value) => Ok(Expr::String(value.clone())),
        TomlValue::Integer(value) => Ok(Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "i64"),
            canonical: value.to_string(),
        })),
        TomlValue::Bool(value) => Ok(Expr::Bool(*value)),
        TomlValue::Array(items) => items
            .iter()
            .map(value_expr)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map(Expr::List),
    }
}

fn insert_section(
    entries: &mut Vec<(Expr, Expr)>,
    path: &[String],
    value: Expr,
    repeated: bool,
) -> std::result::Result<(), String> {
    let Some((head, tail)) = path.split_first() else {
        return Err("config section path cannot be empty".to_owned());
    };
    if tail.is_empty() {
        if repeated {
            insert_repeated_table(entries, head, value)
        } else {
            insert_regular_table(entries, head, value)
        }
    } else {
        let table = ensure_table(entries, head)?;
        insert_section(table, tail, value, repeated)
    }
}

fn insert_regular_table(
    entries: &mut Vec<(Expr, Expr)>,
    key: &str,
    value: Expr,
) -> std::result::Result<(), String> {
    let key = key_expr(key);
    let existing = entries.iter_mut().find(|(existing, _)| existing == &key);
    match existing {
        Some((_, Expr::Map(target))) => {
            let Expr::Map(source) = value else {
                return Err(format!(
                    "section {:?} did not decode as a table",
                    display_key(&key)
                ));
            };
            merge_entries(target, source)
        }
        Some(_) => Err(format!(
            "config key {:?} is already a scalar value",
            display_key(&key)
        )),
        None => {
            entries.push((key, value));
            Ok(())
        }
    }
}

fn insert_repeated_table(
    entries: &mut Vec<(Expr, Expr)>,
    key: &str,
    value: Expr,
) -> std::result::Result<(), String> {
    let key = key_expr(key);
    let existing = entries.iter_mut().find(|(existing, _)| existing == &key);
    match existing {
        Some((_, Expr::List(items))) => {
            items.push(value);
            Ok(())
        }
        Some(_) => Err(format!(
            "repeated config table {:?} collides with an existing value",
            display_key(&key)
        )),
        None => {
            entries.push((key, Expr::List(vec![value])));
            Ok(())
        }
    }
}

fn ensure_table<'a>(
    entries: &'a mut Vec<(Expr, Expr)>,
    key: &str,
) -> std::result::Result<&'a mut Vec<(Expr, Expr)>, String> {
    let key = key_expr(key);
    if let Some(position) = entries.iter().position(|(existing, _)| existing == &key) {
        match &mut entries[position].1 {
            Expr::Map(table) => Ok(table),
            _ => Err(format!(
                "config key {:?} is already a scalar value",
                display_key(&key)
            )),
        }
    } else {
        entries.push((key, Expr::Map(Vec::new())));
        match &mut entries.last_mut().expect("inserted table").1 {
            Expr::Map(table) => Ok(table),
            _ => unreachable!("inserted a table"),
        }
    }
}

fn merge_entries(
    target: &mut Vec<(Expr, Expr)>,
    source: Vec<(Expr, Expr)>,
) -> std::result::Result<(), String> {
    for (key, value) in source {
        if target.iter().any(|(existing, _)| existing == &key) {
            return Err(format!("duplicate config key {:?}", display_key(&key)));
        }
        target.push((key, value));
    }
    Ok(())
}

fn key_expr(key: &str) -> Expr {
    Expr::Symbol(Symbol::new(key.to_owned()))
}

fn display_key(key: &Expr) -> String {
    match key {
        Expr::Symbol(symbol) => symbol.as_qualified_str(),
        Expr::String(value) => value.clone(),
        other => format!("{other:?}"),
    }
}

fn codec_error(codec: CodecId, message: impl Into<String>) -> Error {
    Error::CodecError {
        codec,
        message: message.into(),
    }
}
