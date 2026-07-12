//! A generic `#(...)` domain-form parser and formatter.
//!
//! Several domain codecs (`music-shapes`, `sound-shapes`, ...) hand-rolled the
//! same `#(Name key=value value [list,...])` reader. This is that grammar, with
//! no domain knowledge:
//!
//! ```text
//! form  = "#(" name item* ")"
//! item  = key "=" value | value
//! value = form | "[" value ("," value)* "]" | string | atom
//! ```
//!
//! Output is ASCII. A domain crate parses with [`parse_domain_form`], reads
//! fields with [`DomainForm::atom`]/[`string`](DomainForm::string)/
//! [`list`](DomainForm::list), and (optionally) renders with
//! [`format_domain_form`].

use sim_kernel::{Expr, Symbol};

/// A parsed domain-form value.
#[derive(Clone, Debug, PartialEq)]
pub enum DomainValue {
    /// A nested `#(...)` form.
    Form(DomainForm),
    /// A `[...]` list.
    List(Vec<DomainValue>),
    /// A `"..."` string.
    String(String),
    /// A bare atom (number, identifier, `4/4`, ...).
    Atom(String),
}

/// A parsed `#(name ...)` form: a name, keyed fields, and positional values.
#[derive(Clone, Debug, PartialEq)]
pub struct DomainForm {
    /// The form name.
    pub name: String,
    /// Keyed `key=value` fields, in order.
    pub fields: Vec<(String, DomainValue)>,
    /// Positional (un-keyed) values, in order.
    pub positional: Vec<DomainValue>,
}

impl DomainForm {
    /// The value of keyed field `key`, if present.
    pub fn field(&self, key: &str) -> Option<&DomainValue> {
        self.fields
            .iter()
            .find_map(|(name, value)| (name == key).then_some(value))
    }

    /// The atom string of keyed field `key`.
    pub fn atom(&self, key: &str) -> Result<&str, DomainFormError> {
        match self.field(key) {
            Some(DomainValue::Atom(value)) => Ok(value),
            Some(_) => Err(DomainFormError::WrongFieldKind(key.to_owned())),
            None => Err(DomainFormError::MissingField(key.to_owned())),
        }
    }

    /// The string of keyed field `key`.
    pub fn string(&self, key: &str) -> Result<&str, DomainFormError> {
        match self.field(key) {
            Some(DomainValue::String(value)) => Ok(value),
            Some(_) => Err(DomainFormError::WrongFieldKind(key.to_owned())),
            None => Err(DomainFormError::MissingField(key.to_owned())),
        }
    }

    /// The list items of keyed field `key`.
    pub fn list(&self, key: &str) -> Result<&[DomainValue], DomainFormError> {
        match self.field(key) {
            Some(DomainValue::List(items)) => Ok(items),
            Some(_) => Err(DomainFormError::WrongFieldKind(key.to_owned())),
            None => Err(DomainFormError::MissingField(key.to_owned())),
        }
    }

    /// The nested form of keyed field `key`.
    pub fn form(&self, key: &str) -> Result<&DomainForm, DomainFormError> {
        match self.field(key) {
            Some(DomainValue::Form(value)) => Ok(value),
            Some(_) => Err(DomainFormError::WrongFieldKind(key.to_owned())),
            None => Err(DomainFormError::MissingField(key.to_owned())),
        }
    }

    /// The atom or string text of keyed field `key`.
    ///
    /// Accepts either a bare atom or a quoted string, returning the underlying
    /// text in both cases (the reverse of the `#(...)` writer, which renders a
    /// symbol as an atom and a string with quotes).
    pub fn field_atom_or_string(&self, name: &str) -> Result<&str, DomainFormError> {
        match self.field(name) {
            Some(DomainValue::Atom(value) | DomainValue::String(value)) => Ok(value),
            Some(_) => Err(DomainFormError::WrongFieldKind(name.to_owned())),
            None => Err(DomainFormError::MissingField(name.to_owned())),
        }
    }

    /// The rendered [`render_text`](DomainValue::render_text) form of keyed
    /// field `name`.
    ///
    /// Errors with [`MissingField`](DomainFormError::MissingField) when the
    /// field is absent; any present value renders (an atom to its text, a
    /// string to its quoted form, a list or nested form to its `#(...)`/`[...]`
    /// text).
    pub fn field_text(&self, name: &str) -> Result<String, DomainFormError> {
        match self.field(name) {
            Some(value) => Ok(value.render_text()),
            None => Err(DomainFormError::MissingField(name.to_owned())),
        }
    }

    /// Project this parsed form to an [`Expr::Map`] for shape validation.
    ///
    /// The map always contains `form` with the form name. Positional values are
    /// carried as `args` only when present. Keyed fields keep their field names
    /// as symbol keys, and values are projected with [`DomainValue::to_expr`].
    pub fn to_expr_map(&self) -> Expr {
        let mut entries = vec![(
            Expr::Symbol(Symbol::new("form")),
            Expr::String(self.name.clone()),
        )];

        if !self.positional.is_empty() {
            entries.push((
                Expr::Symbol(Symbol::new("args")),
                Expr::List(self.positional.iter().map(DomainValue::to_expr).collect()),
            ));
        }

        entries.extend(
            self.fields
                .iter()
                .map(|(key, value)| (Expr::Symbol(Symbol::new(key.clone())), value.to_expr())),
        );

        Expr::Map(entries)
    }
}

impl DomainValue {
    /// The nested form, if this value is a `#(...)` form.
    ///
    /// Errors with [`WrongValueKind`](DomainFormError::WrongValueKind) for a
    /// list, string, or atom.
    pub fn as_form(&self) -> Result<&DomainForm, DomainFormError> {
        match self {
            DomainValue::Form(form) => Ok(form),
            _ => Err(DomainFormError::WrongValueKind),
        }
    }

    /// The text of this value when it is a bare atom or a quoted string.
    ///
    /// Errors with [`WrongValueKind`](DomainFormError::WrongValueKind) for a
    /// list or nested form.
    pub fn atom_or_string(&self) -> Result<&str, DomainFormError> {
        match self {
            DomainValue::Atom(value) | DomainValue::String(value) => Ok(value),
            _ => Err(DomainFormError::WrongValueKind),
        }
    }

    /// Renders this value back to its ASCII `#(...)` source text: an atom to
    /// its literal text, a string to its escaped quoted form, a list to
    /// `[a,b,...]`, and a nested form via [`format_domain_form`].
    ///
    /// Round-trips through [`parse_domain_form`] for a top-level form value.
    pub fn render_text(&self) -> String {
        match self {
            DomainValue::Form(form) => format_domain_form(form),
            DomainValue::List(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(DomainValue::render_text)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            DomainValue::String(value) => {
                format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
            }
            DomainValue::Atom(value) => value.clone(),
        }
    }

    /// Project this domain-form value to an expression map/list/string tree.
    ///
    /// Atoms become strings so their original text is preserved rather than
    /// interpreted as a symbol or number.
    pub fn to_expr(&self) -> Expr {
        match self {
            DomainValue::Form(form) => form.to_expr_map(),
            DomainValue::List(items) => {
                Expr::List(items.iter().map(DomainValue::to_expr).collect())
            }
            DomainValue::String(value) | DomainValue::Atom(value) => Expr::String(value.clone()),
        }
    }
}

/// A domain-form parse or access error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainFormError {
    /// Input did not start with `#(`.
    ExpectedForm,
    /// Input ended mid-form.
    UnexpectedEof,
    /// An invalid character was found where a token was expected.
    InvalidToken,
    /// A form repeated a field key.
    DuplicateField(String),
    /// Extra input followed the top-level form.
    TrailingInput,
    /// A required field was missing.
    MissingField(String),
    /// A field had the wrong value kind.
    WrongFieldKind(String),
    /// A value had the wrong kind for a keyless accessor (not the atom,
    /// string, or form that was expected).
    WrongValueKind,
}

/// Parse a top-level `#(...)` domain form.
///
/// # Examples
///
/// ```
/// use sim_codec::{parse_domain_form, DomainValue};
///
/// let form = parse_domain_form("#(Note dur=4/4 60 64)").unwrap();
/// assert_eq!(form.name, "Note");
/// assert_eq!(form.atom("dur").unwrap(), "4/4");
/// assert_eq!(
///     form.positional,
///     vec![DomainValue::Atom("60".into()), DomainValue::Atom("64".into())],
/// );
/// ```
pub fn parse_domain_form(input: &str) -> Result<DomainForm, DomainFormError> {
    let mut parser = Parser { input, index: 0 };
    parser.skip_ws();
    if !parser.consume_str("#(") {
        return Err(DomainFormError::ExpectedForm);
    }
    let form = parser.parse_form_body()?;
    parser.skip_ws();
    if parser.index != parser.input.len() {
        return Err(DomainFormError::TrailingInput);
    }
    Ok(form)
}

/// Render a domain form as an ASCII `#(...)` string. Round-trips through
/// [`parse_domain_form`].
///
/// # Examples
///
/// ```
/// use sim_codec::{format_domain_form, parse_domain_form};
///
/// let source = "#(Note dur=4/4 pitches=[60,64])";
/// let form = parse_domain_form(source).unwrap();
/// let rendered = format_domain_form(&form);
/// assert_eq!(parse_domain_form(&rendered).unwrap(), form);
/// ```
pub fn format_domain_form(form: &DomainForm) -> String {
    let mut out = String::from("#(");
    out.push_str(&form.name);
    for value in &form.positional {
        out.push(' ');
        format_value(value, &mut out);
    }
    for (key, value) in &form.fields {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        format_value(value, &mut out);
    }
    out.push(')');
    out
}

fn format_value(value: &DomainValue, out: &mut String) {
    match value {
        DomainValue::Form(form) => out.push_str(&format_domain_form(form)),
        DomainValue::List(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                format_value(item, out);
            }
            out.push(']');
        }
        DomainValue::String(text) => {
            out.push('"');
            for ch in text.chars() {
                if ch == '\\' || ch == '"' {
                    out.push('\\');
                }
                out.push(ch);
            }
            out.push('"');
        }
        DomainValue::Atom(text) => out.push_str(text),
    }
}

struct Parser<'a> {
    input: &'a str,
    index: usize,
}

impl Parser<'_> {
    fn parse_form_body(&mut self) -> Result<DomainForm, DomainFormError> {
        let name = self.parse_ident()?;
        let mut fields: Vec<(String, DomainValue)> = Vec::new();
        let mut positional = Vec::new();
        loop {
            self.skip_ws();
            if self.consume_char(')') {
                break;
            }
            match self.peek_char() {
                Some('#') | Some('[') | Some('"') => positional.push(self.parse_value()?),
                _ => {
                    let atom = self.parse_atom()?;
                    if self.consume_char('=') {
                        if fields.iter().any(|(key, _)| key == &atom) {
                            return Err(DomainFormError::DuplicateField(atom));
                        }
                        fields.push((atom, self.parse_value()?));
                    } else {
                        positional.push(DomainValue::Atom(atom));
                    }
                }
            }
        }
        Ok(DomainForm {
            name,
            fields,
            positional,
        })
    }

    fn parse_value(&mut self) -> Result<DomainValue, DomainFormError> {
        self.skip_ws();
        if self.consume_str("#(") {
            return self.parse_form_body().map(DomainValue::Form);
        }
        if self.consume_char('[') {
            return self.parse_list();
        }
        if self.peek_char() == Some('"') {
            return self.parse_string().map(DomainValue::String);
        }
        self.parse_atom().map(DomainValue::Atom)
    }

    fn parse_list(&mut self) -> Result<DomainValue, DomainFormError> {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.consume_char(']') {
                break;
            }
            items.push(self.parse_value()?);
            self.skip_ws();
            if self.consume_char(',') {
                continue;
            }
            self.expect_char(']')?;
            break;
        }
        Ok(DomainValue::List(items))
    }

    fn parse_string(&mut self) -> Result<String, DomainFormError> {
        self.expect_char('"')?;
        let mut out = String::new();
        while let Some(ch) = self.next_char() {
            match ch {
                '"' => return Ok(out),
                '\\' => out.push(self.next_char().ok_or(DomainFormError::UnexpectedEof)?),
                other => out.push(other),
            }
        }
        Err(DomainFormError::UnexpectedEof)
    }

    fn parse_atom(&mut self) -> Result<String, DomainFormError> {
        let start = self.index;
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() || [',', ')', ']', '='].contains(&ch) {
                break;
            }
            self.index += ch.len_utf8();
        }
        if self.index == start {
            return Err(DomainFormError::UnexpectedEof);
        }
        Ok(self.input[start..self.index].to_owned())
    }

    fn parse_ident(&mut self) -> Result<String, DomainFormError> {
        let atom = self.parse_atom()?;
        if atom
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            Ok(atom)
        } else {
            Err(DomainFormError::InvalidToken)
        }
    }

    fn expect_char(&mut self, expected: char) -> Result<(), DomainFormError> {
        match self.next_char() {
            Some(ch) if ch == expected => Ok(()),
            Some(_) => Err(DomainFormError::InvalidToken),
            None => Err(DomainFormError::UnexpectedEof),
        }
    }

    fn consume_char(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.index += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn consume_str(&mut self, expected: &str) -> bool {
        if self.input[self.index..].starts_with(expected) {
            self.index += expected.len();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.index += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.index..].chars().next()
    }

    fn next_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.index += ch.len_utf8();
        Some(ch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keyed_form_with_list_and_nested_form() {
        let form = parse_domain_form("#(Note dur=4/4 pitch=C4 tags=[a,b] inner=#(Rest dur=1/4))")
            .expect("parse");
        assert_eq!(form.name, "Note");
        assert_eq!(form.atom("dur").unwrap(), "4/4");
        assert_eq!(form.atom("pitch").unwrap(), "C4");
        assert_eq!(
            form.list("tags").unwrap(),
            &[
                DomainValue::Atom("a".to_owned()),
                DomainValue::Atom("b".to_owned())
            ]
        );
        assert_eq!(form.form("inner").unwrap().name, "Rest");
    }

    #[test]
    fn parses_positional_values() {
        let form = parse_domain_form("#(Chord 60 64 67)").expect("parse");
        assert_eq!(form.positional.len(), 3);
        assert!(form.fields.is_empty());
    }

    #[test]
    fn round_trips_through_format() {
        let source = "#(Note dur=4/4 sym=\"a\\\"b\" pitches=[60,64])";
        let form = parse_domain_form(source).expect("parse");
        let rendered = format_domain_form(&form);
        assert_eq!(parse_domain_form(&rendered).unwrap(), form);
    }

    #[test]
    fn missing_close_paren_is_unexpected_eof() {
        assert_eq!(
            parse_domain_form("#(Note dur=4/4"),
            Err(DomainFormError::UnexpectedEof)
        );
    }

    #[test]
    fn duplicate_field_is_rejected() {
        assert_eq!(
            parse_domain_form("#(Note dur=4/4 dur=1/2)"),
            Err(DomainFormError::DuplicateField("dur".to_owned()))
        );
    }

    #[test]
    fn trailing_input_is_rejected() {
        assert_eq!(
            parse_domain_form("#(Note dur=4/4) extra"),
            Err(DomainFormError::TrailingInput)
        );
    }

    #[test]
    fn non_form_input_is_rejected() {
        assert_eq!(
            parse_domain_form("Note"),
            Err(DomainFormError::ExpectedForm)
        );
    }

    #[test]
    fn escaped_quotes_round_trip() {
        let form = parse_domain_form("#(S v=\"he said \\\"hi\\\"\")").expect("parse");
        assert_eq!(form.string("v").unwrap(), "he said \"hi\"");
    }

    #[test]
    fn field_atom_or_string_accepts_both_kinds() {
        let form = parse_domain_form("#(N a=bare s=\"quoted\")").expect("parse");
        assert_eq!(form.field_atom_or_string("a").unwrap(), "bare");
        assert_eq!(form.field_atom_or_string("s").unwrap(), "quoted");
    }

    #[test]
    fn field_atom_or_string_rejects_list_and_missing() {
        let form = parse_domain_form("#(N tags=[a,b])").expect("parse");
        assert_eq!(
            form.field_atom_or_string("tags"),
            Err(DomainFormError::WrongFieldKind("tags".to_owned()))
        );
        assert_eq!(
            form.field_atom_or_string("nope"),
            Err(DomainFormError::MissingField("nope".to_owned()))
        );
    }

    #[test]
    fn field_text_renders_every_value_kind() {
        let form = parse_domain_form("#(N a=bare s=\"q\\\"x\" tags=[a,b] inner=#(Rest dur=1/4))")
            .expect("parse");
        assert_eq!(form.field_text("a").unwrap(), "bare");
        assert_eq!(form.field_text("s").unwrap(), "\"q\\\"x\"");
        assert_eq!(form.field_text("tags").unwrap(), "[a,b]");
        assert_eq!(form.field_text("inner").unwrap(), "#(Rest dur=1/4)");
        assert_eq!(
            form.field_text("nope"),
            Err(DomainFormError::MissingField("nope".to_owned()))
        );
    }

    #[test]
    fn value_as_form_and_atom_or_string() {
        let form = parse_domain_form("#(N inner=#(Rest dur=1/4) a=bare s=\"q\")").expect("parse");
        assert_eq!(form.field("inner").unwrap().as_form().unwrap().name, "Rest");
        assert_eq!(
            form.field("a").unwrap().as_form(),
            Err(DomainFormError::WrongValueKind)
        );
        assert_eq!(form.field("a").unwrap().atom_or_string().unwrap(), "bare");
        assert_eq!(form.field("s").unwrap().atom_or_string().unwrap(), "q");
        assert_eq!(
            form.field("inner").unwrap().atom_or_string(),
            Err(DomainFormError::WrongValueKind)
        );
    }

    #[test]
    fn render_text_round_trips_a_form_value() {
        let source = "#(Note dur=4/4 sym=\"a\\\"b\" pitches=[60,64])";
        let form = parse_domain_form(source).expect("parse");
        let value = DomainValue::Form(form.clone());
        let rendered = value.render_text();
        assert_eq!(parse_domain_form(&rendered).unwrap(), form);
    }

    #[test]
    fn render_text_formats_list_and_string_and_atom() {
        assert_eq!(DomainValue::Atom("60".to_owned()).render_text(), "60");
        assert_eq!(
            DomainValue::String("a\"b\\c".to_owned()).render_text(),
            "\"a\\\"b\\\\c\""
        );
        let list = DomainValue::List(vec![
            DomainValue::Atom("a".to_owned()),
            DomainValue::String("b".to_owned()),
        ]);
        assert_eq!(list.render_text(), "[a,\"b\"]");
    }
}
