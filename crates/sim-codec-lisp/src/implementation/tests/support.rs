pub(super) use std::sync::Arc;

pub(super) use sim_codec::{
    CodecRuntime, DecodeLimits, Input, ReadCx, decode_tree_with_codec_and_limits,
    decode_with_codec, decode_with_codec_and_limits, encode_with_codec,
};
pub(super) use sim_kernel::{
    Args, Callable, Class, ClassId, ClassRef, Cx, DefaultFactory, EagerPolicy, EncodePosition,
    Expr, Factory, NumberLiteral, Object, ObjectEncode, ObjectEncoding, ReadPolicy, ShapeRef,
    SourceId, Symbol, TableRef, Trivia, Value, WriteCx as KernelWriteCx, read_construct_capability,
    read_eval_capability,
};

pub(super) use super::super::{
    LispCodecLib, decode_lisp_located, decode_lisp_tree, encode_object_lisp,
};
pub(super) use crate::implementation::forms::lower_eval_surface;

#[derive(Clone)]
pub(super) struct PointValue {
    pub(super) args: Vec<Expr>,
    pub(super) fields: Vec<(Symbol, Value)>,
}

pub(super) struct RationalDomain;

pub(super) struct ComplexDomain;

impl sim_kernel::NumberDomain for RationalDomain {
    fn symbol(&self) -> Symbol {
        Symbol::qualified("numbers", "rational")
    }

    fn parse_literal(&self, cx: &mut Cx, text: &str) -> sim_kernel::Result<Option<Value>> {
        let Some((left, right)) = text.split_once('/') else {
            return Ok(None);
        };
        if left.parse::<i64>().is_err() || right.parse::<i64>().is_err() {
            return Ok(None);
        }
        cx.factory()
            .number_literal(self.symbol(), text.to_owned())
            .map(Some)
    }

    fn encode_literal(
        &self,
        cx: &mut Cx,
        value: Value,
    ) -> sim_kernel::Result<Option<NumberLiteral>> {
        match value.object().as_expr(cx)? {
            Expr::Number(number) if number.domain == self.symbol() => Ok(Some(number)),
            _ => Ok(None),
        }
    }
}

impl Object for RationalDomain {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("#<number-domain numbers/rational>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for RationalDomain {
    fn class(&self, cx: &mut Cx) -> sim_kernel::Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "NumberDomain"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_NUMBER_DOMAIN_CLASS_ID,
            Symbol::qualified("core", "NumberDomain"),
        )
    }
    fn as_number_domain(&self) -> Option<&dyn sim_kernel::NumberDomain> {
        Some(self)
    }
}

impl sim_kernel::NumberDomain for ComplexDomain {
    fn symbol(&self) -> Symbol {
        Symbol::qualified("numbers", "complex")
    }

    fn parse_literal(&self, cx: &mut Cx, text: &str) -> sim_kernel::Result<Option<Value>> {
        let Some(stripped) = text.strip_suffix('i') else {
            return Ok(None);
        };
        let split = stripped
            .char_indices()
            .skip(1)
            .find(|(_, ch)| *ch == '+' || *ch == '-')
            .map(|(index, _)| index);
        let Some(index) = split else {
            return Ok(None);
        };
        let (left, right) = stripped.split_at(index);
        if left.parse::<f64>().is_err() || right.parse::<f64>().is_err() {
            return Ok(None);
        }
        cx.factory()
            .number_literal(self.symbol(), text.to_owned())
            .map(Some)
    }

    fn encode_literal(
        &self,
        cx: &mut Cx,
        value: Value,
    ) -> sim_kernel::Result<Option<NumberLiteral>> {
        match value.object().as_expr(cx)? {
            Expr::Number(number) if number.domain == self.symbol() => Ok(Some(number)),
            _ => Ok(None),
        }
    }
}

impl Object for ComplexDomain {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("#<number-domain numbers/complex>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for ComplexDomain {
    fn class(&self, cx: &mut Cx) -> sim_kernel::Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "NumberDomain"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_NUMBER_DOMAIN_CLASS_ID,
            Symbol::qualified("core", "NumberDomain"),
        )
    }
    fn as_number_domain(&self) -> Option<&dyn sim_kernel::NumberDomain> {
        Some(self)
    }
}

impl Object for PointValue {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("#<instance Point>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for PointValue {
    fn class(&self, cx: &mut Cx) -> sim_kernel::Result<ClassRef> {
        cx.resolve_class(&Symbol::new("Point"))
    }
    fn as_expr(&self, cx: &mut Cx) -> sim_kernel::Result<Expr> {
        Ok(Expr::Map(
            self.fields
                .iter()
                .map(|(key, value)| Ok((Expr::Symbol(key.clone()), value.object().as_expr(cx)?)))
                .collect::<sim_kernel::Result<Vec<_>>>()?,
        ))
    }
    fn as_object_encoder(&self) -> Option<&dyn ObjectEncode> {
        Some(self)
    }
}

impl ObjectEncode for PointValue {
    fn object_encoding(&self, _cx: &mut Cx) -> sim_kernel::Result<ObjectEncoding> {
        Ok(ObjectEncoding::Constructor {
            class: Symbol::new("Point"),
            args: self.args.clone(),
        })
    }
}

#[derive(Clone)]
pub(super) struct PointClass;

impl Object for PointClass {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("#<class Point>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for PointClass {
    fn class(&self, cx: &mut Cx) -> sim_kernel::Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Class"))
    }
    fn as_expr(&self, _cx: &mut Cx) -> sim_kernel::Result<Expr> {
        Ok(Expr::Symbol(Symbol::new("Point")))
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
    fn as_class(&self) -> Option<&dyn Class> {
        Some(self)
    }
}

impl Callable for PointClass {
    fn call(&self, cx: &mut Cx, args: Args) -> sim_kernel::Result<Value> {
        let values = args.into_vec();
        let exprs = values
            .iter()
            .map(|value| value.object().as_expr(cx))
            .collect::<sim_kernel::Result<Vec<_>>>()?;
        let fields = vec![
            (Symbol::new("x"), values[0].clone()),
            (Symbol::new("y"), values[1].clone()),
        ];
        cx.factory().opaque(Arc::new(PointValue {
            args: exprs,
            fields,
        }))
    }
}

impl Class for PointClass {
    fn id(&self) -> ClassId {
        ClassId(100)
    }

    fn symbol(&self) -> Symbol {
        Symbol::new("Point")
    }

    fn constructor_shape(&self, cx: &mut Cx) -> sim_kernel::Result<ShapeRef> {
        cx.factory().nil()
    }

    fn instance_shape(&self, cx: &mut Cx) -> sim_kernel::Result<ShapeRef> {
        cx.factory().nil()
    }

    fn read_constructor(
        &self,
        _cx: &mut Cx,
    ) -> sim_kernel::Result<Option<sim_kernel::ReadConstructorRef>> {
        Ok(Some(
            DefaultFactory
                .opaque(Arc::new(PointReadConstructor))
                .unwrap(),
        ))
    }

    fn members(&self, cx: &mut Cx) -> sim_kernel::Result<TableRef> {
        cx.factory().table(Vec::new())
    }
}

#[derive(Clone)]
pub(super) struct PointReadConstructor;

impl Object for PointReadConstructor {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("#<read-constructor Point>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for PointReadConstructor {
    fn class(&self, cx: &mut Cx) -> sim_kernel::Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }
    fn as_read_constructor(&self) -> Option<&dyn sim_kernel::ReadConstructor> {
        Some(self)
    }
}

impl sim_kernel::ReadConstructor for PointReadConstructor {
    fn symbol(&self) -> Symbol {
        Symbol::new("Point")
    }

    fn args_shape(&self, cx: &mut Cx) -> sim_kernel::Result<ShapeRef> {
        cx.factory().nil()
    }

    fn construct_read(&self, cx: &mut Cx, args: Vec<Value>) -> sim_kernel::Result<Value> {
        PointClass.call(cx, Args::new(args))
    }
}

pub(super) fn install_point(cx: &mut Cx) {
    let point_class = cx.factory().opaque(Arc::new(PointClass)).unwrap();
    cx.registry_mut()
        .register_class_value(Symbol::new("Point"), point_class)
        .unwrap();
}

pub(super) fn cx() -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    sim_test_support::register_f64_number_domain(&mut cx);
    cx
}

pub(super) fn register_lisp_codec(cx: &mut Cx) {
    let lib = LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    cx.load_lib(&lib).unwrap();
}

pub(super) fn runtime_codec_id(cx: &mut Cx, symbol: &Symbol) -> sim_kernel::CodecId {
    cx.resolve_codec(symbol)
        .unwrap()
        .object()
        .as_any()
        .downcast_ref::<CodecRuntime>()
        .unwrap()
        .id
}

pub(super) fn assert_codec_error(
    err: sim_kernel::Error,
    expected: sim_kernel::CodecId,
    needle: &str,
) {
    match err {
        sim_kernel::Error::CodecError { codec, message } => {
            assert_eq!(codec, expected);
            assert_ne!(codec, sim_kernel::CodecId(0));
            assert!(message.contains(needle), "{message}");
        }
        other => panic!("unexpected error {other:?}"),
    }
}

pub(super) fn policy_with(capabilities: Vec<sim_kernel::CapabilityName>) -> ReadPolicy {
    ReadPolicy {
        trust: sim_kernel::TrustLevel::TrustedSource,
        capabilities: capabilities
            .into_iter()
            .fold(sim_kernel::CapabilitySet::new(), |set, capability| {
                set.grant(capability)
            }),
    }
}
