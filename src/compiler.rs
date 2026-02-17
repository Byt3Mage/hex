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
mod tokens;

#[test]
fn test_all() {
    use crate::{
        arena::Interner,
        compiler::{
            ast::{AstArena, DeclKind},
            lexer::Lexer,
            parser::Parser,
            sema::{ScopeId, Sema},
        },
    };

    let source = include_str!("test.tks");

    let mut lexer = Lexer::new(source);
    let mut interner = Interner::new();
    let func_name = interner.get_or_intern("make_arr");
    let mut ast = AstArena::new();
    let mut parser = Parser::new(&mut lexer, &mut interner, &mut ast).unwrap();
    let module = parser.parse_source().unwrap();
    let mut sema = Sema::new(&ast, &interner);

    if let DeclKind::Module(decls) = &ast.decls[module].kind {
        sema.register_package(module, decls).unwrap();
        for &decl_id in decls {
            let decl = &ast.decls[decl_id];
            if let DeclKind::Function {
                generics,
                params,
                ret,
                body,
            } = &decl.kind
            {
                let scope = ScopeId::Decl(decl_id);
                sema.analyze_function(scope, decl_id, params, *ret, *body, decl.span)
                    .unwrap()
            }
        }
    }
}
