use crate::compiler::{error::ResolveError, lexer::LexError, parser::ParseError};

mod ast;
mod codegen;
mod error;
mod lexer;
mod liveness;
mod mir;
mod op;
mod parse_rules;
mod parser;
pub mod sema;
mod symbol_table;
mod tokens;

#[derive(Debug, thiserror::Error)]
pub enum CompilerError {
    #[error(transparent)]
    LexerError(#[from] LexError),
    #[error(transparent)]
    ParseError(#[from] ParseError),
    #[error("Resolve Error: `{0:?}`")]
    ResolveError(#[from] ResolveError),
}

#[test]
fn test_all() -> Result<(), CompilerError> {
    use crate::{
        arena::Interner,
        compiler::{
            ast::{AstArena, DeclKind},
            lexer::Lexer,
            parser::Parser,
            sema::Sema,
        },
    };

    let source = include_str!("../test.tks");

    let mut lexer = Lexer::new(source);
    let mut interner = Interner::new();
    let mut ast = AstArena::new();
    let mut parser = Parser::new(&mut lexer, &mut interner, &mut ast)?;

    let root_module = parser.parse_source("test")?;

    let mut sema = Sema::new(&ast, &interner);
    sema.register_root_module(root_module)?;

    if let DeclKind::Module(decls) = &ast.decls[root_module].kind {
        for &decl_id in decls {
            let decl = &ast.decls[decl_id];
            if let DeclKind::Function {
                generics,
                params,
                ret,
                body,
            } = &decl.kind
            {}
        }
    }

    Ok(())
}
