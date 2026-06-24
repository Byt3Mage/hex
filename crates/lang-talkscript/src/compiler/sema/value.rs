use crate::compiler::{
    ast::DeclId,
    sema::{
        eval::EvalError,
        types::{TypeArena, TypeId},
    },
};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Uint(u64),
    Bool(bool),
    Float(f64),
    Void,
    Type(TypeId),
    Fn(DeclId),
}

impl Value {
    /// The TYPE of this value, as a TypeId. Needed when the checker
    /// asks "what type does this comptime value have?".
    /// Note: the type of `Value::Type(_)` is `Type` itself.
    pub fn type_of(&self, types: &TypeArena) -> TypeId {
        match self {
            Value::Int(_) => types.int(),
            Value::Uint(_) => types.uint(),
            Value::Bool(_) => types.bool(),
            Value::Float(_) => types.float(),
            Value::Void => types.void(),
            Value::Type(_) => types.type_(),
            Value::Fn(_) => unreachable!("fn value typing handled separately in phase one"),
        }
    }

    /// Coerce this value to a TypeId, erroring if it isn't a type.
    /// Used wherever an expression is in *type position* (annotations,
    /// return types). This is the single choke point where "is this
    /// actually a type?" is enforced.
    pub fn as_type(&self) -> Result<TypeId, EvalError> {
        match self {
            Value::Type(id) => Ok(*id),
            other => Err(EvalError::NotAType {
                found: other.clone(),
            }),
        }
    }
}
