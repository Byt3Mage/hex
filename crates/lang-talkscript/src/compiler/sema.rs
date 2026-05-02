use crate::{arena::Ident, compiler::ast::*};
use ahash::AHashMap;
use hex_mir::{FuncDef, Inst, Term, Ty, Val};

/// What a type expression evaluates to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemaTy {
    Int,
    Uint,
    Bool,
    Float,
    Void,
    Never,
    Type,
}

#[derive(Debug, Clone, Copy)]
pub enum CompVal {
    Int(i64),
    Uint(u64),
    Bool(bool),
    Type(SemaTy),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SemaVal(u32);

#[derive(Debug, Clone, Copy)]
pub enum ExprResult {
    Runtime(SemaVal, SemaTy),
    Comptime(CompVal, SemaTy),
    Typed(SemaTy),
}
