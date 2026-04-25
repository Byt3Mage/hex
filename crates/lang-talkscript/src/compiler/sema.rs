use std::{collections::hash_map::Entry, rc::Rc};

use ahash::{AHashMap, AHashSet};
use simple_ternary::tnr;

use crate::{
    arena::{Ident, Interner},
    compiler::{
        ast::{
            AstArena, AstType, AstTypeId, AstTypeKind, DeclId, DeclKind, ExprId, ExprKind,
            FieldDef, FieldInit, ParamDef, PathId, PatternId, PatternKind, StmtId, StmtKind,
            VariantDef,
        },
        error::{ResolveError, bug},
        mir::{self},
        op::{AssignOp, BinOp, UnOp},
        sema::{
            sema_type::{
                EnumInfo, FieldInfo, SemaType, SemaTypeId, StructInfo, TypeArena, UnionInfo,
                VariantInfo,
            },
            sema_value::{ComptimeInt, SemaValue, SemaValueId, ValueArena},
        },
        symbol_table::{Lookup, ScopeId, ScopeKind, Symbol, SymbolId, SymbolKind, SymbolTable},
        tokens::Span,
    },
};

pub mod sema_type;
pub mod sema_value;

use ResolveError::*;
type Result<T> = std::result::Result<T, ResolveError>;

pub struct FunctionEnv {
    fn_scope: ScopeId,
    ret_type: SemaTypeId,
    values: mir::ValueAllocator,
}

pub struct Sema<'a> {
    ast: &'a AstArena,
    interner: &'a Interner,
    symbols: SymbolTable,
    types: TypeArena,
    values: ValueArena,
    value_bindings: AHashMap<SymbolId, SemaValueId>,
    expr_types: AHashMap<ExprId, SemaTypeId>,

    pattern_symbols: AHashMap<PatternId, SymbolId>,
    func_ids: AHashMap<DeclId, mir::FuncId>,
    next_func_id: u32,
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
            pattern_symbols: AHashMap::new(),
            func_ids: AHashMap::new(),
            next_func_id: 0,
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

    fn alloc_func_id(&mut self, decl: DeclId) -> mir::FuncId {
        let id = mir::FuncId(self.next_func_id);
        self.next_func_id += 1;
        self.func_ids.insert(decl, id);
        id
    }

    pub fn register_root_module(&mut self, root_decl: DeclId) -> Result<ScopeId> {
        let root_scope = self.symbols.scope(ScopeKind::Module, None);
        self.symbols.decl_scopes.insert(root_decl, root_scope);

        match &self.ast.decls[root_decl].kind {
            DeclKind::Module(decls) => {
                for &decl in decls {
                    self.register_decl(decl, root_scope)?;
                }
            }
            _ => bug!("expected module decl"),
        }

        Ok(root_scope)
    }

    fn register_decl(&mut self, decl_id: DeclId, in_scope: ScopeId) -> Result<()> {
        let decl = &self.ast.decls[decl_id];

        match &decl.kind {
            DeclKind::Module(decls) => {
                let mod_symbol = self.define(
                    Symbol {
                        kind: SymbolKind::Module(decl_id),
                        name: decl.name,
                        span: decl.span,
                        ty_id: None,
                    },
                    in_scope,
                )?;

                self.symbols.decl_symbols.insert(decl_id, mod_symbol);

                let mod_scope = self.symbols.scope(ScopeKind::Module, Some(in_scope));
                self.symbols.decl_scopes.insert(decl_id, mod_scope);

                for &decl in decls {
                    self.register_decl(decl, mod_scope)?;
                }
            }
            DeclKind::Function { .. } => {
                let fn_symbol = self.define(
                    Symbol {
                        kind: SymbolKind::Function(decl_id),
                        name: decl.name,
                        span: decl.span,
                        ty_id: None,
                    },
                    in_scope,
                )?;
                self.symbols.decl_symbols.insert(decl_id, fn_symbol);

                let fn_scope = self.symbols.scope(ScopeKind::Function, Some(in_scope));
                self.symbols.decl_scopes.insert(decl_id, fn_scope);
            }
            DeclKind::Enum { variants, .. } => {
                let enum_symbol = self.define(
                    Symbol {
                        kind: SymbolKind::Enum(decl_id),
                        name: decl.name,
                        span: decl.span,
                        ty_id: None,
                    },
                    in_scope,
                )?;

                self.symbols.decl_symbols.insert(decl_id, enum_symbol);

                let enum_scope = self.symbols.scope(ScopeKind::Enum, Some(in_scope));
                self.symbols.decl_scopes.insert(decl_id, enum_scope);

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
                let const_symbol = self.define(
                    Symbol {
                        kind: SymbolKind::Const(decl_id),
                        name: decl.name,
                        span: decl.span,
                        ty_id: None,
                    },
                    in_scope,
                )?;
                self.symbols.decl_symbols.insert(decl_id, const_symbol);
            }
            DeclKind::Struct { .. } => {
                let struct_symbol = self.define(
                    Symbol {
                        kind: SymbolKind::Struct(decl_id),
                        name: decl.name,
                        span: decl.span,
                        ty_id: None,
                    },
                    in_scope,
                )?;

                self.symbols.decl_symbols.insert(decl_id, struct_symbol);
            }
            DeclKind::Union { .. } => {
                let union_symbol = self.define(
                    Symbol {
                        kind: SymbolKind::Union(decl_id),
                        name: decl.name,
                        span: decl.span,
                        ty_id: None,
                    },
                    in_scope,
                )?;
                self.symbols.decl_symbols.insert(decl_id, union_symbol);
            }
        }

        Ok(())
    }

    // Type-check and lower a function declaration to MIR
    pub fn analyze_function(&mut self, decl_id: DeclId) -> Result<mir::Function> {
        let decl = &self.ast.decls[decl_id];
        let fn_scope = self.symbols.decl_scopes[&decl_id];
        let parent_scope = self
            .symbols
            .parent_scope(fn_scope)
            .expect("expected parent");

        let DeclKind::Function {
            generics: _,
            params,
            ret,
            body,
        } = &decl.kind
        else {
            bug!("expected function declaration");
        };

        // Clone what we need before mutating self
        let params = params.clone();
        let ret = *ret;
        let body = *body;

        // Step 1: Evaluate parameter types and bind parameter names
        let mut param_symbols = Vec::with_capacity(params.len());
        let mut param_sema_types = Vec::with_capacity(params.len());

        for param in &params {
            let param_ty = self.eval_type(parent_scope, param.ty)?;
            self.bind_variable_pattern(fn_scope, param.pattern, param_ty)?;
            param_sema_types.push(param_ty);
        }

        // Step 2: Evaluate return type
        let ret_type = match ret {
            Some(ty) => self.eval_type(parent_scope, ty)?,
            None => self.types.void,
        };

        // Step 3: Type-check the body
        let env = FunctionEnv {
            fn_scope,
            ret_type,
            values: mir::ValueAllocator::new(),
        };
        let body_type = self.check_expr(&env, body)?;

        // Step 4: Check that body value can be assigned to return type
        self.coerce(ret_type, body_type, decl.span)?;

        // Step 5: Assign a MIR FuncId
        let func_id = self.alloc_func_id(decl_id);

        // Step 6: Lower to MIR
        let mut builder = MirBuilder::new(self.ast, self.interner, &self.types, &self.expr_types);

        // Bind parameter values in the builder
        let mut mir_params = Vec::with_capacity(params.len());
        let mut mir_param_tys = Vec::with_capacity(params.len());
        for (sym, sema_ty) in param_symbols.iter().zip(&param_sema_types) {
            let val = builder.alloc_value();
            builder.var_map.insert(*sym, val);
            mir_params.push(val);
            mir_param_tys.push(builder.mir_type(*sema_ty));
        }

        // Set up return value slot
        let no_ret_val = matches!(self.types.get(ret_type), SemaType::Void | SemaType::Never);

        if !no_ret_val {
            builder.ret_value = Some(builder.alloc_value());
        }

        // Lower body
        let result = builder.lower_expr(body);

        // If body produced a value, copy to return slot and jump to return block
        if !builder.is_terminated() {
            if let (Some(ret_slot), Some(src)) = (builder.ret_value, result) {
                let ty = builder.mir_type(ret_type);
                builder.emit(mir::Inst::Copy {
                    dst: ret_slot,
                    src,
                    ty,
                });
            }

            builder.terminate(mir::Terminator::Jump {
                target: builder.ret_block,
                args: vec![],
            });
        }

        // Patch return block
        let ret_block = builder.ret_block;
        builder.blocks[ret_block].terminator = mir::Terminator::Return {
            value: builder.ret_value,
        };

        Ok(mir::Function {
            id: func_id,
            name: self.interner.resolve(decl.name).unwrap_or("?").to_owned(),
            params: mir_params,
            param_tys: mir_param_tys,
            return_ty: no_ret_val.then(|| builder.mir_type(ret_type)),
            blocks: builder.blocks,
            next_value: builder.values.alloc().0,
        })
    }

    fn build_fn_signature(
        &mut self,
        scope: ScopeId,
        symbol: SymbolId,
        params: &[ParamDef],
        ret: Option<AstTypeId>,
    ) -> Result<SemaTypeId> {
        let mut param_types = Vec::with_capacity(params.len());
        for def in params {
            param_types.push(self.eval_type(scope, def.ty)?);
        }

        let ret_type = match ret {
            Some(r) => self.eval_type(scope, r)?,
            None => self.types.void,
        };

        let signature = self
            .types
            .insert(SemaType::Function(param_types.into(), ret_type));
        self.symbols.get_mut(symbol).ty_id = Some(signature);

        Ok(signature)
    }

    fn bind_variable_pattern(
        &mut self,
        scope: ScopeId,
        pattern_id: PatternId,
        expected_ty: SemaTypeId,
    ) -> Result<()> {
        let pattern = &self.ast.patterns[pattern_id];

        // Recursively walk patterns and create symbols for identifiers.
        // Refutable patterns can't be used as variables or parameters.
        match &pattern.kind {
            PatternKind::Int(_)
            | PatternKind::Float(_)
            | PatternKind::Bool(_)
            | PatternKind::Char(_)
            | PatternKind::CStr(_)
            | PatternKind::Path(_)
            | PatternKind::Range { .. } => Err(RefutableVariablePattern {
                pattern_id,
                expected_ty,
                span: pattern.span,
            }),

            PatternKind::Wildcard | PatternKind::Rest => Ok(()),

            PatternKind::Identifier { name, mutable } => {
                let symbol = self.define(
                    Symbol {
                        kind: SymbolKind::Variable(*mutable),
                        name: *name,
                        span: pattern.span,
                        ty_id: Some(expected_ty),
                    },
                    scope,
                )?;

                self.pattern_symbols.insert(pattern_id, symbol);
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
            AstTypeKind::Any => Ok(self.types.any),
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

                Ok(self
                    .types
                    .insert(SemaType::Function(fn_params.into(), fn_ret)))
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
                    curr.scope = self.symbols.decl_scopes[&decl];
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
                let signature = self.types.insert(SemaType::Function(fn_params.into(), ret));
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

                let base = self.make_enum_base(*base)?;
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

    #[inline]
    fn make_enum_base(&mut self, ast_base: Option<AstTypeId>) -> Result<SemaTypeId> {
        let base = match ast_base {
            None => self.types.uint, // default enum base type is uint
            Some(b) => {
                // enum base type must be uint or int
                let AstType { kind, span } = &self.ast.types[b];
                match kind {
                    AstTypeKind::Int => self.types.int,
                    AstTypeKind::Uint => self.types.uint,
                    _ => return Err(InvalidEnumBaseType { span: *span }),
                }
            }
        };

        Ok(base)
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

            ExprKind::Path(path) => self.check_path(env.fn_scope, *path, expr.span)?,

            ExprKind::ArrayLit(expr_ids) => todo!(),

            ExprKind::ArrayRepeat { value, count } => {
                let elem = self.check_expr(env, *value)?;
                let count = self.eval_expr(env.fn_scope, *count)?;

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
                let Some(fn_scope) = self.symbols.find_function_scope(env.fn_scope) else {
                    return Err(ReturnOutsideFunction { span: expr.span });
                };

                let ret_value_type = match value {
                    Some(v) => self.check_expr(env, *v)?,
                    None => self.types.void,
                };

                // verify that return value type matches function return type
                self.coerce(env.ret_type, ret_value_type, expr.span)?;

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
                    _ => bug!("variant parent must be an enum"),
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
        let ty = self.eval_type(env.fn_scope, ty)?;

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

                // unions must be initialized with only one field
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
                        let tgt_ty = self.eval_type(env.fn_scope, *ty)?;
                        self.coerce(tgt_ty, val_ty, stmt.span)?;
                        val_ty = tgt_ty;
                    }

                    self.bind_variable_pattern(env.fn_scope, *pattern, val_ty)?;
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

/// Tracks the MIR block being built and the break/continue targets for loops.
struct LoopContext {
    /// mir::Block to jump to on `break`
    break_block: mir::BlockId,
    /// mir::Block to jump to on `continue`
    continue_block: mir::BlockId,
    /// Value to write break results into (if the loop is used as an expression)
    break_dest: Option<mir::Value>,
}

/// Lowers a type-checked function body into MIR.
///
/// The builder maintains a "current block" that instructions are appended to.
/// Control flow (if/while/loop) creates new blocks and wires them with terminators.
struct MirBuilder<'a> {
    ast: &'a AstArena,
    interner: &'a Interner,
    types: &'a TypeArena,
    expr_types: &'a AHashMap<ExprId, SemaTypeId>,

    /// MIR type table being built (maps SemaTypeId → mir::TypeId)
    mir_types: mir::TypeTable,
    type_map: AHashMap<SemaTypeId, mir::TypeId>,

    /// Function-level state
    values: mir::ValueAllocator,
    blocks: Vec<mir::Block>,
    current_block: mir::BlockId,

    /// Variable bindings: symbol → MIR value
    var_map: AHashMap<SymbolId, mir::Value>,

    /// Loop context stack for break/continue
    loop_stack: Vec<LoopContext>,

    /// Return block and return slot
    ret_block: mir::BlockId,
    ret_value: Option<mir::Value>,
}

impl<'a> MirBuilder<'a> {
    fn new(
        ast: &'a AstArena,
        interner: &'a Interner,
        types: &'a TypeArena,
        expr_types: &'a AHashMap<ExprId, SemaTypeId>,
    ) -> Self {
        let mir_types = mir::TypeTable { types: vec![] };
        let entry_block = mir::Block {
            id: 0,
            params: vec![],
            insts: vec![],
            terminator: mir::Terminator::Unreachable,
        };

        // Reserve block 0 for the return block, block 1 for entry.
        // We'll swap them at the end so entry is 0.
        let ret_block = mir::Block {
            id: 1,
            params: vec![],
            insts: vec![],
            terminator: mir::Terminator::Unreachable,
        };

        Self {
            ast,
            interner,
            types,
            expr_types,
            mir_types,
            type_map: AHashMap::new(),
            values: mir::ValueAllocator::new(),
            blocks: vec![entry_block, ret_block],
            current_block: 0,
            var_map: AHashMap::new(),
            loop_stack: vec![],
            ret_block: 1,
            ret_value: None,
        }
    }

    // ── Helpers ───────────────────────────────

    fn alloc_value(&mut self) -> mir::Value {
        self.values.alloc()
    }

    fn new_block(&mut self) -> mir::BlockId {
        let id = self.blocks.len();
        self.blocks.push(mir::Block {
            id,
            params: vec![],
            insts: vec![],
            terminator: mir::Terminator::Unreachable,
        });
        id
    }

    fn emit(&mut self, inst: mir::Inst) {
        self.blocks[self.current_block].insts.push(inst);
    }

    fn switch_to(&mut self, block: mir::BlockId) {
        self.current_block = block;
    }

    fn terminate(&mut self, term: mir::Terminator) {
        self.blocks[self.current_block].terminator = term;
    }

    fn is_terminated(&self) -> bool {
        !matches!(
            self.blocks[self.current_block].terminator,
            mir::Terminator::Unreachable
        )
    }

    /// Resolve a SemaTypeId to a MIR scalar type for use in BinOp/UnOp.
    fn scalar_type(&self, ty: SemaTypeId) -> mir::ScalarType {
        match self.types.get(ty) {
            SemaType::Int | SemaType::Cint => mir::ScalarType::Int,
            SemaType::Uint => mir::ScalarType::UInt,
            SemaType::Float => mir::ScalarType::Float,
            SemaType::Bool => mir::ScalarType::Bool,
            SemaType::Ptr { .. } => mir::ScalarType::Pointer,
            _ => bug!("scalar_type called on non-scalar type"),
        }
    }

    /// Get or create a MIR TypeId for a SemaTypeId.
    fn mir_type(&mut self, sema_ty: SemaTypeId) -> mir::TypeId {
        if let Some(&id) = self.type_map.get(&sema_ty) {
            return id;
        }

        let info = match self.types.get(sema_ty) {
            SemaType::Int | SemaType::Cint => mir::TypeInfo::Scalar(mir::ScalarType::Int),
            SemaType::Uint => mir::TypeInfo::Scalar(mir::ScalarType::UInt),
            SemaType::Float => mir::TypeInfo::Scalar(mir::ScalarType::Float),
            SemaType::Bool => mir::TypeInfo::Scalar(mir::ScalarType::Bool),
            SemaType::Char => mir::TypeInfo::Scalar(mir::ScalarType::Int),
            SemaType::Ptr { .. } => mir::TypeInfo::Scalar(mir::ScalarType::Pointer),

            SemaType::Struct(info) => {
                let fields: Vec<mir::FieldLayout> = info
                    .fields
                    .iter()
                    .enumerate()
                    .scan(0usize, |offset, (_, f)| {
                        let ty = self.mir_type(f.ty);
                        let field_size = self.mir_type_size(ty);
                        let field_offset = *offset;
                        *offset += field_size;
                        Some(mir::FieldLayout {
                            name: self.interner.resolve(f.name).unwrap_or("?").to_owned(),
                            ty,
                            offset: field_offset,
                        })
                    })
                    .collect();
                let size = fields.iter().map(|f| self.mir_type_size(f.ty)).sum();
                let name = info
                    .name
                    .and_then(|n| self.interner.resolve(n))
                    .unwrap_or("anon")
                    .to_owned();
                mir::TypeInfo::Struct(mir::StructLayout { name, fields, size })
            }

            SemaType::Union(info) => {
                let fields: Vec<mir::FieldLayout> = info
                    .fields
                    .iter()
                    .map(|f| {
                        let ty = self.mir_type(f.ty);
                        mir::FieldLayout {
                            name: self.interner.resolve(f.name).unwrap_or("?").to_owned(),
                            ty,
                            offset: 1, // all union fields start after the tag
                        }
                    })
                    .collect();
                let max_field = fields
                    .iter()
                    .map(|f| self.mir_type_size(f.ty))
                    .max()
                    .unwrap_or(0);
                let size = 1 + max_field; // tag + largest field
                let name = info
                    .name
                    .and_then(|n| self.interner.resolve(n))
                    .unwrap_or("anon")
                    .to_owned();
                mir::TypeInfo::Union(mir::UnionLayout { name, fields, size })
            }

            SemaType::Array { elem, len } => {
                let elem_ty = self.mir_type(*elem);
                let elem_size = self.mir_type_size(elem_ty);
                let length = *len as usize;
                mir::TypeInfo::Array(mir::ArrayLayout {
                    elem_ty,
                    length,
                    elem_size,
                    size: elem_size * length,
                })
            }

            SemaType::Function(params, ret) => {
                // Function values are just pointers (scalar), but we track the
                // signature in the type table for call emission.
                let param_tys = params.iter().map(|p| self.mir_type(*p)).collect();
                let ret_ty = if *ret == self.types.void {
                    None
                } else {
                    Some(self.mir_type(*ret))
                };
                mir::TypeInfo::FuncPtr(param_tys, ret_ty)
            }

            // Void, Never, Null, Enum, etc. — these don't occupy registers
            // in most contexts. We map them to a zero-sized scalar.
            _ => mir::TypeInfo::Scalar(mir::ScalarType::Int),
        };

        let id = mir::TypeId(self.mir_types.types.len() as u32);
        self.mir_types.types.push(info);
        self.type_map.insert(sema_ty, id);
        id
    }

    fn mir_type_size(&self, ty: mir::TypeId) -> usize {
        match &self.mir_types.types[ty.0 as usize] {
            mir::TypeInfo::Scalar(_) | mir::TypeInfo::FuncPtr(..) => 1,
            mir::TypeInfo::Struct(s) => s.size,
            mir::TypeInfo::Array(a) => a.size,
            mir::TypeInfo::Union(u) => u.size,
        }
    }

    fn lower_function(
        &mut self,
        name: String,
        func_id: mir::FuncId,
        params: &[ParamDef],
        param_symbols: &[SymbolId],
        ret_ty: SemaTypeId,
        body: ExprId,
    ) -> mir::Function {
        // Set up return slot
        if ret_ty != self.types.void && ret_ty != self.types.never {
            let ret_val = self.alloc_value();
            self.ret_value = Some(ret_val);
        }

        // Allocate MIR values for parameters and bind them
        let mut mir_params = Vec::with_capacity(params.len());
        let mut mir_param_tys = Vec::with_capacity(params.len());
        for (param, &sym_id) in params.iter().zip(param_symbols) {
            let val = self.alloc_value();
            let sema_ty = self.expr_types.get(&ExprId::from(0)); // won't work — use symbol ty
            mir_params.push(val);
            self.var_map.insert(sym_id, val);
        }

        // We need param types from symbols, not expr_types
        // This is handled by the caller setting up param_tys

        // Lower the body
        let result = self.lower_expr(body);

        // If the body produced a value and we're not already terminated,
        // write it to the return slot and jump to the return block.
        if !self.is_terminated() {
            if let Some(result_val) = result {
                if let Some(ret_val) = self.ret_value {
                    let ty = self.mir_type(ret_ty);
                    self.emit(mir::Inst::Copy {
                        dst: ret_val,
                        src: result_val,
                        ty,
                    });
                }
            }
            self.terminate(mir::Terminator::Jump {
                target: self.ret_block,
                args: vec![],
            });
        }

        // Patch the return block's terminator
        self.blocks[self.ret_block].terminator = mir::Terminator::Return {
            value: self.ret_value,
        };

        let return_ty = if ret_ty != self.types.void {
            Some(self.mir_type(ret_ty))
        } else {
            None
        };

        mir::Function {
            id: func_id,
            name,
            params: mir_params,
            param_tys: mir_param_tys,
            return_ty,
            blocks: std::mem::take(&mut self.blocks),
            next_value: self.values.alloc().0, // hacky but gets the count
        }
    }

    fn lower_expr(&mut self, expr_id: ExprId) -> Option<mir::Value> {
        let expr = &self.ast.exprs[expr_id];
        let sema_ty = self.expr_types[&expr_id];

        match &expr.kind {
            ExprKind::CintLit(c) => {
                let dst = self.alloc_value();
                let lit = match c.get_signed() {
                    Some(i) => mir::Literal::Int(i),
                    None => mir::Literal::Uint(c.get_unsigned().unwrap()),
                };
                self.emit(mir::Inst::Const { dst, val: lit });
                Some(dst)
            }

            ExprKind::IntLit(i) => {
                let dst = self.alloc_value();
                let val = mir::Literal::Int(*i);
                self.emit(mir::Inst::Const { dst, val });
                Some(dst)
            }

            ExprKind::UintLit(u) => {
                let dst = self.alloc_value();
                let val = mir::Literal::Uint(*u);
                self.emit(mir::Inst::Const { dst, val });
                Some(dst)
            }

            ExprKind::FloatLit(f) => {
                let dst = self.alloc_value();
                let val = mir::Literal::Float(*f);
                self.emit(mir::Inst::Const { dst, val });
                Some(dst)
            }

            ExprKind::True => {
                let dst = self.alloc_value();
                let val = mir::Literal::Bool(true);
                self.emit(mir::Inst::Const { dst, val });
                Some(dst)
            }

            ExprKind::False => {
                let dst = self.alloc_value();
                let val = mir::Literal::Bool(false);
                self.emit(mir::Inst::Const { dst, val });
                Some(dst)
            }

            ExprKind::Char(c) => {
                let dst = self.alloc_value();
                let val = mir::Literal::Char(*c);
                self.emit(mir::Inst::Const { dst, val });
                Some(dst)
            }

            ExprKind::StrLit(s) => {
                let dst = self.alloc_value();
                let val = mir::Literal::Str(*s);
                self.emit(mir::Inst::Const { dst, val });
                Some(dst)
            }

            ExprKind::Null | ExprKind::Void => None,

            // ── Variables / paths ─────────────
            ExprKind::Path(path) => {
                let path = &self.ast.paths[*path];

                // Simple single-segment path → variable or function reference
                if path.is_simple() {
                    // Look up in var_map first
                    // For now, we just handle the simple local variable case.
                    // Function references and qualified paths need the symbol table.
                    // This is a simplification — in a full impl we'd resolve through
                    // the symbol table here too.
                    None // handled below via symbol resolution
                } else {
                    None
                }
            }

            ExprKind::Group(inner) => self.lower_expr(*inner),

            ExprKind::Unary { op, rhs } => {
                let src = self.lower_expr(*rhs)?;
                let dst = self.alloc_value();
                let rhs_ty = self.expr_types[rhs];

                self.emit(mir::Inst::UnOp {
                    dst,
                    src,
                    op: *op,
                    ty: self.scalar_type(rhs_ty),
                });
                Some(dst)
            }

            // ── Binary ───────────────────────
            ExprKind::Binary { op, lhs, rhs } => {
                let lhs_val = self.lower_expr(*lhs)?;
                let rhs_val = self.lower_expr(*rhs)?;
                let dst = self.alloc_value();

                // The result type determines which instruction variant we use.
                // For comparisons, the operand type matters, not the result.
                let operand_ty = self.expr_types[lhs];
                self.emit(mir::Inst::BinOp {
                    dst,
                    lhs: lhs_val,
                    rhs: rhs_val,
                    op: *op,
                    ty: self.scalar_type(operand_ty),
                });
                Some(dst)
            }

            ExprKind::Assign { op, tgt, val } => {
                let rhs = self.lower_expr(*val)?;

                match *op {
                    AssignOp::Assign => {
                        // For simple variable targets, just copy.
                        // For field/index targets, we'd need store instructions.
                        if let Some(lhs) = self.lower_lvalue(*tgt) {
                            let ty = self.mir_type(sema_ty);
                            self.emit(mir::Inst::Copy {
                                dst: lhs,
                                src: rhs,
                                ty,
                            });
                        }
                    }
                    _ => {
                        // Compound assignment: load, compute, store
                        if let Some(lhs) = self.lower_lvalue(*tgt) {
                            let bin_op = assign_op_to_bin_op(*op);
                            let tmp = self.alloc_value();
                            let operand_ty = self.expr_types[tgt];
                            self.emit(mir::Inst::BinOp {
                                dst: tmp,
                                lhs,
                                rhs,
                                op: bin_op,
                                ty: self.scalar_type(operand_ty),
                            });
                            let ty = self.mir_type(operand_ty);
                            self.emit(mir::Inst::Copy {
                                dst: lhs,
                                src: tmp,
                                ty,
                            });
                        }
                    }
                }
                None
            }

            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_val = self.lower_expr(*cond)?;

                let then_block = self.new_block();
                let else_block = self.new_block();
                let merge_block = self.new_block();

                self.terminate(mir::Terminator::BranchIf {
                    cond: cond_val,
                    then_target: then_block,
                    then_args: vec![],
                    else_target: else_block,
                    else_args: vec![],
                });

                // Result value for the if expression
                let is_void = sema_ty == self.types.void;
                let result = if is_void {
                    None
                } else {
                    Some(self.alloc_value())
                };

                // Then branch
                self.switch_to(then_block);
                let then_val = self.lower_expr(*then_branch);
                if !self.is_terminated() {
                    if let (Some(dst), Some(src)) = (result, then_val) {
                        let ty = self.mir_type(sema_ty);
                        self.emit(mir::Inst::Copy { dst, src, ty });
                    }
                    self.terminate(mir::Terminator::Jump {
                        target: merge_block,
                        args: vec![],
                    });
                }

                // Else branch
                self.switch_to(else_block);
                if let Some(else_expr) = else_branch {
                    let else_val = self.lower_expr(*else_expr);
                    if !self.is_terminated() {
                        if let (Some(dst), Some(src)) = (result, else_val) {
                            let ty = self.mir_type(sema_ty);
                            self.emit(mir::Inst::Copy { dst, src, ty });
                        }
                        self.terminate(mir::Terminator::Jump {
                            target: merge_block,
                            args: vec![],
                        });
                    }
                } else {
                    self.terminate(mir::Terminator::Jump {
                        target: merge_block,
                        args: vec![],
                    });
                }

                self.switch_to(merge_block);
                result
            }

            ExprKind::While { cond, body } => {
                let cond_block = self.new_block();
                let body_block = self.new_block();
                let exit_block = self.new_block();

                self.terminate(mir::Terminator::Jump {
                    target: cond_block,
                    args: vec![],
                });

                // Condition
                self.switch_to(cond_block);
                let cond_val = self.lower_expr(*cond).unwrap();
                self.terminate(mir::Terminator::BranchIf {
                    cond: cond_val,
                    then_target: body_block,
                    then_args: vec![],
                    else_target: exit_block,
                    else_args: vec![],
                });

                // Body
                self.loop_stack.push(LoopContext {
                    break_block: exit_block,
                    continue_block: cond_block,
                    break_dest: None,
                });

                self.switch_to(body_block);
                self.lower_expr(*body);
                if !self.is_terminated() {
                    self.terminate(mir::Terminator::Jump {
                        target: cond_block,
                        args: vec![],
                    });
                }

                self.loop_stack.pop();
                self.switch_to(exit_block);
                None // while is always void
            }

            // ── Loop ─────────────────────────
            ExprKind::Loop(body) => {
                let body_block = self.new_block();
                let exit_block = self.new_block();

                // If the loop type is non-void, allocate a result slot for break values
                let is_void = sema_ty == self.types.void || sema_ty == self.types.never;
                let break_dest = if is_void {
                    None
                } else {
                    Some(self.alloc_value())
                };

                self.terminate(mir::Terminator::Jump {
                    target: body_block,
                    args: vec![],
                });

                self.loop_stack.push(LoopContext {
                    break_block: exit_block,
                    continue_block: body_block,
                    break_dest,
                });

                self.switch_to(body_block);
                self.lower_expr(*body);
                if !self.is_terminated() {
                    self.terminate(mir::Terminator::Jump {
                        target: body_block,
                        args: vec![],
                    });
                }

                self.loop_stack.pop();
                self.switch_to(exit_block);
                break_dest
            }

            ExprKind::Block(stmts) => self.lower_block(stmts),

            ExprKind::Return(value) => {
                if let Some(val_expr) = value {
                    let val = self.lower_expr(*val_expr);
                    if let (Some(ret_slot), Some(v)) = (self.ret_value, val) {
                        let ret_ty = self.expr_types[val_expr];
                        let ty = self.mir_type(ret_ty);
                        self.emit(mir::Inst::Copy {
                            dst: ret_slot,
                            src: v,
                            ty,
                        });
                    }
                }
                self.terminate(mir::Terminator::Jump {
                    target: self.ret_block,
                    args: vec![],
                });
                None
            }

            // ── Break ────────────────────────
            ExprKind::Break(value) => {
                let ctx = self.loop_stack.last().expect("break outside loop");
                let break_block = ctx.break_block;
                let break_dest = ctx.break_dest;

                if let Some(val_expr) = value {
                    let val = self.lower_expr(*val_expr);
                    if let (Some(dst), Some(src)) = (break_dest, val) {
                        let val_ty = self.expr_types[val_expr];
                        let ty = self.mir_type(val_ty);
                        self.emit(mir::Inst::Copy { dst, src, ty });
                    }
                }

                self.terminate(mir::Terminator::Jump {
                    target: break_block,
                    args: vec![],
                });
                None
            }

            // ── Continue ─────────────────────
            ExprKind::Continue => {
                let ctx = self.loop_stack.last().expect("continue outside loop");
                let continue_block = ctx.continue_block;
                self.terminate(mir::Terminator::Jump {
                    target: continue_block,
                    args: vec![],
                });
                None
            }

            // ── Call ─────────────────────────
            ExprKind::Call { callee, args } => {
                let mut arg_vals = Vec::with_capacity(args.len());
                for &arg in args {
                    if let Some(v) = self.lower_expr(arg) {
                        arg_vals.push(v);
                    }
                }

                let callee_ty = self.expr_types[callee];

                // Check if callee is a direct function reference
                let is_void = sema_ty == self.types.void;

                // For simplicity, try direct call via path, fall back to indirect.
                if let ExprKind::Path(path) = &self.ast.exprs[*callee].kind {
                    let path = &self.ast.paths[*path];
                    // In a full impl, resolve the path to a mir::FuncId.
                    // For now, emit indirect call through the callee value.
                }

                let callee_val = self.lower_expr(*callee);

                if is_void {
                    if let Some(func_ptr) = callee_val {
                        self.emit(mir::Inst::CallIndirectVoid {
                            func_ptr,
                            args: arg_vals,
                        });
                    }
                    None
                } else {
                    let dst = self.alloc_value();
                    if let Some(func_ptr) = callee_val {
                        self.emit(mir::Inst::CallIndirect {
                            dst,
                            func_ptr,
                            args: arg_vals,
                        });
                    }
                    Some(dst)
                }
            }

            // ── Field access ─────────────────
            ExprKind::Field { object, field } => {
                let base = self.lower_expr(*object)?;
                let object_ty = self.expr_types[object];
                let base_mir_ty = self.mir_type(object_ty);

                // Find field index
                let field_idx = self.find_field_index(object_ty, *field);
                let dst = self.alloc_value();
                self.emit(mir::Inst::FieldAddr {
                    dst,
                    base,
                    field: field_idx,
                    base_ty: base_mir_ty,
                });
                Some(dst)
            }

            // ── Index ────────────────────────
            ExprKind::Index { object, index } => {
                // Array indexing lowers to FieldAddr with a runtime index.
                // But MIR FieldAddr uses compile-time indices.
                // For dynamic indexing we'd need a different instruction.
                // For now, we lower to a direct field access if the index is const,
                // otherwise this needs a heap load (arrays on heap) or
                // a special indexed-access instruction.
                //
                // Since the VM uses register-file arrays, dynamic indexing
                // can't be done with FieldAddr. We'd need to extend MIR.
                // For now, emit a placeholder — the index is lowered as a value
                // and we use FieldAddr with index 0 as a stub.
                let _base = self.lower_expr(*object);
                let _idx = self.lower_expr(*index);
                let dst = self.alloc_value();
                // TODO: proper dynamic index instruction
                Some(dst)
            }

            // ── Struct literal ────────────────
            ExprKind::StructLit { ty, fields } => {
                let mir_ty = self.mir_type(sema_ty);
                let dst = self.alloc_value();
                self.emit(mir::Inst::RegAlloc { dst, ty: mir_ty });

                for field_init in fields {
                    let field_idx = self.find_field_index(sema_ty, field_init.name);
                    let val = self.lower_expr(field_init.value);

                    if let Some(src) = val {
                        let field_dst = self.alloc_value();
                        self.emit(mir::Inst::FieldAddr {
                            dst: field_dst,
                            base: dst,
                            field: field_idx,
                            base_ty: mir_ty,
                        });
                        let field_ty_sema = self.field_sema_type(sema_ty, field_idx);
                        let field_ty = self.mir_type(field_ty_sema);
                        self.emit(mir::Inst::Copy {
                            dst: field_dst,
                            src,
                            ty: field_ty,
                        });
                    }
                }

                Some(dst)
            }

            // ── Unwrap ───────────────────────
            ExprKind::Unwrap(_inner) => {
                // TODO: emit runtime null check + trap
                let val = self.lower_expr(*_inner);
                val
            }

            // ── Cast ─────────────────────────
            ExprKind::Cast { expr: inner, ty: _ } => {
                let src = self.lower_expr(*inner)?;
                let from_ty = self.expr_types[inner];
                let dst = self.alloc_value();
                self.emit(mir::Inst::Cast {
                    dst,
                    src,
                    from: self.scalar_type(from_ty),
                    to: self.scalar_type(sema_ty),
                });
                Some(dst)
            }

            ExprKind::Const(inner) => {
                // Comptime expressions lower the same way at this stage.
                // The comptime evaluator would have already folded these.
                self.lower_expr(*inner)
            }

            // Things we haven't implemented yet
            ExprKind::ArrayLit(_)
            | ExprKind::ArrayRepeat { .. }
            | ExprKind::Match { .. }
            | ExprKind::For { .. }
            | ExprKind::OptionalField { .. }
            | ExprKind::Range { .. } => {
                // TODO: implement these
                None
            }
        }
    }

    /// Lower statements in a block, returning the block's result value.
    fn lower_block(&mut self, stmts: &[StmtId]) -> Option<mir::Value> {
        if stmts.is_empty() {
            return None;
        }

        let last = stmts.len() - 1;
        let mut result = None;

        for (i, &stmt_id) in stmts.iter().enumerate() {
            if self.is_terminated() {
                break;
            }

            let stmt = &self.ast.stmts[stmt_id];

            match &stmt.kind {
                StmtKind::Empty => {}

                StmtKind::Let { pattern, ty, value } => {
                    let val = self.lower_expr(*value);
                    // Bind the pattern's variables to the lowered value.
                    if let Some(v) = val {
                        self.bind_pattern(*pattern, v);
                    }
                }

                StmtKind::Semi(expr) => {
                    self.lower_expr(*expr);
                }

                StmtKind::Expr(expr) => {
                    let val = self.lower_expr(*expr);
                    if i == last {
                        result = val;
                    }
                }
            }
        }

        result
    }

    /// Resolve an expression to its MIR lvalue (the Value it's stored in).
    /// Returns None for non-assignable expressions.
    fn lower_lvalue(&mut self, expr_id: ExprId) -> Option<mir::Value> {
        let expr = &self.ast.exprs[expr_id];
        match &expr.kind {
            ExprKind::Path(path) => {
                // Look up variable in var_map via the path
                // For a simple single-name path, this is a local variable.
                let path = &self.ast.paths[*path];
                if path.is_simple() {
                    // We need the SymbolId — but we only have an Ident.
                    // In the current architecture, we'd need a scope to look this up.
                    // For now, return None; proper implementation needs scope threading.
                    None
                } else {
                    None
                }
            }
            ExprKind::Field { object, field } => {
                let base = self.lower_lvalue(*object)?;
                let object_ty = self.expr_types[object];
                let base_mir_ty = self.mir_type(object_ty);
                let field_idx = self.find_field_index(object_ty, *field);
                let dst = self.alloc_value();
                self.emit(mir::Inst::FieldAddr {
                    dst,
                    base,
                    field: field_idx,
                    base_ty: base_mir_ty,
                });
                Some(dst)
            }
            _ => None,
        }
    }

    /// Bind pattern variables to a MIR value.
    fn bind_pattern(&mut self, pattern_id: PatternId, value: mir::Value) {
        let pattern = &self.ast.patterns[pattern_id];
        match &pattern.kind {
            PatternKind::Identifier { .. } => {
                // We need the SymbolId that was created during type checking.
                // The proper way is to have sema record pattern → SymbolId mappings.
                // For now, this is a known gap.
            }
            PatternKind::Wildcard | PatternKind::Rest => {}
            _ => {} // TODO: destructuring patterns
        }
    }

    fn find_field_index(&self, sema_ty: SemaTypeId, field_name: Ident) -> usize {
        match self.types.get(sema_ty) {
            SemaType::Struct(info) => info
                .fields
                .iter()
                .position(|f| f.name == field_name)
                .unwrap_or_else(|| bug!("field not found in struct")),
            SemaType::Union(info) => info
                .fields
                .iter()
                .position(|f| f.name == field_name)
                .unwrap_or_else(|| bug!("field not found in union")),
            _ => bug!("field access on non-struct/union"),
        }
    }

    fn field_sema_type(&self, sema_ty: SemaTypeId, field_idx: usize) -> SemaTypeId {
        match self.types.get(sema_ty) {
            SemaType::Struct(info) => info.fields[field_idx].ty,
            SemaType::Union(info) => info.fields[field_idx].ty,
            _ => bug!("field_sema_type on non-struct/union"),
        }
    }
}

fn assign_op_to_bin_op(op: AssignOp) -> BinOp {
    match op {
        AssignOp::AddAssign => BinOp::Add,
        AssignOp::SubAssign => BinOp::Sub,
        AssignOp::MulAssign => BinOp::Mul,
        AssignOp::DivAssign => BinOp::Div,
        AssignOp::ModAssign => BinOp::Mod,
        AssignOp::BitAndAssign => BinOp::BitAnd,
        AssignOp::BitOrAssign => BinOp::BitOr,
        AssignOp::BitXorAssign => BinOp::BitXor,
        AssignOp::ShlAssign => BinOp::Shl,
        AssignOp::ShrAssign => BinOp::Shr,
        AssignOp::Assign => bug!("plain assign is not a compound op"),
    }
}
