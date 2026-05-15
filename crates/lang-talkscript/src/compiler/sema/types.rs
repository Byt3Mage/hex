use crate::{
    arena::{Ident, define_id},
    compiler::{error::bug, ir},
};

define_id!(TypeId);
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Type,
    Int,
    Uint,
    Bool,
    Float,
    Void,
    Null,
    Never,
    Optional(Box<Ty>),
    Array { elem_ty: Box<Ty>, len: usize },
    Pointer { pointee: Box<Ty>, is_mut: bool },
    Struct(StructTy),
    Union(UnionTy),
    Enum(EnumTy),
    Infer,
}

impl Ty {
    #[inline(always)]
    pub fn array(elem_ty: Self, len: usize) -> Self {
        Ty::Array {
            elem_ty: Box::new(elem_ty),
            len,
        }
    }

    #[inline(always)]
    pub fn option(ty: Self) -> Self {
        Ty::Optional(Box::new(ty))
    }

    pub fn size(&self) -> usize {
        match self {
            Ty::Void | Ty::Never => 0,
            Ty::Int | Ty::Uint | Ty::Bool | Ty::Float | Ty::Pointer { .. } => 1,
            Ty::Optional(inner) => 1 + inner.size(),
            Ty::Array { elem_ty, len } => len * elem_ty.size(),
            Ty::Struct(ty) => ty.size(),
            Ty::Union(ty) => ty.size(),
            Ty::Enum(ty) => ty.size(),
            Ty::Type | Ty::Infer | Ty::Null => {
                bug!("{self} should never reach width-querying context")
            }
        }
    }

    pub fn to_ir(&self) -> Vec<ir::Ty> {
        match self {
            Ty::Void | Ty::Never => vec![],
            Ty::Int => vec![ir::Ty::Int],
            Ty::Uint => vec![ir::Ty::Uint],
            Ty::Bool => vec![ir::Ty::Bool],
            Ty::Float => vec![ir::Ty::Float],
            Ty::Optional(inner) => {
                let mut ty = vec![ir::Ty::Bool];
                ty.extend(inner.to_ir());
                ty
            }
            Ty::Array { elem_ty, len } => {
                let elem = elem_ty.to_ir();
                let mut arr_ty = Vec::with_capacity(elem.len() * len);
                (0..*len).for_each(|_| arr_ty.extend(&elem));
                arr_ty
            }
            Ty::Pointer { .. } => todo!("pointers not supported in ir"),
            Ty::Struct(s) => s.fields.iter().map(|f| f.ty.to_ir()).flatten().collect(),
            Ty::Union(ty) => vec![ir::Ty::Uint; ty.size()],
            Ty::Enum(ty) => ty.base.to_ir(),
            Ty::Type | Ty::Null | Ty::Infer => {
                bug!("{self} must be coerced or realized before ir type lowering")
            }
        }
    }
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Type => f.write_str("type"),
            Ty::Int => f.write_str("int"),
            Ty::Uint => f.write_str("uint"),
            Ty::Bool => f.write_str("bool"),
            Ty::Float => f.write_str("float"),
            Ty::Void => f.write_str("void"),
            Ty::Null => f.write_str("null"),
            Ty::Never => f.write_str("!"),
            Ty::Optional(inner) => write!(f, "?{inner}"),
            Ty::Array { elem_ty, len } => write!(f, "[{elem_ty}; {len}]"),
            Ty::Pointer { pointee, is_mut } => {
                if *is_mut {
                    write!(f, "&mut {}", pointee)
                } else {
                    write!(f, "&{}", pointee)
                }
            }
            Ty::Struct(ty) => {
                let name = match ty.name {
                    Some(n) => &format!("{n:?}"),
                    None => "<anon>",
                };
                write!(f, "{}", name)
            }
            Ty::Union(ty) => {
                let name = match ty.name {
                    Some(n) => &format!("{n:?}"),
                    None => "<anon>",
                };
                write!(f, "{}", name)
            }
            Ty::Enum(ty) => {
                let name = match ty.name {
                    Some(n) => &format!("{n:?}"),
                    None => "<anon>",
                };
                write!(f, "{}", name)
            }
            Ty::Infer => f.write_str("{unknown}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: Ident,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub name: Ident,
    pub tag: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructTy {
    pub name: Option<Ident>,
    pub fields: Vec<Field>,
}

impl StructTy {
    pub fn size(&self) -> usize {
        self.fields.iter().map(|f| f.ty.size()).sum()
    }

    pub fn field(&self, name: Ident) -> Option<(usize, &Field)> {
        self.fields.iter().enumerate().find(|(_, f)| f.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnionTy {
    pub name: Option<Ident>,
    pub fields: Vec<Field>,
}

impl UnionTy {
    pub fn size(&self) -> usize {
        self.fields
            .iter()
            .map(|f| f.ty.size())
            .max()
            .map_or(0, |m| m + 1)
    }

    pub fn field(&self, name: Ident) -> Option<(usize, &Field)> {
        self.fields.iter().enumerate().find(|(_, f)| f.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumTy {
    pub name: Option<Ident>,
    pub base: Box<Ty>,
    pub variants: Vec<Variant>,
}

impl EnumTy {
    pub fn size(&self) -> usize {
        self.base.size()
    }

    pub fn variant(&self, name: Ident) -> Option<&Variant> {
        self.variants.iter().find(|v| v.name == name)
    }
}
