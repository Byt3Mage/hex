pub mod constants;
pub mod liveness;
pub mod lowering;

pub type RegTy = hex_vm::Reg;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Val(RegTy);

pub const ZERO_VAL: Val = Val(0);

impl std::fmt::Display for Val {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "%{}", self.0)
    }
}

impl Val {
    pub fn add(&self, i: RegTy) -> Val {
        Val(self.0 + i)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum BinOp {
    IAdd,
    ISub,
    IMul,
    IDiv,
    IRem,

    UAdd,
    USub,
    UMul,
    UDiv,
    URem,
    FAdd,
    FSub,
    FMul,
    FDiv,
    FRem,
    IEq,
    INe,
    ILt,
    IGt,
    ILe,
    IGe,
    UEq,
    UNe,
    ULt,
    UGt,
    ULe,
    UGe,
    FEq,
    FNe,
    FLt,
    FGt,
    FLe,
    FGe,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum UnOp {
    INeg,
    FNeg,
    BNot,
    INot,
    UNot,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum CastOp {
    IToF,
    FToI,
    UToF,
    FToU,
    IToU,
    UToI,
}

#[derive(Debug, Clone)]
pub enum Inst {
    LoadInt {
        dst: Val,
        value: i64,
    },
    LoadUint {
        dst: Val,
        value: u64,
    },
    LoadBool {
        dst: Val,
        value: bool,
    },
    LoadFloat {
        dst: Val,
        value: f64,
    },
    BinOp {
        dst: Val,
        op: BinOp,
        lhs: Val,
        rhs: Val,
    },
    UnOp {
        dst: Val,
        op: UnOp,
        src: Val,
    },
    Cast {
        dst: Val,
        op: CastOp,
        src: Val,
    },
    Mov {
        dst: Val,
        src: Val,
    },
    Assign {
        dst: Val,
        args: Vec<Val>,
    },
    Call {
        dst: Val,
        func: usize,
        args: Vec<Val>,
        nret: RegTy,
    },
    CallNative {
        dst: Val,
        func: usize,
        args: Vec<Val>,
        nret: RegTy,
    },

    CallIndirect {
        dst: Val,
        func: Val,
        args: Vec<Val>,
        nret: RegTy,
    },
    CallNativeIndirect {
        dst: Val,
        func: Val,
        args: Vec<Val>,
        nret: RegTy,
    },
}

#[derive(Debug, Clone)]
pub enum Terminator {
    Br {
        tgt: usize,
        args: Vec<Val>,
    },
    BrIf {
        cond: Val,
        then_br: usize,
        else_br: usize,
        then_args: Vec<Val>,
        else_args: Vec<Val>,
    },
    Ret(Vec<Val>),
}

#[derive(Debug, Clone)]
pub struct Block {
    pub params: Vec<Val>,
    pub insts: Vec<Inst>,
    pub term: Terminator,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub blocks: Vec<Block>,
    pub nreg: RegTy,
    pub narg: RegTy,
    pub nret: RegTy,
}

impl Inst {
    pub fn for_each_def(&self, mut f: impl FnMut(Val)) {
        match self {
            Inst::Call { dst, nret, .. }
            | Inst::CallNative { dst, nret, .. }
            | Inst::CallIndirect { dst, nret, .. }
            | Inst::CallNativeIndirect { dst, nret, .. } => {
                for i in 0..*nret {
                    f(Val(dst.0 + i));
                }
            }
            Inst::Assign { dst, args } => {
                for i in 0..args.len() as RegTy {
                    f(Val(dst.0 + i));
                }
            }

            Inst::LoadInt { dst, .. }
            | Inst::LoadUint { dst, .. }
            | Inst::LoadBool { dst, .. }
            | Inst::LoadFloat { dst, .. }
            | Inst::BinOp { dst, .. }
            | Inst::UnOp { dst, .. }
            | Inst::Cast { dst, .. }
            | Inst::Mov { dst, .. } => f(*dst),
        }
    }

    pub fn for_each_use(&self, mut f: impl FnMut(Val)) {
        match self {
            Inst::LoadInt { .. }
            | Inst::LoadUint { .. }
            | Inst::LoadBool { .. }
            | Inst::LoadFloat { .. } => {}
            Inst::BinOp { lhs, rhs, .. } => {
                f(*lhs);
                f(*rhs);
            }
            Inst::UnOp { src, .. } | Inst::Cast { src, .. } | Inst::Mov { src, .. } => f(*src),
            Inst::Assign { args, .. } | Inst::Call { args, .. } | Inst::CallNative { args, .. } => {
                args.iter().copied().for_each(f)
            }
            Inst::CallIndirect { func, args, .. } | Inst::CallNativeIndirect { func, args, .. } => {
                f(*func);
                args.iter().copied().for_each(f);
            }
        }
    }
}

impl Terminator {
    pub fn for_each_use(&self, mut f: impl FnMut(Val)) {
        match self {
            Terminator::Br { args, .. } => args.iter().copied().for_each(f),
            Terminator::BrIf {
                cond,
                then_args,
                else_args,
                ..
            } => {
                f(*cond);
                then_args.iter().copied().for_each(&mut f);
                else_args.iter().copied().for_each(&mut f);
            }
            Terminator::Ret(vals) => vals.iter().copied().for_each(f),
        }
    }

    pub fn for_each_successor(&self, mut f: impl FnMut(usize)) {
        match self {
            Terminator::Br { tgt, .. } => f(*tgt),
            Terminator::BrIf {
                then_br, else_br, ..
            } => {
                f(*then_br);
                f(*else_br);
            }
            Terminator::Ret(_) => {}
        }
    }
}

pub struct FunctionBuilder {
    blocks: Vec<Block>,
    current: Vec<Inst>,
    current_params: Vec<Val>,
    next_val: RegTy,
    narg: RegTy,
    nret: RegTy,
    name: String,
    terminated: bool,
}

impl FunctionBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            blocks: Vec::new(),
            current: Vec::new(),
            current_params: Vec::new(),
            next_val: 0,
            narg: 0,
            nret: 0,
            name: name.into(),
            terminated: false,
        }
    }

    pub fn is_terminated(&self) -> bool {
        self.terminated
    }

    pub fn alloc_n(&mut self, n: usize) -> Result<Val, MirError> {
        let base = self.next_val;
        let new_next = (base as usize)
            .checked_add(n)
            .filter(|&v| v <= RegTy::MAX as usize)
            .ok_or(MirError::ValueOverflow)?;
        self.next_val = new_next as RegTy;
        Ok(Val(base))
    }

    pub fn add_arg(&mut self, width: usize) -> Result<Val, MirError> {
        let base = self.alloc_n(width)?;
        self.narg += width as RegTy;
        Ok(base)
    }

    pub fn set_ret(&mut self, width: usize) -> Result<(), MirError> {
        if (self.nret as usize) + width > (RegTy::MAX as usize) {
            return Err(MirError::ValueOverflow);
        }
        self.nret = width as RegTy;
        Ok(())
    }

    #[inline(always)]
    pub fn load_int(&mut self, dst: Val, value: i64) {
        self.current.push(Inst::LoadInt { dst, value });
    }

    #[inline(always)]
    pub fn load_uint(&mut self, dst: Val, value: u64) {
        self.current.push(Inst::LoadUint { dst, value });
    }

    #[inline(always)]
    pub fn load_float(&mut self, dst: Val, value: f64) {
        self.current.push(Inst::LoadFloat { dst, value });
    }

    #[inline(always)]
    pub fn load_bool(&mut self, dst: Val, value: bool) {
        self.current.push(Inst::LoadBool { dst, value });
    }

    #[inline(always)]
    pub fn binop(&mut self, op: BinOp, lhs: Val, rhs: Val) -> Result<Val, MirError> {
        let dst = self.alloc_n(1)?;
        self.current.push(Inst::BinOp { dst, op, lhs, rhs });
        Ok(dst)
    }

    pub fn unop(&mut self, op: UnOp, src: Val) -> Result<Val, MirError> {
        let dst = self.alloc_n(1)?;
        self.current.push(Inst::UnOp { dst, op, src });
        Ok(dst)
    }

    pub fn cast(&mut self, op: CastOp, src: Val) -> Result<Val, MirError> {
        let dst = self.alloc_n(1)?;
        self.current.push(Inst::Cast { dst, op, src });
        Ok(dst)
    }

    pub fn mov(&mut self, src: Val) -> Result<Val, MirError> {
        let dst = self.alloc_n(1)?;
        self.current.push(Inst::Mov { dst, src });
        Ok(dst)
    }

    #[inline(always)]
    pub fn assign(&mut self, dst: Val, args: Vec<Val>) {
        self.current.push(Inst::Assign { dst, args });
    }

    pub fn call(&mut self, func: usize, args: Vec<Val>, nret: RegTy) -> Result<Val, MirError> {
        let dst = self.alloc_n(nret as usize)?;
        self.current.push(Inst::Call {
            dst,
            func,
            args,
            nret,
        });
        Ok(dst)
    }

    pub fn call_native(
        &mut self,
        func: usize,
        args: Vec<Val>,
        nret: RegTy,
    ) -> Result<Val, MirError> {
        let dst = self.alloc_n(nret as usize)?;
        self.current.push(Inst::CallNative {
            dst,
            func,
            args,
            nret,
        });
        Ok(dst)
    }

    pub fn call_indirect(
        &mut self,
        func: Val,
        args: Vec<Val>,
        nret: RegTy,
    ) -> Result<Val, MirError> {
        let dst = self.alloc_n(nret as usize)?;
        self.current.push(Inst::CallIndirect {
            dst,
            func,
            args,
            nret,
        });
        Ok(dst)
    }

    pub fn call_native_indirect(
        &mut self,
        func: Val,
        args: Vec<Val>,
        nret: RegTy,
    ) -> Result<Val, MirError> {
        let dst = self.alloc_n(nret as usize)?;
        self.current.push(Inst::CallNativeIndirect {
            dst,
            func,
            args,
            nret,
        });
        Ok(dst)
    }

    pub fn block_param(&mut self) -> Result<Val, MirError> {
        self.alloc_n(1)
    }

    pub fn begin_block(&mut self, params: Vec<Val>) {
        if !self.terminated {
            panic!("previous block not terminated");
        }

        self.terminated = false;
        self.current_params = params;
    }

    pub fn br(&mut self, tgt: usize, args: Vec<Val>) {
        if self.terminated {
            return;
        }

        let insts = std::mem::take(&mut self.current);
        let params = std::mem::take(&mut self.current_params);
        self.blocks.push(Block {
            params,
            insts,
            term: Terminator::Br { tgt, args },
        });

        self.terminated = true;
    }

    pub fn br_if(
        &mut self,
        cond: Val,
        then_br: usize,
        then_args: Vec<Val>,
        else_br: usize,
        else_args: Vec<Val>,
    ) {
        let insts = std::mem::take(&mut self.current);
        let params = std::mem::take(&mut self.current_params);
        self.blocks.push(Block {
            params,
            insts,
            term: Terminator::BrIf {
                cond,
                then_br,
                else_br,
                then_args,
                else_args,
            },
        });
    }

    pub fn ret(&mut self, vals: Vec<Val>) {
        let insts = std::mem::take(&mut self.current);
        let params = std::mem::take(&mut self.current_params);
        self.blocks.push(Block {
            params,
            insts,
            term: Terminator::Ret(vals),
        });
    }

    pub fn build(self) -> Function {
        assert!(self.current.is_empty(), "unterminated block");
        Function {
            name: self.name,
            blocks: self.blocks,
            nreg: self.next_val,
            narg: self.narg,
            nret: self.nret,
        }
    }
}

pub struct Module {
    pub name: String,
    pub functions: Vec<Function>,
}

#[derive(Debug, thiserror::Error)]
pub enum MirError {
    #[error("value allocation overflowed max value size")]
    ValueOverflow,
    #[error("register assignment overflowed max register size")]
    RegisterOverflow,
}
