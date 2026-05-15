//! Typed AST: AST with resolved types, ready for MIR lowering.
//!
//! Produced by the AST→TAST walker. After constraint solving and defaulting,
//! every `TyRef` in the tree is resolved to a concrete `Ty`.

use super::types::Ty;
use crate::{
    arena::{Arena, Ident, define_id},
    compiler::{ast::ExprId, token::Span},
};

/// Identifies an inference variable owned by `InferCtx`.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct InferVarId(pub u32);

/// A type reference: either a concrete type or an unresolved inference variable.
///
/// After constraint solving + defaulting, every `TyRef` in a finished TAST
/// should be `Concrete(_)`. The MIR lowering pass treats `Var(_)` as a bug.
#[derive(Clone, Debug)]
pub enum TyRef {
    Concrete(Ty),
    Var(InferVarId),
}

define_id!(TBindingId);
#[derive(Debug)]
pub struct TBinding {
    pub name: Ident,
    pub ty: TyRef,
    pub mutable: bool,
    pub kind: BindingKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum BindingKind {
    /// A runtime binding from a `let` statement or function parameter.
    Runtime,
    /// A compile-time binding from a `const` statement.
    /// The value is computed during AST→TAST via `ComptimeEval`.
    Comptime,
}

define_id!(TExprId);
#[derive(Debug)]
pub struct TExpr {
    pub kind: TExprKind,
    pub ty: TyRef,
    pub span: Span,
    pub origin: ExprId,
}

#[derive(Debug)]
pub enum TExprKind {
    /// Unsuffixed integer literal (`5`). Type is `Var(_)` until inference resolves it.
    CintLit(u64),
    /// Suffixed integer literal (`5i`). Type is always `Concrete(Int)`.
    IntLit(i64),
    /// Suffixed unsigned literal (`5u`). Type is always `Concrete(Uint)`.
    UintLit(u64),
    /// Boolean literal. Type is always `Concrete(Bool)`.
    BoolLit(bool),
    /// Float literal. Type is always `Concrete(Float)`.
    FloatLit(f64),
    /// Reference to a binding (variable, parameter).
    Ident(TBindingId),
    /// A block of statements ending in an optional final expression.
    /// The block's type is the final expression's type, or `Void` if none.
    Block(Vec<TStmt>),
    /// Early return from the function.
    Return(Option<TExprId>),
}

#[derive(Debug)]
pub struct TStmt {
    pub kind: TStmtKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum TStmtKind {
    /// `let x = value;` or `let x: T = value;`
    /// The binding's type is recorded in `TBinding`.
    Let { binding: TBindingId, value: TExprId },
    /// `expr;`. Value is discarded.
    Semi(TExprId),
    /// `expr` (last in block, no semicolon). Value becomes the block's value.
    Expr(TExprId),
}

#[derive(Debug)]
pub struct TFunction {
    pub name: String,
    /// Function parameters, as `BindingId`s into `bindings`.
    pub params: Vec<TBindingId>,
    /// Concrete return types. Resolved before the body is walked.
    pub ret_type: Ty,
    /// All bindings created in this function (params, lets, consts).
    pub bindings: Arena<TBindingId, TBinding>,
    /// Arena of all expression nodes in this function.
    pub exprs: Arena<TExprId, TExpr>,
    /// The body expression — typically a block.
    pub body: TExprId,
}

impl TFunction {
    #[inline(always)]
    pub fn binding(&self, id: TBindingId) -> &TBinding {
        self.bindings.get(id)
    }

    #[inline(always)]
    pub fn expr(&self, id: TExprId) -> &TExpr {
        self.exprs.get(id)
    }
}

pub enum Constriant {
    /// Two types must be equal. Used for binary op operands and other
    /// places where types must match exactly.
    Equal { a: TyRef, b: TyRef, span: Span },
    /// `from` must coerce to `to`. Used for let-bindings with annotations,
    /// function args, return values. Allows narrow conversions like
    /// CintVar → Int or CintVar → Uint.
    Coerce { from: TyRef, to: TyRef, span: Span },
}
