use crate::{
    arena::Ident,
    compiler::{
        ast::PatternId,
        ast_op::UnaryOp,
        name_resolver::{ScopeId, SymbolId},
        sema::sema_type::SemaTypeId,
        tokens::Span,
    },
};

pub type SemaResult<T> = Result<T, SemaError>;

#[derive(Debug, Clone)]
pub enum SemaError {
    TypeMismatch {
        exp: SemaTypeId,
        got: SemaTypeId,
        span: Span,
    },
    InvalidArrayLength {
        span: Span,
    },
    NotCallable {
        ty: SemaTypeId,
        span: Span,
    },
    DuplicateEnumValue {
        name: Ident,
        value: i64,
        first_def: Span,
        dupe_def: Span,
    },
    CannotInfer {
        span: Span,
    },

    NotDeclScope {
        span: Span,
    },
    InvalidConstOp {
        span: Span,
    },
    DivisionByZero {
        span: Span,
    },
    TypeJoinInvalid {
        first: SemaTypeId,
        second: SemaTypeId,
        span: Span,
    },
    NotImplemented {
        span: Span,
    },
    UndefinedSymbol {
        name: Ident,
        span: Span,
    },
    UninitVariable {
        name: Ident,
        span: Span,
    },
    CycleDetected {
        name: Ident,
        span: Span,
    },
    InvalidAssignment {
        name: Ident,
        span: Span,
    },
    SymbolNotInScope {
        scope: ScopeId,
        name: Ident,
        span: Span,
    },
    VariantNotFound {
        ty: SemaTypeId,
        name: Ident,
        span: Span,
    },
    FieldNotFound {
        ty: SemaTypeId,
        name: Ident,
        span: Span,
    },
    InvalidUnaryOp {
        op: UnaryOp,
        ty: SemaTypeId,
        span: Span,
    },
    TypeKindMismatch {
        exp: &'static str,
        got: SemaTypeId,
        span: Span,
    },
    ExpectedValue {
        found: SymbolId,
        span: Span,
    },
    ExpectedType {
        found: SymbolId,
        span: Span,
    },
    CoerceFailed {
        from: SemaTypeId,
        to: SemaTypeId,
    },
    RefutableParamPattern {
        pattern_id: PatternId,
        expected_ty: SemaTypeId,
        span: Span,
    },
    InvalidEnumBase {
        base_id: SemaTypeId,
        span: Span,
    },
    IntLitOveflow {
        val: i64,
        span: Span,
    },
}
