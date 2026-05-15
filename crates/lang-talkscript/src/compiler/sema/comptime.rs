use crate::{
    arena::Ident,
    compiler::{
        ast::{Ast, ExprId, ExprKind},
        token::Span,
    },
};

use super::types::Ty;

#[derive(Debug, thiserror::Error)]
pub enum ComptimeEvalError {
    #[error("Unsupported expression at {0:?}")]
    UnsupportedExpr(Span),
    #[error("Expected type, found {0}")]
    ExpectedType(Ty),
}

#[derive(Debug, Clone)]
pub enum ComptimeVal {
    Type(Ty),
    Int(i64),
    Uint(u64),
    Bool(bool),
    Array(Vec<ComptimeVal>),
    Struct(Ty, Vec<(Ident, ComptimeVal)>),
}

pub struct ComptimeEval<'a> {
    ast: &'a Ast,
}

impl<'a> ComptimeEval<'a> {
    pub fn new(ast: &'a Ast) -> Self {
        Self { ast }
    }

    pub fn value_ty(&self, value: &ComptimeVal) -> Ty {
        todo!()
    }

    pub fn eval(&self, expr_id: ExprId) -> Result<ComptimeVal, ComptimeEvalError> {
        let expr = self.ast.expr(expr_id);
        let result = match &expr.kind {
            ExprKind::IntLit(i) => ComptimeVal::Int(*i),
            ExprKind::UintLit(u) => ComptimeVal::Uint(*u),
            ExprKind::BoolLit(b) => ComptimeVal::Bool(*b),

            ExprKind::IntType => ComptimeVal::Type(Ty::Int),
            ExprKind::UintType => ComptimeVal::Type(Ty::Uint),
            ExprKind::BoolType => ComptimeVal::Type(Ty::Bool),
            ExprKind::FloatType => ComptimeVal::Type(Ty::Float),
            ExprKind::VoidType => ComptimeVal::Type(Ty::Void),
            ExprKind::OptionType(ty) => {
                ComptimeVal::Type(Ty::Optional(Box::new(self.eval_type(*ty)?)))
            }
            _ => return Err(ComptimeEvalError::UnsupportedExpr(expr.span)),
        };

        Ok(result)
    }

    pub fn eval_type(&self, expr_id: ExprId) -> Result<Ty, ComptimeEvalError> {
        match self.eval(expr_id)? {
            ComptimeVal::Type(ty) => Ok(ty),
            val => Err(ComptimeEvalError::ExpectedType(self.value_ty(&val))),
        }
    }
}
