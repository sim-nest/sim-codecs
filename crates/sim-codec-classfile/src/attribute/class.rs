/// One bootstrap method and its ordered, arity-preserving constant-pool arguments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapMethod {
    /// Constant-pool index of the method handle.
    pub method_ref: u16,
    /// Constant-pool argument indices in invocation order.
    pub arguments: Vec<u16>,
}

/// An ordered `BootstrapMethods` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapMethodsAttribute {
    /// Bootstrap methods in classfile index order.
    pub methods: Vec<BootstrapMethod>,
}

impl BootstrapMethodsAttribute {
    /// Decode without resolving method handles, arguments, or dynamic constants.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut methods = Vec::with_capacity(n);
        for _ in 0..n {
            let method_ref = reader.read_u2()?;
            let argc = usize::from(reader.read_u2()?);
            reader.preflight_allocation(argc)?;
            let mut arguments = Vec::with_capacity(argc);
            for _ in 0..argc {
                arguments.push(reader.read_u2()?)
            }
            methods.push(BootstrapMethod {
                method_ref,
                arguments,
            });
        }
        finish(reader)?;
        Ok(Self { methods })
    }
    /// Encode method and argument sequences exactly in stored order.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(count(self.methods.len(), "bootstrap methods")?)?;
        for method in &self.methods {
            out.write_u2(method.method_ref)?;
            out.write_u2(count(method.arguments.len(), "bootstrap arguments")?)?;
            for argument in &method.arguments {
                out.write_u2(*argument)?
            }
        }
        Ok(out.into_bytes())
    }
}

/// One nested attribute attached to a record component.
pub type RecordComponentAttribute = NestedAttribute;

/// One component in a `Record` attribute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordComponent {
    /// Component name index.
    pub name_index: u16,
    /// Component descriptor index.
    pub descriptor_index: u16,
    /// Component attributes in classfile order.
    pub attributes: Vec<RecordComponentAttribute>,
}

/// The ordered `Record` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordAttribute {
    /// Components in declaration order.
    pub components: Vec<RecordComponent>,
}

impl RecordAttribute {
    /// Decode components and retain every nested attribute as exact bytes.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut components = Vec::with_capacity(n);
        for _ in 0..n {
            let name_index = reader.read_u2()?;
            let descriptor_index = reader.read_u2()?;
            let attribute_count = usize::from(reader.read_u2()?);
            reader.preflight_allocation(attribute_count)?;
            let mut attributes = Vec::with_capacity(attribute_count);
            for order in 0..attribute_count {
                let start = reader.offset();
                let name_index = reader.read_u2()?;
                let declared_length = reader.read_u4()?;
                let length = usize::try_from(declared_length).map_err(|_| {
                    error(
                        AttributeErrorKind::StaticConstraint,
                        reader.offset(),
                        "record component attribute length is not addressable",
                    )
                })?;
                attributes.push(NestedAttribute {
                    name_index,
                    owner: NestedAttributeOwner::RecordComponent,
                    order,
                    declared_length,
                    bytes: reader.take(length)?.to_vec(),
                    origin: annotation_origin(start, reader),
                });
            }
            components.push(RecordComponent {
                name_index,
                descriptor_index,
                attributes,
            });
        }
        finish(reader)?;
        Ok(Self { components })
    }

    /// Encode component and nested-attribute order exactly as stored.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(count(self.components.len(), "record components")?)?;
        for component in &self.components {
            out.write_u2(component.name_index)?;
            out.write_u2(component.descriptor_index)?;
            out.write_u2(count(
                component.attributes.len(),
                "record component attributes",
            )?)?;
            for attribute in &component.attributes {
                if usize::try_from(attribute.declared_length).ok() != Some(attribute.bytes.len()) {
                    return Err(error(
                        AttributeErrorKind::StaticConstraint,
                        attribute.origin.start,
                        "record component attribute declared length differs from retained bytes",
                    ));
                }
                out.write_u2(attribute.name_index)?;
                out.write_u4(attribute.declared_length)?;
                out.write_bytes(&attribute.bytes)?;
            }
        }
        Ok(out.into_bytes())
    }
}

/// One `requires` directive in a `Module` attribute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRequire {
    /// Module constant index.
    pub module_index: u16,
    /// Raw requires flags.
    pub flags: u16,
    /// Version string index, or zero.
    pub version_index: u16,
}
/// One `exports` or `opens` directive in a `Module` attribute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleExport {
    /// Package constant index.
    pub package_index: u16,
    /// Raw directive flags.
    pub flags: u16,
    /// Target module indices in encoded order.
    pub targets: Vec<u16>,
}
/// One `provides` directive in a `Module` attribute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleProvide {
    /// Service class index.
    pub service_index: u16,
    /// Provider class indices in encoded order.
    pub providers: Vec<u16>,
}
/// The complete structural `Module` payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleAttribute {
    /// Module constant index.
    pub name_index: u16,
    /// Raw module flags.
    pub flags: u16,
    /// Module version string index, or zero.
    pub version_index: u16,
    /// Required modules in declaration order.
    pub requires: Vec<ModuleRequire>,
    /// Export directives in declaration order.
    pub exports: Vec<ModuleExport>,
    /// Open directives in declaration order.
    pub opens: Vec<ModuleExport>,
    /// Used service class indices in declaration order.
    pub uses: Vec<u16>,
    /// Service provider directives in declaration order.
    pub provides: Vec<ModuleProvide>,
}

fn decode_module_exports(reader: &mut ByteReader<'_>) -> Result<Vec<ModuleExport>, AttributeError> {
    let n = usize::from(reader.read_u2()?);
    reader.preflight_allocation(n)?;
    let mut values = Vec::with_capacity(n);
    for _ in 0..n {
        let package_index = reader.read_u2()?;
        let flags = reader.read_u2()?;
        let m = usize::from(reader.read_u2()?);
        reader.preflight_allocation(m)?;
        let mut targets = Vec::with_capacity(m);
        for _ in 0..m {
            targets.push(reader.read_u2()?);
        }
        values.push(ModuleExport {
            package_index,
            flags,
            targets,
        });
    }
    Ok(values)
}
fn encode_module_exports(
    values: &[ModuleExport],
    out: &mut ByteWriter,
    what: &str,
) -> Result<(), AttributeError> {
    out.write_u2(count(values.len(), what)?)?;
    for v in values {
        out.write_u2(v.package_index)?;
        out.write_u2(v.flags)?;
        out.write_u2(count(v.targets.len(), "module directive targets")?)?;
        for target in &v.targets {
            out.write_u2(*target)?;
        }
    }
    Ok(())
}

impl ModuleAttribute {
    /// Decode all module directives without resolving or interpreting them.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, AttributeError> {
        let name_index = reader.read_u2()?;
        let flags = reader.read_u2()?;
        let version_index = reader.read_u2()?;
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut requires = Vec::with_capacity(n);
        for _ in 0..n {
            requires.push(ModuleRequire {
                module_index: reader.read_u2()?,
                flags: reader.read_u2()?,
                version_index: reader.read_u2()?,
            });
        }
        let exports = decode_module_exports(reader)?;
        let opens = decode_module_exports(reader)?;
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut uses = Vec::with_capacity(n);
        for _ in 0..n {
            uses.push(reader.read_u2()?);
        }
        let n = usize::from(reader.read_u2()?);
        reader.preflight_allocation(n)?;
        let mut provides = Vec::with_capacity(n);
        for _ in 0..n {
            let service_index = reader.read_u2()?;
            let m = usize::from(reader.read_u2()?);
            reader.preflight_allocation(m)?;
            let mut providers = Vec::with_capacity(m);
            for _ in 0..m {
                providers.push(reader.read_u2()?);
            }
            provides.push(ModuleProvide {
                service_index,
                providers,
            });
        }
        finish(reader)?;
        Ok(Self {
            name_index,
            flags,
            version_index,
            requires,
            exports,
            opens,
            uses,
            provides,
        })
    }
    /// Encode every directive exactly in stored order.
    pub fn encode(&self, budget: usize) -> Result<Vec<u8>, AttributeError> {
        let mut out = ByteWriter::new(budget);
        out.write_u2(self.name_index)?;
        out.write_u2(self.flags)?;
        out.write_u2(self.version_index)?;
        out.write_u2(count(self.requires.len(), "module requires")?)?;
        for v in &self.requires {
            out.write_u2(v.module_index)?;
            out.write_u2(v.flags)?;
            out.write_u2(v.version_index)?;
        }
        encode_module_exports(&self.exports, &mut out, "module exports")?;
        encode_module_exports(&self.opens, &mut out, "module opens")?;
        out.write_u2(count(self.uses.len(), "module uses")?)?;
        for v in &self.uses {
            out.write_u2(*v)?;
        }
        out.write_u2(count(self.provides.len(), "module provides")?)?;
        for v in &self.provides {
            out.write_u2(v.service_index)?;
            out.write_u2(count(v.providers.len(), "module providers")?)?;
            for provider in &v.providers {
                out.write_u2(*provider)?;
            }
        }
        Ok(out.into_bytes())
    }
}

/// Earliest classfile major version for a standard attribute.
pub fn standard_attribute_min_major(name: &str) -> Option<u16> {
    match name {
        "Signature"
        | "SourceDebugExtension"
        | "LocalVariableTypeTable"
        | "EnclosingMethod"
        | "RuntimeVisibleAnnotations"
        | "RuntimeInvisibleAnnotations"
        | "RuntimeVisibleParameterAnnotations"
        | "RuntimeInvisibleParameterAnnotations"
        | "AnnotationDefault" => Some(49),
        "StackMapTable" => Some(50),
        "BootstrapMethods" => Some(51),
        "MethodParameters"
        | "RuntimeVisibleTypeAnnotations"
        | "RuntimeInvisibleTypeAnnotations" => Some(52),
        "Module" | "ModulePackages" | "ModuleMainClass" => Some(53),
        "NestHost" | "NestMembers" => Some(55),
        "Record" => Some(60),
        "PermittedSubclasses" => Some(61),
        "ConstantValue" | "Code" | "Exceptions" | "InnerClasses" | "Synthetic" | "SourceFile"
        | "LineNumberTable" | "LocalVariableTable" | "Deprecated" => Some(45),
        _ => None,
    }
}
