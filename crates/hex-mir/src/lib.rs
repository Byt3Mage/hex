pub mod liveness;
pub mod lowering;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Val(u32);

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
    Const(Ty, u64),
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

#[test]
fn test_mir() {
    use crate::lowering::lower_function;
    use hex_vm::disassemble::disassemble;

    // struct Enemy {
    //     x: i64,
    //     y: i64,
    //     health: i64,
    //     scores: [i64; 2],  // kills, deaths
    // }
    //
    // Flattened: 5 registers [x, y, health, kills, deaths]
    //
    // fn spawn_enemy(x: i64, y: i64) -> Enemy {
    //     return Enemy { x, y, health: 100, scores: [0, 0] };
    // }
    //
    // fn total_score(e: Enemy) -> i64 {
    //     return e.scores[0] + e.scores[1];
    // }

    // --- spawn_enemy ---
    let mut spawn = FuncDef::new(
        "spawn_enemy",
        vec![Ty::I64, Ty::I64, Ty::I64, Ty::I64, Ty::I64],
    );
    let (sb0, sp) = spawn.new_block(vec![Ty::I64, Ty::I64]); // x, y

    let health = spawn.push_inst(sb0, Ty::I64, Inst::Const(Ty::I64, 100));
    let zero1 = spawn.push_inst(sb0, Ty::I64, Inst::Const(Ty::I64, 0));
    let zero2 = spawn.push_inst(sb0, Ty::I64, Inst::Const(Ty::I64, 0));

    // Return [x, y, health, kills, deaths]
    spawn.set_term(sb0, Term::Ret(vec![sp[0], sp[1], health, zero1, zero2]));

    // --- total_score ---
    // Calls spawn_enemy, then extracts scores and adds them.
    let mut total = FuncDef::new("total_score", vec![Ty::I64]);
    let (tb0, tp) = total.new_block(vec![Ty::I64, Ty::I64]); // x, y args to pass through

    // Call spawn_enemy(x, y) -> returns 5 values
    let call = total.push_inst(tb0, Ty::I64, Inst::Call(FuncRef(0), vec![tp[0], tp[1]]));

    // Extract kills (index 3) and deaths (index 4)
    let kills = total.push_inst(tb0, Ty::I64, Inst::Result(call, 3));
    let deaths = total.push_inst(tb0, Ty::I64, Inst::Result(call, 4));

    // Add them
    let sum = total.push_inst(tb0, Ty::I64, Inst::BinOp(BinOp::IAdd, kills, deaths));

    total.set_term(tb0, Term::Ret(vec![sum]));

    // Lower and print function
    println!("spawn:");
    let lowered = lower_function(&spawn);
    disassemble(&lowered.bytecode, &lowered.constants);

    println!("total_score:");
    let lowered = lower_function(&total);
    disassemble(&lowered.bytecode, &lowered.constants);
}
