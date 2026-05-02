use crate::{
    arena::{Arena, Ident, define_id},
    compiler::tokens::Span,
};

define_id!(ExprId);
define_id!(DeclId);

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    // Primitive Literals
    // CintLit(ComptimeInt),
    UintLit(u64),
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    NullLit,
    VoidLit,

    // Identifiers
    Ident(Ident),

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
        ty: ExprId,
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
    While {
        cond: ExprId,
        body: ExprId,
    },
    Loop(ExprId),

    Block(Vec<Stmt>),
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

    /// Expression to be evaluated at compile time
    ///
    /// Example
    /// ```
    /// let x = comptime fibonacci(n);
    /// ```
    Comptime(ExprId),

    IntType,
    UintType,
    BoolType,
    VoidType,
}

#[derive(Debug, Clone)]
pub struct FieldInit {
    pub name: Ident,
    pub value: ExprId,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub is_comptime: bool,
    pub name: Ident,
    pub ty: ExprId,
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
        name: Ident,
        ty: Option<ExprId>,
        value: ExprId,
    },

    // Expression with semicolon.
    Expr(ExprId),

    // Expression with semicolon.
    Semi(ExprId),
}

#[derive(Debug, Clone, Copy)]
pub enum Visibility {
    Public,
    Private,
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub vis: Visibility,
    pub name: Ident,
    pub kind: DeclKind,
    pub span: Span,
}

impl Decl {
    pub fn as_module(&self) -> Option<&[DeclId]> {
        match &self.kind {
            DeclKind::Mod(module) => Some(module),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum DeclKind {
    Mod(Vec<DeclId>),
    Func {
        params: Vec<Param>,
        ret: Option<ExprId>,
        body: ExprId,
    },
    Const {
        ty: Option<ExprId>,
        val: ExprId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssignOp {
    Eq,
    AddEq,
    SubEq,
    MulEq,
    DivEq,
    ModEq,
    BitAndEq,
    BitOrEq,
    BitXorEq,
    ShlEq,
    ShrEq,
}

pub struct Ast {
    exprs: Arena<ExprId, Expr>,
    decls: Arena<DeclId, Decl>,
}

impl Ast {
    pub fn new() -> Self {
        Self {
            exprs: Arena::new(),
            decls: Arena::new(),
        }
    }

    #[inline]
    pub fn expr(&self, id: ExprId) -> &Expr {
        self.exprs.get(id)
    }

    #[inline]
    pub fn decl(&self, id: DeclId) -> &Decl {
        self.decls.get(id)
    }

    #[inline]
    pub fn insert_expr(&mut self, expr: Expr) -> ExprId {
        self.exprs.insert(expr)
    }

    #[inline]
    pub fn insert_decl(&mut self, decl: Decl) -> DeclId {
        self.decls.insert(decl)
    }
}
