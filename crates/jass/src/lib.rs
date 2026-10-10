//! JASS (Warcraft III scripting language) front end: lexer and parser.
//! The interpreter/VM that must match the original's behaviour comes later.

pub mod ast;
pub mod lexer;
pub mod parser;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("line {line}: lex error: {msg}")]
    Lex { line: u32, msg: String },
    #[error("line {line}: parse error: {msg}")]
    Parse { line: u32, msg: String },
}

/// Parses a complete JASS source file (e.g. common.j, blizzard.j or a map's war3map.j).
pub fn parse(src: &str) -> Result<ast::Program, Error> {
    parser::Parser::new(lexer::lex(src)?).program()
}
