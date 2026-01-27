use crate::{
    arena::{Arena, Ident},
    compiler::{ast::DeclId, sema::sema_value::SemaValueId},
};

slotmap::new_key_type! {
    pub struct SemaTypeId;
}

#[derive(Debug)]
pub enum SemaType {
    // Compile time primitives
    CInt,
    CStr,

    // Primitives
    Int,
    Uint,
    Float,
    Bool,
    Char,
    Str,
    Null,
    Void,
    Never,
    IntRange,
    UintRange,

    // Compound
    Opt(SemaTypeId),
    Ptr {
        mutable: bool,
        pointee: SemaTypeId,
    },
    Array {
        elem: SemaTypeId,
        len: u64,
    },
    Slice(SemaTypeId),
    Tuple(Vec<SemaTypeId>),

    // User-defined
    Struct(StructInfo),
    Union(UnionInfo),
    Enum(EnumInfo),

    // Functions
    Function {
        params: Vec<SemaTypeId>,
        ret: SemaTypeId,
    },

    Infer,

    Resolving(DeclId),
}

type Name = Ident;

#[derive(Debug)]
pub struct StructInfo {
    pub name: Option<Name>,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone)]
pub struct UnionInfo {
    pub name: Option<Name>,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: Option<Name>,
    pub base: SemaTypeId,
    pub variants: Vec<VariantInfo>,
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: Name,
    pub ty: SemaTypeId,
}

#[derive(Debug, Clone)]
pub struct VariantInfo {
    pub name: Name,
    pub value: SemaValueId, // resolved discriminant value
}

pub struct TypeArena {
    types: Arena<SemaTypeId, SemaType>,

    pub cint: SemaTypeId,
    pub cstr: SemaTypeId,

    pub int: SemaTypeId,
    pub uint: SemaTypeId,
    pub float: SemaTypeId,
    pub bool: SemaTypeId,
    pub char: SemaTypeId,
    pub str: SemaTypeId,
    pub null: SemaTypeId,
    pub void: SemaTypeId,
    pub never: SemaTypeId,
    pub int_range: SemaTypeId,
    pub uint_range: SemaTypeId,
}

impl TypeArena {
    pub fn new() -> Self {
        let mut types = Arena::with_key();
        Self {
            cint: types.insert(SemaType::CInt),
            cstr: types.insert(SemaType::CStr),
            int: types.insert(SemaType::Int),
            uint: types.insert(SemaType::Uint),
            float: types.insert(SemaType::Float),
            bool: types.insert(SemaType::Bool),
            char: types.insert(SemaType::Char),
            str: types.insert(SemaType::Str),
            null: types.insert(SemaType::Null),
            void: types.insert(SemaType::Void),
            never: types.insert(SemaType::Never),
            int_range: types.insert(SemaType::IntRange),
            uint_range: types.insert(SemaType::UintRange),
            types,
        }
    }

    pub fn insert(&mut self, ty: SemaType) -> SemaTypeId {
        self.types.insert(ty)
    }

    pub fn get(&self, id: SemaTypeId) -> &SemaType {
        &self.types[id]
    }

    pub fn get_mut(&mut self, id: SemaTypeId) -> &mut SemaType {
        &mut self.types[id]
    }

    pub fn as_enum(&mut self, id: SemaTypeId) -> Option<&EnumInfo> {
        match &self.types[id] {
            SemaType::Enum(info) => Some(info),
            _ => None,
        }
    }
}
