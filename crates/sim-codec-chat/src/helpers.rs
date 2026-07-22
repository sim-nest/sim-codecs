//! Constructors and validators for chat transcript `Expr` values (model
//! request, response, error, and card maps) plus the shared
//! `validate_chat_transcript` shape check used by the codec.

use std::collections::BTreeSet;

use sim_kernel::{Error, Expr, Result, Symbol};
use sim_value::access::{entry_field, entry_field_any, field as expr_field};

/// Returns `true` when `expr` is a chat transcript map carrying a true
/// `model-request` marker.
pub fn is_model_request_expr(expr: &Expr) -> bool {
    marker_is_true(expr, "model-request")
}

/// Returns the `messages` list of a model-request transcript, erroring if
/// `expr` is not a valid request or its `messages` field is not a list.
pub fn model_request_messages_expr(expr: &Expr) -> Result<&[Expr]> {
    validate_chat_transcript(expr)?;
    if !is_model_request_expr(expr) {
        return Err(chat_eval("expr must be a model-request transcript"));
    }
    match expr_field(expr, "messages") {
        Some(Expr::List(messages)) => Ok(messages),
        _ => Err(chat_eval("model request messages field must be a list")),
    }
}

/// Builds a model-response transcript map from a `runner`, `model` name,
/// `content` parts, and `stop_reason`.
///
/// The result carries a true `model-response` marker and validates under
/// [`validate_chat_transcript`].
pub fn model_response_expr(
    runner: Symbol,
    model: impl Into<String>,
    content: Vec<Expr>,
    stop_reason: Symbol,
) -> Expr {
    Expr::Map(vec![
        key_bool("model-response", true),
        key_expr("runner", Expr::Symbol(runner)),
        key_expr("model", Expr::String(model.into())),
        key_expr("content", Expr::List(content)),
        key_expr("stop-reason", Expr::Symbol(stop_reason)),
    ])
}

/// Builds a model-response transcript carrying an error: a single text content
/// part holding `message`, a `stop-reason` of `error`, and `text`/`shape-ok`
/// fields recording the failure.
pub fn model_error_expr(
    runner: Symbol,
    model: impl Into<String>,
    message: impl Into<String>,
) -> Expr {
    let text = message.into();
    let content = vec![Expr::Map(vec![
        key_expr("type", Expr::Symbol(Symbol::new("text"))),
        key_expr("text", Expr::String(text.clone())),
    ])];
    let mut response = match model_response_expr(runner, model, content, Symbol::new("error")) {
        Expr::Map(entries) => entries,
        _ => unreachable!("model_response_expr always returns a map"),
    };
    response.push(key_expr("text", Expr::String(text)));
    response.push(key_bool("shape-ok", false));
    Expr::Map(response)
}

/// Builds a model-card transcript map describing a model's identity and
/// capabilities (provider, locality, modalities, and stream/tool/JSON/shape
/// support), with conservative defaults the caller can override.
///
/// # Examples
///
/// ```
/// use sim_codec_chat::{model_card_expr, validate_chat_transcript};
/// use sim_kernel::Symbol;
///
/// let card = model_card_expr(
///     Symbol::new("local-reasoner"),
///     "qwen2.5-coder:14b",
///     Symbol::new("ollama"),
///     Symbol::new("local"),
/// );
/// assert!(validate_chat_transcript(&card).is_ok());
/// ```
pub fn model_card_expr(
    runner: Symbol,
    model: impl Into<String>,
    provider: Symbol,
    locality: Symbol,
) -> Expr {
    Expr::Map(vec![
        key_bool("model-card", true),
        key_expr("runner", Expr::Symbol(runner)),
        key_expr("model", Expr::String(model.into())),
        key_expr("provider", Expr::Symbol(provider)),
        key_expr("locality", Expr::Symbol(locality)),
        key_expr(
            "modalities-in",
            Expr::List(vec![Expr::Symbol(Symbol::new("text"))]),
        ),
        key_expr(
            "modalities-out",
            Expr::List(vec![Expr::Symbol(Symbol::new("text"))]),
        ),
        key_bool("supports-stream", false),
        key_bool("supports-tools", false),
        key_bool("supports-json", true),
        key_bool("supports-shape", false),
        key_expr("health", Expr::Symbol(Symbol::new("unknown"))),
    ])
}

/// Validates that `expr` is a well-formed chat transcript and fails closed
/// otherwise.
///
/// A transcript must be an `Expr::Map` with exactly one true marker among
/// `model-request`, `model-response`, `model-event`, and `model-card`; the
/// matching variant's required fields are then checked. This is the domain
/// gate the codec runs on both decode and encode.
///
/// The validator owns a finite field set for each transcript variant and
/// rejects duplicates in that set. Nested canonical records such as messages,
/// content parts, tool-call arguments, usage, and provider raw projections are
/// strict over their bare-symbol keys. Qualified symbols, non-symbol keys, and
/// model-card advisory fields beyond runner/model/provider/locality stay open
/// extension space.
///
/// # Examples
///
/// ```
/// use sim_codec_chat::{model_response_expr, validate_chat_transcript};
/// use sim_kernel::Symbol;
///
/// let response = model_response_expr(
///     Symbol::new("local-reasoner"),
///     "qwen2.5-coder:14b",
///     Vec::new(),
///     Symbol::new("stop"),
/// );
/// assert!(validate_chat_transcript(&response).is_ok());
/// ```
pub fn validate_chat_transcript(expr: &Expr) -> Result<()> {
    let entries = require_map(expr, "chat transcript")?;
    reject_duplicate_bare_fields(
        entries,
        "chat transcript",
        &[
            "model-request",
            "model-response",
            "model-event",
            "model-card",
        ],
    )?;
    let markers = [
        marker_is_true_in(entries, "model-request"),
        marker_is_true_in(entries, "model-response"),
        marker_is_true_in(entries, "model-event"),
        marker_is_true_in(entries, "model-card"),
    ]
    .into_iter()
    .filter(|value| *value)
    .count();

    if markers != 1 {
        return Err(chat_eval(
            "chat transcript must have exactly one true model-request, model-response, model-event, or model-card marker",
        ));
    }

    if marker_is_true_in(entries, "model-request") {
        validate_request(entries)
    } else if marker_is_true_in(entries, "model-response") {
        validate_response(entries)
    } else if marker_is_true_in(entries, "model-event") {
        validate_event(entries)
    } else {
        validate_card(entries)
    }
}

fn validate_request(entries: &[(Expr, Expr)]) -> Result<()> {
    reject_duplicate_bare_fields(
        entries,
        "chat transcript",
        &[
            "model-request",
            "task",
            "messages",
            "budget",
            "max-tokens",
            "tools",
            "tool-choice",
        ],
    )?;
    require_field(entries, "task")?;
    for message in require_list_field(entries, "messages")? {
        validate_message(message)?;
    }
    validate_optional_projected_field(entries, "budget")?;
    validate_optional_projected_field(entries, "max-tokens")?;
    validate_optional_projected_field(entries, "tools")?;
    validate_optional_projected_field(entries, "tool-choice")?;
    Ok(())
}

fn validate_response(entries: &[(Expr, Expr)]) -> Result<()> {
    reject_duplicate_bare_fields(
        entries,
        "chat transcript",
        &[
            "model-response",
            "runner",
            "model",
            "stop-reason",
            "content",
            "usage",
            "raw-provider-response",
        ],
    )?;
    require_symbol_field(entries, "runner")?;
    require_string_field(entries, "model")?;
    require_symbol_field(entries, "stop-reason")?;
    validate_content_list(entries, "content")?;
    validate_optional_usage(entries)?;
    validate_optional_projected_field(entries, "raw-provider-response")
}

fn validate_event(entries: &[(Expr, Expr)]) -> Result<()> {
    reject_duplicate_bare_fields(
        entries,
        "chat transcript",
        &[
            "model-event",
            "event",
            "runner",
            "model",
            "span-id",
            "tool-call",
            "tool-result",
            "usage",
            "response",
            "raw-provider-response",
        ],
    )?;
    require_symbol_field(entries, "event")?;
    require_symbol_field(entries, "runner")?;
    require_string_field(entries, "model")?;
    require_field(entries, "span-id")?;
    if let Some(tool_call) = entry_field(entries, "tool-call") {
        validate_projected_expr(tool_call, "chat tool-call event")?;
    }
    if let Some(tool_result) = entry_field(entries, "tool-result") {
        validate_projected_expr(tool_result, "chat tool-result event")?;
    }
    validate_optional_usage(entries)?;
    if let Some(response) = entry_field(entries, "response") {
        validate_chat_transcript(response)?;
    }
    validate_optional_projected_field(entries, "raw-provider-response")?;
    Ok(())
}

fn validate_card(entries: &[(Expr, Expr)]) -> Result<()> {
    reject_duplicate_bare_fields(
        entries,
        "chat transcript",
        &["model-card", "runner", "model", "provider", "locality"],
    )?;
    require_symbol_field(entries, "runner")?;
    require_string_field(entries, "model")?;
    require_symbol_field(entries, "provider")?;
    require_symbol_field(entries, "locality")?;
    Ok(())
}

fn validate_message(expr: &Expr) -> Result<()> {
    let entries = require_strict_map(expr, "chat message")?;
    require_symbol_field(entries, "role")?;
    validate_content_list(entries, "content")
}

fn validate_content_list(entries: &[(Expr, Expr)], field_name: &'static str) -> Result<()> {
    let content = require_list_field(entries, field_name)?;
    for part in content {
        validate_content_part(part)?;
    }
    Ok(())
}

fn validate_content_part(expr: &Expr) -> Result<()> {
    let entries = require_strict_map(expr, "chat content part")?;
    let kind = require_symbol_field_any(entries, "type")?;
    if kind.namespace.is_some() {
        validate_optional_projected_field(entries, "raw-provider-part")
    } else {
        match kind.name.as_ref() {
            "text" => {
                require_string_field_any(entries, "text")?;
                Ok(())
            }
            "tool-call" => validate_tool_call(entries),
            "tool-result" => validate_tool_result(entries),
            _ => validate_optional_projected_field(entries, "raw-provider-part"),
        }
    }
}

fn validate_tool_call(entries: &[(Expr, Expr)]) -> Result<()> {
    require_string_field_any(entries, "id")?;
    require_string_field_any(entries, "name")?;
    validate_projected_expr(
        require_field_any(entries, "arguments")?,
        "chat tool-call arguments",
    )
}

fn validate_tool_result(entries: &[(Expr, Expr)]) -> Result<()> {
    require_string_field_any(entries, "tool-call-id")?;
    require_symbol_field_any(entries, "status")?;
    validate_projected_expr(
        require_field_any(entries, "output")?,
        "chat tool-result output",
    )
}

fn validate_optional_usage(entries: &[(Expr, Expr)]) -> Result<()> {
    if let Some(usage) = entry_field(entries, "usage") {
        validate_projected_expr(usage, "chat usage")?;
    }
    Ok(())
}

fn validate_optional_projected_field(entries: &[(Expr, Expr)], name: &'static str) -> Result<()> {
    if let Some(value) = entry_field(entries, name) {
        validate_projected_expr(value, name)?;
    }
    Ok(())
}

fn validate_projected_expr(expr: &Expr, context: &str) -> Result<()> {
    match expr {
        Expr::Map(entries) => {
            reject_duplicate_bare_keys(entries, context)?;
            for (_, value) in entries {
                validate_projected_expr(value, context)?;
            }
        }
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            for item in items {
                validate_projected_expr(item, context)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn require_map<'a>(expr: &'a Expr, context: &str) -> Result<&'a [(Expr, Expr)]> {
    match expr {
        Expr::Map(entries) => Ok(entries),
        _ => Err(chat_eval(format!("{context} must be an Expr::Map"))),
    }
}

fn require_strict_map<'a>(expr: &'a Expr, context: &str) -> Result<&'a [(Expr, Expr)]> {
    match expr {
        Expr::Map(entries) => {
            reject_duplicate_bare_keys(entries, context)?;
            Ok(entries)
        }
        _ => Err(chat_eval(format!("{context} must be an Expr::Map"))),
    }
}

fn reject_duplicate_bare_fields(
    entries: &[(Expr, Expr)],
    context: &str,
    owned: &[&str],
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (key, _) in entries {
        if let Expr::Symbol(symbol) = key
            && symbol.namespace.is_none()
            && owned.contains(&symbol.name.as_ref())
            && !seen.insert(symbol.name.as_ref())
        {
            return Err(chat_eval(format!(
                "{context} duplicate {} field",
                symbol.name.as_ref()
            )));
        }
    }
    Ok(())
}

fn reject_duplicate_bare_keys(entries: &[(Expr, Expr)], context: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (key, _) in entries {
        if let Expr::Symbol(symbol) = key
            && symbol.namespace.is_none()
            && !seen.insert(symbol.name.as_ref())
        {
            return Err(chat_eval(format!(
                "{context} duplicate {} field",
                symbol.name.as_ref()
            )));
        }
    }
    Ok(())
}

fn require_field<'a>(entries: &'a [(Expr, Expr)], name: &'static str) -> Result<&'a Expr> {
    entry_field(entries, name)
        .ok_or_else(|| chat_eval(format!("chat transcript missing {name} field")))
}

fn require_field_any<'a>(entries: &'a [(Expr, Expr)], name: &'static str) -> Result<&'a Expr> {
    entry_field_any(entries, name)
        .ok_or_else(|| chat_eval(format!("chat transcript missing {name} field")))
}

fn require_symbol_field<'a>(entries: &'a [(Expr, Expr)], name: &'static str) -> Result<&'a Symbol> {
    match require_field(entries, name)? {
        Expr::Symbol(symbol) => Ok(symbol),
        _ => Err(chat_eval(format!(
            "chat transcript {name} field must be a symbol"
        ))),
    }
}

fn require_symbol_field_any<'a>(
    entries: &'a [(Expr, Expr)],
    name: &'static str,
) -> Result<&'a Symbol> {
    match require_field_any(entries, name)? {
        Expr::Symbol(symbol) => Ok(symbol),
        _ => Err(chat_eval(format!(
            "chat transcript {name} field must be a symbol"
        ))),
    }
}

fn require_string_field<'a>(entries: &'a [(Expr, Expr)], name: &'static str) -> Result<&'a str> {
    match require_field(entries, name)? {
        Expr::String(text) => Ok(text),
        _ => Err(chat_eval(format!(
            "chat transcript {name} field must be a string"
        ))),
    }
}

fn require_string_field_any<'a>(
    entries: &'a [(Expr, Expr)],
    name: &'static str,
) -> Result<&'a str> {
    match require_field_any(entries, name)? {
        Expr::String(text) => Ok(text),
        _ => Err(chat_eval(format!(
            "chat transcript {name} field must be a string"
        ))),
    }
}

fn require_list_field<'a>(entries: &'a [(Expr, Expr)], name: &'static str) -> Result<&'a [Expr]> {
    match require_field(entries, name)? {
        Expr::List(items) => Ok(items),
        _ => Err(chat_eval(format!(
            "chat transcript {name} field must be a list"
        ))),
    }
}

fn marker_is_true(expr: &Expr, name: &str) -> bool {
    matches!(expr_field(expr, name), Some(Expr::Bool(true)))
}

fn marker_is_true_in(entries: &[(Expr, Expr)], name: &str) -> bool {
    matches!(entry_field(entries, name), Some(Expr::Bool(true)))
}

fn key_bool(name: &str, value: bool) -> (Expr, Expr) {
    key_expr(name, Expr::Bool(value))
}

fn key_expr(name: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(name.to_owned())), value)
}

fn chat_eval(message: impl Into<String>) -> Error {
    Error::Eval(message.into())
}
