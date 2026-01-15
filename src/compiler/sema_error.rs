use crate::{arena::Ident, compiler::tokens::Span, compiler::type_info::TypeId};

#[derive(Debug, Clone)]
pub enum SemaError {
    TypeMismatch {
        exp: TypeId,
        got: TypeId,
        span: Span,
    },
    InvalidArrayLength {
        span: Span,
    },
    FieldNotFound {
        ty: TypeId,
        field: Ident,
        span: Span,
    },
    NotCallable {
        ty: TypeId,
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
        first: TypeId,
        second: TypeId,
        span: Span,
    },
    NotImplemented {
        span: Span,
    },
    Undefined {
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
    TypeUsedAsValue {
        span: Span,
    },
    ItemNotFound {
        module: TypeId,
        item_name: Ident,
        span: Span,
    },
}
