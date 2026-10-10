//! JASS (Warcraft III scripting language): lexer, parser, type checker, bytecode compiler and VM.
//! The VM's behaviour is modelled on docs/spec/jass-vm.md; see the UNVERIFIED notes in each module.

pub mod ast;
pub mod bytecode;
pub mod check;
pub mod compile;
pub mod lexer;
pub mod natives;
pub mod parser;
pub mod vm;

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
