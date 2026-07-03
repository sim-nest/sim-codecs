//! Encode and decode the ABI byte frames exchanged with a wasm guest: value,
//! manifest, and export frames, built on the shared binary frame codec.

use crate::types::{AbiValue, Frame, Handle, WasmDependency, WasmExport, WasmManifest};
use sim_codec_binary::{decode_frame, encode_frame};
use sim_kernel::{AbiVersion, Error, Expr, LibTarget, Result, Symbol};

/// Encodes an [`AbiValue`] into a value byte frame via the binary codec.
///
/// # Examples
///
/// ```
/// use sim_wasm_abi::{AbiValue, decode_value_frame, encode_value_frame};
/// use sim_kernel::Expr;
///
/// let value = AbiValue::Expr(Expr::String("ok".to_owned()));
/// let frame = encode_value_frame(&value).unwrap();
/// assert_eq!(decode_value_frame(&frame).unwrap(), value);
/// ```
pub fn encode_value_frame(value: &AbiValue) -> Result<Frame> {
    Ok(Frame::new(encode_frame(&value_to_expr(value))?.0))
}

/// Decodes a value byte frame back into an [`AbiValue`].
pub fn decode_value_frame(frame: &Frame) -> Result<AbiValue> {
    let (_, expr) = decode_frame(sim_kernel::CodecId(0), frame.bytes())?;
    expr_to_value(expr)
}

/// Encodes a [`WasmManifest`] into a manifest byte frame.
///
/// # Examples
///
/// ```
/// use sim_wasm_abi::{WasmManifest, decode_manifest_frame, encode_manifest_frame};
/// use sim_kernel::{AbiVersion, LibTarget, Symbol};
///
/// let manifest = WasmManifest {
///     id: Symbol::new("geometry"),
///     version: "0.1.0".to_owned(),
///     abi: AbiVersion { major: 0, minor: 1 },
///     target: LibTarget::WasmComponent,
///     requires: Vec::new(),
///     capabilities: Vec::new(),
///     exports: Vec::new(),
/// };
/// let frame = encode_manifest_frame(&manifest).unwrap();
/// assert_eq!(decode_manifest_frame(&frame).unwrap(), manifest);
/// ```
pub fn encode_manifest_frame(manifest: &WasmManifest) -> Result<Frame> {
    encode_value_frame(&AbiValue::Expr(manifest_to_expr(manifest)))
}

/// Decodes a manifest byte frame back into a [`WasmManifest`].
///
/// Errors if the frame does not carry an expr payload.
pub fn decode_manifest_frame(frame: &Frame) -> Result<WasmManifest> {
    let AbiValue::Expr(expr) = decode_value_frame(frame)? else {
        return Err(Error::TypeMismatch {
            expected: "manifest expr frame",
            found: "non-expr frame",
        });
    };
    expr_to_manifest(expr)
}

/// Encodes a slice of [`WasmExport`] into an export byte frame.
pub fn encode_exports_frame(exports: &[WasmExport]) -> Result<Frame> {
    let expr = Expr::List(exports.iter().map(export_to_expr).collect());
    encode_value_frame(&AbiValue::Expr(expr))
}

/// Decodes an export byte frame back into a list of [`WasmExport`].
///
/// Errors if the frame does not carry an expr list payload.
pub fn decode_exports_frame(frame: &Frame) -> Result<Vec<WasmExport>> {
    let AbiValue::Expr(expr) = decode_value_frame(frame)? else {
        return Err(Error::TypeMismatch {
            expected: "export expr frame",
            found: "non-expr frame",
        });
    };
    let Expr::List(items) = expr else {
        return Err(Error::TypeMismatch {
            expected: "export list",
            found: "non-list expr",
        });
    };
    items.into_iter().map(expr_to_export).collect()
}

fn value_to_expr(value: &AbiValue) -> Expr {
    match value {
        AbiValue::Expr(expr) => Expr::Extension {
            tag: Symbol::qualified("wasm", "expr"),
            payload: Box::new(expr.clone()),
        },
        AbiValue::Handle(handle) => Expr::Extension {
            tag: Symbol::qualified("wasm", "handle"),
            payload: Box::new(Expr::String(handle.0.to_string())),
        },
        AbiValue::Error(message) => Expr::Extension {
            tag: Symbol::qualified("wasm", "error"),
            payload: Box::new(Expr::String(message.clone())),
        },
    }
}

fn expr_to_value(expr: Expr) -> Result<AbiValue> {
    let Expr::Extension { tag, payload } = expr else {
        return Err(Error::TypeMismatch {
            expected: "wasm ABI extension",
            found: "non-extension expr",
        });
    };
    match (tag.namespace.as_deref(), tag.name.as_ref(), *payload) {
        (Some("wasm"), "expr", payload) => Ok(AbiValue::Expr(payload)),
        (Some("wasm"), "handle", Expr::String(value)) => {
            Ok(AbiValue::Handle(Handle(value.parse::<u64>().map_err(
                |err| Error::HostError(format!("invalid handle payload: {err}")),
            )?)))
        }
        (Some("wasm"), "error", Expr::String(message)) => Ok(AbiValue::Error(message)),
        _ => Err(Error::TypeMismatch {
            expected: "known wasm ABI extension",
            found: "unknown extension payload",
        }),
    }
}

fn manifest_to_expr(manifest: &WasmManifest) -> Expr {
    Expr::Map(vec![
        symbol_entry("id", Expr::Symbol(manifest.id.clone())),
        symbol_entry("version", Expr::String(manifest.version.clone())),
        symbol_entry("abi-major", number_expr(manifest.abi.major)),
        symbol_entry("abi-minor", number_expr(manifest.abi.minor)),
        symbol_entry(
            "target",
            Expr::String(lib_target_name(&manifest.target).to_owned()),
        ),
        symbol_entry(
            "requires",
            Expr::List(manifest.requires.iter().map(dependency_to_expr).collect()),
        ),
        symbol_entry(
            "capabilities",
            Expr::List(
                manifest
                    .capabilities
                    .iter()
                    .cloned()
                    .map(Expr::String)
                    .collect(),
            ),
        ),
        symbol_entry(
            "exports",
            Expr::List(manifest.exports.iter().map(export_to_expr).collect()),
        ),
    ])
}

fn expr_to_manifest(expr: Expr) -> Result<WasmManifest> {
    Ok(WasmManifest {
        id: expect_symbol_field(&expr, "id")?,
        version: expect_string_field(&expr, "version")?,
        abi: AbiVersion {
            major: expect_u16_field(&expr, "abi-major")?,
            minor: expect_u16_field(&expr, "abi-minor")?,
        },
        target: parse_lib_target(&expect_string_field(&expr, "target")?)?,
        requires: expect_list_field(&expr, "requires")?
            .into_iter()
            .map(expr_to_dependency)
            .collect::<Result<Vec<_>>>()?,
        capabilities: expect_list_field(&expr, "capabilities")?
            .into_iter()
            .map(|expr| match expr {
                Expr::String(value) => Ok(value),
                _ => Err(Error::TypeMismatch {
                    expected: "capability string",
                    found: "non-string capability",
                }),
            })
            .collect::<Result<Vec<_>>>()?,
        exports: expect_list_field(&expr, "exports")?
            .into_iter()
            .map(expr_to_export)
            .collect::<Result<Vec<_>>>()?,
    })
}

fn dependency_to_expr(dependency: &WasmDependency) -> Expr {
    Expr::Map(vec![
        symbol_entry("id", Expr::Symbol(dependency.id.clone())),
        symbol_entry(
            "minimum-version",
            dependency
                .minimum_version
                .clone()
                .map(Expr::String)
                .unwrap_or(Expr::Nil),
        ),
    ])
}

fn expr_to_dependency(expr: Expr) -> Result<WasmDependency> {
    let minimum_version = field_value(&expr, "minimum-version")?;
    Ok(WasmDependency {
        id: expect_symbol_field(&expr, "id")?,
        minimum_version: match minimum_version {
            Expr::Nil => None,
            Expr::String(value) => Some(value),
            _ => {
                return Err(Error::TypeMismatch {
                    expected: "string or nil minimum-version",
                    found: "non-string minimum-version",
                });
            }
        },
    })
}

fn export_to_expr(export: &WasmExport) -> Expr {
    match export {
        WasmExport::Function { symbol } => Expr::Map(vec![
            symbol_entry("kind", Expr::String("function".to_owned())),
            symbol_entry("symbol", Expr::Symbol(symbol.clone())),
        ]),
        WasmExport::Macro { symbol } => Expr::Map(vec![
            symbol_entry("kind", Expr::String("macro".to_owned())),
            symbol_entry("symbol", Expr::Symbol(symbol.clone())),
        ]),
        WasmExport::Class {
            symbol,
            constructor,
        } => Expr::Map(vec![
            symbol_entry("kind", Expr::String("class".to_owned())),
            symbol_entry("symbol", Expr::Symbol(symbol.clone())),
            symbol_entry(
                "constructor",
                constructor.clone().map(Expr::Symbol).unwrap_or(Expr::Nil),
            ),
        ]),
        WasmExport::Codec { symbol } => Expr::Map(vec![
            symbol_entry("kind", Expr::String("codec".to_owned())),
            symbol_entry("symbol", Expr::Symbol(symbol.clone())),
        ]),
        WasmExport::Shape { symbol } => Expr::Map(vec![
            symbol_entry("kind", Expr::String("shape".to_owned())),
            symbol_entry("symbol", Expr::Symbol(symbol.clone())),
        ]),
        WasmExport::NumberDomain { symbol } => Expr::Map(vec![
            symbol_entry("kind", Expr::String("number-domain".to_owned())),
            symbol_entry("symbol", Expr::Symbol(symbol.clone())),
        ]),
        WasmExport::Site { symbol } => Expr::Map(vec![
            symbol_entry("kind", Expr::String("site".to_owned())),
            symbol_entry("symbol", Expr::Symbol(symbol.clone())),
        ]),
        WasmExport::Value { symbol } => Expr::Map(vec![
            symbol_entry("kind", Expr::String("value".to_owned())),
            symbol_entry("symbol", Expr::Symbol(symbol.clone())),
        ]),
    }
}

fn expr_to_export(expr: Expr) -> Result<WasmExport> {
    let kind = expect_string_field(&expr, "kind")?;
    let symbol = expect_symbol_field(&expr, "symbol")?;
    match kind.as_str() {
        "function" => Ok(WasmExport::Function { symbol }),
        "macro" => Ok(WasmExport::Macro { symbol }),
        "class" => {
            let constructor = field_value(&expr, "constructor")?;
            Ok(WasmExport::Class {
                symbol,
                constructor: match constructor {
                    Expr::Nil => None,
                    Expr::Symbol(symbol) => Some(symbol),
                    _ => {
                        return Err(Error::TypeMismatch {
                            expected: "class constructor symbol or nil",
                            found: "non-symbol constructor",
                        });
                    }
                },
            })
        }
        "codec" => Ok(WasmExport::Codec { symbol }),
        "shape" => Ok(WasmExport::Shape { symbol }),
        "number-domain" => Ok(WasmExport::NumberDomain { symbol }),
        "site" => Ok(WasmExport::Site { symbol }),
        "value" => Ok(WasmExport::Value { symbol }),
        other => Err(Error::HostError(format!(
            "unknown wasm export kind {other}"
        ))),
    }
}

fn symbol_entry(name: &str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(name)), value)
}

fn number_expr(value: impl ToString) -> Expr {
    Expr::String(value.to_string())
}

fn lib_target_name(target: &LibTarget) -> String {
    // Map through the kernel's open symbol form: closed variants render as their
    // unqualified tag, `CodecSource(sym)` as the codec symbol verbatim.
    target.to_symbol().as_qualified_str()
}

fn parse_lib_target(name: &str) -> Result<LibTarget> {
    let symbol = match name.split_once('/') {
        Some((namespace, local)) => Symbol::qualified(namespace.to_owned(), local.to_owned()),
        None => Symbol::new(name.to_owned()),
    };
    Ok(LibTarget::from_symbol(&symbol))
}

fn field_value(expr: &Expr, field: &str) -> Result<Expr> {
    let Expr::Map(entries) = expr else {
        return Err(Error::TypeMismatch {
            expected: "map expr",
            found: "non-map expr",
        });
    };
    entries
        .iter()
        .find_map(|(key, value)| match key {
            Expr::Symbol(symbol) if symbol == &Symbol::new(field) => Some(value.clone()),
            _ => None,
        })
        .ok_or_else(|| Error::HostError(format!("missing field {field}")))
}

fn expect_string_field(expr: &Expr, field: &str) -> Result<String> {
    match field_value(expr, field)? {
        Expr::String(value) => Ok(value),
        _ => Err(Error::TypeMismatch {
            expected: "string field",
            found: "non-string field",
        }),
    }
}

fn expect_symbol_field(expr: &Expr, field: &str) -> Result<Symbol> {
    match field_value(expr, field)? {
        Expr::Symbol(value) => Ok(value),
        _ => Err(Error::TypeMismatch {
            expected: "symbol field",
            found: "non-symbol field",
        }),
    }
}

fn expect_u16_field(expr: &Expr, field: &str) -> Result<u16> {
    match field_value(expr, field)? {
        Expr::String(value) => value
            .parse::<u16>()
            .map_err(|err| Error::HostError(format!("invalid u16 field {field}: {err}"))),
        _ => Err(Error::TypeMismatch {
            expected: "u16 string field",
            found: "non-string numeric field",
        }),
    }
}

fn expect_list_field(expr: &Expr, field: &str) -> Result<Vec<Expr>> {
    match field_value(expr, field)? {
        Expr::List(values) => Ok(values),
        _ => Err(Error::TypeMismatch {
            expected: "list field",
            found: "non-list field",
        }),
    }
}
