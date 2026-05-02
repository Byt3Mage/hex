use std::collections::HashSet;

use hex_vm::instruction::{self as hx, InstType, Instruction, Opcode, Reg};

use crate::{
    BinOp, CastOp, Function, Inst, Module, Terminator, UnOp, Val,
    constants::ConstantPool,
    liveness::{Liveness, compute_liveness},
};

pub struct RegAssignment {
    map: Vec<Reg>,
    pub nreg: Reg,
}

impl RegAssignment {
    pub fn get(&self, v: Val) -> Reg {
        self.map[v.0 as usize]
    }
}

pub fn assign_registers(func: &Function, liveness: &Liveness) -> RegAssignment {
    let num_vals = func.nreg as usize;

    let mut block_starts = Vec::with_capacity(func.blocks.len());
    let mut point = 0usize;
    for blk in &func.blocks {
        block_starts.push(point);
        point += blk.insts.len() + 1;
    }
    let total_points = point;

    let mut intervals: Vec<Vec<bool>> = vec![vec![false; total_points]; num_vals];

    for (b, blk) in func.blocks.iter().enumerate() {
        let base = block_starts[b];
        let mut live: HashSet<Val> = liveness.live_out[b].clone();

        let term_point = base + blk.insts.len();
        blk.term.for_each_use(|v| {
            live.insert(*v);
        });
        for &v in &live {
            intervals[v.0 as usize][term_point] = true;
        }

        for (i, inst) in blk.insts.iter().enumerate().rev() {
            inst.for_each_def(|v| {
                live.remove(&v);
            });
            inst.for_each_use(|v| {
                live.insert(*v);
            });
            let p = base + i;
            for &v in &live {
                intervals[v.0 as usize][p] = true;
            }
        }
    }

    let mut map = vec![0 as Reg; num_vals];
    let mut nreg: Reg = 0;

    for v in 0..num_vals {
        if !intervals[v].iter().any(|&b| b) {
            continue;
        }

        let mut reg = 0;
        'search: loop {
            for other in 0..v {
                if map[other] == reg {
                    let overlaps = intervals[v]
                        .iter()
                        .zip(intervals[other].iter())
                        .any(|(&a, &b)| a && b);
                    if overlaps {
                        reg += 1;
                        continue 'search;
                    }
                }
            }
            break;
        }

        map[v] = reg;
        nreg = nreg.max(reg + 1);
    }

    RegAssignment { map, nreg }
}

struct JumpPatch {
    bc_idx: usize,
    target_block: usize,
}

pub fn emit_module(module: &Module) -> hex_vm::Module {
    let mut bc = Vec::new();
    let mut pool = ConstantPool::new();
    let mut functions = Vec::with_capacity(module.functions.len());

    for func in &module.functions {
        let entry_pc = bc.len();
        let liveness = compute_liveness(func);
        let regs = assign_registers(func, &liveness);
        let mut next_reg = regs.nreg;
        let mut patches = Vec::new();
        let mut blk_offsets = Vec::new();

        for blk in func.blocks.iter() {
            blk_offsets.push(bc.len());

            for inst in &blk.insts {
                emit_inst(inst, &regs, &mut bc, &mut pool, &mut next_reg);
            }

            emit_term(&blk.term, func, &regs, &mut bc, &mut patches, &mut next_reg);
        }

        patch_jumps(&patches, &mut bc, &blk_offsets);

        functions.push(hex_vm::Function {
            name: func.name.clone(),
            entry_pc,
            nreg: next_reg,
            narg: func.narg,
            nret: func.nret,
        });
    }

    hex_vm::Module {
        name: module.name.clone(),
        bytecode: bc.into(),
        constants: pool.into_values().into(),
        functions: functions.into(),
        native_functions: vec![].into(),
        exports: vec![].into(),
        imports: vec![].into(),
    }
}

fn emit_inst(
    inst: &Inst,
    regs: &RegAssignment,
    bc: &mut Vec<Instruction>,
    pool: &mut ConstantPool,
    next_reg: &mut Reg,
) {
    match inst {
        Inst::LoadInt { dst, value } => {
            bc.push(hx::const_(regs.get(*dst), pool.insert(*value as u64)));
        }
        Inst::LoadUint { dst, value } => {
            bc.push(hx::const_(regs.get(*dst), pool.insert(*value)));
        }
        Inst::LoadFloat { dst, value } => {
            bc.push(hx::const_(regs.get(*dst), pool.insert(value.to_bits())));
        }
        Inst::LoadBool { dst, value } => {
            bc.push(hx::const_(regs.get(*dst), pool.insert(*value as u64)));
        }
        Inst::Mov { dst, src } => {
            let d = regs.get(*dst);
            let s = regs.get(*src);
            if d != s {
                bc.push(hx::mov(d, s));
            }
        }
        Inst::BinOp { dst, op, lhs, rhs } => {
            bc.push(emit_binop(
                *op,
                regs.get(*dst),
                regs.get(*lhs),
                regs.get(*rhs),
            ));
        }
        Inst::UnOp { dst, op, src } => {
            bc.push(emit_unop(*op, regs.get(*dst), regs.get(*src)));
        }
        Inst::Cast { dst, op, src } => {
            emit_cast(*op, regs.get(*dst), regs.get(*src), bc);
        }
        Inst::Call { dst, func, args } => {
            let d = regs.get(*dst);
            let arg_moves: Vec<(Reg, Reg)> = args
                .iter()
                .enumerate()
                .map(|(i, a)| (regs.get(*a), d + i as Reg))
                .collect();
            resolve_parallel_moves(&arg_moves, next_reg, bc);
            bc.push(hx::call(d, *func as InstType));
        }
        Inst::CallNative { dst, func, args } => {
            let d = regs.get(*dst);
            let arg_moves: Vec<(Reg, Reg)> = args
                .iter()
                .enumerate()
                .map(|(i, a)| (regs.get(*a), d + i as Reg))
                .collect();
            resolve_parallel_moves(&arg_moves, next_reg, bc);
            bc.push(hx::calln(d, *func as InstType));
        }
        Inst::CallIndirect { dst, func, args } => {
            let f = regs.get(*func);
            let d = regs.get(*dst);
            let arg_moves: Vec<(Reg, Reg)> = args
                .iter()
                .enumerate()
                .map(|(i, a)| (regs.get(*a), d + i as Reg))
                .collect();
            resolve_parallel_moves(&arg_moves, next_reg, bc);
            bc.push(hx::callr(d, f));
        }
        Inst::CallNativeIndirect { dst, func, args } => {
            let f = regs.get(*func);
            let d = regs.get(*dst);
            let arg_moves: Vec<(Reg, Reg)> = args
                .iter()
                .enumerate()
                .map(|(i, a)| (regs.get(*a), d + i as Reg))
                .collect();
            resolve_parallel_moves(&arg_moves, next_reg, bc);
            bc.push(hx::callnr(d, f));
        }
    }
}

fn emit_term(
    term: &Terminator,
    func: &Function,
    regs: &RegAssignment,
    bc: &mut Vec<Instruction>,
    patches: &mut Vec<JumpPatch>,
    next_reg: &mut Reg,
) {
    match term {
        Terminator::Br { tgt, args } => {
            let target_params = &func.blocks[*tgt].params;
            let moves: Vec<(Reg, Reg)> = args
                .iter()
                .zip(target_params.iter())
                .map(|(a, p)| (regs.get(*a), regs.get(*p)))
                .collect();
            resolve_parallel_moves(&moves, next_reg, bc);
            patches.push(JumpPatch {
                bc_idx: bc.len(),
                target_block: *tgt,
            });
            bc.push(hx::jmp(0));
        }
        Terminator::BrIf {
            cond,
            then_br,
            then_args,
            else_br,
            else_args,
        } => {
            let c = regs.get(*cond);

            let then_params = &func.blocks[*then_br].params;
            let then_moves: Vec<(Reg, Reg)> = then_args
                .iter()
                .zip(then_params.iter())
                .map(|(a, p)| (regs.get(*a), regs.get(*p)))
                .collect();

            let else_params = &func.blocks[*else_br].params;
            let else_moves: Vec<(Reg, Reg)> = else_args
                .iter()
                .zip(else_params.iter())
                .map(|(a, p)| (regs.get(*a), regs.get(*p)))
                .collect();

            let has_then_moves = then_moves.iter().any(|(s, d)| s != d);
            let has_else_moves = else_moves.iter().any(|(s, d)| s != d);

            if !has_then_moves && !has_else_moves {
                // No block args to move, simple case
                patches.push(JumpPatch {
                    bc_idx: bc.len(),
                    target_block: *then_br,
                });
                bc.push(hx::jmp_t(c, 0));
                patches.push(JumpPatch {
                    bc_idx: bc.len(),
                    target_block: *else_br,
                });
                bc.push(hx::jmp(0));
            } else {
                // Emit separate move sequences per path
                let jmp_to_else = bc.len();
                bc.push(hx::jmp_f(c, 0)); // placeholder

                resolve_parallel_moves(&then_moves, next_reg, bc);
                patches.push(JumpPatch {
                    bc_idx: bc.len(),
                    target_block: *then_br,
                });
                bc.push(hx::jmp(0));

                let else_start = bc.len() as InstType;
                bc[jmp_to_else] = hx::jmp_f(c, else_start);
                resolve_parallel_moves(&else_moves, next_reg, bc);
                patches.push(JumpPatch {
                    bc_idx: bc.len(),
                    target_block: *else_br,
                });
                bc.push(hx::jmp(0));
            }
        }
        Terminator::Ret(vals) => {
            let ret_moves: Vec<(Reg, Reg)> = vals
                .iter()
                .enumerate()
                .map(|(i, v)| (regs.get(*v), i as Reg))
                .collect();
            resolve_parallel_moves(&ret_moves, next_reg, bc);
            bc.push(hx::ret());
        }
    }
}

fn emit_binop(op: BinOp, dst: Reg, lhs: Reg, rhs: Reg) -> Instruction {
    match op {
        BinOp::IAdd => hx::iadd(dst, lhs, rhs),
        BinOp::ISub => hx::isub(dst, lhs, rhs),
        BinOp::IMul => hx::imul(dst, lhs, rhs),
        BinOp::IDiv => hx::idiv(dst, lhs, rhs),
        BinOp::IRem => hx::irem(dst, lhs, rhs),
        BinOp::UAdd => hx::uadd(dst, lhs, rhs),
        BinOp::USub => hx::usub(dst, lhs, rhs),
        BinOp::UMul => hx::umul(dst, lhs, rhs),
        BinOp::UDiv => hx::udiv(dst, lhs, rhs),
        BinOp::URem => hx::urem(dst, lhs, rhs),
        BinOp::FAdd => hx::fadd(dst, lhs, rhs),
        BinOp::FSub => hx::fsub(dst, lhs, rhs),
        BinOp::FMul => hx::fmul(dst, lhs, rhs),
        BinOp::FDiv => hx::fdiv(dst, lhs, rhs),
        BinOp::FRem => hx::frem(dst, lhs, rhs),
        BinOp::IEq => hx::ieq(dst, lhs, rhs),
        BinOp::INe => hx::ine(dst, lhs, rhs),
        BinOp::ILt => hx::ilt(dst, lhs, rhs),
        BinOp::IGt => hx::igt(dst, lhs, rhs),
        BinOp::ILe => hx::ile(dst, lhs, rhs),
        BinOp::IGe => hx::ige(dst, lhs, rhs),
        BinOp::UEq => hx::ueq(dst, lhs, rhs),
        BinOp::UNe => hx::une(dst, lhs, rhs),
        BinOp::ULt => hx::ult(dst, lhs, rhs),
        BinOp::UGt => hx::ugt(dst, lhs, rhs),
        BinOp::ULe => hx::ule(dst, lhs, rhs),
        BinOp::UGe => hx::uge(dst, lhs, rhs),
        BinOp::FEq => hx::feq(dst, lhs, rhs),
        BinOp::FNe => hx::fne(dst, lhs, rhs),
        BinOp::FLt => hx::flt(dst, lhs, rhs),
        BinOp::FGt => hx::fgt(dst, lhs, rhs),
        BinOp::FLe => hx::fle(dst, lhs, rhs),
        BinOp::FGe => hx::fge(dst, lhs, rhs),
    }
}

fn emit_unop(op: UnOp, dst: Reg, src: Reg) -> Instruction {
    match op {
        UnOp::INeg => hx::ineg(dst, src),
        UnOp::FNeg => hx::fneg(dst, src),
        UnOp::BNot => hx::bnot(dst, src),
        UnOp::INot => hx::inot(dst, src),
        UnOp::UNot => hx::unot(dst, src),
    }
}

fn emit_cast(op: CastOp, _dst: Reg, _src: Reg, _bc: &mut Vec<Instruction>) {
    todo!("cast opcodes not yet in hex-vm: {:?}", op)
}

fn resolve_parallel_moves(moves: &[(Reg, Reg)], next_reg: &mut Reg, bc: &mut Vec<Instruction>) {
    if moves.is_empty() {
        return;
    }

    let mut pending: Vec<(Reg, Reg)> = moves.iter().copied().filter(|(s, d)| s != d).collect();
    if pending.is_empty() {
        return;
    }

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
                bc.push(hx::mov(dst, src));
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

            bc.push(hx::mov(tmp, src));

            for j in 0..total {
                if !emitted[j] && pending[j].0 == src {
                    pending[j].0 = tmp;
                }
            }
        }
    }
}

fn patch_jumps(patches: &[JumpPatch], bc: &mut Vec<Instruction>, blk_offsets: &[usize]) {
    for patch in patches {
        let tgt = blk_offsets[patch.target_block] as InstType;
        let inst = bc[patch.bc_idx];
        bc[patch.bc_idx] = match inst.op() {
            Opcode::JMP => hx::jmp(tgt),
            Opcode::JMP_T => hx::jmp_t(inst.a(), tgt),
            Opcode::JMP_F => hx::jmp_f(inst.a(), tgt),
            _ => unreachable!("patching non-jump instruction"),
        };
    }
}
