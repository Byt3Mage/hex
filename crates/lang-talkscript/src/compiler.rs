use hex_vm::{IsValue, disassemble::disassemble};

use crate::compiler::{ast::DeclKind, lexer::LexError, parser::ParseError, sema::SemaError};

mod ast;
mod error;
mod lexer;
mod parse_rules;
mod parser;
pub mod sema;
mod token;

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error(transparent)]
    LexerError(#[from] LexError),
    #[error(transparent)]
    ParseError(#[from] ParseError),
    #[error(transparent)]
    SemaError(#[from] SemaError),
    #[error("mir codegen error: {0}")]
    MirCodegenError(#[from] hex_mir::MirError),
}

#[test]
fn test_compile() -> Result<(), CompileError> {
    let src = include_str!("../test.tks");

    let mut intern = crate::arena::Interner::new();
    let mut ast = ast::Ast::new();
    let mut lexer = lexer::Lexer::new(&src);
    let mut parser = parser::Parser::new(&mut lexer, &mut intern, &mut ast)?;
    let mod_id = parser.parse_source("test")?;
    let mod_decl = ast.decl(mod_id);

    let DeclKind::Mod(decls) = &mod_decl.kind else {
        panic!("expected module");
    };

    let mut module = hex_mir::Module {
        name: "test".into(),
        functions: vec![],
    };

    for &decl_id in decls {
        let decl = ast.decl(decl_id);

        if let DeclKind::Func { params, ret, body } = &decl.kind {
            let name = intern.resolve(decl.name).expect("unknown function name");
            let func = sema::lower_function(&ast, name, params, *ret, *body)?;
            module.functions.push(func);
        }
    }

    let program = hex_mir::lowering::emit_program(&module)?;

    disassemble(&program.instructions, &program.constants);

    let mut vm = hex_vm::VM::new(&program, hex_vm::extensions::NoExtensions);
    let args = &[
        i64::into_value(5),
        i64::into_value(56),
        i64::into_value(590),
    ];
    let res = vm.execute(0, args).unwrap();

    println!(
        "result is: [{}, {}, {}, {}]",
        i64::from_value(res[0]),
        i64::from_value(res[1]),
        i64::from_value(res[2]),
        i64::from_value(res[3]),
    );

    Ok(())
}
