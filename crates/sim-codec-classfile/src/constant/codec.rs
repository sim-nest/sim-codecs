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
