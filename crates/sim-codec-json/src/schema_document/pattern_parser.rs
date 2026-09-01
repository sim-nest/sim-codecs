struct PatternParser<'a> {
    chars: Vec<char>,
    pos: usize,
    _source: &'a str,
}

impl<'a> PatternParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            _source: source,
        }
    }

    fn parse_alternation(&mut self, terminator: Option<char>) -> Result<PatternNode, PatternError> {
        let mut choices = vec![self.parse_concat(terminator)?];
        while self.peek() == Some('|') {
            self.bump();
            choices.push(self.parse_concat(terminator)?);
        }
        if choices.len() == 1 {
            Ok(choices.remove(0))
        } else {
            Ok(PatternNode::Alternation(choices))
        }
    }

    fn parse_concat(&mut self, terminator: Option<char>) -> Result<PatternNode, PatternError> {
        let mut nodes = Vec::new();
        while let Some(ch) = self.peek() {
            if Some(ch) == terminator || ch == '|' {
                break;
            }
            nodes.push(self.parse_quantified()?);
        }
        if nodes.is_empty() {
            Ok(PatternNode::Empty)
        } else if nodes.len() == 1 {
            Ok(nodes.remove(0))
        } else {
            Ok(PatternNode::Concat(nodes))
        }
    }

    fn parse_quantified(&mut self) -> Result<PatternNode, PatternError> {
        let atom = self.parse_atom()?;
        let Some(ch) = self.peek() else {
            return Ok(atom);
        };
        let (min, max) = match ch {
            '*' => {
                self.bump();
                (0, None)
            }
            '+' => {
                self.bump();
                (1, None)
            }
            '?' => {
                self.bump();
                (0, Some(1))
            }
            '{' => self.parse_braced_repeat()?,
            _ => return Ok(atom),
        };
        if self.peek() == Some('?') {
            self.bump();
        }
        Ok(PatternNode::Repeat {
            node: Box::new(atom),
            min,
            max,
        })
    }

    fn parse_atom(&mut self) -> Result<PatternNode, PatternError> {
        let Some(ch) = self.bump() else {
            return Ok(PatternNode::Empty);
        };
        match ch {
            '^' => Ok(PatternNode::Start),
            '$' => Ok(PatternNode::End),
            '.' => Ok(PatternNode::Any),
            '[' => self.parse_class().map(PatternNode::Class),
            '(' => {
                if self.peek() == Some('?') {
                    self.bump();
                    if self.peek() == Some(':') {
                        self.bump();
                    } else {
                        return Err(PatternError::Unsupported);
                    }
                }
                let node = self.parse_alternation(Some(')'))?;
                if self.bump() != Some(')') {
                    return Err(PatternError::Invalid);
                }
                Ok(node)
            }
            ')' | ']' => Err(PatternError::Invalid),
            '\\' => self.parse_escape(false).map(|atom| match atom {
                EscapeAtom::Node(node) => node,
                EscapeAtom::Class(atom) => PatternNode::Class(CharClass {
                    negated: false,
                    atoms: vec![atom],
                }),
            }),
            '*' | '+' | '?' => Err(PatternError::Invalid),
            '{' => {
                if self.try_literal_brace()? {
                    Ok(PatternNode::Literal('{'))
                } else {
                    Err(PatternError::Invalid)
                }
            }
            _ => Ok(PatternNode::Literal(ch)),
        }
    }

    fn parse_braced_repeat(&mut self) -> Result<(usize, Option<usize>), PatternError> {
        let checkpoint = self.pos;
        self.bump();
        let Some(min) = self.parse_usize() else {
            self.pos = checkpoint;
            return Ok((1, Some(1)));
        };
        let max = if self.peek() == Some(',') {
            self.bump();
            self.parse_usize()
        } else {
            Some(min)
        };
        if self.bump() != Some('}') {
            return Err(PatternError::Invalid);
        }
        if max.is_some_and(|max| max < min) {
            return Err(PatternError::Invalid);
        }
        Ok((min, max))
    }

    fn parse_class(&mut self) -> Result<CharClass, PatternError> {
        let negated = if self.peek() == Some('^') {
            self.bump();
            true
        } else {
            false
        };
        let mut atoms = Vec::new();
        let mut first = true;
        while let Some(ch) = self.peek() {
            if ch == ']' && !first {
                self.bump();
                return Ok(CharClass { negated, atoms });
            }
            first = false;
            let atom = self.parse_class_atom()?;
            if self.peek() == Some('-') {
                let checkpoint = self.pos;
                self.bump();
                if self.peek() != Some(']') {
                    let end = self.parse_class_atom()?;
                    if let (ClassAtom::Char(start), ClassAtom::Char(end)) = (&atom, &end) {
                        if start > end {
                            return Err(PatternError::Invalid);
                        }
                        atoms.push(ClassAtom::Range(*start, *end));
                    } else {
                        return Err(PatternError::Unsupported);
                    }
                    continue;
                }
                self.pos = checkpoint;
            }
            atoms.push(atom);
        }
        Err(PatternError::Invalid)
    }

    fn parse_class_atom(&mut self) -> Result<ClassAtom, PatternError> {
        match self.bump() {
            Some('\\') => match self.parse_escape(true)? {
                EscapeAtom::Class(atom) => Ok(atom),
                EscapeAtom::Node(PatternNode::Literal(ch)) => Ok(ClassAtom::Char(ch)),
                _ => Err(PatternError::Unsupported),
            },
            Some(ch) => Ok(ClassAtom::Char(ch)),
            None => Err(PatternError::Invalid),
        }
    }

    fn parse_escape(&mut self, in_class: bool) -> Result<EscapeAtom, PatternError> {
        let Some(ch) = self.bump() else {
            return Err(PatternError::Invalid);
        };
        match ch {
            'd' => Ok(EscapeAtom::Class(ClassAtom::Digit(false))),
            'D' => Ok(EscapeAtom::Class(ClassAtom::Digit(true))),
            's' => Ok(EscapeAtom::Class(ClassAtom::Space(false))),
            'S' => Ok(EscapeAtom::Class(ClassAtom::Space(true))),
            'w' => Ok(EscapeAtom::Class(ClassAtom::Word(false))),
            'W' => Ok(EscapeAtom::Class(ClassAtom::Word(true))),
            'n' => Ok(EscapeAtom::Node(PatternNode::Literal('\n'))),
            'r' => Ok(EscapeAtom::Node(PatternNode::Literal('\r'))),
            't' => Ok(EscapeAtom::Node(PatternNode::Literal('\t'))),
            'f' => Ok(EscapeAtom::Node(PatternNode::Literal('\u{000C}'))),
            'v' => Ok(EscapeAtom::Node(PatternNode::Literal('\u{000B}'))),
            'x' => Ok(EscapeAtom::Node(PatternNode::Literal(
                self.parse_hex_char(2)?,
            ))),
            'u' => Ok(EscapeAtom::Node(PatternNode::Literal(
                self.parse_hex_char(4)?,
            ))),
            'b' | 'B' if !in_class => Err(PatternError::Unsupported),
            'p' | 'P' => Err(PatternError::Unsupported),
            '0'..='9' => Err(PatternError::Unsupported),
            other => Ok(EscapeAtom::Node(PatternNode::Literal(other))),
        }
    }

    fn parse_hex_char(&mut self, digits: usize) -> Result<char, PatternError> {
        let mut value = 0u32;
        for _ in 0..digits {
            let Some(ch) = self.bump() else {
                return Err(PatternError::Invalid);
            };
            value = value
                .checked_mul(16)
                .and_then(|value| ch.to_digit(16).map(|digit| value + digit))
                .ok_or(PatternError::Invalid)?;
        }
        char::from_u32(value).ok_or(PatternError::Invalid)
    }

    fn parse_usize(&mut self) -> Option<usize> {
        let start = self.pos;
        let mut value = 0usize;
        while let Some(ch) = self.peek() {
            let Some(digit) = ch.to_digit(10) else {
                break;
            };
            self.bump();
            value = value.saturating_mul(10).saturating_add(digit as usize);
        }
        (self.pos != start).then_some(value)
    }

    fn try_literal_brace(&mut self) -> Result<bool, PatternError> {
        let checkpoint = self.pos;
        self.pos = self.pos.saturating_sub(1);
        let repeat = self.parse_braced_repeat();
        self.pos = checkpoint;
        match repeat {
            Ok((1, Some(1))) => Ok(true),
            Ok(_) => Ok(false),
            Err(PatternError::Invalid) => Ok(true),
            Err(error) => Err(error),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }
}

enum EscapeAtom {
    Node(PatternNode),
    Class(ClassAtom),
}

fn dedup_positions(mut positions: Vec<usize>) -> Vec<usize> {
    positions.sort_unstable();
    positions.dedup();
    positions
}

