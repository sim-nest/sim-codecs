//! Runtime registration and bounded inspection projection.

use std::sync::Arc;

use sim_codec::{Decoder, DomainCodecLib, Encoder, Input, Output};
use sim_kernel::{
    CodecId, Error, Expr, Lib, LibManifest, Linker, NumberLiteral, Result, SourceId, Symbol,
};
use sim_shape::{
    ExprKind, ExprKindShape, TableExtraPolicy, TableFieldSpec, TableShape, shape_value,
};

use crate::{
    AttributeShell, ByteReader, ClassShell, CodeAttribute, ConstantSlot, ShellBudget,
    decode_instructions,
};

const CLASSFILE_TAG: &str = "Classfile";

/// Binary JVM classfile decoder/encoder exposed as `codec/classfile`.
pub struct ClassfileCodec;

impl Decoder for ClassfileCodec {
    fn decode(&self, cx: &mut sim_codec::ReadCx<'_>, input: Input) -> Result<Expr> {
        let bytes = match input {
            Input::Bytes(bytes) => bytes,
            Input::Text(_) => return Err(codec_error(cx.codec, "classfile input must be bytes")),
        };
        cx.limits
            .max_input_bytes
            .checked_sub(bytes.len())
            .ok_or_else(|| {
                codec_error(
                    cx.codec,
                    "classfile exceeds the configured input-byte bound",
                )
            })?;
        inspect_classfile(cx.codec, bytes, cx.limits.max_collection_len)
    }
}

impl Encoder for ClassfileCodec {
    fn encode(&self, cx: &mut sim_kernel::WriteCx<'_>, expr: &Expr) -> Result<Output> {
        let Expr::Extension { tag, payload } = expr else {
            return Err(codec_error(
                cx.codec,
                "expected a retained Classfile projection",
            ));
        };
        if tag != &Symbol::qualified("classfile", CLASSFILE_TAG) {
            return Err(codec_error(
                cx.codec,
                "expected a retained Classfile projection",
            ));
        }
        let Expr::Map(entries) = payload.as_ref() else {
            return Err(codec_error(cx.codec, "malformed Classfile projection"));
        };
        entries
            .iter()
            .find_map(|(key, value)| match (key, value) {
                (Expr::Symbol(key), Expr::Bytes(bytes)) if key == &Symbol::new("bytes") => {
                    Some(Output::Bytes(bytes.clone()))
                }
                _ => None,
            })
            .ok_or_else(|| codec_error(cx.codec, "Classfile projection has no retained bytes"))
    }
}

/// Decode retained classfile bytes into bounded Table/Dir-compatible data.
///
/// Every instruction row carries both its method-local `code-offset` and its
/// absolute `byte-offset`, allowing a browse result to navigate back to the
/// retained byte string without consulting a JVM.
pub fn inspect_classfile(codec: CodecId, bytes: Vec<u8>, bound: usize) -> Result<Expr> {
    let cap = bound.min(65_536);
    let budget = ShellBudget {
        interfaces: cap,
        fields: cap,
        methods: cap,
        attributes: cap,
        attribute_bytes: bytes.len(),
    };
    let shell = ClassShell::decode(
        &bytes,
        bytes.len().saturating_mul(4).max(1024),
        budget,
        codec,
        SourceId("classfile".into()),
    )
    .map_err(|error| codec_error(codec, error.to_string()))?;
    shell
        .validate()
        .map_err(|error| codec_error(codec, error.to_string()))?;

    let constants = shell
        .constant_pool
        .slots()
        .iter()
        .enumerate()
        .take(cap)
        .map(|(index, slot)| {
            map([
                ("index", number(index)),
                ("kind", string(constant_kind(slot))),
            ])
        })
        .collect();
    let mut instructions = Vec::new();
    let mut attributes = Vec::new();
    for (method_index, method) in shell.methods.iter().enumerate() {
        for attribute in &method.attributes {
            attributes.push(attribute_row("method", method_index, attribute));
            if is_utf8(&shell, attribute.name_index, "Code") {
                let code = CodeAttribute::decode(&mut ByteReader::new(
                    &attribute.bytes,
                    bytes.len().max(1024),
                ))
                .map_err(|error| codec_error(codec, error.to_string()))?;
                let decoded =
                    decode_instructions(&code.code, shell.major_version, &shell.constant_pool)
                        .map_err(|error| codec_error(codec, error.to_string()))?;
                let code_start = attribute.origin.span.start.saturating_add(14);
                for located in decoded
                    .instructions
                    .into_iter()
                    .take(cap.saturating_sub(instructions.len()))
                {
                    instructions.push(map([
                        ("method", number(method_index)),
                        ("code-offset", number(located.offset)),
                        (
                            "byte-offset",
                            number(code_start.saturating_add(located.offset as usize)),
                        ),
                        (
                            "opcode",
                            string(located.instruction.opcode.metadata().mnemonic),
                        ),
                        (
                            "operands",
                            string(format!("{:?}", located.instruction.operands)),
                        ),
                    ]));
                }
            }
        }
    }
    for (index, attribute) in shell.attributes.iter().enumerate().take(cap) {
        attributes.push(attribute_row("class", index, attribute));
    }
    Ok(Expr::Extension {
        tag: Symbol::qualified("classfile", CLASSFILE_TAG),
        payload: Box::new(map([
            ("bytes", Expr::Bytes(bytes)),
            ("major-version", number(shell.major_version)),
            ("minor-version", number(shell.minor_version)),
            ("constants", Expr::Vector(constants)),
            (
                "attributes",
                Expr::Vector(attributes.into_iter().take(cap).collect()),
            ),
            ("instructions", Expr::Vector(instructions)),
        ])),
    })
}

fn attribute_row(owner: &str, index: usize, attribute: &AttributeShell) -> Expr {
    map([
        ("owner", string(owner)),
        ("index", number(index)),
        ("name-index", number(attribute.name_index)),
        ("byte-offset", number(attribute.origin.span.start)),
        (
            "byte-length",
            number(
                attribute
                    .origin
                    .span
                    .end
                    .saturating_sub(attribute.origin.span.start),
            ),
        ),
    ])
}

fn is_utf8(shell: &ClassShell, index: u16, expected: &str) -> bool {
    shell.constant_pool.entry(index, index).is_ok_and(|constant| {
        matches!(constant, crate::Constant::Utf8(value) if value.as_code_units().iter().copied().eq(expected.encode_utf16()))
    })
}

fn constant_kind(slot: &ConstantSlot) -> &'static str {
    match slot {
        ConstantSlot::Reserved => "reserved",
        ConstantSlot::Unusable => "unusable",
        ConstantSlot::Entry(value) => match value {
            crate::Constant::Utf8(_) => "utf8",
            crate::Constant::Integer(_) => "integer",
            crate::Constant::Float(_) => "float",
            crate::Constant::Long(_) => "long",
            crate::Constant::Double(_) => "double",
            crate::Constant::Class { .. } => "class",
            crate::Constant::String { .. } => "string",
            crate::Constant::Fieldref { .. } => "fieldref",
            crate::Constant::Methodref { .. } => "methodref",
            crate::Constant::InterfaceMethodref { .. } => "interface-methodref",
            crate::Constant::NameAndType { .. } => "name-and-type",
            crate::Constant::MethodHandle { .. } => "method-handle",
            crate::Constant::MethodType { .. } => "method-type",
            crate::Constant::Dynamic { .. } => "dynamic",
            crate::Constant::InvokeDynamic { .. } => "invoke-dynamic",
            crate::Constant::Module { .. } => "module",
            crate::Constant::Package { .. } => "package",
        },
    }
}

fn map<const N: usize>(entries: [(&str, Expr); N]) -> Expr {
    Expr::Map(
        entries
            .into_iter()
            .map(|(key, value)| (Expr::Symbol(Symbol::new(key)), value))
            .collect(),
    )
}
fn string(value: impl Into<String>) -> Expr {
    Expr::String(value.into())
}
fn number(value: impl ToString) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "u64"),
        canonical: value.to_string(),
    })
}
fn codec_error(codec: CodecId, message: impl Into<String>) -> Error {
    Error::CodecError {
        codec,
        message: message.into(),
    }
}

/// Host-registered library that installs the codec object and its browse Shapes.
pub struct ClassfileCodecLib {
    symbol: Symbol,
    codec_id: CodecId,
}
impl ClassfileCodecLib {
    /// Create a classfile library bound to a runtime-assigned codec id.
    pub fn new(codec_id: CodecId) -> Self {
        Self {
            symbol: Symbol::qualified("codec", "classfile"),
            codec_id,
        }
    }

    fn domain_lib(&self) -> DomainCodecLib {
        let shapes = classfile_shapes()
            .into_iter()
            .map(|(symbol, shape)| (symbol.clone(), shape_value(symbol, shape)))
            .collect();
        DomainCodecLib::new(
            self.symbol.clone(),
            self.codec_id,
            Arc::new(ClassfileCodec),
            Arc::new(ClassfileCodec),
            Symbol::qualified("codec", "Classfile"),
        )
        .with_shapes(shapes)
    }
}
impl Lib for ClassfileCodecLib {
    fn manifest(&self) -> LibManifest {
        self.domain_lib().manifest()
    }
    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker) -> Result<()> {
        self.domain_lib().load(cx, linker)
    }
}

fn classfile_shapes() -> Vec<(Symbol, Arc<dyn sim_kernel::Shape>)> {
    let number = || Arc::new(ExprKindShape::new(ExprKind::Number)) as Arc<dyn sim_kernel::Shape>;
    let string = || Arc::new(ExprKindShape::new(ExprKind::String)) as Arc<dyn sim_kernel::Shape>;
    let row = |fields: Vec<TableFieldSpec>| {
        Arc::new(TableShape::new(fields, TableExtraPolicy::Reject)) as Arc<dyn sim_kernel::Shape>
    };
    let field = |key: &str, shape: Arc<dyn sim_kernel::Shape>| TableFieldSpec {
        key: Symbol::new(key),
        shape,
        required: true,
    };
    vec![
        (
            Symbol::qualified("codec", "Classfile"),
            Arc::new(ExprKindShape::new(ExprKind::Extension)),
        ),
        (
            Symbol::qualified("classfile", "ConstantRow"),
            row(vec![field("index", number()), field("kind", string())]),
        ),
        (
            Symbol::qualified("classfile", "AttributeRow"),
            row(vec![
                field("owner", string()),
                field("index", number()),
                field("name-index", number()),
                field("byte-offset", number()),
                field("byte-length", number()),
            ]),
        ),
        (
            Symbol::qualified("classfile", "InstructionRow"),
            row(vec![
                field("method", number()),
                field("code-offset", number()),
                field("byte-offset", number()),
                field("opcode", string()),
                field("operands", string()),
            ]),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_codec::{DecodeLimits, Encoder};
    use sim_kernel::{Cx, DefaultFactory, EagerPolicy, EncodeOptions, WriteCx};

    const POSITIVE: &[u8] = include_bytes!("../fixtures/positive.class");

    #[test]
    fn runtime_registers_codec_and_inspection_shapes() {
        let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        cx.load_lib(&ClassfileCodecLib::new(CodecId(73))).unwrap();
        assert!(
            cx.registry()
                .codec_by_symbol(&Symbol::qualified("codec", "classfile"))
                .is_some()
        );
        for symbol in [
            Symbol::qualified("codec", "Classfile"),
            Symbol::qualified("classfile", "ConstantRow"),
            Symbol::qualified("classfile", "AttributeRow"),
            Symbol::qualified("classfile", "InstructionRow"),
        ] {
            assert!(cx.registry().shape_by_symbol(&symbol).is_some(), "{symbol}");
        }
    }

    #[test]
    fn inspection_is_bounded_and_instruction_rows_navigate_to_bytes() {
        let projection = inspect_classfile(CodecId(73), POSITIVE.to_vec(), 4096).unwrap();
        let Expr::Extension { payload, .. } = &projection else {
            panic!("not retained")
        };
        let Expr::Map(root) = payload.as_ref() else {
            panic!("not browseable")
        };
        let instructions = root
            .iter()
            .find_map(|(key, value)| {
                matches!(key, Expr::Symbol(symbol) if symbol == &Symbol::new("instructions"))
                    .then_some(value)
            })
            .unwrap();
        let Expr::Vector(rows) = instructions else {
            panic!("instructions are not a directory")
        };
        let Expr::Map(first) = rows.first().expect("fixture has instructions") else {
            panic!("row is not a table")
        };
        let offset = first
            .iter()
            .find_map(|(key, value)| match (key, value) {
                (Expr::Symbol(key), Expr::Number(value)) if key == &Symbol::new("byte-offset") => {
                    value.canonical.parse::<usize>().ok()
                }
                _ => None,
            })
            .expect("instruction has an absolute byte offset");
        assert!(offset < POSITIVE.len());

        let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        let codec = ClassfileCodec;
        let mut write = WriteCx {
            cx: &mut cx,
            codec: CodecId(73),
            options: EncodeOptions::default(),
        };
        assert_eq!(
            codec.encode(&mut write, &projection).unwrap(),
            Output::Bytes(POSITIVE.to_vec())
        );
        assert!(rows.len() <= DecodeLimits::default().max_collection_len);
    }
}
