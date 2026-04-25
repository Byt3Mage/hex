use std::collections::hash_map::Entry;

use ahash::AHashMap;

use crate::{
    arena::{Arena, Ident, define_id},
    compiler::{ast::DeclId, error::ResolveError, sema::sema_type::SemaTypeId, tokens::Span},
};

define_id!(SymbolId);
define_id!(ScopeId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Variable(bool),
    Module(DeclId),
    Function(DeclId),
    Const(DeclId),
    Enum(DeclId),
    Struct(DeclId),
    Union(DeclId),
    Variant(SymbolId),
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: Ident,
    pub span: Span,
    pub ty_id: Option<SemaTypeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Package,
    Module,
    Function,
    Enum,
    Block,
    Loop,
}

pub struct Scope {
    pub kind: ScopeKind,
    pub parent: Option<ScopeId>,
    pub symbols: AHashMap<Ident, SymbolId>,
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

pub struct SymbolTable {
    pub scopes: Arena<ScopeId, Scope>,
    pub symbols: Arena<SymbolId, Symbol>,
    pub decl_scopes: AHashMap<DeclId, ScopeId>,
    pub decl_symbols: AHashMap<DeclId, SymbolId>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            scopes: Arena::new(),
            symbols: Arena::new(),
            decl_scopes: AHashMap::new(),
            decl_symbols: AHashMap::new(),
        }
    }

    pub fn scope(&mut self, kind: ScopeKind, parent: Option<ScopeId>) -> ScopeId {
        self.scopes.insert(Scope::new(kind, parent))
    }

    pub fn get(&self, symbol_id: SymbolId) -> &Symbol {
        &self.symbols[symbol_id]
    }

    pub fn parent_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.scopes[scope_id].parent
    }

    pub fn get_mut(&mut self, symbol_id: SymbolId) -> &mut Symbol {
        &mut self.symbols[symbol_id]
    }

    pub fn lookup_local(&self, name: Ident, scope_id: ScopeId) -> Option<SymbolId> {
        self.scopes[scope_id].symbols.get(&name).copied()
    }

    pub fn lookup(&self, name: Ident, mut scope_id: ScopeId) -> Option<Lookup> {
        loop {
            let scope = &self.scopes[scope_id];
            match scope.symbols.get(&name) {
                Some(symbol) => {
                    return Some(Lookup {
                        symbol: *symbol,
                        scope: scope_id,
                    });
                }
                None => scope_id = scope.parent?,
            }
        }
    }

    pub fn define(&mut self, symbol: Symbol, scope_id: ScopeId) -> Result<SymbolId, ResolveError> {
        let scope = &mut self.scopes[scope_id];
        match scope.symbols.entry(symbol.name) {
            Entry::Vacant(e) => Ok(*e.insert(self.symbols.insert(symbol))),
            Entry::Occupied(e) => Err(ResolveError::DuplicateSymbol {
                name: symbol.name,
                first: self.symbols[*e.get()].span,
                duplicate: symbol.span,
            }),
        }
    }

    pub fn find_enclosing(&self, mut scope_id: ScopeId, kind: ScopeKind) -> Option<ScopeId> {
        loop {
            let scope = &self.scopes[scope_id];
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
