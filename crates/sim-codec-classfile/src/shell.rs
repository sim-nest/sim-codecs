//! Bounded structural classfile shells and their separate typed validation projection.

use core::fmt;

use sim_kernel::{CodecId, Origin, SourceId, Span};

use crate::{
    ByteError, ByteReader, ByteWriter, CodeAttribute, Constant, ConstantPool, ConstantPoolError,
};

/// Limits for allocations made while structurally decoding one classfile shell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellBudget {
    /// Maximum implemented interfaces.
    pub interfaces: usize,
    /// Maximum field declarations.
    pub fields: usize,
    /// Maximum method declarations.
    pub methods: usize,
    /// Maximum attributes across the class and all members.
    pub attributes: usize,
    /// Maximum aggregate bytes retained in attribute bodies.
    pub attribute_bytes: usize,
}

/// An uninterpreted attribute spine entry, retained in its original order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributeShell {
    /// Raw constant-pool index of the attribute name.
    pub name_index: u16,
    /// Declared attribute body length.
    pub declared_length: u32,
    /// Exact uninterpreted attribute body.
    pub bytes: Vec<u8>,
    /// Source span covering the complete attribute, including its header.
    pub origin: Origin,
    /// Owner and ordinal captured at decode time.
    pub location: AttributeLocation,
}

/// The legal classfile owner of an attribute shell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeOwner {
    /// The class declaration.
    Class,
    /// A field declaration at the given classfile ordinal.
    Field(usize),
    /// A method declaration at the given classfile ordinal.
    Method(usize),
}

/// Stable owner and order evidence for a retained attribute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttributeLocation {
    /// Attribute owner.
    pub owner: AttributeOwner,
    /// Zero-based position within that owner's attribute table.
    pub order: usize,
}

/// Checked evidence invalidated by a classfile edit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutInvalidation {
    /// Declaration path whose original bytes are no longer evidence for current content/layout.
    pub path: String,
    /// Whether byte positions after this path moved.
    pub shifts_following_layout: bool,
}

/// Result of one checked method-body edit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditReport {
    /// Exact layout evidence invalidated by the edit.
    pub invalidated: Vec<LayoutInvalidation>,
}

/// A raw field declaration whose indices have not been validated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldShell {
    /// Raw JVM field access flags.
    pub access_flags: u16,
    /// Raw constant-pool index of the field name.
    pub name_index: u16,
    /// Raw constant-pool index of the field descriptor.
    pub descriptor_index: u16,
    /// Ordered attribute spine.
    pub attributes: Vec<AttributeShell>,
    /// Source span covering the declaration.
    pub origin: Origin,
}

/// A raw method declaration whose indices have not been validated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MethodShell {
    /// Raw JVM method access flags.
    pub access_flags: u16,
    /// Raw constant-pool index of the method name.
    pub name_index: u16,
    /// Raw constant-pool index of the method descriptor.
    pub descriptor_index: u16,
    /// Ordered attribute spine.
    pub attributes: Vec<AttributeShell>,
    /// Source span covering the declaration.
    pub origin: Origin,
}

/// The bounded structural classfile read, before any shell index is checked.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassShell {
    /// Classfile minor version.
    pub minor_version: u16,
    /// Classfile major version.
    pub major_version: u16,
    /// Structurally decoded constant pool.
    pub constant_pool: ConstantPool,
    /// Raw JVM class access flags.
    pub access_flags: u16,
    /// Raw constant-pool index of this class.
    pub this_class: u16,
    /// Raw constant-pool index of the superclass, or zero for `java/lang/Object`.
    pub super_class: u16,
    /// Raw constant-pool indices of directly implemented interfaces.
    pub interfaces: Vec<u16>,
    /// Field declarations in classfile order.
    pub fields: Vec<FieldShell>,
    /// Method declarations in classfile order.
    pub methods: Vec<MethodShell>,
    /// Class attributes in classfile order.
    pub attributes: Vec<AttributeShell>,
    /// Source span covering the whole shell.
    pub origin: Origin,
}

/// A constant-pool index proven to name a `Class` entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClassIndex(pub u16);

/// A constant-pool index proven to name a `Utf8` entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Utf8Index(pub u16);

/// A field projection whose name, descriptor, and attribute names are typed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedFieldShell {
    /// Validated field-name index.
    pub name: Utf8Index,
    /// Validated field-descriptor index.
    pub descriptor: Utf8Index,
    /// Validated attribute-name indices in original order.
    pub attribute_names: Vec<Utf8Index>,
}

/// A method projection whose name, descriptor, and attribute names are typed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedMethodShell {
    /// Validated method-name index.
    pub name: Utf8Index,
    /// Validated method-descriptor index.
    pub descriptor: Utf8Index,
    /// Validated attribute-name indices in original order.
    pub attribute_names: Vec<Utf8Index>,
}

/// Typed shell references produced only after structural decoding succeeds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedClassShell {
    /// Validated index of this class.
    pub this_class: ClassIndex,
    /// Validated superclass index, absent only when the raw index is zero.
    pub super_class: Option<ClassIndex>,
    /// Validated interface indices in classfile order.
    pub interfaces: Vec<ClassIndex>,
    /// Validated field projections.
    pub fields: Vec<ValidatedFieldShell>,
    /// Validated method projections.
    pub methods: Vec<ValidatedMethodShell>,
    /// Validated class-attribute name indices in classfile order.
    pub attribute_names: Vec<Utf8Index>,
}

/// Stable failure category for shell decoding and validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellErrorKind {
    /// The classfile magic was not `CAFEBABE`.
    Magic,
    /// The bounded byte lane failed.
    Bytes,
    /// Constant-pool decoding or validation failed.
    ConstantPool,
    /// A collection or retained attribute body exceeded its shell budget.
    Budget,
    /// A raw index was zero, outside the pool, unusable, or of the wrong category.
    InvalidIndex,
    /// Bytes remained after the complete class shell.
    TrailingBytes,
    /// A requested checked edit cannot be applied to this shell.
    Edit,
}

/// A located shell failure, optionally naming the offending raw index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellError {
    /// Stable machine-matchable category.
    pub kind: ShellErrorKind,
    /// Absolute byte offset associated with the failure.
    pub offset: usize,
    /// Offending raw constant-pool index when applicable.
    pub index: Option<u16>,
    /// Declaration path associated with the failure.
    pub path: String,
    /// Human-readable detail.
    pub message: String,
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {} (byte {})",
            self.message, self.path, self.offset
        )
    }
}

impl std::error::Error for ShellError {}

impl ClassShell {
    /// Structurally decode a complete classfile without validating shell indices.
    pub fn decode(
        bytes: &[u8],
        allocation_budget: usize,
        budget: ShellBudget,
        codec: CodecId,
        source: SourceId,
    ) -> Result<Self, ShellError> {
        let mut reader = ByteReader::new(bytes, allocation_budget);
        if reader.read_u4().map_err(|e| byte_error("magic", e))? != 0xcafe_babe {
            return Err(error(
                ShellErrorKind::Magic,
                0,
                None,
                "magic",
                "invalid classfile magic",
            ));
        }
        let minor_version = reader
            .read_u2()
            .map_err(|e| byte_error("minor_version", e))?;
        let major_version = reader
            .read_u2()
            .map_err(|e| byte_error("major_version", e))?;
        let constant_pool = ConstantPool::decode(&mut reader, major_version).map_err(pool_error)?;
        let access_flags = reader
            .read_u2()
            .map_err(|e| byte_error("access_flags", e))?;
        let this_class = reader.read_u2().map_err(|e| byte_error("this_class", e))?;
        let super_class = reader.read_u2().map_err(|e| byte_error("super_class", e))?;
        let mut state = DecodeState {
            budget,
            attributes: 0,
            attribute_bytes: 0,
            codec,
            source,
        };
        let interfaces = read_indices(&mut reader, budget.interfaces, "interfaces")?;
        let fields = read_members(
            &mut reader,
            budget.fields,
            "fields",
            AttributeOwner::Field,
            &mut state,
        )?
        .into_iter()
        .map(Member::into_field)
        .collect();
        let methods = read_members(
            &mut reader,
            budget.methods,
            "methods",
            AttributeOwner::Method,
            &mut state,
        )?
        .into_iter()
        .map(Member::into_method)
        .collect();
        let attributes =
            read_attributes(&mut reader, "attributes", AttributeOwner::Class, &mut state)?;
        if reader.remaining() != 0 {
            return Err(error(
                ShellErrorKind::TrailingBytes,
                reader.offset(),
                None,
                "class",
                "trailing bytes after class shell",
            ));
        }
        Ok(Self {
            minor_version,
            major_version,
            constant_pool,
            access_flags,
            this_class,
            super_class,
            interfaces,
            fields,
            methods,
            attributes,
            origin: origin(codec, state.source, 0, reader.offset()),
        })
    }

    /// Validate every raw shell index and return a separate typed projection.
    pub fn validate(&self) -> Result<ValidatedClassShell, ShellError> {
        let this_class = self.class_index(self.this_class, "this_class", &self.origin)?;
        let super_class = if self.super_class == 0 {
            None
        } else {
            Some(self.class_index(self.super_class, "super_class", &self.origin)?)
        };
        let interfaces = self
            .interfaces
            .iter()
            .enumerate()
            .map(|(position, &index)| {
                self.class_index(index, &format!("interfaces[{position}]"), &self.origin)
            })
            .collect::<Result<_, _>>()?;
        let fields = self
            .fields
            .iter()
            .enumerate()
            .map(|(position, member)| {
                Ok(ValidatedFieldShell {
                    name: self.utf8_index(
                        member.name_index,
                        &format!("fields[{position}].name_index"),
                        &member.origin,
                    )?,
                    descriptor: self.utf8_index(
                        member.descriptor_index,
                        &format!("fields[{position}].descriptor_index"),
                        &member.origin,
                    )?,
                    attribute_names: self
                        .validate_attributes(&member.attributes, &format!("fields[{position}]"))?,
                })
            })
            .collect::<Result<_, ShellError>>()?;
        let methods = self
            .methods
            .iter()
            .enumerate()
            .map(|(position, member)| {
                Ok(ValidatedMethodShell {
                    name: self.utf8_index(
                        member.name_index,
                        &format!("methods[{position}].name_index"),
                        &member.origin,
                    )?,
                    descriptor: self.utf8_index(
                        member.descriptor_index,
                        &format!("methods[{position}].descriptor_index"),
                        &member.origin,
                    )?,
                    attribute_names: self
                        .validate_attributes(&member.attributes, &format!("methods[{position}]"))?,
                })
            })
            .collect::<Result<_, ShellError>>()?;
        Ok(ValidatedClassShell {
            this_class,
            super_class,
            interfaces,
            fields,
            methods,
            attribute_names: self.validate_attributes(&self.attributes, "class")?,
        })
    }

    /// Encode the complete shell, retaining all attribute bytes and table order exactly.
    pub fn encode(&self, allocation_budget: usize) -> Result<Vec<u8>, ShellError> {
        self.validate()?;
        let mut out = ByteWriter::new(allocation_budget);
        out.write_u4(0xcafe_babe)
            .map_err(|e| byte_error("magic", e))?;
        out.write_u2(self.minor_version)
            .map_err(|e| byte_error("minor_version", e))?;
        out.write_u2(self.major_version)
            .map_err(|e| byte_error("major_version", e))?;
        self.constant_pool
            .encode(&mut out, self.major_version)
            .map_err(pool_error)?;
        out.write_u2(self.access_flags)
            .map_err(|e| byte_error("access_flags", e))?;
        out.write_u2(self.this_class)
            .map_err(|e| byte_error("this_class", e))?;
        out.write_u2(self.super_class)
            .map_err(|e| byte_error("super_class", e))?;
        write_indices(&mut out, &self.interfaces, "interfaces")?;
        write_members(&mut out, &self.fields, "fields")?;
        write_members(&mut out, &self.methods, "methods")?;
        write_attributes(&mut out, &self.attributes, "class")?;
        Ok(out.into_bytes())
    }

    /// Replace one method's `Code` byte array without disturbing unrelated raw attributes.
    ///
    /// The selected payload is decoded and re-encoded structurally, so malformed code metadata or
    /// an edit that invalidates exception ranges is rejected. The report precisely distinguishes a
    /// same-size content edit from an edit that shifts following byte positions.
    pub fn replace_method_code(
        &mut self,
        method_index: usize,
        code: Vec<u8>,
        allocation_budget: usize,
    ) -> Result<EditReport, ShellError> {
        let code_name = self.constant_pool.slots().iter().position(|slot| {
            matches!(slot, crate::ConstantSlot::Entry(Constant::Utf8(value)) if value.as_code_units() == ['C' as u16, 'o' as u16, 'd' as u16, 'e' as u16])
        }).ok_or_else(|| edit_error("constant_pool", "constant pool does not contain Code"))? as u16;
        let method = self.methods.get_mut(method_index).ok_or_else(|| {
            edit_error(
                format!("methods[{method_index}]"),
                "method index is out of range",
            )
        })?;
        let (attribute_index, attribute) = method
            .attributes
            .iter_mut()
            .enumerate()
            .find(|(_, attribute)| attribute.name_index == code_name)
            .ok_or_else(|| {
                edit_error(
                    format!("methods[{method_index}]"),
                    "method has no Code attribute",
                )
            })?;
        let old_len = attribute.bytes.len();
        let mut structured =
            CodeAttribute::decode(&mut ByteReader::new(&attribute.bytes, allocation_budget))
                .map_err(|cause| {
                    edit_error(
                        format!("methods[{method_index}].attributes[{attribute_index}]"),
                        cause.to_string(),
                    )
                })?;
        structured.code = code;
        let bytes = structured.encode(allocation_budget).map_err(|cause| {
            edit_error(
                format!("methods[{method_index}].attributes[{attribute_index}]"),
                cause.to_string(),
            )
        })?;
        attribute.declared_length = u32::try_from(bytes.len())
            .map_err(|_| edit_error("Code", "encoded Code attribute exceeds u32"))?;
        attribute.bytes = bytes;
        Ok(EditReport {
            invalidated: vec![LayoutInvalidation {
                path: format!("methods[{method_index}].attributes[{attribute_index}].bytes"),
                shifts_following_layout: old_len != attribute.bytes.len(),
            }],
        })
    }

    fn class_index(&self, index: u16, path: &str, at: &Origin) -> Result<ClassIndex, ShellError> {
        self.expect(index, path, at, |entry| {
            matches!(entry, Constant::Class { .. })
        })?;
        Ok(ClassIndex(index))
    }

    fn utf8_index(&self, index: u16, path: &str, at: &Origin) -> Result<Utf8Index, ShellError> {
        self.expect(index, path, at, |entry| matches!(entry, Constant::Utf8(_)))?;
        Ok(Utf8Index(index))
    }

    fn expect(
        &self,
        index: u16,
        path: &str,
        at: &Origin,
        predicate: impl FnOnce(&Constant) -> bool,
    ) -> Result<(), ShellError> {
        let entry = self.constant_pool.entry(index, index).map_err(|cause| {
            error(
                ShellErrorKind::InvalidIndex,
                at.span.start,
                Some(index),
                path,
                format!("invalid constant-pool index {index}: {cause}"),
            )
        })?;
        if !predicate(entry) {
            return Err(error(
                ShellErrorKind::InvalidIndex,
                at.span.start,
                Some(index),
                path,
                format!("constant-pool index {index} has the wrong category"),
            ));
        }
        Ok(())
    }

    fn validate_attributes(
        &self,
        attributes: &[AttributeShell],
        owner: &str,
    ) -> Result<Vec<Utf8Index>, ShellError> {
        attributes
            .iter()
            .enumerate()
            .map(|(position, attribute)| {
                self.utf8_index(
                    attribute.name_index,
                    &format!("{owner}.attributes[{position}].name_index"),
                    &attribute.origin,
                )
            })
            .collect()
    }
}

struct DecodeState {
    budget: ShellBudget,
    attributes: usize,
    attribute_bytes: usize,
    codec: CodecId,
    source: SourceId,
}

struct Member {
    access_flags: u16,
    name_index: u16,
    descriptor_index: u16,
    attributes: Vec<AttributeShell>,
    origin: Origin,
}

impl Member {
    fn into_field(self) -> FieldShell {
        FieldShell {
            access_flags: self.access_flags,
            name_index: self.name_index,
            descriptor_index: self.descriptor_index,
            attributes: self.attributes,
            origin: self.origin,
        }
    }
    fn into_method(self) -> MethodShell {
        MethodShell {
            access_flags: self.access_flags,
            name_index: self.name_index,
            descriptor_index: self.descriptor_index,
            attributes: self.attributes,
            origin: self.origin,
        }
    }
}

fn read_indices(
    reader: &mut ByteReader<'_>,
    limit: usize,
    path: &str,
) -> Result<Vec<u16>, ShellError> {
    let count = usize::from(reader.read_u2().map_err(|e| byte_error(path, e))?);
    check_count(reader, count, limit, path)?;
    (0..count)
        .map(|position| {
            reader
                .read_u2()
                .map_err(|e| byte_error(&format!("{path}[{position}]"), e))
        })
        .collect()
}

fn read_members(
    reader: &mut ByteReader<'_>,
    limit: usize,
    path: &str,
    owner: fn(usize) -> AttributeOwner,
    state: &mut DecodeState,
) -> Result<Vec<Member>, ShellError> {
    let count = usize::from(reader.read_u2().map_err(|e| byte_error(path, e))?);
    check_count(reader, count, limit, path)?;
    let mut members = Vec::with_capacity(count);
    for position in 0..count {
        let member_path = format!("{path}[{position}]");
        let start = reader.offset();
        let access_flags = reader.read_u2().map_err(|e| byte_error(&member_path, e))?;
        let name_index = reader.read_u2().map_err(|e| byte_error(&member_path, e))?;
        let descriptor_index = reader.read_u2().map_err(|e| byte_error(&member_path, e))?;
        let attributes = read_attributes(reader, &member_path, owner(position), state)?;
        members.push(Member {
            access_flags,
            name_index,
            descriptor_index,
            attributes,
            origin: origin(state.codec, state.source.clone(), start, reader.offset()),
        });
    }
    Ok(members)
}

fn read_attributes(
    reader: &mut ByteReader<'_>,
    owner: &str,
    attribute_owner: AttributeOwner,
    state: &mut DecodeState,
) -> Result<Vec<AttributeShell>, ShellError> {
    let count = usize::from(reader.read_u2().map_err(|e| byte_error(owner, e))?);
    state.attributes = state
        .attributes
        .checked_add(count)
        .ok_or_else(|| budget_error(reader.offset(), owner))?;
    check_count(reader, state.attributes, state.budget.attributes, owner)?;
    let mut attributes = Vec::with_capacity(count);
    for position in 0..count {
        let path = format!("{owner}.attributes[{position}]");
        let start = reader.offset();
        let name_index = reader.read_u2().map_err(|e| byte_error(&path, e))?;
        let declared_length = reader.read_u4().map_err(|e| byte_error(&path, e))?;
        let length =
            usize::try_from(declared_length).map_err(|_| budget_error(reader.offset(), &path))?;
        state.attribute_bytes = state
            .attribute_bytes
            .checked_add(length)
            .ok_or_else(|| budget_error(reader.offset(), &path))?;
        check_count(
            reader,
            state.attribute_bytes,
            state.budget.attribute_bytes,
            &path,
        )?;
        let bytes = reader
            .take(length)
            .map_err(|e| byte_error(&path, e))?
            .to_vec();
        attributes.push(AttributeShell {
            name_index,
            declared_length,
            bytes,
            origin: origin(state.codec, state.source.clone(), start, reader.offset()),
            location: AttributeLocation {
                owner: attribute_owner,
                order: position,
            },
        });
    }
    Ok(attributes)
}

trait MemberShell {
    fn header(&self) -> (u16, u16, u16);
    fn attributes(&self) -> &[AttributeShell];
}
impl MemberShell for FieldShell {
    fn header(&self) -> (u16, u16, u16) {
        (self.access_flags, self.name_index, self.descriptor_index)
    }
    fn attributes(&self) -> &[AttributeShell] {
        &self.attributes
    }
}
impl MemberShell for MethodShell {
    fn header(&self) -> (u16, u16, u16) {
        (self.access_flags, self.name_index, self.descriptor_index)
    }
    fn attributes(&self) -> &[AttributeShell] {
        &self.attributes
    }
}
fn write_indices(out: &mut ByteWriter, values: &[u16], path: &str) -> Result<(), ShellError> {
    out.write_u2(u16::try_from(values.len()).map_err(|_| edit_error(path, "count exceeds u16"))?)
        .map_err(|e| byte_error(path, e))?;
    for value in values {
        out.write_u2(*value).map_err(|e| byte_error(path, e))?;
    }
    Ok(())
}
fn write_members<T: MemberShell>(
    out: &mut ByteWriter,
    values: &[T],
    path: &str,
) -> Result<(), ShellError> {
    out.write_u2(u16::try_from(values.len()).map_err(|_| edit_error(path, "count exceeds u16"))?)
        .map_err(|e| byte_error(path, e))?;
    for value in values {
        let (flags, name, descriptor) = value.header();
        out.write_u2(flags).map_err(|e| byte_error(path, e))?;
        out.write_u2(name).map_err(|e| byte_error(path, e))?;
        out.write_u2(descriptor).map_err(|e| byte_error(path, e))?;
        write_attributes(out, value.attributes(), path)?;
    }
    Ok(())
}
fn write_attributes(
    out: &mut ByteWriter,
    values: &[AttributeShell],
    path: &str,
) -> Result<(), ShellError> {
    out.write_u2(
        u16::try_from(values.len()).map_err(|_| edit_error(path, "attribute count exceeds u16"))?,
    )
    .map_err(|e| byte_error(path, e))?;
    for attribute in values {
        if usize::try_from(attribute.declared_length).ok() != Some(attribute.bytes.len()) {
            return Err(edit_error(
                path,
                "declared attribute length differs from retained bytes",
            ));
        }
        out.write_u2(attribute.name_index)
            .map_err(|e| byte_error(path, e))?;
        out.write_u4(attribute.declared_length)
            .map_err(|e| byte_error(path, e))?;
        out.write_bytes(&attribute.bytes)
            .map_err(|e| byte_error(path, e))?;
    }
    Ok(())
}

fn edit_error(path: impl Into<String>, message: impl Into<String>) -> ShellError {
    error(ShellErrorKind::Edit, 0, None, path, message)
}

fn check_count(
    reader: &ByteReader<'_>,
    count: usize,
    limit: usize,
    path: &str,
) -> Result<(), ShellError> {
    if count > limit {
        return Err(budget_error(reader.offset(), path));
    }
    reader
        .preflight_allocation(count)
        .map_err(|_| budget_error(reader.offset(), path))
}

fn origin(codec: CodecId, source: SourceId, start: usize, end: usize) -> Origin {
    Origin {
        codec,
        source,
        span: Span { start, end },
        trivia: Vec::new(),
    }
}

fn byte_error(path: &str, cause: ByteError) -> ShellError {
    error(
        ShellErrorKind::Bytes,
        cause.offset,
        None,
        path,
        cause.to_string(),
    )
}
fn pool_error(cause: ConstantPoolError) -> ShellError {
    error(
        ShellErrorKind::ConstantPool,
        8,
        cause.target_index,
        "constant_pool",
        cause.to_string(),
    )
}
fn budget_error(offset: usize, path: &str) -> ShellError {
    error(
        ShellErrorKind::Budget,
        offset,
        None,
        path,
        "shell decode budget exceeded",
    )
}
fn error(
    kind: ShellErrorKind,
    offset: usize,
    index: Option<u16>,
    path: impl Into<String>,
    message: impl Into<String>,
) -> ShellError {
    ShellError {
        kind,
        offset,
        index,
        path: path.into(),
        message: message.into(),
    }
}
