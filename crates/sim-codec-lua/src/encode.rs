use sim_codec::Output;
use sim_kernel::{Error, Expr, Result, Symbol};

/// Encodes a lowered Lua chunk expression back to Lua source text.
pub fn encode_lua_chunk_expr(expr: &Expr) -> Result<Output> {
    Ok(Output::Text(match lua_call(expr) {
        Some(("chunk", args)) => join_statements(args)?,
        _ => encode_expr(expr)?,
    }))
}

fn join_statements(args: &[Expr]) -> Result<String> {
    args.iter()
        .map(encode_stmt)
        .collect::<Result<Vec<_>>>()
        .map(|items| items.join("\n"))
}

fn encode_stmt(expr: &Expr) -> Result<String> {
    let Some((head, args)) = lua_call(expr) else {
        return encode_expr(expr);
    };
    match head {
        "block" => join_statements(args),
        "local" if args.len() == 2 => encode_local(args),
        "assign" if args.len() == 2 => Ok(format!(
            "{} = {}",
            encode_expr_vec(&args[0])?,
            encode_expr_vec(&args[1])?
        )),
        "if" => encode_if(args),
        "while" if args.len() == 2 => Ok(format!(
            "while {} do\n{}\nend",
            encode_expr(&args[0])?,
            indent(&encode_stmt(&args[1])?)
        )),
        "repeat" if args.len() == 2 => Ok(format!(
            "repeat\n{}\nuntil {}",
            indent(&encode_stmt(&args[0])?),
            encode_expr(&args[1])?
        )),
        "for-range" if args.len() == 5 => encode_for_range(args),
        "for-in" if args.len() == 3 => Ok(format!(
            "for {} in {} do\n{}\nend",
            encode_symbol_list(&args[0])?,
            encode_expr_vec(&args[1])?,
            indent(&encode_stmt(&args[2])?)
        )),
        "function-decl" if args.len() == 3 => Ok(format!(
            "function {}({})\n{}\nend",
            encode_function_name(&args[0])?,
            encode_params(&args[1])?,
            indent(&encode_stmt(&args[2])?)
        )),
        "local-function" if args.len() == 3 => Ok(format!(
            "local function {}({})\n{}\nend",
            encode_symbol(&args[0])?,
            encode_params(&args[1])?,
            indent(&encode_stmt(&args[2])?)
        )),
        "return" => Ok(if args.is_empty() {
            "return".to_owned()
        } else {
            format!("return {}", encode_exprs(args)?)
        }),
        "break" if args.is_empty() => Ok("break".to_owned()),
        "label" if args.len() == 1 => Ok(format!("::{}::", encode_symbol(&args[0])?)),
        "goto" if args.len() == 1 => Ok(format!("goto {}", encode_symbol(&args[0])?)),
        "do" if args.len() == 1 => Ok(format!("do\n{}\nend", indent(&encode_stmt(&args[0])?))),
        "expr" if args.len() == 1 => encode_expr(&args[0]),
        _ => encode_expr(expr),
    }
}

fn encode_local(args: &[Expr]) -> Result<String> {
    let bindings = match &args[0] {
        Expr::Vector(items) => items
            .iter()
            .map(encode_binding)
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(error("lua/local bindings must be a vector")),
    };
    let values = expr_vec_items(&args[1])?;
    if values.is_empty() {
        return Ok(format!("local {}", bindings.join(", ")));
    }
    Ok(format!(
        "local {} = {}",
        bindings.join(", "),
        encode_exprs(values)?
    ))
}

fn encode_binding(expr: &Expr) -> Result<String> {
    if let Expr::Symbol(symbol) = expr {
        return Ok(symbol.name.to_string());
    }
    let Some(("binding", args)) = lua_call(expr) else {
        return Err(error("lua binding must be a symbol or lua/binding"));
    };
    if args.len() != 2 {
        return Err(error("lua/binding expects name and attribute"));
    }
    Ok(format!(
        "{} <{}>",
        encode_symbol(&args[0])?,
        encode_symbol(&args[1])?
    ))
}

fn encode_if(args: &[Expr]) -> Result<String> {
    if args.len() < 2 {
        return Err(error("lua/if expects condition and block"));
    }
    let mut out = format!(
        "if {} then\n{}",
        encode_expr(&args[0])?,
        indent(&encode_stmt(&args[1])?)
    );
    for arm in &args[2..] {
        match lua_call(arm) {
            Some(("elseif", arm_args)) if arm_args.len() == 2 => {
                out.push_str(&format!(
                    "\nelseif {} then\n{}",
                    encode_expr(&arm_args[0])?,
                    indent(&encode_stmt(&arm_args[1])?)
                ));
            }
            Some(("else", arm_args)) if arm_args.len() == 1 => {
                out.push_str(&format!("\nelse\n{}", indent(&encode_stmt(&arm_args[0])?)));
            }
            _ => return Err(error("lua/if arm must be lua/elseif or lua/else")),
        }
    }
    out.push_str("\nend");
    Ok(out)
}

fn encode_for_range(args: &[Expr]) -> Result<String> {
    let mut header = format!(
        "for {} = {}, {}",
        encode_symbol(&args[0])?,
        encode_expr(&args[1])?,
        encode_expr(&args[2])?
    );
    if args[3] != Expr::Nil {
        header.push_str(&format!(", {}", encode_expr(&args[3])?));
    }
    Ok(format!(
        "{header} do\n{}\nend",
        indent(&encode_stmt(&args[4])?)
    ))
}

fn encode_expr(expr: &Expr) -> Result<String> {
    if let Some((head, args)) = lua_call(expr) {
        return encode_lua_expr_call(head, args);
    }
    match expr {
        Expr::Nil => Ok("nil".to_owned()),
        Expr::Bool(true) => Ok("true".to_owned()),
        Expr::Bool(false) => Ok("false".to_owned()),
        Expr::Number(number) => Ok(number.canonical.clone()),
        Expr::String(text) => Ok(quote_string(text)),
        Expr::Symbol(symbol) | Expr::Local(symbol) => Ok(symbol.name.to_string()),
        other => Err(error(format!("cannot encode expression as lua: {other:?}"))),
    }
}

fn encode_lua_expr_call(head: &str, args: &[Expr]) -> Result<String> {
    match head {
        "vararg" if args.is_empty() => Ok("...".to_owned()),
        "index" if args.len() == 2 => encode_index(args),
        "call" if !args.is_empty() => Ok(format!(
            "{}({})",
            encode_expr(&args[0])?,
            encode_exprs(&args[1..])?
        )),
        "method-call" if args.len() >= 2 => Ok(format!(
            "{}:{}({})",
            encode_expr(&args[0])?,
            encode_symbol(&args[1])?,
            encode_exprs(&args[2..])?
        )),
        "function" if args.len() == 2 => Ok(format!(
            "function({})\n{}\nend",
            encode_params(&args[0])?,
            indent(&encode_stmt(&args[1])?)
        )),
        "table" => Ok(format!(
            "{{{}}}",
            args.iter()
                .map(encode_field)
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        )),
        _ if let Some(op) = binary_operator(head)
            && args.len() == 2 =>
        {
            Ok(format!(
                "({} {op} {})",
                encode_expr(&args[0])?,
                encode_expr(&args[1])?
            ))
        }
        _ if let Some(op) = unary_operator(head)
            && args.len() == 1 =>
        {
            Ok(format!("{op} {}", encode_expr(&args[0])?))
        }
        _ => Err(error(format!("unsupported lua expression head lua/{head}"))),
    }
}

fn encode_index(args: &[Expr]) -> Result<String> {
    if let Expr::String(field) = &args[1]
        && is_identifier(field)
    {
        return Ok(format!("{}.{}", encode_expr(&args[0])?, field));
    }
    Ok(format!(
        "{}[{}]",
        encode_expr(&args[0])?,
        encode_expr(&args[1])?
    ))
}

fn encode_field(expr: &Expr) -> Result<String> {
    match lua_call(expr) {
        Some(("field", args)) if args.len() == 1 => encode_expr(&args[0]),
        Some(("named-field", args)) if args.len() == 2 => Ok(format!(
            "{} = {}",
            encode_symbol(&args[0])?,
            encode_expr(&args[1])?
        )),
        Some(("keyed-field", args)) if args.len() == 2 => Ok(format!(
            "[{}] = {}",
            encode_expr(&args[0])?,
            encode_expr(&args[1])?
        )),
        _ => Err(error("lua table field must be a lua field form")),
    }
}

fn encode_function_name(expr: &Expr) -> Result<String> {
    if let Expr::Symbol(symbol) = expr {
        return Ok(symbol.name.to_string());
    }
    let Some(("name", args)) = lua_call(expr) else {
        return Err(error("lua function name must be symbol or lua/name"));
    };
    if args.len() != 3 {
        return Err(error("lua/name expects base, fields, method"));
    }
    let mut out = encode_symbol(&args[0])?;
    for field in expr_vec_items(&args[1])? {
        out.push('.');
        out.push_str(&encode_symbol(field)?);
    }
    if args[2] != Expr::Nil {
        out.push(':');
        out.push_str(&encode_symbol(&args[2])?);
    }
    Ok(out)
}

fn encode_params(expr: &Expr) -> Result<String> {
    let Expr::List(items) = expr else {
        return Err(error("lua params must be a list"));
    };
    items
        .iter()
        .map(|item| match lua_call(item) {
            Some(("vararg", [])) => Ok("...".to_owned()),
            _ => encode_symbol(item),
        })
        .collect::<Result<Vec<_>>>()
        .map(|items| items.join(", "))
}

fn encode_symbol_list(expr: &Expr) -> Result<String> {
    let Expr::List(items) = expr else {
        return Err(error("lua symbol list must be a list"));
    };
    items
        .iter()
        .map(encode_symbol)
        .collect::<Result<Vec<_>>>()
        .map(|items| items.join(", "))
}

fn encode_expr_vec(expr: &Expr) -> Result<String> {
    expr_vec_items(expr).and_then(encode_exprs)
}

fn expr_vec_items(expr: &Expr) -> Result<&[Expr]> {
    match expr {
        Expr::Vector(items) => Ok(items),
        _ => Err(error("lua expression collection must be a vector")),
    }
}

fn encode_exprs(args: &[Expr]) -> Result<String> {
    args.iter()
        .map(encode_expr)
        .collect::<Result<Vec<_>>>()
        .map(|items| items.join(", "))
}

fn encode_symbol(expr: &Expr) -> Result<String> {
    let Expr::Symbol(symbol) = expr else {
        return Err(error("expected lua symbol"));
    };
    Ok(symbol.name.to_string())
}

fn lua_call(expr: &Expr) -> Option<(&str, &[Expr])> {
    let Expr::Call { operator, args } = expr else {
        return None;
    };
    let Expr::Symbol(Symbol { namespace, name }) = operator.as_ref() else {
        return None;
    };
    (namespace.as_ref().map(|value| value.as_ref()) == Some("lua"))
        .then_some((name.as_ref(), args.as_slice()))
}

fn binary_operator(head: &str) -> Option<&'static str> {
    match head {
        "or" => Some("or"),
        "and" => Some("and"),
        "lt" => Some("<"),
        "gt" => Some(">"),
        "le" => Some("<="),
        "ge" => Some(">="),
        "ne" => Some("~="),
        "eq" => Some("=="),
        "bit-or" => Some("|"),
        "bit-xor" => Some("~"),
        "bit-and" => Some("&"),
        "shl" => Some("<<"),
        "shr" => Some(">>"),
        "concat" => Some(".."),
        "add" => Some("+"),
        "sub" => Some("-"),
        "mul" => Some("*"),
        "div" => Some("/"),
        "floor-div" => Some("//"),
        "mod" => Some("%"),
        "pow" => Some("^"),
        _ => None,
    }
}

fn unary_operator(head: &str) -> Option<&'static str> {
    match head {
        "not" => Some("not"),
        "len" => Some("#"),
        "neg" => Some("-"),
        "bit-not" => Some("~"),
        _ => None,
    }
}

fn quote_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn error(message: impl Into<String>) -> Error {
    Error::CodecError {
        codec: crate::LUA_CODEC_ID,
        message: message.into(),
    }
}
