//! Phase-one driver: walk a module, evaluate consts and fn signatures,
//! then typecheck fn bodies. Detects cyclic const definitions.

use crate::arena::Ident;
use crate::compiler::ast::{Ast, DeclId, DeclKind, ExprId};
use crate::compiler::sema::eval::{Binding, EvalError};
use crate::compiler::sema::type_check::{TypeCheckError, TypeChecker};
use crate::compiler::sema::types::{TypeArena, TypeId};
use crate::compiler::sema::value::Value;

use std::collections::HashMap;

pub mod eval;
pub mod type_check;
pub mod types;
pub mod value;

#[derive(Debug, thiserror::Error)]
pub enum SemaError {
    #[error("eval: {0}")]
    Eval(#[from] EvalError),
    #[error("check: {0}")]
    Check(#[from] TypeCheckError),
}

#[derive(Debug, Clone)]
enum DeclState {
    Const { state: ConstState, decl: DeclId },
    Func { sig: Option<TypeId>, decl: DeclId },
}

#[derive(Debug, Clone)]
enum ConstState {
    Pending,
    InProgress,
    Evaluated(Value),
}

pub struct Sema<'a> {
    ast: &'a Ast,
    types: TypeArena,
    decls: HashMap<Ident, DeclState>,
}

impl<'a> Sema<'a> {
    pub fn new(ast: &'a Ast) -> Self {
        Self {
            ast,
            types: TypeArena::new(),
            decls: HashMap::new(),
        }
    }

    pub fn register_decls(&mut self, decls: &[DeclId]) {
        for &id in decls {
            let d = self.ast.decl(id);
            match d.kind {
                DeclKind::Func { .. } => self.decls.insert(
                    d.name,
                    DeclState::Func {
                        sig: None,
                        decl: id,
                    },
                ),
                DeclKind::Const { .. } => self.decls.insert(
                    d.name,
                    DeclState::Const {
                        state: ConstState::Pending,
                        decl: id,
                    },
                ),
            };
        }
    }

    fn eval(&mut self, expr_id: ExprId) -> Result<Value, SemaError> {
        todo!()
    }
}
