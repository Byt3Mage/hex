use crate::{arena::Ident, compiler::error::bug};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemaTy {
    Type,
    Int,
    Uint,
    Bool,
    Float,
    Void,
    Null,
    Never,
    Optional(Box<SemaTy>),
    Array { elem_ty: Box<SemaTy>, len: usize },
    Pointer { pointee: Box<SemaTy>, is_mut: bool },
    Struct(StructTy),
    Union(UnionTy),
    Infer,
}

impl SemaTy {
    pub fn width(&self) -> usize {
        match self {
            SemaTy::Void | SemaTy::Null | SemaTy::Never | SemaTy::Type => 0,
            SemaTy::Int | SemaTy::Uint | SemaTy::Bool | SemaTy::Float | SemaTy::Pointer { .. } => 1,
            SemaTy::Optional(inner) => 1 + inner.width(),
            SemaTy::Array { elem_ty, len } => len * elem_ty.width(),
            SemaTy::Struct(ty) => ty.width(),
            SemaTy::Union(ty) => ty.width(),
            SemaTy::Infer => bug!("can not find width for unknown type"),
        }
    }
}

impl std::fmt::Display for SemaTy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SemaTy::Type => f.write_str("type"),
            SemaTy::Int => f.write_str("int"),
            SemaTy::Uint => f.write_str("uint"),
            SemaTy::Bool => f.write_str("bool"),
            SemaTy::Float => f.write_str("float"),
            SemaTy::Void => f.write_str("void"),
            SemaTy::Null => f.write_str("null"),
            SemaTy::Never => f.write_str("!"),
            SemaTy::Optional(inner) => write!(f, "?{inner}"),
            SemaTy::Array { elem_ty, len } => write!(f, "[{elem_ty}; {len}]"),
            SemaTy::Pointer { pointee, is_mut } => {
                if *is_mut {
                    write!(f, "&mut {}", pointee)
                } else {
                    write!(f, "&{}", pointee)
                }
            }
            SemaTy::Struct(ty) => {
                let name = match ty.name {
                    Some(n) => &format!("{n:?}"),
                    None => "<anon>",
                };
                write!(f, "{}", name)
            }
            SemaTy::Union(ty) => {
                let name = match ty.name {
                    Some(n) => &format!("{n:?}"),
                    None => "<anon>",
                };
                write!(f, "{}", name)
            }
            SemaTy::Infer => f.write_str("{unknown}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: Ident,
    pub ty: SemaTy,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructTy {
    pub name: Option<Ident>,
    pub fields: Vec<Field>,
}

impl StructTy {
    pub fn width(&self) -> usize {
        self.fields.iter().map(|f| f.ty.width()).sum()
    }

    pub fn field(&self, name: Ident) -> Result<&Field, TypeError> {
        self.fields
            .iter()
            .find(|f| f.name == name)
            .ok_or_else(|| TypeError::FieldNotFound(name))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnionTy {
    pub name: Option<Ident>,
    pub fields: Vec<Field>,
}

impl UnionTy {
    pub fn width(&self) -> usize {
        self.fields
            .iter()
            .map(|f| f.ty.width())
            .max()
            .map_or(0, |m| m + 1)
    }

    pub fn field(&self, name: Ident) -> Result<&Field, TypeError> {
        self.fields
            .iter()
            .find(|f| f.name == name)
            .ok_or_else(|| TypeError::FieldNotFound(name))
    }
}

#[derive(Debug, thiserror::Error, Copy, Clone, PartialEq, Eq)]
pub enum TypeError {
    #[error("field {0:?} not found")]
    FieldNotFound(Ident),
}
