//! Lossless, index-preserving JVM constant-pool grammar.

use core::fmt;

use sim_text::CodeUnitString;

use crate::{ByteError, ByteReader, ByteWriter, decode_modified_utf8, encode_modified_utf8};

/// One usable JVM constant-pool entry.
#[derive(Clone, Debug, PartialEq)]
pub enum Constant {
    /// A modified-UTF-8 string, represented without losing UTF-16 code units.
    Utf8(CodeUnitString),
    /// A signed `int` bit pattern.
    Integer(u32),
    /// An IEEE 754 single-precision bit pattern.
    Float(u32),
    /// A signed `long` bit pattern.
    Long(u64),
    /// An IEEE 754 double-precision bit pattern.
    Double(u64),
    /// A class or interface name index.
    Class {
        /// Index of the `Utf8` internal name.
        name_index: u16,
    },
    /// A string contents index.
    String {
        /// Index of the `Utf8` string contents.
        string_index: u16,
    },
    /// A field reference.
    Fieldref {
        /// Index of the declaring `Class`.
        class_index: u16,
        /// Index of the member `NameAndType`.
        name_and_type_index: u16,
    },
    /// A class method reference.
    Methodref {
        /// Index of the declaring `Class`.
        class_index: u16,
        /// Index of the member `NameAndType`.
        name_and_type_index: u16,
    },
    /// An interface method reference.
    InterfaceMethodref {
        /// Index of the declaring interface `Class`.
        class_index: u16,
        /// Index of the member `NameAndType`.
        name_and_type_index: u16,
    },
    /// A name and descriptor pair.
    NameAndType {
        /// Index of the member-name `Utf8`.
        name_index: u16,
        /// Index of the descriptor `Utf8`.
        descriptor_index: u16,
    },
    /// A direct method-handle reference.
    MethodHandle {
        /// JVM reference-kind discriminator from 1 through 9.
        reference_kind: u8,
        /// Index of the category selected by `reference_kind`.
        reference_index: u16,
    },
    /// A method descriptor.
    MethodType {
        /// Index of the method-descriptor `Utf8`.
        descriptor_index: u16,
    },
    /// A dynamically computed constant.
    Dynamic {
        /// Index into the class's `BootstrapMethods` attribute.
        bootstrap_method_attr_index: u16,
        /// Index of the constant's `NameAndType`.
        name_and_type_index: u16,
    },
    /// A dynamically selected call site.
    InvokeDynamic {
        /// Index into the class's `BootstrapMethods` attribute.
        bootstrap_method_attr_index: u16,
        /// Index of the call site's `NameAndType`.
        name_and_type_index: u16,
    },
    /// A module name.
    Module {
        /// Index of the module-name `Utf8`.
        name_index: u16,
    },
    /// A package name.
    Package {
        /// Index of the package-name `Utf8`.
        name_index: u16,
    },
}

/// One physical constant-pool index, including indices that cannot be referenced.
#[derive(Clone, Debug, PartialEq)]
pub enum ConstantSlot {
    /// The mandated, non-encoded index zero.
    Reserved,
    /// A usable entry encoded at this index.
    Entry(Constant),
    /// The explicit second slot occupied by the preceding `Long` or `Double`.
    Unusable,
}

/// A stable constant-pool failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConstantPoolErrorKind {
    /// The underlying bounded byte lane failed.
    Bytes,
    /// The pool uses an unassigned constant tag.
    UnknownTag,
    /// The entry is not admitted by the classfile major version.
    Version,
    /// A method-handle reference kind is outside 1 through 9.
    ReferenceKind,
    /// A referenced index is zero or outside the pool.
    InvalidIndex,
    /// A reference points at a reserved or unusable slot.
    UnusableTarget,
    /// A reference points at the wrong constant category.
    WrongCategory,
    /// An in-memory slot sequence cannot be encoded faithfully.
    InvalidLayout,
}

/// A typed constant-pool failure located at the entry that caused it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstantPoolError {
    /// Stable machine-matchable failure category.
    pub kind: ConstantPoolErrorKind,
    /// Constant-pool index containing the invalid value, or the next index while decoding.
    pub index: u16,
    /// Referenced constant-pool index when the failure concerns a target.
    pub target_index: Option<u16>,
    /// Human-readable context.
    pub message: String,
}

impl ConstantPoolError {
    fn new(
        kind: ConstantPoolErrorKind,
        index: u16,
        target: Option<u16>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            index,
            target_index: target,
            message: message.into(),
        }
    }

    fn bytes(index: u16, error: ByteError) -> Self {
        Self::new(ConstantPoolErrorKind::Bytes, index, None, error.to_string())
    }
}

impl fmt::Display for ConstantPoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at constant-pool index {}", self.message, self.index)
    }
}

impl std::error::Error for ConstantPoolError {}

/// An index-preserving constant pool whose `slots()[index]` is the physical JVM slot.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstantPool {
    slots: Vec<ConstantSlot>,
}

impl ConstantPool {
    /// Decode the `constant_pool_count` and all following entries for `major_version`.
    pub fn decode(
        reader: &mut ByteReader<'_>,
        major_version: u16,
    ) -> Result<Self, ConstantPoolError> {
        let count = reader
            .read_u2()
            .map_err(|e| ConstantPoolError::bytes(0, e))?;
        if count == 0 {
            return Err(ConstantPoolError::new(
                ConstantPoolErrorKind::InvalidLayout,
                0,
                None,
                "constant_pool_count must include the reserved zero index",
            ));
        }
        reader
            .preflight_allocation(usize::from(count))
            .map_err(|e| ConstantPoolError::bytes(0, e))?;
        let mut slots = Vec::with_capacity(usize::from(count));
        slots.push(ConstantSlot::Reserved);
        let mut index = 1u16;
        while index < count {
            let tag = reader
                .read_u1()
                .map_err(|e| ConstantPoolError::bytes(index, e))?;
            let constant = decode_constant(reader, index, tag, major_version)?;
            let two_slot = matches!(constant, Constant::Long(_) | Constant::Double(_));
            slots.push(ConstantSlot::Entry(constant));
            index += 1;
            if two_slot {
                if index >= count {
                    return Err(ConstantPoolError::new(
                        ConstantPoolErrorKind::InvalidLayout,
                        index - 1,
                        None,
                        "two-slot constant has no trailing unusable index",
                    ));
                }
                slots.push(ConstantSlot::Unusable);
                index += 1;
            }
        }
        let pool = Self { slots };
        pool.validate(major_version)?;
        Ok(pool)
    }

    /// Borrow every physical slot. Index zero and two-slot holes remain explicit.
    pub fn slots(&self) -> &[ConstantSlot] {
        &self.slots
    }

    /// Return a usable entry or a typed located target error.
    pub fn entry(
        &self,
        source_index: u16,
        target_index: u16,
    ) -> Result<&Constant, ConstantPoolError> {
        match self.slots.get(usize::from(target_index)) {
            Some(ConstantSlot::Entry(value)) => Ok(value),
            Some(ConstantSlot::Reserved | ConstantSlot::Unusable) => Err(ConstantPoolError::new(
                ConstantPoolErrorKind::UnusableTarget,
                source_index,
                Some(target_index),
                format!("index {source_index} points at unusable index {target_index}"),
            )),
            None => Err(ConstantPoolError::new(
                ConstantPoolErrorKind::InvalidIndex,
                source_index,
                Some(target_index),
                format!("index {source_index} points outside the pool at index {target_index}"),
            )),
        }
    }

    /// Validate layout, version bounds, reference indices, and target categories.
    pub fn validate(&self, major_version: u16) -> Result<(), ConstantPoolError> {
        if !matches!(self.slots.first(), Some(ConstantSlot::Reserved))
            || self.slots.len() > usize::from(u16::MAX)
        {
            return Err(ConstantPoolError::new(
                ConstantPoolErrorKind::InvalidLayout,
                0,
                None,
                "pool must begin with exactly one reserved slot and fit u16",
            ));
        }
        for (position, slot) in self.slots.iter().enumerate().skip(1) {
            let index = position as u16;
            match slot {
                ConstantSlot::Reserved => {
                    return Err(ConstantPoolError::new(
                        ConstantPoolErrorKind::InvalidLayout,
                        index,
                        None,
                        "reserved slot is only legal at index zero",
                    ));
                }
                ConstantSlot::Unusable => {
                    if !matches!(
                        self.slots.get(position - 1),
                        Some(ConstantSlot::Entry(Constant::Long(_) | Constant::Double(_)))
                    ) {
                        return Err(ConstantPoolError::new(
                            ConstantPoolErrorKind::InvalidLayout,
                            index,
                            None,
                            "unusable slot must follow a long or double",
                        ));
                    }
                }
                ConstantSlot::Entry(value) => {
                    if matches!(
                        self.slots.get(position.wrapping_add(1)),
                        Some(ConstantSlot::Unusable)
                    ) != matches!(value, Constant::Long(_) | Constant::Double(_))
                    {
                        return Err(ConstantPoolError::new(
                            ConstantPoolErrorKind::InvalidLayout,
                            index,
                            None,
                            "long and double entries must own exactly one following unusable slot",
                        ));
                    }
                    let minimum_major = match value {
                        Constant::MethodHandle { .. }
                        | Constant::MethodType { .. }
                        | Constant::InvokeDynamic { .. } => Some(51),
                        Constant::Module { .. } | Constant::Package { .. } => Some(53),
                        Constant::Dynamic { .. } => Some(55),
                        _ => None,
                    };
                    if let Some(minimum_major) = minimum_major
                        && major_version < minimum_major
                    {
                        return Err(ConstantPoolError::new(
                            ConstantPoolErrorKind::Version,
                            index,
                            None,
                            format!(
                                "constant at index {index} requires classfile major version {minimum_major}"
                            ),
                        ));
                    }
                    validate_constant(self, index, value, major_version)?;
                }
            }
        }
        Ok(())
    }

    /// Encode the count and entries after revalidating the pool.
    pub fn encode(
        &self,
        writer: &mut ByteWriter,
        major_version: u16,
    ) -> Result<(), ConstantPoolError> {
        self.validate(major_version)?;
        writer
            .write_u2(self.slots.len() as u16)
            .map_err(|e| ConstantPoolError::bytes(0, e))?;
        for (position, slot) in self.slots.iter().enumerate().skip(1) {
            if let ConstantSlot::Entry(value) = slot {
                encode_constant(writer, position as u16, value)?;
            }
        }
        Ok(())
    }
}

fn decode_constant(
    reader: &mut ByteReader<'_>,
    index: u16,
    tag: u8,
    major: u16,
) -> Result<Constant, ConstantPoolError> {
    let read_u1 = |reader: &mut ByteReader<'_>| {
        reader
            .read_u1()
            .map_err(|e| ConstantPoolError::bytes(index, e))
    };
    let read_u2 = |reader: &mut ByteReader<'_>| {
        reader
            .read_u2()
            .map_err(|e| ConstantPoolError::bytes(index, e))
    };
    let read_u4 = |reader: &mut ByteReader<'_>| {
        reader
            .read_u4()
            .map_err(|e| ConstantPoolError::bytes(index, e))
    };
    let version = |minimum| {
        if major < minimum {
            Err(ConstantPoolError::new(
                ConstantPoolErrorKind::Version,
                index,
                None,
                format!("constant tag {tag} requires classfile major version {minimum}"),
            ))
        } else {
            Ok(())
        }
    };
    Ok(match tag {
        1 => {
            let length = read_u2(reader)?;
            let bytes = reader
                .take(usize::from(length))
                .map_err(|e| ConstantPoolError::bytes(index, e))?;
            Constant::Utf8(
                decode_modified_utf8(bytes, reader.allocation_budget())
                    .map_err(|e| ConstantPoolError::bytes(index, e))?,
            )
        }
        3 => Constant::Integer(read_u4(reader)?),
        4 => Constant::Float(read_u4(reader)?),
        5 => Constant::Long((u64::from(read_u4(reader)?) << 32) | u64::from(read_u4(reader)?)),
        6 => Constant::Double((u64::from(read_u4(reader)?) << 32) | u64::from(read_u4(reader)?)),
        7 => Constant::Class {
            name_index: read_u2(reader)?,
        },
        8 => Constant::String {
            string_index: read_u2(reader)?,
        },
        9 => Constant::Fieldref {
            class_index: read_u2(reader)?,
            name_and_type_index: read_u2(reader)?,
        },
        10 => Constant::Methodref {
            class_index: read_u2(reader)?,
            name_and_type_index: read_u2(reader)?,
        },
        11 => Constant::InterfaceMethodref {
            class_index: read_u2(reader)?,
            name_and_type_index: read_u2(reader)?,
        },
        12 => Constant::NameAndType {
            name_index: read_u2(reader)?,
            descriptor_index: read_u2(reader)?,
        },
        15 => {
            version(51)?;
            Constant::MethodHandle {
                reference_kind: read_u1(reader)?,
                reference_index: read_u2(reader)?,
            }
        }
        16 => {
            version(51)?;
            Constant::MethodType {
                descriptor_index: read_u2(reader)?,
            }
        }
        17 => {
            version(55)?;
            Constant::Dynamic {
                bootstrap_method_attr_index: read_u2(reader)?,
                name_and_type_index: read_u2(reader)?,
            }
        }
        18 => {
            version(51)?;
            Constant::InvokeDynamic {
                bootstrap_method_attr_index: read_u2(reader)?,
                name_and_type_index: read_u2(reader)?,
            }
        }
        19 => {
            version(53)?;
            Constant::Module {
                name_index: read_u2(reader)?,
            }
        }
        20 => {
            version(53)?;
            Constant::Package {
                name_index: read_u2(reader)?,
            }
        }
        _ => {
            return Err(ConstantPoolError::new(
                ConstantPoolErrorKind::UnknownTag,
                index,
                None,
                format!("unknown constant tag {tag}"),
            ));
        }
    })
}

fn expect(
    pool: &ConstantPool,
    source: u16,
    target: u16,
    category: &'static str,
    predicate: impl FnOnce(&Constant) -> bool,
) -> Result<(), ConstantPoolError> {
    let value = pool.entry(source, target)?;
    if predicate(value) {
        Ok(())
    } else {
        Err(ConstantPoolError::new(
            ConstantPoolErrorKind::WrongCategory,
            source,
            Some(target),
            format!("index {source} requires {category} at index {target}"),
        ))
    }
}

fn validate_constant(
    pool: &ConstantPool,
    index: u16,
    value: &Constant,
    major: u16,
) -> Result<(), ConstantPoolError> {
    let utf8 = |value: &Constant| matches!(value, Constant::Utf8(_));
    let class = |value: &Constant| matches!(value, Constant::Class { .. });
    let nat = |value: &Constant| matches!(value, Constant::NameAndType { .. });
    match value {
        Constant::Class { name_index }
        | Constant::String {
            string_index: name_index,
        }
        | Constant::MethodType {
            descriptor_index: name_index,
        }
        | Constant::Module { name_index }
        | Constant::Package { name_index } => expect(pool, index, *name_index, "Utf8", utf8),
        Constant::NameAndType {
            name_index,
            descriptor_index,
        } => {
            expect(pool, index, *name_index, "Utf8", utf8)?;
            expect(pool, index, *descriptor_index, "Utf8", utf8)
        }
        Constant::Fieldref {
            class_index,
            name_and_type_index,
        }
        | Constant::Methodref {
            class_index,
            name_and_type_index,
        }
        | Constant::InterfaceMethodref {
            class_index,
            name_and_type_index,
        } => {
            expect(pool, index, *class_index, "Class", class)?;
            expect(pool, index, *name_and_type_index, "NameAndType", nat)
        }
        Constant::Dynamic {
            name_and_type_index,
            ..
        }
        | Constant::InvokeDynamic {
            name_and_type_index,
            ..
        } => expect(pool, index, *name_and_type_index, "NameAndType", nat),
        Constant::MethodHandle {
            reference_kind,
            reference_index,
        } => {
            let legal = match *reference_kind {
                1..=4 => matches!(
                    pool.entry(index, *reference_index)?,
                    Constant::Fieldref { .. }
                ),
                5 | 8 => matches!(
                    pool.entry(index, *reference_index)?,
                    Constant::Methodref { .. }
                ),
                6 | 7 => {
                    matches!(
                        pool.entry(index, *reference_index)?,
                        Constant::Methodref { .. }
                    ) || (major >= 52
                        && matches!(
                            pool.entry(index, *reference_index)?,
                            Constant::InterfaceMethodref { .. }
                        ))
                }
                9 => matches!(
                    pool.entry(index, *reference_index)?,
                    Constant::InterfaceMethodref { .. }
                ),
                _ => {
                    return Err(ConstantPoolError::new(
                        ConstantPoolErrorKind::ReferenceKind,
                        index,
                        Some(*reference_index),
                        format!("invalid method-handle reference kind {reference_kind}"),
                    ));
                }
            };
            if legal {
                Ok(())
            } else {
                Err(ConstantPoolError::new(
                    ConstantPoolErrorKind::WrongCategory,
                    index,
                    Some(*reference_index),
                    format!(
                        "method-handle kind {reference_kind} has illegal target index {reference_index}"
                    ),
                ))
            }
        }
        _ => Ok(()),
    }
}

fn encode_constant(
    writer: &mut ByteWriter,
    index: u16,
    value: &Constant,
) -> Result<(), ConstantPoolError> {
    macro_rules! write {
        ($method:ident($value:expr)) => {
            writer
                .$method($value)
                .map_err(|e| ConstantPoolError::bytes(index, e))?
        };
    }
    let (tag, fields): (u8, &[u16]) = match value {
        Constant::Class { name_index } => (7, core::slice::from_ref(name_index)),
        Constant::String { string_index } => (8, core::slice::from_ref(string_index)),
        Constant::MethodType { descriptor_index } => (16, core::slice::from_ref(descriptor_index)),
        Constant::Module { name_index } => (19, core::slice::from_ref(name_index)),
        Constant::Package { name_index } => (20, core::slice::from_ref(name_index)),
        _ => (0, &[]),
    };
    if tag != 0 {
        write!(write_u1(tag));
        for field in fields {
            write!(write_u2(*field));
        }
        return Ok(());
    }
    match value {
        Constant::Utf8(text) => {
            write!(write_u1(1));
            let bytes = encode_modified_utf8(text, usize::from(u16::MAX))
                .map_err(|e| ConstantPoolError::bytes(index, e))?;
            let length = u16::try_from(bytes.len()).map_err(|_| {
                ConstantPoolError::new(
                    ConstantPoolErrorKind::InvalidLayout,
                    index,
                    None,
                    "Utf8 constant exceeds u16 length",
                )
            })?;
            write!(write_u2(length));
            writer
                .write_bytes(&bytes)
                .map_err(|e| ConstantPoolError::bytes(index, e))?;
        }
        Constant::Integer(bits) => {
            write!(write_u1(3));
            write!(write_u4(*bits));
        }
        Constant::Float(bits) => {
            write!(write_u1(4));
            write!(write_u4(*bits));
        }
        Constant::Long(bits) => {
            write!(write_u1(5));
            write!(write_u4((*bits >> 32) as u32));
            write!(write_u4(*bits as u32));
        }
        Constant::Double(bits) => {
            write!(write_u1(6));
            write!(write_u4((*bits >> 32) as u32));
            write!(write_u4(*bits as u32));
        }
        Constant::Fieldref {
            class_index,
            name_and_type_index,
        } => {
            write!(write_u1(9));
            write!(write_u2(*class_index));
            write!(write_u2(*name_and_type_index));
        }
        Constant::Methodref {
            class_index,
            name_and_type_index,
        } => {
            write!(write_u1(10));
            write!(write_u2(*class_index));
            write!(write_u2(*name_and_type_index));
        }
        Constant::InterfaceMethodref {
            class_index,
            name_and_type_index,
        } => {
            write!(write_u1(11));
            write!(write_u2(*class_index));
            write!(write_u2(*name_and_type_index));
        }
        Constant::NameAndType {
            name_index,
            descriptor_index,
        } => {
            write!(write_u1(12));
            write!(write_u2(*name_index));
            write!(write_u2(*descriptor_index));
        }
        Constant::MethodHandle {
            reference_kind,
            reference_index,
        } => {
            write!(write_u1(15));
            write!(write_u1(*reference_kind));
            write!(write_u2(*reference_index));
        }
        Constant::Dynamic {
            bootstrap_method_attr_index,
            name_and_type_index,
        } => {
            write!(write_u1(17));
            write!(write_u2(*bootstrap_method_attr_index));
            write!(write_u2(*name_and_type_index));
        }
        Constant::InvokeDynamic {
            bootstrap_method_attr_index,
            name_and_type_index,
        } => {
            write!(write_u1(18));
            write!(write_u2(*bootstrap_method_attr_index));
            write!(write_u2(*name_and_type_index));
        }
        Constant::Class { .. }
        | Constant::String { .. }
        | Constant::MethodType { .. }
        | Constant::Module { .. }
        | Constant::Package { .. } => unreachable!(),
    }
    Ok(())
}
