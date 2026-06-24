pub(crate) use hex_mir as ir;

mod ast;
mod error;
mod lexer;
mod parse_rules;
mod parser;
//mod resolver;
mod sema;
mod token;
mod typed_ast;

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error(transparent)]
    ParseError(#[from] parser::ParseError),

    #[error(transparent)]
    SemaError(#[from] sema::SemaError),
    #[error(transparent)]
    CodegenError(#[from] hex_mir::codegen::CodegenError),
}

#[test]
fn test_compile() -> Result<(), CompileError> {
    let src = include_str!("../main.tks");
    let mut intern = crate::arena::Interner::new();
    let mut ast = ast::Ast::new();
    let module = parser::parse(&mut ast, &mut intern, src)?;

    Ok(())
}
