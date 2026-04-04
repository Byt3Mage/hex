use crate::{
    arena::Ident,
    compiler::{
        ast::PatternId,
        op::{BinOp, UnOp},
        sema::{ScopeId, SymbolId, sema_type::SemaTypeId, sema_value::ComptimeInt},
        tokens::Span,
    },
};

#[derive(Debug, Clone, thiserror::Error)]
#[error("Resolve error")]
pub enum ResolveError {
    DuplicateSymbol {
        name: Ident,
        first: Span,
        duplicate: Span,
    },
    DuplicateFieldDef {
        name: Ident,
        first: Span,
        duplicate: Span,
    },
    DuplicateVariantDef {
        name: Ident,
        first: Span,
        duplicate: Span,
    },
    DuplicateVariantVal {
        value: ComptimeInt,
        first: Span,
        duplicate: Span,
    },
    UndefinedSymbol {
        name: Ident,
        span: Span,
    },
    UndefinedField {
        name: Ident,
        span: Span,
    },
    SymbolNotInScope {
        scope: ScopeId,
        name: Ident,
        span: Span,
    },
    BreakOutsideLoop {
        span: Span,
    },
    ContinueOutsideLoop {
        span: Span,
    },
    ReturnOutsideFunction {
        span: Span,
    },
    RefutableVariablePattern {
        pattern_id: PatternId,
        expected_ty: SemaTypeId,
        span: Span,
    },
    NotDeclScope {
        span: Span,
    },
    ExpectedType {
        found: SymbolId,
        span: Span,
    },
    ExpectedValue {
        found: SymbolId,
        span: Span,
    },
    TypeMismatch {
        exp: SemaTypeId,
        got: SemaTypeId,
        span: Span,
    },
    CoerceFailed {
        tgt: SemaTypeId,
        val: SemaTypeId,
        span: Span,
    },
    InvalidEnumBaseType {
        span: Span,
    },
    UninitVariable {
        name: Ident,
        span: Span,
    },
    NegativeArrayLength {
        span: Span,
    },
    InvalidArrayLength {
        span: Span,
    },
    ExpectedStructOrUnion {
        ty: SemaTypeId,
        span: Span,
    },
    UnionFieldInit {
        num_fields: usize,
        span: Span,
    },
    DuplicateFieldInit {
        name: Ident,
        span: Span,
    },
    MissingFieldInit {
        ty: SemaTypeId,
        name: Ident,
        span: Span,
    },
    InvalidUnaryOp {
        op: UnOp,
        ty: SemaTypeId,
        span: Span,
    },
    InvalidBinaryOp {
        op: BinOp,
        lhs: SemaTypeId,
        rhs: SemaTypeId,
        span: Span,
    },
    ExpectedIndexable {
        found: SemaTypeId,
        span: Span,
    },
    ExpectedOptional {
        found: SemaTypeId,
        span: Span,
    },
    ExpectedCallable {
        ty: SemaTypeId,
        span: Span,
    },
}

macro_rules! bug {
    ($($arg:tt)*) => {
        panic!("INTERNAL COMPILER ERROR: {}", format!($($arg)*))
    };
}

pub(crate) use bug;
