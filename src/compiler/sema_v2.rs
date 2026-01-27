use std::collections::hash_map::Entry;

use ahash::AHashMap;

use crate::{
    arena::{Arena, Ident, Interner},
    compiler::{
        ast::{
            AstArena, AstTypeId, AstTypeKind, DeclId, DeclKind, ExprId, ExprKind, Field, Param,
            PathId, PatternId, PatternKind, Variant,
        },
        error::ResolveError,
        sema::{
            sema_type::{
                EnumInfo, FieldInfo, SemaType, SemaTypeId, StructInfo, TypeArena, UnionInfo,
                VariantInfo,
            },
            sema_value::{SemaValue, SemaValueId, ValueArena},
        },
        tokens::Span,
    },
};

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
            Entry::Occupied(entry) => {
                let first_def = self.symbols[*entry.get()].span;
                Err(DuplicateSymbol {
                    name: symbol.name,
                    first_def,
                    dupe_def: symbol.span,
                })
            }
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

pub struct Sema<'a> {
    ast: &'a AstArena,
    interner: &'a Interner,
    symbols: SymbolTable,
    types: TypeArena,
    values: ValueArena,
    value_bindings: AHashMap<SymbolId, SemaValueId>,
    errors: Vec<ResolveError>,
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
            errors: vec![],
        }
    }

    #[inline]
    fn error(&mut self, err: ResolveError) {
        self.errors.push(err);
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

    fn can_assign(&self, tgt: SemaTypeId, value: SemaTypeId) -> bool {
        todo!()
    }

    fn register_decl(&mut self, decl_id: DeclId, scope: ScopeId) -> Result<()> {
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
                    self.register_decl(decl, mod_scope)?;
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

    pub fn analyze_function(
        &mut self,
        scope: ScopeId,
        symbol: SymbolId,
        fn_params: &[Param],
        ret: Option<AstTypeId>,
        body: ExprId,
        span: Span,
    ) -> Result<()> {
        // Evaluate param types and bind param names
        let mut params = Vec::with_capacity(fn_params.len());
        for param in fn_params {
            let param_ty = self.eval_type(scope, param.ty)?;
            self.bind_param(scope, param.pattern, param_ty)?;
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

        // Evaluate body type
        let body = self.eval_expr_type(scope, body)?;

        // Check if body type can be assigned to return type
        if !self.can_assign(ret, body) {
            return Err(TypeMismatch {
                exp: ret,
                got: body,
                span,
            });
        }

        Ok(())
    }

    fn bind_param(
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
                let len = self.eval_expr_value(scope, *len)?;

                match self.values.get(len) {
                    SemaValue::ComptimeInt(n) => match n.get_unsigned() {
                        Some(len) => Ok(self.types.insert(SemaType::Array { elem, len })),
                        None => Err(NegativeArrayLength { span: ast_ty.span }),
                    },
                    SemaValue::Uint(n) => Ok(self.types.insert(SemaType::Array { elem, len: *n })),
                    _ => Err(TypeMismatch {
                        exp: self.types.cint,
                        got: todo!("type of value"),
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
                    None => self.eval_type_decl(res.scope, res.symbol, decl),
                }
            }
            _ => Err(ExpectedType {
                found: res.symbol,
                span,
            }),
        }
    }

    fn resolve_path(&mut self, scope: ScopeId, path_id: PathId) -> Result<Lookup> {
        // TODO: handle generics
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

    fn eval_type_decl(
        &mut self,
        scope: ScopeId,
        sym_id: SymbolId,
        decl_id: DeclId,
    ) -> Result<SemaTypeId> {
        let decl = &self.ast.decls[decl_id];
        match &decl.kind {
            DeclKind::Struct { generics, fields } => {
                let struct_ty = self.types.insert(SemaType::Resolving(decl_id));

                self.symbols.get_mut(sym_id).ty_id = Some(struct_ty);
                *self.types.get_mut(struct_ty) = SemaType::Struct(StructInfo {
                    name: Some(decl.name),
                    fields: self.make_fields(scope, fields)?,
                });

                Ok(struct_ty)
            }
            DeclKind::Union { generics, fields } => {
                let union_ty = self.types.insert(SemaType::Resolving(decl_id));

                self.symbols.get_mut(sym_id).ty_id = Some(union_ty);
                *self.types.get_mut(union_ty) = SemaType::Union(UnionInfo {
                    name: Some(decl.name),
                    fields: self.make_fields(scope, fields)?,
                });

                Ok(union_ty)
            }
            DeclKind::Enum { base, variants } => {
                let enum_ty = self.types.insert(SemaType::Resolving(decl_id));
                self.symbols.get_mut(sym_id).ty_id = Some(enum_ty);

                let base = match base {
                    None => self.types.uint,
                    Some(base) => {
                        let base = &self.ast.types[*base];
                        match &base.kind {
                            AstTypeKind::Uint => self.types.uint,
                            AstTypeKind::Int => self.types.int,
                            _ => return Err(InvalidEnumBase { span: base.span }),
                        }
                    }
                };

                *self.types.get_mut(enum_ty) = SemaType::Enum(EnumInfo {
                    name: Some(decl.name),
                    base,
                    variants: self.make_variants(scope, base, variants)?,
                });

                Ok(enum_ty)
            }
            _ => Err(ExpectedType {
                found: sym_id,
                span: decl.span,
            }),
        }
    }

    #[inline(always)]
    fn make_fields(&mut self, scope: ScopeId, fields: &[Field]) -> Result<Vec<FieldInfo>> {
        let mut field_infos = Vec::with_capacity(fields.len());
        for field in fields {
            field_infos.push(FieldInfo {
                name: field.name,
                ty: self.eval_type(scope, field.ty)?,
            });
        }

        Ok(field_infos.into())
    }

    fn make_variants(
        &mut self,
        scope: ScopeId,
        base: SemaTypeId,
        variants: &[Variant],
    ) -> Result<Vec<VariantInfo>> {
        let mut variant_infos = Vec::with_capacity(variants.len());
        for variant in variants {
            let name = variant.name;
            let value = match variant.value {
                Some(expr_id) => self.eval_expr_value(scope, expr_id)?,
                None => self.values.insert(SemaValue::Void),
            };

            variant_infos.push(VariantInfo { name, value });
        }
        todo!("check duplicates and base type");
        Ok(variant_infos)
    }

    fn eval_expr_type(&mut self, scope: ScopeId, expr_id: ExprId) -> Result<SemaTypeId> {
        let expr = &self.ast.exprs[expr_id];

        let ty = match &expr.kind {
            ExprKind::IntLit(_) => self.types.cint,
            ExprKind::FloatLit(_) => self.types.float,
            ExprKind::True => self.types.bool,
            ExprKind::False => self.types.bool,
            ExprKind::Char(_) => self.types.char,
            ExprKind::StrLit(_) => self.types.cstr,
            ExprKind::Null => self.types.null,
            ExprKind::Void => self.types.void,

            ExprKind::Path(path) => self.eval_type_path(scope, *path, expr.span)?,
            ExprKind::ArrayLit(expr_ids) => todo!(),
            ExprKind::ArrayRepeat { value, count } => {
                let elem = self.eval_expr_type(scope, *value)?;
                let count = self.eval_expr_value(scope, *count)?;

                match self.values.get(count) {
                    SemaValue::ComptimeInt(n) => match n.get_unsigned() {
                        Some(len) => self.types.insert(SemaType::Array { elem, len }),
                        None => return Err(NegativeArrayLength { span: expr.span }),
                    },
                    SemaValue::Uint(n) => self.types.insert(SemaType::Array { elem, len: *n }),
                    _ => {
                        return Err(TypeMismatch {
                            exp: self.types.cint,
                            got: todo!("type of value"),
                            span: expr.span,
                        });
                    }
                }
            }
            ExprKind::StructLit { ty, fields } => todo!(),
            ExprKind::Group(inner) => self.eval_expr_type(scope, *inner)?,
            ExprKind::Unary { op, expr } => todo!(),
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
            ExprKind::Block(stmt_ids) => todo!(),
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

        Ok(ty)
    }

    fn eval_expr_value(&mut self, scope: ScopeId, expr_id: ExprId) -> Result<SemaValueId> {
        let expr = &self.ast.exprs[expr_id];

        let val = match &expr.kind {
            ExprKind::IntLit(n) => self.values.insert(SemaValue::from_int_lit(*n)),
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
            ExprKind::Unary { op, expr } => todo!(),
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
            ExprKind::Block(stmt_ids) => todo!(),
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
                let value = self.eval_expr_value(scope, *value)?;

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
}
