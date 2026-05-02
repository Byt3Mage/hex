pub use hex_vm::Reg;

pub mod constants;
pub mod liveness;
pub mod lowering;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Val(pub Reg);

impl std::fmt::Display for Val {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "%{}", self.0)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum UnOp {
    INeg,
    FNeg,
    BNot,
    INot,
    UNot,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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
    Call {
        dst: Val,
        func: usize,
        args: Vec<Val>,
    },
    CallNative {
        dst: Val,
        func: usize,
        args: Vec<Val>,
    },

    CallIndirect {
        dst: Val,
        func: Val,
        args: Vec<Val>,
    },
    CallNativeIndirect {
        dst: Val,
        func: Val,
        args: Vec<Val>,
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
    pub nreg: Reg,
    pub narg: Reg,
    pub nret: Reg,
}

impl Inst {
    pub fn for_each_def(&self, mut f: impl FnMut(&Val)) {
        match self {
            Inst::LoadInt { dst, .. }
            | Inst::LoadUint { dst, .. }
            | Inst::LoadBool { dst, .. }
            | Inst::LoadFloat { dst, .. }
            | Inst::BinOp { dst, .. }
            | Inst::UnOp { dst, .. }
            | Inst::Cast { dst, .. }
            | Inst::Mov { dst, .. }
            | Inst::Call { dst, .. }
            | Inst::CallNative { dst, .. }
            | Inst::CallIndirect { dst, .. }
            | Inst::CallNativeIndirect { dst, .. } => f(dst),
        }
    }

    pub fn for_each_use(&self, mut f: impl FnMut(&Val)) {
        match self {
            Inst::LoadInt { .. }
            | Inst::LoadUint { .. }
            | Inst::LoadBool { .. }
            | Inst::LoadFloat { .. } => {}
            Inst::BinOp { lhs, rhs, .. } => {
                f(lhs);
                f(rhs);
            }
            Inst::UnOp { src, .. } | Inst::Cast { src, .. } | Inst::Mov { src, .. } => f(src),
            Inst::Call { args, .. } | Inst::CallNative { args, .. } => args.iter().for_each(f),
            Inst::CallIndirect { func, args, .. } | Inst::CallNativeIndirect { func, args, .. } => {
                f(func);
                args.iter().for_each(f);
            }
        }
    }
}

impl Terminator {
    pub fn for_each_use(&self, mut f: impl FnMut(&Val)) {
        match self {
            Terminator::Br { args, .. } => args.iter().for_each(f),
            Terminator::BrIf {
                cond,
                then_args,
                else_args,
                ..
            } => {
                f(cond);
                then_args.iter().for_each(&mut f);
                else_args.iter().for_each(&mut f);
            }
            Terminator::Ret(vals) => vals.iter().for_each(f),
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
    next_val: Reg,
    narg: Reg,
    nret: Reg,
    name: String,
}

impl FunctionBuilder {
    pub fn new(name: impl Into<String>, narg: usize, nret: usize) -> Self {
        assert!(narg < 256 && nret < 256);
        Self {
            blocks: Vec::new(),
            current: Vec::new(),
            current_params: Vec::new(),
            next_val: narg as Reg,
            narg: narg as Reg,
            nret: nret as Reg,
            name: name.into(),
        }
    }

    pub fn arg(&self, n: Reg) -> Val {
        assert!(n < self.narg, "arg index out of bounds");
        Val(n)
    }

    fn alloc(&mut self) -> Val {
        let v = Val(self.next_val);
        self.next_val += 1;
        v
    }

    pub fn load_int(&mut self, value: i64) -> Val {
        let dst = self.alloc();
        self.current.push(Inst::LoadInt { dst, value });
        dst
    }

    pub fn load_uint(&mut self, value: u64) -> Val {
        let dst = self.alloc();
        self.current.push(Inst::LoadUint { dst, value });
        dst
    }

    pub fn load_float(&mut self, value: f64) -> Val {
        let dst = self.alloc();
        self.current.push(Inst::LoadFloat { dst, value });
        dst
    }

    pub fn load_bool(&mut self, value: bool) -> Val {
        let dst = self.alloc();
        self.current.push(Inst::LoadBool { dst, value });
        dst
    }

    pub fn binop(&mut self, op: BinOp, lhs: Val, rhs: Val) -> Val {
        let dst = self.alloc();
        self.current.push(Inst::BinOp { dst, op, lhs, rhs });
        dst
    }

    pub fn unop(&mut self, op: UnOp, src: Val) -> Val {
        let dst = self.alloc();
        self.current.push(Inst::UnOp { dst, op, src });
        dst
    }

    pub fn cast(&mut self, op: CastOp, src: Val) -> Val {
        let dst = self.alloc();
        self.current.push(Inst::Cast { dst, op, src });
        dst
    }

    pub fn mov(&mut self, src: Val) -> Val {
        let dst = self.alloc();
        self.current.push(Inst::Mov { dst, src });
        dst
    }

    pub fn call(&mut self, func: usize, args: Vec<Val>) -> Val {
        let dst = self.alloc();
        self.current.push(Inst::Call { dst, func, args });
        dst
    }

    pub fn call_native(&mut self, func: usize, args: Vec<Val>) -> Val {
        let dst = self.alloc();
        self.current.push(Inst::CallNative { dst, func, args });
        dst
    }

    pub fn call_indirect(&mut self, func: Val, args: Vec<Val>) -> Val {
        let dst = self.alloc();
        self.current.push(Inst::CallIndirect { dst, func, args });
        dst
    }

    pub fn block_param(&mut self) -> Val {
        self.alloc()
    }

    pub fn begin_block(&mut self, params: Vec<Val>) {
        self.current_params = params;
    }

    pub fn br(&mut self, tgt: usize, args: Vec<Val>) {
        let insts = std::mem::take(&mut self.current);
        let params = std::mem::take(&mut self.current_params);
        self.blocks.push(Block {
            params,
            insts,
            term: Terminator::Br { tgt, args },
        });
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
