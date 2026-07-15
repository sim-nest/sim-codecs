use sim_kernel::{Error, Expr, Result, Symbol};

use crate::{BridgeBook, BridgePart, render_frame, render_frame_with_prose};

/// Renders a `bridge/Frame` part as a cited fluent sentence.
pub fn render_frame_part(book: &BridgeBook, part: &BridgePart) -> Result<String> {
    render_frame_part_with_prose(book, part, |name, _| {
        Err(Error::Eval(format!(
            "BRIDGE prose hole {} requires fenced rendering",
            name.as_qualified_str()
        )))
    })
}

/// Renders a `bridge/Frame` part as a cited fluent sentence, delegating prose
/// holes to `render_prose`.
pub fn render_frame_part_with_prose<F>(
    book: &BridgeBook,
    part: &BridgePart,
    render_prose: F,
) -> Result<String>
where
    F: FnMut(&Symbol, &Expr) -> Result<String>,
{
    if part.kind != Symbol::qualified("bridge", "Frame") {
        return Err(Error::Eval(format!(
            "BRIDGE fluent rendering requires bridge/Frame, found {}",
            part.kind
        )));
    }
    let payload = book.frames.validate_payload(&part.payload)?;
    let spec = book.frames.require_spec(&payload.frame)?;
    let sentence = if spec
        .holes
        .iter()
        .any(|hole| matches!(hole.kind, crate::FrameHoleKind::Prose))
    {
        render_frame_with_prose(spec, &part.payload, render_prose)?
    } else {
        render_frame(spec, &part.payload)?
    };
    Ok(format!("[{}] {sentence}", part.id.as_qualified_str()))
}
