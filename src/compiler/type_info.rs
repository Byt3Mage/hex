use std::rc::Rc;

use ahash::AHashMap;

use crate::{
    arena::{Arena, Ident},
    compiler::{ast::DeclId, sema::ConstValue},
};

slotmap::new_key_type! {
    pub struct TypeId;
}

#[derive(Debug, Clone)]
pub enum TypeValue {
    // Primitives
    Type,
    Int,
    Uint,
    Float,
    Bool,
    Char,
    Str,
    Cstr,
    Null,
    Void,
    Never,

    // Compound
    Pointer { pointee: TypeId, mutable: bool },
    Optional(TypeId),
    Array { elem: TypeId, len: usize },
    Slice(TypeId),
    Tuple(Rc<[TypeId]>),

    // User-defined
    Struct(StructInfo),
    Union(UnionInfo),
    Enum(EnumInfo),
    Module(ModuleInfo),

    // Functions
    Function { params: Rc<[TypeId]>, ret: TypeId },

    // Placeholder for recursive types
    Incomplete,
}

type Name = Ident;

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: Option<Name>,
    pub decls: AHashMap<Name, DeclId>,
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: Option<Name>,
    pub fields: Rc<[FieldInfo]>,
}

#[derive(Debug, Clone)]
pub struct UnionInfo {
    pub name: Option<Name>,
    pub fields: Rc<[FieldInfo]>,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: Option<Name>,
    pub variants: Rc<[VariantInfo]>,
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: Name,
    pub ty: ConstValue,
}

#[derive(Debug, Clone)]
pub struct VariantInfo {
    pub name: Name,
    pub value: ConstValue, // resolved discriminant value
}

pub struct TypeArena {
    types: Arena<TypeId, TypeValue>,
}

impl TypeArena {
    pub fn new() -> Self {
        Self {
            types: slotmap::SlotMap::with_key(),
        }
    }

    pub fn insert(&mut self, ty: TypeValue) -> TypeId {
        self.types.insert(ty)
    }

    pub fn get(&self, id: TypeId) -> &TypeValue {
        &self.types[id]
    }

    pub fn get_mut(&mut self, id: TypeId) -> &mut TypeValue {
        &mut self.types[id]
    }
}
