mod ast;
mod ast_op;
mod lexer;
mod name_resolver;
mod parse_rules;
mod parser;
mod sema;
mod sema_error;
mod tokens;
mod type_info;

#[test]
fn test_all() {
    use crate::{
        arena::Interner,
        compiler::{
            ast::{AstArena, ExprKind},
            lexer::Lexer,
            name_resolver::NameResolver,
            parser::Parser,
        },
    };

    let source = r#"
        const Point = struct {
            x: flt,
            ptr: @mut Point,
        };

        const Alias = Point;
    "#;

    let mut lexer = Lexer::new(source);
    let mut interner = Interner::new();
    let mut ast = AstArena::new();
    let mut parser = Parser::new(&mut lexer, &mut interner, &mut ast).unwrap();
    let module = parser.parse_source().unwrap();
    let resolver = NameResolver::new(&ast, &interner);

    if let ExprKind::ModuleType(decls) = &ast.exprs[module].kind {
        let (_, errors) = resolver.resolve(module, decls);

        for error in errors {
            println!("Resolve Error: {error:?}")
        }
    }
}
