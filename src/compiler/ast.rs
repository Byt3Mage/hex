use crate::{
    arena::{Arena, Ident, StrSymbol},
    compiler::{
        op::{AssignOp, BinOp, UnOp},
        sema::sema_value::ComptimeInt,
        tokens::Span,
    },
};

slotmap::new_key_type! {
    pub struct ExprId;
    pub struct StmtId;
    pub struct DeclId;
    pub struct PathId;
    pub struct AstTypeId;
    pub struct PatternId;
}

pub struct AstArena {
    pub exprs: Arena<ExprId, Expr>,
    pub stmts: Arena<StmtId, Stmt>,
    pub decls: Arena<DeclId, Decl>,
    pub paths: Arena<PathId, Path>,
    pub types: Arena<AstTypeId, AstType>,
    pub patterns: Arena<PatternId, Pattern>,
}

impl AstArena {
    pub fn new() -> Self {
        Self {
            exprs: Arena::with_key(),
            stmts: Arena::with_key(),
            decls: Arena::with_key(),
            paths: Arena::with_key(),
            types: Arena::with_key(),
            patterns: Arena::with_key(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    // Primitive Literals
    CintLit(ComptimeInt),
    UintLit(u64),
    IntLit(i64),
    FloatLit(f64),
    True,
    False,
    Char(char),
    StrLit(StrSymbol),
    Null,
    Void,

    Path(PathId),

    /// Array literal value for arrays and tuples.
    ///
    /// Example:
    /// ```
    /// [1, 2, 3], [69, "hello", false]
    /// ```
    ArrayLit(Vec<ExprId>),

    /// Array repeat syntax.
    ///
    /// Example:
    /// ```
    /// [1; 5]
    /// ```
    ArrayRepeat {
        value: ExprId,
        count: ExprId,
    },

    /// Struct literal value for both structs and unions
    ///
    /// Example:
    /// ```
    /// let anon = _{ x: 10, y: 20 };
    /// let point = Point{ x: 10, y: 30 };
    /// let result = Result{ ok: 42 };
    /// ```
    StructLit {
        ty: AstTypeId,
        fields: Vec<FieldInit>,
    },

    /// Grouped expression in parenthesis
    ///
    /// Example:
    /// ```
    /// ((x * y) + sqrt(5 % 9))
    /// ```
    Group(ExprId),

    Unary {
        op: UnOp,
        rhs: ExprId,
    },

    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },

    Assign {
        op: AssignOp,
        tgt: ExprId,
        val: ExprId,
    },

    /// Explicit value cast
    ///
    /// Example:
    /// ```
    /// 78 as uint, true as int
    /// ```
    Cast {
        expr: ExprId,
        ty: ExprId,
    },

    // Control flow
    If {
        cond: ExprId,
        then_branch: ExprId,
        else_branch: Option<ExprId>,
    },
    Match {
        scrutinee: ExprId,
        arms: Vec<MatchArm>,
    },
    While {
        cond: ExprId,
        body: ExprId,
    },
    Loop(ExprId),
    For {
        pattern: PatternId,
        iter: ExprId,
        body: ExprId,
    },

    Block(Vec<StmtId>),
    Return(Option<ExprId>),
    Break(Option<ExprId>),
    Continue,

    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },

    /// Field/method access on a value
    ///
    /// Example:
    /// ```
    /// let x = point.x;
    /// ```
    Field {
        object: ExprId,
        field: Ident,
    },

    /// Field access on optional value
    ///
    /// Example
    /// ```
    /// let x: ?int = point?.x;
    /// ```
    OptionalField {
        object: ExprId,
        field: Ident,
    },

    /// Indexed access on arrays/maps
    ///
    /// Example:
    /// ```
    /// let x: int = arr[0];
    /// arr[idx] = 78;
    /// ```
    Index {
        object: ExprId,
        index: ExprId,
    },

    /// Inclusive/exclusive signed or unsigned range
    ///
    /// E.g. let inc = 0..=10, let exc = 0..10
    Range {
        start: Option<ExprId>,
        end: Option<ExprId>,
        inclusive: bool,
    },

    /// Unwrap operation on optional value
    ///
    /// Example
    /// ```
    /// let x = point!.x
    /// ```
    Unwrap(ExprId),

    /// Expression to be evaluated at compile time
    ///
    /// Example
    /// ```
    /// let x = const fibonacci(n);
    /// ```
    Const(ExprId),
}

#[derive(Debug, Clone)]
pub struct FieldInit {
    pub name: Ident,
    pub value: ExprId,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub pattern: PatternId,
    pub ty: AstTypeId,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: PatternId,
    pub guard: Option<ExprId>,
    pub body: ExprId,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    // let x = 5;
    // let x: int = 5;
    // let mut x = 5;
    // let Point{mut x, y} = Point{x: 5, y: 6};
    Let {
        pattern: PatternId,
        ty: Option<AstTypeId>,
        value: ExprId,
    },

    // Expression with semicolon.
    Expr(ExprId),

    // Expression with semicolon.
    Semi(ExprId),

    // Empty statement (just a semicolon).
    Empty,
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PatternKind {
    Wildcard,

    // ..
    Rest,

    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    CStr(StrSymbol),

    Identifier {
        mutable: bool,
        name: Ident,
    },

    Path(PathId),

    // Point { x, y }
    // std::geo::Point { x, y, .. }
    // #{x, y}
    // Option<int>{ some: x }
    Struct {
        ty: ExprId,
        fields: Vec<FieldPattern>,
        rest: bool,
    },

    // #{}
    Tuple(Vec<PatternId>),

    // [.., last], [1, 2, 3]
    Array(Vec<PatternId>),

    //  Status::Active | Status::Inactive
    Or(Vec<PatternId>),

    // a..10, 0..=b
    Range {
        start: Option<ExprId>,
        end: Option<ExprId>,
        inclusive: bool,
    },
}

#[derive(Debug, Clone)]
pub struct FieldPattern {
    pub name: Ident,
    pub pattern: Option<PatternId>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub visibility: Visibility,
    pub name: Ident,
    pub kind: DeclKind,
    pub span: Span,
}

impl Decl {
    pub fn as_module(&self) -> Option<&[DeclId]> {
        match &self.kind {
            DeclKind::Module(module) => Some(module),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
}

#[derive(Debug, Clone)]
pub enum DeclKind {
    Module(Vec<DeclId>),

    Function {
        generics: Vec<GenericParam>,
        params: Vec<Param>,
        ret: Option<AstTypeId>,
        body: ExprId,
    },

    Const {
        ty: Option<AstTypeId>,
        value: ExprId,
    },

    Struct {
        generics: Vec<GenericParam>,
        fields: Vec<FieldDef>,
    },

    Union {
        generics: Vec<GenericParam>,
        fields: Vec<FieldDef>,
    },

    Enum {
        base: Option<AstTypeId>,
        variants: Vec<VariantDef>,
    },
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub visibility: Visibility,
    pub name: Ident,
    pub ty: AstTypeId,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct VariantDef {
    pub name: Ident,
    pub value: Option<ExprId>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Path {
    pub first: PathSegment,
    pub rest: Vec<PathSegment>,
    pub span: Span,
}

impl Path {
    pub fn is_simple(&self) -> bool {
        self.first.is_simple() && self.rest.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct PathSegment {
    pub name: Ident,
    pub generics: Vec<GenericArg>,
    pub span: Span,
}

impl PathSegment {
    pub fn is_simple(&self) -> bool {
        self.generics.is_empty()
    }
}

#[derive(Debug, Clone)]
pub enum GenericArg {
    Type(PathId),
    Const(ExprId),
}

#[derive(Debug, Clone)]
pub enum GenericParam {
    Type(Ident),
    Const { name: Ident, ty: PathId },
}

#[derive(Debug, Clone)]
pub struct AstType {
    pub kind: AstTypeKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum AstTypeKind {
    CInt,
    CStr,
    Int,
    Uint,
    Bool,
    Float,
    Char,
    Never,
    Str,
    Void,
    Inferred,
    Path(PathId),
    Tuple(Vec<AstTypeId>),
    Array {
        elem: AstTypeId,
        len: ExprId,
    },
    Slice(AstTypeId),
    Optional(AstTypeId),
    Pointer {
        mutable: bool,
        pointee: AstTypeId,
    },
    Function {
        params: Vec<AstTypeId>,
        ret: Option<AstTypeId>,
    },
}
