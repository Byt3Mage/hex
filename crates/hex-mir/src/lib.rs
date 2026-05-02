pub mod liveness;
pub mod lowering;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Val(pub u32);

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Block(u32);

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct FuncRef(u32);

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct NativeFuncRef(u32);

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    I64,
    U64,
    F64,
    Bool,
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
pub enum ConvOp {
    IToF,
    FToI,
    UToF,
    FToU,
    IToU,
    UToI,
}

#[derive(Debug, Clone)]
pub enum Inst {
    IntLit(i64),
    UintLit(u64),
    BoolLit(bool),
    BinOp(BinOp, Val, Val),
    UnOp(UnOp, Val),
    Conv(ConvOp, Val),
    Call(FuncRef, Vec<Val>),
    CallNative(NativeFuncRef, Vec<Val>),
    CallIndirect(Val, Vec<Val>),
    Result(Val, u32),
}

#[derive(Debug, Clone)]
pub enum Term {
    Br(Block, Vec<Val>),
    BrIf(Val, Block, Vec<Val>, Block, Vec<Val>),
    Ret(Vec<Val>),
}

#[derive(Debug, Clone)]
pub struct InstDef {
    pub val: Val,
    pub inst: Inst,
}

#[derive(Debug, Clone)]
pub struct BlockDef {
    pub params: Vec<Val>,
    pub insts: Vec<InstDef>,
    pub term: Term,
}

#[derive(Debug, Clone)]
pub struct FuncDef {
    pub name: String,
    pub ret_tys: Vec<Ty>,
    pub entry: Block,
    pub blocks: Vec<BlockDef>,
    val_tys: Vec<Ty>,
    next_val: u32,
    next_block: u32,
}

impl FuncDef {
    pub fn new(name: impl Into<String>, ret_tys: Vec<Ty>) -> Self {
        Self {
            name: name.into(),
            ret_tys,
            entry: Block(0),
            blocks: Vec::new(),
            val_tys: Vec::new(),
            next_val: 0,
            next_block: 0,
        }
    }

    fn alloc_val(&mut self, ty: Ty) -> Val {
        let v = Val(self.next_val);
        self.next_val += 1;
        self.val_tys.push(ty);
        v
    }

    pub fn val_ty(&self, val: Val) -> Ty {
        self.val_tys[val.0 as usize]
    }

    pub fn new_block(&mut self, param_tys: Vec<Ty>) -> (Block, Vec<Val>) {
        let block = Block(self.next_block);
        self.next_block += 1;

        let params: Vec<Val> = param_tys.iter().map(|&ty| self.alloc_val(ty)).collect();

        self.blocks.push(BlockDef {
            params: params.clone(),
            insts: Vec::new(),
            term: Term::Ret(vec![]),
        });

        (block, params)
    }

    pub fn push_inst(&mut self, block: Block, ty: Ty, inst: Inst) -> Val {
        let val = self.alloc_val(ty);
        let block = block.0 as usize;
        self.blocks[block].insts.push(InstDef { val, inst });
        val
    }

    pub fn set_term(&mut self, block: Block, term: Term) {
        self.blocks[block.0 as usize].term = term;
    }
}

#[derive(Debug, Clone)]
pub struct Module {
    pub name: String,
    pub functions: Vec<FuncDef>,
    pub native_functions: Vec<NativeFuncDecl>,
    pub exports: Vec<Export>,
    pub imports: Vec<Import>,
}

#[derive(Debug, Clone)]
pub struct NativeFuncDecl {
    pub name: String,
    pub arg_tys: Vec<Ty>,
    pub ret_tys: Vec<Ty>,
}

#[derive(Debug, Clone)]
pub struct Export {
    pub name: String,
    pub func: FuncRef,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub arg_tys: Vec<Ty>,
    pub ret_tys: Vec<Ty>,
}

impl std::fmt::Display for Val {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "%{}", self.0)
    }
}

impl std::fmt::Display for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

impl std::fmt::Display for Inst {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Inst::IntLit(i) => write!(f, "int({i})"),
            Inst::UintLit(u) => write!(f, "uint({u})"),
            Inst::BoolLit(b) => write!(f, "bool({b})"),
            Inst::BinOp(op, a, b) => write!(f, "{op:?} {a} {b}"),
            Inst::UnOp(op, a) => write!(f, "{op:?} {a}"),
            Inst::Conv(op, a) => write!(f, "{op:?} {a}"),
            Inst::Call(func, vals) => write!(f, "call({func:?}) {vals:?}"),
            Inst::CallNative(func, vals) => write!(f, "call({func:?}) {vals:?}"),
            Inst::CallIndirect(func, vals) => write!(f, "call({func}) {vals:?}"),
            Inst::Result(val, idx) => write!(f, "res({val})[{idx}]"),
        }
    }
}
