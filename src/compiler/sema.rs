use std::rc::Rc;

use ahash::AHashMap;

use crate::{
    arena::{Ident, Interner, StrSymbol},
    compiler::{
        ast::*,
        ast_op::{AssignOp, BinaryOp, UnaryOp},
        name_resolver::{ScopeId, SymbolTable},
        sema_error::SemaError,
        tokens::Span,
        type_info::{
            EnumInfo, FieldInfo, ModuleInfo, StructInfo, Type, TypeArena, TypeId, UnionInfo,
            VariantInfo,
        },
    },
};

type SemaResult<T> = Result<T, SemaError>;

type CallKey = (DeclId, Rc<[ConstValue]>);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConstValue {
    Function(DeclId),
    Type(TypeId),
    Int(i64),
    Uint(u64),
    Bool(bool),
    Char(char),
    Str(StrSymbol),
    Null,
    Void,
    Never,
    Array(Rc<[ConstValue]>),
    Struct(Rc<[ConstValue]>),
    Tuple(Rc<[ConstValue]>),
}

enum Binding {
    Local { mutable: bool, value: ConstValue },
    Pending(DeclId),
    Resolving,
    Resolved(ConstValue),
}

enum ScopeKind {
    Module,
    Function,
    Block,
}

struct Scope {
    bindings: AHashMap<Ident, Binding>,
    kind: ScopeKind,
}

pub struct Sema<'a> {
    ast: &'a AstArena,
    symbols: &'a SymbolTable,
    interner: &'a Interner,

    scopes: Vec<Scope>,

    // Type storage
    types: TypeArena,

    /// Memoization cache: fn(args) -> ConstValue
    memo_cache: AHashMap<CallKey, ConstValue>,
}

impl<'a> Sema<'a> {
    pub fn new(ast: &'a AstArena, symbols: &'a SymbolTable, interner: &'a Interner) -> Self {
        Self {
            ast,
            symbols,
            interner,
            scopes: Vec::new(),
            types: TypeArena::new(),
            memo_cache: AHashMap::new(),
        }
    }

    fn lookup(&self, name: Ident, span: Span) -> SemaResult<&Binding> {
        for scope in self.scopes.iter().rev() {
            if let Some(binding) = scope.bindings.get(&name) {
                return Ok(binding);
            }
        }

        Err(SemaError::Undefined { name, span })
    }

    fn lookup_mut(&mut self, name: Ident, span: Span) -> SemaResult<&mut Binding> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(binding) = scope.bindings.get_mut(&name) {
                return Ok(binding);
            }
        }

        Err(SemaError::Undefined { name, span })
    }

    fn eval_ident(&mut self, name: Ident, scope: ScopeId, span: Span) -> SemaResult<ConstValue> {
        let binding = self.lookup_mut(name, span)?;

        match binding {
            Binding::Local { value, .. } => Ok(value.clone()),
            Binding::Resolved(value) => Ok(value.clone()),
            Binding::Resolving => Err(SemaError::CycleDetected { name, span }),
            Binding::Pending(decl_id) => {
                let decl_id = *decl_id;

                // Mark as resolving for cycle detection
                *binding = Binding::Resolving;

                // Evaluate value
                let value = self.eval_decl(decl_id, scope)?;

                // Cache result
                *self.lookup_mut(name, span)? = Binding::Resolved(value.clone());
                Ok(value)
            }
        }
    }

    fn eval_expr(&mut self, expr_id: ExprId, scope: ScopeId) -> SemaResult<ConstValue> {
        let expr = &self.ast.exprs[expr_id];
        let span = expr.span;

        let val = match &expr.kind {
            ExprKind::IntLit(n) => ConstValue::Uint(*n),
            ExprKind::FloatLit(f) => todo!("float literal"),
            ExprKind::True => ConstValue::Bool(true),
            ExprKind::False => ConstValue::Bool(false),
            ExprKind::Char(c) => ConstValue::Char(*c),
            ExprKind::StrLit(s) => ConstValue::Str(*s),
            ExprKind::Null => ConstValue::Null,
            ExprKind::Void => ConstValue::Void,
            ExprKind::Ident(name) => self.eval_ident(*name, scope, span)?,
            ExprKind::ArrayLit(elems) => {
                let mut arr = Vec::with_capacity(elems.len());
                for &elem in elems {
                    arr.push(self.eval_expr(elem, scope)?);
                }
                ConstValue::Array(arr.into())
            }
            ExprKind::ArrayRepeat { value, count } => {
                let value = self.eval_expr(*value, scope)?;
                let count = self.eval_expr(*count, scope)?;
                match count {
                    ConstValue::Uint(n) => ConstValue::Array(vec![value; n as usize].into()),
                    _ => return Err(SemaError::InvalidArrayLength { span }),
                }
            }
            ExprKind::StructLit { ty, fields } => {
                self.eval_expr(*ty, scope)?;

                let mut values = Vec::with_capacity(fields.len());
                for field in fields {
                    values.push(self.eval_expr(field.value, scope)?);
                }

                ConstValue::Struct(values.into())
            }
            ExprKind::TupleLit(elems) => {
                let mut tuple = Vec::with_capacity(elems.len());
                for &elem in elems {
                    tuple.push(self.eval_expr(elem, scope)?);
                }
                ConstValue::Array(tuple.into())
            }
            ExprKind::ScopeAccess { ty, item } => todo!("eval scope accces"),
            ExprKind::Group(expr) => self.eval_expr(*expr, scope)?,
            ExprKind::Unary { op, expr } => {
                let value = self.eval_expr(*expr, scope)?;
                self.eval_unary_expr(*op, &value, span)?
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

            ExprKind::ModuleType(decls) => self.eval_module(decls)?,
            ExprKind::StructType(fields) => self.eval_struct(fields, scope)?,
            ExprKind::UnionType(fields) => self.eval_union(fields, scope)?,
            ExprKind::EnumType(variants) => self.eval_enum(variants, scope)?,
            ExprKind::ArrayType { elem, size } => todo!(),
            ExprKind::SliceType(expr_id) => todo!(),
            ExprKind::PointerType { mutable, pointee } => todo!(),
            ExprKind::OptionType(expr_id) => todo!(),
            ExprKind::FunctionType { params, ret } => todo!(),

            ExprKind::WildcardType => todo!(),
        };

        Ok(val)
    }

    pub fn eval_module(&mut self, decls: &[DeclId]) -> SemaResult<ConstValue> {
        let mut mod_decls = AHashMap::new();
        for &decl_id in decls {
            mod_decls.insert(self.ast.decls[decl_id].name, decl_id);
        }

        let ty = self.types.insert(Type::Module(ModuleInfo {
            name: None,
            decls: mod_decls,
        }));

        Ok(ConstValue::Type(ty))
    }

    pub fn eval_struct(&mut self, fields: &[AstField], scope: ScopeId) -> SemaResult<ConstValue> {
        let fields = self.make_fields(fields, scope)?;
        let name = None;
        let ty = self.types.insert(Type::Struct(StructInfo { name, fields }));
        Ok(ConstValue::Type(ty))
    }

    pub fn eval_union(&mut self, fields: &[AstField], scope: ScopeId) -> SemaResult<ConstValue> {
        let fields = self.make_fields(fields, scope)?;
        let name = None;
        let ty = self.types.insert(Type::Union(UnionInfo { name, fields }));
        Ok(ConstValue::Type(ty))
    }

    pub fn eval_enum(&mut self, variants: &[AstVariant], scope: ScopeId) -> SemaResult<ConstValue> {
        let variants = self.make_variants(variants, scope)?;
        let name = None;
        let ty = self.types.insert(Type::Enum(EnumInfo { name, variants }));
        Ok(ConstValue::Type(ty))
    }

    fn eval_decl(&mut self, decl_id: DeclId, scope: ScopeId) -> SemaResult<ConstValue> {
        let decl = &self.ast.decls[decl_id];

        match &decl.kind {
            DeclKind::Function { params, ret, body } => todo!(),
            DeclKind::Const { ty, value } => {
                if let Some(ty_expr) = ty {
                    self.eval_expr(*ty_expr, scope)?;
                }

                let result = self.eval_expr(*value, scope)?;
                Ok(result)
            }
        }
    }

    pub fn eval_scope_access(
        &mut self,
        ty_expr: ExprId,
        name: Ident,
        scope: ScopeId,
        span: Span,
    ) -> SemaResult<ConstValue> {
        if let ConstValue::Type(ty_id) = self.eval_expr(ty_expr, scope)? {
            match self.types.get(ty_id) {
                Type::Module(info) => match info.decls.get(&name) {
                    Some(decl_id) => self.eval_decl(*decl_id, ScopeId::Expr(ty_expr)),
                    None => Err(SemaError::ItemNotFound {
                        module: ty_id,
                        item_name: name,
                        span,
                    }),
                },
                Type::Enum(info) => todo!("enum value"),
                _ => Err(SemaError::NotDeclScope { span }),
            }
        } else {
            Err(todo!("symbol not found in scope"))
        }
    }

    fn eval_call(
        &mut self,
        callee: DeclId,
        args: &Rc<[ConstValue]>,
        scope: ScopeId,
    ) -> SemaResult<ConstValue> {
        let key = (callee, args.clone());

        if let Some(val) = self.memo_cache.get(&key) {
            return Ok(val.clone());
        }

        let result = self.eval_function_body(callee, args, scope)?;

        self.memo_cache.insert(key, result.clone());
        Ok(result)
    }

    fn eval_function_body(
        &mut self,
        callee: DeclId,
        args: &Rc<[ConstValue]>,
        scope: ScopeId,
    ) -> SemaResult<ConstValue> {
        todo!()
    }

    #[inline(always)]
    fn make_fields(&mut self, fields: &[AstField], scope: ScopeId) -> SemaResult<Rc<[FieldInfo]>> {
        let mut field_infos = vec![];
        for field in fields {
            let name = field.name;
            let ty = self.eval_expr(field.ty, scope)?;
            field_infos.push(FieldInfo { name, ty });
        }

        Ok(field_infos.into())
    }

    fn make_variants(
        &mut self,
        variants: &[AstVariant],
        scope: ScopeId,
    ) -> SemaResult<Rc<[VariantInfo]>> {
        let mut variant_infos = Vec::new();
        for variant in variants {
            let name = variant.name;
            let value = match variant.value {
                Some(expr_id) => self.eval_expr(expr_id, scope)?,
                None => ConstValue::Void,
            };

            variant_infos.push(VariantInfo { name, value });
        }
        Ok(variant_infos.into())
    }

    fn bind_pattern(&mut self, pattern_id: PatternId, ty: TypeId) {}

    fn eval_unary_expr(
        &mut self,
        op: UnaryOp,
        value: &ConstValue,
        span: Span,
    ) -> SemaResult<ConstValue> {
        match (op, value) {
            (UnaryOp::Neg, ConstValue::Int(v)) => Ok(ConstValue::Int(-v)),
            (UnaryOp::Not, ConstValue::Bool(v)) => Ok(ConstValue::Bool(!v)),
            (UnaryOp::BitNot, ConstValue::Int(v)) => Ok(ConstValue::Int(!v)),
            (UnaryOp::BitNot, ConstValue::Uint(v)) => Ok(ConstValue::Uint(!v)),
            _ => Err(SemaError::InvalidConstOp { span }),
        }
    }

    fn eval_binary_expr(
        &mut self,
        op: BinaryOp,
        lhs: ConstValue,
        rhs: ConstValue,
        span: Span,
    ) -> SemaResult<ConstValue> {
        let val = match (op, lhs, rhs) {
            (BinaryOp::Add, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Int(a + b),
            (BinaryOp::Sub, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Int(a - b),
            (BinaryOp::Mul, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Int(a * b),
            (BinaryOp::Div, ConstValue::Int(a), ConstValue::Int(b)) => {
                if b == 0 {
                    return Err(SemaError::DivisionByZero { span });
                } else {
                    ConstValue::Int(a / b)
                }
            }
            (BinaryOp::Mod, ConstValue::Int(a), ConstValue::Int(b)) => {
                if b == 0 {
                    return Err(SemaError::DivisionByZero { span });
                } else {
                    ConstValue::Int(a % b)
                }
            }

            (BinaryOp::Add, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Uint(a + b),
            (BinaryOp::Sub, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Uint(a - b),
            (BinaryOp::Mul, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Uint(a * b),
            (BinaryOp::Div, ConstValue::Uint(a), ConstValue::Uint(b)) => {
                if b == 0 {
                    return Err(SemaError::DivisionByZero { span });
                } else {
                    ConstValue::Uint(a / b)
                }
            }
            (BinaryOp::Mod, ConstValue::Uint(a), ConstValue::Uint(b)) => {
                if b == 0 {
                    return Err(SemaError::DivisionByZero { span });
                } else {
                    ConstValue::Uint(a % b)
                }
            }

            (BinaryOp::Eq, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Bool(a == b),
            (BinaryOp::Ne, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Bool(a != b),
            (BinaryOp::Lt, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Bool(a < b),
            (BinaryOp::Le, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Bool(a <= b),
            (BinaryOp::Gt, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Bool(a > b),
            (BinaryOp::Ge, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Bool(a >= b),

            (BinaryOp::Eq, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Bool(a == b),
            (BinaryOp::Ne, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Bool(a != b),
            (BinaryOp::Lt, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Bool(a < b),
            (BinaryOp::Le, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Bool(a <= b),
            (BinaryOp::Gt, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Bool(a > b),
            (BinaryOp::Ge, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Bool(a >= b),

            (BinaryOp::Eq, ConstValue::Bool(a), ConstValue::Bool(b)) => ConstValue::Bool(a == b),
            (BinaryOp::Ne, ConstValue::Bool(a), ConstValue::Bool(b)) => ConstValue::Bool(a != b),

            (BinaryOp::And, ConstValue::Bool(a), ConstValue::Bool(b)) => ConstValue::Bool(a && b),
            (BinaryOp::Or, ConstValue::Bool(a), ConstValue::Bool(b)) => ConstValue::Bool(a || b),

            (BinaryOp::BitAnd, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Int(a & b),
            (BinaryOp::BitOr, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Int(a | b),
            (BinaryOp::BitXor, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Int(a ^ b),
            (BinaryOp::Shl, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Int(a << b),
            (BinaryOp::Shr, ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Int(a >> b),

            (BinaryOp::BitAnd, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Uint(a & b),
            (BinaryOp::BitOr, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Uint(a | b),
            (BinaryOp::BitXor, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Uint(a ^ b),
            (BinaryOp::Shl, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Uint(a << b),
            (BinaryOp::Shr, ConstValue::Uint(a), ConstValue::Uint(b)) => ConstValue::Uint(a >> b),

            _ => return Err(SemaError::InvalidConstOp { span }),
        };

        Ok(val)
    }

    fn eval_assign_expr(
        &mut self,
        op: AssignOp,
        name: Ident,
        val: ConstValue,
        span: Span,
    ) -> SemaResult<ConstValue> {
        match op {
            AssignOp::Assign => match self.lookup_mut(name, span)? {
                Binding::Local { mutable, value } if *mutable => {
                    *value = val;
                    Ok(ConstValue::Void)
                }
                _ => Err(SemaError::InvalidAssignment { name, span }),
            },
            _ => Err(SemaError::InvalidConstOp { span }),
        }
    }
}
