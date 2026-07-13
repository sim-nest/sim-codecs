//! Reversible document-domain edits for the shared markup IR.

use sim_kernel::{Error, Expr, NumberLiteral, Result};
use sim_value::build::{entry, list, sym, text, uint};

use crate::{Inline, MarkupBlock, MarkupDoc, MarkupError, SpanState};

/// A reversible edit over a semantic markup document.
#[derive(Clone, Debug, PartialEq)]
pub enum MarkupEdit {
    /// Replace the document title.
    SetTitle {
        /// Title value expected before the edit.
        old: Option<String>,
        /// Title value written by the edit.
        new: Option<String>,
    },
    /// Insert a block at an index.
    InsertBlock {
        /// Insertion index.
        index: usize,
        /// Block to insert.
        block: MarkupBlock,
    },
    /// Replace the block at an index.
    ReplaceBlock {
        /// Replacement index.
        index: usize,
        /// Block expected before the edit.
        old: MarkupBlock,
        /// Block written by the edit.
        new: MarkupBlock,
    },
    /// Delete the block at an index.
    DeleteBlock {
        /// Deletion index.
        index: usize,
        /// Block expected before deletion.
        old: MarkupBlock,
    },
    /// Replace a text inline inside a block.
    SetInlineText {
        /// Block index containing the inline.
        block: usize,
        /// Index path from the block's editable inline list to a text inline.
        path: Vec<usize>,
        /// Inline text expected before the edit.
        old: String,
        /// Inline text written by the edit.
        new: String,
    },
}

impl MarkupEdit {
    /// Project this edit into ordinary SIM data.
    pub fn as_expr(&self) -> Expr {
        match self {
            Self::SetTitle { old, new } => {
                let mut entries = edit_entries("set-title");
                push_optional_string(&mut entries, "old", old);
                push_optional_string(&mut entries, "new", new);
                Expr::Map(entries)
            }
            Self::InsertBlock { index, block } => Expr::Map(vec![
                entry("kind", sym("markup-edit")),
                entry("op", sym("insert-block")),
                entry("index", uint(*index as u64)),
                entry("block", block.as_expr()),
            ]),
            Self::ReplaceBlock { index, old, new } => Expr::Map(vec![
                entry("kind", sym("markup-edit")),
                entry("op", sym("replace-block")),
                entry("index", uint(*index as u64)),
                entry("old", old.as_expr()),
                entry("new", new.as_expr()),
            ]),
            Self::DeleteBlock { index, old } => Expr::Map(vec![
                entry("kind", sym("markup-edit")),
                entry("op", sym("delete-block")),
                entry("index", uint(*index as u64)),
                entry("old", old.as_expr()),
            ]),
            Self::SetInlineText {
                block,
                path,
                old,
                new,
            } => Expr::Map(vec![
                entry("kind", sym("markup-edit")),
                entry("op", sym("set-inline-text")),
                entry("block", uint(*block as u64)),
                entry(
                    "path",
                    list(path.iter().map(|index| uint(*index as u64)).collect()),
                ),
                entry("old", text(old)),
                entry("new", text(new)),
            ]),
        }
    }

    /// Reconstruct an edit from ordinary SIM data.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let entries = map_entries(expr, "markup edit")?;
        require_kind(entries)?;
        match required_symbol(entries, "op", "markup edit")?.as_str() {
            "set-title" => Ok(Self::SetTitle {
                old: optional_string(entries, "old")?.map(str::to_owned),
                new: optional_string(entries, "new")?.map(str::to_owned),
            }),
            "insert-block" => Ok(Self::InsertBlock {
                index: required_usize(entries, "index", "insert block")?,
                block: MarkupBlock::from_expr(required_field(entries, "block", "insert block")?)?,
            }),
            "replace-block" => Ok(Self::ReplaceBlock {
                index: required_usize(entries, "index", "replace block")?,
                old: MarkupBlock::from_expr(required_field(entries, "old", "replace block")?)?,
                new: MarkupBlock::from_expr(required_field(entries, "new", "replace block")?)?,
            }),
            "delete-block" => Ok(Self::DeleteBlock {
                index: required_usize(entries, "index", "delete block")?,
                old: MarkupBlock::from_expr(required_field(entries, "old", "delete block")?)?,
            }),
            "set-inline-text" => Ok(Self::SetInlineText {
                block: required_usize(entries, "block", "set inline text")?,
                path: required_path(entries, "path", "set inline text")?,
                old: required_string(entries, "old", "set inline text")?.to_owned(),
                new: required_string(entries, "new", "set inline text")?.to_owned(),
            }),
            other => Err(Error::Eval(format!("unknown markup edit op {other}"))),
        }
    }
}

/// Apply a reversible markup edit to a document.
pub fn apply_edit(doc: &mut MarkupDoc, edit: &MarkupEdit) -> std::result::Result<(), MarkupError> {
    match edit {
        MarkupEdit::SetTitle { old, new } => {
            if &doc.title != old {
                return invalid_edit("title precondition failed");
            }
            doc.title = new.clone();
        }
        MarkupEdit::InsertBlock { index, block } => {
            if *index > doc.blocks.len() {
                return invalid_edit("insert block index out of range");
            }
            doc.blocks.insert(*index, block.clone());
        }
        MarkupEdit::ReplaceBlock { index, old, new } => {
            let current = doc.blocks.get_mut(*index).ok_or_else(|| {
                MarkupError::InvalidDocument("replace block index out of range".to_owned())
            })?;
            if current != old {
                return invalid_edit("replace block precondition failed");
            }
            *current = new.clone();
        }
        MarkupEdit::DeleteBlock { index, old } => {
            let current = doc.blocks.get(*index).ok_or_else(|| {
                MarkupError::InvalidDocument("delete block index out of range".to_owned())
            })?;
            if current != old {
                return invalid_edit("delete block precondition failed");
            }
            doc.blocks.remove(*index);
        }
        MarkupEdit::SetInlineText {
            block,
            path,
            old,
            new,
        } => {
            let block = doc.blocks.get_mut(*block).ok_or_else(|| {
                MarkupError::InvalidDocument("inline block index out of range".to_owned())
            })?;
            match block {
                MarkupBlock::Heading { text, .. } => apply_inline_text(text, path, old, new)?,
                MarkupBlock::Paragraph { content, .. } => {
                    apply_inline_text(content, path, old, new)?
                }
                MarkupBlock::Figure { caption, .. } => apply_inline_text(caption, path, old, new)?,
                _ => return invalid_edit("block has no editable inline text"),
            }
            mark_block_dirty(block);
        }
    }
    doc.source = None;
    Ok(())
}

/// Return the inverse edit for `edit`.
pub fn invert_edit(edit: &MarkupEdit) -> MarkupEdit {
    match edit {
        MarkupEdit::SetTitle { old, new } => MarkupEdit::SetTitle {
            old: new.clone(),
            new: old.clone(),
        },
        MarkupEdit::InsertBlock { index, block } => MarkupEdit::DeleteBlock {
            index: *index,
            old: block.clone(),
        },
        MarkupEdit::ReplaceBlock { index, old, new } => MarkupEdit::ReplaceBlock {
            index: *index,
            old: new.clone(),
            new: old.clone(),
        },
        MarkupEdit::DeleteBlock { index, old } => MarkupEdit::InsertBlock {
            index: *index,
            block: old.clone(),
        },
        MarkupEdit::SetInlineText {
            block,
            path,
            old,
            new,
        } => MarkupEdit::SetInlineText {
            block: *block,
            path: path.clone(),
            old: new.clone(),
            new: old.clone(),
        },
    }
}

fn edit_entries(op: &str) -> Vec<(Expr, Expr)> {
    vec![entry("kind", sym("markup-edit")), entry("op", sym(op))]
}

fn push_optional_string(entries: &mut Vec<(Expr, Expr)>, name: &str, value: &Option<String>) {
    if let Some(value) = value {
        entries.push(entry(name, text(value)));
    }
}

fn apply_inline_text(
    items: &mut [Inline],
    path: &[usize],
    old: &str,
    new: &str,
) -> std::result::Result<(), MarkupError> {
    let inline = inline_at_path_mut(items, path)?;
    match inline {
        Inline::Text(value) if value == old => {
            *value = new.to_owned();
            Ok(())
        }
        Inline::Text(_) => invalid_edit("inline text precondition failed"),
        _ => invalid_edit("inline path does not point at text"),
    }
}

fn inline_at_path_mut<'a>(
    items: &'a mut [Inline],
    path: &[usize],
) -> std::result::Result<&'a mut Inline, MarkupError> {
    let (index, rest) = path
        .split_first()
        .ok_or_else(|| MarkupError::InvalidDocument("inline path must not be empty".to_owned()))?;
    let inline = items
        .get_mut(*index)
        .ok_or_else(|| MarkupError::InvalidDocument("inline path index out of range".to_owned()))?;
    if rest.is_empty() {
        return Ok(inline);
    }
    match inline {
        Inline::Emph(children) | Inline::Strong(children) => inline_at_path_mut(children, rest),
        Inline::Link { label, .. } => inline_at_path_mut(label, rest),
        _ => invalid_edit("inline path cannot descend through this node"),
    }
}

fn mark_block_dirty(block: &mut MarkupBlock) {
    match block {
        MarkupBlock::Heading { span, .. }
        | MarkupBlock::Paragraph { span, .. }
        | MarkupBlock::CodeBlock { span, .. }
        | MarkupBlock::MathBlock { span, .. }
        | MarkupBlock::Table { span, .. }
        | MarkupBlock::Figure { span, .. }
        | MarkupBlock::Raw { span, .. } => mark_span_dirty(span),
        MarkupBlock::Quote { blocks, span } => {
            mark_span_dirty(span);
            for block in blocks {
                mark_block_dirty(block);
            }
        }
        MarkupBlock::List { items, span, .. } => {
            mark_span_dirty(span);
            for item in items {
                for block in item {
                    mark_block_dirty(block);
                }
            }
        }
    }
}

fn mark_span_dirty(span: &mut Option<crate::Span>) {
    if let Some(span) = span {
        span.state = SpanState::Dirty;
    }
}

fn invalid_edit<T>(message: &str) -> std::result::Result<T, MarkupError> {
    Err(MarkupError::InvalidDocument(message.to_owned()))
}

fn map_entries<'a>(expr: &'a Expr, expected: &str) -> Result<&'a [(Expr, Expr)]> {
    match expr {
        Expr::Map(entries) => Ok(entries),
        _ => Err(Error::Eval(format!("{expected} must be a map"))),
    }
}

fn require_kind(entries: &[(Expr, Expr)]) -> Result<()> {
    let actual = required_symbol(entries, "kind", "markup edit")?;
    if actual == "markup-edit" {
        Ok(())
    } else {
        Err(Error::Eval(
            "markup edit kind must be markup-edit".to_owned(),
        ))
    }
}

fn required_field<'a>(entries: &'a [(Expr, Expr)], name: &str, context: &str) -> Result<&'a Expr> {
    field(entries, name).ok_or_else(|| Error::Eval(format!("{context} requires {name} field")))
}

fn required_symbol(entries: &[(Expr, Expr)], name: &str, context: &str) -> Result<String> {
    match required_field(entries, name, context)? {
        Expr::Symbol(symbol) if symbol.namespace.is_none() => Ok(symbol.name.to_string()),
        Expr::String(value) => Ok(value.clone()),
        _ => Err(Error::Eval(format!(
            "{context} field {name} must be a symbol"
        ))),
    }
}

fn required_string<'a>(entries: &'a [(Expr, Expr)], name: &str, context: &str) -> Result<&'a str> {
    match required_field(entries, name, context)? {
        Expr::String(value) => Ok(value),
        _ => Err(Error::Eval(format!(
            "{context} field {name} must be a string"
        ))),
    }
}

fn optional_string<'a>(entries: &'a [(Expr, Expr)], name: &str) -> Result<Option<&'a str>> {
    match field(entries, name) {
        Some(Expr::String(value)) => Ok(Some(value)),
        Some(_) => Err(Error::Eval(format!("{name} field must be a string"))),
        None => Ok(None),
    }
}

fn required_usize(entries: &[(Expr, Expr)], name: &str, context: &str) -> Result<usize> {
    match required_field(entries, name, context)? {
        Expr::Number(NumberLiteral { canonical, .. }) => canonical
            .parse()
            .map_err(|_| Error::Eval(format!("{context} field {name} must be an integer"))),
        _ => Err(Error::Eval(format!(
            "{context} field {name} must be a number"
        ))),
    }
}

fn required_path(entries: &[(Expr, Expr)], name: &str, context: &str) -> Result<Vec<usize>> {
    match required_field(entries, name, context)? {
        Expr::List(items) => items
            .iter()
            .map(|item| match item {
                Expr::Number(NumberLiteral { canonical, .. }) => canonical.parse().map_err(|_| {
                    Error::Eval(format!("{context} field {name} must contain integers"))
                }),
                _ => Err(Error::Eval(format!(
                    "{context} field {name} must contain numbers"
                ))),
            })
            .collect(),
        _ => Err(Error::Eval(format!(
            "{context} field {name} must be a list"
        ))),
    }
}

fn field<'a>(entries: &'a [(Expr, Expr)], name: &str) -> Option<&'a Expr> {
    entries
        .iter()
        .find_map(|(key, value)| (key_name(key).as_deref() == Some(name)).then_some(value))
}

fn key_name(key: &Expr) -> Option<String> {
    match key {
        Expr::Symbol(symbol) if symbol.namespace.is_none() => Some(symbol.name.to_string()),
        Expr::String(value) => Some(value.clone()),
        _ => None,
    }
}
