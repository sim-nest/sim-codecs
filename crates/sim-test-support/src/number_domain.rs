//! A test-only `numbers/f64` number-domain fixture and registration helper for
//! codec number-literal handling at the codec boundary.

use std::sync::Arc;

use sim_citizen_derive::non_citizen;
use sim_kernel::{
    ClassRef, Cx, Expr, NumberDomain, NumberLiteral, Object, ObjectCompat, Symbol, Value,
};

#[non_citizen(
    reason = "test-only f64 number-domain fixture for codec boundary tests",
    kind = "test-fixture",
    descriptor = "numbers/f64"
)]
struct TestF64NumberDomain;

impl NumberDomain for TestF64NumberDomain {
    fn symbol(&self) -> Symbol {
        Symbol::qualified("numbers", "f64")
    }

    fn parse_literal(&self, cx: &mut Cx, text: &str) -> sim_kernel::Result<Option<Value>> {
        let Ok(value) = text.parse::<f64>() else {
            return Ok(None);
        };
        cx.factory()
            .number_literal(self.symbol(), value.to_string())
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

impl Object for TestF64NumberDomain {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("#<number-domain numbers/f64>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for TestF64NumberDomain {
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

    fn as_number_domain(&self) -> Option<&dyn NumberDomain> {
        Some(self)
    }
}

/// Register a tiny `numbers/f64` domain fixture for codec tests.
///
/// This fixture lives beside the codec crates so codec tests can parse and
/// encode basic f64 literals without depending on the higher-level
/// `sim-lib-numbers-f64` implementation crate.
pub fn register_f64_number_domain(cx: &mut Cx) {
    let symbol = Symbol::qualified("numbers", "f64");
    let value = cx
        .factory()
        .opaque(Arc::new(TestF64NumberDomain))
        .expect("test f64 domain should be boxable");
    cx.registry_mut()
        .register_number_domain_value(symbol, value)
        .expect("register test f64 number domain");
}
