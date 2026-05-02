use ahash::AHashMap;
use hex_mir::{Function, FunctionBuilder, Val};

use crate::{
    arena::{Ident, Interner},
    compiler::{
        ast::{Ast, BinOp, ExprId, ExprKind, Param, Stmt, StmtKind, UnOp},
        sema::{
            comptime::{ComptimeEval, ComptimeEvalError, ComptimeVal},
            type_engine::SemaTy,
        },
    },
};

pub mod comptime;
pub mod type_engine;

struct Binding {
    val: SemaVal,
    ty: SemaTy,
}

struct Scope {
    bindings: AHashMap<Ident, Binding>,
}

pub struct FnSig {
    params: Vec<SemaTy>,
    ret: SemaTy,
}

#[derive(Debug, Clone, Copy)]
pub struct SemaVal {
    pub base: Val,
    pub width: u32,
}

impl SemaVal {
    pub fn scalar(val: Val) -> Self {
        Self {
            base: val,
            width: 1,
        }
    }

    pub fn void() -> Self {
        Self {
            base: Val(0),
            width: 0,
        }
    }
}

pub enum ExprResult {
    Runtime(SemaVal, SemaTy),
    Comptime(ComptimeVal),
}

#[derive(Debug, thiserror::Error)]
pub enum SemaError {
    #[error("undefined variable: {0:?}")]
    Undefined(Ident),
    #[error("expected a type expression")]
    ExpectedType,
    #[error("expected a runtime value")]
    ExpectedRuntimeValue,
    #[error("unsupported binary op {0:?} for type {1:?}")]
    InvalidBinOp(BinOp, SemaTy),
    #[error("unsupported unary op {0:?} for type {1:?}")]
    InvalidUnOp(UnOp, SemaTy),
    #[error("comptime evaluation error: {0}")]
    ComptimeEvalError(#[from] ComptimeEvalError),
}

pub struct Sema<'a> {
    ast: &'a Ast,
    intern: &'a Interner,

    func_builder: FunctionBuilder,
    scopes: Vec<Scope>,
    fn_sigs: AHashMap<Ident, FnSig>,
    comp_eval: ComptimeEval<'a>,
}

impl<'a> Sema<'a> {
    fn push_scope(&mut self) {
        self.scopes.push(Scope {
            bindings: AHashMap::new(),
        });
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: Ident, val: SemaVal, ty: SemaTy) {
        let scope = self.scopes.last_mut().unwrap();
        scope.bindings.insert(name, Binding { val, ty });
    }

    fn lookup(&self, name: Ident) -> Result<&Binding, SemaError> {
        for scope in self.scopes.iter().rev() {
            if let Some(binding) = scope.bindings.get(&name) {
                return Ok(binding);
            }
        }
        Err(SemaError::Undefined(name))
    }

    fn check_expr(&mut self, expr_id: ExprId) -> Result<ExprResult, SemaError> {
        let expr = self.ast.expr(expr_id);

        let result = match &expr.kind {
            ExprKind::IntLit(i) => {
                let val = self.func_builder.load_int(*i);
                ExprResult::Runtime(SemaVal::scalar(val), SemaTy::Int)
            }
            ExprKind::UintLit(u) => {
                let val = self.func_builder.load_uint(*u);
                ExprResult::Runtime(SemaVal::scalar(val), SemaTy::Uint)
            }
            ExprKind::BoolLit(b) => {
                let val = self.func_builder.load_bool(*b);
                ExprResult::Runtime(SemaVal::scalar(val), SemaTy::Bool)
            }
            ExprKind::Ident(name) => {
                let binding = self.lookup(*name)?;
                ExprResult::Runtime(binding.val, binding.ty.clone())
            }

            ExprKind::Binary { op, lhs, rhs } => self.check_binary(*op, *lhs, *rhs)?,
            ExprKind::Unary { op, rhs } => self.check_unary(*op, *rhs)?,
            ExprKind::Block(stmts) => self.check_block(stmts)?,
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.check_if(*cond, *then_branch, *else_branch)?,
            ExprKind::Return(value) => self.check_return(*value)?,

            ExprKind::IntType => ExprResult::Comptime(ComptimeVal::Type(SemaTy::Int)),
            ExprKind::UintType => ExprResult::Comptime(ComptimeVal::Type(SemaTy::Uint)),
            ExprKind::BoolType => ExprResult::Comptime(ComptimeVal::Type(SemaTy::Bool)),
            ExprKind::VoidType => ExprResult::Comptime(ComptimeVal::Type(SemaTy::Void)),

            _ => todo!("unhandled expr kind"),
        };

        Ok(result)
    }

    fn check_binary(
        &mut self,
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    ) -> Result<ExprResult, SemaError> {
        let lhs = self.check_expr(lhs)?;
        let rhs = self.check_expr(rhs)?;

        todo!()
    }

    fn check_unary(&mut self, op: UnOp, rhs: ExprId) -> Result<ExprResult, SemaError> {
        let rhs = self.check_expr(rhs)?;
        todo!()
    }

    fn check_block(&mut self, stmts: &[Stmt]) -> Result<ExprResult, SemaError> {
        self.push_scope();

        let mut result = ExprResult::Runtime(SemaVal::void(), SemaTy::Void);

        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Let { name, ty, value } => {
                    let ann_ty = match ty {
                        Some(ty_expr) => Some(self.check_expr(*ty_expr)?),
                        None => None,
                    };

                    let val = self.check_expr(*value)?;

                    // todo: coerce val to ann_ty

                    // self.define(*name, val, ty);
                    result = ExprResult::Runtime(SemaVal::void(), SemaTy::Void);
                }

                StmtKind::Semi(expr_id) => {
                    self.check_expr(*expr_id)?;
                    result = ExprResult::Runtime(SemaVal::void(), SemaTy::Void);
                }

                StmtKind::Expr(expr_id) => {
                    result = self.check_expr(*expr_id)?;
                }
            }
        }

        self.pop_scope();
        Ok(result)
    }

    fn check_if(
        &mut self,
        cond: ExprId,
        then_branch: ExprId,
        else_branch: Option<ExprId>,
    ) -> Result<ExprResult, SemaError> {
        let cond = self.check_expr(cond)?;

        let then_block = self.func_builder.begin_block(vec![]);
        let else_block = self.func_builder.begin_block(vec![]);

        todo!()
    }

    fn check_return(&mut self, value: Option<ExprId>) -> Result<ExprResult, SemaError> {
        todo!()
    }

    #[inline]
    fn eval_type(&mut self, expr_id: ExprId) -> Result<SemaTy, ComptimeEvalError> {
        self.comp_eval.eval_type(expr_id)
    }

    pub fn lower_function(
        parent: &'a mut Sema<'a>,
        ast: &'a Ast,
        intern: &'a Interner,
        name: impl Into<String>,
        params: &[Param],
        ret: Option<ExprId>,
        body: ExprId,
        fn_sigs: AHashMap<Ident, FnSig>,
    ) -> Result<Function, SemaError> {
        let mut param_tys = Vec::new();

        let mut narg = 0;
        for param in params {
            let ty = parent.eval_type(param.ty)?;
            narg += ty.width();
            param_tys.push(ty);
        }

        let ret_ty = match ret {
            Some(r) => parent.eval_type(r)?,
            None => SemaTy::Void,
        };
        let nret = ret_ty.width();

        let func_builder = FunctionBuilder::new(name, narg, nret);

        let mut sema = Sema {
            ast,
            intern,
            func_builder,
            scopes: Vec::new(),
            fn_sigs,
            comp_eval: ComptimeEval::new(ast),
        };

        sema.push_scope();

        // Check body
        let result = sema.check_expr(body)?;

        sema.pop_scope();
        Ok(sema.func_builder.build())
    }
}
