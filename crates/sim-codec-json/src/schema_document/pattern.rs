#[derive(Clone, Debug, PartialEq, Eq)]
struct EcmaPattern {
    root: PatternNode,
}

impl EcmaPattern {
    fn compile(source: &str) -> Result<Self, PatternError> {
        let mut parser = PatternParser::new(source);
        let root = parser.parse_alternation(None)?;
        if parser.peek().is_some() {
            return Err(PatternError::Invalid);
        }
        Ok(Self { root })
    }

    fn is_match(&self, text: &str, budget: usize) -> Result<(bool, usize), PatternError> {
        let chars: Vec<char> = text.chars().collect();
        let mut work = 0usize;
        for start in 0..=chars.len() {
            let ends = self.match_node(&self.root, &chars, start, &mut work, budget)?;
            if !ends.is_empty() {
                return Ok((true, work));
            }
        }
        Ok((false, work))
    }

    fn match_node(
        &self,
        node: &PatternNode,
        chars: &[char],
        pos: usize,
        work: &mut usize,
        budget: usize,
    ) -> Result<Vec<usize>, PatternError> {
        *work = work.saturating_add(1);
        if *work > budget {
            return Err(PatternError::Budget);
        }
        match node {
            PatternNode::Empty => Ok(vec![pos]),
            PatternNode::Start => {
                if pos == 0 {
                    Ok(vec![pos])
                } else {
                    Ok(Vec::new())
                }
            }
            PatternNode::End => {
                if pos == chars.len() {
                    Ok(vec![pos])
                } else {
                    Ok(Vec::new())
                }
            }
            PatternNode::Literal(ch) => {
                if chars.get(pos) == Some(ch) {
                    Ok(vec![pos + 1])
                } else {
                    Ok(Vec::new())
                }
            }
            PatternNode::Any => {
                if chars
                    .get(pos)
                    .is_some_and(|ch| !matches!(*ch, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
                {
                    Ok(vec![pos + 1])
                } else {
                    Ok(Vec::new())
                }
            }
            PatternNode::Class(class) => {
                if chars.get(pos).is_some_and(|ch| class.matches(*ch)) {
                    Ok(vec![pos + 1])
                } else {
                    Ok(Vec::new())
                }
            }
            PatternNode::Concat(nodes) => {
                let mut positions = vec![pos];
                for child in nodes {
                    let mut next = Vec::new();
                    for position in positions {
                        next.extend(self.match_node(child, chars, position, work, budget)?);
                    }
                    positions = dedup_positions(next);
                    if positions.is_empty() {
                        break;
                    }
                }
                Ok(positions)
            }
            PatternNode::Alternation(nodes) => {
                let mut positions = Vec::new();
                for child in nodes {
                    positions.extend(self.match_node(child, chars, pos, work, budget)?);
                }
                Ok(dedup_positions(positions))
            }
            PatternNode::Repeat { node, min, max } => {
                let mut out = Vec::new();
                self.match_repeat(
                    node,
                    RepeatRequest {
                        min: *min,
                        max: *max,
                        count: 0,
                        pos,
                    },
                    chars,
                    work,
                    budget,
                    &mut out,
                )?;
                Ok(dedup_positions(out))
            }
        }
    }

    fn match_repeat(
        &self,
        node: &PatternNode,
        request: RepeatRequest,
        chars: &[char],
        work: &mut usize,
        budget: usize,
        out: &mut Vec<usize>,
    ) -> Result<(), PatternError> {
        *work = work.saturating_add(1);
        if *work > budget {
            return Err(PatternError::Budget);
        }
        if request.count >= request.min {
            out.push(request.pos);
        }
        if request.max.is_some_and(|max| request.count >= max) {
            return Ok(());
        }
        for next in self.match_node(node, chars, request.pos, work, budget)? {
            if next == request.pos {
                continue;
            }
            self.match_repeat(
                node,
                RepeatRequest {
                    count: request.count + 1,
                    pos: next,
                    ..request
                },
                chars,
                work,
                budget,
                out,
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct RepeatRequest {
    min: usize,
    max: Option<usize>,
    count: usize,
    pos: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PatternNode {
    Empty,
    Start,
    End,
    Literal(char),
    Any,
    Class(CharClass),
    Concat(Vec<PatternNode>),
    Alternation(Vec<PatternNode>),
    Repeat {
        node: Box<PatternNode>,
        min: usize,
        max: Option<usize>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CharClass {
    negated: bool,
    atoms: Vec<ClassAtom>,
}

impl CharClass {
    fn matches(&self, ch: char) -> bool {
        self.atoms.iter().any(|atom| atom.matches(ch)) != self.negated
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ClassAtom {
    Char(char),
    Range(char, char),
    Digit(bool),
    Space(bool),
    Word(bool),
}

impl ClassAtom {
    fn matches(&self, ch: char) -> bool {
        match self {
            Self::Char(want) => ch == *want,
            Self::Range(start, end) => *start <= ch && ch <= *end,
            Self::Digit(negated) => ch.is_ascii_digit() != *negated,
            Self::Space(negated) => {
                matches!(
                    ch,
                    '\t' | '\n'
                        | '\u{000B}'
                        | '\u{000C}'
                        | '\r'
                        | ' '
                        | '\u{00A0}'
                        | '\u{1680}'
                        | '\u{2000}'
                        ..='\u{200A}'
                            | '\u{2028}'
                            | '\u{2029}'
                            | '\u{202F}'
                            | '\u{205F}'
                            | '\u{3000}'
                            | '\u{FEFF}'
                ) != *negated
            }
            Self::Word(negated) => (ch.is_ascii_alphanumeric() || ch == '_') != *negated,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PatternError {
    Invalid,
    Unsupported,
    Budget,
}

impl PatternError {
    fn compile_message(&self) -> &'static str {
        match self {
            Self::Invalid => "invalid regular expression",
            Self::Unsupported => "unsupported regular expression",
            Self::Budget => "regex work budget exceeded",
        }
    }

    fn runtime_message(&self) -> &'static str {
        self.compile_message()
    }
}
