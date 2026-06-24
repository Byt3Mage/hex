//! Runtime typechecker. Assigns a TypeId to each runtime expression
//! and checks consistency. Calls into Eval for every type-position expr.

use crate::{
    arena::{Arena, Ident},
    compiler::{
        ast::{Ast, BinOp, ExprId, ExprKind, Stmt, StmtKind, UnOp},
        sema::{
            eval::{Binding, ConstEval, EvalError},
            types::{TypeArena, TypeId, TypeVal},
            value::Value,
        },
        typed_ast::{TBinding, TBindingKind, TExpr, TExprId, TExprKind, TyRef},
    },
};
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
pub enum TypeCheckError {
    #[error("undefined variable")]
    Undefined(Ident),
    #[error("type mismatch: expected {expected:?}, found {found:?}")]
    Mismatch { expected: TypeId, found: TypeId },
    #[error("invalid binary op for operand types")]
    InvalidBinOp,
    #[error("invalid unary op for operand type")]
    InvalidUnOp,
    #[error("eval error: {0}")]
    Eval(#[from] EvalError),
    #[error("not callable")]
    NotCallable,
    #[error("wrong number of arguments")]
    ArgCount,
}

pub struct TypeChecker<'a> {
    pub ast: &'a Ast,
    pub types: &'a mut TypeArena,
    exprs: Arena<TExprId, TExpr>,
    scopes: Vec<HashMap<Ident, TypeId>>,
}

impl<'a> TypeChecker<'a> {
    pub fn new(ast: &'a Ast, types: &'a mut TypeArena) -> Self {
        Self {
            ast,
            types,
            exprs: Arena::with_capacity(ast.exprs.len()),
            scopes: vec![HashMap::new()],
        }
    }

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop(&mut self) {
        self.scopes.pop();
    }
    fn define(&mut self, n: Ident, t: TypeId) {
        self.scopes.last_mut().unwrap().insert(n, t);
    }

    fn lookup(&self, n: Ident) -> Option<TypeId> {
        self.scopes.iter().rev().find_map(|s| s.get(&n).copied())
    }

    /// Run the evaluator for a type-position expr, sharing globals.
    fn eval_type(&mut self, id: ExprId) -> Result<TypeId, TypeCheckError> {
        let mut ev = ConstEval {
            ast: self.ast,
            types: self.types,
            scope: self.globals.clone(),
        };
        Ok(ev.eval_type(id)?)
    }

    fn type_of(&mut self, expr: TExprId) -> TyRef {
        todo!()
    }

    pub fn check_expr(
        &mut self,
        id: ExprId,
        expected: Option<TyRef>,
    ) -> Result<TExprId, TypeCheckError> {
        let expr = self.ast.expr(id);
        let typed_expr = match &expr.kind {
            ExprKind::IntLit(i) => {
                let expr = TExpr {
                    kind: TExprKind::IntLit(*i),
                    ty: TyRef::Concrete(self.types.int()),
                    origin: id,
                };
            }
            ExprKind::UintLit(_) => self.types.uint(),
            ExprKind::CintLit(_) => {
                // phase one: default cint to the expected int/uint, else int
                match expected.map(|e| self.types.get(e)) {
                    Some(TypeVal::Uint) => self.types.uint(),
                    _ => self.types.int(),
                }
            }
            ExprKind::FloatLit(_) => self.types.float(),
            ExprKind::BoolLit(_) => self.types.bool(),
            ExprKind::VoidLit => self.types.void(),

            ExprKind::Ident(n) => {
                if let Some(t) = self.lookup(*n) {
                    t
                } else if let Some(Binding::Comptime(v)) = self.globals.get(n) {
                    // a const used in value position: its type
                    match v {
                        Value::Int(_) => self.types.int(),
                        Value::Uint(_) => self.types.uint(),
                        Value::Bool(_) => self.types.bool(),
                        Value::Float(_) => self.types.float(),
                        Value::Void => self.types.void(),
                        Value::Type(_) => self.types.type_(),
                        Value::Fn(_) => {
                            return Err(TypeCheckError::NotCallable);
                        }
                    }
                } else {
                    return Err(TypeCheckError::Undefined(*n));
                }
            }

            ExprKind::Group(inner) => self.check_expr(*inner, expected)?,

            ExprKind::Unary { op, rhs } => {
                let r = self.check_expr(*rhs, None)?;
                match (op, self.types.get(r)) {
                    (UnOp::Neg, TypeVal::Int) => self.types.int(),
                    (UnOp::Neg, TypeVal::Float) => self.types.float(),
                    (UnOp::Not, TypeVal::Bool) => self.types.bool(),
                    _ => return Err(TypeCheckError::InvalidUnOp),
                }
            }

            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.check_expr(*lhs, None)?;
                let r = self.check_expr(*rhs, Some(l))?;
                self.check_binary(*op, l, r)?
            }

            ExprKind::Block(stmts) => self.check_block(stmts, expected)?,

            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let c = self.check_expr(*cond, Some(self.types.bool()))?;
                self.unify(self.types.bool(), c)?;
                let t = self.check_expr(*then_branch, expected)?;
                match else_branch {
                    Some(e) => {
                        let et = self.check_expr(*e, expected)?;
                        self.unify(t, et)?;
                        t
                    }
                    None => self.types.void(),
                }
            }

            ExprKind::Return(_) => self.types.void(), // never-typed; simplified for phase one

            ExprKind::Call { callee, args } => self.check_call(*callee, args)?,

            _ => return Err(TypeCheckError::Eval(EvalError::Unsupported)),
        };

        if let Some(exp) = expected {
            self.unify(exp, ty)?;
        }

        Ok(ty)
    }

    fn check_block(
        &mut self,
        stmts: &[Stmt],
        expected: Option<TypeId>,
    ) -> Result<TExprId, TypeCheckError> {
        self.push();
        let mut result;

        let last = stmts.len().saturating_sub(1);
        for (i, stmt) in stmts.iter().enumerate() {
            match &stmt.kind {
                StmtKind::Let {
                    name,
                    ty,
                    value,
                    mutable,
                } => {
                    let bind_ty = match ty {
                        Some(t) => {
                            let ann = self.eval_type(*t)?;
                            let ann_ref = TyRef::Concrete(ann);
                            let val = self.check_expr(*value, Some(ann_ref))?;

                            ann_ref
                        }
                        None => {
                            let val = self.check_expr(*value, None)?;
                            self.type_of(val)
                        }
                    };

                    let binding = TBinding {
                        name: *name,
                        ty: bind_ty,
                        mutable: *mutable,
                        kind: TBindingKind::Let,
                    };

                    todo!("define binding")
                }
                StmtKind::Semi(e) => {
                    self.check_expr(*e, None)?;
                    result = self.types.void();
                }
                StmtKind::Expr(e) => {
                    let exp = if i == last { expected } else { None };
                    let t = self.check_expr(*e, exp)?;
                    if i == last {
                        result = t;
                    }
                }
            }
        }
        self.pop();
        Ok(result)
    }

    fn check_call(&mut self, callee: ExprId, args: &[ExprId]) -> Result<TypeId, TypeCheckError> {
        // phase one: callee must be an Ident bound to a Value::Fn whose
        // signature type we look up. The driver registers fn types in globals.
        let cty = self.check_expr(callee, None)?;
        let TypeVal::Fn { params, ret } = self.types.get(cty).clone() else {
            return Err(TypeCheckError::NotCallable);
        };
        if params.len() != args.len() {
            return Err(TypeCheckError::ArgCount);
        }
        for (&p, &a) in params.iter().zip(args) {
            let at = self.check_expr(a, Some(p))?;
            self.unify(p, at)?;
        }
        Ok(ret)
    }

    fn check_binary(&mut self, op: BinOp, l: TypeId, r: TypeId) -> Result<TypeId, TypeCheckError> {
        use TypeVal::{Bool, Float, Int, Uint};
        let lt = self.types.get(l).clone();
        let rt = self.types.get(r).clone();
        Ok(match (op, &lt, &rt) {
            (BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem, Int, Int) => {
                self.types.int()
            }
            (BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem, Uint, Uint) => {
                self.types.uint()
            }
            (BinOp::Add | BinOp::Sub | BinOp::Mul, Float, Float) => self.types.float(),
            (op, Int, Int) | (op, Uint, Uint) | (op, Float, Float) if op.is_comparison() => {
                self.types.bool()
            }
            (BinOp::And | BinOp::Or, Bool, Bool) => self.types.bool(),
            _ => return Err(TypeCheckError::InvalidBinOp),
        })
    }

    /// Phase one: types are interned, so equality is id equality.
    fn unify(&self, expected: TypeId, found: TypeId) -> Result<(), TypeCheckError> {
        if expected == found {
            Ok(())
        } else {
            Err(TypeCheckError::Mismatch { expected, found })
        }
    }
}
