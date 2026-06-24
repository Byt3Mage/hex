use crate::arena::{Arena, define_id};

define_id!(TypeId);
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeVal {
    Type,
    Int,
    Uint,
    Bool,
    Float,
    Void,
    Null,
    Never,
    Optional(TypeId),
    Array { elem: TypeId, len: u64 },
    Fn { params: Vec<TypeId>, ret: TypeId },
}

#[derive(Debug)]
pub struct TypeArena {
    arena: Arena<TypeId, TypeVal>,
    int_type: TypeId,
    uint_type: TypeId,
    bool_type: TypeId,
    float_type: TypeId,
    void_type: TypeId,
    type_type: TypeId,
}

impl TypeArena {
    pub fn new() -> Self {
        let mut arena = Arena::new();
        let int_type = arena.insert(TypeVal::Int);
        let uint_type = arena.insert(TypeVal::Uint);
        let bool_type = arena.insert(TypeVal::Bool);
        let float_type = arena.insert(TypeVal::Float);
        let void_type = arena.insert(TypeVal::Void);
        let type_type = arena.insert(TypeVal::Type);
        Self {
            arena,
            int_type,
            uint_type,
            bool_type,
            float_type,
            void_type,
            type_type,
        }
    }

    #[inline]
    pub fn get(&self, id: TypeId) -> &TypeVal {
        self.arena.get(id)
    }

    /// Intern a compound type (Fn, and later struct/union/etc).
    /// Phase one: linear dedup is fine. Swap for a hashmap when
    /// the arena grows — the call sites won't change.
    pub fn intern(&mut self, t: TypeVal) -> TypeId {
        if let Some(id) = self.arena.iter().find(|(_, v)| **v == t).map(|(id, _)| id) {
            return id;
        }
        self.arena.insert(t)
    }

    // convenience accessors
    pub fn int(&self) -> TypeId {
        self.int_type
    }
    pub fn uint(&self) -> TypeId {
        self.uint_type
    }
    pub fn bool(&self) -> TypeId {
        self.bool_type
    }
    pub fn float(&self) -> TypeId {
        self.float_type
    }
    pub fn void(&self) -> TypeId {
        self.void_type
    }
    pub fn type_(&self) -> TypeId {
        self.type_type
    }

    pub fn optional(&mut self, inner: TypeId) -> TypeId {
        self.intern(TypeVal::Optional(inner))
    }
    pub fn array(&mut self, elem: TypeId, len: u64) -> TypeId {
        self.intern(TypeVal::Array { elem, len })
    }
    pub fn func(&mut self, params: Vec<TypeId>, ret: TypeId) -> TypeId {
        self.intern(TypeVal::Fn { params, ret })
    }
}
