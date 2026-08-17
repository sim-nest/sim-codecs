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
