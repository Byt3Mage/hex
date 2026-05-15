use std::collections::HashMap;

use hex_vm::{self as vm, FunctionId, Program};

use crate::{
    BlockId, Function, Inst, Module, Term, Val,
    constants::ConstantPool,
    liveness::Liveness,
    op::{BinOp, UnOp},
    register_alloc::{EdgeMoves, Move, RegAlloc, RegAllocError, resolve_parallel_moves},
};

/// Shared state for compiling a whole module. Each [compile_function] call
/// appends to shared bytecode, constants, and function table.
struct Context {
    pub bytecode: Vec<vm::Instruction>,
    pub functions: Vec<vm::Function>,
    pub constants: ConstantPool,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum CodegenError {
    /// A function needs more registers than VmReg can encode.
    #[error("too many registers: {0}")]
    TooManyRegisters(usize),
    /// Function has more parameters than VmReg can encode.
    #[error("too many params: {0}")]
    TooManyParams(usize),
    /// Function has more return values than VmReg can encode.
    #[error("too many returns: {0}")]
    TooManyReturns(usize),
    /// Bitwise op requested but VM has no vm::Opcode for it yet.
    #[error("unsupported op: {0}")]
    UnsupportedOp(&'static str),
    /// Register allocation failed
    #[error("register allocation failed: {0}")]
    RegisterAllocationFailed(#[from] RegAllocError),
}

pub struct Compilation {
    pub program: vm::Program,
    pub functions: HashMap<String, vm::FunctionId>,
}

pub fn compile_module(module: &Module) -> Result<Compilation, CodegenError> {
    let mut ctx = Context {
        bytecode: vec![],
        functions: Vec::new(),
        constants: ConstantPool::new(),
    };

    let functions: HashMap<String, FunctionId> = module
        .functions
        .iter()
        .map(|func| {
            let live = Liveness::compute(func);
            let mut alloc = RegAlloc::compute(func, &live)?;
            let edges = EdgeMoves::compute(func, &mut alloc)?;
            let ptr = compile_function(func, &mut alloc, &edges, &mut ctx)?;
            Ok((func.name.clone(), ptr))
        })
        .collect::<Result<_, CodegenError>>()?;

    Ok(Compilation {
        program: Program {
            instructions: ctx.bytecode.into(),
            constants: ctx.constants.into(),
            functions: ctx.functions.into(),
        },
        functions,
    })
}

fn compile_function(
    func: &Function,
    alloc: &mut RegAlloc,
    edges: &EdgeMoves,
    ctx: &mut Context,
) -> Result<vm::FunctionId, CodegenError> {
    // Validate counts fit in vm::Reg.
    let entry_block = func.block(func.entry);
    if entry_block.params.len() > vm::Reg::MAX as usize {
        return Err(CodegenError::TooManyParams(entry_block.params.len()));
    }
    if func.ret_tys.len() > vm::Reg::MAX as usize {
        return Err(CodegenError::TooManyReturns(func.ret_tys.len()));
    }

    let mut codegen = Codegen {
        ctx,
        func,
        alloc,
        edges,
        blk_pcs: HashMap::new(),
        pending: Vec::new(),
    };

    // Emit each block. Record its starting PC so jumps can be resolved later.
    for blk in &codegen.func.blocks {
        codegen
            .blk_pcs
            .insert(blk.id, codegen.ctx.bytecode.len() as vm::InstType);

        for inst in &blk.insts {
            codegen.emit_inst(inst);
        }

        codegen.emit_terminator(blk.id, &blk.term)?;
    }

    // Patch all pending jump targets.
    for patch in &codegen.pending {
        let target_pc = *codegen
            .blk_pcs
            .get(&patch.tgt)
            .expect("block has recoded PC");

        codegen.ctx.bytecode[patch.inst_idx] = match patch.kind {
            JumpKind::Jmp => vm::jmp(target_pc),
            JumpKind::JmpT { cond } => vm::jmp_t(cond, target_pc),
            JumpKind::JmpF { cond } => vm::jmp_f(cond, target_pc),
        };
    }

    let entry_pc = codegen.blk_pcs[&func.entry] as usize;
    let narg = entry_block.params.len() as vm::Reg;
    let nret = func.ret_tys.len() as vm::Reg;
    let nreg = alloc.nreg(nret);
    let ptr = ctx.functions.len() as FunctionId;

    ctx.functions.push(vm::Function {
        callable: vm::Callable::Vm(entry_pc),
        narg,
        nret,
        nreg,
    });

    Ok(ptr)
}

struct Codegen<'a> {
    ctx: &'a mut Context,
    func: &'a Function,
    alloc: &'a mut RegAlloc,
    edges: &'a EdgeMoves,
    /// Map from BlockId to the PC at which that block's first instruction lives.
    blk_pcs: HashMap<BlockId, vm::InstType>,
    /// Jumps whose targets we couldn't resolve yet, to be patched at the end.
    pending: Vec<JumpPatch>,
}

/// A jump that needs its target PC filled in once all blocks have been emitted.
struct JumpPatch {
    /// Index into `instructions` of the jump to patch.
    inst_idx: usize,
    /// Block whose starting PC is the jump target.
    tgt: BlockId,
    /// What kind of jump this is, so we know how to re-encode it.
    kind: JumpKind,
}

#[derive(Clone, Copy)]
enum JumpKind {
    Jmp,
    JmpT { cond: vm::Reg },
    JmpF { cond: vm::Reg },
}

impl<'a> Codegen<'a> {
    fn emit(&mut self, inst: vm::Instruction) {
        self.ctx.bytecode.push(inst);
    }

    fn emit_inst(&mut self, inst: &Inst) {
        let dst = self.alloc.reg_of(inst.def());
        match inst {
            Inst::Const { val, .. } => {
                let idx = self.ctx.constants.insert(val);
                self.emit(vm::const_(dst, idx));
            }

            Inst::Binary { op, lhs, rhs, .. } => {
                let a = self.alloc.reg_of(*lhs);
                let b = self.alloc.reg_of(*rhs);
                self.emit(vm::encode_abc(binopcode(*op), dst, a, b));
            }

            Inst::Unary { op, src, .. } => {
                let s = self.alloc.reg_of(*src);
                self.emit(vm::encode_abc(unopcode(*op), dst, s, vm::R0));
            }

            Inst::Copy { src, .. } => {
                let s = self.alloc.reg_of(*src);
                if s != dst {
                    self.emit(vm::copy(dst, s));
                }
            }
        }
    }

    #[inline(always)]
    fn emit_moves(&mut self, moves: &[Move]) {
        moves.iter().for_each(|m| self.emit(vm::copy(m.dst, m.src)));
    }

    fn emit_terminator(&mut self, block: BlockId, term: &Term) -> Result<(), RegAllocError> {
        match term {
            Term::Jump { tgt, .. } => {
                if let Some(moves) = self.edges.on_jump.get(&block) {
                    self.emit_moves(moves);
                }

                let inst_idx = self.ctx.bytecode.len();
                self.emit(vm::jmp(0));
                self.pending.push(JumpPatch {
                    inst_idx,
                    tgt: *tgt,
                    kind: JumpKind::Jmp,
                });
            }

            Term::Branch {
                cond,
                then_blk,
                else_blk,
                ..
            } => {
                let cond = self.alloc.reg_of(*cond);
                let then_movs = self.edges.on_branch_then.get(&block);
                let else_movs = self.edges.on_branch_else.get(&block);
                self.emit_branch(cond, *then_blk, *else_blk, then_movs, else_movs);
            }
            Term::Return { vals } => self.emit_return(vals)?,
            Term::Unreachable => self.emit(vm::halt()),
        }

        Ok(())
    }

    /// Emit a conditional branch with optional pre-jump moves on each edge.
    /// Five layouts, picked by which edges have moves; see comments below.
    fn emit_branch(
        &mut self,
        cond: vm::Reg,
        then_blk: BlockId,
        else_blk: BlockId,
        then_moves: Option<&Vec<Move>>,
        else_moves: Option<&Vec<Move>>,
    ) {
        match (then_moves, else_moves) {
            // No moves: plain branch.
            //   JMP_T cond, then_blk
            //   JMP   else_blk
            (None, None) => {
                self.emit_pending(JumpKind::JmpT { cond }, then_blk);
                self.emit_pending(JumpKind::Jmp, else_blk);
            }

            // Moves on then-edge only:
            //   JMP_F cond, else_blk
            //   <then moves>
            //   JMP   then_blk
            (Some(then_moves), None) => {
                self.emit_pending(JumpKind::JmpF { cond }, else_blk);
                self.emit_moves(then_moves);
                self.emit_pending(JumpKind::Jmp, then_blk);
            }

            // Moves on else-edge only:
            //   JMP_T cond, then_blk
            //   <else moves>
            //   JMP   else_blk
            (None, Some(else_moves)) => {
                self.emit_pending(JumpKind::JmpT { cond }, then_blk);
                self.emit_moves(else_moves);
                self.emit_pending(JumpKind::Jmp, else_blk);
            }

            // Moves on both edges:
            //   JMP_T cond, <then-moves PC>     (resolved inline)
            //   <else moves>
            //   JMP   else_blk
            //   <then moves>                    (then-moves PC starts here)
            //   JMP   then_blk
            (Some(then_moves), Some(else_moves)) => {
                let jmp_t_idx = self.ctx.bytecode.len();
                self.emit(vm::jmp_t(cond, 0)); // patched below

                self.emit_moves(&else_moves);
                self.emit_pending(JumpKind::Jmp, else_blk);

                let then_pc = self.ctx.bytecode.len() as vm::InstType;
                self.ctx.bytecode[jmp_t_idx] = vm::jmp_t(cond, then_pc);

                self.emit_moves(&then_moves);
                self.emit_pending(JumpKind::Jmp, then_blk);
            }
        }
    }

    /// Emit a jump-style instruction with a placeholder target and record
    /// it for later patching.
    fn emit_pending(&mut self, kind: JumpKind, tgt: BlockId) {
        let inst_idx = self.ctx.bytecode.len();
        let placeholder = match kind {
            JumpKind::Jmp => vm::jmp(0),
            JumpKind::JmpT { cond } => vm::jmp_t(cond, 0),
            JumpKind::JmpF { cond } => vm::jmp_f(cond, 0),
        };
        self.emit(placeholder);
        self.pending.push(JumpPatch {
            inst_idx,
            tgt,
            kind,
        });
    }

    /// Emit return: place each `vals[i]` into register `i`, then RET.
    fn emit_return(&mut self, vals: &[Val]) -> Result<(), RegAllocError> {
        if !vals.is_empty() {
            let moves: Vec<Move> = vals
                .iter()
                .enumerate()
                .map(|(i, &v)| Move {
                    src: self.alloc.reg_of(v),
                    dst: i as vm::Reg,
                })
                .collect();

            let moves = resolve_parallel_moves(&moves, self.alloc)?;
            self.emit_moves(&moves);
        }
        self.emit(vm::ret());
        Ok(())
    }
}

#[inline(always)]
fn binopcode(op: BinOp) -> vm::Opcode {
    match op {
        BinOp::Add => vm::Opcode::ADD,
        BinOp::Sub => vm::Opcode::SUB,
        BinOp::Mul => vm::Opcode::MUL,

        BinOp::SDiv => vm::Opcode::SDIV,
        BinOp::SRem => vm::Opcode::SREM,
        BinOp::UDiv => vm::Opcode::UDIV,
        BinOp::URem => vm::Opcode::UREM,

        BinOp::And | BinOp::Or | BinOp::Xor | BinOp::Shl | BinOp::LShr | BinOp::AShr => {
            unimplemented!("VM has no bitwise opcodes yet")
        }

        BinOp::Eq => vm::Opcode::EQ,
        BinOp::Ne => vm::Opcode::NE,

        BinOp::SLt => vm::Opcode::ILT,
        BinOp::SLe => vm::Opcode::ILE,
        BinOp::SGt => vm::Opcode::IGT,
        BinOp::SGe => vm::Opcode::IGE,

        BinOp::ULt => vm::Opcode::ULT,
        BinOp::ULe => vm::Opcode::ULE,
        BinOp::UGt => vm::Opcode::UGT,
        BinOp::UGe => vm::Opcode::UGE,

        BinOp::FAdd => vm::Opcode::FADD,
        BinOp::FSub => vm::Opcode::FSUB,
        BinOp::FMul => vm::Opcode::FMUL,
        BinOp::FDiv => vm::Opcode::FDIV,
        BinOp::FRem => vm::Opcode::FREM,

        BinOp::FEq => vm::Opcode::FEQ,
        BinOp::FNe => vm::Opcode::FNE,
        BinOp::FLt => vm::Opcode::FLT,
        BinOp::FLe => vm::Opcode::FLE,
        BinOp::FGt => vm::Opcode::FGT,
        BinOp::FGe => vm::Opcode::FGE,
    }
}

#[inline(always)]
fn unopcode(op: UnOp) -> vm::Opcode {
    match op {
        UnOp::Not => vm::Opcode::NOT,
        UnOp::BNot => vm::Opcode::BNOT,
        UnOp::INeg => vm::Opcode::INEG,
        UnOp::FNeg => vm::Opcode::FNEG,
    }
}
