mod disassemble;
mod error;
mod host;
mod instruction;
mod program;
mod value;

pub use disassemble::disassemble;
pub use error::*;
pub use host::{Flow, Host, HostCtx, Syscode};
pub use instruction::*;
pub use program::*;
pub use value::{Args, IsValue, Value};

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum RunOutcome {
    /// HALT or top-level RET. Returns are in registers.
    Completed,
    /// A syscall suspended execution. Resumable: call run() again.
    Suspended,
    /// The program faulted. The language decides what to do next.
    Trapped(Fault),
}

#[allow(non_camel_case_types)]
pub type word = u64;

#[derive(Debug, Default, Clone, Copy)]
pub struct Frame {
    pub ret_pc: usize,
    pub ret_base: usize,
    pub caller_id: FunctionId,
}

#[derive(Debug, Default, Clone)]
pub struct VM {
    pub registers: Vec<Value>,
    call_stack: Vec<Frame>,
    pc: usize,
    base: usize,
    curr_func: FunctionId,
}

impl VM {
    pub fn from_entry(program: &Program, func: FunctionId, args: Args<'_>) -> Result<Self, Error> {
        let mut vm = Self::default();
        vm.set_entry(program, func, args)?;
        Ok(vm)
    }

    pub fn set_entry(&mut self, program: &Program, entry: FunctionId, args: Args) -> Result<(), Error> {
        let func = program.function(entry);
        let argc = args.count();

        if argc != func.narg {
            return Err(Error::ArgcMismatch { exp: func.narg, got: argc });
        }

        self.reset();
        self.registers.resize(func.nreg as usize, Value::ZERO);
        self.registers[..args.len()].copy_from_slice(&args);
        self.pc = func.ty.entry_pc()?;
        self.base = 0;
        self.curr_func = entry;
        Ok(())
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.registers.clear();
        self.call_stack.clear();
        self.pc = 0;
        self.base = 0;
        self.curr_func = FunctionId::MAX;
    }

    #[inline]
    pub fn pc(&self) -> usize {
        self.pc
    }

    #[inline]
    pub fn base(&self) -> usize {
        self.base
    }

    #[inline]
    pub fn call_stack(&self) -> &[Frame] {
        &self.call_stack
    }

    #[inline(always)]
    fn reg_raw(&self, reg: Reg) -> Value {
        // TODO: add program validation and change to get_unchecked.
        self.registers[self.base + reg as usize]
    }

    #[inline(always)]
    fn reg<T: IsValue>(&self, reg: Reg) -> T {
        T::from_value(self.registers[self.base + reg as usize])
    }

    #[inline(always)]
    fn set_reg(&mut self, reg: Reg, value: impl IsValue) {
        self.registers[self.base + reg as usize] = value.into_value();
    }

    #[inline(always)]
    fn set_reg_raw(&mut self, reg: Reg, value: Value) {
        self.registers[self.base + reg as usize] = value;
    }

    #[inline(always)]
    fn two_reg<T: IsValue>(&self, reg_a: Reg, reg_b: Reg) -> (T, T) {
        (self.reg(reg_a), self.reg(reg_b))
    }

    #[inline(always)]
    fn reg_offset(&self, reg: Reg) -> usize {
        self.base + reg as usize
    }

    #[inline(always)]
    fn callvm(&mut self, callee: FunctionId, ret: Reg, entry: usize, nreg: Reg) {
        // Call convention: args are in caller registers[Rret..Rn]
        //
        // We use overlapping register windows for caller and callee
        // to avoid copying arguments. This is safe because argument
        // registers are not clobbered by caller until callee returns.

        // Callee window starts at caller's return register.
        let base = self.reg_offset(ret);
        let last = base + nreg as usize;

        // Grow regs to fit callee's full register count beyond the arg base.
        if last > self.registers.len() {
            self.registers.resize(last, Value::ZERO);
        }

        // Create new call frame and save return point
        self.call_stack.push(Frame {
            ret_pc: self.pc,
            ret_base: self.base,
            caller_id: self.curr_func,
        });

        // Jump to callee code
        self.curr_func = callee;
        self.base = base;
        self.pc = entry;
    }

    #[inline(always)]
    fn callh<H: Host>(
        &mut self,
        ret: Reg,
        host: &mut H,
        syscode: Syscode,
        narg: Reg,
        nret: Reg,
    ) -> Result<Flow, Error> {
        host.syscall(syscode, HostCtx { base: self.reg_offset(ret), narg, nret, vm: self })
    }
}

#[inline(always)]
fn cmp_branch(vm: &mut VM, program: &Program, jmp: bool) {
    vm.pc = if jmp { program.instruction(vm.pc) as usize } else { vm.pc + 1 };
}

#[inline(always)]
pub fn run<H: Host>(vm: &mut VM, program: &Program, host: &mut H, memory: &mut [u8]) -> Result<RunOutcome, Error> {
    // Raise a fault: unwind to a handler, or exit run if uncaught.
    macro_rules! fault {
        ($f:expr) => {{
            let f = $f;
            let v = fault_to_value(f);
            match unwind(vm, program, v, f) {
                Ok(()) => continue,                 // caught: resume at handler
                Err(outcome) => return Ok(outcome), // uncaught: terminal
            }
        }};
    }

    while vm.pc < program.len() {
        let i = program.instruction(vm.pc);
        vm.pc += 1;

        match inst::op(i) {
            // Moves
            Opcode::COPY => vm.set_reg_raw(inst::a(i), vm.reg_raw(inst::b(i))),
            Opcode::LOADK => vm.set_reg_raw(inst::a(i), program.constant(inst::bx(i) as usize)),
            Opcode::LOADI => vm.set_reg(inst::a(i), inst::bx_imm(i)),
            Opcode::LOADF => vm.set_reg(inst::a(i), inst::bx_imm(i) as f64),

            // Unary operations
            Opcode::NOT => vm.set_reg(inst::a(i), !vm.reg::<u64>(inst::b(i))),
            Opcode::BNOT => vm.set_reg(inst::a(i), !vm.reg::<bool>(inst::b(i))),
            Opcode::INEG => vm.set_reg(inst::a(i), -vm.reg::<i64>(inst::b(i))),
            Opcode::FNEG => vm.set_reg(inst::a(i), -vm.reg::<f64>(inst::b(i))),

            // Signed/unsigned integer arithmetic
            Opcode::ADD => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a.wrapping_add(b));
            }
            Opcode::SUB => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a.wrapping_sub(b));
            }
            Opcode::MUL => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a.wrapping_mul(b));
            }
            Opcode::ADDI => {
                let src = vm.reg::<u64>(inst::b(i));
                vm.set_reg(inst::a(i), src.wrapping_add(inst::imm8(i) as u64));
            }
            Opcode::SUBI => {
                let src = vm.reg::<u64>(inst::b(i));
                vm.set_reg(inst::a(i), src.wrapping_sub(inst::imm8(i) as u64));
            }
            Opcode::MULI => {
                let src = vm.reg::<u64>(inst::b(i));
                vm.set_reg(inst::a(i), src.wrapping_mul(inst::imm8(i) as u64));
            }
            Opcode::ADDK => {
                let a = vm.reg::<u64>(inst::b(i));
                let k = program.constant(inst::c(i) as usize).get::<u64>();
                vm.set_reg(inst::a(i), a.wrapping_add(k));
            }
            Opcode::SUBK => {
                let a = vm.reg::<u64>(inst::b(i));
                let k = program.constant(inst::c(i) as usize).get::<u64>();
                vm.set_reg(inst::a(i), a.wrapping_sub(k));
            }
            Opcode::MULK => {
                let a = vm.reg::<u64>(inst::b(i));
                let k = program.constant(inst::c(i) as usize).get::<u64>();
                vm.set_reg(inst::a(i), a.wrapping_mul(k));
            }

            // Float arithmetic with constant-pool operand
            Opcode::FADDK => {
                let a = vm.reg::<f64>(inst::b(i));
                let k = program.constant(inst::c(i) as usize).get::<f64>();
                vm.set_reg(inst::a(i), a + k);
            }
            Opcode::FSUBK => {
                let a = vm.reg::<f64>(inst::b(i));
                let k = program.constant(inst::c(i) as usize).get::<f64>();
                vm.set_reg(inst::a(i), a - k);
            }
            Opcode::FMULK => {
                let a = vm.reg::<f64>(inst::b(i));
                let k = program.constant(inst::c(i) as usize).get::<f64>();
                vm.set_reg(inst::a(i), a * k);
            }
            Opcode::FDIVK => {
                let a = vm.reg::<f64>(inst::b(i));
                let k = program.constant(inst::c(i) as usize).get::<f64>();
                vm.set_reg(inst::a(i), a / k);
            }

            // Signed/unsigned integer division
            Opcode::SDIV => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                match a.checked_div(b) {
                    Some(v) => vm.set_reg(inst::a(i), v),
                    None => fault!(Fault::DivisionByZero),
                }
            }
            Opcode::SREM => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                match a.checked_rem(b) {
                    Some(v) => vm.set_reg(inst::a(i), v),
                    None => fault!(Fault::DivisionByZero),
                }
            }
            Opcode::UDIV => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                match a.checked_div(b) {
                    Some(v) => vm.set_reg(inst::a(i), v),
                    None => fault!(Fault::DivisionByZero),
                }
            }
            Opcode::UREM => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                match a.checked_rem(b) {
                    Some(v) => vm.set_reg(inst::a(i), v),
                    None => fault!(Fault::DivisionByZero),
                }
            }

            // Floating point arithmetic
            Opcode::FADD => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a + b);
            }
            Opcode::FSUB => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a - b);
            }
            Opcode::FMUL => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a * b);
            }
            Opcode::FDIV => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a / b);
            }
            Opcode::FREM => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a % b);
            }

            Opcode::EQ => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a == b);
            }
            Opcode::NE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a != b);
            }

            Opcode::SGT => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a > b);
            }
            Opcode::SLT => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a < b);
            }
            Opcode::SGE => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a >= b);
            }
            Opcode::SLE => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a <= b);
            }

            Opcode::UGT => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a > b);
            }
            Opcode::ULT => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a < b);
            }
            Opcode::UGE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a >= b);
            }
            Opcode::ULE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a <= b);
            }

            Opcode::FEQ => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a == b);
            }
            Opcode::FNE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a != b);
            }
            Opcode::FGT => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a > b);
            }
            Opcode::FLT => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a < b);
            }
            Opcode::FGE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a >= b);
            }
            Opcode::FLE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a <= b);
            }

            // Jumps
            Opcode::JMP => {
                vm.pc = inst::ax(i) as usize;
            }
            Opcode::JMP_T => {
                if vm.reg::<bool>(inst::a(i)) {
                    vm.pc = inst::bx(i) as usize;
                }
            }
            Opcode::JMP_F => {
                if !vm.reg::<bool>(inst::a(i)) {
                    vm.pc = inst::bx(i) as usize;
                }
            }
            Opcode::JEQ => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a == b);
            }
            Opcode::JNE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a != b);
            }
            Opcode::JSLT => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a < b);
            }
            Opcode::JSGT => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a > b);
            }
            Opcode::JSLE => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a <= b);
            }
            Opcode::JSGE => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a >= b);
            }
            Opcode::JULT => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a < b);
            }
            Opcode::JUGT => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a > b);
            }
            Opcode::JULE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a <= b);
            }
            Opcode::JUGE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a >= b);
            }
            Opcode::JFEQ => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a == b);
            }
            Opcode::JFNE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a != b);
            }
            Opcode::JFLT => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a < b);
            }
            Opcode::JFGT => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a > b);
            }
            Opcode::JFLE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a <= b);
            }
            Opcode::JFGE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch(vm, program, a >= b);
            }

            // Memory ops
            Opcode::LOAD => {
                let (ptr, off) = vm.two_reg::<word>(inst::b(i), inst::c(i));
                let start = (ptr + off) as usize;
                match memory.get(start..start + size_of::<Value>()) {
                    Some(slice) => vm.set_reg_raw(inst::a(i), Value::copy_from_slice(slice)),
                    None => fault!(Fault::MemoryOOB),
                }
            }
            Opcode::STORE => {
                let (ptr, off) = vm.two_reg::<word>(inst::b(i), inst::c(i));
                let start = (ptr + off) as usize;
                match memory.get_mut(start..start + size_of::<Value>()) {
                    Some(slice) => slice.copy_from_slice(&Value::to_le_bytes(vm.reg_raw(inst::a(i)))),
                    None => fault!(Fault::MemoryOOB),
                }
            }

            // Calls
            Opcode::CALL => {
                let id = inst::bx(i) as FunctionId;
                let func = program.function(id);
                match func.ty {
                    FnType::Hxvm { entry_pc } => vm.callvm(id, inst::a(i), entry_pc, func.nreg),
                    FnType::Host { syscode } => match vm.callh(inst::a(i), host, syscode, func.narg, func.nret)? {
                        Flow::Suspend => return Ok(RunOutcome::Suspended),
                        Flow::Continue => {}
                    },
                }
            }
            Opcode::CALL_IND => {
                let id = vm.reg::<FunctionId>(inst::b(i));
                let func = program.function(id);
                match func.ty {
                    FnType::Hxvm { entry_pc } => vm.callvm(id, inst::a(i), entry_pc, func.nreg),
                    FnType::Host { syscode } => match vm.callh(inst::a(i), host, syscode, func.narg, func.nret)? {
                        Flow::Suspend => return Ok(RunOutcome::Suspended),
                        Flow::Continue => {}
                    },
                }
            }
            Opcode::TCALL => {
                let id = inst::bx(i) as FunctionId;

                let func = program.function(id);
                match func.ty {
                    FnType::Hxvm { entry_pc } => {
                        let src = vm.reg_offset(inst::a(i));

                        // Move args down to the CURRENT base (reusing this frame's slot).
                        // src >= self.base always (a_reg >= 0), so copy_within is forward-safe
                        // only if src >= dst; here dst = self.base <= src, so left-shift: OK.
                        vm.registers.copy_within(src..src + func.narg as usize, vm.base);

                        // grow window to callee's nreg from the SAME base (no frame push)
                        let last = vm.base + func.nreg as usize;
                        if last > vm.registers.len() {
                            vm.registers.resize(last, Value::ZERO);
                        }

                        vm.pc = entry_pc;
                    }
                    FnType::Host { syscode } => {
                        let base = vm.base;
                        let ctx = HostCtx { vm, base, narg: func.narg, nret: func.nret };
                        match host.syscall(syscode, ctx)? {
                            Flow::Suspend => return Ok(RunOutcome::Suspended),
                            Flow::Continue => match vm.call_stack.pop() {
                                Some(fr) => {
                                    vm.pc = fr.ret_pc;
                                    vm.base = fr.ret_base;
                                }
                                None => return Ok(RunOutcome::Completed),
                            },
                        }
                    }
                }
            }
            Opcode::TCALL_IND => {
                let id = vm.reg::<FunctionId>(inst::b(i));
                let func = program.function(id);
                match func.ty {
                    FnType::Hxvm { entry_pc } => {
                        let src = vm.reg_offset(inst::a(i));

                        // Move args down to the CURRENT base (reusing this frame's slot).
                        // src >= self.base always (a_reg >= 0), so copy_within is forward-safe
                        // only if src >= dst; here dst = self.base <= src, so left-shift: OK.
                        vm.registers.copy_within(src..src + func.narg as usize, vm.base);

                        // grow window to callee's nreg from the SAME base (no frame push)
                        let last = vm.base + func.nreg as usize;
                        if last > vm.registers.len() {
                            vm.registers.resize(last, Value::ZERO);
                        }

                        vm.pc = entry_pc;
                    }
                    FnType::Host { syscode } => {
                        let base = vm.base;
                        let ctx = HostCtx { vm, base, narg: func.narg, nret: func.nret };
                        match host.syscall(syscode, ctx)? {
                            Flow::Suspend => return Ok(RunOutcome::Suspended),
                            Flow::Continue => match vm.call_stack.pop() {
                                Some(frame) => {
                                    vm.pc = frame.ret_pc;
                                    vm.base = frame.ret_base;
                                }
                                None => return Ok(RunOutcome::Completed),
                            },
                        }
                    }
                }
            }
            Opcode::RET => match vm.call_stack.pop() {
                Some(frame) => {
                    vm.pc = frame.ret_pc;
                    vm.base = frame.ret_base;
                }
                None => return Ok(RunOutcome::Completed),
            },

            Opcode::THROW => {
                let thrown = vm.reg_raw(inst::a(i));
                match unwind(vm, program, thrown, Fault::Uncaught) {
                    Ok(()) => {}
                    Err(outcome) => return Ok(outcome),
                }
            }

            Opcode::HALT => return Ok(RunOutcome::Completed),

            op => return Err(Error::UnknownOp(op)),
        }
    }
    Err(Error::PcOOB)
}

/// Unwind from the current pc carrying `thrown`. On finding a handler, sets
/// pc/base/cur_func to resume at the handler and returns Ok(()). If no handler
/// exists anywhere on the stack, returns Err(outcome) to exit `run`.
fn unwind(vm: &mut VM, program: &Program, thrown: Value, uncaught: Fault) -> Result<(), RunOutcome> {
    let mut func = program.function(vm.curr_func);
    let mut check_pc = vm.pc - 1;

    loop {
        if let Some(h) = func.handler_for(check_pc) {
            vm.set_reg_raw(h.catch_reg, thrown);
            vm.pc = h.handler_pc;
            return Ok(()); // Resume dispatch at handler; base unchanged (this frame)
        }

        match vm.call_stack.pop() {
            Some(frame) => {
                vm.base = frame.ret_base;
                vm.pc = frame.ret_pc;
                vm.curr_func = frame.caller_id;
                func = program.function(vm.curr_func);
                check_pc = frame.ret_pc - 1;
            }
            None => {
                // stack empty, no handler anywhere
                return Err(RunOutcome::Trapped(uncaught));
            }
        }
    }
}

#[inline]
fn fault_to_value(f: Fault) -> Value {
    match f {
        Fault::Uncaught => 0,
        Fault::DivisionByZero => 1,
        Fault::MemoryOOB => 2,
        Fault::Abort(c) => 0x100 | c as i64,
    }
    .into_value()
}
