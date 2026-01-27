use crate::{
    arena::Ident,
    compiler::{
        ast::PatternId,
        sema::sema_type::SemaTypeId,
        sema_v2::{ScopeId, SymbolId},
        tokens::Span,
    },
};

#[derive(Debug, Clone)]
pub enum ResolveError {
    DuplicateSymbol {
        name: Ident,
        first_def: Span,
        dupe_def: Span,
    },
    DuplicateField {
        name: Ident,
        first_def: Span,
        dupe_def: Span,
    },
    DuplicateVariant {
        name: Ident,
        first_def: Span,
        dupe_def: Span,
    },
    UndefinedSymbol {
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
    RefutableParamPattern {
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
    InvalidEnumBase {
        span: Span,
    },
    UninitVariable {
        name: Ident,
        span: Span,
    },
    NegativeArrayLength {
        span: Span,
    },
}
