use std::collections::hash_map::Entry;

use ahash::AHashMap;

use crate::{
    arena::{Ident, Interner},
    compiler::{
        ast::{
            AstArena, AstField, AstVariant, DeclId, DeclKind, ExprId, ExprKind, PatternId,
            PatternKind, StmtId, StmtKind,
        },
        tokens::Span,
    },
};

#[derive(Debug, Clone)]
pub enum ResolveError {
    DuplicateSymbol {
        name: Ident,
        first_def: Span,
        dupe_def: Span,
    },
    DuplicateField {
        name: Ident,
        first_def: Span,
        dupe_def: Span,
    },
    DuplicateVariant {
        name: Ident,
        first_def: Span,
        dupe_def: Span,
    },
    UndefinedSymbol {
        name: Ident,
        span: Span,
    },
    BreakOutsideLoop {
        span: Span,
    },
    ContinueOutsideLoop {
        span: Span,
    },
    ReturnOutsideFunction {
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Item(DeclId),
    Variable,     // let bindings, function params
    GenericType,  // <T>
    GenericConst, // <const N: int>
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: Ident,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeId {
    Decl(DeclId),
    Expr(ExprId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Module,
    Function,
    Block,
    Loop,
}

pub struct Scope {
    pub kind: ScopeKind,
    pub parent: Option<ScopeId>,
    pub symbols: AHashMap<Ident, Symbol>,
}

impl Scope {
    pub fn new(kind: ScopeKind, parent: Option<ScopeId>) -> Self {
        Self {
            kind,
            parent,
            symbols: AHashMap::new(),
        }
    }

    pub fn define(&mut self, symbol: Symbol) -> Result<(), Span> {
        match self.symbols.entry(symbol.name) {
            Entry::Occupied(entry) => Err(entry.get().span),
            Entry::Vacant(entry) => {
                entry.insert(symbol);
                Ok(())
            }
        }
    }
}

pub struct SymbolTable {
    pub scopes: AHashMap<ScopeId, Scope>,
}

impl SymbolTable {
    fn new() -> Self {
        Self {
            scopes: AHashMap::new(),
        }
    }

    fn scope(&mut self, id: ScopeId, kind: ScopeKind, parent: Option<ScopeId>) {
        self.scopes.insert(id, Scope::new(kind, parent));
    }

    pub fn lookup_local(&self, name: Ident, scope_id: ScopeId) -> Option<&Symbol> {
        self.scopes.get(&scope_id)?.symbols.get(&name)
    }

    pub fn lookup(&self, name: Ident, mut scope_id: ScopeId) -> Option<(&Symbol, ScopeId)> {
        loop {
            let scope = self.scopes.get(&scope_id)?;
            if let Some(symbol) = scope.symbols.get(&name) {
                return Some((symbol, scope_id));
            }
            scope_id = scope.parent?;
        }
    }

    fn define(&mut self, symbol: Symbol, scope_id: ScopeId) -> Result<(), Span> {
        self.scopes
            .get_mut(&scope_id)
            .expect("scope not found")
            .define(symbol)
    }

    pub fn find_enclosing(&self, mut scope_id: ScopeId, kind: ScopeKind) -> Option<ScopeId> {
        loop {
            let scope = self.scopes.get(&scope_id)?;
            if scope.kind == kind {
                return Some(scope_id);
            }
            scope_id = scope.parent?;
        }
    }

    pub fn find_loop_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.find_enclosing(scope_id, ScopeKind::Loop)
    }

    pub fn find_function_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.find_enclosing(scope_id, ScopeKind::Function)
    }
}

pub struct NameResolver<'a> {
    ast: &'a AstArena,
    interner: &'a Interner,
    table: SymbolTable,
    errors: Vec<ResolveError>,
}

impl<'a> NameResolver<'a> {
    pub fn new(ast: &'a AstArena, interner: &'a Interner) -> Self {
        Self {
            ast,
            interner,
            table: SymbolTable::new(),
            errors: vec![],
        }
    }

    fn error(&mut self, err: ResolveError) {
        self.errors.push(err);
    }

    fn define(&mut self, symbol: Symbol, scope_id: ScopeId) {
        let name = symbol.name;
        let span = symbol.span;
        if let Err(first_def) = self.table.define(symbol, scope_id) {
            self.error(ResolveError::DuplicateSymbol {
                name,
                first_def,
                dupe_def: span,
            });
        }
    }

    pub fn resolve(
        mut self,
        expr_id: ExprId,
        decls: &[DeclId],
    ) -> (SymbolTable, Vec<ResolveError>) {
        self.resolve_module(expr_id, decls, None);
        (self.table, self.errors)
    }

    fn resolve_module(&mut self, expr_id: ExprId, decls: &[DeclId], parent: Option<ScopeId>) {
        let scope = ScopeId::Expr(expr_id);
        self.table.scope(scope, ScopeKind::Module, parent);

        // First pass: register all items
        for &decl_id in decls {
            self.register_decl(decl_id, scope);
        }

        // Second pass: resolve all items
        for &decl_id in decls {
            self.resolve_decl(decl_id, scope);
        }
    }

    fn register_decl(&mut self, item_id: DeclId, parent: ScopeId) {
        let item = &self.ast.decls[item_id];

        self.define(
            Symbol {
                kind: SymbolKind::Item(item_id),
                name: item.name,
                span: item.span,
            },
            parent,
        );

        match &item.kind {
            DeclKind::Function { .. } => {
                let scope = ScopeId::Decl(item_id);
                self.table.scope(scope, ScopeKind::Function, Some(parent));
            }

            DeclKind::Const { .. } => {}
        }
    }

    fn resolve_decl(&mut self, item_id: DeclId, parent_scope: ScopeId) {
        let item = &self.ast.decls[item_id];

        match &item.kind {
            DeclKind::Function { params, ret, body } => {
                let scope = ScopeId::Decl(item_id);

                // Register function parameters
                for param in params {
                    self.resolve_expr(param.ty, scope);
                    self.add_pattern_bindings(param.pattern, scope);
                }

                // Resolve return type
                if let Some(ret_ty) = ret {
                    self.resolve_expr(*ret_ty, scope);
                }

                // Resolve body
                self.resolve_expr(*body, scope);
            }

            DeclKind::Const { ty, value } => {
                if let Some(ty) = ty {
                    self.resolve_expr(*ty, parent_scope);
                }
                self.resolve_expr(*value, parent_scope);
            }
        }
    }

    fn resolve_expr(&mut self, expr_id: ExprId, scope: ScopeId) {
        let expr = &self.ast.exprs[expr_id];

        match &expr.kind {
            ExprKind::IntLit(_)
            | ExprKind::FloatLit(_)
            | ExprKind::True
            | ExprKind::False
            | ExprKind::Char(_)
            | ExprKind::StrLit(_)
            | ExprKind::Null
            | ExprKind::Void
            | ExprKind::Continue
            | ExprKind::WildcardType => {}

            ExprKind::Ident(name) => {
                if self.table.lookup(*name, scope).is_none() {
                    self.error(ResolveError::UndefinedSymbol {
                        name: *name,
                        span: expr.span,
                    })
                }
            }

            ExprKind::ArrayLit(elems) => elems.iter().for_each(|&e| self.resolve_expr(e, scope)),
            ExprKind::ArrayRepeat { value, count } => {
                self.resolve_expr(*value, scope);
                self.resolve_expr(*count, scope);
            }
            ExprKind::StructLit { ty, fields } => {
                self.resolve_expr(*ty, scope);
                for field in fields {
                    self.resolve_expr(field.value, scope);
                }
            }
            ExprKind::TupleLit(elems) => elems.iter().for_each(|&e| self.resolve_expr(e, scope)),
            ExprKind::ScopeAccess { ty, .. } => self.resolve_expr(*ty, scope),
            ExprKind::Group(expr) => self.resolve_expr(*expr, scope),
            ExprKind::Unary { expr, .. } => self.resolve_expr(*expr, scope),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.resolve_expr(*lhs, scope);
                self.resolve_expr(*rhs, scope);
            }
            ExprKind::Assign { tgt, val, .. } => {
                self.resolve_expr(*tgt, scope);
                self.resolve_expr(*val, scope);
            }
            ExprKind::Cast { expr, ty } => {
                self.resolve_expr(*expr, scope);
                self.resolve_expr(*ty, scope);
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.resolve_expr(*cond, scope);
                self.resolve_expr(*then_branch, scope);
                if let Some(e) = else_branch {
                    self.resolve_expr(*e, scope);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.resolve_expr(*scrutinee, scope);

                for arm in arms {
                    self.add_pattern_bindings(arm.pattern, scope);
                }
            }
            ExprKind::While { cond, body } => {
                self.resolve_expr(*cond, scope);
                self.resolve_expr(*body, scope);
            }
            ExprKind::Loop(body) => self.resolve_expr(*body, scope),
            ExprKind::For {
                pattern,
                iter,
                body,
            } => {
                self.resolve_pattern(*pattern, scope);
                self.resolve_expr(*iter, scope);
                self.resolve_expr(*body, scope);
            }
            ExprKind::Block(stmts) => {
                let scope = ScopeId::Expr(expr_id);
                self.table.scope(scope, ScopeKind::Block, Some(scope));
                for stmt in stmts {
                    self.resolve_stmt(*stmt, scope);
                }
            }
            ExprKind::Return(expr_id) | ExprKind::Break(expr_id) => {
                if let Some(expr_id) = expr_id {
                    self.resolve_expr(*expr_id, scope);
                }
            }
            ExprKind::Call { callee, args } => {
                self.resolve_expr(*callee, scope);
                for arg in args {
                    self.resolve_expr(*arg, scope);
                }
            }
            ExprKind::Field { object, .. } | ExprKind::OptionalField { object, .. } => {
                self.resolve_expr(*object, scope);
            }

            ExprKind::Index { object, index } => {
                self.resolve_expr(*object, scope);
                self.resolve_expr(*index, scope);
            }
            ExprKind::Range { start, end, .. } => {
                if let Some(start) = start {
                    self.resolve_expr(*start, scope);
                }

                if let Some(end) = end {
                    self.resolve_expr(*end, scope);
                }
            }
            ExprKind::Unwrap(expr_id) | ExprKind::Const(expr_id) => {
                self.resolve_expr(*expr_id, scope);
            }
            ExprKind::StructType(fields) | ExprKind::UnionType(fields) => {
                self.resolve_fields(fields, scope);
            }
            ExprKind::EnumType(variants) => {
                self.resolve_variants(variants, scope);
            }
            ExprKind::ArrayType { elem, size } => {
                self.resolve_expr(*elem, scope);
                self.resolve_expr(*size, scope);
            }
            ExprKind::SliceType(expr_id) | ExprKind::OptionType(expr_id) => {
                self.resolve_expr(*expr_id, scope);
            }
            ExprKind::PointerType { pointee, .. } => {
                self.resolve_expr(*pointee, scope);
            }
            ExprKind::FunctionType { params, ret } => {
                for &param in params {
                    self.resolve_expr(param, scope);
                }

                if let Some(ret) = ret {
                    self.resolve_expr(*ret, scope);
                }
            }
            ExprKind::ModuleType(decls) => {
                self.resolve_module(expr_id, &decls, Some(scope));
            }
        }
    }

    fn resolve_stmt(&mut self, stmt_id: StmtId, scope_id: ScopeId) {
        let stmt = &self.ast.stmts[stmt_id];

        match &stmt.kind {
            StmtKind::Let { pattern, ty, value } => {
                if let Some(ty) = ty {
                    self.resolve_expr(*ty, scope_id);
                }
                self.resolve_expr(*value, scope_id);
                self.add_pattern_bindings(*pattern, scope_id);
            }

            StmtKind::Expr { expr, .. } => self.resolve_expr(*expr, scope_id),
            StmtKind::Empty => {}
        }
    }

    fn resolve_pattern(&mut self, pattern_id: PatternId, scope: ScopeId) {
        let pattern = &self.ast.patterns[pattern_id];

        match &pattern.kind {
            PatternKind::Wildcard
            | PatternKind::Int(_)
            | PatternKind::Float(_)
            | PatternKind::Bool(_)
            | PatternKind::Char(_)
            | PatternKind::CStr(_)
            | PatternKind::Rest => {}

            PatternKind::Identifier { ident, .. } => {
                if self.table.lookup(*ident, scope).is_none() {
                    self.error(ResolveError::UndefinedSymbol {
                        name: *ident,
                        span: pattern.span,
                    })
                }
            }

            PatternKind::Struct { ty, fields, .. } => {
                self.resolve_expr(*ty, scope);
                for pattern in fields.iter().filter_map(|f| f.pattern) {
                    self.resolve_pattern(pattern, scope);
                }
            }
            PatternKind::Tuple(elems) => {
                for &elem in elems {
                    self.resolve_pattern(elem, scope);
                }
            }
            PatternKind::Array(elems) => {
                for &elem in elems {
                    self.resolve_pattern(elem, scope);
                }
            }
            PatternKind::Or(patterns) => {
                for &pat in patterns {
                    self.resolve_pattern(pat, scope);
                }
            }
            PatternKind::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.resolve_expr(*s, scope);
                }
                if let Some(e) = end {
                    self.resolve_expr(*e, scope);
                }
            }
        }
    }

    fn add_pattern_bindings(&mut self, pattern_id: PatternId, scope_id: ScopeId) {
        let pattern = &self.ast.patterns[pattern_id];

        match &pattern.kind {
            PatternKind::Wildcard
            | PatternKind::Int(_)
            | PatternKind::Float(_)
            | PatternKind::Bool(_)
            | PatternKind::Char(_)
            | PatternKind::CStr(_)
            | PatternKind::Range { .. }
            | PatternKind::Rest => {}

            PatternKind::Identifier { ident, .. } => {
                self.define(
                    Symbol {
                        kind: SymbolKind::Variable,
                        name: *ident,
                        span: pattern.span,
                    },
                    scope_id,
                );
            }

            PatternKind::Struct { fields, .. } => {
                for pattern in fields.iter().filter_map(|f| f.pattern) {
                    self.add_pattern_bindings(pattern, scope_id);
                }
            }

            PatternKind::Tuple(elems) | PatternKind::Array(elems) => {
                for &elem in elems {
                    self.add_pattern_bindings(elem, scope_id);
                }
            }

            PatternKind::Or(patterns) => {
                // All branches must bind the same names - just use first
                if let Some(&first) = patterns.first() {
                    self.add_pattern_bindings(first, scope_id);
                }
            }
        }
    }

    fn resolve_fields(&mut self, fields: &[AstField], scope_id: ScopeId) {
        let mut seen: AHashMap<Ident, Span> = AHashMap::new();
        for field in fields {
            match seen.entry(field.name) {
                Entry::Occupied(entry) => {
                    self.error(ResolveError::DuplicateField {
                        name: field.name,
                        first_def: *entry.get(),
                        dupe_def: field.span,
                    });
                }
                Entry::Vacant(entry) => {
                    entry.insert(field.span);
                }
            }
            self.resolve_expr(field.ty, scope_id);
        }
    }

    fn resolve_variants(&mut self, variants: &[AstVariant], scope_id: ScopeId) {
        let mut seen: AHashMap<Ident, Span> = AHashMap::new();
        for variant in variants {
            match seen.entry(variant.name) {
                Entry::Occupied(entry) => {
                    self.error(ResolveError::DuplicateField {
                        name: variant.name,
                        first_def: *entry.get(),
                        dupe_def: variant.span,
                    });
                }
                Entry::Vacant(entry) => {
                    entry.insert(variant.span);
                }
            }

            if let Some(value) = variant.value {
                self.resolve_expr(value, scope_id);
            }
        }
    }
}
