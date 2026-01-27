mod ast;
mod ast_op;
mod error;
mod lexer;
mod name_resolver;
mod parse_rules;
mod parser;
pub mod sema;
pub mod sema_v2;
mod tokens;

#[test]
fn test_all() {
    use crate::{
        arena::Interner,
        compiler::{
            ast::{AstArena, DeclKind},
            lexer::Lexer,
            name_resolver::NameResolver,
            parser::Parser,
            sema::Sema,
        },
    };

    let source = include_str!("test.tks");

    let mut lexer = Lexer::new(source);
    let mut interner = Interner::new();
    let mut ast = AstArena::new();
    let mut parser = Parser::new(&mut lexer, &mut interner, &mut ast).unwrap();
    let module = parser.parse_source().unwrap();
    let resolver = NameResolver::new(&ast, &interner);

    if let DeclKind::Module(decls) = &ast.decls[module].kind {
        let (symbols, errors) = resolver.resolve(module, decls);

        for error in &errors {
            println!("Resolve Error: {error:?}")
        }

        if errors.is_empty() {
            let mut sema = Sema::new(&ast, &symbols, &interner);

            for &decl_id in decls {
                let decl = &ast.decls[decl_id];
                if let DeclKind::Function { .. } = &decl.kind {
                    sema.analyze_function(decl_id).unwrap()
                }
            }
        }
    }
}
