use ahash::AHashMap;

use crate::{
    mir::{BlockId, Const, Func, Inst, Module, Term, Type, TypeId},
    vm::{
        instruction::*,
        object::{AsValue, Value},
        program::{CallInfo, FunctionInfo, Program},
    },
};

pub struct Codegen<'a> {
    module: &'a Module,

    // Output
    bytecode: Vec<Instruction>,
    constants: Vec<Value>,
    functions: Vec<FunctionInfo>,

    // Per-function state
    block_offsets: AHashMap<BlockId, usize>,
    pending_jumps: Vec<PendingJump>,
}

struct PendingJump {
    pc: usize,       // where the jump instruction is
    target: BlockId, // where it should go
    kind: JumpKind,
}

enum JumpKind {
    Unconditional,    // JMP - uses ax format
    ConditionalTrue,  // JMP_T - uses abx format
    ConditionalFalse, // JMP_F - uses abx format
}

impl<'a> Codegen<'a> {
    pub fn new(module: &'a Module) -> Self {
        Self {
            module,
            bytecode: Vec::new(),
            constants: Vec::new(),
            functions: Vec::new(),
            block_offsets: AHashMap::new(),
            pending_jumps: Vec::new(),
        }
    }

    pub fn compile(mut self) -> Program {
        for func in &self.module.funcs {
            self.compile_func(func);
        }

        Program {
            bytecode: self.bytecode,
            constants: self.constants,
            functions: self.functions,
            native_functions: Vec::new(), // TODO: handle natives
        }
    }

    fn compile_func(&mut self, func: &Func) {
        let entry_pc = self.pc();

        // Reset per-function state
        self.block_offsets.clear();
        self.pending_jumps.clear();

        // Compile all blocks
        for block_id in 0..func.blocks.len() {
            self.compile_block(func, BlockId(block_id))
        }

        self.patch_jumps();

        let narg = func.params.iter().map(|t| self.type_size(*t)).sum::<u64>();
        let nret = self.type_size(func.ret);

        // Compile function body
        // self.compile_block(func.blocks[0]);

        let func_info = FunctionInfo {
            name: func.name.clone(),
            call_info: CallInfo {
                entry_pc,
                nreg: func.nregs,
                narg: narg as u8,
                nret: nret as u8,
                ncap: 0,
            },
        };

        // Add function info to list
        self.functions.push(func_info);
    }

    fn compile_block(&mut self, func: &Func, block_id: BlockId) {
        // Record block entry pc
        self.block_offsets.insert(block_id, self.pc());

        let block = &func.blocks[block_id.0];

        for &inst in &block.insts {
            self.compile_inst(inst);
        }

        self.compile_term(&block.term)
    }

    fn compile_term(&mut self, term: &Term) {
        match term {
            Term::Return(reg) => {
                self.emit(ret(*reg));
            }

            Term::ReturnVoid => {
                self.emit(ret(0));
            }

            Term::Jump(target) => {
                let pc = self.emit(jmp(0));
                self.pending_jumps.push(PendingJump {
                    pc,
                    target: *target,
                    kind: JumpKind::Unconditional,
                });
            }

            Term::Branch {
                cond,
                then_blk,
                else_blk,
            } => {
                // Jump to then_blk if true
                let pc = self.emit(jmp_t(*cond, 0));
                self.pending_jumps.push(PendingJump {
                    pc,
                    target: *then_blk,
                    kind: JumpKind::ConditionalTrue,
                });

                // Fall through to else_blk
                let pc = self.emit(jmp(0));
                self.pending_jumps.push(PendingJump {
                    pc,
                    target: *else_blk,
                    kind: JumpKind::Unconditional,
                });
            }

            Term::Switch {
                cond,
                cases,
                default,
            } => {
                // Series of comparisons and branches
                for (value, target) in cases {
                    // tmp = cond == value
                    // TODO: need a scratch register for this
                    // For now, assume simple implementation

                    // This is inefficient - real impl would use jump table
                    // Leaving as TODO for now
                }

                // Default jump
                let pc = self.emit(jmp(0));
                self.pending_jumps.push(PendingJump {
                    pc,
                    target: *default,
                    kind: JumpKind::Unconditional,
                });
            }

            Term::Unreachable => {
                self.emit(halt());
            }
        }
    }

    fn compile_inst(&mut self, inst: Inst) -> usize {
        match inst {
            Inst::LoadConst { dst, val } => {
                let val = self.add_const(val);
                self.emit(konst(dst, val))
            }

            Inst::Copy { dst, src } => self.emit(mov(dst, src)),

            Inst::Not { dst } => self.emit(not(dst)),
            Inst::INeg { dst } => self.emit(ineg(dst)),
            Inst::FNeg { dst } => self.emit(fneg(dst)),

            Inst::IAdd { dst, a, b } => self.emit(iadd(dst, a, b)),
            Inst::ISub { dst, a, b } => self.emit(isub(dst, a, b)),
            Inst::IMul { dst, a, b } => self.emit(imul(dst, a, b)),
            Inst::IDiv { dst, a, b } => self.emit(idiv(dst, a, b)),
            Inst::IRem { dst, a, b } => self.emit(irem(dst, a, b)),

            Inst::UAdd { dst, a, b } => self.emit(uadd(dst, a, b)),
            Inst::USub { dst, a, b } => self.emit(usub(dst, a, b)),
            Inst::UMul { dst, a, b } => self.emit(umul(dst, a, b)),
            Inst::UDiv { dst, a, b } => self.emit(udiv(dst, a, b)),
            Inst::URem { dst, a, b } => self.emit(urem(dst, a, b)),

            Inst::FAdd { dst, a, b } => self.emit(fadd(dst, a, b)),
            Inst::FSub { dst, a, b } => self.emit(fsub(dst, a, b)),
            Inst::FMul { dst, a, b } => self.emit(fmul(dst, a, b)),
            Inst::FDiv { dst, a, b } => self.emit(fdiv(dst, a, b)),
            Inst::FRem { dst, a, b } => self.emit(frem(dst, a, b)),

            Inst::IEq { dst, a, b } => self.emit(ieq(dst, a, b)),
            Inst::INe { dst, a, b } => self.emit(ine(dst, a, b)),
            Inst::ILt { dst, a, b } => self.emit(ilt(dst, a, b)),
            Inst::ILe { dst, a, b } => self.emit(ile(dst, a, b)),
            Inst::IGt { dst, a, b } => self.emit(igt(dst, a, b)),
            Inst::IGe { dst, a, b } => self.emit(ige(dst, a, b)),

            Inst::UEq { dst, a, b } => self.emit(ueq(dst, a, b)),
            Inst::UNe { dst, a, b } => self.emit(une(dst, a, b)),
            Inst::ULt { dst, a, b } => self.emit(ult(dst, a, b)),
            Inst::ULe { dst, a, b } => self.emit(ule(dst, a, b)),
            Inst::UGt { dst, a, b } => self.emit(ugt(dst, a, b)),
            Inst::UGe { dst, a, b } => self.emit(uge(dst, a, b)),

            Inst::FLt { dst, a, b } => todo!(),
            Inst::FLe { dst, a, b } => todo!(),
            Inst::FGt { dst, a, b } => todo!(),
            Inst::FGe { dst, a, b } => todo!(),

            Inst::BitAnd { dst, a, b } => todo!(),
            Inst::BitOr { dst, a, b } => todo!(),
            Inst::BitXor { dst, a, b } => todo!(),
            Inst::BitNot { dst, src } => todo!(),
            Inst::Shl { dst, a, b } => todo!(),
            Inst::Shr { dst, a, b } => todo!(),

            Inst::And { dst, a, b } => todo!(),
            Inst::Or { dst, a, b } => todo!(),
            Inst::BuildAggregate { dst, src, len } => todo!(),
            Inst::GetField { dst, base, field } => todo!(),
            Inst::SetField { base, field, src } => todo!(),
            Inst::GetIndex { dst, base, index } => todo!(),
            Inst::SetIndex { base, index, src } => todo!(),
            Inst::BuildUnion { dst, variant, src } => todo!(),
            Inst::GetTag { dst, base } => todo!(),
            Inst::GetVariant { dst, base, variant } => todo!(),
            Inst::SetVariant { base, variant, src } => todo!(),
            Inst::Alloc { dst, size } => todo!(),
            Inst::Load { dst, ptr } => todo!(),
            Inst::Store { ptr, src } => todo!(),
            Inst::Call { dst, func } => todo!(),
            Inst::CallIndirect { dst, ptr } => todo!(),
            Inst::CallNative { dst, func } => todo!(),
            Inst::CallNativeIndirect { dst, func } => todo!(),
        }
    }

    fn patch_jumps(&mut self) {
        for jump in &self.pending_jumps {
            let tgt = self.block_offsets[&jump.target];

            self.bytecode[jump.pc] = match jump.kind {
                JumpKind::Unconditional => jmp(tgt as u32),
                JumpKind::ConditionalTrue => jmp_t(self.bytecode[jump.pc].a(), tgt as u16),
                JumpKind::ConditionalFalse => jmp_f(self.bytecode[jump.pc].a(), tgt as u16),
            };
        }
    }

    /// Size of a type in registers
    fn type_size(&self, ty: TypeId) -> u64 {
        match self.module.get_type(ty) {
            Type::Void | Type::Never => 0,
            Type::Bool | Type::Int | Type::Uint | Type::Float | Type::Char => 1,
            Type::Ptr { .. } | Type::Enum { .. } | Type::Func { .. } => 1,
            Type::Array { elem, len } => self.type_size(*elem) * len,
            Type::Tuple(fields) => fields.iter().map(|t| self.type_size(*t)).sum(),
            Type::Struct(fields) => fields.iter().map(|t| self.type_size(*t)).sum(),
            Type::Union(fields) => fields
                .iter()
                .map(|t| self.type_size(*t))
                .max()
                .map_or(0, |max| max + 1),
        }
    }

    /// Offset of a field within a struct/tuple
    fn field_offset(&self, ty: TypeId, field: u64) -> u64 {
        match self.module.get_type(ty) {
            Type::Struct(fields) | Type::Tuple(fields) => fields[..field as usize]
                .iter()
                .map(|t| self.type_size(*t))
                .sum(),
            _ => panic!("field_offset on non-aggregate"),
        }
    }

    /// Emit an instruction, return its pc
    fn emit(&mut self, inst: Instruction) -> usize {
        let pc = self.bytecode.len();
        self.bytecode.push(inst);
        pc
    }

    /// Current bytecode position
    fn pc(&self) -> usize {
        self.bytecode.len()
    }

    /// Add constant to pool, return index
    fn add_const(&mut self, val: Const) -> u16 {
        // TODO: dedup constants

        let val = match val {
            Const::Bool(b) => b.into_value(),
            Const::Int(i) => i.into_value(),
            Const::Uint(u) => u.into_value(),
            Const::Float(f) => f.into_value(),
            Const::Char(c) => c.into_value(),
        };

        let idx = self.constants.len();
        self.constants.push(val);
        idx as u16
    }
}
