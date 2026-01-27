use std::rc::Rc;

use ahash::AHashMap;
use simple_ternary::tnr;

use crate::{
    arena::{Ident, Interner},
    compiler::{
        ast::*,
        ast_op::{AssignOp, BinaryOp, UnaryOp},
        name_resolver::{ScopeId, SymbolId, SymbolKind, SymbolTable},
        sema::{
            sema_error::{SemaError, SemaResult},
            sema_type::{
                EnumInfo, FieldInfo, SemaType, SemaTypeId, StructInfo, TypeArena, UnionInfo,
                VariantInfo,
            },
            sema_value::{SemaValue, SemaValueId, ValueArena},
        },
        tokens::Span,
    },
};

pub mod sema_error;
pub mod sema_type;
pub mod sema_value;

struct BuiltinTypes {
    cint: SemaTypeId,
    cstr: SemaTypeId,
    int: SemaTypeId,
    uint: SemaTypeId,
    float: SemaTypeId,
    bool: SemaTypeId,
    char: SemaTypeId,
    str: SemaTypeId,
    null: SemaTypeId,
    void: SemaTypeId,
    never: SemaTypeId,
    int_range: SemaTypeId,
    uint_range: SemaTypeId,
}

enum GenericSub {
    Type(AstTypeId),
    Const(SemaValue),
}

type GenericKey = (DeclId, Rc<[GenericSub]>);

pub struct Sema<'a> {
    ast: &'a AstArena,
    symbols: &'a SymbolTable,
    interner: &'a Interner,

    // Type storage
    types: TypeArena,
    builtin_types: BuiltinTypes,

    // Value storage
    values: ValueArena,

    type_bindings: AHashMap<SymbolId, SemaTypeId>,
    value_bindings: AHashMap<SymbolId, SemaValueId>,

    expr_types: AHashMap<ExprId, SemaTypeId>,
    expr_values: AHashMap<ExprId, SemaValueId>,

    generic_type_cache: AHashMap<GenericKey, SemaTypeId>,
}

impl<'a> Sema<'a> {
    pub fn new(ast: &'a AstArena, symbols: &'a SymbolTable, interner: &'a Interner) -> Self {
        let mut types = TypeArena::new();
        let builtin_types = BuiltinTypes {
            cint: types.insert(SemaType::CInt),
            int: types.insert(SemaType::Int),
            uint: types.insert(SemaType::Uint),
            float: types.insert(SemaType::Float),
            bool: types.insert(SemaType::Bool),
            char: types.insert(SemaType::Char),
            cstr: types.insert(SemaType::CStr),
            str: types.insert(SemaType::Str),
            null: types.insert(SemaType::Null),
            void: types.insert(SemaType::Void),
            never: types.insert(SemaType::Never),
            int_range: types.insert(SemaType::IntRange),
            uint_range: types.insert(SemaType::UintRange),
        };

        Self {
            ast,
            symbols,
            interner,
            types,
            builtin_types,
            values: ValueArena::new(),
            type_bindings: AHashMap::new(),
            value_bindings: AHashMap::new(),
            expr_types: AHashMap::new(),
            expr_values: AHashMap::new(),
            generic_type_cache: AHashMap::new(),
        }
    }

    pub fn analyze_function(&mut self, decl_id: DeclId) -> SemaResult<()> {
        let decl = &self.ast.decls[decl_id];

        match &decl.kind {
            DeclKind::Function {
                generics,
                params,
                ret,
                body,
            } => {
                debug_assert!(generics.is_empty());

                let func_scope = ScopeId::Decl(decl_id);

                for param in params {
                    let expected_ty = self.eval_type(param.ty, func_scope)?;
                    self.bind_function_param(param.pattern, expected_ty, func_scope)?;
                }

                let ret_ty = match ret {
                    Some(ret) => self.eval_type(*ret, func_scope)?,
                    None => self.builtin_types.void,
                };

                let body_ty = self.check_expr(*body, func_scope)?;

                if !self.can_assign(ret_ty, body_ty) {
                    return Err(SemaError::TypeMismatch {
                        exp: ret_ty,
                        got: body_ty,
                        span: decl.span,
                    });
                }

                Ok(())
            }

            _ => panic!("Unexpected declaration kind"),
        }
    }

    fn lookup(&self, scope: ScopeId, name: Ident, span: Span) -> SemaResult<(SymbolId, ScopeId)> {
        self.symbols
            .lookup(name, scope)
            .ok_or(SemaError::UndefinedSymbol { name, span })
    }

    fn lookup_local(&self, scope: ScopeId, name: Ident, span: Span) -> SemaResult<SymbolId> {
        self.symbols
            .lookup_local(name, scope)
            .ok_or(SemaError::SymbolNotInScope { scope, name, span })
    }

    fn can_assign(&self, tgt: SemaTypeId, val: SemaTypeId) -> bool {
        if tgt == val {
            return true;
        }

        let tgt = self.types.get(tgt);
        let val = self.types.get(val);

        use SemaType::*;

        // Test nominal/structural equality
        match (tgt, val) {
            (_, Never) // Never can be assigned to any type
            | (Int, Int)
            | (Uint, Uint)
            | (Bool, Bool)
            | (Float, Float)
            | (Char, Char)
            | (CStr, CStr)
            | (Str, Str)
            | (Null, Null)
            | (Void, Void)
            | (IntRange, IntRange)
            | (UintRange, UintRange) => true,

            (Opt(tgt_inner), Opt(val_inner)) => self.can_assign(*tgt_inner, *val_inner),

            (
                Ptr {
                    mutable: tgt_mut,
                    pointee: tgt_ptr,
                },
                Ptr {
                    mutable: val_mut,
                    pointee: val_ptr,
                },
            ) => {
                (*tgt_mut == false || tgt_mut == val_mut)
                    && self.can_assign(*tgt_ptr, *val_ptr)
            }

            (Array{elem: tgt_elem, len: tgt_len}, Array{elem: val_elem, len: val_len}) => {
                tgt_len == val_len && self.can_assign(*tgt_elem, *val_elem)
            }

            (Slice(tgt_elem), Slice(val_elem)) => self.can_assign(*tgt_elem, *val_elem),

            (Tuple(tgts), Tuple(vals)) => {
                tgts.len() == vals.len() && tgts.iter().zip(vals.iter()).all(|(tgt_ty, val_ty)| self.can_assign(*tgt_ty, *val_ty))
            }
            (t, v) => todo!("impl assignable for {:?}", (t, v)),
        }
    }

    fn check_expr(&mut self, expr_id: ExprId, scope: ScopeId) -> SemaResult<SemaTypeId> {
        if let Some(&type_id) = self.expr_types.get(&expr_id) {
            return Ok(type_id);
        }

        let expr = &self.ast.exprs[expr_id];
        let span = expr.span;

        let type_id = match &expr.kind {
            ExprKind::IntLit(_) => self.builtin_types.int,
            ExprKind::FloatLit(_) => self.builtin_types.float,
            ExprKind::True => self.builtin_types.bool,
            ExprKind::False => self.builtin_types.bool,
            ExprKind::Char(_) => self.builtin_types.char,
            ExprKind::StrLit(_) => self.builtin_types.str,
            ExprKind::Null => self.builtin_types.null,
            ExprKind::Void => self.builtin_types.never,

            ExprKind::Path(path) => self.check_path_expr(*path, scope, span)?,

            ExprKind::ArrayLit(exprs) => todo!("array literal"),
            ExprKind::ArrayRepeat { value, count } => {
                let elem = self.check_expr(*value, scope)?;
                let len = self.eval_expr(*count, scope)?;
                match self.values.get(len) {
                    SemaValue::Uint(n) => self.types.insert(SemaType::Array { elem, len: *n }),
                    _ => {
                        return Err(SemaError::TypeMismatch {
                            exp: self.builtin_types.uint,
                            got: self.check_expr(*count, scope)?,
                            span,
                        });
                    }
                }
            }

            ExprKind::StructLit { ty, fields } => {
                let struct_ty = self.eval_type(*ty, scope)?;
                todo!()
            }

            ExprKind::Group(inner) => self.check_expr(*inner, scope)?,
            ExprKind::Unary { op, expr } => {
                let type_id = self.check_expr(*expr, scope)?;
                let ty = self.types.get(type_id);

                match (op, ty) {
                    (UnaryOp::Not, SemaType::Bool) => type_id,
                    (UnaryOp::Neg | UnaryOp::BitNot, SemaType::Int | SemaType::Uint) => type_id,
                    (UnaryOp::Deref, SemaType::Ptr { pointee, .. }) => *pointee,
                    _ => {
                        return Err(SemaError::InvalidUnaryOp {
                            op: *op,
                            ty: type_id,
                            span,
                        });
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => todo!(),
            ExprKind::Assign { op, tgt, val } => todo!(),
            ExprKind::Cast { expr, ty } => todo!(),
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_type_id = self.check_expr(*cond, scope)?;
                if !matches!(self.types.get(cond_type_id), SemaType::Bool) {
                    return Err(SemaError::TypeMismatch {
                        exp: self.builtin_types.bool,
                        got: cond_type_id,
                        span,
                    });
                }

                let then_type_id = self.check_expr(*then_branch, scope)?;
                let else_type_id = match else_branch {
                    Some(else_branch) => self.check_expr(*else_branch, scope)?,
                    None => self.builtin_types.void,
                };

                todo!("type unification for then and else")
            }
            ExprKind::Match { scrutinee, arms } => todo!(),
            ExprKind::While { cond, body } => {
                let cond_type_id = self.check_expr(*cond, scope)?;
                if !matches!(self.types.get(cond_type_id), SemaType::Bool) {
                    return Err(SemaError::TypeMismatch {
                        exp: self.builtin_types.bool,
                        got: cond_type_id,
                        span,
                    });
                }

                let body_type_id = self.check_expr(*body, scope)?;

                if !matches!(self.types.get(body_type_id), SemaType::Void) {
                    return Err(SemaError::TypeMismatch {
                        exp: self.builtin_types.void,
                        got: body_type_id,
                        span,
                    });
                }

                self.builtin_types.void
            }
            ExprKind::Loop(body) => {
                let body_type_id = self.check_expr(*body, scope)?;
                if !matches!(self.types.get(body_type_id), SemaType::Void) {
                    return Err(SemaError::TypeMismatch {
                        exp: self.builtin_types.void,
                        got: body_type_id,
                        span,
                    });
                }

                self.builtin_types.void
            }
            ExprKind::For {
                pattern,
                iter,
                body,
            } => todo!(),
            ExprKind::Block(stmts) => {
                let scope = ScopeId::Expr(expr_id);
                self.check_block(stmts, scope)?
            }
            ExprKind::Return(value) => {
                if let Some(value) = value {
                    self.check_expr(*value, scope)?;
                }
                self.builtin_types.never
            }
            ExprKind::Break(value) => {
                if let Some(value) = value {
                    self.check_expr(*value, scope)?;
                }
                self.builtin_types.never
            }
            ExprKind::Continue => self.builtin_types.never,
            ExprKind::Call { callee, args } => todo!(),
            ExprKind::Field { object, field } => {
                let obj_ty = self.check_expr(*object, scope)?;
                self.check_field_access(obj_ty, *field, span)?
            }
            ExprKind::OptionalField { object, field } => todo!(),
            ExprKind::Index { object, index } => todo!(),
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => {
                todo!()
            }
            ExprKind::Unwrap(opt) => {
                let opt_ty_id = self.check_expr(*opt, scope)?;
                match self.types.get(opt_ty_id) {
                    SemaType::Opt(inner) => *inner,
                    _ => {
                        return Err(SemaError::TypeKindMismatch {
                            exp: "optional",
                            got: opt_ty_id,
                            span,
                        });
                    }
                }
            }
            ExprKind::Const(const_expr) => self.check_expr(*const_expr, scope)?,
        };

        self.expr_types.insert(expr_id, type_id);
        Ok(type_id)
    }

    fn resolve_path(
        &mut self,
        path_id: PathId,
        scope: ScopeId,
        span: Span,
    ) -> SemaResult<SymbolId> {
        let path = &self.ast.paths[path_id];

        let first = &path.first;
        let (mut curr, _) = self.lookup(scope, first.name, first.span)?;

        if path.is_simple() {
            return Ok(curr);
        }

        for seg in &path.rest {
            match &self.symbols.get(curr).kind {
                SymbolKind::Module(decl) | SymbolKind::Enum(decl) => {
                    curr = self.lookup_local(ScopeId::Decl(*decl), seg.name, seg.span)?;
                }
                _ => return Err(SemaError::NotDeclScope { span }),
            }
        }

        // TODO: handle generics
        Ok(curr)
    }

    fn eval_decl_ty(
        &mut self,
        sym_id: SymbolId,
        decl_id: DeclId,
        scope: ScopeId,
    ) -> SemaResult<SemaTypeId> {
        let decl = &self.ast.decls[decl_id];

        match &decl.kind {
            DeclKind::Module(_) => Err(todo!("handle module in type position")),
            DeclKind::Function {
                generics,
                params,
                ret,
                body,
            } => {
                debug_assert!(generics.is_empty());
                todo!()
            }
            DeclKind::Const { ty, value } => {
                let val_ty = self.check_expr(*value, scope)?;

                if let Some(ty) = ty {
                    let ty_id = self.eval_type(*ty, scope)?;
                    todo!("unify value and type")
                }

                self.type_bindings.insert(sym_id, val_ty);
                Ok(val_ty)
            }
            DeclKind::Struct { generics, fields } => {
                debug_assert!(generics.is_empty());

                let struct_ty = self.types.insert(SemaType::Resolving(decl_id));
                self.type_bindings.insert(sym_id, struct_ty);

                let fields = self.make_fields(fields, scope)?;
                *self.types.get_mut(struct_ty) = SemaType::Struct(StructInfo {
                    name: Some(decl.name),
                    fields,
                });

                Ok(struct_ty)
            }
            DeclKind::Union { generics, fields } => {
                debug_assert!(generics.is_empty());

                let union_ty = self.types.insert(SemaType::Resolving(decl_id));
                self.type_bindings.insert(sym_id, union_ty);

                let fields = self.make_fields(fields, scope)?;
                *self.types.get_mut(union_ty) = SemaType::Union(UnionInfo {
                    name: Some(decl.name),
                    fields,
                });

                Ok(union_ty)
            }
            DeclKind::Enum { base, variants } => {
                let enum_ty = self.types.insert(SemaType::Resolving(decl_id));
                self.type_bindings.insert(sym_id, enum_ty);

                let base = match base {
                    None => self.builtin_types.uint,
                    Some(base) => {
                        let base = self.eval_type(*base, scope)?;
                        self.validate_enum_base(base, decl.span)?
                    }
                };

                let variants = self.make_variants(base, variants, scope)?;
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
    fn make_fields(&mut self, fields: &[Field], scope: ScopeId) -> SemaResult<Vec<FieldInfo>> {
        let mut field_infos = Vec::with_capacity(fields.len());
        for field in fields {
            let name = field.name;
            let ty = self.eval_type(field.ty, scope)?;
            field_infos.push(FieldInfo { name, ty });
        }

        Ok(field_infos.into())
    }

    fn make_variants(
        &mut self,
        base_ty: SemaTypeId,
        variants: &[Variant],
        scope: ScopeId,
    ) -> SemaResult<Vec<VariantInfo>> {
        let mut variant_infos = Vec::with_capacity(variants.len());
        for variant in variants {
            let name = variant.name;
            let value = match variant.value {
                Some(expr_id) => self.eval_expr(expr_id, scope)?,
                None => self.values.insert(SemaValue::Void),
            };

            variant_infos.push(VariantInfo { name, value });
        }
        todo!("check duplicates and base type");
        Ok(variant_infos)
    }

    fn validate_enum_base(&self, base_id: SemaTypeId, span: Span) -> SemaResult<SemaTypeId> {
        match self.types.get(base_id) {
            SemaType::Uint => Ok(self.builtin_types.uint),
            SemaType::Int => Ok(self.builtin_types.int),
            _ => Err(SemaError::InvalidEnumBase { base_id, span }),
        }
    }

    fn check_path_expr(
        &mut self,
        path: PathId,
        scope: ScopeId,
        span: Span,
    ) -> SemaResult<SemaTypeId> {
        let sym_id = self.resolve_path(path, scope, span)?;
        let symbol = self.symbols.get(sym_id);

        match symbol.kind {
            SymbolKind::Function(decl) | SymbolKind::Const(decl) => {
                match self.type_bindings.get(&sym_id) {
                    Some(ty) => Ok(*ty),
                    None => self.eval_decl_ty(sym_id, decl, scope),
                }
            }

            // Variable must have been initialized before usage
            SymbolKind::Variable(_) => match self.type_bindings.get(&sym_id) {
                Some(ty) => Ok(*ty),
                None => Err(SemaError::UninitVariable {
                    name: symbol.name,
                    span,
                }),
            },

            SymbolKind::Variant(enum_sym) => match self.type_bindings.get(&enum_sym) {
                Some(ty) => Ok(*ty),
                None => match self.symbols.get(enum_sym).kind {
                    SymbolKind::Enum(decl) => self.eval_decl_ty(enum_sym, decl, scope),
                    _ => Err(todo!("handle wrong scope")),
                },
            },

            _ => Err(SemaError::ExpectedValue {
                found: sym_id,
                span,
            }),
        }
    }

    fn check_block(&mut self, stmts: &[StmtId], scope: ScopeId) -> SemaResult<SemaTypeId> {
        let mut type_id = self.builtin_types.void;

        for stmt_id in stmts {
            type_id = self.check_stmt(*stmt_id, scope)?;
        }

        Ok(type_id)
    }

    fn check_stmt(&mut self, stmt_id: StmtId, scope: ScopeId) -> SemaResult<SemaTypeId> {
        let stmt = &self.ast.stmts[stmt_id];

        match &stmt.kind {
            StmtKind::Let { pattern, ty, value } => {
                todo!("check let")
            }
            StmtKind::Expr { expr, has_semi } => {
                let expr_ty = self.check_expr(*expr, scope)?;
                Ok(tnr! {*has_semi => self.builtin_types.void : expr_ty})
            }
            StmtKind::Empty => Ok(self.builtin_types.void),
        }
    }

    fn bind_function_param(
        &mut self,
        pattern_id: PatternId,
        expected_ty: SemaTypeId,
        scope: ScopeId,
    ) -> SemaResult<()> {
        let pattern = &self.ast.patterns[pattern_id];

        match &pattern.kind {
            PatternKind::Int(_)
            | PatternKind::Float(_)
            | PatternKind::Bool(_)
            | PatternKind::Char(_)
            | PatternKind::CStr(_)
            | PatternKind::Path(_)
            | PatternKind::Range { .. } => Err(SemaError::RefutableParamPattern {
                pattern_id,
                expected_ty,
                span: pattern.span,
            }),

            PatternKind::Wildcard => Ok(()),

            PatternKind::Identifier { name, .. } => {
                let Some((sym_id, _)) = self.symbols.lookup(*name, scope) else {
                    return Err(SemaError::UndefinedSymbol {
                        name: *name,
                        span: pattern.span,
                    });
                };

                self.type_bindings.insert(sym_id, expected_ty);
                Ok(())
            }

            PatternKind::Rest => todo!(),

            PatternKind::Struct { ty, fields, rest } => todo!(),
            PatternKind::Tuple(pattern_ids) => todo!(),
            PatternKind::Array(pattern_ids) => todo!(),
            PatternKind::Or(pattern_ids) => todo!(),
        }
    }

    fn check_field_access(
        &mut self,
        obj_ty: SemaTypeId,
        name: Ident,
        span: Span,
    ) -> SemaResult<SemaTypeId> {
        // Struct field access returns the field of the type.
        // Union field access returns optional field type.
        match self.types.get(obj_ty) {
            SemaType::Struct(info) => match info.fields.iter().find(|f| f.name == name) {
                Some(field) => Ok(field.ty),
                None => Err(SemaError::FieldNotFound {
                    ty: obj_ty,
                    name,
                    span,
                }),
            },
            SemaType::Union(info) => match info.fields.iter().find(|f| f.name == name) {
                Some(field) => Ok(self.types.insert(SemaType::Opt(field.ty))),
                None => Err(SemaError::FieldNotFound {
                    ty: obj_ty,
                    name,
                    span,
                }),
            },
            _ => Err(SemaError::FieldNotFound {
                ty: obj_ty,
                name,
                span,
            }),
        }
    }

    fn eval_type(&mut self, ast_ty: AstTypeId, scope: ScopeId) -> SemaResult<SemaTypeId> {
        let ast_ty = &self.ast.types[ast_ty];

        match &ast_ty.kind {
            AstTypeKind::CInt => Ok(self.builtin_types.cint),
            AstTypeKind::CStr => Ok(self.builtin_types.cstr),
            AstTypeKind::Bool => Ok(self.builtin_types.bool),
            AstTypeKind::Int => Ok(self.builtin_types.int),
            AstTypeKind::Uint => Ok(self.builtin_types.uint),
            AstTypeKind::Float => Ok(self.builtin_types.float),
            AstTypeKind::Char => Ok(self.builtin_types.char),

            AstTypeKind::Never => Ok(self.builtin_types.never),
            AstTypeKind::Str => Ok(self.builtin_types.str),
            AstTypeKind::Void => Ok(self.builtin_types.void),
            AstTypeKind::Inferred => todo!("handle inferred type"),
            AstTypeKind::Path(path) => self.eval_type_path(*path, scope, ast_ty.span),
            AstTypeKind::Tuple(ast_types) => {
                let mut types = Vec::with_capacity(ast_types.len());
                for &ast_ty in ast_types {
                    types.push(self.eval_type(ast_ty, scope)?);
                }
                Ok(self.types.insert(SemaType::Tuple(types.into())))
            }
            AstTypeKind::Array { elem, len } => {
                let elem = self.eval_type(*elem, scope)?;
                let len = self.eval_expr(*len, scope)?;

                match self.values.get(len) {
                    SemaValue::Uint(n) => Ok(self.types.insert(SemaType::Array { elem, len: *n })),
                    _ => Err(SemaError::InvalidArrayLength { span: ast_ty.span }),
                }
            }
            AstTypeKind::Slice(elem) => {
                let elem = self.eval_type(*elem, scope)?;
                Ok(self.types.insert(SemaType::Slice(elem)))
            }
            AstTypeKind::Optional(inner) => {
                let inner = self.eval_type(*inner, scope)?;
                Ok(self.types.insert(SemaType::Opt(inner)))
            }
            AstTypeKind::Pointer { mutable, pointee } => {
                let pointee = self.eval_type(*pointee, scope)?;
                Ok(self.types.insert(SemaType::Ptr {
                    mutable: *mutable,
                    pointee,
                }))
            }
            AstTypeKind::Function { params, ret } => {
                let mut param_tys = Vec::with_capacity(params.len());
                for &param in params {
                    param_tys.push(self.eval_type(param, scope)?);
                }

                let ret = match ret {
                    Some(r) => self.eval_type(*r, scope)?,
                    None => self.builtin_types.void,
                };

                Ok(self.types.insert(SemaType::Function {
                    params: param_tys.into(),
                    ret,
                }))
            }
        }
    }

    fn eval_type_path(
        &mut self,
        path: PathId,
        scope: ScopeId,
        span: Span,
    ) -> SemaResult<SemaTypeId> {
        let sym_id = self.resolve_path(path, scope, span)?;
        let symbol = self.symbols.get(sym_id);

        match symbol.kind {
            SymbolKind::Struct(decl) | SymbolKind::Union(decl) | SymbolKind::Enum(decl) => {
                match self.type_bindings.get(&sym_id) {
                    Some(ty) => Ok(*ty),
                    None => self.eval_decl_ty(sym_id, decl, scope),
                }
            }
            _ => Err(SemaError::ExpectedType {
                found: sym_id,
                span,
            }),
        }
    }

    fn eval_expr(&mut self, expr_id: ExprId, scope: ScopeId) -> SemaResult<SemaValueId> {
        let expr = &self.ast.exprs[expr_id];
        let span = expr.span;

        let val = match &expr.kind {
            ExprKind::IntLit(n) => self.values.insert(SemaValue::Uint(*n)),
            ExprKind::FloatLit(f) => todo!("float literal"),
            ExprKind::True => self.values.insert(SemaValue::Bool(true)),
            ExprKind::False => self.values.insert(SemaValue::Bool(false)),
            ExprKind::Char(c) => self.values.insert(SemaValue::Char(*c)),
            ExprKind::StrLit(s) => self.values.insert(SemaValue::Str(*s)),
            ExprKind::Null => self.values.insert(SemaValue::Null),
            ExprKind::Void => self.values.insert(SemaValue::Void),
            ExprKind::Path(path) => self.eval_path_expr(*path, scope, span)?,
            ExprKind::ArrayLit(elems) => {
                let mut arr = Vec::with_capacity(elems.len());
                for &elem in elems {
                    arr.push(self.eval_expr(elem, scope)?);
                }
                self.values.insert(SemaValue::Array(arr.into()))
            }
            ExprKind::ArrayRepeat { value, count } => {
                let value = self.eval_expr(*value, scope)?;
                let count = self.eval_expr(*count, scope)?;
                match self.values.get(count) {
                    SemaValue::Uint(n) => self
                        .values
                        .insert(SemaValue::Array(vec![value; *n as usize].into())),
                    _ => return Err(SemaError::InvalidArrayLength { span }),
                }
            }
            ExprKind::StructLit { ty, fields } => {
                let ty = self.eval_type(*ty, scope)?;

                let mut values = AHashMap::new();
                for field in fields {
                    values.insert(field.name, self.eval_expr(field.value, scope)?);
                }

                self.values.insert(SemaValue::Struct(ty, values))
            }
            ExprKind::Group(expr) => self.eval_expr(*expr, scope)?,
            ExprKind::Unary { op, expr } => {
                let value = self.eval_expr(*expr, scope)?;
                self.eval_unary_expr(*op, value, span)?
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lhs = self.eval_expr(*lhs, scope)?;
                let rhs = self.eval_expr(*rhs, scope)?;
                self.eval_binary_expr(*op, lhs, rhs, span)?
            }
            ExprKind::Assign { op, tgt, val } => todo!("eval assignment"),
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

        self.expr_values.insert(expr_id, val);
        Ok(val)
    }

    fn eval_path_expr(
        &mut self,
        path: PathId,
        scope: ScopeId,
        span: Span,
    ) -> SemaResult<SemaValueId> {
        let sym_id = self.resolve_path(path, scope, span)?;
        let symbol = self.symbols.get(sym_id);

        match symbol.kind {
            SymbolKind::Function(decl) | SymbolKind::Const(decl) => {
                match self.value_bindings.get(&sym_id) {
                    Some(val) => Ok(*val),
                    None => self.eval_decl_expr(sym_id, decl, scope),
                }
            }

            // Variable must have been initialized before usage
            SymbolKind::Variable(_) => match self.value_bindings.get(&sym_id) {
                Some(val) => Ok(*val),
                None => Err(SemaError::UninitVariable {
                    name: symbol.name,
                    span,
                }),
            },

            SymbolKind::Variant(symbol_id) => {
                todo!("eval enum variant")
            }

            _ => Err(SemaError::ExpectedValue {
                found: sym_id,
                span,
            }),
        }
    }

    fn eval_decl_expr(
        &mut self,
        sym_id: SymbolId,
        decl_id: DeclId,
        scope: ScopeId,
    ) -> SemaResult<SemaValueId> {
        let decl = &self.ast.decls[decl_id];

        match &decl.kind {
            DeclKind::Function {
                generics,
                params,
                ret,
                body,
            } => {
                let val = self.values.insert(SemaValue::Function(decl_id));
                self.value_bindings.insert(sym_id, val);
                Ok(val)
            }
            DeclKind::Const { ty, value } => {
                let val = self.eval_expr(*value, scope)?;

                if let Some(ty) = ty {
                    let ty_id = self.eval_type(*ty, scope)?;
                    //todo!("check value and type")
                }

                self.value_bindings.insert(sym_id, val);
                Ok(val)
            }
            _ => Err(SemaError::ExpectedValue {
                found: sym_id,
                span: decl.span,
            }),
        }
    }

    fn eval_unary_expr(
        &mut self,
        op: UnaryOp,
        value: SemaValueId,
        span: Span,
    ) -> SemaResult<SemaValueId> {
        let value = self.values.get(value);

        let res = match (op, value) {
            (UnaryOp::Neg, SemaValue::Int(v)) => SemaValue::Int(-v),
            (UnaryOp::Not, SemaValue::Bool(v)) => SemaValue::Bool(!v),
            (UnaryOp::BitNot, SemaValue::Int(v)) => SemaValue::Int(!v),
            (UnaryOp::BitNot, SemaValue::Uint(v)) => SemaValue::Uint(!v),
            _ => return Err(SemaError::InvalidConstOp { span }),
        };

        Ok(self.values.insert(res))
    }

    fn eval_binary_expr(
        &mut self,
        op: BinaryOp,
        lhs: SemaValueId,
        rhs: SemaValueId,
        span: Span,
    ) -> SemaResult<SemaValueId> {
        let lhs = self.values.get(lhs);
        let rhs = self.values.get(rhs);

        let val = match (op, lhs, rhs) {
            (BinaryOp::Add, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Int(a + b),
            (BinaryOp::Sub, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Int(a - b),
            (BinaryOp::Mul, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Int(a * b),
            (BinaryOp::Div, SemaValue::Int(a), SemaValue::Int(b)) => {
                if *b == 0 {
                    return Err(SemaError::DivisionByZero { span });
                } else {
                    SemaValue::Int(a / b)
                }
            }
            (BinaryOp::Mod, SemaValue::Int(a), SemaValue::Int(b)) => {
                if *b == 0 {
                    return Err(SemaError::DivisionByZero { span });
                } else {
                    SemaValue::Int(a % b)
                }
            }

            (BinaryOp::Add, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Uint(a + b),
            (BinaryOp::Sub, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Uint(a - b),
            (BinaryOp::Mul, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Uint(a * b),
            (BinaryOp::Div, SemaValue::Uint(a), SemaValue::Uint(b)) => {
                if *b == 0 {
                    return Err(SemaError::DivisionByZero { span });
                } else {
                    SemaValue::Uint(a / b)
                }
            }
            (BinaryOp::Mod, SemaValue::Uint(a), SemaValue::Uint(b)) => {
                if *b == 0 {
                    return Err(SemaError::DivisionByZero { span });
                } else {
                    SemaValue::Uint(a % b)
                }
            }

            (BinaryOp::Eq, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Bool(a == b),
            (BinaryOp::Ne, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Bool(a != b),
            (BinaryOp::Lt, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Bool(a < b),
            (BinaryOp::Le, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Bool(a <= b),
            (BinaryOp::Gt, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Bool(a > b),
            (BinaryOp::Ge, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Bool(a >= b),

            (BinaryOp::Eq, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Bool(a == b),
            (BinaryOp::Ne, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Bool(a != b),
            (BinaryOp::Lt, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Bool(a < b),
            (BinaryOp::Le, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Bool(a <= b),
            (BinaryOp::Gt, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Bool(a > b),
            (BinaryOp::Ge, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Bool(a >= b),

            (BinaryOp::Eq, SemaValue::Bool(a), SemaValue::Bool(b)) => SemaValue::Bool(a == b),
            (BinaryOp::Ne, SemaValue::Bool(a), SemaValue::Bool(b)) => SemaValue::Bool(a != b),
            (BinaryOp::And, SemaValue::Bool(a), SemaValue::Bool(b)) => SemaValue::Bool(*a && *b),
            (BinaryOp::Or, SemaValue::Bool(a), SemaValue::Bool(b)) => SemaValue::Bool(*a || *b),
            (BinaryOp::BitXor, SemaValue::Bool(a), SemaValue::Bool(b)) => SemaValue::Bool(*a ^ *b),

            (BinaryOp::BitAnd, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Int(a & b),
            (BinaryOp::BitOr, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Int(a | b),
            (BinaryOp::BitXor, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Int(a ^ b),
            (BinaryOp::Shl, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Int(a << b),
            (BinaryOp::Shr, SemaValue::Int(a), SemaValue::Int(b)) => SemaValue::Int(a >> b),

            (BinaryOp::BitAnd, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Uint(a & b),
            (BinaryOp::BitOr, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Uint(a | b),
            (BinaryOp::BitXor, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Uint(a ^ b),
            (BinaryOp::Shl, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Uint(a << b),
            (BinaryOp::Shr, SemaValue::Uint(a), SemaValue::Uint(b)) => SemaValue::Uint(a >> b),

            _ => return Err(SemaError::InvalidConstOp { span }),
        };

        Ok(self.values.insert(val))
    }

    fn eval_assign_expr(
        &mut self,
        op: AssignOp,
        name: Ident,
        val: SemaValueId,
        scope: ScopeId,
        span: Span,
    ) -> SemaResult<SemaValue> {
        let Some((sym_id, _)) = self.symbols.lookup(name, scope) else {
            return Err(SemaError::UndefinedSymbol { name, span });
        };

        match op {
            AssignOp::Assign => match &self.symbols.get(sym_id).kind {
                SymbolKind::Variable(mutable) if *mutable => {
                    self.value_bindings.insert(sym_id, val);
                    Ok(SemaValue::Void)
                }
                _ => Err(SemaError::InvalidAssignment { name, span }),
            },
            _ => Err(SemaError::InvalidConstOp { span }),
        }
    }
}
