use crate::Error;
use softfloat::Tables;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i32),
    Real(f32),
    Str(String),
    Newline,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Assign,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
}

/// Converts JASS source into tokens, without lookup tables. Keywords are returned as `Ident` and
/// recognised by the parser. Line breaks are significant in JASS, so they are emitted as
/// `Newline` (collapsed).
///
/// Integer literals are exact. Real literals are only correctly rounded (Rust's parser) here:
/// the original converts them with its soft-float helpers, which need the reciprocal table.
/// Use [`lex_with`] with the table for the original's bits.
pub fn lex(src: &str) -> Result<Vec<Token>, Error> {
    lex_with(src, None)
}

/// Like [`lex`], but real literals use the original's soft-float conversion
/// (`softfloat::text::real_literal`, the scanner's real action `0x6F466690`): for example `0.1`
/// is `0x3DCCCCCE`, one ulp above the correctly rounded value. Without the table (`None`) the
/// real literals fall back to the correctly rounded parse, as [`lex`] does.
pub fn lex_with(src: &str, tables: Option<&Tables>) -> Result<Vec<Token>, Error> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut line = 1u32;
    let mut out: Vec<Token> = Vec::new();
    let push = |out: &mut Vec<Token>, tok: Tok, line: u32| {
        if tok == Tok::Newline
            && matches!(
                out.last(),
                None | Some(Token {
                    tok: Tok::Newline,
                    ..
                })
            )
        {
            return;
        }
        out.push(Token { tok, line });
    };
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' => i += 1,
            b'\r' | b'\n' => {
                if c == b'\r' && b.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                push(&mut out, Tok::Newline, line);
                line += 1;
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' && b[i] != b'\r' {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                let mut s = Vec::new();
                loop {
                    let Some(&ch) = b.get(i) else {
                        return Err(Error::Lex {
                            line,
                            msg: "unterminated string".into(),
                        });
                    };
                    i += 1;
                    match ch {
                        b'"' => break,
                        b'\\' => {
                            let e = *b.get(i).ok_or(Error::Lex {
                                line,
                                msg: "bad escape".into(),
                            })?;
                            i += 1;
                            // The original accepts exactly these escapes (probed for every
                            // printable character); any other `\x` is a lexical error.
                            s.push(match e {
                                b'n' => b'\n',
                                b'r' => b'\r',
                                b't' => b'\t',
                                b'b' => 0x08,
                                b'f' => 0x0C,
                                b'"' | b'\'' | b'\\' => e,
                                _ => {
                                    return Err(Error::Lex {
                                        line,
                                        msg: format!("bad escape \\{}", e as char),
                                    })
                                }
                            });
                        }
                        b'\n' => {
                            line += 1;
                            s.push(ch);
                        }
                        _ => s.push(ch),
                    }
                }
                push(
                    &mut out,
                    Tok::Str(String::from_utf8_lossy(&s).into_owned()),
                    line,
                );
            }
            b'\'' => {
                // Rawcode: 1 or 4 characters packed big-endian into an integer.
                let start = i + 1;
                let end = b[start..]
                    .iter()
                    .position(|&x| x == b'\'')
                    .map(|p| start + p)
                    .ok_or(Error::Lex {
                        line,
                        msg: "unterminated rawcode".into(),
                    })?;
                let raw = &b[start..end];
                // A single escaped character (`'\n'`) is one character; escapes inside a four
                // character code are an error in the original.
                if let [b'\\', e] = raw {
                    let v = match e {
                        b'b' => 0x08,
                        b'f' => 0x0C,
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'\\' => b'\\',
                        _ => {
                            return Err(Error::Lex {
                                line,
                                msg: format!("bad rawcode escape \\{}", *e as char),
                            })
                        }
                    };
                    push(&mut out, Tok::Int(v as i32), line);
                    i = end + 1;
                    continue;
                }
                if (raw.len() != 1 && raw.len() != 4) || (raw.len() == 4 && raw.contains(&b'\\')) {
                    return Err(Error::Lex {
                        line,
                        msg: format!("rawcode must be 1 or 4 chars, got {}", raw.len()),
                    });
                }
                let v = raw.iter().fold(0u32, |acc, &x| (acc << 8) | x as u32) as i32;
                push(&mut out, Tok::Int(v), line);
                i = end + 1;
            }
            b'0'..=b'9' | b'.' | b'$' => {
                let start = i;
                let tok = if c == b'$' || (c == b'0' && matches!(b.get(i + 1), Some(b'x' | b'X'))) {
                    i += if c == b'$' { 1 } else { 2 };
                    let s = i;
                    while i < b.len() && b[i].is_ascii_hexdigit() {
                        i += 1;
                    }
                    Tok::Int(parse_radix(&src[s..i], 16, line)?)
                } else {
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i < b.len() && b[i] == b'.' {
                        i += 1;
                        while i < b.len() && b[i].is_ascii_digit() {
                            i += 1;
                        }
                        let text = &src[start..i];
                        if text == "." {
                            return Err(Error::Lex {
                                line,
                                msg: "stray '.'".into(),
                            });
                        }
                        let value = match softfloat::text::real_literal(tables, text.as_bytes()) {
                            Some(v) => v.to_f32(),
                            None => text.parse::<f32>().map_err(|e| Error::Lex {
                                line,
                                msg: e.to_string(),
                            })?,
                        };
                        Tok::Real(value)
                    } else {
                        let text = &src[start..i];
                        if text.len() > 1 && text.starts_with('0') {
                            Tok::Int(parse_radix(&text[1..], 8, line)?)
                        } else {
                            Tok::Int(parse_radix(text, 10, line)?)
                        }
                    }
                };
                push(&mut out, tok, line);
            }
            // An identifier starts with a letter; a leading `_` is a lexical error in the original.
            c if c.is_ascii_alphabetic() => {
                let s = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                push(&mut out, Tok::Ident(src[s..i].to_string()), line);
            }
            _ => {
                let two = b.get(i + 1).copied();
                let (tok, n) = match (c, two) {
                    (b'=', Some(b'=')) => (Tok::Eq, 2),
                    (b'!', Some(b'=')) => (Tok::Ne, 2),
                    (b'<', Some(b'=')) => (Tok::Le, 2),
                    (b'>', Some(b'=')) => (Tok::Ge, 2),
                    (b'=', _) => (Tok::Assign, 1),
                    (b'<', _) => (Tok::Lt, 1),
                    (b'>', _) => (Tok::Gt, 1),
                    (b'(', _) => (Tok::LParen, 1),
                    (b')', _) => (Tok::RParen, 1),
                    (b'[', _) => (Tok::LBracket, 1),
                    (b']', _) => (Tok::RBracket, 1),
                    (b',', _) => (Tok::Comma, 1),
                    (b'+', _) => (Tok::Plus, 1),
                    (b'-', _) => (Tok::Minus, 1),
                    (b'*', _) => (Tok::Star, 1),
                    (b'/', _) => (Tok::Slash, 1),
                    _ => {
                        return Err(Error::Lex {
                            line,
                            msg: format!("unexpected character {:?}", c as char),
                        })
                    }
                };
                push(&mut out, tok, line);
                i += n;
            }
        }
    }
    push(&mut out, Tok::Newline, line);
    out.push(Token {
        tok: Tok::Eof,
        line,
    });
    Ok(out)
}

/// Integer literals wrap on overflow to 32 bits (TODO: confirm the original's overflow behaviour).
fn parse_radix(digits: &str, radix: u32, line: u32) -> Result<i32, Error> {
    if digits.is_empty() {
        return Err(Error::Lex {
            line,
            msg: "empty number".into(),
        });
    }
    let mut v: u32 = 0;
    for ch in digits.chars() {
        let d = ch.to_digit(radix).ok_or(Error::Lex {
            line,
            msg: format!("bad digit {ch:?}"),
        })?;
        v = v.wrapping_mul(radix).wrapping_add(d);
    }
    Ok(v as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn literals() {
        assert_eq!(
            toks("10 0x1F $ff 017 'hfoo' 'A' 1.5 .25 \"a\\\"b\""),
            vec![
                Tok::Int(10),
                Tok::Int(31),
                Tok::Int(255),
                Tok::Int(15),
                Tok::Int(0x68666F6F),
                Tok::Int(65),
                Tok::Real(1.5),
                Tok::Real(0.25),
                Tok::Str("a\"b".into()),
                Tok::Newline,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn comments_and_newlines_collapse() {
        assert_eq!(
            toks("a // x\r\n\n\nb != c"),
            vec![
                Tok::Ident("a".into()),
                Tok::Newline,
                Tok::Ident("b".into()),
                Tok::Ne,
                Tok::Ident("c".into()),
                Tok::Newline,
                Tok::Eof
            ]
        );
    }
}
