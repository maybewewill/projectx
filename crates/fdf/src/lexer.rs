//! Tokenizer for FDF text.
//!
//! Numbers are plain decimal floats. WC3 has no unit suffixes: values are
//! screen fractions. A number directly followed by a letter or `%` is an
//! error, so a unit typo is not silently split into two tokens.

use crate::error::{ParseError, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Str(String),
    Num(f32),
    LBrace,
    RBrace,
    Comma,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

struct Cursor {
    chars: Vec<char>,
    i: usize,
    line: u32,
    col: u32,
}

impl Cursor {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }
    fn peek_at(&self, off: usize) -> Option<char> {
        self.chars.get(self.i + off).copied()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }
    fn span(&self) -> Span {
        Span {
            line: self.line,
            col: self.col,
        }
    }
}

/// Splits FDF source into tokens. Comments (`//`, `/* */`) are dropped.
pub fn lex(src: &str) -> Result<Vec<Token>, ParseError> {
    let mut cur = Cursor {
        chars: src.chars().collect(),
        i: 0,
        line: 1,
        col: 1,
    };
    let mut out = Vec::new();
    while let Some(c) = cur.peek() {
        if c.is_whitespace() {
            cur.bump();
            continue;
        }
        if c == '/' && cur.peek_at(1) == Some('/') {
            while let Some(ch) = cur.peek() {
                if ch == '\n' {
                    break;
                }
                cur.bump();
            }
            continue;
        }
        if c == '/' && cur.peek_at(1) == Some('*') {
            let start = cur.span();
            cur.bump();
            cur.bump();
            loop {
                match cur.peek() {
                    None => return Err(ParseError::at(start, "unterminated /* comment")),
                    Some('*') if cur.peek_at(1) == Some('/') => {
                        cur.bump();
                        cur.bump();
                        break;
                    }
                    Some(_) => {
                        cur.bump();
                    }
                }
            }
            continue;
        }
        let span = cur.span();
        match c {
            '{' => {
                cur.bump();
                out.push(Token {
                    tok: Tok::LBrace,
                    span,
                });
            }
            '}' => {
                cur.bump();
                out.push(Token {
                    tok: Tok::RBrace,
                    span,
                });
            }
            ',' => {
                cur.bump();
                out.push(Token {
                    tok: Tok::Comma,
                    span,
                });
            }
            '"' => {
                cur.bump();
                let s = lex_string(&mut cur, span)?;
                out.push(Token {
                    tok: Tok::Str(s),
                    span,
                });
            }
            '0'..='9' => out.push(lex_number(&mut cur, span)?),
            '+' | '-' | '.' => {
                let digit_next = match c {
                    '.' => cur.peek_at(1).is_some_and(|d| d.is_ascii_digit()),
                    _ => cur
                        .peek_at(1)
                        .is_some_and(|d| d.is_ascii_digit() || d == '.'),
                };
                if !digit_next {
                    return Err(ParseError::at(span, format!("unexpected character '{c}'")));
                }
                out.push(lex_number(&mut cur, span)?);
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut s = String::new();
                while let Some(ch) = cur.peek() {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        s.push(ch);
                        cur.bump();
                    } else {
                        break;
                    }
                }
                out.push(Token {
                    tok: Tok::Ident(s),
                    span,
                });
            }
            other => {
                return Err(ParseError::at(
                    span,
                    format!("unexpected character '{other}'"),
                ));
            }
        }
    }
    Ok(out)
}

fn lex_string(cur: &mut Cursor, start: Span) -> Result<String, ParseError> {
    let mut s = String::new();
    loop {
        match cur.bump() {
            None | Some('\n') => return Err(ParseError::at(start, "unterminated string")),
            Some('"') => return Ok(s),
            Some('\\') => match cur.peek() {
                // `\"` and `\\` are the only escapes. Any other backslash is
                // kept as-is, because FDF paths use single backslashes.
                Some('"') => {
                    cur.bump();
                    s.push('"');
                }
                Some('\\') => {
                    cur.bump();
                    s.push('\\');
                }
                _ => s.push('\\'),
            },
            Some(ch) => s.push(ch),
        }
    }
}

fn lex_number(cur: &mut Cursor, start: Span) -> Result<Token, ParseError> {
    let mut text = String::new();
    if let Some(sign @ ('+' | '-')) = cur.peek() {
        text.push(sign);
        cur.bump();
    }
    while let Some(ch) = cur.peek() {
        if ch.is_ascii_digit() || ch == '.' {
            text.push(ch);
            cur.bump();
        } else {
            break;
        }
    }
    if matches!(cur.peek(), Some('e' | 'E')) {
        let exp_ok = match cur.peek_at(1) {
            Some(d) if d.is_ascii_digit() => true,
            Some('+' | '-') => cur.peek_at(2).is_some_and(|d| d.is_ascii_digit()),
            _ => false,
        };
        if exp_ok {
            text.push('e');
            cur.bump();
            if let Some(sign @ ('+' | '-')) = cur.peek() {
                text.push(sign);
                cur.bump();
            }
            while let Some(ch) = cur.peek() {
                if ch.is_ascii_digit() {
                    text.push(ch);
                    cur.bump();
                } else {
                    break;
                }
            }
        }
    }
    if cur
        .peek()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '%' || c == '_')
    {
        return Err(ParseError::at(
            start,
            "number followed by a letter or '%': FDF numbers take no unit suffix",
        ));
    }
    let value: f32 = text
        .parse()
        .map_err(|_| ParseError::at(start, format!("malformed number '{text}'")))?;
    Ok(Token {
        tok: Tok::Num(value),
        span: start,
    })
}
