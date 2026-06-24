use ahash::AHashMap;

use crate::{
    arena::{define_id, Arena, Ident},
    compiler::{
        ast::{BinOp, ExprId, UnOp},
        sema::{
            types::{TypeArena, TypeId},
            value::Value,
        },
        token::Span,
    },
};

define_id!(TExprId);
define_id!(TBindingId);
define_id!(TVarId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TyRef {
    Concrete(TypeId),
    Var(TVarId),
}

#[derive(Debug)]
pub struct TBinding {
    pub name: Ident,
    pub ty: TyRef,
    pub mutable: bool,
    pub kind: TBindingKind,
}

#[derive(Debug)]
pub enum TBindingKind {
    Param,
    Let,
}

#[derive(Debug)]
pub struct TExpr {
    pub kind: TExprKind,
    pub ty: TyRef,
    pub origin: ExprId,
}

#[derive(Debug)]
pub enum TExprKind {
    IntLit(i64),
    UintLit(u64),
    BoolLit(bool),
    FloatLit(f64),
    Void,

    Local(TBindingId),

    Const(Ident),

    Unary {
        op: UnOp,
        rhs: TExprId,
    },
    Binary {
        op: BinOp,
        lhs: TExprId,
        rhs: TExprId,
    },

    Block(Vec<TStmt>),

    If {
        cond: TExprId,
        then_branch: TExprId,
        else_branch: Option<TExprId>,
    },

    Return(Option<TExprId>),

    Call {
        callee: Callee,
        args: Vec<TExprId>,
    },
}

#[derive(Debug)]
pub enum Callee {
    Direct(Ident),
    // TODO: Instance(InstanceId) for a monomorphized generic.
}

#[derive(Debug)]
pub struct TStmt {
    pub kind: TStmtKind,
    pub origin: Span,
}

#[derive(Debug)]
pub enum TStmtKind {
    Let { binding: TBindingId, value: TExprId },
    Semi(TExprId),
    Expr(TExprId),
}

#[derive(Debug)]
pub struct TFunction {
    pub name: Ident,
    pub params: Vec<TBindingId>,
    pub ret: TypeId,
    pub body: TExprId,
    pub exprs: Arena<TExprId, TExpr>,
    pub bindings: Arena<TBindingId, TBinding>,
}

#[derive(Debug)]
pub struct TProgram {
    pub types: TypeArena,
    pub constants: AHashMap<Ident, Value>,
    pub functions: AHashMap<Ident, TFunction>,
}
