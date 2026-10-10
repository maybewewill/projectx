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
///
/// Real literals here are correctly rounded only; use [`parse_with`] with the lookup tables to
/// get the original's bits (see [`lexer::lex_with`]).
pub fn parse(src: &str) -> Result<ast::Program, Error> {
    parse_with(src, None)
}

/// Parses a JASS source file with the `softfloat` lookup tables of the user's `Game.dll`, so real
/// literals get the original's soft-float value (e.g. `0.1` is `0x3DCCCCCE`).
pub fn parse_with(src: &str, tables: Option<&softfloat::Tables>) -> Result<ast::Program, Error> {
    parser::Parser::new(lexer::lex_with(src, tables)?).program()
}
