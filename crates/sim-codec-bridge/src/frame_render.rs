use sim_kernel::{Error, Expr, Result, Symbol};

use crate::{
    BridgeFramePayload, FrameHoleKind, FrameHoleSpec, FrameSpec, frame_book::validate_frame_payload,
};

/// Renders a frame payload through its deterministic sentence template.
///
/// Prose holes are rejected here because they require a caller-provided fence.
pub fn render_frame(spec: &FrameSpec, payload: &Expr) -> Result<String> {
    render_frame_with_prose(spec, payload, |name, _| {
        Err(Error::Eval(format!(
            "BRIDGE prose hole {} requires fenced rendering",
            name.as_qualified_str()
        )))
    })
}

/// Renders a frame payload, delegating prose holes to `render_prose`.
pub fn render_frame_with_prose<F>(
    spec: &FrameSpec,
    payload: &Expr,
    mut render_prose: F,
) -> Result<String>
where
    F: FnMut(&Symbol, &Expr) -> Result<String>,
{
    let payload = BridgeFramePayload::from_expr(payload)?;
    if payload.frame != spec.id {
        return Err(Error::Eval(format!(
            "BRIDGE frame payload {} does not match spec {}",
            payload.frame, spec.id
        )));
    }
    validate_frame_payload(spec, &payload)?;
    let mut body = spec.template.to_owned();
    for hole in &spec.holes {
        let value = payload.slots.get(&hole.name).ok_or_else(|| {
            Error::Eval(format!(
                "BRIDGE frame {} missing hole {}",
                spec.id, hole.name
            ))
        })?;
        let rendered = render_hole(hole, value, &mut render_prose)?;
        let marker = format!("{{{}}}", hole.name.name);
        if !body.contains(&marker) {
            return Err(Error::Eval(format!(
                "BRIDGE frame {} template is missing marker {marker}",
                spec.id
            )));
        }
        body = body.replace(&marker, &rendered);
    }
    Ok(format!("{}{}", spec.prefix, body))
}

fn render_hole<F>(hole: &FrameHoleSpec, value: &Expr, render_prose: &mut F) -> Result<String>
where
    F: FnMut(&Symbol, &Expr) -> Result<String>,
{
    match hole.kind {
        FrameHoleKind::Ref | FrameHoleKind::Term | FrameHoleKind::Choice => token_value(value),
        FrameHoleKind::Path => path_value(value),
        FrameHoleKind::Number => match value {
            Expr::Number(number) => Ok(number.canonical.clone()),
            _ => unreachable!("validated number hole"),
        },
        FrameHoleKind::Prose => render_prose(&hole.name, value),
    }
}

fn token_value(value: &Expr) -> Result<String> {
    match value {
        Expr::Symbol(symbol) => Ok(symbol.as_qualified_str().to_owned()),
        Expr::String(text) => Ok(text.clone()),
        _ => unreachable!("validated token hole"),
    }
}

fn path_value(value: &Expr) -> Result<String> {
    let items = match value {
        Expr::Vector(items) | Expr::List(items) => items,
        _ => unreachable!("validated path hole"),
    };
    items
        .iter()
        .map(token_value)
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("."))
}
