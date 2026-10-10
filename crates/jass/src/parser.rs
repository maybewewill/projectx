use crate::ast::*;
use crate::lexer::{Tok, Token};
use crate::Error;

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Self { toks, pos: 0 }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn line(&self) -> u32 {
        self.toks[self.pos].line
    }

    fn bump(&mut self) -> Tok {
        let t = self.toks[self.pos].tok.clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn err<T>(&self, msg: impl Into<String>) -> Result<T, Error> {
        Err(Error::Parse {
            line: self.line(),
            msg: format!("{} (found {:?})", msg.into(), self.peek()),
        })
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s == kw)
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_kw(&mut self, kw: &str) -> Result<(), Error> {
        if self.eat_kw(kw) {
            Ok(())
        } else {
            self.err(format!("expected '{kw}'"))
        }
    }

    fn expect(&mut self, t: Tok) -> Result<(), Error> {
        if *self.peek() == t {
            self.bump();
            Ok(())
        } else {
            self.err(format!("expected {t:?}"))
        }
    }

    fn ident(&mut self) -> Result<String, Error> {
        match self.peek() {
            Tok::Ident(s) if !is_keyword(s) => {
                let s = s.clone();
                self.bump();
                Ok(s)
            }
            _ => self.err("expected identifier"),
        }
    }

    fn newline(&mut self) -> Result<(), Error> {
        match self.peek() {
            Tok::Newline => {
                self.bump();
                Ok(())
            }
            Tok::Eof => Ok(()),
            _ => self.err("expected end of line"),
        }
    }

    fn skip_newlines(&mut self) {
        while *self.peek() == Tok::Newline {
            self.bump();
        }
    }

    pub fn program(&mut self) -> Result<Program, Error> {
        let mut p = Program {
            types: vec![],
            globals: vec![],
            natives: vec![],
            functions: vec![],
        };
        loop {
            self.skip_newlines();
            if *self.peek() == Tok::Eof {
                return Ok(p);
            }
            let constant = self.eat_kw("constant");
            if self.eat_kw("type") {
                let name = self.ident()?;
                self.expect_kw("extends")?;
                let parent = self.ident()?;
                p.types.push(TypeDef { name, parent });
                self.newline()?;
            } else if self.eat_kw("globals") {
                self.newline()?;
                loop {
                    self.skip_newlines();
                    if self.eat_kw("endglobals") {
                        break;
                    }
                    p.globals.push(self.global()?);
                }
                self.newline()?;
            } else if self.eat_kw("native") {
                p.natives.push(Native {
                    constant,
                    sig: self.signature()?,
                });
                self.newline()?;
            } else if self.is_kw("function") {
                let line = self.line();
                self.bump();
                let mut f = self.function_body(constant)?;
                f.line = line;
                p.functions.push(f);
            } else {
                return self.err("expected declaration");
            }
        }
    }

    fn global(&mut self) -> Result<Global, Error> {
        let line = self.line();
        let constant = self.eat_kw("constant");
        let ty = self.ident()?;
        let array = self.eat_kw("array");
        let name = self.ident()?;
        let init = if *self.peek() == Tok::Assign {
            self.bump();
            Some(self.expr()?)
        } else {
            None
        };
        self.newline()?;
        Ok(Global {
            constant,
            ty,
            array,
            name,
            init,
            line,
        })
    }

    fn signature(&mut self) -> Result<Signature, Error> {
        let name = self.ident()?;
        self.expect_kw("takes")?;
        let mut params = Vec::new();
        if !self.eat_kw("nothing") {
            loop {
                let ty = self.ident()?;
                let name = self.ident()?;
                params.push(Param { ty, name });
                if *self.peek() != Tok::Comma {
                    break;
                }
                self.bump();
            }
        }
        self.expect_kw("returns")?;
        let returns = if self.eat_kw("nothing") {
            None
        } else {
            Some(self.ident()?)
        };
        Ok(Signature {
            name,
            params,
            returns,
        })
    }

    fn function_body(&mut self, constant: bool) -> Result<Function, Error> {
        let sig = self.signature()?;
        self.newline()?;
        let mut locals = Vec::new();
        loop {
            self.skip_newlines();
            if !self.eat_kw("local") {
                break;
            }
            let ty = self.ident()?;
            let array = self.eat_kw("array");
            let name = self.ident()?;
            let init = if *self.peek() == Tok::Assign {
                self.bump();
                Some(self.expr()?)
            } else {
                None
            };
            self.newline()?;
            locals.push(Local {
                ty,
                array,
                name,
                init,
            });
        }
        let body = self.block(&["endfunction"])?;
        self.expect_kw("endfunction")?;
        self.newline()?;
        Ok(Function {
            constant,
            sig,
            locals,
            body,
            line: 0,
        })
    }

    /// Parses statements until one of `terminators` (not consumed).
    fn block(&mut self, terminators: &[&str]) -> Result<Vec<Stmt>, Error> {
        let mut out = Vec::new();
        loop {
            self.skip_newlines();
            if terminators.iter().any(|t| self.is_kw(t)) {
                return Ok(out);
            }
            if *self.peek() == Tok::Eof {
                return self.err(format!("expected one of {terminators:?}"));
            }
            out.push(self.statement()?);
        }
    }

    fn statement(&mut self) -> Result<Stmt, Error> {
        if self.eat_kw("debug") {
            return Ok(Stmt::Debug(Box::new(self.statement()?)));
        }
        let stmt = if self.eat_kw("set") {
            let name = self.ident()?;
            let index = if *self.peek() == Tok::LBracket {
                self.bump();
                let e = self.expr()?;
                self.expect(Tok::RBracket)?;
                Some(e)
            } else {
                None
            };
            self.expect(Tok::Assign)?;
            Stmt::Set {
                name,
                index,
                value: self.expr()?,
            }
        } else if self.eat_kw("call") {
            let name = self.ident()?;
            Stmt::Call {
                name,
                args: self.args()?,
            }
        } else if self.eat_kw("if") {
            let mut branches = Vec::new();
            let mut cond = self.expr()?;
            let otherwise;
            loop {
                self.expect_kw("then")?;
                self.newline()?;
                let body = self.block(&["elseif", "else", "endif"])?;
                branches.push((cond, body));
                if self.eat_kw("elseif") {
                    cond = self.expr()?;
                    continue;
                }
                if self.eat_kw("else") {
                    self.newline()?;
                    otherwise = self.block(&["endif"])?;
                } else {
                    otherwise = Vec::new();
                }
                self.expect_kw("endif")?;
                break;
            }
            Stmt::If {
                branches,
                otherwise,
            }
        } else if self.eat_kw("loop") {
            self.newline()?;
            let body = self.block(&["endloop"])?;
            self.expect_kw("endloop")?;
            Stmt::Loop(body)
        } else if self.eat_kw("exitwhen") {
            Stmt::ExitWhen(self.expr()?)
        } else if self.eat_kw("return") {
            if matches!(self.peek(), Tok::Newline | Tok::Eof) {
                Stmt::Return(None)
            } else {
                Stmt::Return(Some(self.expr()?))
            }
        } else {
            return self.err("expected statement");
        };
        self.newline()?;
        Ok(stmt)
    }

    fn args(&mut self) -> Result<Vec<Expr>, Error> {
        self.expect(Tok::LParen)?;
        let mut args = Vec::new();
        if *self.peek() != Tok::RParen {
            loop {
                args.push(self.expr()?);
                if *self.peek() != Tok::Comma {
                    break;
                }
                self.bump();
            }
        }
        self.expect(Tok::RParen)?;
        Ok(args)
    }

    // Precedence follows pjass: `and`/`or` share the lowest level (left-assoc), then comparisons,
    // then `not`, then + -, then * /, then unary minus.
    // TODO: verify against the original game's parser in Game.dll (parser.cpp) — affects behaviour.
    pub fn expr(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.cmp()?;
        loop {
            let op = if self.is_kw("or") {
                BinOp::Or
            } else if self.is_kw("and") {
                BinOp::And
            } else {
                return Ok(lhs);
            };
            self.bump();
            let rhs = self.cmp()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn cmp(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.not()?;
        loop {
            let op = match self.peek() {
                Tok::Eq => BinOp::Eq,
                Tok::Ne => BinOp::Ne,
                Tok::Lt => BinOp::Lt,
                Tok::Le => BinOp::Le,
                Tok::Gt => BinOp::Gt,
                Tok::Ge => BinOp::Ge,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.not()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn not(&mut self) -> Result<Expr, Error> {
        if self.eat_kw("not") {
            Ok(Expr::Not(Box::new(self.not()?)))
        } else {
            self.add()
        }
    }

    fn add(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.mul()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.mul()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn mul(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.unary()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn unary(&mut self) -> Result<Expr, Error> {
        match self.peek() {
            Tok::Minus => {
                self.bump();
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Tok::Plus => {
                self.bump();
                self.unary()
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Expr, Error> {
        match self.bump() {
            Tok::Int(v) => Ok(Expr::Int(v)),
            Tok::Real(v) => Ok(Expr::Real(v)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::LParen => {
                let e = self.expr()?;
                self.expect(Tok::RParen)?;
                Ok(e)
            }
            Tok::Ident(s) => match s.as_str() {
                "true" => Ok(Expr::Bool(true)),
                "false" => Ok(Expr::Bool(false)),
                "null" => Ok(Expr::Null),
                "function" => Ok(Expr::FuncRef(self.ident()?)),
                "not" => Ok(Expr::Not(Box::new(self.not()?))),
                kw if is_keyword(kw) => {
                    self.pos -= 1;
                    self.err("unexpected keyword in expression")
                }
                _ => match self.peek() {
                    Tok::LParen => Ok(Expr::Call(s, self.args()?)),
                    Tok::LBracket => {
                        self.bump();
                        let idx = self.expr()?;
                        self.expect(Tok::RBracket)?;
                        Ok(Expr::Index(s, Box::new(idx)))
                    }
                    _ => Ok(Expr::Var(s)),
                },
            },
            _ => {
                self.pos -= 1;
                self.err("expected expression")
            }
        }
    }
}

pub fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "and"
            | "array"
            | "call"
            | "constant"
            | "debug"
            | "else"
            | "elseif"
            | "endfunction"
            | "endglobals"
            | "endif"
            | "endloop"
            | "exitwhen"
            | "extends"
            | "false"
            | "function"
            | "globals"
            | "if"
            | "local"
            | "loop"
            | "native"
            | "not"
            | "nothing"
            | "null"
            | "or"
            | "return"
            | "returns"
            | "set"
            | "takes"
            | "then"
            | "true"
            | "type"
    )
}
