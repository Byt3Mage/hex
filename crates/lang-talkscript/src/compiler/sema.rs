use ahash::AHashMap;
use hex_mir::ConstVal;

use crate::{
    arena::Ident,
    compiler::{
        ast::{Ast, BinOp, ExprId, ExprKind, Param, Stmt, StmtKind, UnOp},
        ir,
    },
};

use comptime::{ComptimeEval, ComptimeEvalError, ComptimeVal};
use types::Ty;

pub mod comptime;
pub mod typed_ast;
pub mod types;

#[derive(Debug, thiserror::Error)]
pub enum SemaError {
    #[error("undefined variable: {0:?}")]
    UndefinedVariable(Ident),
    #[error("type mismatch: expected {exp}, got {got}")]
    TypeMismatch { exp: Ty, got: Ty },
    #[error("unsupported binary op {0} for lhs: {1} and rhs: {2}")]
    InvalidBinOp(BinOp, Ty, Ty),
    #[error("unsupported unary op {0} for type {1}")]
    InvalidUnOp(UnOp, Ty),
    #[error("comptime evaluation error: {0}")]
    ComptimeEvalError(#[from] ComptimeEvalError),
    #[error("attempted field access on non-struct/union type: {0}")]
    InvalidFieldAccess(Ty),
    #[error("invalid optional field access on non-optional type: {0}")]
    InvalidOptFieldAccess(Ty),
    #[error("integer literal {val} out of range for {tgt}")]
    IntLitOutOfRange { val: u64, tgt: Ty },
    #[error("division by zero in comptime evaluation")]
    DivisionByZero,
}

#[derive(Debug, Clone)]
struct Expr(Ty, Vec<ir::Val>);

const VOID_EXPR: Expr = Expr(Ty::Void, vec![]);
const NEVER_EXPR: Expr = Expr(Ty::Never, vec![]);
const NULL_EXPR: Expr = Expr(Ty::Null, vec![]);

struct Binding {
    value: Expr,
    mutable: bool,
}

pub struct Sema<'a> {
    ast: &'a Ast,
    func: ir::FunctionBuilder,
    ret_ty: Ty,
    scopes: Vec<AHashMap<Ident, Binding>>,
    comp_eval: ComptimeEval<'a>,
}

impl<'a> Sema<'a> {
    fn push_scope(&mut self) {
        self.scopes.push(AHashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: Ident, value: Expr, mutable: bool) {
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name, Binding { value, mutable });
    }

    fn lookup(&self, name: Ident) -> Result<&Binding, SemaError> {
        for scope in self.scopes.iter().rev() {
            if let Some(binding) = scope.get(&name) {
                return Ok(binding);
            }
        }

        Err(SemaError::UndefinedVariable(name))
    }

    #[inline(always)]
    fn emit(&mut self, inst: ir::Inst) {
        self.func.emit(inst);
    }

    #[inline(always)]
    fn constant(&mut self, dst: ir::Val, val: ConstVal) {
        self.emit(ir::Inst::Const { dst, val });
    }

    fn alloc(&mut self, ty: &[ir::Ty]) -> Vec<ir::Val> {
        ty.iter().map(|&t| self.func.new_val(t)).collect()
    }

    fn lower_expr(&mut self, expr_id: ExprId, expected: Option<&Ty>) -> Result<Expr, SemaError> {
        let expr = self.ast.expr(expr_id);

        let result = match &expr.kind {
            ExprKind::CintLit(c) => {
                let tgt = match expected {
                    Some(Ty::Uint) => Ty::Uint,
                    Some(Ty::Int) | None => Ty::Int,
                    _ => Ty::Int,
                };

                let val = *c;

                match tgt {
                    Ty::Int => {
                        if val > i64::MAX as u64 {
                            return Err(SemaError::IntLitOutOfRange { val, tgt });
                        }
                        let dst = self.func.new_val(ir::Ty::Int);
                        self.emit(ir::Inst::Const {
                            dst,
                            val: ir::ConstVal::Int(val as i64),
                        });
                        Expr(tgt, vec![dst])
                    }
                    Ty::Uint => {
                        let dst = self.func.new_val(ir::Ty::Uint);
                        self.emit(ir::Inst::Const {
                            dst,
                            val: ir::ConstVal::Uint(val),
                        });
                        Expr(tgt, vec![dst])
                    }
                    _ => unreachable!(),
                }
            }
            ExprKind::IntLit(i) => {
                let dst = self.func.new_val(ir::Ty::Int);
                self.constant(dst, ir::ConstVal::Int(*i));
                Expr(Ty::Int, vec![dst])
            }
            ExprKind::UintLit(u) => {
                let dst = self.func.new_val(ir::Ty::Uint);
                self.constant(dst, ir::ConstVal::Uint(*u));
                Expr(Ty::Uint, vec![dst])
            }
            ExprKind::BoolLit(b) => {
                let dst = self.func.new_val(ir::Ty::Bool);
                self.constant(dst, ir::ConstVal::Bool(*b));
                Expr(Ty::Bool, vec![dst])
            }
            ExprKind::FloatLit(f) => {
                let dst = self.func.new_val(ir::Ty::Float);
                self.constant(dst, ir::ConstVal::Float(*f));
                Expr(Ty::Float, vec![dst])
            }
            ExprKind::VoidLit => VOID_EXPR,
            ExprKind::NullLit => NULL_EXPR,
            ExprKind::ArrayLit(elems) => {
                let elem_expected = match expected {
                    Some(Ty::Array { elem_ty, .. }) => Some(elem_ty.as_ref()),
                    _ => None,
                };
                self.lower_arr_lit(elems, elem_expected)?
            }
            ExprKind::ArrayRep { value, count } => {
                let elem_expected = match expected {
                    Some(Ty::Array { elem_ty, .. }) => Some(elem_ty.as_ref()),
                    _ => None,
                };
                self.lower_arr_rep(*value, *count, elem_expected)?
            }
            ExprKind::Ident(name) => self.lookup(*name)?.value.clone(),
            ExprKind::Group(inner) => self.lower_expr(*inner, expected)?,
            ExprKind::Binary { op, lhs, rhs } => self.lower_binary(*op, *lhs, *rhs, expected)?,
            ExprKind::Unary { op, rhs } => self.lower_unary(*op, *rhs, None)?,
            ExprKind::Block(stmts) => self.lower_block(stmts, None)?,

            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.lower_if(*cond, *then_branch, *else_branch, None)?,

            ExprKind::Return(value) => self.lower_return(*value)?,
            ExprKind::StructLit { ty, fields } => todo!(),
            ExprKind::Assign { op, tgt, val } => todo!(),
            ExprKind::While { cond, body } => todo!(),
            ExprKind::Loop(expr_id) => todo!(),
            ExprKind::Break(expr_id) => todo!(),
            ExprKind::Continue => todo!(),
            ExprKind::Call { callee, args } => todo!(),
            ExprKind::Field { object, field } => self.lower_field(*object, *field, None)?,
            ExprKind::OptField { object, field } => self.lower_opt_field(*object, *field, None)?,
            ExprKind::Index { object, index } => todo!(),
            ExprKind::Comptime(expr_id) => todo!(),

            ExprKind::IntType => todo!(),
            ExprKind::UintType => todo!(),
            ExprKind::BoolType => todo!(),
            ExprKind::FloatType => todo!(),
            ExprKind::VoidType => todo!(),
            ExprKind::OptionType(_) => todo!(),
        };

        Ok(result)
    }

    fn lower_binary(
        &mut self,
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
        expected: Option<&Ty>,
    ) -> Result<Expr, SemaError> {
        let lhs_expected = if op.is_comparison() { None } else { expected };
        let Expr(lhs_ty, lhs) = self.lower_expr(lhs, lhs_expected)?;
        let Expr(rhs_ty, rhs) = self.lower_expr(rhs, Some(&lhs_ty))?;

        let (op, ty, ir_ty) = match (op, lhs_ty, rhs_ty) {
            (BinOp::Add, Ty::Int, Ty::Int) => (ir::BinOp::Add, Ty::Int, ir::Ty::Int),
            (BinOp::Sub, Ty::Int, Ty::Int) => (ir::BinOp::Sub, Ty::Int, ir::Ty::Int),
            (BinOp::Mul, Ty::Int, Ty::Int) => (ir::BinOp::Mul, Ty::Int, ir::Ty::Int),
            (BinOp::Div, Ty::Int, Ty::Int) => (ir::BinOp::SDiv, Ty::Int, ir::Ty::Int),
            (BinOp::Rem, Ty::Int, Ty::Int) => (ir::BinOp::SRem, Ty::Int, ir::Ty::Int),

            (BinOp::Add, Ty::Uint, Ty::Uint) => (ir::BinOp::Add, Ty::Uint, ir::Ty::Uint),
            (BinOp::Sub, Ty::Uint, Ty::Uint) => (ir::BinOp::Sub, Ty::Uint, ir::Ty::Uint),
            (BinOp::Mul, Ty::Uint, Ty::Uint) => (ir::BinOp::Mul, Ty::Uint, ir::Ty::Uint),
            (BinOp::Div, Ty::Uint, Ty::Uint) => (ir::BinOp::UDiv, Ty::Uint, ir::Ty::Uint),
            (BinOp::Rem, Ty::Uint, Ty::Uint) => (ir::BinOp::URem, Ty::Uint, ir::Ty::Uint),

            (BinOp::Eq, Ty::Int, Ty::Int) => (ir::BinOp::Eq, Ty::Bool, ir::Ty::Bool),
            (BinOp::Ne, Ty::Int, Ty::Int) => (ir::BinOp::Ne, Ty::Bool, ir::Ty::Bool),
            (BinOp::Eq, Ty::Uint, Ty::Uint) => (ir::BinOp::Eq, Ty::Bool, ir::Ty::Bool),
            (BinOp::Ne, Ty::Uint, Ty::Uint) => (ir::BinOp::Ne, Ty::Bool, ir::Ty::Bool),

            (BinOp::Gt, Ty::Int, Ty::Int) => (ir::BinOp::SGt, Ty::Bool, ir::Ty::Bool),
            (BinOp::Lt, Ty::Int, Ty::Int) => (ir::BinOp::SLt, Ty::Bool, ir::Ty::Bool),
            (BinOp::Ge, Ty::Int, Ty::Int) => (ir::BinOp::SGe, Ty::Bool, ir::Ty::Bool),
            (BinOp::Le, Ty::Int, Ty::Int) => (ir::BinOp::SLe, Ty::Bool, ir::Ty::Bool),

            (BinOp::Gt, Ty::Uint, Ty::Uint) => (ir::BinOp::UGt, Ty::Bool, ir::Ty::Bool),
            (BinOp::Lt, Ty::Uint, Ty::Uint) => (ir::BinOp::ULt, Ty::Bool, ir::Ty::Bool),
            (BinOp::Ge, Ty::Uint, Ty::Uint) => (ir::BinOp::UGe, Ty::Bool, ir::Ty::Bool),
            (BinOp::Le, Ty::Uint, Ty::Uint) => (ir::BinOp::ULe, Ty::Bool, ir::Ty::Bool),

            (BinOp::Gt, Ty::Float, Ty::Float) => (ir::BinOp::FGt, Ty::Bool, ir::Ty::Bool),
            (BinOp::Lt, Ty::Float, Ty::Float) => (ir::BinOp::FLt, Ty::Bool, ir::Ty::Bool),
            (BinOp::Ge, Ty::Float, Ty::Float) => (ir::BinOp::FGe, Ty::Bool, ir::Ty::Bool),
            (BinOp::Le, Ty::Float, Ty::Float) => (ir::BinOp::FLe, Ty::Bool, ir::Ty::Bool),

            (BinOp::Coalesce, Ty::Optional(inner), rhs_ty) if *inner == rhs_ty => {
                todo!("null coalesce")
            }

            (op, lhs, rhs) => return Err(SemaError::InvalidBinOp(op, lhs, rhs)),
        };

        let dst = self.func.new_val(ir_ty);
        self.func.emit(ir::Inst::Binary {
            dst,
            op,
            lhs: lhs[0],
            rhs: rhs[0],
        });

        Ok(Expr(ty, vec![dst]))
    }

    fn lower_unary(
        &mut self,
        op: UnOp,
        rhs: ExprId,
        expected: Option<&Ty>,
    ) -> Result<Expr, SemaError> {
        let Expr(rhs_ty, rhs) = self.lower_expr(rhs, expected)?;

        let (op, ty, ir_ty) = match (op, rhs_ty) {
            (UnOp::Not, Ty::Int) => (ir::UnOp::Not, Ty::Int, ir::Ty::Int),
            (UnOp::Not, Ty::Uint) => (ir::UnOp::Not, Ty::Uint, ir::Ty::Uint),
            (UnOp::Not, Ty::Bool) => (ir::UnOp::BNot, Ty::Bool, ir::Ty::Bool),
            (UnOp::Neg, Ty::Int) => (ir::UnOp::INeg, Ty::Int, ir::Ty::Int),
            (UnOp::Neg, Ty::Float) => (ir::UnOp::FNeg, Ty::Float, ir::Ty::Float),

            (op, ty) => return Err(SemaError::InvalidUnOp(op, ty)),
        };

        let dst = self.func.new_val(ir_ty);
        self.emit(ir::Inst::Unary {
            dst,
            op,
            src: rhs[0],
        });

        Ok(Expr(ty, vec![dst]))
    }

    fn lower_block(&mut self, stmts: &[Stmt], expected: Option<&Ty>) -> Result<Expr, SemaError> {
        self.push_scope();

        let last = stmts.len() - 1;
        let mut result = VOID_EXPR;

        for (i, stmt) in stmts.iter().enumerate() {
            match &stmt.kind {
                StmtKind::Let {
                    name,
                    ty,
                    value,
                    mutable,
                } => {
                    let tgt_ty = match ty {
                        Some(ty_expr) => Some(self.comp_eval.eval_type(*ty_expr)?),
                        None => None,
                    };

                    let val = self.lower_expr(*value, tgt_ty.as_ref())?;

                    let bound = match &tgt_ty {
                        None => val,
                        Some(t) => self.coerce(t, val)?,
                    };

                    self.define(*name, bound, *mutable);
                    result = VOID_EXPR;
                }

                StmtKind::Semi(expr_id) => {
                    // Lower expression, but result is supressed.
                    // Diverging expressions always diverge the block.
                    let Expr(val_ty, _) = self.lower_expr(*expr_id, None)?;
                    result = match val_ty {
                        Ty::Never => NEVER_EXPR,
                        _ => VOID_EXPR,
                    };
                }

                StmtKind::Expr(expr_id) => {
                    // Expression without semicolon must be the last or coerce to void type.
                    // We use coerce instead of direct comparison to support diverging expressions.
                    // E.g. `return (x, y)` // no semi colon.
                    let val = self.lower_expr(*expr_id, if i == last { expected } else { None })?;
                    if i == last {
                        result = val;
                    } else {
                        self.coerce(&Ty::Void, val)?;
                    }
                }
            }
        }

        self.pop_scope();
        Ok(result)
    }

    fn lower_if(
        &mut self,
        cond: ExprId,
        then_expr: ExprId,
        else_expr: Option<ExprId>,
        expected: Option<&Ty>,
    ) -> Result<Expr, SemaError> {
        let cond = self.lower_expr(cond, expected)?;
        let Expr(_, cond) = self.coerce(&Ty::Bool, cond)?;

        let then_blk = self.func.new_block();
        let else_blk = self.func.new_block();
        let join_blk = self.func.new_block();

        self.func.set_term(ir::Term::Branch {
            cond: cond[0],
            then_blk,
            then_args: vec![],
            else_blk,
            else_args: vec![],
        });

        self.func.switch_to(then_blk);
        let Expr(then_ty, then_res) = self.lower_expr(then_expr, expected)?;
        let then_terminates = matches!(then_ty, Ty::Never);

        self.func.switch_to(else_blk);
        let Expr(else_ty, else_res) = match else_expr {
            Some(e) => self.lower_expr(e, expected)?,
            None => VOID_EXPR,
        };
        let else_terminates = matches!(else_ty, Ty::Never);

        // Determine the unified result type.
        let res_ty = match (then_terminates, else_terminates) {
            (true, true) => {
                // Both branches diverge. No join block needed; just close out
                // both branches with whatever they ended on (presumably already
                // terminated). The if-expression's type is Never.
                return Ok(NEVER_EXPR);
            }
            (true, false) => else_ty.clone(),
            (false, true) => then_ty.clone(),
            (false, false) => {
                // TODO: proper unify
                if then_ty != else_ty {
                    return Err(SemaError::TypeMismatch {
                        exp: then_ty,
                        got: else_ty,
                    });
                }
                then_ty.clone()
            }
        };

        let res_vals: Vec<ir::Val> = res_ty
            .to_ir()
            .iter()
            .map(|&t| self.func.add_param(join_blk, t))
            .collect();

        if !then_terminates {
            self.func.switch_to(then_blk);
            self.func.set_term(ir::Term::Jump {
                tgt: join_blk,
                args: then_res,
            });
        }

        if !else_terminates {
            self.func.switch_to(else_blk);
            self.func.set_term(ir::Term::Jump {
                tgt: join_blk,
                args: else_res,
            });
        }

        self.func.switch_to(join_blk);

        Ok(Expr(res_ty, res_vals))
    }

    fn lower_return(&mut self, value: Option<ExprId>) -> Result<Expr, SemaError> {
        let ret_ty = self.ret_ty.clone();

        let val = match value {
            Some(expr) => self.lower_expr(expr, Some(&ret_ty))?,
            None => VOID_EXPR,
        };

        let Expr(_, vals) = self.coerce(&ret_ty, val)?;
        self.func.set_term(ir::Term::Return { vals });
        Ok(NEVER_EXPR)
    }

    fn lower_arr_lit(
        &mut self,
        elems: &[ExprId],
        expected: Option<&Ty>,
    ) -> Result<Expr, SemaError> {
        let [first, rest @ ..] = elems else {
            todo!("lower empty array")
        };

        let Expr(first_ty, first) = self.lower_expr(*first, expected)?;
        let ty = Ty::array(first_ty.clone(), elems.len());
        let mut vals = first.clone();

        for elem in rest {
            let elem = self.lower_expr(*elem, expected)?;
            let Expr(_, elem) = self.coerce(&first_ty, elem)?;
            vals.extend(elem);
        }

        Ok(Expr(ty, vals))
    }

    fn lower_arr_rep(
        &mut self,
        value: ExprId,
        count: ExprId,
        expected: Option<&Ty>,
    ) -> Result<Expr, SemaError> {
        let Expr(val_ty, value) = self.lower_expr(value, expected)?;
        let len = match self.comp_eval.eval(count)? {
            ComptimeVal::Uint(len) => len as usize,
            val => {
                return Err(SemaError::TypeMismatch {
                    exp: Ty::Uint,
                    got: self.comp_eval.value_ty(&val),
                });
            }
        };

        let ty = Ty::array(val_ty.clone(), len);
        let mut vals = Vec::with_capacity(value.len() * len);
        (0..len).for_each(|_| vals.extend(&value));
        Ok(Expr(ty, vals))
    }

    fn lower_field(
        &mut self,
        object: ExprId,
        field: Ident,
        expected: Option<&Ty>,
    ) -> Result<Expr, SemaError> {
        let Expr(obj_ty, obj) = self.lower_expr(object, expected)?;

        match &obj_ty {
            Ty::Struct(ty) => {
                let (offset, field) = ty.field(field).unwrap();
                let ty = field.ty.clone();
                let vals = Vec::from(&obj[offset..offset + ty.size()]);
                Ok(Expr(ty, vals))
            }
            Ty::Union(ty) => {
                let (idx, field) = ty.field(field).unwrap();
                let field_tag = self.func.new_val(ir::Ty::Uint);
                let opt_tag = self.func.new_val(ir::Ty::Bool);
                self.constant(field_tag, ir::ConstVal::Uint(idx as u64));
                self.emit(ir::Inst::Binary {
                    dst: opt_tag,
                    op: ir::BinOp::Eq,
                    lhs: obj[0],
                    rhs: field_tag,
                });
                let mut vals = vec![opt_tag];
                vals.extend(&obj[1..1 + field.ty.size()]);
                Ok(Expr(Ty::option(field.ty.clone()), vals))
            }
            _ => Err(SemaError::InvalidFieldAccess(obj_ty)),
        }
    }

    fn lower_opt_field(
        &mut self,
        object: ExprId,
        field: Ident,
        expected: Option<&Ty>,
    ) -> Result<Expr, SemaError> {
        let Expr(obj_ty, obj) = self.lower_expr(object, expected)?;
        let Ty::Optional(obj_ty) = obj_ty else {
            return Err(SemaError::InvalidOptFieldAccess(obj_ty));
        };

        match *obj_ty {
            Ty::Struct(ty) => {
                let (offset, field) = ty.field(field).unwrap();
                let mut vals = vec![obj[0]];
                vals.extend(&obj[offset..offset + field.ty.size()]);
                Ok(Expr(Ty::option(field.ty.clone()), vals))
            }

            ty => Err(SemaError::InvalidFieldAccess(ty)),
        }
    }

    fn coerce(&mut self, tgt: &Ty, Expr(val_ty, val): Expr) -> Result<Expr, SemaError> {
        if tgt == &val_ty {
            return Ok(Expr(val_ty, val));
        }

        match (tgt, &val_ty) {
            (_, Ty::Never) => Ok(Expr(tgt.clone(), vec![])),

            (Ty::Optional(inner), _) if **inner == val_ty => {
                let tag = self.func.new_val(ir::Ty::Bool);
                self.constant(tag, ir::ConstVal::Bool(true));

                let mut vals = vec![tag];
                vals.extend(&val);
                Ok(Expr(tgt.clone(), vals))
            }

            (Ty::Optional(_), Ty::Null) => {
                let vals = self.alloc(&tgt.to_ir());
                self.constant(vals[0], ir::ConstVal::Bool(false));
                for &v in &vals[1..] {
                    self.constant(v, ConstVal::Uint(0));
                }
                Ok(Expr(tgt.clone(), vals))
            }

            (
                Ty::Pointer {
                    pointee: tgt_ptr,
                    is_mut: false,
                },
                Ty::Pointer {
                    pointee: val_ptr,
                    is_mut: val_mut,
                },
            ) if tgt_ptr == val_ptr => todo!("pointer coerce"),

            _ => Err(SemaError::TypeMismatch {
                exp: tgt.clone(),
                got: val_ty.clone(),
            }),
        }
    }
}

pub fn lower_function<'a>(
    ast: &'a Ast,
    name: impl Into<String>,
    params: &[Param],
    ret: Option<ExprId>,
    body: ExprId,
) -> Result<ir::Function, SemaError> {
    let comp_eval = ComptimeEval::new(ast);

    let ret_ty = match ret {
        Some(r) => comp_eval.eval_type(r)?,
        None => Ty::Void,
    };

    let ret_ir_ty = ret_ty.to_ir();

    let mut func = ir::FunctionBuilder::new(name, ret_ir_ty.clone());
    let entry = func.entry();

    let mut param_bindings = AHashMap::new();
    for param in params {
        let ty = comp_eval.eval_type(param.ty)?;
        let ir_ty = ty.to_ir();
        let vals = ir_ty.iter().map(|&ty| func.add_param(entry, ty)).collect();

        param_bindings.insert(
            param.name,
            Binding {
                value: Expr(ty, vals),
                mutable: param.mutable,
            },
        );
    }

    let mut sema = Sema {
        ast,
        func,
        ret_ty,
        scopes: vec![param_bindings],
        comp_eval,
    };

    sema.func.switch_to(entry);

    let result = sema.lower_expr(body, None)?;

    if !sema.func.is_terminated() {
        let ret_ty = sema.ret_ty.clone();
        let Expr(_, vals) = sema.coerce(&ret_ty, result)?;
        sema.func.set_term(ir::Term::Return { vals });
    }

    Ok(sema.func.finish())
}
