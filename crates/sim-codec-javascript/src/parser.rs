//! Structural Script/Module parser over the lossless token stream.

use crate::{
    Asi, Diagnostic, DiagnosticCode as Code, Goal, Limits, Node, NodeKind, Span, SyntaxTree, Token,
    TokenKind, tokenize_with_limits,
};

/// Parses a Script with default bounds.
pub fn parse_script(source: &str) -> Result<SyntaxTree, Diagnostic> {
    parse_script_with_limits(source, Limits::default())
}
/// Parses a Script with explicit bounds.
pub fn parse_script_with_limits(source: &str, limits: Limits) -> Result<SyntaxTree, Diagnostic> {
    parse(source, Goal::Script, limits)
}
/// Parses a Module with default bounds.
pub fn parse_module(source: &str) -> Result<SyntaxTree, Diagnostic> {
    parse_module_with_limits(source, Limits::default())
}
/// Parses a Module with explicit bounds.
pub fn parse_module_with_limits(source: &str, limits: Limits) -> Result<SyntaxTree, Diagnostic> {
    parse(source, Goal::Module, limits)
}

fn parse(source: &str, goal: Goal, limits: Limits) -> Result<SyntaxTree, Diagnostic> {
    let tokens = tokenize_with_limits(source, limits)?;
    let mut p = Parser {
        source,
        tokens: &tokens,
        goal,
        limits,
        nodes: 0,
    };
    let root = p.program()?;
    Ok(SyntaxTree::new(source, goal, tokens, root))
}
struct Parser<'a> {
    source: &'a str,
    tokens: &'a [Token],
    goal: Goal,
    limits: Limits,
    nodes: usize,
}
impl Parser<'_> {
    fn program(&mut self) -> Result<Node, Diagnostic> {
        let visible: Vec<usize> = (0..self.tokens.len())
            .filter(|i| !matches!(self.tokens[*i].kind, TokenKind::Trivia | TokenKind::End))
            .collect();
        self.check_delimiters(&visible)?;
        self.early_errors(&visible)?;
        let mut children = Vec::new();
        let mut start = 0;
        let mut depth = 0usize;
        for (i, t) in self.tokens.iter().enumerate() {
            if t.kind == TokenKind::Trivia || t.kind == TokenKind::End {
                continue;
            }
            match self.text(i) {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                ";" if depth == 0 => {
                    children.push(self.item(start, i + 1, Some(Asi::Explicit(t.span)))?);
                    start = i + 1;
                }
                _ => {}
            }
            if depth == 0
                && self.has_line_terminator_after(i)
                && self.can_end_statement(i)
                && self
                    .next_visible(i + 1)
                    .is_some_and(|n| self.must_separate(i, n))
            {
                let span = self.tokens[i].span;
                children.push(self.item(
                    start,
                    i + 1,
                    Some(Asi::LineTerminator(Span {
                        start: span.end,
                        end: span.end,
                    })),
                )?);
                start = i + 1;
            }
        }
        let end = self.tokens.len().saturating_sub(1);
        if self.significant(start, end) {
            children.push(self.item(start, end, Some(Asi::EndOfInput(self.tokens[end].span)))?);
        }
        let statements = self.raw_node(NodeKind::StatementList, 0..end, children, None)?;
        self.node(
            if self.goal == Goal::Script {
                NodeKind::Script
            } else {
                NodeKind::Module
            },
            0..self.tokens.len(),
            vec![statements],
            None,
        )
    }
    fn item(&mut self, start: usize, end: usize, asi: Option<Asi>) -> Result<Node, Diagnostic> {
        let Some(first) = self.first_visible(start, end) else {
            return self.raw_node(NodeKind::Statement, start..end, Vec::new(), asi);
        };
        let word = self.text(first);
        let kind = match word {
            "import" => NodeKind::Import,
            "export" => NodeKind::Export,
            "function" | "async" => NodeKind::Function,
            "class" => NodeKind::Class,
            "const" | "let" | "var" => NodeKind::Declaration,
            _ => NodeKind::Statement,
        };
        let mut children = Vec::new();
        if (first..end).any(|i| {
            self.tokens[i].kind != TokenKind::Trivia && is_expression_operator(self.text(i))
        }) {
            children.push(self.raw_node(NodeKind::Expression, first..end, Vec::new(), None)?);
        }
        self.node(kind, start..end, children, asi)
    }
    fn early_errors(&self, v: &[usize]) -> Result<(), Diagnostic> {
        for (i, pos) in v.iter().copied().enumerate() {
            let word = self.text(pos);
            if self.goal == Goal::Script && matches!(word, "import" | "export") {
                return Err(self.diag(
                    Code::EarlyError,
                    pos,
                    "import/export declaration is only valid in Module goal",
                ));
            }
            if self.goal == Goal::Module && word == "with" {
                return Err(self.diag(
                    Code::EarlyError,
                    pos,
                    "with statement is forbidden in strict Module code",
                ));
            }
            if word == "throw" && v.get(i + 1).is_some_and(|n| self.line_between(pos, *n)) {
                return Err(self.diag(
                    Code::EarlyError,
                    pos,
                    "line terminator is forbidden after throw",
                ));
            }
            if matches!(word, "break" | "continue" | "return" | "yield")
                && v.get(i + 1).is_some_and(|n| self.line_between(pos, *n))
            {
                continue;
            }
            if matches!(word, "const" | "let" | "var")
                && let (Some(name), Some(next)) = (v.get(i + 1), v.get(i + 2))
                && self.text(*name) == self.text(*next)
                && self.text(*name) != ","
            {
                return Err(self.diag(Code::EarlyError, *next, "duplicate binding in declaration"));
            }
            if word == "function"
                && let Some(open) = v.iter().skip(i).find(|p| self.text(**p) == "(")
                && let Some(close) = v
                    .iter()
                    .skip_while(|p| **p != *open)
                    .find(|p| self.text(**p) == ")")
            {
                let mut names = std::collections::BTreeSet::new();
                for p in v.iter().copied().filter(|p| {
                    *p > *open && *p < *close && self.tokens[*p].kind == TokenKind::Identifier
                }) {
                    if !names.insert(self.text(p)) {
                        return Err(self.diag(Code::EarlyError, p, "duplicate parameter name"));
                    }
                }
            }
        }
        Ok(())
    }
    fn check_delimiters(&self, v: &[usize]) -> Result<(), Diagnostic> {
        let mut stack = Vec::new();
        for p in v.iter().copied() {
            match self.text(p) {
                "(" | "[" | "{" => {
                    stack.push((self.text(p), p));
                    if stack.len() > self.limits.max_nesting {
                        return Err(self.diag(
                            Code::ResourceLimit,
                            p,
                            "parser nesting limit exceeded",
                        ));
                    }
                }
                ")" | "]" | "}" => {
                    let expected = match self.text(p) {
                        ")" => "(",
                        "]" => "[",
                        _ => "{",
                    };
                    if stack.pop().is_none_or(|x| x.0 != expected) {
                        return Err(self.diag(
                            Code::UnmatchedDelimiter,
                            p,
                            "unmatched or crossed closing delimiter",
                        ));
                    }
                }
                _ => {}
            }
        }
        if let Some((_, p)) = stack.pop() {
            return Err(self.diag(Code::UnmatchedDelimiter, p, "unclosed delimiter"));
        }
        Ok(())
    }
    fn must_separate(&self, left: usize, right: usize) -> bool {
        matches!(
            self.text(left),
            "return" | "break" | "continue" | "yield" | "++" | "--"
        ) || matches!(
            self.text(right),
            "const"
                | "let"
                | "var"
                | "function"
                | "class"
                | "if"
                | "for"
                | "while"
                | "switch"
                | "try"
                | "throw"
                | "return"
                | "import"
                | "export"
        )
    }
    fn can_end_statement(&self, p: usize) -> bool {
        matches!(
            self.tokens[p].kind,
            TokenKind::Identifier
                | TokenKind::Number
                | TokenKind::String
                | TokenKind::RegExp
                | TokenKind::Template
        ) || matches!(
            self.text(p),
            ")" | "]" | "}" | "++" | "--" | "break" | "continue" | "return"
        )
    }
    fn has_line_terminator_after(&self, p: usize) -> bool {
        self.tokens
            .get(p + 1..)
            .unwrap_or_default()
            .iter()
            .take_while(|t| t.kind == TokenKind::Trivia)
            .any(|t| self.slice(t.span).contains(['\n', '\r']))
    }
    fn line_between(&self, a: usize, b: usize) -> bool {
        self.source[self.tokens[a].span.end..self.tokens[b].span.start].contains(['\n', '\r'])
    }
    fn first_visible(&self, s: usize, e: usize) -> Option<usize> {
        (s..e).find(|i| !matches!(self.tokens[*i].kind, TokenKind::Trivia | TokenKind::End))
    }
    fn next_visible(&self, s: usize) -> Option<usize> {
        (s..self.tokens.len())
            .find(|i| !matches!(self.tokens[*i].kind, TokenKind::Trivia | TokenKind::End))
    }
    fn significant(&self, s: usize, e: usize) -> bool {
        self.first_visible(s, e).is_some()
    }
    fn text(&self, p: usize) -> &str {
        self.slice(self.tokens[p].span)
    }
    fn slice(&self, s: Span) -> &str {
        &self.source[s.start..s.end]
    }
    fn raw_node(
        &mut self,
        k: NodeKind,
        t: std::ops::Range<usize>,
        c: Vec<Node>,
        a: Option<Asi>,
    ) -> Result<Node, Diagnostic> {
        self.nodes += 1;
        if self.nodes > self.limits.max_nodes {
            return Err(self.diag_at(
                Code::ResourceLimit,
                self.source.len(),
                "node limit exceeded",
            ));
        }
        Ok(Node {
            kind: k,
            tokens: t,
            children: c,
            asi: a,
        })
    }
    fn node(
        &mut self,
        k: NodeKind,
        t: std::ops::Range<usize>,
        c: Vec<Node>,
        a: Option<Asi>,
    ) -> Result<Node, Diagnostic> {
        self.raw_node(k, t, c, a)
    }
    fn diag(&self, c: Code, p: usize, m: &str) -> Diagnostic {
        let t = &self.tokens[p];
        Diagnostic {
            code: c,
            span: t.span,
            line: t.line,
            column: t.column,
            message: m.to_owned(),
        }
    }
    fn diag_at(&self, c: Code, p: usize, m: &str) -> Diagnostic {
        let before = &self.source[..p];
        Diagnostic {
            code: c,
            span: Span { start: p, end: p },
            line: before.bytes().filter(|b| *b == b'\n').count() + 1,
            column: before
                .rsplit_once('\n')
                .map_or(before, |x| x.1)
                .chars()
                .count(),
            message: m.to_owned(),
        }
    }
}
fn is_expression_operator(s: &str) -> bool {
    matches!(
        s,
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "**"
            | "<"
            | ">"
            | "<="
            | ">="
            | "=="
            | "!="
            | "==="
            | "!=="
            | "&&"
            | "||"
            | "??"
            | "="
            | "=>"
    )
}
