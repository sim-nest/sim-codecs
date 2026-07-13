//! Typst backend implementation over `typst-syntax`.

use std::collections::BTreeMap;
use std::iter::Peekable;

use typst_syntax::ast::{self, AstNode};
use typst_syntax::{SyntaxNode, parse};

use crate::backend::{
    MarkupBackend, MarkupDecodeOptions, MarkupEncodeOptions, MarkupError, MarkupFidelity,
    MarkupLoss,
};
use crate::markup::{
    BackendId, Inline, MarkupBlock, MarkupDoc, MathSource, SourceDoc, Span, SpanState,
};

#[path = "typst_writer.rs"]
mod typst_writer;

use typst_writer::TypstEncoder;

/// Safe Typst markup backend.
#[derive(Clone, Debug, Default)]
pub struct TypstBackend;

impl MarkupBackend for TypstBackend {
    fn id(&self) -> BackendId {
        typst_id()
    }

    fn decode(
        &self,
        input: &str,
        opts: &MarkupDecodeOptions,
    ) -> Result<(MarkupDoc, MarkupFidelity), MarkupError> {
        let root = parse(input);
        let (errors, warnings) = root.errors_and_warnings();
        if !errors.is_empty() {
            return Err(MarkupError::Decode(format!(
                "typst syntax contains {} error(s)",
                errors.len()
            )));
        }

        let markup = root
            .cast::<ast::Markup>()
            .ok_or_else(|| MarkupError::Decode("typst root is not markup".to_owned()))?;
        let mut parser = TypstParser::new(input, opts);
        parser.fidelity.warnings.extend(
            warnings
                .into_iter()
                .map(|_| "typst syntax warning".to_owned()),
        );
        let blocks = parser.parse_markup(markup);
        let title = blocks.iter().find_map(|block| match block {
            MarkupBlock::Heading { level: 1, text, .. } => Some(inline_plain_text(text)),
            _ => None,
        });
        let source = opts.preserve_source.then(|| SourceDoc {
            backend: typst_id(),
            text: input.to_owned(),
        });
        Ok((
            MarkupDoc {
                title,
                blocks,
                attrs: BTreeMap::new(),
                source,
            },
            parser.fidelity,
        ))
    }

    fn encode(
        &self,
        doc: &MarkupDoc,
        opts: &MarkupEncodeOptions,
    ) -> Result<(String, MarkupFidelity), MarkupError> {
        let mut encoder = TypstEncoder::new(opts);
        let source = encoder.write_doc(doc);
        if opts.fail_on_loss && !encoder.fidelity().dropped.is_empty() {
            return Err(MarkupError::Encode(format!(
                "typst encode dropped {} unsupported fragment(s)",
                encoder.fidelity().dropped.len()
            )));
        }
        Ok((source, encoder.into_fidelity()))
    }
}

struct TypstParser<'a> {
    input: &'a str,
    cursor: usize,
    preserve_raw: bool,
    fidelity: MarkupFidelity,
}

impl<'a> TypstParser<'a> {
    fn new(input: &'a str, opts: &MarkupDecodeOptions) -> Self {
        Self {
            input,
            cursor: 0,
            preserve_raw: opts.preserve_raw,
            fidelity: MarkupFidelity::exact(typst_id()),
        }
    }

    fn parse_markup(&mut self, markup: ast::Markup<'_>) -> Vec<MarkupBlock> {
        let mut blocks = Vec::new();
        let mut paragraph = PendingParagraph::default();
        let mut list: Option<PendingList> = None;
        for expr in markup.exprs() {
            let span = self.span_for(expr.to_untyped());
            match expr {
                ast::Expr::Parbreak(_) => {
                    self.flush_paragraph(&mut blocks, &mut paragraph);
                    self.flush_list(&mut blocks, &mut list);
                }
                ast::Expr::Heading(heading) => {
                    self.flush_paragraph(&mut blocks, &mut paragraph);
                    self.flush_list(&mut blocks, &mut list);
                    blocks.push(MarkupBlock::Heading {
                        level: heading.depth().get().min(u8::MAX as usize) as u8,
                        text: self.inlines_from_markup(heading.body()),
                        id: None,
                        span,
                    });
                }
                ast::Expr::Raw(raw) if raw.block() => {
                    self.flush_paragraph(&mut blocks, &mut paragraph);
                    self.flush_list(&mut blocks, &mut list);
                    blocks.push(MarkupBlock::CodeBlock {
                        lang: raw.lang().map(|lang| lang.get().to_string()),
                        code: raw_text(raw),
                        span,
                    });
                }
                ast::Expr::Equation(equation) if equation.block() => {
                    self.flush_paragraph(&mut blocks, &mut paragraph);
                    self.flush_list(&mut blocks, &mut list);
                    blocks.push(MarkupBlock::MathBlock {
                        source: typst_math(equation.body()),
                        span,
                    });
                }
                ast::Expr::ListItem(item) => {
                    self.flush_paragraph(&mut blocks, &mut paragraph);
                    self.push_list_item(&mut blocks, &mut list, false, item.body(), span);
                }
                ast::Expr::EnumItem(item) => {
                    self.flush_paragraph(&mut blocks, &mut paragraph);
                    self.push_list_item(&mut blocks, &mut list, true, item.body(), span);
                }
                ast::Expr::FuncCall(call) if self.call_name(call.callee()) == Some("table") => {
                    self.flush_paragraph(&mut blocks, &mut paragraph);
                    self.flush_list(&mut blocks, &mut list);
                    if let Some(table) = self.table_from_call(call, span.clone()) {
                        blocks.push(table);
                    } else if let Some(raw) =
                        self.raw_block(expr_source(expr), "table", span, "unsupported typst table")
                    {
                        blocks.push(raw);
                    }
                }
                ast::Expr::FuncCall(call) if self.call_name(call.callee()) == Some("figure") => {
                    self.flush_paragraph(&mut blocks, &mut paragraph);
                    self.flush_list(&mut blocks, &mut list);
                    if let Some(figure) = self.figure_from_call(call, span.clone()) {
                        blocks.push(figure);
                    } else if let Some(raw) = self.raw_block(
                        expr_source(expr),
                        "figure",
                        span,
                        "unsupported typst figure",
                    ) {
                        blocks.push(raw);
                    }
                }
                ast::Expr::FuncCall(call) if self.call_name(call.callee()) != Some("link") => {
                    self.flush_paragraph(&mut blocks, &mut paragraph);
                    self.flush_list(&mut blocks, &mut list);
                    let path = self
                        .call_name(call.callee())
                        .unwrap_or("function")
                        .to_owned();
                    if let Some(raw) = self.raw_block(
                        call.to_untyped().full_text().to_string(),
                        &path,
                        span,
                        "unsupported typst function is preserved but not executed",
                    ) {
                        blocks.push(raw);
                    }
                }
                ast::Expr::ModuleInclude(_) => {
                    self.flush_paragraph(&mut blocks, &mut paragraph);
                    self.flush_list(&mut blocks, &mut list);
                    if let Some(raw) = self.raw_block(
                        expr_source(expr),
                        "include",
                        span,
                        "typst include is preserved but not executed",
                    ) {
                        blocks.push(raw);
                    }
                }
                ast::Expr::ModuleImport(_) => {
                    self.flush_paragraph(&mut blocks, &mut paragraph);
                    self.flush_list(&mut blocks, &mut list);
                    if let Some(raw) = self.raw_block(
                        expr_source(expr),
                        "import",
                        span,
                        "typst import is preserved but not executed",
                    ) {
                        blocks.push(raw);
                    }
                }
                other => {
                    self.flush_list(&mut blocks, &mut list);
                    if let Some(inline) = self.inline_from_expr(other, "inline") {
                        paragraph.push(inline, span);
                    }
                }
            }
        }
        self.flush_paragraph(&mut blocks, &mut paragraph);
        self.flush_list(&mut blocks, &mut list);
        blocks
    }

    fn push_list_item(
        &mut self,
        blocks: &mut Vec<MarkupBlock>,
        list: &mut Option<PendingList>,
        ordered: bool,
        body: ast::Markup<'_>,
        span: Option<Span>,
    ) {
        if list.as_ref().is_some_and(|list| list.ordered != ordered) {
            self.flush_list(blocks, list);
        }
        let item = self.blocks_from_item_body(body);
        list.get_or_insert_with(|| PendingList::new(ordered))
            .push(item, span);
    }

    fn blocks_from_item_body<'b>(&mut self, body: ast::Markup<'b>) -> Vec<MarkupBlock> {
        let content = self.inlines_from_markup(body);
        if content.is_empty() {
            Vec::new()
        } else {
            vec![MarkupBlock::Paragraph {
                content,
                span: None,
            }]
        }
    }

    fn inlines_from_markup<'b>(&mut self, markup: ast::Markup<'b>) -> Vec<Inline> {
        let mut items = Vec::new();
        let mut exprs = markup.exprs().peekable();
        while let Some(expr) = exprs.next() {
            if let Some(link) = self.link_from_call_with_body(expr, &mut exprs) {
                items.push(link);
                continue;
            }
            if let Some(inline) = self.inline_from_expr(expr, "inline") {
                items.push(inline);
            }
        }
        items
    }

    fn inline_from_expr<'b>(&mut self, expr: ast::Expr<'b>, path: &str) -> Option<Inline> {
        match expr {
            ast::Expr::Text(text) => Some(Inline::Text(text.get().to_string())),
            ast::Expr::Space(_) => Some(Inline::Text(" ".to_owned())),
            ast::Expr::Linebreak(_) => Some(Inline::Text("\n".to_owned())),
            ast::Expr::Escape(escape) => Some(Inline::Text(escape.get().to_string())),
            ast::Expr::Shorthand(shorthand) => Some(Inline::Text(shorthand.get().to_string())),
            ast::Expr::SmartQuote(quote) => Some(Inline::Text(
                if quote.double() { "\"" } else { "'" }.to_owned(),
            )),
            ast::Expr::Strong(strong) => {
                Some(Inline::Strong(self.inlines_from_markup(strong.body())))
            }
            ast::Expr::Emph(emph) => Some(Inline::Emph(self.inlines_from_markup(emph.body()))),
            ast::Expr::Raw(raw) if !raw.block() => Some(Inline::Code(raw_text(raw))),
            ast::Expr::Link(link) => {
                let target = link.get().to_string();
                Some(Inline::Link {
                    label: vec![Inline::Text(target.clone())],
                    target,
                })
            }
            ast::Expr::Equation(equation) if !equation.block() => {
                Some(Inline::Math(typst_math(equation.body())))
            }
            ast::Expr::FuncCall(call) if self.call_name(call.callee()) == Some("link") => {
                let target = self.first_string_arg(call)?;
                Some(Inline::Link {
                    label: vec![Inline::Text(target.clone())],
                    target,
                })
            }
            ast::Expr::Parbreak(_) => None,
            other => self.raw_inline(expr_source(other), path, "unsupported typst inline"),
        }
    }

    fn link_from_call_with_body<'b, I>(
        &mut self,
        expr: ast::Expr<'b>,
        exprs: &mut Peekable<I>,
    ) -> Option<Inline>
    where
        I: Iterator<Item = ast::Expr<'b>>,
    {
        let ast::Expr::FuncCall(call) = expr else {
            return None;
        };
        if self.call_name(call.callee()) != Some("link") {
            return None;
        }
        let target = self.first_string_arg(call)?;
        let label = match exprs.peek().copied() {
            Some(ast::Expr::ContentBlock(block)) => {
                exprs.next();
                self.inlines_from_markup(block.body())
            }
            _ => vec![Inline::Text(target.clone())],
        };
        Some(Inline::Link { label, target })
    }

    fn table_from_call(
        &mut self,
        call: ast::FuncCall<'_>,
        span: Option<Span>,
    ) -> Option<MarkupBlock> {
        let mut columns = None;
        let mut cells = Vec::new();
        for arg in call.args().items() {
            match arg {
                ast::Arg::Named(named) if named.name().as_str() == "columns" => {
                    if let ast::Expr::Int(value) = named.expr() {
                        columns = usize::try_from(value.get()).ok().filter(|value| *value > 0);
                    }
                }
                ast::Arg::Pos(ast::Expr::ContentBlock(block)) => {
                    cells.push(self.inlines_from_markup(block.body()));
                }
                _ => self
                    .fidelity
                    .warnings
                    .push("unsupported typst table argument preserved by source span".to_owned()),
            }
        }
        if cells.is_empty() {
            return None;
        }
        let width = columns.unwrap_or(cells.len()).max(1);
        let mut rows = cells
            .chunks(width)
            .map(|chunk| {
                let mut row = chunk.to_vec();
                row.resize_with(width, Vec::new);
                row
            })
            .collect::<Vec<_>>();
        let header = rows.drain(..1).next().unwrap_or_default();
        Some(MarkupBlock::Table { header, rows, span })
    }

    fn figure_from_call(
        &mut self,
        call: ast::FuncCall<'_>,
        span: Option<Span>,
    ) -> Option<MarkupBlock> {
        let mut src = None;
        let mut caption = Vec::new();
        for arg in call.args().items() {
            match arg {
                ast::Arg::Pos(ast::Expr::FuncCall(image))
                    if self.call_name(image.callee()) == Some("image") =>
                {
                    src = self.first_string_arg(image);
                }
                ast::Arg::Named(named) if named.name().as_str() == "caption" => {
                    if let ast::Expr::ContentBlock(block) = named.expr() {
                        caption = self.inlines_from_markup(block.body());
                    }
                }
                ast::Arg::Pos(ast::Expr::ContentBlock(block)) if caption.is_empty() => {
                    caption = self.inlines_from_markup(block.body());
                }
                _ => self
                    .fidelity
                    .warnings
                    .push("unsupported typst figure argument preserved by source span".to_owned()),
            }
        }
        Some(MarkupBlock::Figure {
            src: src?,
            caption,
            span,
        })
    }

    fn first_string_arg(&self, call: ast::FuncCall<'_>) -> Option<String> {
        call.args().items().find_map(|arg| match arg {
            ast::Arg::Pos(ast::Expr::Str(value)) => Some(value.get().to_string()),
            _ => None,
        })
    }

    fn call_name<'b>(&self, expr: ast::Expr<'b>) -> Option<&'b str> {
        match expr {
            ast::Expr::Ident(ident) => Some(ident.as_str()),
            _ => None,
        }
    }

    fn flush_paragraph(&mut self, blocks: &mut Vec<MarkupBlock>, paragraph: &mut PendingParagraph) {
        if paragraph.content.is_empty() {
            return;
        }

        let content = std::mem::take(&mut paragraph.content);
        let span = paragraph.span.take();
        if inline_plain_text(&content).trim().is_empty() {
            return;
        }

        blocks.push(MarkupBlock::Paragraph { content, span });
    }

    fn flush_list(&mut self, blocks: &mut Vec<MarkupBlock>, list: &mut Option<PendingList>) {
        if let Some(list) = list.take() {
            blocks.push(MarkupBlock::List {
                ordered: list.ordered,
                items: list.items,
                span: list.span,
            });
        }
    }

    fn raw_block(
        &mut self,
        text: String,
        path: &str,
        span: Option<Span>,
        reason: &str,
    ) -> Option<MarkupBlock> {
        if self.preserve_raw {
            self.fidelity.preserved_raw.push(text.clone());
            Some(MarkupBlock::Raw {
                backend: typst_id(),
                text,
                span,
            })
        } else {
            self.drop_raw(path, reason);
            None
        }
    }

    fn raw_inline(&mut self, text: String, path: &str, reason: &str) -> Option<Inline> {
        if self.preserve_raw {
            self.fidelity.preserved_raw.push(text.clone());
            Some(Inline::Raw {
                backend: typst_id(),
                text,
            })
        } else {
            self.drop_raw(path, reason);
            None
        }
    }

    fn drop_raw(&mut self, path: &str, reason: &str) {
        self.fidelity.dropped.push(MarkupLoss {
            path: path.to_owned(),
            reason: reason.to_owned(),
        });
    }

    fn span_for(&mut self, node: &SyntaxNode) -> Option<Span> {
        let text = node.full_text();
        if text.is_empty() {
            return None;
        }
        let haystack = self.input.get(self.cursor..)?;
        let start = haystack.find(text.as_str())? + self.cursor;
        let end = start + text.len();
        self.cursor = end;
        Some(span(start, end))
    }
}

#[derive(Default)]
struct PendingParagraph {
    content: Vec<Inline>,
    span: Option<Span>,
}

impl PendingParagraph {
    fn push(&mut self, inline: Inline, span: Option<Span>) {
        merge_span(&mut self.span, &span);
        self.content.push(inline);
    }
}

struct PendingList {
    ordered: bool,
    items: Vec<Vec<MarkupBlock>>,
    span: Option<Span>,
}

impl PendingList {
    fn new(ordered: bool) -> Self {
        Self {
            ordered,
            items: Vec::new(),
            span: None,
        }
    }

    fn push(&mut self, item: Vec<MarkupBlock>, span: Option<Span>) {
        merge_span(&mut self.span, &span);
        self.items.push(item);
    }
}

fn typst_id() -> BackendId {
    BackendId::new("typst")
}

fn expr_source(expr: ast::Expr<'_>) -> String {
    expr.to_untyped().full_text().to_string()
}

fn raw_text(raw: ast::Raw<'_>) -> String {
    let lines = raw
        .lines()
        .map(|line| line.get().to_string())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        raw.to_untyped().full_text().to_string()
    } else {
        lines.join("\n")
    }
}

fn typst_math(math: ast::Math<'_>) -> MathSource {
    MathSource {
        notation: "typst".to_owned(),
        text: math.to_untyped().full_text().trim().to_owned(),
    }
}

fn span(start: usize, end: usize) -> Span {
    Span {
        start,
        end,
        state: SpanState::Preserved,
    }
}

fn merge_span(target: &mut Option<Span>, next: &Option<Span>) {
    let Some(next) = next else {
        return;
    };
    match target {
        Some(current) => {
            current.start = current.start.min(next.start);
            current.end = current.end.max(next.end);
        }
        None => *target = Some(next.clone()),
    }
}

fn inline_plain_text(items: &[Inline]) -> String {
    let mut text = String::new();
    for item in items {
        match item {
            Inline::Text(value) | Inline::Code(value) => text.push_str(value),
            Inline::Emph(children) | Inline::Strong(children) => {
                text.push_str(&inline_plain_text(children));
            }
            Inline::Link { label, .. } => text.push_str(&inline_plain_text(label)),
            Inline::Math(source) => text.push_str(&source.text),
            Inline::Raw { text: raw, .. } => text.push_str(raw),
        }
    }
    text
}
