use sim_kernel::{Error, Expr, Result, Symbol};
use sim_value::build::entry;

/// Media kind carried by a packed ASK call argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallArgumentMedia {
    /// The declared codec produced text.
    Text,
    /// The declared codec produced bytes that were rendered as text inside a fence.
    Bytes,
}

impl CallArgumentMedia {
    /// Returns the stable media symbol.
    pub fn symbol(self) -> Symbol {
        match self {
            Self::Text => Symbol::qualified("bridge", "Text"),
            Self::Bytes => Symbol::qualified("bridge", "Bytes"),
        }
    }

    fn from_symbol(symbol: &Symbol) -> Result<Self> {
        if symbol == &Symbol::qualified("bridge", "Text") {
            Ok(Self::Text)
        } else if symbol == &Symbol::qualified("bridge", "Bytes") {
            Ok(Self::Bytes)
        } else {
            Err(Error::Eval(format!(
                "unknown BRIDGE call argument media {symbol}"
            )))
        }
    }
}

/// One packed argument in a `bridge/Call` payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeCallArgument {
    /// Argument name.
    pub name: Symbol,
    /// Codec for encoding the original value at data position.
    pub codec: Symbol,
    /// Encoded media kind.
    pub media: CallArgumentMedia,
    /// Content id of the pre-fence encoded datum.
    pub content_id: String,
    /// Fence-wrapped encoded argument text shown to the model.
    pub fenced: String,
}

impl BridgeCallArgument {
    /// Builds a packed call argument.
    pub fn new(
        name: Symbol,
        codec: Symbol,
        media: CallArgumentMedia,
        content_id: String,
        fenced: String,
    ) -> Self {
        Self {
            name,
            codec,
            media,
            content_id,
            fenced,
        }
    }

    /// Decodes a call argument expression.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/Call argument")?;
        reject_unknown(fields, &["name", "codec", "media", "content-id", "fenced"])?;
        let media = CallArgumentMedia::from_symbol(required_symbol(fields, "media")?)?;
        Ok(Self::new(
            required_symbol(fields, "name")?.clone(),
            required_symbol(fields, "codec")?.clone(),
            media,
            required_string(fields, "content-id")?.to_owned(),
            required_string(fields, "fenced")?.to_owned(),
        ))
    }

    /// Encodes this call argument as a canonical expression map.
    pub fn to_expr(&self) -> Expr {
        Expr::Map(vec![
            entry("name", Expr::Symbol(self.name.clone())),
            entry("codec", Expr::Symbol(self.codec.clone())),
            entry("media", Expr::Symbol(self.media.symbol())),
            entry("content-id", Expr::String(self.content_id.clone())),
            entry("fenced", Expr::String(self.fenced.clone())),
        ])
    }
}

/// Typed payload for a `bridge/Call` part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeCallPayload {
    /// Call name.
    pub name: Symbol,
    /// Packed arguments.
    pub args: Vec<BridgeCallArgument>,
    /// Model parameters covered by the replay key.
    pub model_params: Vec<(Symbol, Expr)>,
}

impl BridgeCallPayload {
    /// Builds an empty call payload for `name`.
    pub fn new(name: Symbol) -> Self {
        Self {
            name,
            args: Vec::new(),
            model_params: Vec::new(),
        }
    }

    /// Adds a packed argument.
    pub fn with_arg(mut self, arg: BridgeCallArgument) -> Self {
        self.args.push(arg);
        self
    }

    /// Adds a model parameter.
    pub fn with_model_param(mut self, name: Symbol, value: Expr) -> Self {
        self.model_params.push((name, value));
        self
    }

    /// Decodes a call payload expression.
    pub fn from_expr(expr: &Expr) -> Result<Self> {
        let fields = map_fields(expr, "bridge/Call payload")?;
        reject_unknown(fields, &["name", "args", "model-params"])?;
        Ok(Self {
            name: required_symbol(fields, "name")?.clone(),
            args: required_vector(fields, "args")?
                .iter()
                .map(BridgeCallArgument::from_expr)
                .collect::<Result<Vec<_>>>()?,
            model_params: optional_symbol_map(fields, "model-params")?,
        })
    }

    /// Encodes this payload as a canonical expression map.
    pub fn to_expr(&self) -> Expr {
        let mut fields = vec![
            entry("name", Expr::Symbol(self.name.clone())),
            entry(
                "args",
                Expr::Vector(self.args.iter().map(BridgeCallArgument::to_expr).collect()),
            ),
        ];
        if !self.model_params.is_empty() {
            fields.push(entry(
                "model-params",
                Expr::Map(
                    self.model_params
                        .iter()
                        .map(|(name, value)| (Expr::Symbol(name.clone()), value.clone()))
                        .collect(),
                ),
            ));
        }
        Expr::Map(fields)
    }
}

/// Parses and validates a `bridge/Call` payload.
pub fn validate_call_payload(payload: &Expr) -> Result<BridgeCallPayload> {
    let payload = BridgeCallPayload::from_expr(payload)?;
    for arg in &payload.args {
        validate_arg(arg)?;
    }
    Ok(payload)
}

fn validate_arg(arg: &BridgeCallArgument) -> Result<()> {
    if arg.content_id.trim().is_empty() {
        return Err(Error::Eval(format!(
            "BRIDGE call argument {} is missing a content id",
            arg.name
        )));
    }
    if !arg.fenced.contains("<sim-data-") || !arg.fenced.contains("</sim-data-") {
        return Err(Error::Eval(format!(
            "BRIDGE call argument {} must be fence-wrapped",
            arg.name
        )));
    }
    Ok(())
}

fn map_fields<'a>(expr: &'a Expr, label: &str) -> Result<&'a [(Expr, Expr)]> {
    match expr {
        Expr::Map(fields) => Ok(fields),
        _ => Err(Error::Eval(format!("{label} must be a map"))),
    }
}

fn reject_unknown(fields: &[(Expr, Expr)], allowed: &[&str]) -> Result<()> {
    for (key, _) in fields {
        let Some(name) = field_name(key) else {
            return Err(Error::Eval(
                "BRIDGE call field keys must be symbols".to_owned(),
            ));
        };
        if !allowed.contains(&name.as_str()) {
            return Err(Error::Eval(format!("unknown BRIDGE call field {name}")));
        }
    }
    Ok(())
}

fn required_field<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a Expr> {
    fields
        .iter()
        .find_map(|(key, value)| (field_name(key).as_deref() == Some(name)).then_some(value))
        .ok_or_else(|| Error::Eval(format!("BRIDGE call record is missing {name}")))
}

fn required_symbol<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a Symbol> {
    match required_field(fields, name)? {
        Expr::Symbol(symbol) => Ok(symbol),
        _ => Err(Error::TypeMismatch {
            expected: "symbol",
            found: "non-symbol",
        }),
    }
}

fn required_string<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a str> {
    match required_field(fields, name)? {
        Expr::String(value) => Ok(value),
        _ => Err(Error::TypeMismatch {
            expected: "string",
            found: "non-string",
        }),
    }
}

fn required_vector<'a>(fields: &'a [(Expr, Expr)], name: &str) -> Result<&'a [Expr]> {
    match required_field(fields, name)? {
        Expr::Vector(items) | Expr::List(items) => Ok(items),
        _ => Err(Error::Eval(format!(
            "BRIDGE call {name} field must be a vector"
        ))),
    }
}

fn optional_symbol_map(fields: &[(Expr, Expr)], name: &str) -> Result<Vec<(Symbol, Expr)>> {
    let Some(value) = fields
        .iter()
        .find_map(|(key, value)| (field_name(key).as_deref() == Some(name)).then_some(value))
    else {
        return Ok(Vec::new());
    };
    let Expr::Map(entries) = value else {
        return Err(Error::Eval(format!(
            "BRIDGE call {name} field must be a map"
        )));
    };
    entries
        .iter()
        .map(|(key, value)| match key {
            Expr::Symbol(symbol) => Ok((symbol.clone(), value.clone())),
            _ => Err(Error::Eval(
                "BRIDGE call model parameter keys must be symbols".to_owned(),
            )),
        })
        .collect()
}

fn field_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Symbol(symbol) => Some(symbol.name.to_string()),
        Expr::String(value) => Some(value.clone()),
        _ => None,
    }
}
