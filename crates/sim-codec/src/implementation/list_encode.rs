//! Shared value-to-`Expr` encode machinery for lists and nested data.
//!
//! Forces list values into bounded vectors and walks values (lists, tables,
//! atoms) into the kernel `Expr` graph for encoding.

use sim_kernel::{Cx, Error, Expr, Result, Value, WriteCx, force_list_to_vec};

/// Force a list value into a bounded Vec for v1 encoding.
pub fn force_list_for_encode(cx: &mut Cx, list: &Value) -> Result<Vec<Value>> {
    let lv = list
        .object()
        .as_list()
        .ok_or_else(|| Error::Eval("encode: value is not a list".to_owned()))?;
    force_list_to_vec(cx, lv, "encode")
}

fn force_table_for_encode(cx: &mut Cx, table: &Value) -> Result<Expr> {
    let table = table
        .object()
        .as_table_impl()
        .ok_or_else(|| Error::Eval("encode: value is not a table".to_owned()))?;
    let expr = table.as_table_expr(cx)?;
    if matches!(expr, Expr::Map(_)) {
        Ok(expr)
    } else {
        Err(Error::Eval(
            "encode: table backend did not materialize to Expr::Map".to_owned(),
        ))
    }
}

/// Walk a runtime `Value` into a kernel `Expr` ready for encoding.
///
/// Lists and tables are forced and recursed into; every other value is lowered
/// through its own [`as_expr`](sim_kernel::ObjectCompat::as_expr). This is the
/// value-side entry point that [`encode_value_with_codec`](crate::encode_value_with_codec)
/// runs before handing the `Expr` to a codec.
pub fn encode_value_expr(cx: &mut WriteCx<'_>, value: &Value) -> Result<Expr> {
    if value.object().as_list().is_some() {
        return Ok(Expr::List(
            force_list_for_encode(cx.cx, value)?
                .into_iter()
                .map(|item| encode_value_expr(cx, &item))
                .collect::<Result<Vec<_>>>()?,
        ));
    }

    if value.object().as_table_impl().is_some() {
        return force_table_for_encode(cx.cx, value);
    }

    value.object().as_expr(cx.cx)
}
