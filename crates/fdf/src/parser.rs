//! Recursive-descent parser for FDF.
//!
//! Grammar (informal):
//! ```text
//! document  := (IncludeFile STR ','? | StringList | Frame) ','?
//! StringList:= 'StringList' '{' (IDENT (STR|IDENT|NUM) ','?)* '}'
//! Frame     := 'Frame' NAME NAME ('INHERITS' 'WITHCHILDREN'? NAME)? '{' members '}'
//! members   := (property | Frame | Layer | Texture | String | ',')*
//! Layer     := 'Layer' NAME '{' members '}'          (only frames and blocks)
//! Texture / String := ('Texture'|'String') NAME? '{' property* '}'
//! property  := IDENT value* ','?
//! ```
//! A property's values are separated by commas or whitespace. A comma ends
//! the property unless the next token is a value on the same line or a number
//! or string, so `TexCoord 0, 1, 0, 1,` and `SetPoint TOPLEFT, "X", ...` work.
//! A property may also end at the next line that starts with a known keyword.

use crate::ast::*;
use crate::error::{ParseError, Span};
use crate::lexer::{lex, Tok, Token};

/// Property keys and block keywords that end a value list.
const KEYWORDS: &[&str] = &[
    "FRAME",
    "LAYER",
    "TEXTURE",
    "STRING",
    "STRINGLIST",
    "INCLUDEFILE",
    "INHERITS",
    "WIDTH",
    "HEIGHT",
    "SETPOINT",
    "SETALLPOINTS",
    "ANCHOR",
    "DECORATEFILENAMES",
    "TEXT",
    "TEXTLENGTH",
    "FRAMEFONT",
    "FONT",
    "FONTCOLOR",
    "FONTDISABLEDCOLOR",
    "FONTHIGHLIGHTCOLOR",
    "FONTSHADOWCOLOR",
    "FONTSHADOWOFFSET",
    "FONTJUSTIFICATIONH",
    "FONTJUSTIFICATIONV",
    "FONTJUSTIFICATIONOFFSET",
    "FONTFLAGS",
    "BACKDROPBACKGROUND",
    "BACKDROPBACKGROUNDINSETS",
    "BACKDROPBACKGROUNDSIZE",
    "BACKDROPEDGEFILE",
    "BACKDROPCORNERFILE",
    "BACKDROPCORNERFLAGS",
    "BACKDROPCORNERSIZE",
    "BACKDROPTILEBACKGROUND",
    "BACKDROPBLENDALL",
    "BACKDROPMIRRORED",
    "BACKDROPHALFSIDES",
    "BACKDROPBOTTOMFILE",
    "BACKDROPTOPFILE",
    "BACKDROPLEFTFILE",
    "BACKDROPRIGHTFILE",
    "CONTROLSTYLE",
    "CONTROLBACKDROP",
    "CONTROLDISABLEDBACKDROP",
    "CONTROLPUSHEDBACKDROP",
    "CONTROLSHORTCUTKEY",
    "CONTROLMOUSEOVERHIGHLIGHT",
    "BUTTONTEXT",
    "TOOLTIP",
    "LAYERSTYLE",
    "FILE",
    "ALPHAMODE",
    "TEXCOORD",
    "BACKGROUNDART",
    "SPRITECAMERA",
    "SPRITESCALE",
    "STATUSBARSPRITE",
    "BARTEXTURE",
    "SLIDERLAYOUTHORIZONTAL",
    "SLIDERLAYOUTVERTICAL",
    "SLIDERMINVALUE",
    "SLIDERMAXVALUE",
    "SLIDERINITIALVALUE",
    "SLIDERSTEPSIZE",
    "SLIDERTHUMBBUTTONFRAME",
    "SCROLLBARDECBUTTONFRAME",
    "SCROLLBARINCBUTTONFRAME",
    "TEXTAREALINEHEIGHT",
    "TEXTAREALINEGAP",
    "TEXTAREAINSET",
    "TEXTAREAMAXLINES",
    "TEXTAREASCROLLBAR",
    "TABFOCUSPUSH",
    "TABFOCUSDEFAULT",
    "TABFOCUSNEXT",
    "MENUITEM",
    "MENUITEMHEIGHT",
    "MENUBORDER",
    "MENUTEXTHIGHLIGHTCOLOR",
    "POPUPBUTTONINSET",
    "POPUPMENUFRAME",
    "POPUPTITLEFRAME",
    "POPUPARROWFRAME",
    "MODELFILE",
];

fn is_keyword(word: &str) -> bool {
    let up = word.to_ascii_uppercase();
    KEYWORDS.contains(&up.as_str())
}

/// Parses an FDF source file. Errors carry the line and column.
pub fn parse(src: &str) -> Result<Document, ParseError> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0 };
    let mut items = Vec::new();
    while let Some(t) = p.peek().cloned() {
        match &t.tok {
            Tok::Comma => {
                p.bump();
            }
            Tok::Ident(name) => match name.to_ascii_uppercase().as_str() {
                "INCLUDEFILE" => {
                    p.bump();
                    let (path, span) = p.expect_str("IncludeFile path")?;
                    p.skip_comma();
                    items.push(TopItem::Include { path, span });
                }
                "STRINGLIST" => {
                    let list = p.parse_string_list()?;
                    items.push(TopItem::StringList(list));
                }
                "FRAME" => {
                    let f = p.parse_frame(None)?;
                    p.skip_comma();
                    items.push(TopItem::Frame(f));
                }
                _ => {
                    return Err(ParseError::at(
                        t.span,
                        format!("unexpected '{name}' at top level"),
                    ))
                }
            },
            _ => {
                return Err(ParseError::at(
                    t.span,
                    "expected IncludeFile, StringList or Frame",
                ))
            }
        }
    }
    Ok(Document { items })
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

#[derive(Default)]
struct Members {
    props: Vec<Property>,
    children: Vec<Child>,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    fn peek_at(&self, off: usize) -> Option<&Token> {
        self.toks.get(self.pos + off)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eof_span(&self) -> Span {
        self.toks.last().map(|t| t.span).unwrap_or_default()
    }

    fn unexpected_eof(&self, what: &str) -> ParseError {
        ParseError::at(
            self.eof_span(),
            format!("unexpected end of file, expected {what}"),
        )
    }

    fn skip_comma(&mut self) {
        if matches!(self.peek().map(|t| &t.tok), Some(Tok::Comma)) {
            self.bump();
        }
    }

    fn expect_str(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        match self.bump() {
            Some(Token {
                tok: Tok::Str(s),
                span,
            }) => Ok((s, span)),
            Some(t) => Err(ParseError::at(
                t.span,
                format!("expected a quoted string for {what}"),
            )),
            None => Err(self.unexpected_eof(what)),
        }
    }

    /// A quoted string or a bare word (frame types and names may be either).
    fn expect_name(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        match self.bump() {
            Some(Token {
                tok: Tok::Str(s) | Tok::Ident(s),
                span,
            }) => Ok((s, span)),
            Some(t) => Err(ParseError::at(
                t.span,
                format!("expected a name for {what}"),
            )),
            None => Err(self.unexpected_eof(what)),
        }
    }

    fn expect_tok(&mut self, want: Tok, what: &str) -> Result<Span, ParseError> {
        match self.bump() {
            Some(t) if t.tok == want => Ok(t.span),
            Some(t) => Err(ParseError::at(t.span, format!("expected {what}"))),
            None => Err(self.unexpected_eof(what)),
        }
    }

    fn parse_string_list(&mut self) -> Result<StringList, ParseError> {
        self.bump(); // StringList
        self.expect_tok(Tok::LBrace, "'{' after StringList")?;
        let mut list = StringList::default();
        loop {
            let t = match self.bump() {
                Some(t) => t,
                None => return Err(self.unexpected_eof("'}' closing StringList")),
            };
            match t.tok {
                Tok::RBrace => break,
                Tok::Comma => {}
                Tok::Ident(key) => {
                    let value = match self.bump() {
                        Some(Token {
                            tok: Tok::Str(s) | Tok::Ident(s),
                            ..
                        }) => s,
                        Some(Token {
                            tok: Tok::Num(n), ..
                        }) => format!("{n}"),
                        Some(v) => {
                            return Err(ParseError::at(
                                v.span,
                                format!("expected a value for '{key}'"),
                            ))
                        }
                        None => return Err(self.unexpected_eof("a StringList value")),
                    };
                    list.entries.push(StringEntry {
                        key,
                        value,
                        span: t.span,
                    });
                    self.skip_comma();
                }
                _ => return Err(ParseError::at(t.span, "expected a StringList key")),
            }
        }
        self.skip_comma();
        Ok(list)
    }

    /// Parses `Frame "TYPE" "Name" [INHERITS [WITHCHILDREN] "Parent"] { ... }`.
    fn parse_frame(&mut self, layer: Option<String>) -> Result<Frame, ParseError> {
        let head = self.bump().ok_or_else(|| self.unexpected_eof("Frame"))?;
        let (kind, _) = self.expect_name("frame type")?;
        let (name, _) = self.expect_name("frame name")?;
        let mut inherits = None;
        if matches!(self.peek().map(|t| &t.tok), Some(Tok::Ident(w)) if w.eq_ignore_ascii_case("INHERITS"))
        {
            self.bump();
            let mut with_children = false;
            if matches!(self.peek().map(|t| &t.tok), Some(Tok::Ident(w)) if w.eq_ignore_ascii_case("WITHCHILDREN"))
            {
                self.bump();
                with_children = true;
            }
            let (parent, _) = self.expect_name("INHERITS parent name")?;
            inherits = Some(Inherits {
                parent,
                with_children,
            });
        }
        self.expect_tok(Tok::LBrace, "'{' opening a Frame")?;
        let members = self.parse_members(None)?;
        self.expect_tok(Tok::RBrace, "'}' closing a Frame")?;
        Ok(Frame {
            kind: kind.to_ascii_uppercase(),
            name,
            inherits,
            props: members.props,
            children: members.children,
            layer,
            span: head.span,
        })
    }

    fn parse_members(&mut self, layer: Option<&str>) -> Result<Members, ParseError> {
        let mut m = Members::default();
        while let Some(t) = self.peek().cloned() {
            match &t.tok {
                Tok::RBrace => break,
                Tok::Comma => {
                    self.bump();
                }
                Tok::Ident(word) => match word.to_ascii_uppercase().as_str() {
                    "FRAME" => {
                        let f = self.parse_frame(layer.map(str::to_string))?;
                        m.children.push(Child::Frame(f));
                    }
                    "LAYER" => {
                        if layer.is_some() {
                            return Err(ParseError::at(
                                t.span,
                                "Layer cannot be nested in a Layer",
                            ));
                        }
                        self.bump();
                        let (level, _) = self.expect_name("layer level")?;
                        self.expect_tok(Tok::LBrace, "'{' opening a Layer")?;
                        let inner = self.parse_members(Some(&level))?;
                        self.expect_tok(Tok::RBrace, "'}' closing a Layer")?;
                        if let Some(p) = inner.props.first() {
                            return Err(ParseError::at(
                                p.span,
                                "properties are not allowed directly in a Layer",
                            ));
                        }
                        m.children.extend(inner.children);
                    }
                    "TEXTURE" | "STRING" => {
                        self.bump();
                        let kind = if word.eq_ignore_ascii_case("TEXTURE") {
                            BlockKind::Texture
                        } else {
                            BlockKind::String
                        };
                        let name = match self.peek().map(|t| &t.tok) {
                            Some(Tok::Str(s) | Tok::Ident(s)) => {
                                let s = s.clone();
                                self.bump();
                                Some(s)
                            }
                            _ => None,
                        };
                        self.expect_tok(Tok::LBrace, "'{' opening a Texture or String")?;
                        let inner = self.parse_members(None)?;
                        self.expect_tok(Tok::RBrace, "'}' closing a Texture or String")?;
                        if let Some(c) = inner.children.first() {
                            let span = match c {
                                Child::Frame(f) => f.span,
                                Child::Block(b) => b.span,
                            };
                            return Err(ParseError::at(
                                span,
                                "only properties are allowed in a Texture or String",
                            ));
                        }
                        m.children.push(Child::Block(Block {
                            kind,
                            name,
                            props: inner.props,
                            layer: layer.map(str::to_string),
                            span: t.span,
                        }));
                    }
                    _ => {
                        let p = self.parse_property()?;
                        m.props.push(p);
                    }
                },
                _ => {
                    return Err(ParseError::at(
                        t.span,
                        "expected a property, Frame, Layer, Texture or String",
                    ))
                }
            }
        }
        Ok(m)
    }

    fn parse_property(&mut self) -> Result<Property, ParseError> {
        let key_tok = self
            .bump()
            .ok_or_else(|| self.unexpected_eof("a property"))?;
        let key = match &key_tok.tok {
            Tok::Ident(k) => k.clone(),
            _ => return Err(ParseError::at(key_tok.span, "expected a property key")),
        };
        let mut values = Vec::new();
        let mut last_line = key_tok.span.line;
        // True right after a comma that continues the value list: the next
        // value may then be on any line.
        let mut after_comma = false;
        while let Some(t) = self.peek().cloned() {
            match &t.tok {
                Tok::LBrace | Tok::RBrace => break,
                Tok::Comma => {
                    // A bare word after the comma continues the list only when it
                    // is followed by a separator, not by another value (then it
                    // is the next, unknown, key: `A 1, B .5,`).
                    let continues = match (self.peek_at(1), self.peek_at(2)) {
                        (Some(n), _) if matches!(n.tok, Tok::Num(_) | Tok::Str(_)) => true,
                        (Some(n), next) => match &n.tok {
                            Tok::Ident(w) => {
                                let next_is_value = matches!(
                                    next.map(|x| &x.tok),
                                    Some(Tok::Num(_) | Tok::Str(_) | Tok::Ident(_))
                                );
                                !is_keyword(w) && n.span.line == t.span.line && !next_is_value
                            }
                            _ => false,
                        },
                        _ => false,
                    };
                    self.bump();
                    last_line = t.span.line;
                    if !continues {
                        break;
                    }
                    after_comma = true;
                }
                Tok::Ident(w) => {
                    if is_keyword(w) || (!after_comma && t.span.line != last_line) {
                        break;
                    }
                    values.push(Value::Ident(w.clone()));
                    self.bump();
                    last_line = t.span.line;
                    after_comma = false;
                }
                Tok::Str(s) => {
                    if !after_comma && t.span.line != last_line {
                        break;
                    }
                    values.push(Value::Str(s.clone()));
                    self.bump();
                    last_line = t.span.line;
                    after_comma = false;
                }
                Tok::Num(n) => {
                    if !after_comma && t.span.line != last_line {
                        break;
                    }
                    values.push(Value::Num(*n));
                    self.bump();
                    last_line = t.span.line;
                    after_comma = false;
                }
            }
        }
        Ok(Property {
            key,
            values,
            span: key_tok.span,
        })
    }
}

/// Parses a TOC file: one FDF path per line, `//` comments, optional quotes
/// and trailing commas.
pub fn parse_toc(src: &str) -> Vec<String> {
    src.lines()
        .filter_map(|line| {
            let line = match line.find("//") {
                Some(i) => &line[..i],
                None => line,
            };
            let line = line.trim().trim_end_matches(',').trim();
            let line = line.trim_matches('"').trim();
            if line.is_empty() {
                None
            } else {
                Some(line.to_string())
            }
        })
        .collect()
}
