use crate::{
    arena::{Arena, Ident, StrSymbol},
    compiler::{
        ast_op::{AssignOp, BinaryOp, UnaryOp},
        tokens::Span,
    },
};

slotmap::new_key_type! {
    pub struct ExprId;
    pub struct StmtId;
    pub struct DeclId;
    pub struct PatternId;
}

pub struct AstArena {
    pub exprs: Arena<ExprId, Expr>,
    pub stmts: Arena<StmtId, Stmt>,
    pub decls: Arena<DeclId, Decl>,
    pub patterns: Arena<PatternId, Pattern>,
}

impl AstArena {
    pub fn new() -> Self {
        Self {
            exprs: Arena::with_key(),
            stmts: Arena::with_key(),
            decls: Arena::with_key(),
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
    IntLit(u64),
    FloatLit(f64),
    True,
    False,
    Char(char),
    StrLit(StrSymbol),
    Null,
    Void,

    // Identifer
    Ident(Ident),

    // Array literals
    ArrayLit(Vec<ExprId>),
    ArrayRepeat {
        value: ExprId,
        count: ExprId,
    },

    /// Struct literal value for both structs and unions
    ///
    /// E.g. #{ x: 10, y: 20 }, Point{ x: 10, y: 30 }
    StructLit {
        ty: ExprId,
        fields: Vec<FieldInit>,
    },

    /// Tuple literal value. Similar to struct literals but without field names
    ///
    /// E.g. #{15, "hello"}
    TupleLit(Vec<ExprId>),

    /// Scope access to inner items. The LHS must always resolve to a type
    ///
    /// E.g. std::math::sqrt, Status::Active, #::Inactive
    ScopeAccess {
        ty: ExprId,
        item: Ident,
    },

    /// Grouped expression in parenthesis
    ///
    /// E.g ((x * y) + sqrt(5 % 9))
    Group(ExprId),

    Unary {
        op: UnaryOp,
        expr: ExprId,
    },
    Binary {
        op: BinaryOp,
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
    /// E.g. 78 as uint, true as int
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
    /// E.g. let x = point.x;
    Field {
        object: ExprId,
        field: Ident,
    },

    /// Field access on optional value
    ///
    /// E.g. let x: ?int = point?.x;
    OptionalField {
        object: ExprId,
        field: Ident,
    },

    /// Indexed access on arrays/maps
    ///
    /// E.g. let x: int = arr\[0\], arr\[idx\] = 78;
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
    /// E.g. let x = point!.x
    Unwrap(ExprId),

    /// Expression to be evaluated at compile time
    ///
    /// E.g. let x = const fibonacci(n);
    Const(ExprId),

    /// Module type literal
    ///
    /// E.g. mod { const PI: float = 3.14; }
    ModuleType(Vec<DeclId>),

    /// Struct type literal
    ///
    /// E.g. struct {x: int, y: int}
    StructType(Vec<AstField>),

    /// Union type literal
    ///
    /// E.g. union {x: int, y: int}
    UnionType(Vec<AstField>),

    /// Enum type literal
    ///
    /// E.g. enum {Active, Inactive}
    EnumType(Vec<AstVariant>),

    /// Array type literal
    ///
    /// E.g. [T; 69]
    ArrayType {
        elem: ExprId,
        size: ExprId,
    },

    /// Slice type literal
    ///
    /// E.g. [T]
    SliceType(ExprId),

    /// Pointer type literal
    ///
    /// E.g. @T, @mut T
    PointerType {
        mutable: bool,
        pointee: ExprId,
    },

    /// Optional type literal
    ///
    /// E.g. ?T
    OptionType(ExprId),

    /// Function type litersl
    ///
    /// E.g. fn(A, B) -> C
    FunctionType {
        params: Vec<ExprId>,
        ret: Option<ExprId>,
    },

    /// Wildcard/Inferred type literal - #
    ///
    /// E.g. let x: Point = #{x: 69, y: 42};
    WildcardType,
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
    pub ty: ExprId,
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
        ty: Option<ExprId>,
        value: ExprId,
    },

    Expr {
        expr: ExprId,
        has_semi: bool,
    },

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

    Identifier {
        mutable: bool,
        ident: Ident,
    },

    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    CStr(StrSymbol),

    // Point { x, y }
    // std::geo::Point { x, y, .. }
    // #{x, y}
    // Option(int){ some: x }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
}

#[derive(Debug, Clone)]
pub enum DeclKind {
    // fn foo<T>(x: T) -> T { ... }
    Function {
        params: Vec<Param>,
        ret: Option<ExprId>,
        body: ExprId,
    },

    // const PI: float = 3.14159;
    // const math = module { fn sqrt(x: uint) -> uint {...}}
    Const {
        ty: Option<ExprId>,
        value: ExprId,
    },
}

#[derive(Debug, Clone)]
pub struct AstField {
    pub visibility: Visibility,
    pub name: Ident,
    pub ty: ExprId,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AstVariant {
    pub name: Ident,
    pub value: Option<ExprId>,
    pub span: Span,
}
