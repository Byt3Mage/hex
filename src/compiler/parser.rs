use simple_ternary::tnr;

use super::{
    ast::{AstArena, Expr, ExprKind, Param, Stmt, StmtKind},
    ast_op::{AssignOp, BinaryOp, UnaryOp},
    lexer::{LexError, Lexer},
    parse_rules::{InfixRule, ParseRule, Precedence, PrefixRule},
    tokens::{Span, Token, TokenType},
};

use crate::{
    arena::Interner,
    compiler::ast::{AstField, AstVariant, Decl, DeclKind, ExprId, Pattern, Visibility},
    tt,
};

#[derive(Debug)]
pub struct ParseError {
    msg: String,
    span: Span,
}

type ParseResult<T> = Result<T, ParseError>;

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
    ) -> Result<Parser<'a>, LexError> {
        let current = lexer.next_token()?;

        Ok(Parser {
            interner,
            lexer,
            current,
            ast,
        })
    }

    pub fn advance(&mut self) -> Result<(), ParseError> {
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

    fn advance_if(&mut self, ty: TokenType) -> Result<bool, ParseError> {
        if self.current.ty == ty {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn consume(&mut self) -> Result<Token<'a>, ParseError> {
        let token = self.current.clone();
        self.advance()?;
        Ok(token)
    }

    #[inline(always)]
    fn check(&self, ty: TokenType) -> bool {
        self.current.ty == ty
    }

    fn expect(&mut self, ty: TokenType) -> Result<Token<'a>, ParseError> {
        if self.current.ty == ty {
            self.consume()
        } else {
            Err(ParseError {
                msg: format!("Expected {:?}, found {:?}", ty, self.current.ty),
                span: self.current.span,
            })
        }
    }

    pub fn parse_source(&mut self) -> ParseResult<ExprId> {
        let mut items = vec![];

        while !self.check(tt![eof]) {
            let decl = self.parse_decl()?;
            items.push(self.ast.decls.insert(decl));
        }

        let module = Expr {
            kind: ExprKind::ModuleType(items),
            span: Span::default().merge(self.current.span),
        };

        Ok(self.ast.exprs.insert(module))
    }

    fn parse_decl(&mut self) -> ParseResult<Decl> {
        let is_pub = self.advance_if(tt![pub])?;
        let vis = tnr! { is_pub => Visibility::Public : Visibility::Private};

        match self.current.ty {
            tt![fn] => self.parse_function(vis),
            tt![const] => self.parse_const(vis),
            tt => Err(ParseError {
                msg: format!("Expected 'const' or 'fn', found {tt:?}"),
                span: self.current.span,
            }),
        }
    }

    fn parse_function(&mut self, visibility: Visibility) -> ParseResult<Decl> {
        let fn_token = self.expect(tt![fn])?;
        let func_name = self.expect(tt![ident])?;

        self.expect(tt!['('])?;

        let mut params = vec![];
        while !matches!(self.current.ty, tt![')'] | tt![eof]) {
            let pattern = self.parse_pattern()?;
            self.expect(tt![:])?;
            let ty = self.parse_expr()?;

            params.push(Param {
                span: pattern.span.merge(ty.span),
                pattern: self.ast.patterns.insert(pattern),
                ty: self.ast.exprs.insert(ty),
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        self.expect(tt![')'])?;

        let ret = if self.advance_if(tt![->])? {
            let ret = self.parse_expr()?;
            Some(self.ast.exprs.insert(ret))
        } else {
            None
        };

        let body = self.parse_block()?;

        Ok(Decl {
            visibility,
            name: self.interner.get_or_intern(func_name.lexeme),
            span: fn_token.span.merge(body.span),
            kind: DeclKind::Function {
                params,
                ret,
                body: self.ast.exprs.insert(body),
            },
        })
    }

    fn parse_const(&mut self, visibility: Visibility) -> ParseResult<Decl> {
        let const_token = self.expect(tt![const])?;
        let const_name = self.expect(tt![ident])?;

        let ty = if self.advance_if(tt![:])? {
            let ty = self.parse_expr()?;
            Some(self.ast.exprs.insert(ty))
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

    fn parse_module_type(&mut self) -> ParseResult<Expr> {
        let mod_token = self.expect(tt![mod])?;
        self.expect(tt!['{'])?;

        let mut items = vec![];

        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let item = self.parse_decl()?;
            items.push(self.ast.decls.insert(item));
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Expr {
            kind: ExprKind::ModuleType(items),
            span: mod_token.span.merge(r_brace.span),
        })
    }

    fn parse_struct_type(&mut self) -> ParseResult<Expr> {
        let struct_token = self.expect(tt![struct])?;

        self.expect(tt!['{'])?;

        let mut fields = vec![];
        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let vis = tnr! {self.advance_if(tt![pub])? => Visibility::Public : Visibility::Private};
            let field_name = self.expect(tt![ident])?;
            self.expect(tt![:])?;
            let field_ty = self.parse_expr()?;

            fields.push(AstField {
                visibility: vis,
                span: field_name.span.merge(field_ty.span),
                name: self.interner.get_or_intern(field_name.lexeme),
                ty: self.ast.exprs.insert(field_ty),
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Expr {
            kind: ExprKind::StructType(fields),
            span: struct_token.span.merge(r_brace.span),
        })
    }

    fn parse_union_type(&mut self) -> ParseResult<Expr> {
        let union_token = self.expect(tt![union])?;

        self.expect(tt!['{'])?;

        let mut fields = vec![];
        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let vis = tnr! {self.advance_if(tt![pub])? => Visibility::Public : Visibility::Private};
            let field_name = self.expect(tt![ident])?;
            self.expect(tt![:])?;
            let field_ty = self.parse_expr()?;

            fields.push(AstField {
                visibility: vis,
                span: field_name.span.merge(field_ty.span),
                name: self.interner.get_or_intern(field_name.lexeme),
                ty: self.ast.exprs.insert(field_ty),
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Expr {
            kind: ExprKind::UnionType(fields),
            span: union_token.span.merge(r_brace.span),
        })
    }

    fn parse_enum_type(&mut self) -> ParseResult<Expr> {
        let enum_token = self.expect(tt![enum])?;

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

            variants.push(AstVariant {
                name: self.interner.get_or_intern(variant_name.lexeme),
                value,
                span,
            });

            if !self.advance_if(tt![,])? {
                break;
            }
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Expr {
            kind: ExprKind::EnumType(variants),
            span: enum_token.span.merge(r_brace.span),
        })
    }

    fn parse_semi(&mut self) -> ParseResult<Stmt> {
        let semi_token = self.expect(tt![;])?;

        Ok(Stmt {
            kind: StmtKind::Empty,
            span: semi_token.span,
        })
    }

    fn parse_pattern(&mut self) -> ParseResult<Pattern> {
        todo!("parse pattern")
    }

    fn parse_let(&mut self) -> ParseResult<Stmt> {
        let pat = self.parse_pattern()?;
        let ty = tnr! {self.advance_if(tt![:])? => Some(self.parse_expr()?) : None};

        self.expect(tt![=])?;

        let value = self.parse_expr()?;
        let semi_token = self.expect(tt![;])?;

        Ok(Stmt {
            span: pat.span.merge(semi_token.span),
            kind: StmtKind::Let {
                pattern: self.ast.patterns.insert(pat),
                ty: ty.map(|t| self.ast.exprs.insert(t)),
                value: self.ast.exprs.insert(value),
            },
        })
    }

    fn parse_expr_stmt(&mut self) -> ParseResult<Stmt> {
        let expr = self.parse_expr()?;

        Ok(Stmt {
            span: expr.span,
            kind: StmtKind::Expr {
                expr: self.ast.exprs.insert(expr),
                has_semi: self.advance_if(tt![;])?,
            },
        })
    }

    fn parse_expr(&mut self) -> ParseResult<Expr> {
        self.parse_precedence(Precedence::Assignment)
    }

    fn parse_precedence(&mut self, precedence: Precedence) -> ParseResult<Expr> {
        let rule = ParseRule::get(self.current.ty);
        let mut expr = self.parse_prefix(rule.prefix)?;

        loop {
            let infix_rule = ParseRule::get(self.current.ty);

            if precedence <= infix_rule.precedence && infix_rule.infix != InfixRule::None {
                expr = self.parse_infix(infix_rule.infix, expr)?;
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_prefix(&mut self, rule: PrefixRule) -> ParseResult<Expr> {
        match rule {
            PrefixRule::LiteralInt => self.parse_int(),
            PrefixRule::LiteralFloat => self.parse_float(),
            PrefixRule::LiteralString => self.parse_string(),
            PrefixRule::True => self.parse_true(),
            PrefixRule::False => self.parse_false(),
            PrefixRule::LiteralNull => self.parse_null(),
            PrefixRule::LiteralVoid => self.parse_void(),
            PrefixRule::LiteralArray => self.parse_array(),
            PrefixRule::Identifier => self.parse_identifier(),
            PrefixRule::Grouping => self.parse_group(),
            PrefixRule::Unary => self.parse_unary(),
            PrefixRule::If => self.parse_if(),
            PrefixRule::Block => self.parse_block(),
            PrefixRule::While => self.parse_while(),
            PrefixRule::Loop => self.parse_loop(),
            PrefixRule::Match => todo!("parse match"),
            PrefixRule::Return => self.parse_return(),
            PrefixRule::Break => self.parse_break(),
            PrefixRule::Continue => self.parse_continue(),
            PrefixRule::LiteralStruct => todo!(),
            PrefixRule::ModuleType => self.parse_module_type(),
            PrefixRule::StructType => self.parse_struct_type(),
            PrefixRule::UnionType => self.parse_union_type(),
            PrefixRule::EnumType => self.parse_enum_type(),
            PrefixRule::PointerType => self.parse_pointer_type(),
            PrefixRule::None => Err(ParseError {
                msg: format!("Expected expression, found {:?}", self.current.ty),
                span: self.current.span,
            }),
        }
    }

    fn parse_infix(&mut self, rule: InfixRule, left: Expr) -> ParseResult<Expr> {
        match rule {
            InfixRule::Binary => self.parse_binary(left),
            InfixRule::Assign => self.parse_assign(left),
            InfixRule::Call => self.parse_call(left),
            InfixRule::Dot => self.parse_dot(left),
            InfixRule::Index => self.parse_index(left),
            InfixRule::None => Err(ParseError {
                msg: format!("Expected expression, found {:?}", self.current.ty),
                span: self.current.span,
            }),
        }
    }

    fn parse_identifier(&mut self) -> ParseResult<Expr> {
        let ident_token = self.expect(tt![ident])?;

        Ok(Expr {
            kind: ExprKind::Ident(self.interner.get_or_intern(ident_token.lexeme)),
            span: ident_token.span,
        })
    }

    fn parse_int(&mut self) -> ParseResult<Expr> {
        let int_token = self.expect(tt![int_lit])?;

        match str::parse::<u64>(int_token.lexeme) {
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

    fn parse_string(&mut self) -> ParseResult<Expr> {
        let str_token = self.expect(tt![str_lit])?;

        Ok(Expr {
            kind: ExprKind::StrLit(self.interner.get_or_intern(str_token.lexeme)),
            span: str_token.span,
        })
    }

    fn parse_true(&mut self) -> ParseResult<Expr> {
        let true_token = self.expect(tt![true])?;

        Ok(Expr {
            kind: ExprKind::True,
            span: true_token.span,
        })
    }

    fn parse_false(&mut self) -> ParseResult<Expr> {
        let false_token = self.expect(tt![false])?;

        Ok(Expr {
            kind: ExprKind::False,
            span: false_token.span,
        })
    }

    fn parse_null(&mut self) -> ParseResult<Expr> {
        let null_token = self.expect(tt![null])?;

        Ok(Expr {
            kind: ExprKind::Null,
            span: null_token.span,
        })
    }

    fn parse_void(&mut self) -> ParseResult<Expr> {
        let void_token = self.expect(tt![void])?;

        Ok(Expr {
            kind: ExprKind::Void,
            span: void_token.span,
        })
    }

    fn parse_array(&mut self) -> ParseResult<Expr> {
        let l_brkt = self.expect(tt!['['])?;

        let mut elems = vec![];

        while !matches!(self.current.ty, tt![']'] | tt![eof]) {
            let elem = self.parse_expr()?;
            elems.push(self.ast.exprs.insert(elem));

            if !self.advance_if(tt![,])? {
                break;
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
            kind: ExprKind::Group(self.ast.exprs.insert(expr)),
            span: open.span.merge(close.span),
        })
    }

    fn parse_unary(&mut self) -> ParseResult<Expr> {
        let op_token = self.consume()?;

        let op = match op_token.ty {
            tt![-] => UnaryOp::Neg,
            tt![!] => UnaryOp::Not,
            _ => {
                return Err(ParseError {
                    msg: format!("Unsupported unary operator: {:?}", op_token.ty),
                    span: op_token.span,
                });
            }
        };

        let expr = self.parse_precedence(Precedence::Unary)?;

        Ok(Expr {
            span: op_token.span.merge(expr.span),
            kind: ExprKind::Unary {
                op,
                expr: self.ast.exprs.insert(expr),
            },
        })
    }

    fn parse_binary(&mut self, lhs: Expr) -> ParseResult<Expr> {
        let op_token = self.consume()?;
        let op = match op_token.ty {
            tt![+] => BinaryOp::Add,
            tt![-] => BinaryOp::Sub,
            tt![*] => BinaryOp::Mul,
            tt![/] => BinaryOp::Div,
            tt![%] => BinaryOp::Mod,
            tt![==] => BinaryOp::Eq,
            tt![!=] => BinaryOp::Ne,
            tt![<] => BinaryOp::Lt,
            tt![<=] => BinaryOp::Le,
            tt![>] => BinaryOp::Gt,
            tt![>=] => BinaryOp::Ge,
            _ => {
                return Err(ParseError {
                    msg: format!("Unsupported binary operator: {:?}", op_token.ty),
                    span: op_token.span,
                });
            }
        };

        let precedence = ParseRule::get(op_token.ty).precedence;
        let rhs = self.parse_precedence(precedence.next())?;

        Ok(Expr {
            span: lhs.span.merge(rhs.span),
            kind: ExprKind::Binary {
                op,
                lhs: self.ast.exprs.insert(lhs),
                rhs: self.ast.exprs.insert(rhs),
            },
        })
    }

    fn parse_assign(&mut self, tgt: Expr) -> ParseResult<Expr> {
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

        let val = self.parse_precedence(Precedence::Assignment)?;

        Ok(Expr {
            span: tgt.span.merge(val.span),
            kind: ExprKind::Assign {
                op,
                tgt: self.ast.exprs.insert(tgt),
                val: self.ast.exprs.insert(val),
            },
        })
    }

    fn parse_block(&mut self) -> ParseResult<Expr> {
        let l_brace = self.expect(tt!['{'])?;
        let mut stmts = vec![];

        while !matches!(self.current.ty, tt!['}'] | tt![eof]) {
            let stmt = match self.current.ty {
                tt![;] => self.parse_semi()?,
                tt![let] => self.parse_let()?,
                _ => self.parse_expr_stmt()?,
            };

            stmts.push(self.ast.stmts.insert(stmt))
        }

        let r_brace = self.expect(tt!['}'])?;

        Ok(Expr {
            kind: ExprKind::Block(stmts),
            span: l_brace.span.merge(r_brace.span),
        })
    }

    fn parse_if(&mut self) -> ParseResult<Expr> {
        let if_token = self.expect(tt![if])?;

        let cond = self.parse_expr()?;
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

    fn parse_while(&mut self) -> ParseResult<Expr> {
        let while_token = self.expect(tt![while])?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;

        Ok(Expr {
            span: while_token.span.merge(body.span),
            kind: ExprKind::While {
                cond: self.ast.exprs.insert(cond),
                body: self.ast.exprs.insert(body),
            },
        })
    }

    fn parse_loop(&mut self) -> ParseResult<Expr> {
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

    fn parse_return(&mut self) -> ParseResult<Expr> {
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

    fn parse_break(&mut self) -> ParseResult<Expr> {
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

    fn parse_continue(&mut self) -> ParseResult<Expr> {
        let continue_token = self.expect(tt![continue])?;

        Ok(Expr {
            kind: ExprKind::Continue,
            span: continue_token.span,
        })
    }

    fn parse_call(&mut self, callee: Expr) -> ParseResult<Expr> {
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

    fn parse_dot(&mut self, object: Expr) -> ParseResult<Expr> {
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

    fn parse_index(&mut self, object: Expr) -> ParseResult<Expr> {
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

    fn parse_pointer_type(&mut self) -> ParseResult<Expr> {
        let at_token = self.expect(tt![@])?;
        let mutable = self.advance_if(tt![mut])?;
        let pointee = self.parse_expr()?;
        let span = at_token.span.merge(pointee.span);
        let pointee = self.ast.exprs.insert(pointee);

        Ok(Expr {
            kind: ExprKind::PointerType { mutable, pointee },
            span,
        })
    }
}
