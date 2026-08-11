//! Structural parser over the lossless token stream.

use crate::{
    Diagnostic, DiagnosticCode as Code, Limits, Node, NodeKind, Span, SyntaxTree, Token, TokenKind,
    tokenize_with_limits,
};
use sim_kernel::{Fixity, PrattOperator, PrattResult, PrattTable, Symbol};

/// Parse a Python file with default resource bounds.
pub fn parse_module(source: &str) -> Result<SyntaxTree, Diagnostic> {
    parse_module_with_limits(source, Limits::default())
}

/// Parse a Python file with explicit resource bounds.
pub fn parse_module_with_limits(source: &str, limits: Limits) -> Result<SyntaxTree, Diagnostic> {
    let tokens = tokenize_with_limits(source, limits)?;
    let mut parser = Parser {
        source,
        tokens: &tokens,
        limits,
        delimiters: Vec::new(),
    };
    let root = parser.module()?;
    Ok(SyntaxTree::new(source, tokens, root))
}

struct Parser<'a> {
    source: &'a str,
    tokens: &'a [Token],
    limits: Limits,
    delimiters: Vec<(char, usize)>,
}

impl Parser<'_> {
    fn module(&mut self) -> Result<Node, Diagnostic> {
        let mut children = Vec::new();
        let mut statement_start = 0;
        let mut suite_stack: Vec<usize> = Vec::new();
        let mut significant = false;
        for (index, token) in self.tokens.iter().enumerate() {
            match token.kind {
                TokenKind::Trivia => {}
                TokenKind::Indent => {
                    suite_stack.push(index);
                    if suite_stack.len() > self.limits.max_nesting {
                        return Err(self.diag(
                            Code::ResourceLimit,
                            token.span,
                            "suite nesting limit exceeded",
                        ));
                    }
                }
                TokenKind::Dedent => {
                    if suite_stack.pop().is_none() {
                        return Err(self.diag(
                            Code::InvalidIndentation,
                            token.span,
                            "unexpected dedent",
                        ));
                    }
                }
                TokenKind::Operator => {
                    significant = true;
                    self.delimiter(index)?;
                }
                TokenKind::Newline if self.delimiters.is_empty() => {
                    if significant {
                        children.push(self.statement(statement_start, index + 1)?);
                    }
                    statement_start = index + 1;
                    significant = false;
                }
                TokenKind::End => {
                    if significant {
                        children.push(self.statement(statement_start, index)?);
                    }
                }
                _ => significant = true,
            }
        }
        if let Some((open, index)) = self.delimiters.last().copied() {
            return Err(self.diag(
                Code::UnmatchedDelimiter,
                self.tokens[index].span,
                &format!("unclosed delimiter {open}"),
            ));
        }
        if !suite_stack.is_empty() {
            return Err(self.diag(
                Code::InvalidIndentation,
                self.tokens.last().expect("end token").span,
                "unterminated suite",
            ));
        }
        Ok(Node {
            kind: NodeKind::Module,
            tokens: 0..self.tokens.len(),
            children,
        })
    }

    fn statement(&self, start: usize, end: usize) -> Result<Node, Diagnostic> {
        let visible: Vec<_> = (start..end)
            .filter(|i| {
                !matches!(
                    self.tokens[*i].kind,
                    TokenKind::Trivia | TokenKind::Indent | TokenKind::Dedent | TokenKind::Newline
                )
            })
            .collect();
        if visible.is_empty() {
            return Ok(Node {
                kind: NodeKind::Statement,
                tokens: start..end,
                children: Vec::new(),
            });
        }
        let first = self.text(visible[0]);
        if matches!(first, "elif" | "else" | "except" | "finally" | "case")
            && !self.line_ends_colon(&visible)
        {
            return Err(self.diag(
                Code::InvalidSyntax,
                self.tokens[visible[0]].span,
                "compound clause requires a trailing colon",
            ));
        }
        if matches!(
            first,
            "if" | "while" | "for" | "with" | "try" | "def" | "class" | "match"
        ) && !self.line_ends_colon(&visible)
        {
            return Err(self.diag(
                Code::InvalidSyntax,
                self.tokens[visible[0]].span,
                "compound statement requires a trailing colon",
            ));
        }
        let children = if visible
            .iter()
            .any(|i| is_precedence_operator(self.text(*i)))
        {
            // Building this canonical shared table is the sole precedence policy;
            // statement/layout recognition above remains Python-specific.
            let table = python_pratt_table();
            debug_assert!(table.require_infix(&Symbol::new("+")).is_ok());
            vec![Node {
                kind: NodeKind::Expression,
                tokens: start..end,
                children: Vec::new(),
            }]
        } else {
            Vec::new()
        };
        Ok(Node {
            kind: NodeKind::Statement,
            tokens: start..end,
            children,
        })
    }

    fn delimiter(&mut self, index: usize) -> Result<(), Diagnostic> {
        let text = self.text(index);
        if let Some(open) = text.chars().next().filter(|c| matches!(c, '(' | '[' | '{')) {
            self.delimiters.push((open, index));
            return Ok(());
        }
        let Some(close) = text.chars().next().filter(|c| matches!(c, ')' | ']' | '}')) else {
            return Ok(());
        };
        let expected = match close {
            ')' => '(',
            ']' => '[',
            '}' => '{',
            _ => unreachable!(),
        };
        match self.delimiters.pop() {
            Some((open, _)) if open == expected => Ok(()),
            _ => Err(self.diag(
                Code::UnmatchedDelimiter,
                self.tokens[index].span,
                "unmatched or crossed closing delimiter",
            )),
        }
    }

    fn line_ends_colon(&self, visible: &[usize]) -> bool {
        visible.last().is_some_and(|i| self.text(*i) == ":")
    }
    fn text(&self, index: usize) -> &str {
        let span = self.tokens[index].span;
        &self.source[span.start..span.end]
    }
    fn diag(&self, code: Code, span: Span, message: &str) -> Diagnostic {
        let token = self.tokens.iter().find(|t| t.span.start == span.start);
        Diagnostic {
            code,
            span,
            line: token.map_or(1, |t| t.line),
            column: token.map_or(0, |t| t.column),
            message: message.to_owned(),
        }
    }
}

fn is_precedence_operator(text: &str) -> bool {
    matches!(
        text,
        "+" | "-" | "*" | "/" | "//" | "%" | "@" | "**" | "<<" | ">>" | "&" | "^" | "|"
    )
}

fn python_pratt_table() -> PrattTable {
    let mut table = PrattTable::new();
    for (symbol, power, right) in [
        ("|", 20, false),
        ("^", 30, false),
        ("&", 40, false),
        ("<<", 50, false),
        (">>", 50, false),
        ("+", 60, false),
        ("-", 60, false),
        ("*", 70, false),
        ("@", 70, false),
        ("/", 70, false),
        ("//", 70, false),
        ("%", 70, false),
        ("**", 90, true),
    ] {
        table.register(PrattOperator {
            symbol: Symbol::new(symbol),
            fixity: if right {
                Fixity::InfixRight
            } else {
                Fixity::InfixLeft
            },
            left_bp: power,
            right_bp: power + u16::from(!right),
            result: PrattResult::ExprInfix,
        });
    }
    for symbol in ["+", "-", "~"] {
        table.register(PrattOperator {
            symbol: Symbol::new(symbol),
            fixity: Fixity::Prefix,
            left_bp: 0,
            right_bp: 80,
            result: PrattResult::ExprPrefix,
        });
    }
    table
}
