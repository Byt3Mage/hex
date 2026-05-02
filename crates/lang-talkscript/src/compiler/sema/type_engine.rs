#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemaTy {
    Type,
    Int,
    Uint,
    Bool,
    Float,
    Void,
    Never,
    Pointer(Box<SemaTy>),
    Array(Box<SemaTy>, usize),
}

impl SemaTy {
    pub fn width(&self) -> usize {
        match self {
            SemaTy::Void | SemaTy::Never | SemaTy::Type => 0,
            SemaTy::Int | SemaTy::Uint | SemaTy::Bool | SemaTy::Float | SemaTy::Pointer(_) => 1,
            SemaTy::Array(ty, len) => len * ty.width(),
        }
    }
}
