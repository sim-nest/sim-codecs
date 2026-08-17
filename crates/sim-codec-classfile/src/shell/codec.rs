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
