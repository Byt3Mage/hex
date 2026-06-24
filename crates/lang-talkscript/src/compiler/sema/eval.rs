//! Comptime evaluator. Reduces an expression to a `Value`.
//! Type-position expressions reduce to `Value::Type`.

use crate::arena::Ident;
use crate::compiler::ast::{Ast, BinOp, ExprId, ExprKind, UnOp};
use crate::compiler::sema::types::{TypeArena, TypeId};
use crate::compiler::sema::value::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, thiserror::Error)]
pub enum EvalError {
    #[error("undefined name")]
    Undefined(Ident),
    #[error("expression is not a type")]
    NotAType { found: Value },
    #[error("cyclic const definition")]
    CyclicConst(Ident),
    #[error("operation not supported on these comptime values")]
    InvalidOp,
    #[error("division by zero")]
    DivByZero,
    #[error("called a non-function value at comptime")]
    NotCallable,
    #[error("comptime evaluation not supported for this expression")]
    Unsupported,
}

/// A binding visible to the evaluator.
#[derive(Clone)]
pub enum Binding {
    /// Known at comptime: builtins, consts, comptime params.
    Comptime(Value),
    /// A runtime binding (`let`, runtime param): has a type, no comptime value.
    Runtime(TypeId),
}

pub struct ConstEval<'a> {
    pub ast: &'a Ast,
    pub types: &'a mut TypeArena,
    pub scope: HashMap<Ident, Binding>,
}

impl<'a> ConstEval<'a> {
    pub fn eval(&mut self, id: ExprId) -> Result<Value, EvalError> {
        let expr = self.ast.expr(id);
        match &expr.kind {
            ExprKind::IntLit(i) => Ok(Value::Int(*i)),
            ExprKind::UintLit(u) => Ok(Value::Uint(*u)),
            ExprKind::CintLit(c) => Ok(Value::Int(*c as i64)), // default cint→int at comptime
            ExprKind::FloatLit(f) => Ok(Value::Float(*f)),
            ExprKind::BoolLit(b) => Ok(Value::Bool(*b)),
            ExprKind::VoidLit => Ok(Value::Void),

            ExprKind::Ident(name) => match self.scope.get(name) {
                Some(Binding::Comptime(v)) => Ok(v.clone()),
                Some(Binding::Runtime(_)) => Err(EvalError::Unsupported),
                None => Err(EvalError::Undefined(*name)),
            },

            ExprKind::Group(inner) => self.eval(*inner),

            ExprKind::Unary { op, rhs } => {
                let v = self.eval(*rhs)?;
                self.eval_unary(*op, v)
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.eval(*lhs)?;
                let r = self.eval(*rhs)?;
                self.eval_binary(*op, l, r)
            }

            ExprKind::OptionType(inner) => {
                let t = self.eval_type(*inner)?;
                Ok(Value::Type(self.types.optional(t)))
            }
            ExprKind::ArrayType { elem, len } => {
                let e = self.eval_type(*elem)?;
                let n = match self.eval(*len)? {
                    Value::Uint(u) => u,
                    Value::Int(i) if i >= 0 => i as u64,
                    _ => return Err(EvalError::InvalidOp),
                };
                Ok(Value::Type(self.types.array(e, n)))
            }
            ExprKind::FnType { params, ret } => {
                let p = params
                    .iter()
                    .map(|p| self.eval_type(*p))
                    .collect::<Result<_, _>>()?;
                let r = self.eval_type(*ret)?;
                Ok(Value::Type(self.types.func(p, r)))
            }

            _ => Err(EvalError::Unsupported),
        }
    }

    /// Evaluate an expression in type position; require a type.
    pub fn eval_type(&mut self, id: ExprId) -> Result<TypeId, EvalError> {
        self.eval(id)?.as_type()
    }

    fn eval_unary(&self, op: UnOp, v: Value) -> Result<Value, EvalError> {
        match (op, v) {
            (UnOp::Neg, Value::Int(i)) => Ok(Value::Int(-i)),
            (UnOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
            (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            _ => Err(EvalError::InvalidOp),
        }
    }

    fn eval_binary(&self, op: BinOp, l: Value, r: Value) -> Result<Value, EvalError> {
        use Value::{Bool, Float, Int, Uint};
        Ok(match (op, l, r) {
            (BinOp::Add, Int(a), Int(b)) => Int(a + b),
            (BinOp::Sub, Int(a), Int(b)) => Int(a - b),
            (BinOp::Mul, Int(a), Int(b)) => Int(a * b),
            (BinOp::Div, Int(a), Int(b)) => {
                if b == 0 {
                    return Err(EvalError::DivByZero);
                }
                Int(a / b)
            }
            (BinOp::Rem, Int(a), Int(b)) => {
                if b == 0 {
                    return Err(EvalError::DivByZero);
                }
                Int(a % b)
            }

            (BinOp::Add, Uint(a), Uint(b)) => Uint(a + b),
            (BinOp::Sub, Uint(a), Uint(b)) => Uint(a - b),
            (BinOp::Mul, Uint(a), Uint(b)) => Uint(a * b),
            (BinOp::Div, Uint(a), Uint(b)) => {
                if b == 0 {
                    return Err(EvalError::DivByZero);
                }
                Uint(a / b)
            }

            (BinOp::Eq, Int(a), Int(b)) => Bool(a == b),
            (BinOp::Ne, Int(a), Int(b)) => Bool(a != b),
            (BinOp::Lt, Int(a), Int(b)) => Bool(a < b),
            (BinOp::Le, Int(a), Int(b)) => Bool(a <= b),
            (BinOp::Gt, Int(a), Int(b)) => Bool(a > b),
            (BinOp::Ge, Int(a), Int(b)) => Bool(a >= b),

            (BinOp::And, Bool(a), Bool(b)) => Bool(a && b),
            (BinOp::Or, Bool(a), Bool(b)) => Bool(a || b),

            (BinOp::Add, Float(a), Float(b)) => Float(a + b),
            (BinOp::Sub, Float(a), Float(b)) => Float(a - b),
            (BinOp::Mul, Float(a), Float(b)) => Float(a * b),

            _ => return Err(EvalError::InvalidOp),
        })
    }
}
