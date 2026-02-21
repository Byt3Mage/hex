use ahash::AHashMap;
use thiserror::Error;

use crate::{
    compiler::{
        error::bug,
        liveness::{InstIdx, LivenessInfo},
        mir::*,
        op::{BinOp, UnOp},
    },
    vm::{self, instruction::*, object::AsValue, program::FunctionPtr},
};

/// Maximum register index.
const MAX_REG: Reg = Reg::MAX;

#[derive(Debug, Error)]
enum CodegenError {
    #[error("Register allocation exceeds max register size {MAX_REG}")]
    RegisterOverflow,
}

#[derive(Clone, Copy, Debug)]
struct Allocation {
    reg: Reg,
    size: Reg,
}

/// Maps IR Values to physical register assignments.
struct RegMap {
    // Size is 1 for scalars, N for aggregates.
    assignments: AHashMap<Value, Allocation>,
}

/// Tracks which register slots are free for reuse.
struct RegAllocator {
    /// Next register to allocate from (bump pointer).
    next_reg: Reg,
    /// Freed register ranges available for reuse: (start, size).
    free_list: Vec<(Reg, Reg)>,
    /// Peak register usage (for frame metadata).
    max_reg: Reg,
}

impl RegAllocator {
    fn alloc(&mut self, size: usize) -> Result<Allocation, CodegenError> {
        // Try to reuse from free list.
        for i in 0..self.free_list.len() {
            let (start, free_size) = self.free_list[i];

            if (free_size as usize) == size {
                let size = size as Reg;
                self.free_list.swap_remove(i);
                return Ok(Allocation { reg: start, size });
            } else if (free_size as usize) > size {
                let size = size as Reg;
                self.free_list[i] = (Reg::new(start.raw() + size), free_size - size);
                return Ok(Allocation { reg: start, size });
            }
        }

        // Check overflow before bump allocating.
        let remaining = MAX_REG - self.next_reg;

        if size > remaining as usize {
            return Err(CodegenError::RegisterOverflow);
        }

        // safe to cast wihout truncating since we checked for overflow
        let size = size as Reg;
        let reg = Reg::new(self.next_reg);

        self.next_reg += size;
        self.max_reg = self.max_reg.max(self.next_reg);

        Ok(Allocation { reg, size })
    }

    fn free(&mut self, reg: Reg, size: Reg) {
        self.free_list.push((reg, size));
    }
}

struct Codegen<'a> {
    types: &'a TypeTable,
    reg_alloc: RegAllocator,
    reg_map: AHashMap<Value, Allocation>,
    liveness: LivenessInfo,

    /// Bytecode output.
    code: Vec<Instruction>,
    /// Constants table output.
    constants: Vec<vm::object::Value>,
    /// Maps BlockId -> bytecode offset (for resolving jumps).
    block_offsets: AHashMap<BlockId, usize>,
    /// Jump instructions that need patching after all blocks are emitted.
    jump_patches: Vec<(usize, BlockId)>,
}

impl<'a> Codegen<'a> {
    fn linearize_blocks(&mut self, func: &Function) -> Vec<BlockId> {
        todo!()
    }

    fn lower_function(&mut self, func: &Function) -> Result<(), CodegenError> {
        // Step 1: Linearize blocks to get a defined order
        let block_order = self.linearize_blocks(func);

        // Step 3: Analyze value liveness
        self.liveness = LivenessInfo::analyze(&func.blocks, &block_order);

        // Step 3: Allocate registers for function params.
        for (&param, &ty) in func.params.iter().zip(&func.param_tys) {
            let size = self.type_size(ty);
            self.reg_alloc(param, size)?;
        }

        // Step 4: Emit instructions. Values are allocated/freed based on livenesss.
        let mut idx: InstIdx = 0;
        for &block_id in &block_order {
            let block = &func.blocks[block_id];

            // Record block start offset.
            self.block_offsets.insert(block_id, self.code.len());

            // Block params are already allocated by the jump that targets this block.
            for inst in &block.insts {
                self.free_dead_values(idx);
                self.emit_inst(inst, idx)?;
                idx += 1;
            }

            self.free_dead_values(idx);
            //self.emit_terminator(&block.terminator, idx)?;
            idx += 1;
        }

        // Step 5: Patch jumps.
        //self.patch_jumps();

        Ok(())
    }

    fn free_dead_values(&mut self, idx: InstIdx) {
        todo!()
    }

    fn emit_inst(&mut self, inst: &Inst, idx: InstIdx) -> Result<(), CodegenError> {
        match inst {
            Inst::Copy { dst, src, ty } => {
                let r_dst = self.reg_of(*dst);
                let r_src = self.reg_of(*src);

                // By the time we hit the copy instruction,
                // both dst and src have valid register ranges
                // so we can safely cast without checking for overflow.
                for i in 0..self.type_size(*ty) {
                    let d = Reg::new(r_dst.raw() + i as Reg);
                    let s = Reg::new(r_src.raw() + i as Reg);
                    self.code.push(mov(d, s));
                }

                // TODO: use bulk copy instruction for larger values.
            }

            Inst::Const { dst, val: value } => {
                let reg = self.reg_alloc(*dst, 1)?;
                let const_idx = match value {
                    Literal::Int(i) => self.add_constant(*i),
                    Literal::UInt(u) => self.add_constant(*u),
                    Literal::Float(f) => self.add_constant(*f),
                    Literal::Bool(b) => self.add_constant(*b),
                    Literal::Str(s) => todo!("add string literal to constants table"),
                };

                self.code.push(konst(reg, const_idx));
            }

            Inst::BinOp {
                dst,
                lhs,
                rhs,
                op,
                ty,
            } => {
                // We only allow scalar types for raw binary operations,
                // so r_dst should have a register allocation of 1.
                let r_lhs = self.reg_of(*lhs);
                let r_rhs = self.reg_of(*rhs);
                let r_dst = self.reg_alloc(*dst, 1)?;
                let op = lower_binop(*op, *ty);
                self.code.push(encode_abc(op, r_dst, r_lhs, r_rhs));
            }

            Inst::UnOp { dst, op, ty, src } => {
                // We only allow scalar types for raw unary operations,
                // so r_dst should have a register allocation of 1.
                let r_src = self.reg_of(*src);
                let r_dst = self.reg_alloc(*dst, 1)?;
                let op = lower_unop(*op, *ty);
                self.code.push(encode_abc(op, r_dst, r_src, R0));
            }

            Inst::Cast { dst, src, .. } => todo!("emit cast instruction"),

            Inst::RegAlloc { dst: dest, ty } => {
                // No bytecode emitted. Just reserve register slots.
                let size = self.type_size(*ty);
                self.reg_alloc(*dest, size)?;
            }

            Inst::FieldAddr {
                dst,
                base,
                field,
                base_ty,
            } => {
                // Compile-time offset calculation.
                let r_base = self.reg_of(*base);
                let offset = self.field_offset(*base_ty, *field);
                let r_dest = Reg::new(r_base.raw() + offset as Reg);

                // Don't allocate, just record the mapping.
                // We don't check offset overflow because fields
                // are obtained from already allocated aggregates,
                // which must fit within register ranges.
                self.reg_map.insert(
                    *dst,
                    Allocation {
                        reg: r_dest,
                        size: 1, // FieldAddr result points to a single field's start
                    },
                );
            }

            Inst::SetTag { dst, field } => {
                // Tag is stored in the first register of the union.
                let r_dst = self.reg_of(*dst);
                let tag = self.add_constant(*field);
                self.code.push(konst(r_dst, tag));
            }

            Inst::GetTag { dst, src } => {
                // Tag is stored the first register of the union.
                let r_src = self.reg_of(*src);
                let r_dst = self.reg_alloc(*dst, 1)?;
                self.code.push(mov(r_dst, r_src));
            }

            Inst::UnionFieldAddr { dst, base } => {
                // Field payload starts at base + 1 (after the tag).
                let r_base = self.reg_of(*base);
                let r_dst = Reg::new(r_base.raw() + 1);

                // Don't allocate, just record the mapping.
                self.reg_map.insert(
                    *dst,
                    Allocation {
                        reg: r_dst,
                        size: 1, // UnionFieldAddr result points to a single field's start
                    },
                );
            }

            Inst::Call { dst, func, args } => {
                todo!("emit call")
            }

            Inst::CallIndirect {
                dst,
                func_ptr,
                args,
            } => {
                todo!("indirect call")
            }

            Inst::CallVoid { func, args } => {
                todo!("emit call")
            }

            Inst::CallIndirectVoid { func_ptr, args } => {
                todo!("indirect call")
            }

            Inst::FuncAddr { dst, func } => {
                let r_dest = self.reg_alloc(*dst, 1)?;
                let func_ptr = self.add_constant(func.0 as FunctionPtr);
                self.code.push(konst(r_dest, func_ptr));
            }
        }
        Ok(())
    }

    fn add_constant<T: AsValue>(&mut self, val: T) -> InstType {
        let idx = self.constants.len();
        self.constants.push(val.into_value());
        idx as InstType
    }

    /// Allocate registers for a new Value and record the mapping.
    fn reg_alloc(&mut self, val: Value, size: usize) -> Result<Reg, CodegenError> {
        let alloc = self.reg_alloc.alloc(size)?;
        self.reg_map.insert(val, alloc);
        Ok(alloc.reg)
    }

    /// Allocate a temporary register (not tied to an IR Value).
    fn reg_alloc_temp(&mut self, size: usize) -> Result<Reg, CodegenError> {
        self.reg_alloc.alloc(size).map(|a| a.reg)
    }

    /// Look up the register assigned to a Value.
    fn reg_of(&self, val: Value) -> Reg {
        self.reg_map[&val].reg
    }

    /// Look up the register size of a type.
    fn type_size(&self, ty: TypeId) -> usize {
        match &self.types.types[ty.0 as usize] {
            TypeInfo::Scalar(_) | TypeInfo::FuncPtr(..) => 1,
            TypeInfo::Struct(s) => s.size,
            TypeInfo::Array(a) => a.size,
            TypeInfo::Union(u) => u.size,
        }
    }

    /// Get the register offset of a field within a struct type.
    fn field_offset(&self, ty: TypeId, field: usize) -> usize {
        match &self.types.types[ty.0 as usize] {
            TypeInfo::Struct(s) => s.fields[field].offset,
            _ => bug!("field_offset on non-struct type"),
        }
    }
}

fn lower_unop(op: UnOp, ty: ScalarType) -> Opcode {
    match (op, ty) {
        (UnOp::Neg, ScalarType::Int) => Opcode::INEG,
        (UnOp::Neg, ScalarType::Float) => Opcode::FNEG,

        (UnOp::Not, ScalarType::Bool) => Opcode::BNOT,
        (UnOp::Not, ScalarType::Int) => Opcode::INOT,
        (UnOp::Not, ScalarType::UInt) => Opcode::UNOT,

        (UnOp::Deref, _) => todo!("add pointer dereference"),

        _ => bug!("unsupported unop {:?} for type {:?}", op, ty),
    }
}

fn lower_binop(op: BinOp, ty: ScalarType) -> Opcode {
    match (op, ty) {
        (BinOp::Add, ScalarType::Int) => Opcode::IADD,
        (BinOp::Sub, ScalarType::Int) => Opcode::ISUB,
        (BinOp::Mul, ScalarType::Int) => Opcode::IMUL,
        (BinOp::Div, ScalarType::Int) => Opcode::IDIV,
        (BinOp::Mod, ScalarType::Int) => Opcode::IREM,

        (BinOp::Add, ScalarType::UInt) => Opcode::UADD,
        (BinOp::Sub, ScalarType::UInt) => Opcode::USUB,
        (BinOp::Mul, ScalarType::UInt) => Opcode::UMUL,
        (BinOp::Div, ScalarType::UInt) => Opcode::UDIV,
        (BinOp::Mod, ScalarType::UInt) => Opcode::UREM,

        (BinOp::Add, ScalarType::Float) => Opcode::FADD,
        (BinOp::Sub, ScalarType::Float) => Opcode::FSUB,
        (BinOp::Mul, ScalarType::Float) => Opcode::FMUL,
        (BinOp::Div, ScalarType::Float) => Opcode::FDIV,
        (BinOp::Mod, ScalarType::Float) => Opcode::FREM,

        (BinOp::Eq, ScalarType::Int) => Opcode::IEQ,
        (BinOp::Ne, ScalarType::Int) => Opcode::INE,
        (BinOp::Lt, ScalarType::Int) => Opcode::ILT,
        (BinOp::Gt, ScalarType::Int) => Opcode::IGT,
        (BinOp::Le, ScalarType::Int) => Opcode::ILE,
        (BinOp::Ge, ScalarType::Int) => Opcode::IGE,

        (BinOp::Eq, ScalarType::UInt) => Opcode::UEQ,
        (BinOp::Ne, ScalarType::UInt) => Opcode::UNE,
        (BinOp::Lt, ScalarType::UInt) => Opcode::ULT,
        (BinOp::Gt, ScalarType::UInt) => Opcode::UGT,
        (BinOp::Le, ScalarType::UInt) => Opcode::ULE,
        (BinOp::Ge, ScalarType::UInt) => Opcode::UGE,

        (BinOp::Eq, ScalarType::Float) => Opcode::FEQ,
        (BinOp::Ne, ScalarType::Float) => Opcode::FNE,
        (BinOp::Lt, ScalarType::Float) => Opcode::FLT,
        (BinOp::Gt, ScalarType::Float) => Opcode::FGT,
        (BinOp::Le, ScalarType::Float) => Opcode::FLE,
        (BinOp::Ge, ScalarType::Float) => Opcode::FGE,

        // Bitwise and logical ops use int opcodes
        (BinOp::BitAnd | BinOp::And, _) => todo!("proper bitwise ops"), // TODO: proper bitwise opcodes
        _ => bug!("unsupported binop {:?} for type {:?}", op, ty),
    }
}
