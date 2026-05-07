use std::ops::Deref;

use ahash::AHashMap;
use hex_mir::{self as hxm};

use crate::{
    arena::Ident,
    compiler::{
        ast::{Ast, BinOp, ExprId, ExprKind, Param, Stmt, StmtKind, UnOp},
        sema::{
            comptime::{ComptimeEval, ComptimeEvalError, ComptimeVal},
            sema_type::{SemaTy, TypeError},
        },
    },
};

pub mod comptime;
pub mod sema_type;

#[derive(Debug, thiserror::Error)]
pub enum SemaError {
    #[error("undefined variable: {0:?}")]
    UndefinedVariable(Ident),
    #[error("type mismatch: expected {exp}, got {got}")]
    TypeMismatch { exp: SemaTy, got: SemaTy },
    #[error("unsupported binary op {0} for lhs: {1} and rhs: {2}")]
    InvalidBinOp(BinOp, SemaTy, SemaTy),
    #[error("unsupported unary op {0} for type {1}")]
    InvalidUnOp(UnOp, SemaTy),
    #[error("comptime evaluation error: {0}")]
    ComptimeEvalError(#[from] ComptimeEvalError),
    #[error("mir codegen error: {0}")]
    MirCodegenError(#[from] hxm::MirError),
    #[error("attempted field access on non-struct/union type: {0}")]
    InvalidFieldAccess(SemaTy),
    #[error("invalid optional field access on non-optional type: {0}")]
    InvalidOptFieldAccess(SemaTy),
    #[error(transparent)]
    TypeError(#[from] TypeError),
}

#[derive(Debug, Clone)]
struct ExprResult {
    base: hxm::Val,
    ty: SemaTy,
}

const VOID_EXPR: ExprResult = ExprResult {
    base: hxm::ZERO_VAL,
    ty: SemaTy::Void,
};

const NEVER_EXPR: ExprResult = ExprResult {
    base: hxm::ZERO_VAL,
    ty: SemaTy::Never,
};

impl ExprResult {
    fn vals(&self) -> impl Iterator<Item = hxm::Val> {
        (0..self.ty.width()).map(|i| self.base.add(i as hxm::RegTy))
    }

    fn field(&self, offset: usize) -> hxm::Val {
        self.base.add(offset as hxm::RegTy)
    }
}

struct Binding {
    value: ExprResult,
    mutable: bool,
}

pub struct Sema<'a> {
    ast: &'a Ast,
    func: hxm::FunctionBuilder,
    scopes: Vec<AHashMap<Ident, Binding>>,
    comp_eval: ComptimeEval<'a>,
}

impl<'a> Sema<'a> {
    fn push_scope(&mut self) {
        self.scopes.push(AHashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: Ident, value: ExprResult, mutable: bool) {
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name, Binding { value, mutable });
    }

    fn lookup(&self, name: Ident) -> Result<&Binding, SemaError> {
        for scope in self.scopes.iter().rev() {
            if let Some(binding) = scope.get(&name) {
                return Ok(binding);
            }
        }

        Err(SemaError::UndefinedVariable(name))
    }

    fn lower_expr(&mut self, expr_id: ExprId) -> Result<ExprResult, SemaError> {
        let expr = self.ast.expr(expr_id);

        let result = match &expr.kind {
            ExprKind::VoidLit => VOID_EXPR,
            ExprKind::IntLit(i) => {
                let base = self.func.alloc_n(1)?;
                let ty = SemaTy::Int;
                self.func.load_int(base, *i);
                ExprResult { base, ty }
            }
            ExprKind::UintLit(u) => {
                let base = self.func.alloc_n(1)?;
                let ty = SemaTy::Uint;
                self.func.load_uint(base, *u);
                ExprResult { base, ty }
            }
            ExprKind::BoolLit(b) => {
                let base = self.func.alloc_n(1)?;
                let ty = SemaTy::Bool;
                self.func.load_bool(base, *b);
                ExprResult { base, ty }
            }
            ExprKind::FloatLit(f) => {
                let base = self.func.alloc_n(1)?;
                let ty = SemaTy::Float;
                self.func.load_float(base, *f);
                ExprResult { base, ty }
            }

            ExprKind::NullLit => todo!(),
            ExprKind::ArrayLit(elems) => self.lower_arr_lit(elems)?,
            ExprKind::ArrayRep { value, count } => self.lower_arr_rep(*value, *count)?,

            ExprKind::Ident(name) => self.lookup(*name)?.value.clone(),
            ExprKind::Group(inner) => self.lower_expr(*inner)?,
            ExprKind::Binary { op, lhs, rhs } => self.lower_binary(*op, *lhs, *rhs)?,
            ExprKind::Unary { op, rhs } => self.lower_unary(*op, *rhs)?,
            ExprKind::Block(stmts) => self.lower_block(stmts)?,

            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.lower_if(*cond, *then_branch, *else_branch)?,

            ExprKind::Return(value) => self.lower_return(*value)?,
            ExprKind::StructLit { ty, fields } => todo!(),
            ExprKind::Assign { op, tgt, val } => todo!(),
            ExprKind::Cast { expr, ty } => todo!(),
            ExprKind::While { cond, body } => todo!(),
            ExprKind::Loop(expr_id) => todo!(),
            ExprKind::Break(expr_id) => todo!(),
            ExprKind::Continue => todo!(),
            ExprKind::Call { callee, args } => todo!(),
            ExprKind::Field { object, field } => self.lower_field(*object, *field)?,
            ExprKind::OptField { object, field } => self.lower_opt_field(*object, *field)?,
            ExprKind::Index { object, index } => todo!(),
            ExprKind::Comptime(expr_id) => todo!(),
            ExprKind::IntType => todo!(),
            ExprKind::UintType => todo!(),
            ExprKind::BoolType => todo!(),
            ExprKind::FloatType => todo!(),
            ExprKind::VoidType => todo!(),
            ExprKind::OptionType(_) => todo!(),
        };

        Ok(result)
    }

    fn lower_binary(
        &mut self,
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    ) -> Result<ExprResult, SemaError> {
        let lhs = self.lower_expr(lhs)?;
        let rhs = self.lower_expr(rhs)?;

        let result = match (op, &lhs.ty, &rhs.ty) {
            (BinOp::Add, SemaTy::Int, SemaTy::Int) => ExprResult {
                base: self.func.binop(hxm::BinOp::IAdd, lhs.base, rhs.base)?,
                ty: SemaTy::Int,
            },
            (BinOp::Sub, SemaTy::Int, SemaTy::Int) => ExprResult {
                base: self.func.binop(hxm::BinOp::ISub, lhs.base, rhs.base)?,
                ty: SemaTy::Int,
            },
            (BinOp::Mul, SemaTy::Int, SemaTy::Int) => ExprResult {
                base: self.func.binop(hxm::BinOp::IMul, lhs.base, rhs.base)?,
                ty: SemaTy::Int,
            },
            (BinOp::Div, SemaTy::Int, SemaTy::Int) => ExprResult {
                base: self.func.binop(hxm::BinOp::IDiv, lhs.base, rhs.base)?,
                ty: SemaTy::Int,
            },
            (BinOp::Rem, SemaTy::Int, SemaTy::Int) => ExprResult {
                base: self.func.binop(hxm::BinOp::IRem, lhs.base, rhs.base)?,
                ty: SemaTy::Int,
            },

            (BinOp::Add, SemaTy::Uint, SemaTy::Uint) => ExprResult {
                base: self.func.binop(hxm::BinOp::UAdd, lhs.base, rhs.base)?,
                ty: SemaTy::Uint,
            },
            (BinOp::Sub, SemaTy::Uint, SemaTy::Uint) => ExprResult {
                base: self.func.binop(hxm::BinOp::USub, lhs.base, rhs.base)?,
                ty: SemaTy::Uint,
            },
            (BinOp::Mul, SemaTy::Uint, SemaTy::Uint) => ExprResult {
                base: self.func.binop(hxm::BinOp::UMul, lhs.base, rhs.base)?,
                ty: SemaTy::Uint,
            },
            (BinOp::Div, SemaTy::Uint, SemaTy::Uint) => ExprResult {
                base: self.func.binop(hxm::BinOp::UDiv, lhs.base, rhs.base)?,
                ty: SemaTy::Uint,
            },
            (BinOp::Rem, SemaTy::Uint, SemaTy::Uint) => ExprResult {
                base: self.func.binop(hxm::BinOp::URem, lhs.base, rhs.base)?,
                ty: SemaTy::Uint,
            },

            (BinOp::Add, SemaTy::Float, SemaTy::Float) => ExprResult {
                base: self.func.binop(hxm::BinOp::FAdd, lhs.base, rhs.base)?,
                ty: SemaTy::Float,
            },
            (BinOp::Sub, SemaTy::Float, SemaTy::Float) => ExprResult {
                base: self.func.binop(hxm::BinOp::FSub, lhs.base, rhs.base)?,
                ty: SemaTy::Float,
            },
            (BinOp::Mul, SemaTy::Float, SemaTy::Float) => ExprResult {
                base: self.func.binop(hxm::BinOp::FMul, lhs.base, rhs.base)?,
                ty: SemaTy::Float,
            },
            (BinOp::Div, SemaTy::Float, SemaTy::Float) => ExprResult {
                base: self.func.binop(hxm::BinOp::FDiv, lhs.base, rhs.base)?,
                ty: SemaTy::Float,
            },
            (BinOp::Rem, SemaTy::Float, SemaTy::Float) => ExprResult {
                base: self.func.binop(hxm::BinOp::FRem, lhs.base, rhs.base)?,
                ty: SemaTy::Float,
            },
            _ => return Err(SemaError::InvalidBinOp(op, lhs.ty, rhs.ty)),
        };

        Ok(result)
    }

    fn lower_unary(&mut self, op: UnOp, rhs: ExprId) -> Result<ExprResult, SemaError> {
        let rhs = self.lower_expr(rhs)?;
        let result = match (op, &rhs.ty) {
            (UnOp::Neg, SemaTy::Int) => ExprResult {
                base: self.func.unop(hxm::UnOp::INeg, rhs.base)?,
                ty: SemaTy::Int,
            },
            (UnOp::Neg, SemaTy::Float) => ExprResult {
                base: self.func.unop(hxm::UnOp::FNeg, rhs.base)?,
                ty: SemaTy::Float,
            },

            (UnOp::Not, SemaTy::Int) => ExprResult {
                base: self.func.unop(hxm::UnOp::INot, rhs.base)?,
                ty: SemaTy::Int,
            },
            (UnOp::Not, SemaTy::Uint) => ExprResult {
                base: self.func.unop(hxm::UnOp::UNot, rhs.base)?,
                ty: SemaTy::Uint,
            },
            (UnOp::Not, SemaTy::Bool) => ExprResult {
                base: self.func.unop(hxm::UnOp::BNot, rhs.base)?,
                ty: SemaTy::Bool,
            },
            _ => return Err(SemaError::InvalidUnOp(op, rhs.ty)),
        };

        Ok(result)
    }

    fn lower_block(&mut self, stmts: &[Stmt]) -> Result<ExprResult, SemaError> {
        self.push_scope();

        let last = stmts.len() - 1;
        let mut result = VOID_EXPR;

        for (i, stmt) in stmts.iter().enumerate() {
            match &stmt.kind {
                StmtKind::Let {
                    name,
                    ty,
                    value,
                    mutable,
                } => {
                    let val = self.lower_expr(*value)?;

                    let expr = match ty {
                        Some(ty) => {
                            let ty = self.comp_eval.eval_type(*ty)?;
                            let base = self.func.alloc_n(ty.width())?;
                            self.coerce(base, &ty, &val)?;
                            ExprResult { base, ty }
                        }
                        None => {
                            let base = self.func.alloc_n(val.ty.width())?;
                            self.func.assign(base, val.vals().collect());
                            ExprResult { base, ty: val.ty }
                        }
                    };

                    self.define(*name, expr, *mutable);
                    result = VOID_EXPR;
                }

                StmtKind::Semi(expr_id) => {
                    // Lower expression, but result is supressed.
                    // Diverging expressions always diverge the block.
                    let val = self.lower_expr(*expr_id)?;
                    result = match val.ty {
                        SemaTy::Never => NEVER_EXPR,
                        _ => VOID_EXPR,
                    };
                }

                StmtKind::Expr(expr_id) => {
                    let val = self.lower_expr(*expr_id)?;

                    // Expression without semicolon must be the last or coerce to void type.
                    // We use coerce instead of direct comparison to support diverging expressions.
                    // E.g. `return (x, y)` // no semi colon.
                    if i == last {
                        result = val;
                    } else {
                        self.coerce(VOID_EXPR.base, &VOID_EXPR.ty, &val)?;
                    }
                }
            }
        }

        self.pop_scope();
        Ok(result)
    }

    fn lower_if(
        &mut self,
        cond: ExprId,
        then_expr: ExprId,
        else_expr: Option<ExprId>,
    ) -> Result<ExprResult, SemaError> {
        let cond = self.lower_expr(cond)?;

        let then_block = self.func.begin_block(vec![]);
        let else_block = self.func.begin_block(vec![]);

        todo!()
    }

    fn lower_return(&mut self, value: Option<ExprId>) -> Result<ExprResult, SemaError> {
        todo!()
    }

    fn lower_arr_lit(&mut self, elems: &[ExprId]) -> Result<ExprResult, SemaError> {
        let [first, rest @ ..] = elems else {
            todo!("handle empty array")
        };

        let first = self.lower_expr(*first)?;
        let width = first.ty.width();
        let len = elems.len();

        let base = self.func.alloc_n(width * len)?;
        self.coerce(base, &first.ty, &first)?;

        let width = width as hxm::RegTy;
        let mut dst = base;

        for elem in rest {
            dst = dst.add(width);
            let elem = self.lower_expr(*elem)?;
            self.coerce(dst, &first.ty, &elem)?;
        }

        Ok(ExprResult {
            base,
            ty: SemaTy::Array {
                elem_ty: Box::new(first.ty),
                len,
            },
        })
    }

    fn lower_arr_rep(&mut self, value: ExprId, count: ExprId) -> Result<ExprResult, SemaError> {
        let value = self.lower_expr(value)?;
        let len = match self.comp_eval.eval(count)? {
            ComptimeVal::Uint(len) => len as usize,
            val => {
                return Err(SemaError::TypeMismatch {
                    exp: SemaTy::Uint,
                    got: val.sema_type(),
                });
            }
        };

        debug_assert!(len != 0);

        let base = self.func.alloc_n(value.ty.width() * len)?;
        let vals = (0..len).flat_map(|_| value.vals()).collect();
        self.func.assign(base, vals);

        Ok(ExprResult {
            base,
            ty: SemaTy::Array {
                elem_ty: Box::new(value.ty),
                len,
            },
        })
    }

    fn lower_field(&mut self, object: ExprId, field: Ident) -> Result<ExprResult, SemaError> {
        let object = self.lower_expr(object)?;

        match &object.ty {
            SemaTy::Struct(ty) => {
                let field = ty.field(field)?;
                Ok(ExprResult {
                    base: object.field(field.offset),
                    ty: field.ty.clone(),
                })
            }
            SemaTy::Union(ty) => {
                let field = ty.field(field)?;

                // TODO: lower field access for union
                // Compare tag, then create optional aggregate

                Ok(ExprResult {
                    base: object.field(field.offset),
                    ty: SemaTy::Optional(Box::new(field.ty.clone())),
                })
            }
            _ => Err(SemaError::InvalidFieldAccess(object.ty)),
        }
    }

    fn lower_opt_field(&mut self, object: ExprId, field: Ident) -> Result<ExprResult, SemaError> {
        let object = self.lower_expr(object)?;
        let SemaTy::Optional(obj_ty) = &object.ty else {
            return Err(SemaError::InvalidOptFieldAccess(object.ty));
        };

        match obj_ty.deref() {
            SemaTy::Struct(ty) => {
                let field = ty.field(field)?;
                let opt_ty = SemaTy::Optional(Box::new(field.ty.clone()));
                let base = self.func.alloc_n(opt_ty.width())?;

                todo!("branching code")
            }
            _ => Err(SemaError::InvalidFieldAccess(SemaTy::clone(obj_ty))),
        }
    }

    fn coerce(
        &mut self,
        dst: hxm::Val,
        dst_ty: &SemaTy,
        val: &ExprResult,
    ) -> Result<(), SemaError> {
        if dst_ty == &val.ty {
            self.func.assign(dst, val.vals().collect());
            return Ok(());
        }

        match (dst_ty, &val.ty) {
            (_, SemaTy::Never) => Ok(()),

            (SemaTy::Optional(inner), _) if **inner == val.ty => {
                // Writing value before setting the tag sees to produce better bytecode
                self.func.assign(dst.add(1), val.vals().collect());
                self.func.load_bool(dst, true);
                Ok(())
            }

            (SemaTy::Optional(_), SemaTy::Null) => {
                // TODO: consider writing zero values
                self.func.load_bool(dst, false);
                Ok(())
            }

            (
                SemaTy::Pointer {
                    pointee: tgt_ptr,
                    is_mut: false,
                },
                SemaTy::Pointer {
                    pointee: val_ptr,
                    is_mut: val_mut,
                },
            ) if tgt_ptr == val_ptr => todo!("pointer coerce"),

            _ => Err(SemaError::TypeMismatch {
                exp: dst_ty.clone(),
                got: val.ty.clone(),
            }),
        }
    }
}

pub fn lower_function<'a>(
    ast: &'a Ast,
    name: impl Into<String>,
    params: &[Param],
    ret: Option<ExprId>,
    body: ExprId,
) -> Result<hxm::Function, SemaError> {
    let comp_eval = ComptimeEval::new(ast);
    let mut func = hxm::FunctionBuilder::new(name);
    let mut param_bindings = AHashMap::new();

    for param in params {
        let ty = comp_eval.eval_type(param.ty)?;
        let base = func.add_arg(ty.width())?;
        param_bindings.insert(
            param.name,
            Binding {
                value: ExprResult { base, ty },
                mutable: param.mutable,
            },
        );
    }

    let ret_ty = match ret {
        Some(r) => comp_eval.eval_type(r)?,
        None => SemaTy::Void,
    };

    func.set_ret(ret_ty.width())?;

    let mut sema = Sema {
        ast,
        func,
        scopes: vec![param_bindings],
        comp_eval,
    };

    let result = sema.lower_expr(body)?;
    let ret = ExprResult {
        base: sema.func.alloc_n(ret_ty.width())?,
        ty: ret_ty,
    };

    sema.coerce(ret.base, &ret.ty, &result)?;

    if !sema.func.is_terminated() {
        sema.func.ret(ret.vals().collect());
    }

    Ok(sema.func.build())
}
