use std::collections::HashSet;

use hex_vm::{
    instruction::*,
    opcode::Opcode,
    value::{IsValue, Value},
};

use crate::{BinOp, Block, ConvOp, FuncDef, Inst, InstDef, Term, UnOp, Val, liveness::Liveness};

pub struct LoweredFunction {
    pub bytecode: Vec<Instruction>,
    pub constants: Vec<Value>,
    pub nreg: Reg,
    pub narg: Reg,
    pub nret: Reg,
}

struct RegAlloc {
    reg_map: Vec<Option<Reg>>,
    free_regs: Vec<Reg>,
    next_reg: Reg,
    protected: HashSet<u32>,
}

impl RegAlloc {
    fn new(num_vals: u32, protected: HashSet<u32>) -> Self {
        Self {
            reg_map: vec![None; num_vals as usize],
            free_regs: Vec::new(),
            next_reg: 0,
            protected,
        }
    }

    fn alloc(&mut self, val: Val) -> Reg {
        let reg = self.free_regs.pop().unwrap_or_else(|| {
            let r = self.next_reg;
            self.next_reg += 1;
            r
        });
        self.reg_map[val.0 as usize] = Some(reg);
        reg
    }

    fn alloc_prefer_dying(
        &mut self,
        val: Val,
        inst: &Inst,
        live: &Liveness,
        blk_idx: usize,
        inst_idx: usize,
    ) -> Reg {
        let result = Liveness::any_inst_operand(inst, |op| {
            if self.protected.contains(&op.0) {
                return None;
            }
            if live.is_last_use(op, blk_idx, inst_idx) {
                if let Some(reg) = self.reg_map[op.0 as usize] {
                    return Some((op, reg));
                }
            }
            None
        });

        if let Some((op, reg)) = result {
            self.reg_map[op.0 as usize] = None;
            self.reg_map[val.0 as usize] = Some(reg);
            return reg;
        }

        self.alloc(val)
    }

    fn free(&mut self, val: Val) {
        if self.protected.contains(&val.0) {
            return;
        }
        if let Some(reg) = self.reg_map[val.0 as usize].take() {
            self.free_regs.push(reg);
        }
    }

    fn get(&self, val: Val) -> Reg {
        self.reg_map[val.0 as usize].expect("value has no register")
    }

    fn nreg(&self) -> Reg {
        self.next_reg
    }
}

struct PendingJump {
    inst_index: usize,
    target_block: Block,
}

pub fn lower_function(func: &FuncDef) -> LoweredFunction {
    let liveness = Liveness::compute(func);

    let protected: HashSet<u32> = func
        .blocks
        .iter()
        .flat_map(|b| b.params.iter().map(|p| p.0))
        .collect();

    let mut regs = RegAlloc::new(func.next_val, protected);
    let mut bytecode: Vec<Instruction> = Vec::new();
    let mut constants: Vec<Value> = Vec::new();
    let mut block_offsets: Vec<Option<u32>> = vec![None; func.next_block as usize];
    let mut pending_jumps: Vec<PendingJump> = Vec::new();

    let narg = func.blocks[func.entry.0 as usize].params.len() as Reg;

    for block in &func.blocks {
        for &param in &block.params {
            regs.alloc(param);
        }
    }

    for (block_idx, block) in func.blocks.iter().enumerate() {
        block_offsets[block_idx] = Some(bytecode.len() as u32);

        for (inst_idx, inst_def) in block.insts.iter().enumerate() {
            lower_inst(
                &mut regs,
                &liveness,
                &mut bytecode,
                &mut constants,
                inst_def,
                block_idx,
                inst_idx,
            );
        }

        lower_term(
            func,
            &mut regs,
            &liveness,
            &mut bytecode,
            &mut pending_jumps,
            &block.term,
            block_idx,
        );
    }

    for jump in &pending_jumps {
        let target = block_offsets[jump.target_block.0 as usize].expect("jump to undefined block");
        bytecode[jump.inst_index] = patch_jump_target(bytecode[jump.inst_index], target);
    }

    let nret = func.ret_tys.len() as Reg;

    LoweredFunction {
        bytecode,
        constants,
        nreg: regs.nreg(),
        narg,
        nret,
    }
}

fn lower_inst(
    regs: &mut RegAlloc,
    live: &Liveness,
    bc: &mut Vec<Instruction>,
    consts: &mut Vec<Value>,
    inst_def: &InstDef,
    blk_idx: usize,
    inst_idx: usize,
) {
    let InstDef { val, inst } = inst_def;

    match inst {
        Inst::Const(_, bits) => {
            let dst = regs.alloc(*val);
            let const_idx = consts.len() as InstType;
            consts.push(bits.into_value());
            bc.push(const_(dst, const_idx));
        }

        Inst::BinOp(op, a, b) => {
            let ra = regs.get(*a);
            let rb = regs.get(*b);
            let dst = regs.alloc_prefer_dying(*val, inst, live, blk_idx, inst_idx);
            bc.push(lower_binop(*op, dst, ra, rb));
        }

        Inst::UnOp(op, a) => {
            let src = regs.get(*a);
            let dst = regs.alloc_prefer_dying(*val, inst, live, blk_idx, inst_idx);
            bc.push(lower_unop(*op, dst, src));
        }

        Inst::Conv(op, a) => {
            let src = regs.get(*a);
            let dst = regs.alloc_prefer_dying(*val, inst, live, blk_idx, inst_idx);
            bc.push(lower_conv(*op, dst, src));
        }

        Inst::Call(func_ref, args) => {
            let dst = regs.alloc(*val);
            let moves: Vec<(Reg, Reg)> = args
                .iter()
                .enumerate()
                .map(|(i, &arg)| (regs.get(arg), dst + i as Reg))
                .filter(|(s, d)| s != d)
                .collect();
            resolve_parallel_moves(&moves, &mut regs.next_reg, bc);
            bc.push(call(dst, func_ref.0 as InstType));
        }

        Inst::CallNative(func_ref, args) => {
            let dst = regs.alloc(*val);
            let moves: Vec<(Reg, Reg)> = args
                .iter()
                .enumerate()
                .map(|(i, &arg)| (regs.get(arg), dst + i as Reg))
                .filter(|(s, d)| s != d)
                .collect();
            resolve_parallel_moves(&moves, &mut regs.next_reg, bc);
            bc.push(calln(dst, func_ref.0 as InstType));
        }

        Inst::CallIndirect(func_val, args) => {
            let dst = regs.alloc(*val);
            let mut func_reg = regs.get(*func_val);
            let moves: Vec<(Reg, Reg)> = args
                .iter()
                .enumerate()
                .map(|(i, &arg)| (regs.get(arg), dst + i as Reg))
                .filter(|(s, d)| s != d)
                .collect();

            // Check if func_reg would be clobbered by any arg move
            if moves.iter().any(|&(_, d)| d == func_reg) {
                let tmp = regs.next_reg;
                regs.next_reg += 1;
                bc.push(mov(tmp, func_reg));
                func_reg = tmp;
            }

            resolve_parallel_moves(&moves, &mut regs.next_reg, bc);
            bc.push(callr(dst, func_reg));
        }

        Inst::Result(call_val, index) => {
            let dst = regs.alloc(*val);
            let call_dst = regs.get(*call_val);
            let src = call_dst + *index as Reg;
            if dst != src {
                bc.push(mov(dst, src));
            }
        }
    }

    Liveness::for_each_operand(&inst_def.inst, |op| {
        if live.is_last_use(op, blk_idx, inst_idx) {
            regs.free(op);
        }
    });
}

fn lower_term(
    func: &FuncDef,
    regs: &mut RegAlloc,
    liveness: &Liveness,
    bytecode: &mut Vec<Instruction>,
    pending_jumps: &mut Vec<PendingJump>,
    term: &Term,
    block_idx: usize,
) {
    let term_idx = func.blocks[block_idx].insts.len();

    match term {
        Term::Br(target, args) => {
            emit_block_args(func, regs, bytecode, *target, args);
            pending_jumps.push(PendingJump {
                inst_index: bytecode.len(),
                target_block: *target,
            });
            bytecode.push(jmp(0));
        }

        Term::BrIf(cond, t_block, t_args, f_block, f_args) => {
            let cond_reg = regs.get(*cond);

            emit_block_args(func, regs, bytecode, *t_block, t_args);
            pending_jumps.push(PendingJump {
                inst_index: bytecode.len(),
                target_block: *t_block,
            });
            bytecode.push(jmp_t(cond_reg, 0));

            emit_block_args(func, regs, bytecode, *f_block, f_args);
            pending_jumps.push(PendingJump {
                inst_index: bytecode.len(),
                target_block: *f_block,
            });
            bytecode.push(jmp(0));
        }

        Term::Ret(vals) => {
            let moves: Vec<(Reg, Reg)> = vals
                .iter()
                .enumerate()
                .map(|(i, v)| (regs.get(*v), i as Reg))
                .filter(|(s, d)| s != d)
                .collect();

            resolve_parallel_moves(&moves, &mut regs.next_reg, bytecode);
            bytecode.push(ret());
        }
    }

    Liveness::for_each_term_operand(term, |op| {
        if liveness.is_last_use(op, block_idx, term_idx) {
            regs.free(op);
        }
    });
}

fn emit_block_args(
    func: &FuncDef,
    regs: &mut RegAlloc,
    bytecode: &mut Vec<Instruction>,
    target: Block,
    args: &[Val],
) {
    let target_block = &func.blocks[target.0 as usize];

    let moves: Vec<(Reg, Reg)> = target_block
        .params
        .iter()
        .zip(args.iter())
        .map(|(param, arg)| (regs.get(*arg), regs.get(*param)))
        .filter(|(src, dst)| src != dst)
        .collect();

    resolve_parallel_moves(&moves, &mut regs.next_reg, bytecode);
}

fn resolve_parallel_moves(
    moves: &[(Reg, Reg)],
    next_reg: &mut Reg,
    bytecode: &mut Vec<Instruction>,
) {
    if moves.is_empty() {
        return;
    }

    let mut pending: Vec<(Reg, Reg)> = moves.to_vec();
    let mut emitted = vec![false; pending.len()];
    let total = pending.len();
    let mut done = 0;

    while done < total {
        let mut progress = false;

        for i in 0..total {
            if emitted[i] {
                continue;
            }

            let (_, dst) = pending[i];

            let blocked = (0..total).any(|j| j != i && !emitted[j] && pending[j].0 == dst);

            if !blocked {
                let (src, dst) = pending[i];
                bytecode.push(mov(dst, src));
                emitted[i] = true;
                done += 1;
                progress = true;
            }
        }

        if !progress {
            let i = emitted.iter().position(|&e| !e).unwrap();
            let (src, _) = pending[i];

            let tmp = *next_reg;
            *next_reg += 1;

            bytecode.push(mov(tmp, src));

            for j in 0..total {
                if !emitted[j] && pending[j].0 == src {
                    pending[j].0 = tmp;
                }
            }
        }
    }
}

fn lower_binop(op: BinOp, dst: Reg, a: Reg, b: Reg) -> Instruction {
    match op {
        BinOp::IAdd => iadd(dst, a, b),
        BinOp::ISub => isub(dst, a, b),
        BinOp::IMul => imul(dst, a, b),
        BinOp::IDiv => idiv(dst, a, b),
        BinOp::IRem => irem(dst, a, b),
        BinOp::UAdd => uadd(dst, a, b),
        BinOp::USub => usub(dst, a, b),
        BinOp::UMul => umul(dst, a, b),
        BinOp::UDiv => udiv(dst, a, b),
        BinOp::URem => urem(dst, a, b),
        BinOp::FAdd => fadd(dst, a, b),
        BinOp::FSub => fsub(dst, a, b),
        BinOp::FMul => fmul(dst, a, b),
        BinOp::FDiv => fdiv(dst, a, b),
        BinOp::FRem => frem(dst, a, b),
        BinOp::IEq => ieq(dst, a, b),
        BinOp::INe => ine(dst, a, b),
        BinOp::ILt => ilt(dst, a, b),
        BinOp::IGt => igt(dst, a, b),
        BinOp::ILe => ile(dst, a, b),
        BinOp::IGe => ige(dst, a, b),
        BinOp::UEq => ueq(dst, a, b),
        BinOp::UNe => une(dst, a, b),
        BinOp::ULt => ult(dst, a, b),
        BinOp::UGt => ugt(dst, a, b),
        BinOp::ULe => ule(dst, a, b),
        BinOp::UGe => uge(dst, a, b),
        BinOp::FEq => feq(dst, a, b),
        BinOp::FNe => fne(dst, a, b),
        BinOp::FLt => flt(dst, a, b),
        BinOp::FGt => fgt(dst, a, b),
        BinOp::FLe => fle(dst, a, b),
        BinOp::FGe => fge(dst, a, b),
    }
}

fn lower_unop(op: UnOp, dst: Reg, src: Reg) -> Instruction {
    match op {
        UnOp::INeg => ineg(dst, src),
        UnOp::FNeg => fneg(dst, src),
        UnOp::BNot => bnot(dst, src),
        UnOp::INot => inot(dst, src),
        UnOp::UNot => unot(dst, src),
    }
}

fn lower_conv(op: ConvOp, _: Reg, _: Reg) -> Instruction {
    todo!("conversion opcodes not yet in VM: {:?}", op)
}

fn patch_jump_target(inst: Instruction, target: u32) -> Instruction {
    match inst.op() {
        Opcode::JMP => jmp(target as InstType),
        Opcode::JMP_T => jmp_t(inst.a(), target as InstType),
        Opcode::JMP_F => jmp_f(inst.a(), target as InstType),
        _ => unreachable!("not a jump instruction"),
    }
}
