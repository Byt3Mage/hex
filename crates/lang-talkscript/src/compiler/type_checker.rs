//! AST → TAST walker.
//!
//! Walks the AST, resolves names, allocates inference variables for
//! ambiguous types (e.g. unsuffixed integer literals), and emits constraints
//! for the constraint solver to resolve later.

use ahash::AHashMap;

use crate::{
    arena::{Arena, Ident},
    compiler::ast::{self, Ast, ExprId, ExprKind, Stmt, StmtKind},
    compiler::sema::{typed_ast::*, types::Ty},
};

#[derive(Debug, thiserror::Error)]
pub enum TypeError {
    #[error("undefined variable: {0:?}")]
    UndefinedVariable(Ident),
}

pub struct InferCtx {
    next_var: u32,
}

impl InferCtx {
    pub fn new() -> Self {
        Self { next_var: 0 }
    }

    /// Allocate a fresh inference variable.
    pub fn fresh(&mut self) -> InferVarId {
        let id = InferVarId(self.next_var);
        self.next_var += 1;
        id
    }
}

pub struct TypeMap {}
pub struct TypeChecker<'a> {
    ast: &'a Ast,
    bindings: Arena<TBindingId, TBinding>,
    exprs: Arena<TExprId, TExpr>,
    scopes: Vec<AHashMap<Ident, TBindingId>>,
    infer: InferCtx,
}

impl<'a> TypeChecker<'a> {
    pub fn new(ast: &'a Ast) -> Self {
        Self {
            ast,
            bindings: Arena::new(),
            exprs: Arena::new(),
            scopes: vec![AHashMap::new()],
            infer: InferCtx::new(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(AHashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, binding: TBinding) -> TBindingId {
        let name = binding.name;
        let id = self.bindings.insert(binding);
        self.scopes
            .last_mut()
            .expect("at least one scope")
            .insert(name, id);
        id
    }

    fn lookup(&self, name: Ident) -> Result<TBindingId, TypeError> {
        for scope in self.scopes.iter().rev() {
            if let Some(&id) = scope.get(&name) {
                return Ok(id);
            }
        }
        Err(TypeError::UndefinedVariable(name))
    }

    #[inline(always)]
    fn expr(&self, expr_id: TExprId) -> &TExpr {
        self.exprs.get(expr_id)
    }

    pub fn check_expr(&mut self, expr_id: ExprId) -> Result<TExprId, TypeError> {
        let expr = self.ast.expr(expr_id);
        let span = expr.span;

        let (kind, ty) = match &expr.kind {
            ExprKind::CintLit(u) => {
                // type unresolved, allocate an inference variable
                (TExprKind::CintLit(*u), TyRef::Var(self.infer.fresh()))
            }
            ExprKind::IntLit(i) => (TExprKind::IntLit(*i), TyRef::Concrete(Ty::Int)),
            ExprKind::UintLit(u) => (TExprKind::UintLit(*u), TyRef::Concrete(Ty::Uint)),
            ExprKind::BoolLit(b) => (TExprKind::BoolLit(*b), TyRef::Concrete(Ty::Bool)),
            ExprKind::FloatLit(f) => (TExprKind::FloatLit(*f), TyRef::Concrete(Ty::Float)),

            ExprKind::Ident(name) => {
                let binding = self.lookup(*name)?;
                let ty = self.bindings.get(binding).ty.clone();
                (TExprKind::Ident(binding), ty)
            }

            ExprKind::Block(stmts) => {
                let (block_stmts, block_ty) = self.check_block(stmts)?;
                (TExprKind::Block(block_stmts), block_ty)
            }

            ExprKind::Return(value) => {
                let inner = match value {
                    Some(e) => Some(self.check_expr(*e)?),
                    None => None,
                };
                // Return diverges, so its type is Never.
                (TExprKind::Return(inner), TyRef::Concrete(Ty::Never))
            }

            ExprKind::Binary { op, lhs, rhs } => {
                let lhs_id = self.check_expr(*lhs)?;
                let rhs_id = self.check_expr(*rhs)?;

                let lhs_ty = self.expr(lhs_id).ty.clone();
                let rhs_ty = self.expr(rhs_id).ty.clone();

                todo!()
            }

            _ => todo!("walk_expr: handle this kind"),
        };

        Ok(self.exprs.insert(TExpr {
            kind,
            ty,
            span,
            origin: expr_id,
        }))
    }

    fn check_block(&mut self, stmts: &[Stmt]) -> Result<(Vec<TStmt>, TyRef), TypeError> {
        self.push_scope();

        let mut result_ty = TyRef::Concrete(Ty::Void);
        let mut typed_stmts = Vec::with_capacity(stmts.len());
        let last = stmts.len().saturating_sub(1);

        for (i, stmt) in stmts.iter().enumerate() {
            let kind = match &stmt.kind {
                StmtKind::Let {
                    name,
                    ty,
                    value,
                    mutable,
                } => {
                    let value = self.check_expr(*value)?;
                    let texpr = self.expr(value);

                    // TODO: when ty_ann is Some, evaluate the annotated type via
                    // comp_eval and emit a Coerce constraint:
                    //   Coerce { from: value_ty, to: annotated_ty }
                    // For now, the binding's type is the value's type.
                    let binding_ty = texpr.ty.clone();

                    let binding = self.define(TBinding {
                        name: *name,
                        ty: binding_ty,
                        mutable: *mutable,
                        kind: BindingKind::Runtime,
                        span: stmt.span,
                    });

                    TStmtKind::Let { binding, value }
                }

                StmtKind::Semi(expr_id) => {
                    let id = self.check_expr(*expr_id)?;
                    TStmtKind::Semi(id)
                }

                StmtKind::Expr(expr_id) => {
                    let id = self.check_expr(*expr_id)?;
                    if i == last {
                        result_ty = self.expr(id).ty.clone();
                    }
                    TStmtKind::Expr(id)
                }
            };

            typed_stmts.push(TStmt {
                kind,
                span: stmt.span,
            });
        }

        self.pop_scope();
        Ok((typed_stmts, result_ty))
    }

    pub fn finish(self) -> (Arena<TBindingId, TBinding>, Arena<TExprId, TExpr>, InferCtx) {
        (self.bindings, self.exprs, self.infer)
    }
}

pub fn check_function<'a>(
    ast: &'a Ast,
    name: impl Into<String>,
    params: &[ast::Param],
    ret_type: Ty,
    body: ExprId,
) -> Result<(TFunction, InferCtx), TypeError> {
    let mut checker = TypeChecker::new(ast);

    // Register params as bindings in the entry scope.
    let mut param_ids = Vec::with_capacity(params.len());
    for param in params {
        // TODO: resolve param.ty via comp_eval. Hardcoded Int for now.
        let ty = TyRef::Concrete(Ty::Int);
        let id = checker.define(TBinding {
            name: param.name,
            ty,
            mutable: param.mutable,
            kind: BindingKind::Runtime,
            span: param.span,
        });
        param_ids.push(id);
    }

    let body = checker.check_expr(body)?;
    let (bindings, exprs, infer) = checker.finish();

    Ok((
        TFunction {
            name: name.into(),
            params: param_ids,
            ret_type,
            bindings,
            exprs,
            body,
        },
        infer,
    ))
}
