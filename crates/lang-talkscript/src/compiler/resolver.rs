use std::collections::hash_map::Entry;

use ahash::AHashMap;

use crate::{
    arena::{Arena, Ident, Interner, define_id},
    compiler::{
        ast::{Ast, DeclId, DeclKind, ExprId, ExprKind, Stmt, StmtKind},
        error::bug,
        token::Span,
    },
};

define_id!(SymbolId);
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: Ident,
    pub kind: SymbolKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Var(bool),
    Mod(DeclId),
    Func(DeclId),
    Const(DeclId),
}

define_id!(ScopeId);
#[derive(Debug, Clone)]
pub struct Scope {
    pub kind: ScopeKind,
    pub parent: Option<ScopeId>,
    pub symbols: AHashMap<Ident, SymbolId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Module,
    Function,
    Block,
    Loop,
}

impl Scope {
    pub fn new(kind: ScopeKind, parent: Option<ScopeId>) -> Self {
        Self {
            kind,
            parent,
            symbols: AHashMap::new(),
        }
    }
}

pub struct Lookup {
    pub symbol: SymbolId,
    pub scope: ScopeId,
}

pub struct SymbolTable<'a> {
    pub ast: &'a Ast,
    pub intern: &'a mut Interner,
    pub scopes: Arena<ScopeId, Scope>,
    pub symbols: Arena<SymbolId, Symbol>,
    pub decl_scopes: AHashMap<DeclId, ScopeId>,
    pub expr_scopes: AHashMap<ExprId, ScopeId>,
}

impl<'a> SymbolTable<'a> {
    pub fn new(ast: &'a Ast, intern: &'a mut Interner) -> Self {
        Self {
            ast,
            intern,
            scopes: Arena::new(),
            symbols: Arena::new(),
            decl_scopes: AHashMap::new(),
            expr_scopes: AHashMap::new(),
        }
    }

    pub fn scope(&mut self, kind: ScopeKind, parent: Option<ScopeId>) -> ScopeId {
        self.scopes.insert(Scope::new(kind, parent))
    }

    pub fn get(&self, symbol_id: SymbolId) -> &Symbol {
        self.symbols.get(symbol_id)
    }

    pub fn get_mut(&mut self, symbol_id: SymbolId) -> &mut Symbol {
        self.symbols.get_mut(symbol_id)
    }

    pub fn lookup_local(&self, name: Ident, scope_id: ScopeId) -> Option<SymbolId> {
        self.scopes.get(scope_id).symbols.get(&name).copied()
    }

    pub fn lookup(&self, name: Ident, mut scope: ScopeId) -> Option<Lookup> {
        loop {
            let s = self.scopes.get(scope);
            match s.symbols.get(&name) {
                Some(&symbol) => {
                    return Some(Lookup { symbol, scope });
                }
                None => scope = s.parent?,
            }
        }
    }

    pub fn define(&mut self, in_scope: ScopeId, symbol: Symbol) -> Result<SymbolId, ResolveError> {
        let scope = self.scopes.get_mut(in_scope);

        match scope.symbols.entry(symbol.name) {
            Entry::Vacant(entry) => Ok(*entry.insert(self.symbols.insert(symbol))),
            Entry::Occupied(e) => Err(ResolveError::DuplicateSymbol {
                name: symbol.name,
                first: self.symbols.get(*e.get()).span,
                duplicate: symbol.span,
            }),
        }
    }

    pub fn find_enclosing(&self, mut scope: ScopeId, kind: ScopeKind) -> Option<ScopeId> {
        loop {
            let s = &self.scopes.get(scope);
            if s.kind == kind {
                return Some(scope);
            }
            scope = s.parent?;
        }
    }

    pub fn find_loop_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.find_enclosing(scope_id, ScopeKind::Loop)
    }

    pub fn find_function_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.find_enclosing(scope_id, ScopeKind::Function)
    }

    fn register_decl(&mut self, in_scope: ScopeId, decl_id: DeclId) -> Result<(), ResolveError> {
        let decl = self.ast.decl(decl_id);

        match &decl.kind {
            DeclKind::Mod(decls) => {
                self.define(
                    in_scope,
                    Symbol {
                        name: decl.name,
                        kind: SymbolKind::Mod(decl_id),
                        span: decl.span,
                    },
                )?;
                let scope = self.scope(ScopeKind::Module, Some(in_scope));
                self.decl_scopes.insert(decl_id, scope);
                decls
                    .iter()
                    .try_for_each(|&id| self.register_decl(scope, id))?;
            }
            DeclKind::Func { .. } => {
                self.define(
                    in_scope,
                    Symbol {
                        name: decl.name,
                        kind: SymbolKind::Func(decl_id),
                        span: decl.span,
                    },
                )?;
                let scope = self.scope(ScopeKind::Function, Some(in_scope));
                self.decl_scopes.insert(decl_id, scope);
            }
            DeclKind::Const { .. } => {
                self.define(
                    in_scope,
                    Symbol {
                        name: decl.name,
                        kind: SymbolKind::Const(decl_id),
                        span: decl.span,
                    },
                )?;
            }
        }

        Ok(())
    }

    fn resolve_decl(&mut self, in_scope: ScopeId, decl_id: DeclId) -> Result<(), ResolveError> {
        let decl = self.ast.decl(decl_id);

        match &decl.kind {
            DeclKind::Mod(decls) => {
                let scope = self.decl_scopes[&decl_id];
                for &id in decls {
                    self.resolve_decl(scope, id)?;
                }
            }
            DeclKind::Func { params, ret, body } => {
                let scope = self.decl_scopes[&decl_id];

                for param in params {
                    self.resolve_expr(scope, param.ty)?;
                    self.define(
                        scope,
                        Symbol {
                            name: param.name,
                            kind: SymbolKind::Var(param.mutable),
                            span: param.span,
                        },
                    )?;
                }

                if let Some(ret) = ret {
                    self.resolve_expr(scope, *ret)?;
                }

                self.resolve_expr(scope, *body)?;
            }
            DeclKind::Const { ty, val } => todo!(),
        }

        Ok(())
    }

    fn resolve_expr(&mut self, in_scope: ScopeId, expr_id: ExprId) -> Result<(), ResolveError> {
        let expr = self.ast.expr(expr_id);
        match &expr.kind {
            ExprKind::CintLit(_)
            | ExprKind::UintLit(_)
            | ExprKind::IntLit(_)
            | ExprKind::FloatLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::NullLit
            | ExprKind::VoidLit
            | ExprKind::Continue => {}

            ExprKind::Ident(name) => {
                if self.lookup(*name, in_scope).is_none() {
                    return Err(ResolveError::UndefinedSymbol {
                        name: *name,
                        span: expr.span,
                    });
                }
            }
            ExprKind::ArrayLit(elems) => {
                for elem in elems {
                    self.resolve_expr(in_scope, *elem)?;
                }
            }
            ExprKind::ArrayRep { value, count } => {
                self.resolve_expr(in_scope, *value)?;
                self.resolve_expr(in_scope, *count)?;
            }
            ExprKind::StructLit { ty, fields } => {
                self.resolve_expr(in_scope, *ty)?;
                for field in fields {
                    self.resolve_expr(in_scope, field.value)?;
                }
            }
            ExprKind::Group(expr) | ExprKind::Comptime(expr) => {
                self.resolve_expr(in_scope, *expr)?
            }
            ExprKind::Unary { rhs, .. } => self.resolve_expr(in_scope, *rhs)?,
            ExprKind::Binary { lhs, rhs, .. } => {
                self.resolve_expr(in_scope, *lhs)?;
                self.resolve_expr(in_scope, *rhs)?;
            }
            ExprKind::Assign { tgt, val, .. } => {
                self.resolve_expr(in_scope, *tgt)?;
                self.resolve_expr(in_scope, *val)?;
            }
            ExprKind::Block(stmts) => {
                let blk_scope = self.scope(ScopeKind::Block, Some(in_scope));
                self.expr_scopes.insert(expr_id, blk_scope);
                for stmt in stmts {
                    self.resolve_stmt(blk_scope, stmt)?;
                }
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.resolve_expr(in_scope, *cond)?;
                self.resolve_expr(in_scope, *then_branch)?;
                if let Some(e) = else_branch {
                    self.resolve_expr(in_scope, *e)?;
                }
            }
            ExprKind::Loop(expr) => {
                let scope = self.scope(ScopeKind::Loop, Some(in_scope));
                self.expr_scopes.insert(expr_id, scope);
                self.resolve_expr(scope, *expr)?;
            }
            ExprKind::While { cond, body } => {
                let scope = self.scope(ScopeKind::Loop, Some(in_scope));
                self.expr_scopes.insert(expr_id, scope);
                self.resolve_expr(scope, *cond)?;
                self.resolve_expr(scope, *body)?;
            }
            ExprKind::Return(expr) | ExprKind::Break(expr) => {
                if let Some(expr_id) = expr {
                    self.resolve_expr(in_scope, *expr_id)?;
                }
            }
            ExprKind::Call { callee, args } => {
                self.resolve_expr(in_scope, *callee)?;
                for arg in args {
                    self.resolve_expr(in_scope, *arg)?;
                }
            }
            ExprKind::Field { object, .. } | ExprKind::OptField { object, .. } => {
                self.resolve_expr(in_scope, *object)?
            }
            ExprKind::Index { object, index } => {
                self.resolve_expr(in_scope, *object)?;
                self.resolve_expr(in_scope, *index)?;
            }

            ExprKind::IntType
            | ExprKind::UintType
            | ExprKind::BoolType
            | ExprKind::FloatType
            | ExprKind::VoidType => {}
            ExprKind::OptionType(expr) => self.resolve_expr(in_scope, *expr)?,
        }

        Ok(())
    }

    fn resolve_stmt(&mut self, in_scope: ScopeId, stmt: &Stmt) -> Result<(), ResolveError> {
        match &stmt.kind {
            StmtKind::Let {
                name,
                ty,
                value,
                mutable,
            } => {
                if let Some(ty) = ty {
                    self.resolve_expr(in_scope, *ty)?;
                }
                self.resolve_expr(in_scope, *value)?;
                self.define(
                    in_scope,
                    Symbol {
                        name: *name,
                        kind: SymbolKind::Var(*mutable),
                        span: stmt.span,
                    },
                )?;
            }
            StmtKind::Expr(expr) | StmtKind::Semi(expr) => {
                self.resolve_expr(in_scope, *expr)?;
            }
        }

        Ok(())
    }
}

pub fn resolve<'a>(
    ast: &'a Ast,
    intern: &'a mut Interner,
    module: DeclId,
) -> Result<SymbolTable<'a>, ResolveError> {
    let mut table = SymbolTable::new(ast, intern);

    let scope = table.scope(ScopeKind::Module, None);
    let decl = table.ast.decl(module);

    table.define(
        scope,
        Symbol {
            name: decl.name,
            kind: SymbolKind::Mod(module),
            span: decl.span,
        },
    )?;

    if let DeclKind::Mod(decls) = &decl.kind {
        for &id in decls {
            table.register_decl(scope, id)?;
        }

        for &id in decls {
            table.resolve_decl(scope, id)?;
        }

        return Ok(table);
    }

    bug!("expected module declaration")
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum ResolveError {
    #[error("duplicate symbol `{name:?}`")]
    DuplicateSymbol {
        name: Ident,
        first: Span,
        duplicate: Span,
    },
    #[error("undefined symbol `{name:?}`")]
    UndefinedSymbol { name: Ident, span: Span },
}
