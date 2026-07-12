use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum Priority {
    Left,
    Normal,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeSelector {
    pub priority: Priority,
    expression: Expression,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Expression {
    Path(Vec<SelectorId>),
    And(Vec<Expression>),
    Or(Vec<Expression>),
    Not(Box<Expression>),
}

pub type SelectorId = u32;

#[derive(Default)]
pub struct SelectorSymbols {
    ids: HashMap<Arc<str>, SelectorId>,
    pub values: Vec<Arc<str>>,
}

impl ScopeSelector {
    pub fn matches(
        &self,
        scopes: &[u32],
        scope_matches: &[Box<[bool]>],
    ) -> bool {
        self.expression.matches(scopes, scope_matches)
    }
}

impl Expression {
    fn matches(&self, scopes: &[u32], scope_matches: &[Box<[bool]>]) -> bool {
        match self {
            Self::Path(path) => {
                let mut next_scope = 0;
                path.iter().all(|selector| {
                    let Some(relative) =
                        scopes[next_scope..].iter().position(|scope| {
                            scope_matches[*scope as usize][*selector as usize]
                        })
                    else {
                        return false;
                    };
                    next_scope += relative + 1;
                    true
                })
            }
            Self::And(expressions) => expressions
                .iter()
                .all(|expression| expression.matches(scopes, scope_matches)),
            Self::Or(expressions) => expressions
                .iter()
                .any(|expression| expression.matches(scopes, scope_matches)),
            Self::Not(expression) => !expression.matches(scopes, scope_matches),
        }
    }
}

pub fn parse_scope_selector(
    source: &str,
    symbols: &mut SelectorSymbols,
) -> Vec<ScopeSelector> {
    let mut parser = Parser {
        tokens: tokenize(source),
        position: 0,
        symbols,
    };
    let mut selectors = Vec::new();
    while parser.peek().is_some() {
        let priority = match parser.peek() {
            Some(Token::Left) => {
                parser.position += 1;
                Priority::Left
            }
            Some(Token::Right) => {
                parser.position += 1;
                Priority::Right
            }
            _ => Priority::Normal,
        };
        let Some(expression) = parser.parse_conjunction() else {
            break;
        };
        selectors.push(ScopeSelector {
            priority,
            expression,
        });
        if parser.peek() == Some(&Token::Comma) {
            parser.position += 1;
        } else {
            break;
        }
    }
    selectors
}

struct Parser<'a> {
    tokens: Vec<Token>,
    position: usize,
    symbols: &'a mut SelectorSymbols,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn parse_operand(&mut self) -> Option<Expression> {
        match self.peek()? {
            Token::Minus => {
                self.position += 1;
                self.parse_operand()
                    .map(|expression| Expression::Not(Box::new(expression)))
            }
            Token::LeftParen => {
                self.position += 1;
                let expression = self.parse_inner()?;
                if self.peek() == Some(&Token::RightParen) {
                    self.position += 1;
                }
                Some(expression)
            }
            Token::Identifier(_) => {
                let mut path = Vec::new();
                while let Some(Token::Identifier(identifier)) = self.peek() {
                    let identifier = identifier.clone();
                    self.position += 1;
                    path.push(self.symbols.intern(&identifier));
                }
                Some(Expression::Path(path))
            }
            _ => None,
        }
    }

    fn parse_conjunction(&mut self) -> Option<Expression> {
        let mut expressions = Vec::new();
        while let Some(expression) = self.parse_operand() {
            expressions.push(expression);
        }
        match expressions.len() {
            0 => None,
            1 => expressions.pop(),
            _ => Some(Expression::And(expressions)),
        }
    }

    fn parse_inner(&mut self) -> Option<Expression> {
        let mut expressions = vec![self.parse_conjunction()?];
        while matches!(self.peek(), Some(Token::Pipe | Token::Comma)) {
            while matches!(self.peek(), Some(Token::Pipe | Token::Comma)) {
                self.position += 1;
            }
            if let Some(expression) = self.parse_conjunction() {
                expressions.push(expression);
            }
        }
        match expressions.len() {
            1 => expressions.pop(),
            _ => Some(Expression::Or(expressions)),
        }
    }
}

impl SelectorSymbols {
    fn intern(&mut self, value: &str) -> SelectorId {
        if let Some(id) = self.ids.get(value) {
            return *id;
        }
        let id = self.values.len() as SelectorId;
        let value: Arc<str> = Arc::from(value);
        self.values.push(value.clone());
        self.ids.insert(value, id);
        id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Left,
    Right,
    Identifier(String),
    Comma,
    Pipe,
    Minus,
    LeftParen,
    RightParen,
}

fn tokenize(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<_> = source.chars().collect();
    let mut position = 0;
    while position < chars.len() {
        let ch = chars[position];
        if matches!(ch, 'L' | 'R') && chars.get(position + 1) == Some(&':') {
            tokens.push(if ch == 'L' { Token::Left } else { Token::Right });
            position += 2;
            continue;
        }
        let token = match ch {
            ',' => Some(Token::Comma),
            '|' => Some(Token::Pipe),
            '-' => Some(Token::Minus),
            '(' => Some(Token::LeftParen),
            ')' => Some(Token::RightParen),
            _ => None,
        };
        if let Some(token) = token {
            tokens.push(token);
            position += 1;
            continue;
        }
        if ch.is_alphanumeric() || matches!(ch, '_' | '.' | ':') {
            let start = position;
            position += 1;
            while position < chars.len()
                && (chars[position].is_alphanumeric()
                    || matches!(chars[position], '_' | '.' | ':' | '-'))
            {
                position += 1;
            }
            tokens.push(Token::Identifier(
                chars[start..position].iter().collect(),
            ));
            continue;
        }
        position += 1;
    }
    tokens
}

pub fn scope_matches(chunks: &[&str], selector: &str) -> bool {
    let mut selector_offset = 0;
    let mut next = None;
    for chunk in chunks {
        let bytes = chunk.as_bytes();
        let remaining = selector.len().saturating_sub(selector_offset);
        let compared = remaining.min(bytes.len());
        if bytes[..compared]
            != selector.as_bytes()[selector_offset..selector_offset + compared]
        {
            return false;
        }
        selector_offset += compared;
        if selector_offset == selector.len() {
            next = bytes.get(compared).copied();
            if next.is_some() {
                break;
            }
        }
    }
    selector_offset == selector.len() && next.is_none_or(|byte| byte == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_astro_injection_selector() {
        let mut symbols = SelectorSymbols::default();
        let selector = parse_scope_selector(
            "L:(meta.script.astro) (meta.lang.js | meta.lang.javascript | meta.lang.partytown | meta.lang.node) - (meta source)",
            &mut symbols,
        );
        assert_eq!(selector.len(), 1);
        assert_eq!(selector[0].priority, Priority::Left);
        let scopes = [
            "source.astro",
            "meta.script.astro",
            "meta.lang.javascript",
            "meta.embedded",
            "source.js",
        ];
        let matches: Vec<Box<[bool]>> = scopes
            .iter()
            .map(|scope| {
                symbols
                    .values
                    .iter()
                    .map(|selector| scope_matches(&[scope], selector))
                    .collect()
            })
            .collect();
        assert!(selector[0].matches(&[0, 1, 2], &matches));
        assert!(!selector[0].matches(&[0, 1, 2, 3, 4], &matches));
    }
}
