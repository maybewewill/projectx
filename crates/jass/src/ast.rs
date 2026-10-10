#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub types: Vec<TypeDef>,
    pub globals: Vec<Global>,
    pub natives: Vec<Native>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub name: String,
    pub parent: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Global {
    pub constant: bool,
    pub ty: String,
    pub array: bool,
    pub name: String,
    pub init: Option<Expr>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub ty: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    pub name: String,
    pub params: Vec<Param>,
    /// `None` for `returns nothing`.
    pub returns: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Native {
    pub constant: bool,
    pub sig: Signature,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Local {
    pub ty: String,
    pub array: bool,
    pub name: String,
    pub init: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub constant: bool,
    pub sig: Signature,
    pub locals: Vec<Local>,
    pub body: Vec<Stmt>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Set {
        name: String,
        index: Option<Expr>,
        value: Expr,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    If {
        branches: Vec<(Expr, Vec<Stmt>)>,
        otherwise: Vec<Stmt>,
    },
    Loop(Vec<Stmt>),
    ExitWhen(Expr),
    Return(Option<Expr>),
    /// `debug` prefix: the statement is skipped in release builds of the original game.
    Debug(Box<Stmt>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i32),
    Real(f32),
    Str(String),
    Bool(bool),
    Null,
    Var(String),
    Index(String, Box<Expr>),
    Call(String, Vec<Expr>),
    FuncRef(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
}
