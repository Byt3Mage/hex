use crate::{
    arena::{Arena, Ident, define_id},
    compiler::tokens::Span,
};

define_id!(ExprId);
define_id!(DeclId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    IntLit(i64),
    UintLit(u64),
    True,
    False,
    Ident(Ident),
    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    Unary {
        op: UnOp,
        rhs: ExprId,
    },
    Group(ExprId),
    Block(Vec<Stmt>),
    If {
        cond: ExprId,
        then: ExprId,
        else_: Option<ExprId>,
    },
    While {
        cond: ExprId,
        body: ExprId,
    },
    Loop(ExprId),
    Ret(Option<ExprId>),
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },

    TyInt,
    TyUint,
    TyBool,
    TyVoid,
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Let {
        name: Ident,
        ty: Option<ExprId>,
        value: ExprId,
    },
    Semi(ExprId),
    Expr(ExprId),
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Ident,
    pub ty: ExprId,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnDecl {}

#[derive(Debug, Clone)]
pub enum DeclKind {
    Mod(Vec<DeclId>),
    Fn {
        params: Vec<Param>,
        ret: Option<ExprId>,
        body: ExprId,
    },
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub vis: Visibility,
    pub name: Ident,
    pub kind: DeclKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
}

pub struct Ast {
    exprs: Arena<ExprId, Expr>,
    decls: Arena<DeclId, Decl>,
    strings: Vec<String>,
}

impl Ast {
    pub fn new() -> Self {
        Self {
            exprs: Arena::new(),
            decls: Arena::new(),
            strings: Vec::new(),
        }
    }

    pub fn expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id]
    }

    pub fn push_expr(&mut self, expr: Expr) -> ExprId {
        self.exprs.insert(expr)
    }

    pub fn decl(&self, id: DeclId) -> &Decl {
        &self.decls[id]
    }

    pub fn push_decl(&mut self, decl: Decl) -> DeclId {
        self.decls.insert(decl)
    }
}
