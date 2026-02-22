use std::{collections::hash_map::Entry, rc::Rc};

use ahash::{AHashMap, AHashSet};
use simple_ternary::tnr;

use crate::{
    arena::{Arena, Ident, Interner},
    compiler::{
        ast::{
            AstArena, AstType, AstTypeId, AstTypeKind, DeclId, DeclKind, ExprId, ExprKind,
            FieldDef, FieldInit, Param, PathId, PatternId, PatternKind, StmtId, StmtKind,
            VariantDef,
        },
        error::{ResolveError, bug},
        op::{BinOp, UnOp},
        sema::{
            sema_type::{
                EnumInfo, FieldInfo, SemaType, SemaTypeId, StructInfo, TypeArena, UnionInfo,
                VariantInfo,
            },
            sema_value::{ComptimeInt, SemaValue, SemaValueId, ValueArena},
        },
        tokens::Span,
    },
};

pub mod sema_type;
pub mod sema_value;

slotmap::new_key_type! {
    pub struct SymbolId;
}

type Result<T> = std::result::Result<T, ResolveError>;

use ResolveError::*;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeId {
    Decl(DeclId),
    Expr(ExprId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
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

struct Lookup {
    symbol: SymbolId,
    scope: ScopeId,
}

pub struct SymbolTable {
    scopes: AHashMap<ScopeId, Scope>,
    symbols: Arena<SymbolId, Symbol>,
}

impl SymbolTable {
    fn new() -> Self {
        Self {
            scopes: AHashMap::new(),
            symbols: Arena::with_key(),
        }
    }

    fn scope(&mut self, id: ScopeId, kind: ScopeKind, parent: Option<ScopeId>) {
        self.scopes.insert(id, Scope::new(kind, parent));
    }

    fn get(&self, symbol_id: SymbolId) -> &Symbol {
        &self.symbols[symbol_id]
    }

    fn get_mut(&mut self, symbol_id: SymbolId) -> &mut Symbol {
        &mut self.symbols[symbol_id]
    }

    fn lookup_local(&self, name: Ident, scope_id: ScopeId) -> Option<SymbolId> {
        self.scopes.get(&scope_id)?.symbols.get(&name).copied()
    }

    fn lookup(&self, name: Ident, mut scope_id: ScopeId) -> Option<Lookup> {
        loop {
            let scope = self.scopes.get(&scope_id)?;
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

    fn define(&mut self, symbol: Symbol, scope_id: ScopeId) -> Result<SymbolId> {
        let scope = self.scopes.get_mut(&scope_id).expect("scope not found");

        match scope.symbols.entry(symbol.name) {
            Entry::Vacant(entry) => Ok(*entry.insert(self.symbols.insert(symbol))),
            Entry::Occupied(entry) => Err(DuplicateSymbol {
                name: symbol.name,
                first: self.symbols[*entry.get()].span,
                duplicate: symbol.span,
            }),
        }
    }

    fn find_enclosing(&self, mut scope_id: ScopeId, kind: ScopeKind) -> Option<ScopeId> {
        loop {
            let scope = self.scopes.get(&scope_id)?;
            if scope.kind == kind {
                return Some(scope_id);
            }
            scope_id = scope.parent?;
        }
    }

    fn find_loop_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.find_enclosing(scope_id, ScopeKind::Loop)
    }

    pub fn find_function_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.find_enclosing(scope_id, ScopeKind::Function)
    }
}

pub struct FunctionEnv {
    scope: ScopeId,
    return_type: SemaTypeId,
    generics: AHashMap<Ident, SemaTypeId>,
}

pub struct Sema<'a> {
    ast: &'a AstArena,
    interner: &'a Interner,
    symbols: SymbolTable,
    types: TypeArena,
    values: ValueArena,
    value_bindings: AHashMap<SymbolId, SemaValueId>,
    expr_types: AHashMap<ExprId, SemaTypeId>,
}

impl<'a> Sema<'a> {
    pub fn new(ast: &'a AstArena, interner: &'a Interner) -> Self {
        Self {
            ast,
            interner,
            symbols: SymbolTable::new(),
            types: TypeArena::new(),
            values: ValueArena::new(),
            value_bindings: AHashMap::new(),
            expr_types: AHashMap::new(),
        }
    }

    #[inline]
    fn define(&mut self, symbol: Symbol, scope: ScopeId) -> Result<SymbolId> {
        self.symbols.define(symbol, scope)
    }

    #[inline]
    fn lookup(&self, scope: ScopeId, name: Ident, span: Span) -> Result<Lookup> {
        self.symbols
            .lookup(name, scope)
            .ok_or(UndefinedSymbol { name, span })
    }

    #[inline]
    fn lookup_local(&self, scope: ScopeId, name: Ident, span: Span) -> Result<SymbolId> {
        self.symbols
            .lookup_local(name, scope)
            .ok_or(SymbolNotInScope { scope, name, span })
    }

    fn coerce(&self, tgt: SemaTypeId, val: SemaTypeId, span: Span) -> Result<()> {
        if tgt == val {
            return Ok(());
        }

        let tgt_ty = self.types.get(tgt);
        let val_ty = self.types.get(val);

        match (tgt_ty, val_ty) {
            (_, SemaType::Never)
            | (SemaType::Cint, SemaType::Cint)
            | (SemaType::Cstr, SemaType::Cstr)
            | (SemaType::Int, SemaType::Int | SemaType::Cint)
            | (SemaType::Uint, SemaType::Uint | SemaType::Cint)
            | (SemaType::Bool, SemaType::Bool)
            | (SemaType::Char, SemaType::Char)
            | (SemaType::Float, SemaType::Float)
            | (SemaType::Void, SemaType::Void)
            | (SemaType::Opt(_) | SemaType::Null, SemaType::Null) => Ok(()),

            (SemaType::Opt(tgt), SemaType::Opt(val)) => self.coerce(*tgt, *val, span),
            (SemaType::Opt(tgt), _) => self.coerce(*tgt, val, span),

            (
                SemaType::Ptr {
                    mutable: tgt_mut,
                    pointee: tgt_ptr,
                },
                SemaType::Ptr {
                    mutable: val_mut,
                    pointee: val_ptr,
                },
            ) => {
                todo!("pointer type check")
            }

            (
                SemaType::Array {
                    elem: tgt_elem,
                    len: tgt_len,
                },
                SemaType::Array {
                    elem: val_elem,
                    len: val_len,
                },
            ) if tgt_len == val_len => self.coerce(*tgt_elem, *val_elem, span),

            _ => Err(CoerceFailed { tgt, val, span }),
        }
    }

    fn coerce_numeric_operands(
        &self,
        lhs: SemaTypeId,
        rhs: SemaTypeId,
    ) -> (SemaTypeId, SemaTypeId) {
        let lhs_ty = self.types.get(lhs);
        let rhs_ty = self.types.get(rhs);

        match (lhs_ty, rhs_ty) {
            (SemaType::Int, SemaType::Cint) => (self.types.int, self.types.int),
            (SemaType::Cint, SemaType::Int) => (self.types.int, self.types.int),

            (SemaType::Uint, SemaType::Cint) => (self.types.uint, self.types.uint),
            (SemaType::Cint, SemaType::Uint) => (self.types.uint, self.types.uint),

            _ => (lhs, rhs),
        }
    }

    fn unify_types(
        &mut self,
        lhs_id: SemaTypeId,
        rhs_id: SemaTypeId,
        span: Span,
    ) -> Result<SemaTypeId> {
        if lhs_id == rhs_id {
            return Ok(lhs_id);
        }

        let lhs = self.types.get(lhs_id);
        let rhs = self.types.get(rhs_id);

        match (lhs, rhs) {
            // Never unifies to the type of the other side
            (_, SemaType::Never) => Ok(lhs_id),
            (SemaType::Never, _) => Ok(rhs_id),

            // Normalize structural types to built-in type ids
            (SemaType::Int, SemaType::Int) => Ok(self.types.int),
            (SemaType::Cint, SemaType::Cint) => Ok(self.types.cint),
            (SemaType::Uint, SemaType::Uint) => Ok(self.types.uint),
            (SemaType::Bool, SemaType::Bool) => Ok(self.types.bool),
            (SemaType::Cstr, SemaType::Cstr) => Ok(self.types.cstr),
            (SemaType::Char, SemaType::Char) => Ok(self.types.char),
            (SemaType::Null, SemaType::Null) => Ok(self.types.null),
            (SemaType::Void, SemaType::Void) => Ok(self.types.void),
            (SemaType::Float, SemaType::Float) => Ok(self.types.float),

            // Normalize comptime int to runtime ints
            (SemaType::Cint, SemaType::Int) | (SemaType::Int, SemaType::Cint) => Ok(self.types.int),
            (SemaType::Cint, SemaType::Uint) | (SemaType::Uint, SemaType::Cint) => {
                Ok(self.types.uint)
            }

            // Null unifies to optional on either side
            (SemaType::Opt(_), SemaType::Null) => Ok(lhs_id),
            (SemaType::Null, SemaType::Opt(_)) => Ok(rhs_id),

            // Optionals unify to optional of unified inner types
            (SemaType::Opt(lhs), SemaType::Opt(rhs)) => {
                let unified = self.unify_types(*lhs, *rhs, span)?;
                Ok(self.types.insert(SemaType::Opt(unified)))
            }

            // Non optionals unify to optionals of inner type unification
            (SemaType::Opt(inner), _) => {
                let unified = self.unify_types(*inner, rhs_id, span)?;
                Ok(self.types.insert(SemaType::Opt(unified)))
            }

            (_, SemaType::Opt(inner)) => {
                let unified = self.unify_types(lhs_id, *inner, span)?;
                Ok(self.types.insert(SemaType::Opt(unified)))
            }

            (
                SemaType::Ptr {
                    mutable: tgt_mut,
                    pointee: tgt_ptr,
                },
                SemaType::Ptr {
                    mutable: val_mut,
                    pointee: val_ptr,
                },
            ) => {
                todo!("pointer type unification")
            }

            // Arrays unify if lengths match and inner types unify
            (
                SemaType::Array {
                    elem: lhs_elem,
                    len: lhs_len,
                },
                SemaType::Array {
                    elem: rhs_elem,
                    len: rhs_len,
                },
            ) if lhs_len == rhs_len => {
                let len = *lhs_len;
                let elem = self.unify_types(*lhs_elem, *rhs_elem, span)?;
                Ok(self.types.insert(SemaType::Array { elem, len }))
            }

            _ => Err(TypeMismatch {
                exp: lhs_id,
                got: rhs_id,
                span,
            }),
        }
    }

    fn register_decl(&mut self, scope: ScopeId, decl_id: DeclId) -> Result<()> {
        let item = &self.ast.decls[decl_id];

        match &item.kind {
            DeclKind::Module(decls) => {
                self.define(
                    Symbol {
                        kind: SymbolKind::Module(decl_id),
                        name: item.name,
                        span: item.span,
                        ty_id: None,
                    },
                    scope,
                )?;

                let mod_scope = ScopeId::Decl(decl_id);
                self.symbols
                    .scope(mod_scope, ScopeKind::Module, Some(scope));

                for &decl in decls {
                    self.register_decl(mod_scope, decl)?;
                }
            }
            DeclKind::Function { .. } => {
                self.define(
                    Symbol {
                        kind: SymbolKind::Function(decl_id),
                        name: item.name,
                        span: item.span,
                        ty_id: None,
                    },
                    scope,
                )?;

                let fn_scope = ScopeId::Decl(decl_id);
                self.symbols
                    .scope(fn_scope, ScopeKind::Function, Some(scope));
            }
            DeclKind::Enum { variants, .. } => {
                let enum_symbol = self.define(
                    Symbol {
                        kind: SymbolKind::Enum(decl_id),
                        name: item.name,
                        span: item.span,
                        ty_id: None,
                    },
                    scope,
                )?;

                let enum_scope = ScopeId::Decl(decl_id);
                self.symbols.scope(enum_scope, ScopeKind::Enum, Some(scope));

                for variant in variants {
                    self.define(
                        Symbol {
                            kind: SymbolKind::Variant(enum_symbol),
                            name: variant.name,
                            span: variant.span,
                            ty_id: None,
                        },
                        enum_scope,
                    )?;
                }
            }
            DeclKind::Const { .. } => {
                self.define(
                    Symbol {
                        kind: SymbolKind::Const(decl_id),
                        name: item.name,
                        span: item.span,
                        ty_id: None,
                    },
                    scope,
                )?;
            }
            DeclKind::Struct { .. } => {
                self.define(
                    Symbol {
                        kind: SymbolKind::Struct(decl_id),
                        name: item.name,
                        span: item.span,
                        ty_id: None,
                    },
                    scope,
                )?;
            }
            DeclKind::Union { .. } => {
                self.define(
                    Symbol {
                        kind: SymbolKind::Union(decl_id),
                        name: item.name,
                        span: item.span,
                        ty_id: None,
                    },
                    scope,
                )?;
            }
        }

        Ok(())
    }

    pub fn register_package(&mut self, decl_id: DeclId, decls: &[DeclId]) -> Result<()> {
        let scope = ScopeId::Decl(decl_id);
        self.symbols.scope(scope, ScopeKind::Module, None);

        for decl in decls {
            self.register_decl(scope, *decl)?;
        }

        Ok(())
    }

    pub fn analyze_function(
        &mut self,
        scope: ScopeId,
        decl_id: DeclId,
        fn_params: &[Param],
        ret: Option<AstTypeId>,
        body: ExprId,
        span: Span,
    ) -> Result<()> {
        let decl = &self.ast.decls[decl_id];
        let lookup = self.lookup(scope, decl.name, decl.span)?;
        let symbol = lookup.symbol;

        // Evaluate param types and bind param names
        let mut params = Vec::with_capacity(fn_params.len());
        for param in fn_params {
            let param_ty = self.eval_type(scope, param.ty)?;
            self.bind_variable_pattern(scope, param.pattern, param_ty)?;
            params.push(param_ty);
        }

        // Evaluate return type
        let ret = match ret {
            Some(r) => self.eval_type(scope, r)?,
            None => self.types.void,
        };

        // Create function signature and bind to its symbol
        let signature = self.types.insert(SemaType::Function { params, ret });
        self.symbols.get_mut(symbol).ty_id = Some(signature);

        let env = FunctionEnv {
            scope,
            return_type: ret,
            generics: AHashMap::new(),
        };

        // Evaluate body type
        let body = self.check_expr(&env, body)?;

        // Ensure body type can be assigned to return type
        self.coerce(ret, body, span)
    }

    fn bind_variable_pattern(
        &mut self,
        scope: ScopeId,
        pattern_id: PatternId,
        expected_ty: SemaTypeId,
    ) -> Result<()> {
        let pattern = &self.ast.patterns[pattern_id];

        // Recursively walk patterns and create symbols for identifiers.
        // Refutable patterns can't be used as function parameters.
        match &pattern.kind {
            PatternKind::Int(_)
            | PatternKind::Float(_)
            | PatternKind::Bool(_)
            | PatternKind::Char(_)
            | PatternKind::CStr(_)
            | PatternKind::Path(_)
            | PatternKind::Range { .. } => Err(RefutableParamPattern {
                pattern_id,
                expected_ty,
                span: pattern.span,
            }),

            PatternKind::Wildcard | PatternKind::Rest => Ok(()),

            PatternKind::Identifier { name, mutable } => {
                self.define(
                    Symbol {
                        kind: SymbolKind::Variable(*mutable),
                        name: *name,
                        span: pattern.span,
                        ty_id: Some(expected_ty),
                    },
                    scope,
                )?;
                Ok(())
            }

            PatternKind::Struct { ty, fields, rest } => todo!(),
            PatternKind::Tuple(pattern_ids) => todo!(),
            PatternKind::Array(pattern_ids) => todo!(),
            PatternKind::Or(pattern_ids) => todo!(),
        }
    }

    fn eval_type(&mut self, scope: ScopeId, ast_ty: AstTypeId) -> Result<SemaTypeId> {
        let ast_ty = &self.ast.types[ast_ty];

        match &ast_ty.kind {
            AstTypeKind::CInt => Ok(self.types.cint),
            AstTypeKind::CStr => Ok(self.types.cstr),
            AstTypeKind::Bool => Ok(self.types.bool),
            AstTypeKind::Int => Ok(self.types.int),
            AstTypeKind::Uint => Ok(self.types.uint),
            AstTypeKind::Float => Ok(self.types.float),
            AstTypeKind::Char => Ok(self.types.char),
            AstTypeKind::Never => Ok(self.types.never),
            AstTypeKind::Str => Ok(self.types.str),
            AstTypeKind::Void => Ok(self.types.void),

            AstTypeKind::Inferred => todo!("handle inferred type"),

            AstTypeKind::Path(path) => self.eval_type_path(scope, *path, ast_ty.span),

            AstTypeKind::Tuple(elems) => {
                let mut tuple_elems = Vec::with_capacity(elems.len());
                for &elem in elems {
                    tuple_elems.push(self.eval_type(scope, elem)?);
                }
                Ok(self.types.insert(SemaType::Tuple(tuple_elems)))
            }

            AstTypeKind::Array { elem, len } => {
                let elem = self.eval_type(scope, *elem)?;
                // TODO: create env from scope
                let len = self.eval_expr(scope, *len)?;

                match self.values.get(len) {
                    SemaValue::Cint(n) => match n.get_unsigned() {
                        Some(len) => Ok(self.types.insert(SemaType::Array { elem, len })),
                        None => Err(NegativeArrayLength { span: ast_ty.span }),
                    },
                    SemaValue::Uint(n) => Ok(self.types.insert(SemaType::Array { elem, len: *n })),
                    v => Err(TypeMismatch {
                        exp: self.types.cint,
                        got: self.get_value_ty(v)?,
                        span: ast_ty.span,
                    }),
                }
            }

            AstTypeKind::Slice(elem) => {
                let elem = self.eval_type(scope, *elem)?;
                Ok(self.types.insert(SemaType::Slice(elem)))
            }

            AstTypeKind::Optional(inner) => {
                let inner = self.eval_type(scope, *inner)?;
                Ok(self.types.insert(SemaType::Opt(inner)))
            }

            AstTypeKind::Pointer { mutable, pointee } => {
                let mutable = *mutable;
                let pointee = self.eval_type(scope, *pointee)?;
                Ok(self.types.insert(SemaType::Ptr { mutable, pointee }))
            }

            AstTypeKind::Function { params, ret } => {
                let mut fn_params = Vec::with_capacity(params.len());
                for &param in params {
                    fn_params.push(self.eval_type(scope, param)?);
                }

                let fn_ret = match ret {
                    Some(ret) => self.eval_type(scope, *ret)?,
                    None => self.types.void,
                };

                Ok(self.types.insert(SemaType::Function {
                    params: fn_params,
                    ret: fn_ret,
                }))
            }
        }
    }

    fn eval_type_path(&mut self, scope: ScopeId, path: PathId, span: Span) -> Result<SemaTypeId> {
        let res = self.resolve_path(scope, path)?;
        let symbol = self.symbols.get(res.symbol);

        match symbol.kind {
            SymbolKind::Struct(decl) | SymbolKind::Union(decl) | SymbolKind::Enum(decl) => {
                match symbol.ty_id {
                    Some(ty) => Ok(ty),
                    None => self.eval_decl_type(res.scope, res.symbol, decl),
                }
            }
            _ => Err(ExpectedType {
                found: res.symbol,
                span,
            }),
        }
    }

    fn resolve_path(&mut self, scope: ScopeId, path_id: PathId) -> Result<Lookup> {
        let path = &self.ast.paths[path_id];
        let mut curr = self.lookup(scope, path.first.name, path.first.span)?;

        // Only modules and enums have child symbols
        for seg in &path.rest {
            match self.symbols.get(curr.symbol).kind {
                SymbolKind::Module(decl) | SymbolKind::Enum(decl) => {
                    curr.scope = ScopeId::Decl(decl);
                    curr.symbol = self.lookup_local(curr.scope, seg.name, seg.span)?;
                }
                _ => return Err(NotDeclScope { span: seg.span }),
            }
        }

        Ok(curr)
    }

    fn eval_decl_type(
        &mut self,
        scope: ScopeId,
        symbol: SymbolId,
        decl_id: DeclId,
    ) -> Result<SemaTypeId> {
        let decl = &self.ast.decls[decl_id];
        match &decl.kind {
            DeclKind::Module(_) => bug!("module can't resolve to a type"),

            DeclKind::Function {
                generics,
                params,
                ret,
                body,
            } => {
                // Evaluate param types and bind param names
                let mut fn_params = Vec::with_capacity(params.len());
                for param in params {
                    let param_ty = self.eval_type(scope, param.ty)?;
                    self.bind_variable_pattern(scope, param.pattern, param_ty)?;
                    fn_params.push(param_ty);
                }

                // Evaluate return type
                let ret = match ret {
                    Some(r) => self.eval_type(scope, *r)?,
                    None => self.types.void,
                };

                // Create function signature and bind to its symbol
                let signature = self.types.insert(SemaType::Function {
                    params: fn_params,
                    ret,
                });

                self.symbols.get_mut(symbol).ty_id = Some(signature);
                Ok(signature)
            }

            DeclKind::Const { ty, value } => todo!(),

            DeclKind::Struct { generics, fields } => {
                let struct_ty = self.types.insert(SemaType::Resolving(decl_id));

                self.symbols.get_mut(symbol).ty_id = Some(struct_ty);
                *self.types.get_mut(struct_ty) = SemaType::Struct(StructInfo {
                    name: Some(decl.name),
                    fields: self.make_fields(scope, fields)?,
                });

                Ok(struct_ty)
            }

            DeclKind::Union { generics, fields } => {
                let union_ty = self.types.insert(SemaType::Resolving(decl_id));

                self.symbols.get_mut(symbol).ty_id = Some(union_ty);
                *self.types.get_mut(union_ty) = SemaType::Union(UnionInfo {
                    name: Some(decl.name),
                    fields: self.make_fields(scope, fields)?,
                });

                Ok(union_ty)
            }

            DeclKind::Enum { base, variants } => {
                let enum_ty = self.types.insert(SemaType::Resolving(decl_id));
                self.symbols.get_mut(symbol).ty_id = Some(enum_ty);

                let base = match base {
                    None => self.types.uint, // default enum base type is uint
                    Some(b) => {
                        let AstType { kind, span } = &self.ast.types[*b];

                        // enum base type must be uint or int
                        match kind {
                            AstTypeKind::Int => self.types.int,
                            AstTypeKind::Uint => self.types.uint,
                            _ => return Err(InvalidEnumBase { span: *span }),
                        }
                    }
                };

                let variants = self.make_variants(scope, base, variants)?;

                *self.types.get_mut(enum_ty) = SemaType::Enum(EnumInfo {
                    name: Some(decl.name),
                    base,
                    variants,
                });

                Ok(enum_ty)
            }
        }
    }

    #[inline(always)]
    fn make_fields(&mut self, scope: ScopeId, field_defs: &[FieldDef]) -> Result<Rc<[FieldInfo]>> {
        let mut seen_fields = AHashMap::new();
        let mut field_infos = Vec::with_capacity(field_defs.len());

        for field_def in field_defs {
            match seen_fields.entry(field_def.name) {
                Entry::Vacant(entry) => {
                    let name = field_def.name;
                    let ty = self.eval_type(scope, field_def.ty)?;

                    field_infos.push(FieldInfo { name, ty });
                    entry.insert(field_def.span);
                }

                Entry::Occupied(entry) => {
                    return Err(DuplicateFieldDef {
                        name: field_def.name,
                        first: *entry.get(),
                        duplicate: field_def.span,
                    });
                }
            }
        }

        Ok(field_infos.into())
    }

    fn make_variants(
        &mut self,
        scope: ScopeId,
        base: SemaTypeId,
        variant_defs: &[VariantDef],
    ) -> Result<Vec<VariantInfo>> {
        let is_base_uint = base == self.types.uint;
        let mut seen_names = AHashMap::new();
        let mut seen_values = AHashMap::new();
        let mut variant_infos = Vec::with_capacity(variant_defs.len());
        let mut curr_val = tnr! {is_base_uint => ComptimeInt::unsigned(0) : ComptimeInt::signed(0)};

        for variant_def in variant_defs {
            match seen_names.entry(variant_def.name) {
                Entry::Vacant(entry) => {
                    if let Some(expr_id) = variant_def.value {
                        let val = self.eval_expr(scope, expr_id)?;

                        curr_val = match self.values.get(val) {
                            SemaValue::Cint(c) => {
                                if is_base_uint && c.is_neg() {
                                    return Err(TypeMismatch {
                                        exp: self.types.uint,
                                        got: self.types.int,
                                        span: variant_def.span,
                                    });
                                }
                                *c
                            }

                            SemaValue::Int(i) if !is_base_uint => ComptimeInt::signed(*i),
                            SemaValue::Uint(u) if is_base_uint => ComptimeInt::unsigned(*u),

                            v => {
                                return Err(TypeMismatch {
                                    exp: base,
                                    got: self.get_value_ty(v)?,
                                    span: variant_def.span,
                                });
                            }
                        };
                    };

                    match seen_values.entry(curr_val) {
                        Entry::Vacant(entry) => {
                            entry.insert(variant_def.span);
                        }

                        Entry::Occupied(entry) => {
                            return Err(DuplicateVariantVal {
                                value: curr_val,
                                first: *entry.get(),
                                duplicate: variant_def.span,
                            });
                        }
                    }

                    variant_infos.push(VariantInfo {
                        name: variant_def.name,
                        value: curr_val,
                    });

                    // auto-increment variant value for the next variant
                    // TODO: cleanly handle errors.
                    curr_val = if is_base_uint {
                        let curr = curr_val.get_unsigned().unwrap();
                        ComptimeInt::unsigned(curr + 1)
                    } else {
                        let curr = curr_val.get_signed().unwrap();
                        ComptimeInt::signed(curr + 1)
                    };

                    entry.insert(variant_def.span);
                }

                Entry::Occupied(entry) => {
                    return Err(DuplicateFieldDef {
                        name: variant_def.name,
                        first: *entry.get(),
                        duplicate: variant_def.span,
                    });
                }
            }
        }

        Ok(variant_infos)
    }

    fn check_expr(&mut self, env: &FunctionEnv, expr_id: ExprId) -> Result<SemaTypeId> {
        let expr = &self.ast.exprs[expr_id];

        let ty_id = match &expr.kind {
            ExprKind::CintLit(_) => self.types.cint,
            ExprKind::UintLit(_) => self.types.uint,
            ExprKind::IntLit(_) => self.types.int,
            ExprKind::FloatLit(_) => self.types.float,
            ExprKind::True => self.types.bool,
            ExprKind::False => self.types.bool,
            ExprKind::Char(_) => self.types.char,
            ExprKind::StrLit(_) => self.types.cstr,
            ExprKind::Null => self.types.null,
            ExprKind::Void => self.types.void,

            ExprKind::Path(path) => self.check_path(env.scope, *path, expr.span)?,

            ExprKind::ArrayLit(expr_ids) => todo!(),

            ExprKind::ArrayRepeat { value, count } => {
                let elem = self.check_expr(env, *value)?;
                let count = self.eval_expr(env.scope, *count)?;

                match self.values.get(count) {
                    SemaValue::Cint(n) => match n.get_unsigned() {
                        Some(len) => self.types.insert(SemaType::Array { elem, len }),
                        None => return Err(NegativeArrayLength { span: expr.span }),
                    },
                    SemaValue::Uint(n) => self.types.insert(SemaType::Array { elem, len: *n }),
                    v => {
                        return Err(TypeMismatch {
                            exp: self.types.cint,
                            got: self.get_value_ty(v)?,
                            span: expr.span,
                        });
                    }
                }
            }

            ExprKind::StructLit { ty, fields } => {
                self.check_struct_lit(env, *ty, fields, expr.span)?
            }

            ExprKind::Group(inner) => self.check_expr(env, *inner)?,
            ExprKind::Unary { op, rhs } => self.check_unary(env, *op, *rhs, expr.span)?,
            ExprKind::Binary { op, lhs, rhs } => {
                self.check_binary(env, *op, *lhs, *rhs, expr.span)?
            }

            ExprKind::Assign { op, tgt, val } => todo!(),
            ExprKind::Cast { expr, ty } => todo!(),
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.check_expr(env, *cond)?;

                // if expressions can only accept booleans as conditions
                self.coerce(self.types.bool, cond_ty, self.ast.exprs[*cond].span)?;

                let then_ty = self.check_expr(env, *then_branch)?;
                let else_ty = match else_branch {
                    Some(e) => self.check_expr(env, *e)?,
                    None => self.types.void,
                };

                self.unify_types(then_ty, else_ty, expr.span)?
            }
            ExprKind::Match { scrutinee, arms } => todo!(),
            ExprKind::While { cond, body } => todo!(),
            ExprKind::Loop(body) => todo!(),
            ExprKind::For {
                pattern,
                iter,
                body,
            } => todo!(),
            ExprKind::Block(stmts) => self.check_block(env, stmts)?,
            ExprKind::Return(value) => {
                let Some(fn_scope) = self.symbols.find_function_scope(env.scope) else {
                    return Err(ReturnOutsideFunction { span: expr.span });
                };

                let ret_value_type = match value {
                    Some(v) => self.check_expr(env, *v)?,
                    None => self.types.void,
                };

                // verify that return value type matches function return type
                self.coerce(env.return_type, ret_value_type, expr.span)?;

                // return expressions are always diverging
                self.types.never
            }
            ExprKind::Break(expr_id) => todo!(),
            ExprKind::Continue => todo!(),
            ExprKind::Call { callee, args } => todo!(),
            ExprKind::Field { object, field } => todo!(),
            ExprKind::OptionalField { object, field } => todo!(),
            ExprKind::Index { object, index } => {
                let object_ty = self.check_expr(env, *object)?;

                match self.types.get(object_ty) {
                    SemaType::Array { elem, .. } | SemaType::Slice(elem) => {
                        let ty = *elem;
                        let index_ty = self.check_expr(env, *index)?;

                        // array/slice can only be indexed by uint
                        self.coerce(self.types.uint, index_ty, self.ast.exprs[*index].span)?;

                        ty
                    }
                    _ => {
                        return Err(ExpectedIndexable {
                            found: object_ty,
                            span: self.ast.exprs[*object].span,
                        });
                    }
                }
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => todo!(),
            ExprKind::Unwrap(opt_id) => {
                let opt_ty = self.check_expr(env, *opt_id)?;

                // only optionals can be unwrapped
                match self.types.get(opt_ty) {
                    SemaType::Opt(inner) => *inner,
                    _ => {
                        return Err(ExpectedOptional {
                            found: opt_ty,
                            span: self.ast.exprs[*opt_id].span,
                        });
                    }
                }
            }
            ExprKind::Const(expr_id) => todo!(),
        };

        self.expr_types.insert(expr_id, ty_id);
        Ok(ty_id)
    }

    fn check_path(&mut self, scope: ScopeId, path: PathId, span: Span) -> Result<SemaTypeId> {
        let res = self.resolve_path(scope, path)?;
        let symbol = self.symbols.get(res.symbol);

        match symbol.kind {
            SymbolKind::Function(decl) | SymbolKind::Const(decl) => match symbol.ty_id {
                Some(ty) => Ok(ty),
                None => self.eval_decl_type(scope, res.symbol, decl),
            },

            // Variable must have been initialized before usage
            SymbolKind::Variable(_) => match symbol.ty_id {
                Some(ty) => Ok(ty),
                None => Err(UninitVariable {
                    name: symbol.name,
                    span,
                }),
            },

            SymbolKind::Variant(enum_sym) => {
                let enum_symbol = self.symbols.get(enum_sym);

                match enum_symbol.kind {
                    SymbolKind::Enum(decl) => match enum_symbol.ty_id {
                        Some(ty) => Ok(ty),
                        None => self.eval_decl_type(scope, enum_sym, decl),
                    },
                    _ => unreachable!("variant parent must be an enum"),
                }
            }

            _ => Err(ExpectedValue {
                found: res.symbol,
                span,
            }),
        }
    }

    fn check_struct_lit(
        &mut self,
        env: &FunctionEnv,
        ty: AstTypeId,
        fields: &[FieldInit],
        span: Span,
    ) -> Result<SemaTypeId> {
        let ty = self.eval_type(env.scope, ty)?;

        match self.types.get(ty) {
            SemaType::Struct(info) => {
                // we clone here to avoid borrowing issues
                let field_infos = Rc::clone(&info.fields);
                let mut seen_fields = AHashSet::new();

                for field_init in fields {
                    let field_info = field_infos
                        .iter()
                        .find(|f| f.name == field_init.name)
                        .ok_or(UndefinedField {
                            name: field_init.name,
                            span: field_init.span,
                        })?;

                    // check field has not been initialized
                    if !seen_fields.insert(field_init.name) {
                        return Err(DuplicateFieldInit {
                            name: field_init.name,
                            span: field_init.span,
                        });
                    }

                    // check that value can be assigned to field
                    let field_ty = field_info.ty;
                    let value_ty = self.check_expr(env, field_init.value)?;
                    self.coerce(field_ty, value_ty, field_init.span)?;
                }

                // check that all fields are initialized
                for field in field_infos.iter() {
                    if !seen_fields.contains(&field.name) {
                        return Err(MissingFieldInit {
                            ty,
                            name: field.name,
                            span,
                        });
                    }
                }

                Ok(ty)
            }

            SemaType::Union(info) => {
                let num_fields = fields.len();

                // unions can only be initialized with one field
                if num_fields != 1 {
                    return Err(UnionFieldInit { num_fields, span });
                }

                // check that union has field defined
                let field_init = &fields[0];
                let field_info = info
                    .fields
                    .iter()
                    .find(|f| f.name == field_init.name)
                    .ok_or(UndefinedField {
                        name: field_init.name,
                        span: field_init.span,
                    })?;

                // check that value can be assigned to field
                let field_ty = field_info.ty;
                let value_ty = self.check_expr(env, field_init.value)?;
                self.coerce(field_ty, value_ty, field_init.span)?;

                Ok(ty)
            }
            _ => Err(ExpectedStructOrUnion { ty, span }),
        }
    }

    fn check_unary(
        &mut self,
        env: &FunctionEnv,
        op: UnOp,
        value: ExprId,
        span: Span,
    ) -> Result<SemaTypeId> {
        let ty = self.check_expr(env, value)?;

        match (op, self.types.get(ty)) {
            (UnOp::Neg, SemaType::Int) => Ok(self.types.int),
            (UnOp::Neg, SemaType::Float) => Ok(self.types.float),

            (UnOp::Not, SemaType::Int) => Ok(self.types.int),
            (UnOp::Not, SemaType::Uint) => Ok(self.types.uint),
            (UnOp::Not, SemaType::Bool) => Ok(self.types.bool),

            (UnOp::Deref, SemaType::Ptr { pointee, .. }) => Ok(*pointee),

            _ => Err(InvalidUnaryOp { op, ty, span }),
        }
    }

    fn check_binary(
        &mut self,
        env: &FunctionEnv,
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
        span: Span,
    ) -> Result<SemaTypeId> {
        let lhs = self.check_expr(env, lhs)?;
        let rhs = self.check_expr(env, rhs)?;
        let (lhs, rhs) = self.coerce_numeric_operands(lhs, rhs);

        let lhs_ty = self.types.get(lhs);

        // Handle null coalesce operation. rhs must coerce to inner optional type.
        if op == BinOp::NullCoalesce
            && let SemaType::Opt(inner) = lhs_ty
        {
            self.coerce(*inner, rhs, span)?;
            return Ok(*inner);
        }

        let rhs_ty = self.types.get(rhs);

        use SemaType::*;

        match (op, (lhs_ty, rhs_ty)) {
            (
                BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Mod
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr,
                (Cint, Cint),
            ) => Ok(self.types.cint),

            (
                BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Mod
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr,
                (Int, Int),
            ) => Ok(self.types.int),

            (
                BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Mod
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr,
                (Uint, Uint),
            ) => Ok(self.types.uint),

            (BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod, (Float, Float)) => {
                Ok(self.types.float)
            }

            // TODO: consider cmp operation for float
            (
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge,
                (Int, Int) | (Uint, Uint) | (Float, Float),
            )
            | (BinOp::And | BinOp::Or, (Bool, Bool)) => Ok(self.types.bool),

            _ => Err(InvalidBinaryOp { op, lhs, rhs, span }),
        }
    }

    fn check_block(&mut self, env: &FunctionEnv, stmts: &[StmtId]) -> Result<SemaTypeId> {
        let mut block_type = self.types.void;

        if stmts.is_empty() {
            return Ok(block_type);
        }

        let last = stmts.len() - 1;

        for (i, stmt) in stmts.iter().enumerate() {
            let stmt = &self.ast.stmts[*stmt];

            match &stmt.kind {
                StmtKind::Empty => {}
                StmtKind::Let { pattern, ty, value } => {
                    let mut val_ty = self.check_expr(env, *value)?;

                    if let Some(ty) = ty {
                        let tgt_ty = self.eval_type(env.scope, *ty)?;
                        self.coerce(tgt_ty, val_ty, stmt.span)?;
                        val_ty = tgt_ty;
                    }

                    self.bind_variable_pattern(env.scope, *pattern, val_ty)?;
                }
                StmtKind::Semi(expr) => {
                    // Evaluate expression type, but result is surpressed by semicolon
                    let expr_ty = self.check_expr(env, *expr)?;

                    // Diverging expressions are always assigned to the block type.
                    if matches!(self.types.get(expr_ty), SemaType::Never) {
                        block_type = self.types.never
                    }
                }
                StmtKind::Expr(expr) => {
                    let expr_type = self.check_expr(env, *expr)?;

                    // Expression without semicolon must be the last or coerce to void type.
                    // We use coerce instead of direct comparison to support diverging expressions.
                    if i == last {
                        block_type = expr_type;
                    } else {
                        self.coerce(expr_type, self.types.void, stmt.span)?;
                    }
                }
            }
        }

        Ok(block_type)
    }

    fn eval_expr(&mut self, scope: ScopeId, expr_id: ExprId) -> Result<SemaValueId> {
        let expr = &self.ast.exprs[expr_id];

        let val = match &expr.kind {
            ExprKind::CintLit(c) => self.values.insert(SemaValue::Cint(*c)),
            ExprKind::UintLit(u) => self.values.insert(SemaValue::Uint(*u)),
            ExprKind::IntLit(i) => self.values.insert(SemaValue::Int(*i)),
            ExprKind::FloatLit(_) => todo!(),
            ExprKind::True => self.values.insert(SemaValue::Bool(true)),
            ExprKind::False => self.values.insert(SemaValue::Bool(false)),
            ExprKind::Char(c) => self.values.insert(SemaValue::Char(*c)),
            ExprKind::StrLit(s) => self.values.insert(SemaValue::Str(*s)),
            ExprKind::Null => self.values.insert(SemaValue::Null),
            ExprKind::Void => self.values.insert(SemaValue::Void),

            ExprKind::Path(path) => self.eval_path_expr(scope, *path, expr.span)?,

            ExprKind::ArrayLit(expr_ids) => todo!(),
            ExprKind::ArrayRepeat { value, count } => todo!(),
            ExprKind::StructLit { ty, fields } => todo!(),
            ExprKind::Group(expr_id) => todo!(),
            ExprKind::Unary { op, rhs: expr } => todo!(),
            ExprKind::Binary { op, lhs, rhs } => todo!(),
            ExprKind::Assign { op, tgt, val } => todo!(),
            ExprKind::Cast { expr, ty } => todo!(),
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => todo!(),
            ExprKind::Match { scrutinee, arms } => todo!(),
            ExprKind::While { cond, body } => todo!(),
            ExprKind::Loop(expr_id) => todo!(),
            ExprKind::For {
                pattern,
                iter,
                body,
            } => todo!(),
            ExprKind::Block(stmts) => todo!(),
            ExprKind::Return(expr_id) => todo!(),
            ExprKind::Break(expr_id) => todo!(),
            ExprKind::Continue => todo!(),
            ExprKind::Call { callee, args } => todo!(),
            ExprKind::Field { object, field } => todo!(),
            ExprKind::OptionalField { object, field } => todo!(),
            ExprKind::Index { object, index } => todo!(),
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => todo!(),
            ExprKind::Unwrap(expr_id) => todo!(),
            ExprKind::Const(expr_id) => todo!(),
        };

        Ok(val)
    }

    fn eval_path_expr(&mut self, scope: ScopeId, path: PathId, span: Span) -> Result<SemaValueId> {
        let res = self.resolve_path(scope, path)?;
        let symbol = self.symbols.get(res.symbol);

        match symbol.kind {
            SymbolKind::Function(decl) | SymbolKind::Const(decl) => {
                match self.value_bindings.get(&res.symbol) {
                    Some(val) => Ok(*val),
                    None => self.eval_value_decl(res.scope, res.symbol, decl),
                }
            }

            // Variable must have been initialized before usage
            SymbolKind::Variable(_) => match self.value_bindings.get(&res.symbol) {
                Some(val) => Ok(*val),
                None => Err(UninitVariable {
                    name: symbol.name,
                    span,
                }),
            },

            SymbolKind::Variant(parent) => {
                todo!("eval enum variant")
            }

            _ => Err(ExpectedValue {
                found: res.symbol,
                span,
            }),
        }
    }

    fn eval_value_decl(
        &mut self,
        scope: ScopeId,
        sym_id: SymbolId,
        decl_id: DeclId,
    ) -> Result<SemaValueId> {
        let decl = &self.ast.decls[decl_id];

        match &decl.kind {
            DeclKind::Function {
                generics,
                params,
                ret,
                body,
            } => {
                let value = self.values.insert(SemaValue::Function(decl_id));
                self.value_bindings.insert(sym_id, value);
                Ok(value)
            }

            DeclKind::Const { ty, value } => {
                let value = self.eval_expr(scope, *value)?;

                // TODO: check value matches type.

                self.value_bindings.insert(sym_id, value);
                Ok(value)
            }

            _ => Err(ExpectedValue {
                found: sym_id,
                span: decl.span,
            }),
        }
    }

    fn get_value_ty(&self, val: &SemaValue) -> Result<SemaTypeId> {
        match val {
            SemaValue::Cint(_) => Ok(self.types.cint),
            SemaValue::Int(_) => Ok(self.types.int),
            SemaValue::Uint(_) => Ok(self.types.uint),
            SemaValue::Bool(_) => Ok(self.types.bool),
            SemaValue::Float(_) => Ok(self.types.float),
            SemaValue::Char(_) => Ok(self.types.char),
            SemaValue::Str(_) => Ok(self.types.cstr),
            SemaValue::Null => Ok(self.types.null),
            SemaValue::Void => Ok(self.types.void),
            SemaValue::Array(sema_value_ids) => todo!(),
            SemaValue::Tuple(sema_value_ids) => todo!(),
            SemaValue::Struct(sema_type_id, ahash_map) => todo!(),
            SemaValue::Union(sema_type_id, _, sema_value_id) => todo!(),
            SemaValue::Variant(sema_type_id, sema_value_id) => todo!(),
            SemaValue::Function(decl_id) => todo!(),
        }
    }
}
