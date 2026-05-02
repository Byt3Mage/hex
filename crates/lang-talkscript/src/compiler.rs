use crate::compiler::{lexer::LexError, parser::ParseError};

mod ast;
mod error;
mod lexer;
mod parse_rules;
mod parser;
pub mod sema;
//mod symbol_table;
mod tokens;

#[derive(Debug, thiserror::Error)]
pub enum CompilerError {
    #[error(transparent)]
    LexerError(#[from] LexError),
    #[error(transparent)]
    ParseError(#[from] ParseError),
}
