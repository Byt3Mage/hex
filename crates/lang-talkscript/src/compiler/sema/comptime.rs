use crate::{
    arena::Ident,
    compiler::{
        ast::{Ast, ExprId, ExprKind},
        tokens::Span,
    },
};

use super::type_engine::SemaTy;

#[derive(Debug, thiserror::Error)]
pub enum ComptimeEvalError {
    #[error("Unsupported expression at {0:?}")]
    UnsupportedExpr(Span),
    #[error("Expected type, found {0:?}")]
    ExpectedType(SemaTy),
}

#[derive(Debug, Clone)]
pub enum ComptimeVal {
    Int(i64),
    Uint(u64),
    Float(f64),
    Bool(bool),

    Array(Vec<ComptimeVal>),
    Struct {
        ty: SemaTy,
        fields: Vec<(Ident, ComptimeVal)>,
    },
    Type(SemaTy),
}

impl ComptimeVal {
    pub fn sema_type(&self) -> SemaTy {
        match self {
            ComptimeVal::Int(_) => SemaTy::Int,
            ComptimeVal::Uint(_) => SemaTy::Uint,
            ComptimeVal::Float(_) => SemaTy::Float,
            ComptimeVal::Bool(_) => SemaTy::Bool,
            ComptimeVal::Array(_) => todo!("impl comptime array type"),
            ComptimeVal::Struct { ty, .. } => ty.clone(),
            ComptimeVal::Type(_) => SemaTy::Type,
        }
    }
}
pub struct ComptimeEval<'a> {
    ast: &'a Ast,
}

impl<'a> ComptimeEval<'a> {
    pub fn new(ast: &'a Ast) -> Self {
        Self { ast }
    }

    pub fn eval(&self, expr_id: ExprId) -> Result<ComptimeVal, ComptimeEvalError> {
        let expr = self.ast.expr(expr_id);
        match &expr.kind {
            ExprKind::IntLit(i) => Ok(ComptimeVal::Int(*i)),
            ExprKind::UintLit(u) => Ok(ComptimeVal::Uint(*u)),
            ExprKind::FloatLit(f) => Ok(ComptimeVal::Float(*f)),
            ExprKind::BoolLit(b) => Ok(ComptimeVal::Bool(*b)),

            ExprKind::IntType => Ok(ComptimeVal::Type(SemaTy::Int)),
            ExprKind::UintType => Ok(ComptimeVal::Type(SemaTy::Uint)),
            ExprKind::BoolType => Ok(ComptimeVal::Type(SemaTy::Bool)),
            ExprKind::VoidType => Ok(ComptimeVal::Type(SemaTy::Void)),

            _ => Err(ComptimeEvalError::UnsupportedExpr(expr.span)),
        }
    }

    pub fn eval_type(&self, expr_id: ExprId) -> Result<SemaTy, ComptimeEvalError> {
        match self.eval(expr_id)? {
            ComptimeVal::Type(ty) => Ok(ty),
            v => Err(ComptimeEvalError::ExpectedType(v.sema_type())),
        }
    }
}
