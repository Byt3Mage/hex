use simple_ternary::tnr;

use super::{
    ast::{AstArena, Expr, ExprKind, Param, Stmt, StmtKind},
    lexer::{LexError, Lexer},
    op::{AssignOp, BinOp, UnOp},
    parse_rules::{InfixRule, ParseRule, Precedence, PrefixRule},
    tokens::{Span, Token, TokenType},
};

use crate::{
    arena::Interner,
    compiler::{
        ast::{
            AstType, AstTypeKind, Decl, DeclId, DeclKind, FieldDef, FieldInit, Path, PathSegment,
            Pattern, PatternKind, VariantDef, Visibility,
        },
        sema::sema_value::ComptimeInt,
    },
    tt,
};

#[derive(Debug)]
pub struct ParseError {
    msg: String,
    span: Span,
}

type Result<T> = std::result::Result<T, ParseError>;

pub struct Parser<'a> {
    lexer: &'a mut Lexer<'a>,
    interner: &'a mut Interner,
    ast: &'a mut AstArena,
    current: Token<'a>,
}

impl<'a> Parser<'a> {
    pub fn new(
        lexer: &'a mut Lexer<'a>,
        interner: &'a mut Interner,
        ast: &'a mut AstArena,
    ) -> std::result::Result<Parser<'a>, LexError> {
        let current = lexer.next_token()?;

        Ok(Parser {
            interner,
            lexer,
            current,
            ast,
        })
    }

    pub fn advance(&mut self) -> Result<()> {
        match self.lexer.next_token() {
            Ok(token) => {
                self.current = token;
                Ok(())
            }
            Err(err) => Err(ParseError {
                msg: format!("Lexer error: {:?}", err),
                span: err.span,
            }),
        }
    }

    fn advance_if(&mut self, ty: TokenType) -> Result<bool> {
        if self.current.ty == ty {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn consume(&mut self) -> Result<Token<'a>> {
        let token = self.current.clone();
        self.advance()?;
        Ok(token)
    }

    #[inline(always)]
    fn check(&self, ty: TokenType) -> bool {
        self.current.ty == ty
    }

    fn expect(&mut self, ty: TokenType) -> Result<Token<'a>> {
        if self.current.ty == ty {
            self.consume()
        } else {
            Err(ParseError {
                msg: format!("Expected {:?}, found {:?}", ty, self.current.ty),
                span: self.current.span,
            })
        }
    }

    pub fn parse_source(&mut self) -> Result<DeclId> {
        let mut items = vec![];

        while !self.check(tt![eof]) {
            let decl = self.parse_decl()?;
            items.push(self.ast.decls.insert(decl));
        }

        let module = Decl {
            visibility: Visibility::Public,
            name: self.interner.get_or_intern("<package>"),
            kind: DeclKind::Module(items),
            span: Span::default().merge(self.current.span),
        };

        Ok(self.ast.decls.insert(module))
    }

    fn parse_decl(&mut self) -> Result<Decl> {
        let vis = tnr! { self.advance_if(tt![pub])? => Visibility::Public : Visibility::Private};

        match self.current.ty {
            tt![mod] => self.parse_mod_decl(vis),
            tt![fn] => self.parse_func_decl(vis),
            tt![const] => self.parse_const_decl(vis),
            tt![struct] => self.parse_struct_decl(vis),
            tt![union] => self.parse_union_decl(vis),
            tt![enum] => self.parse_enum_decl(vis),
            tt => Err(ParseError {
                msg: format!("Expected declaration, found {tt:?}"),
                span: self.current.span,
            }),
        }
    }

    fn parse_mod_decl(&mut self, visibility: Visibility) -> Result<Decl> {
        let mod_token = self.expect(tt![mod])?;
        let mod_name = self.expect(tt![ident])?;

        self.expect(tt!['{'])?;

        let mut decls = vec![];

        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let item = self.parse_decl()?;
            decls.push(self.ast.decls.insert(item));
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Decl {
            visibility,
            name: self.interner.get_or_intern(mod_name.lexeme),
            kind: DeclKind::Module(decls),
            span: mod_token.span.merge(r_brace.span),
        })
    }

    fn parse_func_decl(&mut self, visibility: Visibility) -> Result<Decl> {
        let fn_token = self.expect(tt![fn])?;
        let func_name = self.expect(tt![ident])?;

        self.expect(tt!['('])?;

        let mut params = vec![];
        while !matches!(self.current.ty, tt![')'] | tt![eof]) {
            let pattern = self.parse_pattern()?;
            self.expect(tt![:])?;
            let ty = self.parse_type()?;

            params.push(Param {
                span: pattern.span.merge(ty.span),
                pattern: self.ast.patterns.insert(pattern),
                ty: self.ast.types.insert(ty),
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        self.expect(tt![')'])?;

        let ret = if self.advance_if(tt![:])? {
            let ret = self.parse_type()?;
            Some(self.ast.types.insert(ret))
        } else {
            None
        };

        let body = self.parse_block()?;

        Ok(Decl {
            visibility,
            name: self.interner.get_or_intern(func_name.lexeme),
            span: fn_token.span.merge(body.span),
            kind: DeclKind::Function {
                generics: vec![],
                params,
                ret,
                body: self.ast.exprs.insert(body),
            },
        })
    }

    fn parse_const_decl(&mut self, visibility: Visibility) -> Result<Decl> {
        let const_token = self.expect(tt![const])?;
        let const_name = self.expect(tt![ident])?;

        let ty = if self.advance_if(tt![:])? {
            let ty = self.parse_type()?;
            Some(self.ast.types.insert(ty))
        } else {
            None
        };

        self.expect(tt![=])?;

        let value = self.parse_expr()?;
        let semi = self.expect(tt![;])?;

        Ok(Decl {
            visibility,
            name: self.interner.get_or_intern(const_name.lexeme),
            kind: DeclKind::Const {
                ty,
                value: self.ast.exprs.insert(value),
            },
            span: const_token.span.merge(semi.span),
        })
    }

    fn parse_struct_decl(&mut self, visibility: Visibility) -> Result<Decl> {
        let struct_token = self.expect(tt![struct])?;
        let struct_name = self.expect(tt![ident])?;

        // TODO: parse generics

        self.expect(tt!['{'])?;

        let mut fields = vec![];
        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let vis = tnr! {self.advance_if(tt![pub])? => Visibility::Public : Visibility::Private};
            let field_name = self.expect(tt![ident])?;
            self.expect(tt![:])?;
            let field_ty = self.parse_type()?;

            fields.push(FieldDef {
                visibility: vis,
                span: field_name.span.merge(field_ty.span),
                name: self.interner.get_or_intern(field_name.lexeme),
                ty: self.ast.types.insert(field_ty),
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Decl {
            visibility,
            name: self.interner.get_or_intern(struct_name.lexeme),
            kind: DeclKind::Struct {
                generics: vec![],
                fields,
            },
            span: struct_token.span.merge(r_brace.span),
        })
    }

    fn parse_union_decl(&mut self, visibility: Visibility) -> Result<Decl> {
        let union_token = self.expect(tt![union])?;
        let union_name = self.expect(tt![ident])?;

        self.expect(tt!['{'])?;

        let mut fields = vec![];
        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let vis = tnr! {self.advance_if(tt![pub])? => Visibility::Public : Visibility::Private};
            let field_name = self.expect(tt![ident])?;
            self.expect(tt![:])?;
            let field_ty = self.parse_type()?;

            fields.push(FieldDef {
                visibility: vis,
                span: field_name.span.merge(field_ty.span),
                name: self.interner.get_or_intern(field_name.lexeme),
                ty: self.ast.types.insert(field_ty),
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Decl {
            visibility,
            name: self.interner.get_or_intern(union_name.lexeme),
            kind: DeclKind::Union {
                generics: vec![],
                fields,
            },
            span: union_token.span.merge(r_brace.span),
        })
    }

    fn parse_enum_decl(&mut self, visibility: Visibility) -> Result<Decl> {
        let enum_token = self.expect(tt![enum])?;
        let enum_name = self.expect(tt![ident])?;
        let mut base = None;

        if self.advance_if(tt!['('])? {
            let ty = self.parse_type()?;
            base = Some(self.ast.types.insert(ty));
            self.expect(tt![')'])?;
        }

        self.expect(tt!['{'])?;

        let mut variants = vec![];
        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let variant_name = self.expect(tt![ident])?;
            let mut span = variant_name.span;
            let value = if self.advance_if(tt![=])? {
                let val = self.parse_expr()?;
                span = span.merge(val.span);
                Some(self.ast.exprs.insert(val))
            } else {
                None
            };

            variants.push(VariantDef {
                name: self.interner.get_or_intern(variant_name.lexeme),
                value,
                span,
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Decl {
            visibility,
            name: self.interner.get_or_intern(enum_name.lexeme),
            kind: DeclKind::Enum { base, variants },
            span: enum_token.span.merge(r_brace.span),
        })
    }

    fn parse_type(&mut self) -> Result<AstType> {
        let ty_token = self.consume()?;
        let mut span = ty_token.span;

        let kind = match ty_token.ty {
            tt![cint] => AstTypeKind::CInt,
            tt![cstr] => AstTypeKind::CStr,
            tt![int] => AstTypeKind::Int,
            tt![uint] => AstTypeKind::Uint,
            tt![float] => AstTypeKind::Float,
            tt![bool] => AstTypeKind::Bool,
            tt![char] => AstTypeKind::Char,
            tt![str] => AstTypeKind::Str,
            tt![void] => AstTypeKind::Void,
            tt![!] => AstTypeKind::Never,
            tt![ident] => {
                let path = self.parse_path_w_first(ty_token)?;
                span = path.span;
                AstTypeKind::Path(self.ast.paths.insert(path))
            }
            tt![&] => {
                let mutable = self.advance_if(tt![mut])?;
                let pointee = self.parse_type()?;
                span = span.merge(pointee.span);
                AstTypeKind::Pointer {
                    mutable,
                    pointee: self.ast.types.insert(pointee),
                }
            }
            tt![?] => {
                let inner = self.parse_type()?;
                span = span.merge(inner.span);
                AstTypeKind::Optional(self.ast.types.insert(inner))
            }
            tt!['['] => {
                let first = self.parse_type()?;
                let kind = if self.advance_if(tt![;])? {
                    let len = self.parse_expr()?;
                    AstTypeKind::Array {
                        elem: self.ast.types.insert(first),
                        len: self.ast.exprs.insert(len),
                    }
                } else if self.advance_if(tt![,])? {
                    let mut elems = vec![self.ast.types.insert(first)];
                    while !matches!(self.current.ty, tt![']'] | tt![eof]) {
                        let ty = self.parse_type()?;
                        elems.push(self.ast.types.insert(ty));
                        if !self.advance_if(tt![,])? {
                            break;
                        }
                    }
                    AstTypeKind::Tuple(elems)
                } else {
                    AstTypeKind::Slice(self.ast.types.insert(first))
                };
                span = span.merge(self.expect(tt![']'])?.span);
                kind
            }
            tt![fn] => {
                let mut params = vec![];
                let mut ret = None;

                self.expect(tt!['('])?;

                while !matches!(self.current.ty, tt![')'] | tt![eof]) {
                    let param = self.parse_type()?;
                    params.push(self.ast.types.insert(param));
                    if !self.advance_if(tt![,])? {
                        break;
                    }
                }

                span = span.merge(self.expect(tt![')'])?.span);

                if self.advance_if(tt![->])? {
                    let ret_ty = self.parse_type()?;
                    span = span.merge(ret_ty.span);
                    ret = Some(self.ast.types.insert(ret_ty));
                }

                AstTypeKind::Function { params, ret }
            }
            tt => {
                return Err(ParseError {
                    msg: format!("expected type, found: {tt:?}"),
                    span,
                });
            }
        };

        Ok(AstType { kind, span })
    }

    fn parse_path(&mut self) -> Result<Path> {
        let first = self.parse_path_segment()?;
        let mut span = first.span;

        let mut rest = vec![];
        while self.advance_if(tt![::])? {
            let segment = self.parse_path_segment()?;
            span = span.merge(segment.span);
            rest.push(segment);
        }

        Ok(Path { first, rest, span })
    }

    fn parse_path_w_first(&mut self, first: Token) -> Result<Path> {
        let first = PathSegment {
            name: self.interner.get_or_intern(first.lexeme),
            generics: vec![], // TODO: generics
            span: first.span,
        };

        let mut span = first.span;

        let mut rest = vec![];
        while self.advance_if(tt![::])? {
            let segment = self.parse_path_segment()?;
            span = span.merge(segment.span);
            rest.push(segment);
        }

        Ok(Path { first, rest, span })
    }

    fn parse_path_segment(&mut self) -> Result<PathSegment> {
        let name_token = self.expect(tt![ident])?;
        let span = name_token.span;

        // TODO: generics

        Ok(PathSegment {
            name: self.interner.get_or_intern(name_token.lexeme),
            generics: vec![],
            span,
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        let mutable = self.advance_if(tt![mut])?;
        let name_token = self.expect(tt![ident])?;

        Ok(Pattern {
            kind: PatternKind::Identifier {
                mutable,
                name: self.interner.get_or_intern(name_token.lexeme),
            },
            span: name_token.span,
        })
    }

    fn parse_let(&mut self) -> Result<Stmt> {
        let let_token = self.expect(tt![let])?;
        let pat = self.parse_pattern()?;
        let ty = tnr! {self.advance_if(tt![:])? => Some(self.parse_type()?) : None};

        self.expect(tt![=])?;

        let value = self.parse_expr()?;
        let semi_token = self.expect(tt![;])?;

        Ok(Stmt {
            span: let_token.span.merge(semi_token.span),
            kind: StmtKind::Let {
                pattern: self.ast.patterns.insert(pat),
                ty: ty.map(|t| self.ast.types.insert(t)),
                value: self.ast.exprs.insert(value),
            },
        })
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_precedence(Precedence::Assignment, true)
    }

    fn parse_expr_no_struct(&mut self) -> Result<Expr> {
        self.parse_precedence(Precedence::Assignment, false)
    }

    fn parse_precedence(&mut self, precedence: Precedence, allow_struct: bool) -> Result<Expr> {
        let rule = ParseRule::get(self.current.ty);
        let mut expr = self.parse_prefix(rule.prefix, allow_struct)?;

        loop {
            let infix_rule = ParseRule::get(self.current.ty);

            if precedence <= infix_rule.prec && infix_rule.infix != InfixRule::None {
                expr = self.parse_infix(infix_rule.infix, expr, allow_struct)?;
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_prefix(&mut self, rule: PrefixRule, allow_struct: bool) -> Result<Expr> {
        match rule {
            PrefixRule::LiteralCint => self.parse_cint(),
            PrefixRule::LiteralUint => self.parse_uint(),
            PrefixRule::LiteralInt => self.parse_int(),
            PrefixRule::LiteralFloat => self.parse_float(),
            PrefixRule::LiteralString => self.parse_string(),
            PrefixRule::True => self.parse_true(),
            PrefixRule::False => self.parse_false(),
            PrefixRule::LiteralNull => self.parse_null(),
            PrefixRule::LiteralVoid => self.parse_void(),
            PrefixRule::LiteralArray => self.parse_array(),
            PrefixRule::Identifier => self.parse_path_expr(allow_struct),
            PrefixRule::Grouping => self.parse_group(),
            PrefixRule::Unary => self.parse_unary(allow_struct),
            PrefixRule::If => self.parse_if(),
            PrefixRule::Block => self.parse_block(),
            PrefixRule::While => self.parse_while(),
            PrefixRule::Loop => self.parse_loop(),
            PrefixRule::Match => todo!("parse match"),
            PrefixRule::Return => self.parse_return(),
            PrefixRule::Break => self.parse_break(),
            PrefixRule::Continue => self.parse_continue(),
            PrefixRule::LiteralStruct => todo!(),

            PrefixRule::None => Err(ParseError {
                msg: format!("Expected expression, found {:?}", self.current.ty),
                span: self.current.span,
            }),
        }
    }

    fn parse_infix(&mut self, rule: InfixRule, left: Expr, allow_struct: bool) -> Result<Expr> {
        match rule {
            InfixRule::Binary => self.parse_binary(left, allow_struct),
            InfixRule::Assign => self.parse_assign(left, allow_struct),
            InfixRule::Call => self.parse_call(left),
            InfixRule::Dot => self.parse_dot(left),
            InfixRule::Index => self.parse_index(left),
            InfixRule::None => Err(ParseError {
                msg: format!("Expected expression, found {:?}", self.current.ty),
                span: self.current.span,
            }),
        }
    }

    fn parse_path_expr(&mut self, allow_struct: bool) -> Result<Expr> {
        let path = self.parse_path()?;
        let span = path.span;
        let path = self.ast.paths.insert(path);

        if allow_struct && self.check(tt!['{']) {
            return self.parse_struct_lit(AstType {
                kind: AstTypeKind::Path(path),
                span,
            });
        }

        Ok(Expr {
            kind: ExprKind::Path(path),
            span,
        })
    }

    fn parse_struct_lit(&mut self, ty: AstType) -> Result<Expr> {
        self.expect(tt!['{'])?;

        let mut fields = vec![];
        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let name = self.expect(tt![ident])?;
            self.expect(tt![:])?;
            let value = self.parse_expr()?;

            fields.push(FieldInit {
                span: name.span.merge(value.span),
                name: self.interner.get_or_intern(name.lexeme),
                value: self.ast.exprs.insert(value),
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Expr {
            span: ty.span.merge(r_brace.span),
            kind: ExprKind::StructLit {
                ty: self.ast.types.insert(ty),
                fields,
            },
        })
    }

    fn parse_cint(&mut self) -> Result<Expr> {
        let int_token = self.expect(tt![cint_lit])?;

        match str::parse::<u64>(int_token.lexeme) {
            Ok(u) => Ok(Expr {
                kind: ExprKind::CintLit(ComptimeInt::unsigned(u)),
                span: int_token.span,
            }),
            Err(err) => Err(ParseError {
                msg: format!("Error parsing int literal: {err}"),
                span: int_token.span,
            }),
        }
    }

    fn parse_uint(&mut self) -> Result<Expr> {
        let int_token = self.expect(tt![uint_lit])?;

        match str::parse::<u64>(int_token.lexeme) {
            Ok(u) => Ok(Expr {
                kind: ExprKind::UintLit(u),
                span: int_token.span,
            }),
            Err(err) => Err(ParseError {
                msg: format!("Error parsing int literal: {err}"),
                span: int_token.span,
            }),
        }
    }

    fn parse_int(&mut self) -> Result<Expr> {
        let int_token = self.expect(tt![int_lit])?;

        match str::parse::<i64>(int_token.lexeme) {
            Ok(i) => Ok(Expr {
                kind: ExprKind::IntLit(i),
                span: int_token.span,
            }),
            Err(err) => Err(ParseError {
                msg: format!("Error parsing int literal: {err}"),
                span: int_token.span,
            }),
        }
    }

    fn parse_float(&mut self) -> Result<Expr> {
        let float_token = self.expect(tt![float_lit])?;

        match str::parse::<f64>(float_token.lexeme) {
            Ok(f) => Ok(Expr {
                kind: ExprKind::FloatLit(f),
                span: float_token.span,
            }),
            Err(err) => Err(ParseError {
                msg: format!("Error parsing float literal: {err}"),
                span: float_token.span,
            }),
        }
    }

    fn parse_string(&mut self) -> Result<Expr> {
        let str_token = self.expect(tt![str_lit])?;

        Ok(Expr {
            kind: ExprKind::StrLit(self.interner.get_or_intern(str_token.lexeme)),
            span: str_token.span,
        })
    }

    fn parse_true(&mut self) -> Result<Expr> {
        let true_token = self.expect(tt![true])?;

        Ok(Expr {
            kind: ExprKind::True,
            span: true_token.span,
        })
    }

    fn parse_false(&mut self) -> Result<Expr> {
        let false_token = self.expect(tt![false])?;

        Ok(Expr {
            kind: ExprKind::False,
            span: false_token.span,
        })
    }

    fn parse_null(&mut self) -> Result<Expr> {
        let null_token = self.expect(tt![null])?;

        Ok(Expr {
            kind: ExprKind::Null,
            span: null_token.span,
        })
    }

    fn parse_void(&mut self) -> Result<Expr> {
        let void_token = self.expect(tt![void])?;

        Ok(Expr {
            kind: ExprKind::Void,
            span: void_token.span,
        })
    }

    fn parse_array(&mut self) -> Result<Expr> {
        let l_brkt = self.expect(tt!['['])?;

        // Parse empty array literal
        if self.advance_if(tt![']'])? {
            let r_brkt = self.expect(tt![']'])?;

            return Ok(Expr {
                kind: ExprKind::ArrayLit(vec![]),
                span: l_brkt.span.merge(r_brkt.span),
            });
        }

        let first = self.parse_expr()?;

        // Parse array repeat literal
        if self.advance_if(tt![;])? {
            let len = self.parse_expr()?;
            let r_brkt = self.expect(tt![']'])?;

            let value = self.ast.exprs.insert(first);
            let count = self.ast.exprs.insert(len);

            return Ok(Expr {
                kind: ExprKind::ArrayRepeat { value, count },
                span: l_brkt.span.merge(r_brkt.span),
            });
        }

        let mut elems = vec![self.ast.exprs.insert(first)];

        if self.advance_if(tt![,])? {
            while !matches!(self.current.ty, tt![']'] | tt![eof]) {
                let elem = self.parse_expr()?;
                elems.push(self.ast.exprs.insert(elem));

                if !self.advance_if(tt![,])? {
                    break;
                }
            }
        }

        let r_brkt = self.expect(tt![']'])?;

        Ok(Expr {
            kind: ExprKind::ArrayLit(elems),
            span: l_brkt.span.merge(r_brkt.span),
        })
    }

    fn parse_group(&mut self) -> Result<Expr> {
        let open = self.expect(tt!['('])?;
        let expr = self.parse_expr()?;
        let close = self.expect(tt![')'])?;

        Ok(Expr {
            kind: ExprKind::Group(self.ast.exprs.insert(expr)),
            span: open.span.merge(close.span),
        })
    }

    fn parse_unary(&mut self, allow_struct: bool) -> Result<Expr> {
        let op_token = self.consume()?;

        let op = match op_token.ty {
            tt![-] => UnOp::Neg,
            tt![!] => UnOp::Not,
            _ => {
                return Err(ParseError {
                    msg: format!("Unsupported unary operator: {:?}", op_token.ty),
                    span: op_token.span,
                });
            }
        };

        let expr = self.parse_precedence(Precedence::Unary, allow_struct)?;

        Ok(Expr {
            span: op_token.span.merge(expr.span),
            kind: ExprKind::Unary {
                op,
                rhs: self.ast.exprs.insert(expr),
            },
        })
    }

    fn parse_binary(&mut self, lhs: Expr, allow_struct: bool) -> Result<Expr> {
        let op_token = self.consume()?;
        let op = match op_token.ty {
            tt![+] => BinOp::Add,
            tt![-] => BinOp::Sub,
            tt![*] => BinOp::Mul,
            tt![/] => BinOp::Div,
            tt![%] => BinOp::Mod,
            tt![&] => BinOp::BitAnd,
            tt![|] => BinOp::BitOr,
            tt![^] => BinOp::BitXor,
            tt![<<] => BinOp::Shl,
            tt![>>] => BinOp::Shr,
            tt![==] => BinOp::Eq,
            tt![!=] => BinOp::Ne,
            tt![<] => BinOp::Lt,
            tt![<=] => BinOp::Le,
            tt![>] => BinOp::Gt,
            tt![>=] => BinOp::Ge,
            tt![?:] => BinOp::NullCoalesce,
            _ => {
                return Err(ParseError {
                    msg: format!("Unsupported binary operator: {:?}", op_token.ty),
                    span: op_token.span,
                });
            }
        };

        let rule = ParseRule::get(op_token.ty);
        let rhs = self.parse_precedence(rule.rhs_prec(), allow_struct)?;

        Ok(Expr {
            span: lhs.span.merge(rhs.span),
            kind: ExprKind::Binary {
                op,
                lhs: self.ast.exprs.insert(lhs),
                rhs: self.ast.exprs.insert(rhs),
            },
        })
    }

    fn parse_assign(&mut self, tgt: Expr, allow_struct: bool) -> Result<Expr> {
        let op_token = self.consume()?;
        let op = match op_token.ty {
            tt![=] => AssignOp::Assign,
            tt![+=] => AssignOp::AddAssign,
            tt![-=] => AssignOp::SubAssign,
            tt![*=] => AssignOp::MulAssign,
            tt![/=] => AssignOp::DivAssign,
            tt![%=] => AssignOp::ModAssign,
            _ => {
                return Err(ParseError {
                    msg: format!("Invalid assignment operator: {:?}", op_token.ty),
                    span: op_token.span,
                });
            }
        };

        let val = self.parse_precedence(Precedence::Assignment, allow_struct)?;

        Ok(Expr {
            span: tgt.span.merge(val.span),
            kind: ExprKind::Assign {
                op,
                tgt: self.ast.exprs.insert(tgt),
                val: self.ast.exprs.insert(val),
            },
        })
    }

    fn parse_block(&mut self) -> Result<Expr> {
        let l_brace = self.expect(tt!['{'])?;
        let mut stmts = vec![];

        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            match self.current.ty {
                tt![;] => {
                    //consume leading/trailing semicolons
                    self.consume()?;
                }

                tt![let] => {
                    let let_stmt = self.parse_let()?;
                    stmts.push(self.ast.stmts.insert(let_stmt));
                }

                // Parse expression statements
                tt => {
                    // check if current token can start a block expression
                    let is_blk = matches!(
                        tt,
                        tt!['{'] | tt![if] | tt![match] | tt![while] | tt![for] | tt![loop]
                    );

                    let expr = self.parse_expr()?;
                    let span = expr.span;

                    if self.advance_if(tt![;])? {
                        let kind = StmtKind::Semi(self.ast.exprs.insert(expr));
                        stmts.push(self.ast.stmts.insert(Stmt { span, kind }));
                    } else {
                        let kind = StmtKind::Expr(self.ast.exprs.insert(expr));
                        stmts.push(self.ast.stmts.insert(Stmt { span, kind }));

                        // Expressions without block and without trailing semicolon
                        // should be the last statement in a block.
                        if !is_blk {
                            break;
                        }
                    }
                }
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Expr {
            kind: ExprKind::Block(stmts),
            span: l_brace.span.merge(r_brace.span),
        })
    }

    fn parse_if(&mut self) -> Result<Expr> {
        let if_token = self.expect(tt![if])?;

        let cond = self.parse_expr_no_struct()?;
        let then_branch = self.parse_block()?;

        let (else_branch, span) = if self.advance_if(tt![else])? {
            if self.check(tt![if]) {
                let expr = self.parse_if()?;
                let span = if_token.span.merge(expr.span);
                (Some(self.ast.exprs.insert(expr)), span)
            } else {
                let expr = self.parse_block()?;
                let span = if_token.span.merge(expr.span);
                (Some(self.ast.exprs.insert(expr)), span)
            }
        } else {
            (None, if_token.span.merge(then_branch.span))
        };

        Ok(Expr {
            kind: ExprKind::If {
                cond: self.ast.exprs.insert(cond),
                then_branch: self.ast.exprs.insert(then_branch),
                else_branch: else_branch,
            },
            span,
        })
    }

    fn parse_while(&mut self) -> Result<Expr> {
        let while_token = self.expect(tt![while])?;
        let cond = self.parse_expr_no_struct()?;
        let body = self.parse_block()?;

        Ok(Expr {
            span: while_token.span.merge(body.span),
            kind: ExprKind::While {
                cond: self.ast.exprs.insert(cond),
                body: self.ast.exprs.insert(body),
            },
        })
    }

    fn parse_loop(&mut self) -> Result<Expr> {
        let loop_token = self.expect(tt![loop])?;
        let body = self.parse_block()?;

        Ok(Expr {
            span: loop_token.span.merge(body.span),
            kind: ExprKind::Loop(self.ast.exprs.insert(body)),
        })
    }

    fn can_start_expr(&self) -> bool {
        ParseRule::get(self.current.ty).prefix != PrefixRule::None
    }

    fn parse_return(&mut self) -> Result<Expr> {
        let return_token = self.expect(tt![return])?;
        let mut span = return_token.span;

        let expr = if self.can_start_expr() {
            let expr = self.parse_expr()?;
            span = span.merge(expr.span);
            Some(self.ast.exprs.insert(expr))
        } else {
            None
        };

        Ok(Expr {
            kind: ExprKind::Return(expr),
            span,
        })
    }

    fn parse_break(&mut self) -> Result<Expr> {
        let break_token = self.expect(tt![break])?;
        let mut span = break_token.span;

        let expr = if self.can_start_expr() {
            let expr = self.parse_expr()?;
            span = span.merge(expr.span);
            Some(self.ast.exprs.insert(expr))
        } else {
            None
        };

        Ok(Expr {
            kind: ExprKind::Break(expr),
            span,
        })
    }

    fn parse_continue(&mut self) -> Result<Expr> {
        let continue_token = self.expect(tt![continue])?;

        Ok(Expr {
            kind: ExprKind::Continue,
            span: continue_token.span,
        })
    }

    fn parse_call(&mut self, callee: Expr) -> Result<Expr> {
        self.expect(tt!['('])?;

        let mut args = vec![];
        while !matches!(self.current.ty, tt![')'] | tt![eof]) {
            let arg = self.parse_expr()?;
            args.push(self.ast.exprs.insert(arg));

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_paren = self.expect(tt![')'])?;

        Ok(Expr {
            span: callee.span.merge(r_paren.span),
            kind: ExprKind::Call {
                callee: self.ast.exprs.insert(callee),
                args,
            },
        })
    }

    fn parse_dot(&mut self, object: Expr) -> Result<Expr> {
        self.expect(tt![.])?;
        let field = self.expect(tt![ident])?;

        Ok(Expr {
            span: object.span.merge(field.span),
            kind: ExprKind::Field {
                object: self.ast.exprs.insert(object),
                field: self.interner.get_or_intern(field.lexeme),
            },
        })
    }

    fn parse_index(&mut self, object: Expr) -> Result<Expr> {
        self.expect(tt!['['])?;
        let index = self.parse_expr()?;
        let r_brkt = self.expect(tt![']'])?;

        Ok(Expr {
            span: object.span.merge(r_brkt.span),
            kind: ExprKind::Index {
                object: self.ast.exprs.insert(object),
                index: self.ast.exprs.insert(index),
            },
        })
    }
}
