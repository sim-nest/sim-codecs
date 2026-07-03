//! Constructors and validators for chat transcript `Expr` values (model
//! request, response, error, and card maps) plus the shared
//! `validate_chat_transcript` shape check used by the codec.

use sim_kernel::{Error, Expr, Result, Symbol};

/// Returns `true` when `expr` is a chat transcript map carrying a true
/// `model-request` marker.
pub fn is_model_request_expr(expr: &Expr) -> bool {
    marker_is_true(expr, "model-request")
}

/// Returns the `messages` list of a model-request transcript, erroring if
/// `expr` is not a valid request or its `messages` field is not a list.
pub fn model_request_messages_expr(expr: &Expr) -> Result<&[Expr]> {
    validate_request(expr)?;
    match field(expr, "messages") {
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
    let markers = [
        marker_is_true(expr, "model-request"),
        marker_is_true(expr, "model-response"),
        marker_is_true(expr, "model-event"),
        marker_is_true(expr, "model-card"),
    ]
    .into_iter()
    .filter(|value| *value)
    .count();

    if markers != 1 {
        return Err(chat_eval(
            "chat transcript must have exactly one true model-request, model-response, model-event, or model-card marker",
        ));
    }

    if marker_is_true(expr, "model-request") {
        validate_request(expr)
    } else if marker_is_true(expr, "model-response") {
        validate_response(expr)
    } else if marker_is_true(expr, "model-event") {
        validate_event(expr)
    } else {
        validate_card(expr)
    }
}

fn validate_request(expr: &Expr) -> Result<()> {
    require_map(expr)?;
    require_field(expr, "task")?;
    require_list_field(expr, "messages")?;
    if let Some(Expr::List(messages)) = field(expr, "messages") {
        for message in messages {
            validate_message(message)?;
        }
    }
    Ok(())
}

fn validate_response(expr: &Expr) -> Result<()> {
    require_symbol_field(expr, "runner")?;
    require_string_field(expr, "model")?;
    require_symbol_field(expr, "stop-reason")?;
    validate_content_list(expr, "content")
}

fn validate_event(expr: &Expr) -> Result<()> {
    require_symbol_field(expr, "event")?;
    require_symbol_field(expr, "runner")?;
    require_string_field(expr, "model")?;
    require_field(expr, "span-id")?;
    Ok(())
}

fn validate_card(expr: &Expr) -> Result<()> {
    require_symbol_field(expr, "runner")?;
    require_string_field(expr, "model")?;
    require_symbol_field(expr, "provider")?;
    require_symbol_field(expr, "locality")?;
    Ok(())
}

fn validate_message(expr: &Expr) -> Result<()> {
    require_symbol_field(expr, "role")?;
    validate_content_list(expr, "content")
}

fn validate_content_list(expr: &Expr, field_name: &'static str) -> Result<()> {
    let content = require_list_field(expr, field_name)?;
    for part in content {
        require_symbol_field(part, "type")?;
    }
    Ok(())
}

fn require_map(expr: &Expr) -> Result<&[(Expr, Expr)]> {
    match expr {
        Expr::Map(entries) => Ok(entries),
        _ => Err(chat_eval("chat transcript must be an Expr::Map")),
    }
}

fn require_field<'a>(expr: &'a Expr, name: &'static str) -> Result<&'a Expr> {
    field(expr, name).ok_or_else(|| chat_eval(format!("chat transcript missing {name} field")))
}

fn require_symbol_field<'a>(expr: &'a Expr, name: &'static str) -> Result<&'a Symbol> {
    match require_field(expr, name)? {
        Expr::Symbol(symbol) => Ok(symbol),
        _ => Err(chat_eval(format!(
            "chat transcript {name} field must be a symbol"
        ))),
    }
}

fn require_string_field<'a>(expr: &'a Expr, name: &'static str) -> Result<&'a str> {
    match require_field(expr, name)? {
        Expr::String(text) => Ok(text),
        _ => Err(chat_eval(format!(
            "chat transcript {name} field must be a string"
        ))),
    }
}

fn require_list_field<'a>(expr: &'a Expr, name: &'static str) -> Result<&'a [Expr]> {
    match require_field(expr, name)? {
        Expr::List(items) => Ok(items),
        _ => Err(chat_eval(format!(
            "chat transcript {name} field must be a list"
        ))),
    }
}

use sim_value::access::field;

fn marker_is_true(expr: &Expr, name: &str) -> bool {
    matches!(field(expr, name), Some(Expr::Bool(true)))
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
