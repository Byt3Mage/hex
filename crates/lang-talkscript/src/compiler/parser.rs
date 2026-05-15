use simple_ternary::tnr;

use crate::{
    arena::Interner,
    compiler::{
        ast::{
            AssignOp, Ast, BinOp, Decl, DeclId, DeclKind, Expr, ExprKind, Param, Stmt, StmtKind,
            UnOp, Visibility,
        },
        lexer::{Lexer, LexerError},
        parse_rules::{InfixRule, ParseRule, Precedence, PrefixRule},
        token::{Span, Token, TokenType},
    },
    tt,
};

#[derive(Debug, thiserror::Error)]
#[error("Parse Error: `{msg}` at {span:?}")]
pub struct ParseError {
    msg: String,
    span: Span,
}

impl From<LexerError> for ParseError {
    fn from(err: LexerError) -> Self {
        Self {
            msg: err.to_string(),
            span: err.span,
        }
    }
}

type ParseResult<T> = std::result::Result<T, ParseError>;

pub struct Parser<'a> {
    ast: &'a mut Ast,
    intern: &'a mut Interner,
    lexer: Lexer<'a>,
    current: Token<'a>,
}

impl<'a> Parser<'a> {
    pub fn new(
        ast: &'a mut Ast,
        intern: &'a mut Interner,
        source: &'a str,
    ) -> Result<Parser<'a>, ParseError> {
        let mut lexer = Lexer::new(source);
        let current = lexer.next_token()?;

        Ok(Parser {
            ast,
            intern,
            lexer,
            current,
        })
    }

    pub fn advance(&mut self) -> ParseResult<()> {
        Ok(self.current = self.lexer.next_token()?)
    }

    fn advance_if(&mut self, ty: TokenType) -> ParseResult<bool> {
        Ok(if self.current.ty == ty {
            self.advance()?;
            true
        } else {
            false
        })
    }

    fn consume(&mut self) -> ParseResult<Token<'a>> {
        let token = self.current.clone();
        self.advance()?;
        Ok(token)
    }

    #[inline(always)]
    fn check(&self, ty: TokenType) -> bool {
        self.current.ty == ty
    }

    fn expect(&mut self, ty: TokenType) -> ParseResult<Token<'a>> {
        if self.current.ty == ty {
            self.consume()
        } else {
            Err(ParseError {
                msg: format!("Expected {:?}, found {:?}", ty, self.current.ty),
                span: self.current.span,
            })
        }
    }

    /// Parse a source file, returning the ID of the module declaration.
    ///
    /// Each source file corresponds to a single module declaration.
    pub fn parse(&mut self) -> ParseResult<DeclId> {
        let span = self.current.span;

        let mut decls = vec![];
        while !self.check(tt![eof]) {
            let decl = self.parse_decl()?;
            decls.push(self.ast.insert_decl(decl));
        }

        let module = Decl {
            vis: Visibility::Public,
            name: self.intern.get_or_intern("crate"),
            kind: DeclKind::Mod(decls),
            span: span.merge(self.current.span),
        };

        Ok(self.ast.insert_decl(module))
    }

    fn parse_decl(&mut self) -> ParseResult<Decl> {
        let vis = self.parse_visibility()?;

        match self.current.ty {
            tt![mod] => self.parse_mod_decl(vis),
            tt![fn] => self.parse_func_decl(vis),
            tt![const] => self.parse_const_decl(vis),
            //tt![struct] => self.parse_struct_decl(vis),
            //tt![union] => self.parse_union_decl(vis),
            //tt![enum] => self.parse_enum_decl(vis),
            tt => Err(ParseError {
                msg: format!("Expected declaration, found {tt:?}"),
                span: self.current.span,
            }),
        }
    }

    fn parse_visibility(&mut self) -> ParseResult<Visibility> {
        Ok(tnr! { self.advance_if(tt![pub])? => Visibility::Public : Visibility::Private})
    }

    fn parse_mod_decl(&mut self, vis: Visibility) -> ParseResult<Decl> {
        let mod_token = self.expect(tt![mod])?;
        let mod_name = self.expect(tt![ident])?;

        self.expect(tt!['{'])?;

        let mut decls = vec![];

        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let item = self.parse_decl()?;
            decls.push(self.ast.insert_decl(item));
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Decl {
            vis,
            name: self.intern.get_or_intern(mod_name.lexeme),
            kind: DeclKind::Mod(decls),
            span: mod_token.span.merge(r_brace.span),
        })
    }

    fn parse_func_decl(&mut self, vis: Visibility) -> ParseResult<Decl> {
        let fn_token = self.expect(tt![fn])?;
        let func_name = self.expect(tt![ident])?;

        self.expect(tt!['('])?;

        let mut params = vec![];
        while !matches!(self.current.ty, tt![')'] | tt![eof]) {
            let comptime = self.advance_if(tt![comptime])?;
            let mutable = self.advance_if(tt![mut])?;
            let name = self.expect(tt![ident])?;
            self.expect(tt![:])?;
            let ty = self.parse_expr()?;

            params.push(Param {
                comptime,
                mutable,
                name: self.intern.get_or_intern(name.lexeme),
                span: name.span.merge(ty.span),
                ty: self.ast.insert_expr(ty),
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        self.expect(tt![')'])?;

        let ret = if self.advance_if(tt![:])? {
            let ret = self.parse_expr()?;
            Some(self.ast.insert_expr(ret))
        } else {
            None
        };

        let body = self.parse_block()?;

        Ok(Decl {
            vis,
            name: self.intern.get_or_intern(func_name.lexeme),
            span: fn_token.span.merge(body.span),
            kind: DeclKind::Func {
                params,
                ret,
                body: self.ast.insert_expr(body),
            },
        })
    }

    fn parse_const_decl(&mut self, vis: Visibility) -> ParseResult<Decl> {
        let const_token = self.expect(tt![const])?;
        let const_name = self.expect(tt![ident])?;

        let ty = if self.advance_if(tt![:])? {
            let ty = self.parse_expr()?;
            Some(self.ast.insert_expr(ty))
        } else {
            None
        };

        self.expect(tt![=])?;

        let value = self.parse_expr()?;
        let semi = self.expect(tt![;])?;

        Ok(Decl {
            vis,
            name: self.intern.get_or_intern(const_name.lexeme),
            kind: DeclKind::Const {
                ty,
                val: self.ast.insert_expr(value),
            },
            span: const_token.span.merge(semi.span),
        })
    }

    fn parse_struct_decl(&mut self, visibility: Visibility) -> ParseResult<Decl> {
        /*let struct_token = self.expect(tt![struct])?;
        let struct_name = self.expect(tt![ident])?;

        // TODO: parse generics

        self.expect(tt!['{'])?;

        let mut fields = vec![];
        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let vis = tnr! {self.advance_if(tt![pub])? => Visibility::Public : Visibility::Private};
            let field_name = self.expect(tt![ident])?;
            self.expect(tt![:])?;
            let field_ty = self.parse_type()?;

            fields.push(AstField {
                visibility: vis,
                span: field_name.span.merge(field_ty.span),
                name: self.intern.get_or_intern(field_name.lexeme),
                ty: self.ast.types.insert(field_ty),
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Decl {
            vis: visibility,
            name: self.intern.get_or_intern(struct_name.lexeme),
            kind: DeclKind::Struct {
                generics: vec![],
                fields,
            },
            span: struct_token.span.merge(r_brace.span),
        })*/
        todo!()
    }

    fn parse_union_decl(&mut self, visibility: Visibility) -> ParseResult<Decl> {
        /* let union_token = self.expect(tt![union])?;
        let union_name = self.expect(tt![ident])?;

        self.expect(tt!['{'])?;

        let mut fields = vec![];
        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let vis = tnr! {self.advance_if(tt![pub])? => Visibility::Public : Visibility::Private};
            let field_name = self.expect(tt![ident])?;
            self.expect(tt![:])?;
            let field_ty = self.parse_type()?;

            fields.push(AstField {
                visibility: vis,
                span: field_name.span.merge(field_ty.span),
                name: self.intern.get_or_intern(field_name.lexeme),
                ty: self.ast.types.insert(field_ty),
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Decl {
            vis: visibility,
            name: self.intern.get_or_intern(union_name.lexeme),
            kind: DeclKind::Union {
                generics: vec![],
                fields,
            },
            span: union_token.span.merge(r_brace.span),
        })
        */
        todo!()
    }

    fn parse_enum_decl(&mut self, visibility: Visibility) -> ParseResult<Decl> {
        /*let enum_token = self.expect(tt![enum])?;
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
                name: self.intern.get_or_intern(variant_name.lexeme),
                value,
                span,
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Decl {
            vis: visibility,
            name: self.intern.get_or_intern(enum_name.lexeme),
            kind: DeclKind::Enum { base, variants },
            span: enum_token.span.merge(r_brace.span),
        })*/
        todo!()
    }

    fn parse_let(&mut self) -> ParseResult<Stmt> {
        let let_token = self.expect(tt![let])?;
        let mutable = self.advance_if(tt![mut])?;
        let name = self.expect(tt![ident])?;
        let ty = if self.advance_if(tt![:])? {
            let ty = self.parse_precedence(Precedence::Coalesce, true)?;
            Some(self.ast.insert_expr(ty))
        } else {
            None
        };

        self.expect(tt![=])?;

        let value = self.parse_expr()?;
        let semi_token = self.expect(tt![;])?;

        Ok(Stmt {
            kind: StmtKind::Let {
                name: self.intern.get_or_intern(name.lexeme),
                ty,
                value: self.ast.insert_expr(value),
                mutable,
            },
            span: let_token.span.merge(semi_token.span),
        })
    }

    fn parse_expr(&mut self) -> ParseResult<Expr> {
        self.parse_precedence(Precedence::Assignment, true)
    }

    fn parse_expr_no_struct(&mut self) -> ParseResult<Expr> {
        self.parse_precedence(Precedence::Assignment, false)
    }

    fn parse_precedence(
        &mut self,
        precedence: Precedence,
        allow_struct: bool,
    ) -> ParseResult<Expr> {
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

    fn parse_prefix(&mut self, rule: PrefixRule, allow_struct: bool) -> ParseResult<Expr> {
        match rule {
            PrefixRule::LiteralCint => self.parse_cint(),
            PrefixRule::LiteralUint => self.parse_uint(),
            PrefixRule::LiteralInt => self.parse_int(),
            PrefixRule::LiteralFloat => self.parse_float(),
            PrefixRule::LiteralString => self.parse_string(),
            PrefixRule::LiteralBool => self.parse_bool(),
            PrefixRule::LiteralNull => self.parse_null(),
            PrefixRule::LiteralVoid => self.parse_void_type(),
            PrefixRule::LiteralArray => self.parse_array(),
            PrefixRule::Identifier => self.parse_ident(),
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
            PrefixRule::Int => self.parse_int_type(),
            PrefixRule::Uint => self.parse_uint_type(),
            PrefixRule::Bool => self.parse_bool_type(),
            PrefixRule::Float => self.parse_float_type(),
            PrefixRule::Void => self.parse_void_type(),
            PrefixRule::Option => self.parse_option_type(),
            PrefixRule::None => Err(ParseError {
                msg: format!("Expected expression, found {:?}", self.current.ty),
                span: self.current.span,
            }),
        }
    }

    fn parse_infix(
        &mut self,
        rule: InfixRule,
        left: Expr,
        allow_struct: bool,
    ) -> ParseResult<Expr> {
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

    fn parse_struct_lit(&mut self, ty: Expr) -> ParseResult<Expr> {
        self.expect(tt!['{'])?;

        let mut fields = vec![];
        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let name = self.expect(tt![ident])?;
            self.expect(tt![:])?;
            let value = self.parse_expr()?;

            fields.push(todo!());

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Expr {
            span: ty.span.merge(r_brace.span),
            kind: todo!(),
        })
    }

    fn parse_ident(&mut self) -> ParseResult<Expr> {
        let ident = self.expect(tt![ident])?;
        Ok(Expr {
            kind: ExprKind::Ident(self.intern.get_or_intern(ident.lexeme)),
            span: ident.span,
        })
    }

    fn parse_int_type(&mut self) -> ParseResult<Expr> {
        let int_ty = self.expect(tt![int])?;
        Ok(Expr {
            kind: ExprKind::IntType,
            span: int_ty.span,
        })
    }

    fn parse_uint_type(&mut self) -> ParseResult<Expr> {
        let uint_ty = self.expect(tt![uint])?;
        Ok(Expr {
            kind: ExprKind::UintType,
            span: uint_ty.span,
        })
    }

    fn parse_bool_type(&mut self) -> ParseResult<Expr> {
        let bool_ty = self.expect(tt![bool])?;
        Ok(Expr {
            kind: ExprKind::BoolType,
            span: bool_ty.span,
        })
    }

    fn parse_float_type(&mut self) -> ParseResult<Expr> {
        let float_ty = self.expect(tt![float])?;
        Ok(Expr {
            kind: ExprKind::FloatType,
            span: float_ty.span,
        })
    }

    fn parse_void_type(&mut self) -> ParseResult<Expr> {
        let void_ty = self.expect(tt![void])?;
        Ok(Expr {
            kind: ExprKind::VoidType,
            span: void_ty.span,
        })
    }

    fn parse_option_type(&mut self) -> ParseResult<Expr> {
        let option_ty = self.expect(tt![?])?;
        let inner = self.parse_precedence(Precedence::Unary, true)?;
        Ok(Expr {
            kind: ExprKind::OptionType(self.ast.insert_expr(inner)),
            span: option_ty.span,
        })
    }

    fn parse_cint(&mut self) -> ParseResult<Expr> {
        let int_token = self.expect(tt![cint_lit])?;

        match str::parse::<u64>(int_token.lexeme) {
            Ok(u) => Ok(Expr {
                kind: ExprKind::CintLit(u),
                span: int_token.span,
            }),
            Err(err) => Err(ParseError {
                msg: format!("Error parsing cint literal: {err}"),
                span: int_token.span,
            }),
        }
    }

    fn parse_uint(&mut self) -> ParseResult<Expr> {
        let int_token = self.expect(tt![uint_lit])?;

        match str::parse::<u64>(int_token.lexeme) {
            Ok(u) => Ok(Expr {
                kind: ExprKind::UintLit(u),
                span: int_token.span,
            }),
            Err(err) => Err(ParseError {
                msg: format!("Error parsing uint literal: {err}"),
                span: int_token.span,
            }),
        }
    }

    fn parse_int(&mut self) -> ParseResult<Expr> {
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

    fn parse_float(&mut self) -> ParseResult<Expr> {
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

    fn parse_bool(&mut self) -> ParseResult<Expr> {
        let token = self.consume()?;

        let val = match token.ty {
            tt![true] => true,
            tt![false] => false,
            tt => {
                return Err(ParseError {
                    msg: format!("Expected boolean literal, found: {tt:?}"),
                    span: token.span,
                });
            }
        };

        Ok(Expr {
            kind: ExprKind::BoolLit(val),
            span: token.span,
        })
    }

    fn parse_string(&mut self) -> ParseResult<Expr> {
        let str_token = self.expect(tt![str_lit])?;
        Ok(Expr {
            kind: todo!(),
            span: str_token.span,
        })
    }

    fn parse_null(&mut self) -> ParseResult<Expr> {
        let null_token = self.expect(tt![null])?;

        Ok(Expr {
            kind: ExprKind::NullLit,
            span: null_token.span,
        })
    }

    fn parse_array(&mut self) -> ParseResult<Expr> {
        let l_brkt = self.expect(tt!['['])?;

        // Parse empty array literal
        if self.check(tt![']']) {
            let r_brkt = self.consume()?;

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

            let value = self.ast.insert_expr(first);
            let count = self.ast.insert_expr(len);

            return Ok(Expr {
                kind: ExprKind::ArrayRep { value, count },
                span: l_brkt.span.merge(r_brkt.span),
            });
        }

        let mut elems = vec![self.ast.insert_expr(first)];

        if self.advance_if(tt![,])? {
            while !matches!(self.current.ty, tt![']'] | tt![eof]) {
                let elem = self.parse_expr()?;
                elems.push(self.ast.insert_expr(elem));

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

    fn parse_group(&mut self) -> ParseResult<Expr> {
        let open = self.expect(tt!['('])?;
        let expr = self.parse_expr()?;
        let close = self.expect(tt![')'])?;

        Ok(Expr {
            kind: ExprKind::Group(self.ast.insert_expr(expr)),
            span: open.span.merge(close.span),
        })
    }

    fn parse_unary(&mut self, allow_struct: bool) -> ParseResult<Expr> {
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
                rhs: self.ast.insert_expr(expr),
            },
        })
    }

    fn parse_binary(&mut self, lhs: Expr, allow_struct: bool) -> ParseResult<Expr> {
        let op_token = self.consume()?;
        let op = match op_token.ty {
            tt![+] => BinOp::Add,
            tt![-] => BinOp::Sub,
            tt![*] => BinOp::Mul,
            tt![/] => BinOp::Div,
            tt![%] => BinOp::Rem,
            tt![==] => BinOp::Eq,
            tt![!=] => BinOp::Ne,
            tt![<] => BinOp::Lt,
            tt![<=] => BinOp::Le,
            tt![>] => BinOp::Gt,
            tt![>=] => BinOp::Ge,
            tt![and] => BinOp::And,
            tt![or] => BinOp::Or,
            tt![&] => BinOp::BitAnd,
            tt![|] => BinOp::BitOr,
            tt![^] => BinOp::BitXor,
            tt![<<] => BinOp::Shl,
            tt![>>] => BinOp::Shr,
            tt![?:] => BinOp::Coalesce,
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
                lhs: self.ast.insert_expr(lhs),
                rhs: self.ast.insert_expr(rhs),
            },
        })
    }

    fn parse_assign(&mut self, tgt: Expr, allow_struct: bool) -> ParseResult<Expr> {
        let op_token = self.consume()?;
        let op = match op_token.ty {
            tt![=] => AssignOp::Eq,
            tt![+=] => AssignOp::AddEq,
            tt![-=] => AssignOp::SubEq,
            tt![*=] => AssignOp::MulEq,
            tt![/=] => AssignOp::DivEq,
            tt![%=] => AssignOp::ModEq,
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
                tgt: self.ast.insert_expr(tgt),
                val: self.ast.insert_expr(val),
            },
        })
    }

    fn parse_block(&mut self) -> ParseResult<Expr> {
        let l_brace = self.expect(tt!['{'])?;
        let mut stmts = vec![];

        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            match self.current.ty {
                // Consume leading/trailing semicolons
                tt![;] => {
                    self.consume()?;
                }

                // Parse let statements
                tt![let] => {
                    stmts.push(self.parse_let()?);
                }

                // Parse expression statements
                tt => {
                    // check if current token can start a block expression
                    let is_blk = matches!(
                        tt,
                        tt!['{'] | tt![if] | tt![match] | tt![while] | tt![for] | tt![loop]
                    );

                    let expr = self.parse_expr()?;

                    if self.advance_if(tt![;])? {
                        stmts.push(Stmt {
                            span: expr.span,
                            kind: StmtKind::Semi(self.ast.insert_expr(expr)),
                        });
                    } else {
                        stmts.push(Stmt {
                            span: expr.span,
                            kind: StmtKind::Expr(self.ast.insert_expr(expr)),
                        });

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

    fn parse_if(&mut self) -> ParseResult<Expr> {
        let if_token = self.expect(tt![if])?;

        let cond = self.parse_expr_no_struct()?;
        let then_branch = self.parse_block()?;

        let (else_branch, span) = if self.advance_if(tt![else])? {
            if self.check(tt![if]) {
                let expr = self.parse_if()?;
                let span = if_token.span.merge(expr.span);
                (Some(self.ast.insert_expr(expr)), span)
            } else {
                let expr = self.parse_block()?;
                let span = if_token.span.merge(expr.span);
                (Some(self.ast.insert_expr(expr)), span)
            }
        } else {
            (None, if_token.span.merge(then_branch.span))
        };

        Ok(Expr {
            kind: ExprKind::If {
                cond: self.ast.insert_expr(cond),
                then_branch: self.ast.insert_expr(then_branch),
                else_branch,
            },
            span,
        })
    }

    fn parse_while(&mut self) -> ParseResult<Expr> {
        let while_token = self.expect(tt![while])?;
        let cond = self.parse_expr_no_struct()?;
        let body = self.parse_block()?;

        Ok(Expr {
            span: while_token.span.merge(body.span),
            kind: ExprKind::While {
                cond: self.ast.insert_expr(cond),
                body: self.ast.insert_expr(body),
            },
        })
    }

    fn parse_loop(&mut self) -> ParseResult<Expr> {
        let loop_token = self.expect(tt![loop])?;
        let body = self.parse_block()?;

        Ok(Expr {
            span: loop_token.span.merge(body.span),
            kind: ExprKind::Loop(self.ast.insert_expr(body)),
        })
    }

    fn can_start_expr(&self) -> bool {
        ParseRule::get(self.current.ty).prefix != PrefixRule::None
    }

    fn parse_return(&mut self) -> ParseResult<Expr> {
        let return_token = self.expect(tt![return])?;
        let mut span = return_token.span;

        let expr = if self.can_start_expr() {
            let expr = self.parse_expr()?;
            span = span.merge(expr.span);
            Some(self.ast.insert_expr(expr))
        } else {
            None
        };

        Ok(Expr {
            kind: ExprKind::Return(expr),
            span,
        })
    }

    fn parse_break(&mut self) -> ParseResult<Expr> {
        let break_token = self.expect(tt![break])?;
        let mut span = break_token.span;

        /*let expr = if self.can_start_expr() {
            let expr = self.parse_expr()?;
            span = span.merge(expr.span);
            Some(self.ast.exprs.insert(expr))
        } else {
            None
        };

        Ok(Expr {
            kind: ExprKind::Break(expr),
            span,
        })*/
        todo!()
    }

    fn parse_continue(&mut self) -> ParseResult<Expr> {
        let continue_token = self.expect(tt![continue])?;

        /*  Ok(Expr {
            kind: ExprKind::Continue,
            span: continue_token.span,
        })*/
        todo!()
    }

    fn parse_call(&mut self, callee: Expr) -> ParseResult<Expr> {
        self.expect(tt!['('])?;

        /*let mut args = vec![];
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
        })*/
        todo!()
    }

    fn parse_dot(&mut self, object: Expr) -> ParseResult<Expr> {
        self.expect(tt![.])?;
        let field = self.expect(tt![ident])?;

        /*Ok(Expr {
            span: object.span.merge(field.span),
            kind: ExprKind::Field {
                object: self.ast.exprs.insert(object),
                field: self.intern.get_or_intern(field.lexeme),
            },
        })*/
        todo!()
    }

    fn parse_index(&mut self, object: Expr) -> ParseResult<Expr> {
        self.expect(tt!['['])?;
        let index = self.parse_expr()?;
        let r_brkt = self.expect(tt![']'])?;

        Ok(Expr {
            span: object.span.merge(r_brkt.span),
            kind: ExprKind::Index {
                object: self.ast.insert_expr(object),
                index: self.ast.insert_expr(index),
            },
        })
    }
}

#[inline(always)]
pub fn parse(ast: &mut Ast, intern: &mut Interner, source: &str) -> Result<DeclId, ParseError> {
    Parser::new(ast, intern, source)?.parse()
}
